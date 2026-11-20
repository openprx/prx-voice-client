//! Local HTTP server with injectable faults (std `TcpListener`, ephemeral
//! port on 127.0.0.1). Single responsibility: serve byte blobs the way a
//! model host would, or misbehave on purpose.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Behaviour {
    /// Honour ranges.
    Normal,
    /// Always answer 200 with the whole body.
    IgnoreRange,
    /// Send headers and this many body bytes, then close the connection.
    DropAfter(usize),
    /// Same length, wrong content.
    WrongBytes,
    /// Append this many extra bytes to the body.
    Extra(usize),
    /// 206 whose Content-Range starts one byte late.
    BadContentRange,
    /// Answer with this status and no body.
    Status(u16),
    /// 302 to this absolute URL.
    Redirect(String),
}

#[derive(Clone, Debug)]
struct Rule {
    behaviour: Behaviour,
    from: usize,
    times: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct RequestLog {
    pub path: String,
    /// Start of the requested range, if a Range header was sent.
    pub range_start: Option<u64>,
}

#[derive(Default)]
pub struct State {
    files: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    rules: Mutex<HashMap<String, Vec<Rule>>>,
    counts: Mutex<HashMap<String, usize>>,
    pub log: Mutex<Vec<RequestLog>>,
    active: AtomicUsize,
    pub max_active: AtomicUsize,
    /// Sleep before answering each request (ms).
    latency_ms: AtomicUsize,
    /// Sleep after each 8 KiB of body (ms).
    chunk_delay_ms: AtomicUsize,
}

pub struct FaultServer {
    pub addr: SocketAddr,
    pub state: Arc<State>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl FaultServer {
    pub fn start() -> FaultServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(State::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let st = st.clone();
                        std::thread::spawn(move || {
                            let _ = handle(stream, &st);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        FaultServer {
            addr,
            state,
            stop,
            join: Some(join),
        }
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn add_file(&self, path: &str, data: Vec<u8>) {
        self.state
            .files
            .lock()
            .unwrap()
            .insert(path.to_string(), Arc::new(data));
    }

    /// Apply `behaviour` to every request of `path`.
    pub fn always(&self, path: &str, behaviour: Behaviour) {
        self.window(path, behaviour, 0, None);
    }

    /// Apply `behaviour` to the first `times` requests of `path`.
    pub fn first(&self, path: &str, behaviour: Behaviour, times: usize) {
        self.window(path, behaviour, 0, Some(times));
    }

    /// Apply `behaviour` to requests number `from..from+times` (0-based).
    pub fn window(&self, path: &str, behaviour: Behaviour, from: usize, times: Option<usize>) {
        self.state
            .rules
            .lock()
            .unwrap()
            .entry(path.to_string())
            .or_default()
            .push(Rule {
                behaviour,
                from,
                times,
            });
    }

    pub fn clear_rules(&self, path: &str) {
        self.state.rules.lock().unwrap().remove(path);
    }

    pub fn set_latency(&self, d: Duration) {
        self.state
            .latency_ms
            .store(d.as_millis() as usize, Ordering::SeqCst);
    }

    pub fn set_chunk_delay(&self, d: Duration) {
        self.state
            .chunk_delay_ms
            .store(d.as_millis() as usize, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<RequestLog> {
        self.state.log.lock().unwrap().clone()
    }

    pub fn requests_for(&self, path: &str) -> Vec<RequestLog> {
        self.requests()
            .into_iter()
            .filter(|r| r.path == path)
            .collect()
    }

    pub fn max_active(&self) -> usize {
        self.state.max_active.load(Ordering::SeqCst)
    }
}

impl Drop for FaultServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn read_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 16 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn parse_range(head: &str) -> Option<(u64, Option<u64>)> {
    for line in head.lines() {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range:") {
            let v = v.trim().strip_prefix("bytes=")?;
            let (a, b) = v.split_once('-')?;
            return Some((a.parse().ok()?, b.parse().ok()));
        }
    }
    None
}

fn handle(mut stream: TcpStream, st: &State) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    let head = read_head(&mut stream)?;
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    let range = parse_range(&head);
    let index = {
        let mut c = st.counts.lock().unwrap();
        let n = c.entry(path.clone()).or_insert(0);
        let i = *n;
        *n += 1;
        i
    };
    st.log.lock().unwrap().push(RequestLog {
        path: path.clone(),
        range_start: range.map(|r| r.0),
    });
    let now = st.active.fetch_add(1, Ordering::SeqCst) + 1;
    st.max_active.fetch_max(now, Ordering::SeqCst);
    let result = respond(&mut stream, st, &path, range, index);
    st.active.fetch_sub(1, Ordering::SeqCst);
    let _ = stream.shutdown(Shutdown::Both);
    result
}

fn respond(
    stream: &mut TcpStream,
    st: &State,
    path: &str,
    range: Option<(u64, Option<u64>)>,
    index: usize,
) -> std::io::Result<()> {
    let latency = st.latency_ms.load(Ordering::SeqCst);
    if latency > 0 {
        std::thread::sleep(Duration::from_millis(latency as u64));
    }
    if path == "/" {
        return write_all(
            stream,
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        );
    }
    let behaviour = st
        .rules
        .lock()
        .unwrap()
        .get(path)
        .and_then(|rules| {
            rules
                .iter()
                .find(|r| index >= r.from && r.times.is_none_or(|t| index < r.from + t))
                .map(|r| r.behaviour.clone())
        })
        .unwrap_or(Behaviour::Normal);
    let data = st.files.lock().unwrap().get(path).cloned();
    let Some(data) = data else {
        return write_all(
            stream,
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    match &behaviour {
        Behaviour::Status(code) => {
            let msg =
                format!("HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return write_all(stream, msg.as_bytes());
        }
        Behaviour::Redirect(url) => {
            let msg = format!(
                "HTTP/1.1 302 Found\r\nLocation: {url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return write_all(stream, msg.as_bytes());
        }
        _ => {}
    }
    let range = if matches!(behaviour, Behaviour::IgnoreRange) {
        None
    } else {
        range
    };
    let total = data.len() as u64;
    let (status, start, end) = match range {
        Some((a, b)) if a < total => (
            "206 Partial Content",
            a,
            b.map_or(total - 1, |b| b.min(total - 1)),
        ),
        Some(_) => {
            return write_all(
                stream,
                b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
        None => ("200 OK", 0, total - 1),
    };
    let mut body = data[start as usize..=end as usize].to_vec();
    if matches!(behaviour, Behaviour::WrongBytes) {
        body.iter_mut().for_each(|b| *b ^= 0xFF);
    }
    if let Behaviour::Extra(n) = behaviour {
        body.extend(std::iter::repeat_n(b'X', n));
    }
    let shown_start = if matches!(behaviour, Behaviour::BadContentRange) {
        start + 1
    } else {
        start
    };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if status.starts_with("206") {
        head.push_str(&format!(
            "Content-Range: bytes {shown_start}-{end}/{total}\r\n"
        ));
    }
    head.push_str("\r\n");
    write_all(stream, head.as_bytes())?;
    let limit = if let Behaviour::DropAfter(n) = behaviour {
        n.min(body.len())
    } else {
        body.len()
    };
    let delay = st.chunk_delay_ms.load(Ordering::SeqCst);
    for chunk in body[..limit].chunks(8192) {
        write_all(stream, chunk)?;
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay as u64));
        }
    }
    Ok(())
}

fn write_all(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

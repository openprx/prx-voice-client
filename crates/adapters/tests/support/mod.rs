//! A scriptable HTTP/1.1 server on 127.0.0.1 for the MT adapter tests (CS-5.1).
//!
//! Each connection is served by its own thread, so several requests can be in
//! flight at once. The script of a connection is a list of steps: send bytes
//! as one HTTP chunk, wait for a gate that the test opens, hold the connection
//! until the client closes it, end the body cleanly, or drop the connection
//! without the chunked terminator. A client that goes away is reported through
//! `closed()`.

#![allow(dead_code, reason = "each test binary uses a different part")]

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// A request as the server saw it.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub index: usize,
    pub path: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A door the test opens when the server may go on.
#[derive(Clone, Default)]
pub struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    /// Block the calling server thread until the test opens the gate.
    pub fn wait_for_test(&self) {
        self.wait();
    }

    fn wait(&self) {
        let (lock, cv) = &*self.0;
        let mut open = lock.lock().unwrap();
        while !*open {
            let (guard, _) = cv.wait_timeout(open, Duration::from_millis(50)).unwrap();
            open = guard;
        }
    }
}

pub enum Step {
    /// Send these bytes as one chunk of the body.
    Send(Vec<u8>),
    /// Wait until the test opens the gate.
    Wait(Gate),
    /// Keep the connection open until the client closes it.
    HoldUntilClosed,
    /// End the body properly (the chunked terminator).
    End,
    /// Drop the connection without the terminator.
    Abort,
}

pub struct Script {
    pub status: u16,
    pub steps: Vec<Step>,
}

impl Script {
    /// A 200 response with these steps.
    pub fn ok(steps: Vec<Step>) -> Self {
        Self { status: 200, steps }
    }

    /// An error status with a short JSON body.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            steps: vec![
                Step::Send(br#"{"error":{"message":"scripted"}}"#.to_vec()),
                Step::End,
            ],
        }
    }
}

type Handler = dyn Fn(&Recorded) -> Script + Send + Sync;

pub struct FakeServer {
    pub addr: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    closed_rx: Mutex<Receiver<usize>>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    /// Start a server whose responses come from `handler`.
    pub fn start(handler: impl Fn(&Recorded) -> Script + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (closed_tx, closed_rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                for conn in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let index = counter.fetch_add(1, Ordering::SeqCst);
                    let handler = Arc::clone(&handler);
                    let requests = Arc::clone(&requests);
                    let closed_tx = closed_tx.clone();
                    thread::spawn(move || serve(conn, index, &*handler, &requests, &closed_tx));
                }
            });
        }
        Self {
            addr,
            requests,
            closed_rx: Mutex::new(closed_rx),
            stop,
        }
    }

    /// The base URL to configure.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    /// Wait until the client of some connection went away; returns the
    /// connection index, or `None` after `within`.
    pub fn closed(&self, within: Duration) -> Option<usize> {
        self.closed_rx.lock().unwrap().recv_timeout(within).ok()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect(self.addr);
    }
}

fn serve(
    mut conn: TcpStream,
    index: usize,
    handler: &Handler,
    requests: &Mutex<Vec<Recorded>>,
    closed_tx: &Sender<usize>,
) {
    let Some(recorded) = read_request(&mut conn, index) else {
        return;
    };
    requests.lock().unwrap().push(recorded.clone());
    let script = handler(&recorded);
    let head = format!(
        "HTTP/1.1 {} Scripted\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nCache-Control: no-cache\r\n\r\n",
        script.status
    );
    if conn.write_all(head.as_bytes()).is_err() {
        let _ = closed_tx.send(index);
        return;
    }
    for step in script.steps {
        match step {
            Step::Send(bytes) => {
                let mut framed = format!("{:x}\r\n", bytes.len()).into_bytes();
                framed.extend_from_slice(&bytes);
                framed.extend_from_slice(b"\r\n");
                if conn.write_all(&framed).and_then(|()| conn.flush()).is_err() {
                    let _ = closed_tx.send(index);
                    return;
                }
            }
            Step::Wait(gate) => gate.wait(),
            Step::HoldUntilClosed => {
                hold_until_closed(&mut conn);
                let _ = closed_tx.send(index);
                return;
            }
            Step::End => {
                let _ = conn.write_all(b"0\r\n\r\n");
            }
            Step::Abort => {
                let _ = conn.shutdown(Shutdown::Both);
                return;
            }
        }
    }
    // The script is finished: dropping the connection ends it.
}

/// Block until the peer closes or resets the connection (at most 20 s).
fn hold_until_closed(conn: &mut TcpStream) {
    let _ = conn.set_read_timeout(Some(Duration::from_millis(20)));
    let mut scratch = [0u8; 256];
    for _ in 0..1000 {
        match conn.read(&mut scratch) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

fn read_request(conn: &mut TcpStream, index: usize) -> Option<Recorded> {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    let head_end = loop {
        if let Some(pos) = find(&data, b"\r\n\r\n") {
            break pos;
        }
        let n = conn.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let path = request_line.split_whitespace().nth(1)?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < length {
        let n = conn.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Some(Recorded {
        index,
        path,
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// One content delta as an SSE event.
pub fn sse_token(text: &str) -> Step {
    let event = json!({"choices": [{"index": 0, "delta": {"content": text}}]});
    Step::Send(format!("data: {event}\n\n").into_bytes())
}

/// The final chunk with a finish reason.
pub fn sse_finish(reason: &str) -> Step {
    let event = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]});
    Step::Send(format!("data: {event}\n\n").into_bytes())
}

/// The usage chunk.
pub fn sse_usage(prompt: u32, completion: u32) -> Step {
    let event =
        json!({"choices": [], "usage": {"prompt_tokens": prompt, "completion_tokens": completion}});
    Step::Send(format!("data: {event}\n\n").into_bytes())
}

pub fn sse_raw(line: &str) -> Step {
    Step::Send(format!("{line}\n\n").into_bytes())
}

pub fn sse_done() -> Step {
    Step::Send(b"data: [DONE]\n\n".to_vec())
}

/// A complete, well-formed response streaming `tokens`.
pub fn normal(tokens: &[&str]) -> Script {
    let mut steps: Vec<Step> = tokens.iter().map(|t| sse_token(t)).collect();
    steps.push(sse_finish("stop"));
    steps.push(sse_usage(30, u32::try_from(tokens.len()).unwrap_or(0)));
    steps.push(sse_done());
    steps.push(Step::End);
    Script::ok(steps)
}

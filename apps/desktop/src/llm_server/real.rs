//! The real implementations of the manager's ports: `std::process`, a plain HTTP probe of the
//! loopback port, `ps` and `kill` for the sweep. Every helper process is bounded and short.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::ports::{Child, Clock, LaunchSpec, Launcher, PortState, Probe, ProcInfo, Processes};

/// Starts `llama-server` with its output appended to the log file.
pub(crate) struct RealLauncher;

struct RealChild {
    child: std::process::Child,
}

impl Launcher for RealLauncher {
    fn spawn(&self, spec: &LaunchSpec) -> io::Result<Box<dyn Child>> {
        if let Some(dir) = spec.log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.log)?;
        let err = log.try_clone()?;
        let child = Command::new(&spec.binary)
            .args(spec.args())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err))
            .spawn()?;
        Ok(Box::new(RealChild { child }))
    }
}

impl Child for RealChild {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn try_exit(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(status.to_string()),
            Err(e) => Some(format!("cannot be waited for: {e}")),
        }
    }

    fn terminate(&mut self) {
        signal(self.child.id(), "-TERM");
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

fn signal(pid: u32, which: &str) {
    let _ = Command::new("/bin/kill")
        .arg(which)
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Probes `127.0.0.1:<port>` with plain HTTP.
pub(crate) struct HttpProbe;

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// A GET to the loopback port: the status and the start of the body; `None` when nothing
/// answers. Bounded in time and size.
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = Vec::new();
    let _ = (&mut stream).take(64 * 1024).read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text.strip_prefix("HTTP/1.")?.get(2..5)?.parse().ok()?;
    let body = text
        .split_once("\r\n\r\n")
        .map_or(String::new(), |(_, b)| b.to_owned());
    Some((status, body))
}

impl Probe for HttpProbe {
    fn probe(&self, port: u16, alias: &str) -> PortState {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        if TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_err() {
            return PortState::Closed;
        }
        let healthy = self.healthy(port);
        let serves = http_get(port, "/v1/models")
            .is_some_and(|(status, body)| status == 200 && body.contains(alias));
        if healthy && serves {
            PortState::Compatible
        } else {
            PortState::Foreign
        }
    }

    fn healthy(&self, port: u16) -> bool {
        http_get(port, "/health").is_some_and(|(status, _)| status == 200)
    }
}

/// `ps` and `kill`.
pub(crate) struct RealProcesses;

/// Seconds of a `ps -o etime` value: `[[dd-]hh:]mm:ss`.
pub(crate) fn parse_etime(text: &str) -> Option<u64> {
    let text = text.trim();
    let (days, rest) = match text.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, text),
    };
    let mut seconds = 0u64;
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    for part in parts {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + seconds)
}

fn ps(pid: u32, field: &str) -> Option<String> {
    let out = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", field])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

impl Processes for RealProcesses {
    fn describe(&self, pid: u32) -> Option<ProcInfo> {
        let command = ps(pid, "command=")?;
        let age_secs = parse_etime(&ps(pid, "etime=")?)?;
        Some(ProcInfo { command, age_secs })
    }

    fn terminate(&self, pid: u32) {
        signal(pid, "-TERM");
    }

    fn kill(&self, pid: u32) {
        signal(pid, "-KILL");
    }
}

/// The system clock.
pub(crate) struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub(crate) fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etime_values_parse() {
        assert_eq!(parse_etime("00:05"), Some(5));
        assert_eq!(parse_etime("  12:34 "), Some(754));
        assert_eq!(parse_etime("01:02:03"), Some(3723));
        assert_eq!(parse_etime("2-00:00:01"), Some(172_801));
        assert_eq!(parse_etime("x"), None);
        assert_eq!(parse_etime(""), None);
    }

    #[test]
    fn a_closed_port_is_closed_and_a_foreign_listener_is_foreign() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // A listener that never answers HTTP is not a compatible server.
        assert_eq!(HttpProbe.probe(port, "alias"), PortState::Foreign);
        assert!(!HttpProbe.healthy(port));
        drop(listener);
        assert_eq!(HttpProbe.probe(port, "alias"), PortState::Closed);
    }

    #[test]
    fn a_server_that_is_healthy_and_serves_the_alias_is_compatible() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let serve = std::thread::spawn(move || {
            for _ in 0..3 {
                let Ok((mut s, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 512];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if req.starts_with("GET /health") {
                    "{\"status\":\"ok\"}"
                } else {
                    "{\"data\":[{\"id\":\"alias-x\"}]}"
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        assert_eq!(HttpProbe.probe(port, "alias-x"), PortState::Compatible);
        drop(serve);
    }
}

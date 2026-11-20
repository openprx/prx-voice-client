//! HTTP range client for one file piece. Single responsibility: issue one
//! ranged GET, validate the response and stream the body into the partial
//! file, classifying every failure (design 4.2 rules 5, 6, 8).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::redirect::Policy;
use url::Url;

use crate::control::{JobControl, Stop};
use crate::net::NetworkPolicy;
use crate::types::FailReason;

const MAX_REDIRECTS: usize = 10;
const READ_BUFFER: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) enum PieceError {
    /// Pause or cancel requested.
    Stop(Stop),
    /// Not worth retrying.
    Fatal(FailReason),
    /// Counts as one failure; progress is kept.
    Retry(String),
    /// Writing the partial file failed.
    Io(std::io::Error),
}

/// The open partial file and how many bytes of it are valid.
pub(crate) struct PartWriter {
    pub file: File,
    pub have: u64,
}

impl PartWriter {
    fn truncate_to(&mut self, len: u64) -> std::io::Result<()> {
        self.file.set_len(len)?;
        self.have = len;
        Ok(())
    }
}

/// Total size of the file and the piece length to request.
#[derive(Clone, Copy)]
pub(crate) struct FileWindow {
    pub size: u64,
    pub piece: u64,
}

pub(crate) struct HttpClient {
    client: Client,
    policy: Arc<dyn NetworkPolicy>,
}

pub(crate) struct HttpSettings {
    pub connect_timeout: Duration,
    pub piece_timeout: Duration,
    pub user_agent: String,
}

impl HttpClient {
    pub fn new(s: &HttpSettings, policy: Arc<dyn NetworkPolicy>) -> Result<HttpClient, String> {
        let redirect_policy = policy.clone();
        let client = Client::builder()
            .user_agent(s.user_agent.clone())
            .connect_timeout(s.connect_timeout)
            .timeout(s.piece_timeout)
            .redirect(Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    return attempt.error("too many redirects");
                }
                match redirect_policy.check(attempt.url()) {
                    Ok(()) => attempt.follow(),
                    Err(b) => attempt.error(b.to_string()),
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(HttpClient { client, policy })
    }

    pub fn check_policy(&self, url: &str) -> Result<(), FailReason> {
        let parsed = Url::parse(url).map_err(|e| FailReason::Network {
            message: format!("bad URL: {e}"),
        })?;
        self.policy.check(&parsed).map_err(|_| FailReason::Offline)
    }

    /// True when anything answers at the URL (a single probe request).
    pub fn probe(&self, url: &str) -> bool {
        if self.check_policy(url).is_err() {
            return false;
        }
        self.client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .is_ok()
    }

    /// Fetch bytes `[w.have, w.have + piece)` of a `size`-byte file into `w`.
    /// Returns the final URL after redirects (reused for later pieces).
    /// `reached` is set once any HTTP response has been seen for the job; a
    /// connection failure before that is classified as offline.
    pub fn fetch_piece(
        &self,
        url: &str,
        w: &mut PartWriter,
        window: FileWindow,
        ctl: &JobControl,
        on_have: &dyn Fn(u64),
        reached: &mut bool,
    ) -> Result<String, PieceError> {
        let FileWindow { size, piece } = window;
        self.check_policy(url).map_err(PieceError::Fatal)?;
        let start = w.have;
        let end = (start + piece - 1).min(size - 1);
        let sent = self
            .client
            .get(url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .header(ACCEPT_ENCODING, "identity")
            .send();
        let mut resp = match sent {
            Ok(r) => r,
            Err(e) => {
                if e.is_redirect() && !*reached {
                    return Err(PieceError::Fatal(FailReason::Offline));
                }
                if e.is_connect() && !*reached {
                    return Err(PieceError::Fatal(FailReason::Offline));
                }
                return Err(PieceError::Retry(describe(&e)));
            }
        };
        *reached = true;
        let final_url = resp.url().to_string();
        let status = resp.status();
        let partial_mode = match status {
            StatusCode::PARTIAL_CONTENT => {
                let from = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range_start);
                if from != Some(start) {
                    return Err(PieceError::Retry(format!(
                        "Content-Range starts at {from:?}, expected {start}"
                    )));
                }
                true
            }
            StatusCode::OK => {
                // The server ignored the range: restart from byte 0.
                if w.have > 0 {
                    w.truncate_to(0).map_err(PieceError::Io)?;
                    on_have(0);
                }
                false
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                return Err(PieceError::Fatal(FailReason::SourceRejected {
                    status: status.as_u16(),
                }));
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                w.truncate_to(0).map_err(PieceError::Io)?;
                on_have(0);
                return Err(PieceError::Retry("HTTP 416: range not satisfiable".into()));
            }
            other => return Err(PieceError::Retry(format!("HTTP {}", other.as_u16()))),
        };

        let piece_start = w.have;
        w.file
            .seek(SeekFrom::Start(w.have))
            .map_err(PieceError::Io)?;
        let mut buf = vec![0u8; READ_BUFFER];
        let mut overflow = false;
        loop {
            if let Some(s) = ctl.signal() {
                return Err(PieceError::Stop(s));
            }
            let n = match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Err(PieceError::Retry(format!("connection lost: {e}"))),
            };
            let room = (size - w.have) as usize;
            let take = n.min(room);
            if take > 0 {
                w.file.write_all(&buf[..take]).map_err(PieceError::Io)?;
                w.have += take as u64;
                on_have(w.have);
            }
            if take < n {
                overflow = true;
                break;
            }
        }
        if overflow {
            // Never keep bytes from a server that sends more than the pin says.
            w.truncate_to(piece_start).map_err(PieceError::Io)?;
            on_have(piece_start);
            return Err(PieceError::Fatal(FailReason::Network {
                message: "the server sent more data than the file size".into(),
            }));
        }
        let wanted = if partial_mode { end + 1 } else { size };
        if w.have < wanted {
            return Err(PieceError::Retry(format!(
                "short body: got {} of {} bytes",
                w.have, wanted
            )));
        }
        Ok(final_url)
    }
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "request timed out".into()
    } else {
        e.to_string()
    }
}

/// `bytes 100-199/1000` -> 100.
fn parse_content_range_start(v: &str) -> Option<u64> {
    let rest = v.trim().strip_prefix("bytes ")?;
    rest.split(['-', '/']).next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_start_parsing() {
        assert_eq!(parse_content_range_start("bytes 100-199/1000"), Some(100));
        assert_eq!(parse_content_range_start("bytes 0-0/1"), Some(0));
        assert_eq!(parse_content_range_start("bytes */1000"), None);
        assert_eq!(parse_content_range_start("items 1-2/3"), None);
    }
}

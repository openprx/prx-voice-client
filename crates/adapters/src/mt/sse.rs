//! Incremental parser of the `text/event-stream` body of `/v1/chat/completions`.
//!
//! Feed raw network chunks in order. Chunk boundaries may fall anywhere: inside
//! a line, inside a JSON object or inside a multi-byte UTF-8 character. The
//! parser keeps raw bytes until a full line is available, so characters are
//! never decoded half way.

use serde_json::Value;

/// One meaningful event of the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseEvent {
    /// A non-empty `choices[0].delta.content` fragment.
    Content(String),
    /// A `finish_reason` (`stop`, `length`, ...).
    Finish(String),
    /// Token usage reported by the server.
    Usage {
        /// Tokens of the prompt.
        prompt_tokens: u32,
        /// Tokens of the completion.
        completion_tokens: u32,
    },
    /// The server reported an error inside the stream (`{"error": ...}`).
    ServerError {
        /// The numeric `code` of the error object, when it has one.
        code: Option<u16>,
    },
    /// A `data:` line that is not valid JSON. The line is dropped; the count
    /// lets the caller report it.
    Malformed,
    /// The `data: [DONE]` terminator.
    Done,
}

/// Incremental SSE parser.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    /// An empty parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume a chunk and return the events completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            parse_line(&line, &mut events);
        }
        events
    }

    /// Flush a final line that was not terminated by a newline.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let line = std::mem::take(&mut self.buf);
        let mut events = Vec::new();
        parse_line(&line, &mut events);
        events
    }
}

fn parse_line(raw: &[u8], out: &mut Vec<SseEvent>) {
    let line = String::from_utf8_lossy(raw);
    let line = line.trim();
    let Some(data) = line.strip_prefix("data:") else {
        // Comments (`: keep-alive`), `event:` lines and blank separators.
        return;
    };
    let data = data.trim_start();
    if data == "[DONE]" {
        out.push(SseEvent::Done);
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        out.push(SseEvent::Malformed);
        return;
    };
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_u64)
            .and_then(|c| u16::try_from(c).ok());
        out.push(SseEvent::ServerError { code });
        return;
    }
    if let Some(text) = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        out.push(SseEvent::Content(text.to_owned()));
    }
    if let Some(reason) = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
    {
        out.push(SseEvent::Finish(reason.to_owned()));
    }
    if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
        let count = |key: &str| {
            usage
                .get(key)
                .and_then(Value::as_u64)
                .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
        };
        out.push(SseEvent::Usage {
            prompt_tokens: count("prompt_tokens"),
            completion_tokens: count("completion_tokens"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(text: &str) -> String {
        format!(
            "data: {}\n\n",
            serde_json::json!({"choices": [{"index": 0, "delta": {"content": text}}]})
        )
    }

    fn content(text: &str) -> SseEvent {
        SseEvent::Content(text.into())
    }

    #[test]
    fn parses_content_and_done() {
        let mut p = SseParser::new();
        let input = format!("{}{}data: [DONE]\n\n", delta("Hel"), delta("lo"));
        assert_eq!(
            p.push(input.as_bytes()),
            vec![content("Hel"), content("lo"), SseEvent::Done]
        );
    }

    #[test]
    fn deltas_split_across_chunks() {
        let mut p = SseParser::new();
        let (a, b) = (delta("one"), delta("two"));
        let mut out = p.push(a.as_bytes());
        out.extend(p.push(b.as_bytes()));
        assert_eq!(out, vec![content("one"), content("two")]);
    }

    #[test]
    fn line_split_in_the_middle_of_json() {
        let mut p = SseParser::new();
        let line = delta("split me");
        let (head, tail) = line.split_at(line.find("content").unwrap() + 3);
        assert!(p.push(head.as_bytes()).is_empty());
        assert_eq!(p.push(tail.as_bytes()), vec![content("split me")]);
    }

    #[test]
    fn byte_by_byte_feed_yields_the_same_events() {
        let input = format!("{}{}data: [DONE]\n\n", delta("你好"), delta("世界"));
        let mut p = SseParser::new();
        let mut out = Vec::new();
        for b in input.as_bytes() {
            out.extend(p.push(std::slice::from_ref(b)));
        }
        assert_eq!(out, vec![content("你好"), content("世界"), SseEvent::Done]);
    }

    #[test]
    fn utf8_split_inside_a_character_is_not_corrupted() {
        let line = delta("译文😀");
        let bytes = line.as_bytes();
        // Cut inside the 4-byte emoji.
        let at = line.find('😀').unwrap() + 2;
        let mut p = SseParser::new();
        assert!(p.push(&bytes[..at]).is_empty());
        assert_eq!(p.push(&bytes[at..]), vec![content("译文😀")]);
    }

    #[test]
    fn empty_and_missing_delta_are_ignored() {
        let mut p = SseParser::new();
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":null}}]}\n\n",
        );
        assert!(p.push(input.as_bytes()).is_empty());
    }

    #[test]
    fn comments_blank_lines_and_other_fields_are_ignored() {
        let mut p = SseParser::new();
        let input = format!(": keep-alive\n\nevent: message\nid: 7\n\n{}", delta("ok"));
        assert_eq!(p.push(input.as_bytes()), vec![content("ok")]);
    }

    #[test]
    fn a_data_line_that_is_not_json_is_reported_as_malformed() {
        let mut p = SseParser::new();
        let input = format!("data: not json\n\n{}", delta("ok"));
        assert_eq!(
            p.push(input.as_bytes()),
            vec![SseEvent::Malformed, content("ok")]
        );
    }

    #[test]
    fn crlf_and_no_space_after_colon() {
        let mut p = SseParser::new();
        let input =
            "data:{\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
        assert_eq!(p.push(input.as_bytes()), vec![content("x"), SseEvent::Done]);
    }

    #[test]
    fn finish_flushes_an_unterminated_line() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: [DONE]").is_empty());
        assert_eq!(p.finish(), vec![SseEvent::Done]);
        assert!(p.finish().is_empty());
    }

    #[test]
    fn finish_reason_and_usage_come_with_the_chunk_that_carries_them() {
        let mut p = SseParser::new();
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"end\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":30,\"completion_tokens\":7}}\n\n",
        );
        assert_eq!(
            p.push(input.as_bytes()),
            vec![
                content("end"),
                SseEvent::Finish("stop".into()),
                SseEvent::Usage {
                    prompt_tokens: 30,
                    completion_tokens: 7
                }
            ]
        );
    }

    #[test]
    fn an_error_object_in_the_stream_is_an_event() {
        let mut p = SseParser::new();
        let input = "data: {\"error\":{\"code\":503,\"message\":\"slot busy\"}}\n\n";
        assert_eq!(
            p.push(input.as_bytes()),
            vec![SseEvent::ServerError { code: Some(503) }]
        );
        let input = "data: {\"error\":\"boom\"}\n\n";
        assert_eq!(
            p.push(input.as_bytes()),
            vec![SseEvent::ServerError { code: None }]
        );
    }
}

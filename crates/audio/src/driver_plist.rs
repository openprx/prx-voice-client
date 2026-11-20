//! Reads the driver bundle's `Info.plist` (XML form): the version string, the build number and the transport-variant keys.
//!
//! Pure text processing. Only string values of the top-level dictionary are
//! collected; nested dictionaries and arrays (the plug-in factory tables) are
//! skipped. A **binary** property list (`bplist00`) is not decoded: it yields
//! [`PlistError::Binary`], and the caller reports the driver as unreadable
//! instead of guessing. The driver's build script writes the XML form, which is
//! what `PlistBuddy` and the installer keep unless someone converts it with
//! `plutil -convert binary1`.

use std::collections::HashMap;

/// Why an `Info.plist` could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlistError {
    /// The file is a binary property list; only the XML form is supported.
    #[error("Info.plist is a binary property list (convert with `plutil -convert xml1`)")]
    Binary,
    /// The text is not a well-formed XML property list.
    #[error("Info.plist is malformed: {0}")]
    Malformed(&'static str),
}

/// The string entries of a plist's top-level dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlistStrings {
    entries: HashMap<String, String>,
}

impl PlistStrings {
    /// The string stored under `key`, if the top-level dictionary has one.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// Parse the text of an XML property list.
///
/// # Errors
/// [`PlistError::Binary`] for `bplist` data, [`PlistError::Malformed`] when no
/// top-level `<dict>` is found or tags are unbalanced.
pub fn parse_plist(bytes: &[u8]) -> Result<PlistStrings, PlistError> {
    if bytes.starts_with(b"bplist") {
        return Err(PlistError::Binary);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PlistError::Malformed("not UTF-8 text"))?;
    let mut entries = HashMap::new();
    let mut depth = 0usize; // container nesting below <plist>
    let mut in_root = false;
    let mut pending_key: Option<String> = None;
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after
                .find("-->")
                .ok_or(PlistError::Malformed("unterminated comment"))?;
            rest = &after[end + 3..];
            continue;
        }
        let close = rest
            .find('>')
            .ok_or(PlistError::Malformed("unterminated tag"))?;
        let tag = &rest[1..close];
        rest = &rest[close + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_start_matches('/');
        let is_end = tag.starts_with('/');
        match (name, is_end, self_closing) {
            ("dict", false, false) | ("array", false, false) => {
                depth += 1;
                if name == "dict" && depth == 1 {
                    in_root = true;
                }
                if depth > 1 {
                    pending_key = None;
                }
            }
            ("dict", false, true) | ("array", false, true) => pending_key = None,
            ("dict", true, _) | ("array", true, _) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(PlistError::Malformed("unbalanced tags"))?;
                if depth == 0 {
                    in_root = false;
                }
                pending_key = None;
            }
            ("key", false, false) if in_root && depth == 1 => {
                let end = rest
                    .find("</key>")
                    .ok_or(PlistError::Malformed("unterminated key"))?;
                pending_key = Some(unescape(&rest[..end]).trim().to_owned());
                rest = &rest[end + "</key>".len()..];
            }
            ("string", false, false) if in_root && depth == 1 => {
                let end = rest
                    .find("</string>")
                    .ok_or(PlistError::Malformed("unterminated string"))?;
                if let Some(key) = pending_key.take() {
                    entries.insert(key, unescape(&rest[..end]));
                }
                rest = &rest[end + "</string>".len()..];
            }
            ("string", false, true) if in_root && depth == 1 => {
                if let Some(key) = pending_key.take() {
                    entries.insert(key, String::new());
                }
            }
            // Any other value (true, false, integer, data, ...) consumes the key.
            (_, false, _) if in_root && depth == 1 => pending_key = None,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(PlistError::Malformed("unbalanced tags"));
    }
    if entries.is_empty() && !text.contains("<dict") {
        return Err(PlistError::Malformed("no top-level dictionary"));
    }
    Ok(PlistStrings { entries })
}

fn unescape(raw: &str) -> String {
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests;

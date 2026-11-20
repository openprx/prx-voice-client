//! Model identity from the catalogue's verified checksums.
//!
//! A prepared voice depends on the exact model files. Hashing 2.4 GB at every start is avoided:
//! the model store verified each file against the checksum the catalogue pins, so those checksums
//! identify the files.

use prxvc_models::ModelEntry;
use sha2::{Digest, Sha256};

/// A hex fingerprint of the files of `entry` (purpose, name, size and pinned SHA-256, in order).
pub fn model_fingerprint(entry: &ModelEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"prxvc-model-fingerprint-v1\n");
    hasher.update(entry.id.as_str().as_bytes());
    for file in &entry.files {
        for part in [
            file.purpose.as_str(),
            file.name.as_str(),
            file.sha256.as_str(),
        ] {
            hasher.update([0u8]);
            hasher.update(part.as_bytes());
        }
        hasher.update(file.size.to_le_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_models::Catalogue;

    #[test]
    fn the_fingerprint_is_stable_and_differs_per_model() {
        let cat = Catalogue::embedded();
        let tts: Vec<_> = cat
            .models()
            .iter()
            .filter(|m| m.id.as_str().starts_with("qwen3-tts"))
            .collect();
        assert!(tts.len() >= 2);
        let a = model_fingerprint(tts[0]);
        assert_eq!(a.len(), 64);
        assert_eq!(a, model_fingerprint(tts[0]));
        assert_ne!(a, model_fingerprint(tts[1]));
    }
}

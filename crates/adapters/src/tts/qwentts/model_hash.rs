//! Identity of a model for cache invalidation: a SHA-256 over the talker and codec files.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Hash of the talker and codec model files, in hex.
///
/// The prepared voice (a speaker embedding) depends on the talker's speaker encoder, so it is only
/// valid for the talker it was prepared with; the codec is hashed as well because the pair is
/// what the runtime loads. Streams both files (about 2.4 GB for the 1.7B pair, about 1 s); callers
/// that already hold the catalogue's verified checksums can build the identity from those instead
/// and skip this.
pub fn hash_model_files(talker: &Path, codec: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    for path in [talker, codec] {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        hasher.update(len.to_le_bytes());
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}"); // writing to a String cannot fail
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_covers_both_files_and_their_boundary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (a, b, c) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        std::fs::write(&a, b"talker").expect("write");
        std::fs::write(&b, b"codec").expect("write");
        std::fs::write(&c, b"codex").expect("write");
        let h1 = hash_model_files(&a, &b).expect("hash");
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, hash_model_files(&a, &b).expect("hash"));
        assert_ne!(h1, hash_model_files(&a, &c).expect("hash"), "codec change");
        assert_ne!(h1, hash_model_files(&b, &a).expect("hash"), "order matters");
        assert!(hash_model_files(&a, &dir.path().join("missing")).is_err());
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The SHA-256 of a file, and the check against a pinned digest.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::codes::{Code, Failure};

/// A file's SHA-256, as 64 lower-case hexadecimal digits, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashed {
    /// The digest.
    pub sha256: String,
    /// The size in bytes.
    pub size: u64,
}

/// Reads a file to the end and returns its digest and size.
pub fn hash_file(path: &Path) -> Result<Hashed, Failure> {
    let mut file = File::open(path).map_err(|error| {
        Failure::new(
            Code::Unreadable,
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            Failure::new(
                Code::Unreadable,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    let mut sha256 = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(sha256, "{byte:02x}");
    }
    Ok(Hashed { sha256, size })
}

/// Checks a file against the size and SHA-256 it must have.
///
/// The size is compared first, because it is cheap and a truncated
/// download is the common case; then the digest. On a mismatch the error
/// says both what was expected and what was found.
pub fn verify(path: &Path, expected_sha256: &str, expected_size: u64) -> Result<Hashed, Failure> {
    let hashed = hash_file(path)?;
    if hashed.size != expected_size {
        return Err(Failure::new(
            Code::NotVerified,
            format!(
                "{} is {} bytes; the catalogue pins {expected_size} bytes. Nothing in it was run.",
                path.display(),
                hashed.size
            ),
        ));
    }
    if hashed.sha256 != expected_sha256 {
        return Err(Failure::new(
            Code::NotVerified,
            format!(
                "the SHA-256 of {} is {}; the catalogue pins {expected_sha256}. Nothing in it was run.",
                path.display(),
                hashed.sha256
            ),
        ));
    }
    Ok(hashed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, content: &[u8]) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "tur-apps-digest-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("file");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn the_digest_of_known_input_is_the_published_one() {
        // FIPS 180-2, appendix B.1: SHA-256("abc").
        let path = scratch("abc", b"abc");
        let hashed = hash_file(&path).unwrap();
        assert_eq!(
            hashed.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hashed.size, 3);
        let empty = scratch("empty", b"");
        assert_eq!(
            hash_file(&empty).unwrap().sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_wrong_size_or_digest_is_refused() {
        let path = scratch("verify", b"abc");
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, good, 3).is_ok());
        let short = verify(&path, good, 4).unwrap_err();
        assert_eq!(short.code, Code::NotVerified);
        assert!(short.message.contains("3 bytes"));
        let other = verify(&path, &"0".repeat(64), 3).unwrap_err();
        assert_eq!(other.code, Code::NotVerified);
        assert!(other.message.contains(good));
        assert!(other.message.contains("Nothing in it was run"));
    }
}

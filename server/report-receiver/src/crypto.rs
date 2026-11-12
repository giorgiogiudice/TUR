// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Encryption at rest: XChaCha20-Poly1305 with 256-bit keys read from files
//! outside the data directory.
//!
//! A key file is `<keys_dir>/<id>.key` and holds 64 hexadecimal digits (32
//! random bytes), made with `tur-report-receiver keygen`. It must be
//! readable by its owner only. New data is sealed with the current key;
//! every key in the directory can open what it sealed, so a key is rotated
//! by adding a new one, making it current, and removing the old one once
//! the retention period has passed.
//!
//! A sealed blob is:
//!
//! ```text
//! "TURR1" | key id length (1 byte) | key id | nonce (24 bytes) | ciphertext and tag
//! ```
//!
//! The associated data binds a blob to its purpose and its report, so a
//! summary cannot be passed off as a bundle or one report's bundle as
//! another's.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use crate::config::is_key_id;

const MAGIC: &[u8; 5] = b"TURR1";
const NONCE_BYTES: usize = 24;

/// What a sealed blob holds. Part of the associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A raw bundle.
    Bundle,
    /// A summary waiting to be filed.
    Summary,
}

impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Purpose::Bundle => "bundle",
            Purpose::Summary => "summary",
        }
    }
}

/// The keys, by identifier. Never printed.
pub struct Keyring {
    current: String,
    keys: BTreeMap<String, [u8; 32]>,
}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyring")
            .field("current", &self.current)
            .field("keys", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Fills `buffer` with random bytes from the operating system.
pub fn random_bytes(buffer: &mut [u8]) -> Result<(), String> {
    getrandom::fill(buffer).map_err(|error| format!("no random numbers: {error}"))
}

/// Lower-case hexadecimal of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// A new key, as the 64 hexadecimal digits a key file holds.
pub fn generate_key_hex() -> Result<String, String> {
    let mut key = [0u8; 32];
    random_bytes(&mut key)?;
    Ok(hex(&key))
}

/// Checks that only the owner can read `path`.
pub fn check_private(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{} can be read by others; it must have mode 0400 or 0600",
            path.display()
        ));
    }
    Ok(())
}

impl Keyring {
    /// Reads every `<id>.key` in `directory`. `current` must be among them.
    pub fn load(directory: &Path, current: &str) -> Result<Keyring, String> {
        let entries =
            fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        let mut keys = BTreeMap::new();
        for entry in entries {
            let path = entry
                .map_err(|error| format!("{}: {error}", directory.display()))?
                .path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".key") else {
                continue;
            };
            if !is_key_id(id) {
                return Err(format!("{}: not a valid key name", path.display()));
            }
            check_private(&path)?;
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let bytes = unhex(text.trim())
                .filter(|bytes| bytes.len() == 32)
                .ok_or_else(|| format!("{}: not 64 hexadecimal digits", path.display()))?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            keys.insert(id.to_owned(), key);
        }
        if !keys.contains_key(current) {
            return Err(format!(
                "{}: there is no {current}.key, the current key",
                directory.display()
            ));
        }
        Ok(Keyring {
            current: current.to_owned(),
            keys,
        })
    }

    /// A keyring with the given keys, for tests.
    pub fn from_keys(current: &str, keys: &[(&str, [u8; 32])]) -> Keyring {
        Keyring {
            current: current.to_owned(),
            keys: keys
                .iter()
                .map(|(id, key)| ((*id).to_owned(), *key))
                .collect(),
        }
    }

    /// The identifier of the current key.
    pub fn current(&self) -> &str {
        &self.current
    }

    fn associated_data(purpose: Purpose, report_id: &str) -> Vec<u8> {
        format!("tur-report-receiver-1\n{}\n{report_id}", purpose.as_str()).into_bytes()
    }

    /// Encrypts `plaintext` with the current key.
    pub fn seal(
        &self,
        purpose: Purpose,
        report_id: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let key = self
            .keys
            .get(&self.current)
            .ok_or_else(|| "the current key is missing".to_owned())?;
        let mut nonce = [0u8; NONCE_BYTES];
        random_bytes(&mut nonce)?;
        let cipher = XChaCha20Poly1305::new(&Key::from(*key));
        let aad = Self::associated_data(purpose, report_id);
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| "encryption failed".to_owned())?;
        let id = self.current.as_bytes();
        let mut blob =
            Vec::with_capacity(MAGIC.len() + 1 + id.len() + NONCE_BYTES + ciphertext.len());
        blob.extend_from_slice(MAGIC);
        blob.push(u8::try_from(id.len()).map_err(|_| "key id too long".to_owned())?);
        blob.extend_from_slice(id);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    /// Decrypts a blob made by [`Keyring::seal`] for the same purpose and
    /// report.
    pub fn open(&self, purpose: Purpose, report_id: &str, blob: &[u8]) -> Result<Vec<u8>, String> {
        let rest = blob
            .strip_prefix(MAGIC.as_slice())
            .ok_or_else(|| "not a sealed blob".to_owned())?;
        let (&length, rest) = rest
            .split_first()
            .ok_or_else(|| "the blob is cut short".to_owned())?;
        let length = usize::from(length);
        if rest.len() < length + NONCE_BYTES {
            return Err("the blob is cut short".to_owned());
        }
        let (id, rest) = rest.split_at(length);
        let (nonce, ciphertext) = rest.split_at(NONCE_BYTES);
        let id = std::str::from_utf8(id).map_err(|_| "the key id is not text".to_owned())?;
        let key = self
            .keys
            .get(id)
            .ok_or_else(|| format!("the key {id} is not in the key directory"))?;
        let nonce: [u8; NONCE_BYTES] = nonce
            .try_into()
            .map_err(|_| "the blob is cut short".to_owned())?;
        let cipher = XChaCha20Poly1305::new(&Key::from(*key));
        let aad = Self::associated_data(purpose, report_id);
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                "the blob does not decrypt: wrong key, report or purpose, or damaged".to_owned()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> Keyring {
        Keyring::from_keys("k2", &[("k1", [1u8; 32]), ("k2", [2u8; 32])])
    }

    #[test]
    fn sealed_data_opens_only_for_its_purpose_and_report() {
        let ring = ring();
        let blob = ring
            .seal(Purpose::Bundle, "R-0123456789abcdef", b"secret bundle")
            .expect("sealed");
        assert!(!blob.windows(13).any(|window| window == b"secret bundle"));
        assert_eq!(
            ring.open(Purpose::Bundle, "R-0123456789abcdef", &blob)
                .expect("opens"),
            b"secret bundle"
        );
        assert!(
            ring.open(Purpose::Summary, "R-0123456789abcdef", &blob)
                .is_err()
        );
        assert!(
            ring.open(Purpose::Bundle, "R-fedcba9876543210", &blob)
                .is_err()
        );
        let mut damaged = blob.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(
            ring.open(Purpose::Bundle, "R-0123456789abcdef", &damaged)
                .is_err()
        );
        assert!(
            ring.open(Purpose::Bundle, "R-0123456789abcdef", b"TURR1")
                .is_err()
        );
        assert!(
            ring.open(Purpose::Bundle, "R-0123456789abcdef", b"junk")
                .is_err()
        );
    }

    #[test]
    fn two_seals_of_the_same_data_differ() {
        let ring = ring();
        let a = ring.seal(Purpose::Bundle, "R-1", b"x").expect("sealed");
        let b = ring.seal(Purpose::Bundle, "R-1", b"x").expect("sealed");
        assert_ne!(a, b, "the nonce is random");
    }

    #[test]
    fn rotation_keeps_old_blobs_readable_until_their_key_is_removed() {
        let old = Keyring::from_keys("k1", &[("k1", [1u8; 32])]);
        let blob = old.seal(Purpose::Bundle, "R-1", b"old").expect("sealed");
        let rotated = ring();
        assert_eq!(rotated.current(), "k2");
        assert_eq!(
            rotated.open(Purpose::Bundle, "R-1", &blob).expect("opens"),
            b"old"
        );
        let new_only = Keyring::from_keys("k2", &[("k2", [2u8; 32])]);
        let error = new_only
            .open(Purpose::Bundle, "R-1", &blob)
            .expect_err("k1 is gone");
        assert!(error.contains("k1"));
    }

    #[test]
    fn keys_are_loaded_from_private_files_only() {
        let dir = std::env::temp_dir().join(format!("tur-receiver-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("created");
        let key = dir.join("k1.key");
        fs::write(&key, format!("{}\n", generate_key_hex().expect("a key"))).expect("written");
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(
            Keyring::load(&dir, "k1")
                .expect_err("world-readable")
                .contains("mode")
        );
        fs::set_permissions(&key, fs::Permissions::from_mode(0o400)).expect("chmod");
        let ring = Keyring::load(&dir, "k1").expect("loads");
        assert!(!format!("{ring:?}").contains(&fs::read_to_string(&key).expect("read")[..16]));
        assert!(Keyring::load(&dir, "k9").is_err());
        fs::write(dir.join("bad.key"), "zz").expect("written");
        fs::set_permissions(dir.join("bad.key"), fs::Permissions::from_mode(0o400)).expect("chmod");
        assert!(Keyring::load(&dir, "k1").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        assert_eq!(unhex("000fff"), Some(vec![0, 15, 255]));
        assert_eq!(unhex("0g"), None);
        assert_eq!(unhex("abc"), None);
        assert_eq!(generate_key_hex().expect("a key").len(), 64);
    }
}

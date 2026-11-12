// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! SHA-256 (FIPS 180-4), to record which file an application was installed
//! from.
//!
//! The digest names the installer in the application record, so that the
//! record can later be matched to a published checksum. It is written here,
//! in forty lines of arithmetic, rather than taken from a crate: this
//! component has no cryptographic dependency and needs no other primitive.
//! The implementation is checked against the standard's own test vectors.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// The round constants: the first 32 bits of the fractional parts of the
/// cube roots of the first 64 primes.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The initial hash value: the first 32 bits of the fractional parts of the
/// square roots of the first eight primes.
const INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// A SHA-256 computation in progress.
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A computation over no bytes yet.
    pub fn new() -> Self {
        Sha256 {
            state: INITIAL,
            block: [0u8; 64],
            filled: 0,
            length: 0,
        }
    }

    /// Adds `data` to the message.
    pub fn update(&mut self, data: &[u8]) {
        self.length = self
            .length
            .wrapping_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        for &byte in data {
            if let Some(slot) = self.block.get_mut(self.filled) {
                *slot = byte;
            }
            self.filled += 1;
            if self.filled == 64 {
                let block = self.block;
                self.compress(&block);
                self.filled = 0;
            }
        }
    }

    /// Finishes the computation and returns the 32-byte digest.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        // One 0x80 byte, zeros up to 56 bytes into a block, then the length.
        let mut padding = vec![0x80u8];
        while (self.filled + padding.len()) % 64 != 56 {
            padding.push(0);
        }
        padding.extend_from_slice(&bits.to_be_bytes());
        self.update(&padding);
        let mut digest = [0u8; 32];
        for (chunk, word) in digest.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut schedule = [0u32; 64];
        for (slot, chunk) in schedule.iter_mut().zip(block.chunks_exact(4)) {
            let mut word = [0u8; 4];
            word.copy_from_slice(chunk);
            *slot = u32::from_be_bytes(word);
        }
        for index in 16..64 {
            let fifteen = schedule[index - 15];
            let two = schedule[index - 2];
            let small_0 = fifteen.rotate_right(7) ^ fifteen.rotate_right(18) ^ (fifteen >> 3);
            let small_1 = two.rotate_right(17) ^ two.rotate_right(19) ^ (two >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(small_0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(small_1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (constant, word) in K.iter().zip(schedule.iter()) {
            let big_1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let first = h
                .wrapping_add(big_1)
                .wrapping_add(choose)
                .wrapping_add(*constant)
                .wrapping_add(*word);
            let big_0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let second = big_0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        let added = [a, b, c, d, e, f, g, h];
        for (state, value) in self.state.iter_mut().zip(added) {
            *state = state.wrapping_add(value);
        }
    }
}

/// Bytes as lower-case hexadecimal text.
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// The SHA-256 digest of `data`, as lower-case hexadecimal text.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex(&hasher.finish())
}

/// The SHA-256 digest of a file, as lower-case hexadecimal text, and the
/// number of bytes read.
///
/// # Errors
///
/// The I/O error when the file cannot be opened or read.
pub fn sha256_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        hasher.update(buffer.get(..read).unwrap_or_default());
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    Ok((hex(&hasher.finish()), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::TempDir;

    #[test]
    fn matches_the_standard_test_vectors() {
        // FIPS 180-4 examples and the empty message.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_million_letters_and_every_block_boundary() {
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        // Feeding the same message in pieces of every size gives the same
        // digest: the buffering across block boundaries is right.
        let message: Vec<u8> = (0..200u8).collect();
        let whole = sha256_hex(&message);
        for piece in 1..130 {
            let mut hasher = Sha256::new();
            for chunk in message.chunks(piece) {
                hasher.update(chunk);
            }
            assert_eq!(hex(&hasher.finish()), whole, "pieces of {piece}");
        }
        // Lengths around the padding boundary (55, 56, 63, 64 bytes).
        assert_eq!(
            sha256_hex(&[0u8; 55]),
            "02779466cdec163811d078815c633f21901413081449002f24aa3e80f0b88ef7"
        );
        assert_eq!(
            sha256_hex(&[0u8; 56]),
            "d4817aa5497628e7c77e6b606107042bbba3130888c5f47a375e6179be789fbb"
        );
        assert_eq!(
            sha256_hex(&[0u8; 64]),
            "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b"
        );
    }

    #[test]
    fn a_file_is_hashed_and_measured() {
        let temp = TempDir::new("sha256");
        let path = temp.path().join("file.bin");
        std::fs::write(&path, b"abc").unwrap();
        let (digest, size) = sha256_file(&path).unwrap();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(size, 3);
        assert!(sha256_file(&temp.path().join("absent")).is_err());
    }
}

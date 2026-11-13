// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Getting a model's files onto this computer, checked.
//!
//! A file is written under a temporary name (`<name>.part`) while its size
//! is counted and its SHA-256 digest computed. It is given its final name
//! only when the size and the digest are exactly the ones the catalogue
//! gives. Anything else deletes the temporary file: a file that was not
//! checked is never kept.
//!
//! The download is https only, redirects included, from the URL in the
//! catalogue: the publisher's own repository, pinned to one revision. A file
//! the person downloaded by other means can be imported with the same
//! checks.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::catalogue::ModelFile;
use crate::codes::AiLocalCode;
use crate::store::PART_SUFFIX;

/// The user agent sent with a download.
pub const USER_AGENT: &str = concat!("tur-ai-local/", env!("CARGO_PKG_VERSION"));

/// Why a file could not be made usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The server could not be reached, or the connection broke.
    Network(String),
    /// The server answered with a status other than 200.
    Status(u16),
    /// The file announced or received has another size.
    Size {
        /// The catalogue's size.
        expected: u64,
        /// What was announced or received; for a file that was longer than
        /// expected, the bytes received before it was stopped.
        found: u64,
    },
    /// The file has another digest.
    Digest {
        /// The catalogue's digest.
        expected: String,
        /// The digest of what was received.
        found: String,
    },
    /// The file could not be written, read or renamed.
    Storage(String),
    /// The person asked for the download to stop. What was received is
    /// kept under the temporary name, for the next attempt to go on from.
    Cancelled,
}

impl DownloadError {
    /// The code that reports it.
    pub fn code(&self) -> AiLocalCode {
        match self {
            DownloadError::Network(_) | DownloadError::Status(_) => AiLocalCode::DownloadFailed,
            DownloadError::Size { .. } => AiLocalCode::SizeMismatch,
            DownloadError::Digest { .. } => AiLocalCode::DigestMismatch,
            DownloadError::Storage(_) => AiLocalCode::StorageFailed,
            DownloadError::Cancelled => AiLocalCode::Cancelled,
        }
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Network(detail) => write!(f, "network: {detail}"),
            DownloadError::Status(status) => write!(f, "the server answered with status {status}"),
            DownloadError::Size { expected, found } => {
                write!(f, "size {found} bytes; the catalogue gives {expected}")
            }
            DownloadError::Digest { expected, found } => {
                write!(f, "SHA-256 {found}; the catalogue gives {expected}")
            }
            DownloadError::Storage(detail) => write!(f, "storage: {detail}"),
            DownloadError::Cancelled => f.write_str("stopped on request"),
        }
    }
}

fn storage(path: &Path, error: &io::Error) -> DownloadError {
    DownloadError::Storage(format!("{}: {error}", path.display()))
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The temporary name of a file being written.
pub fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PART_SUFFIX);
    destination.with_file_name(name)
}

/// Copy `reader` to `destination`, checked against `expected`.
///
/// Writes `<destination>.part`, counts and hashes every byte, stops as soon
/// as more bytes than expected arrive, and renames the file only when size
/// and digest match. On any failure the temporary file is deleted.
/// `progress` is called with the bytes written so far.
pub fn write_checked(
    reader: &mut dyn Read,
    expected: &ModelFile,
    destination: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    let temporary = part_path(destination);
    let result = copy_and_hash(reader, expected, &temporary, progress);
    match result {
        Ok(()) => fs::rename(&temporary, destination).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            storage(destination, &error)
        }),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn copy_and_hash(
    reader: &mut dyn Read,
    expected: &ModelFile,
    temporary: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    let file = File::create(temporary).map_err(|error| storage(temporary, &error))?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut written: u64 = 0;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(DownloadError::Network(error.to_string())),
        };
        written += read as u64;
        if written > expected.size_bytes {
            return Err(DownloadError::Size {
                expected: expected.size_bytes,
                found: written,
            });
        }
        hasher.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| storage(temporary, &error))?;
        progress(written);
    }
    let file = writer
        .into_inner()
        .map_err(|error| storage(temporary, error.error()))?;
    file.sync_all()
        .map_err(|error| storage(temporary, &error))?;
    if written != expected.size_bytes {
        return Err(DownloadError::Size {
            expected: expected.size_bytes,
            found: written,
        });
    }
    let found = hex(&hasher.finalize());
    if found != expected.sha256 {
        return Err(DownloadError::Digest {
            expected: expected.sha256.clone(),
            found,
        });
    }
    Ok(())
}

/// Check a file already in place, without changing it.
pub fn check_existing(path: &Path, expected: &ModelFile) -> Result<(), DownloadError> {
    let mut file = File::open(path).map_err(|error| storage(path, &error))?;
    let size = file
        .metadata()
        .map_err(|error| storage(path, &error))?
        .len();
    if size != expected.size_bytes {
        return Err(DownloadError::Size {
            expected: expected.size_bytes,
            found: size,
        });
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(storage(path, &error)),
        };
        hasher.update(&buffer[..read]);
    }
    let found = hex(&hasher.finalize());
    if found != expected.sha256 {
        return Err(DownloadError::Digest {
            expected: expected.sha256.clone(),
            found,
        });
    }
    Ok(())
}

/// The HTTP client for downloads: https only, redirects followed (and held
/// to https), status codes returned rather than turned into errors.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(10)
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// Download `file` from its URL to `destination`, checked.
pub fn download(
    agent: &ureq::Agent,
    file: &ModelFile,
    destination: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    if !file.url.starts_with("https://") {
        return Err(DownloadError::Network("the URL is not https".to_owned()));
    }
    let response = agent
        .get(file.url.as_str())
        .call()
        .map_err(|error| DownloadError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(DownloadError::Status(status));
    }
    // A size announced before any byte is read is checked at once, so that
    // a different file is not downloaded only to be deleted.
    let announced = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    if let Some(announced) = announced
        && announced != file.size_bytes
    {
        return Err(DownloadError::Size {
            expected: file.size_bytes,
            found: announced,
        });
    }
    let mut reader = response.into_body().into_reader();
    write_checked(&mut reader, file, destination, progress)
}

/// Import a file the person already has, checked like a download. The
/// source is copied, never moved.
pub fn import(
    source: &Path,
    file: &ModelFile,
    destination: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    let size = fs::metadata(source)
        .map_err(|error| storage(source, &error))?
        .len();
    if size != file.size_bytes {
        return Err(DownloadError::Size {
            expected: file.size_bytes,
            found: size,
        });
    }
    let mut reader = File::open(source).map_err(|error| storage(source, &error))?;
    write_checked(&mut reader, file, destination, progress)
}

/// Which addresses a download may come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// `https` only, redirects included: what every command uses.
    HttpsOnly,
    /// Also plain `http` to a loopback address. For tests of this library
    /// against a server on the test machine; no command offers it.
    AllowLoopbackHttp,
}

impl Scheme {
    /// Whether a download may start at `url`.
    pub fn allows(self, url: &str) -> bool {
        if url.starts_with("https://") {
            return true;
        }
        if self == Scheme::HttpsOnly {
            return false;
        }
        let Some(rest) = url.strip_prefix("http://") else {
            return false;
        };
        let authority = rest.split('/').next().unwrap_or("");
        let host = authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host);
        host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    }
}

/// The HTTP client for a resumable download.
pub fn agent_for(scheme: Scheme) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(scheme == Scheme::HttpsOnly)
        .http_status_as_error(false)
        .max_redirects(10)
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// The digest of the first bytes of a file: what an earlier attempt
/// received.
fn hash_file(path: &Path, hasher: &mut Sha256) -> Result<u64, DownloadError> {
    let mut file = File::open(path).map_err(|error| storage(path, &error))?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(storage(path, &error)),
        };
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok(total)
}

/// Check the whole temporary file and give it its final name, or delete it.
fn finish(
    temporary: &Path,
    destination: &Path,
    hasher: Sha256,
    expected: &ModelFile,
) -> Result<(), DownloadError> {
    let found = hex(&hasher.finalize());
    if found != expected.sha256 {
        let _ = fs::remove_file(temporary);
        return Err(DownloadError::Digest {
            expected: expected.sha256.clone(),
            found,
        });
    }
    fs::rename(temporary, destination).map_err(|error| {
        let _ = fs::remove_file(temporary);
        storage(destination, &error)
    })
}

/// The bytes of the temporary file of `destination`, if one is there.
pub fn partial_bytes(destination: &Path) -> u64 {
    fs::metadata(part_path(destination)).map_or(0, |metadata| metadata.len())
}

/// Download `file` to `destination`, going on from what an earlier attempt
/// left under the temporary name.
///
/// What is already there is hashed again and the rest is asked for with an
/// HTTP range request; a server that answers with the whole file instead is
/// read from the start. The size and the SHA-256 digest of the whole file
/// are checked before it gets its final name, as for [`download`]. A file
/// that turns out longer than the catalogue's, or of another digest, is
/// deleted. A connection that breaks, and a stop the person asked for
/// (`cancelled` is asked after every piece), keep what was received, for the
/// next attempt. `progress` is called with the bytes of the file so far.
///
/// # Errors
///
/// [`DownloadError`]; `Cancelled` when `cancelled` said so.
pub fn download_resumable(
    agent: &ureq::Agent,
    scheme: Scheme,
    file: &ModelFile,
    destination: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    if !scheme.allows(&file.url) {
        return Err(DownloadError::Network("the URL is not https".to_owned()));
    }
    let temporary = part_path(destination);
    let mut have = partial_bytes(destination);
    if have > file.size_bytes {
        let _ = fs::remove_file(&temporary);
        have = 0;
    }
    let mut hasher = Sha256::new();
    if have > 0 {
        have = hash_file(&temporary, &mut hasher)?;
        progress(have);
    }
    if have == file.size_bytes {
        return finish(&temporary, destination, hasher, file);
    }
    if cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let mut request = agent.get(file.url.as_str());
    if have > 0 {
        request = request.header("range", format!("bytes={have}-"));
    }
    let response = request
        .call()
        .map_err(|error| DownloadError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let resumed = match status {
        206 if have > 0 => {
            let expected_range =
                format!("bytes {have}-{}/{}", file.size_bytes - 1, file.size_bytes);
            if header("content-range").as_deref().map(str::trim) != Some(expected_range.as_str()) {
                // Not the part that was asked for: start again next time.
                let _ = fs::remove_file(&temporary);
                return Err(DownloadError::Network(
                    "the server sent another part of the file than the one asked for".to_owned(),
                ));
            }
            true
        }
        200 => false,
        other => return Err(DownloadError::Status(other)),
    };
    if !resumed && have > 0 {
        have = 0;
        hasher = Sha256::new();
    }
    let remaining = file.size_bytes - have;
    if let Some(announced) =
        header("content-length").and_then(|value| value.trim().parse::<u64>().ok())
        && announced != remaining
    {
        let _ = fs::remove_file(&temporary);
        return Err(DownloadError::Size {
            expected: file.size_bytes,
            found: have.saturating_add(announced),
        });
    }
    let output = if resumed {
        fs::OpenOptions::new().append(true).open(&temporary)
    } else {
        File::create(&temporary)
    }
    .map_err(|error| storage(&temporary, &error))?;
    let mut writer = BufWriter::with_capacity(1 << 20, output);
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = have;
    let flush = |writer: BufWriter<File>| -> Result<(), DownloadError> {
        let file = writer
            .into_inner()
            .map_err(|error| storage(&temporary, error.error()))?;
        file.sync_all().map_err(|error| storage(&temporary, &error))
    };
    loop {
        if cancelled() {
            flush(writer)?;
            return Err(DownloadError::Cancelled);
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                flush(writer)?;
                return Err(DownloadError::Network(error.to_string()));
            }
        };
        total += read as u64;
        if total > file.size_bytes {
            drop(writer);
            let _ = fs::remove_file(&temporary);
            return Err(DownloadError::Size {
                expected: file.size_bytes,
                found: total,
            });
        }
        hasher.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| storage(&temporary, &error))?;
        progress(total);
    }
    flush(writer)?;
    if total < file.size_bytes {
        return Err(DownloadError::Network(format!(
            "the connection closed after {total} of {} bytes",
            file.size_bytes
        )));
    }
    finish(&temporary, destination, hasher, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tur-ai-local-download-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn expected(data: &[u8]) -> ModelFile {
        ModelFile {
            name: "test.gguf".to_owned(),
            url: "https://example.org/r/0123456789abcdef0123456789abcdef01234567/test.gguf"
                .to_owned(),
            size_bytes: data.len() as u64,
            sha256: hex(&Sha256::digest(data)),
        }
    }

    #[test]
    fn a_matching_file_is_kept_under_its_name() {
        let dir = scratch("match");
        let data = b"GGUF the model".to_vec();
        let destination = dir.join("test.gguf");
        let mut seen = 0;
        write_checked(
            &mut data.as_slice(),
            &expected(&data),
            &destination,
            &mut |n| seen = n,
        )
        .expect("kept");
        assert_eq!(fs::read(&destination).expect("written"), data);
        assert_eq!(seen, data.len() as u64);
        assert!(!part_path(&destination).exists());
        check_existing(&destination, &expected(&data)).expect("still the same file");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_changed_byte_is_refused_and_nothing_is_kept() {
        let dir = scratch("digest");
        let data = b"GGUF the model".to_vec();
        let mut changed = data.clone();
        changed[5] ^= 0x01;
        let destination = dir.join("test.gguf");
        let error = write_checked(
            &mut changed.as_slice(),
            &expected(&data),
            &destination,
            &mut |_| {},
        )
        .expect_err("refused");
        assert_eq!(error.code(), AiLocalCode::DigestMismatch);
        assert!(!destination.exists());
        assert!(!part_path(&destination).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_short_or_long_file_is_refused_and_nothing_is_kept() {
        let dir = scratch("size");
        let data = b"GGUF the model".to_vec();
        let destination = dir.join("test.gguf");
        let short = &data[..data.len() - 1];
        let error = write_checked(&mut &short[..], &expected(&data), &destination, &mut |_| {})
            .expect_err("refused");
        assert_eq!(
            error,
            DownloadError::Size {
                expected: data.len() as u64,
                found: data.len() as u64 - 1
            }
        );
        let mut long = data.clone();
        long.extend_from_slice(b"more");
        let error = write_checked(
            &mut long.as_slice(),
            &expected(&data),
            &destination,
            &mut |_| {},
        )
        .expect_err("refused");
        assert_eq!(error.code(), AiLocalCode::SizeMismatch);
        assert!(!destination.exists());
        assert!(!part_path(&destination).exists());

        let source = dir.join("by-hand.gguf");
        fs::write(&source, short).expect("source");
        let error =
            import(&source, &expected(&data), &destination, &mut |_| {}).expect_err("refused");
        assert_eq!(error.code(), AiLocalCode::SizeMismatch);
        assert!(
            source.exists(),
            "an import never moves or deletes the source"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

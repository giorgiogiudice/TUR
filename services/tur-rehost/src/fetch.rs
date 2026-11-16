// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Fetching the files TUR's index pins, with the system's `curl`, and
//! checking each one before anything uses it.
//!
//! As in `tur-apps` (ADR-0019): `curl` ignores every configuration file,
//! speaks HTTPS only, also on redirects, and stops at the pinned size. The
//! file lands under a temporary name, its SHA-256 and size are compared
//! with the index, and only then is it renamed to its digest under
//! `downloads/`. A file that differs is deleted (`TUR-REHOST-FETCH-002`).
//! A file already there is checked again before it is used.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::codes::{Code, Failure};
use crate::index::Artifact;
use crate::runner::{Invocation, Runner};

/// How long one download may take: 30 minutes.
pub const FETCH_SECONDS: u64 = 30 * 60;

/// What was fetched, for the application's record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fetched {
    /// What the file is, in a few words.
    pub what: String,
    /// Where it came from.
    pub url: String,
    /// Its SHA-256, which matched the index.
    pub sha256: String,
    /// Its size.
    pub size: u64,
    /// Its licence, when known.
    pub licence: Option<String>,
    /// Whether it was already in the cache.
    pub cached: bool,
}

/// The SHA-256 and size of a file.
pub fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((hex(&hasher.finalize()), size))
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 15)] as char);
    }
    out
}

/// Where a pinned file is kept once verified.
pub fn cached_path(downloads: &Path, artifact: &Artifact) -> PathBuf {
    downloads.join(&artifact.sha256)
}

/// The `curl` arguments that fetch `url` to `output`, stopping beyond
/// `max_bytes`.
pub fn curl_arguments(url: &str, output: &str, max_bytes: u64) -> Vec<String> {
    vec![
        // Must come first: no ~/.curlrc or other configuration file.
        "--disable".to_owned(),
        "--fail".to_owned(),
        "--location".to_owned(),
        "--proto".to_owned(),
        "=https".to_owned(),
        "--proto-redir".to_owned(),
        "=https".to_owned(),
        "--tlsv1.2".to_owned(),
        "--silent".to_owned(),
        "--show-error".to_owned(),
        "--retry".to_owned(),
        "2".to_owned(),
        "--max-filesize".to_owned(),
        max_bytes.to_string(),
        "--output".to_owned(),
        output.to_owned(),
        "--".to_owned(),
        url.to_owned(),
    ]
}

/// Whether the file at `path` is the pinned one.
pub fn verify(path: &Path, artifact: &Artifact) -> Result<bool, Failure> {
    let (sha256, size) = hash_file(path).map_err(|error| {
        Failure::new(
            Code::Unreadable,
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    Ok(sha256 == artifact.sha256 && size == artifact.size)
}

/// Fetches `artifact` into `downloads` (or finds it there), verified.
/// Returns where it is and the record of it.
pub fn fetch(
    runner: &mut dyn Runner,
    curl: &Path,
    downloads: &Path,
    artifact: &Artifact,
    what: &str,
    licence: Option<&str>,
) -> Result<(PathBuf, Fetched), Failure> {
    let target = cached_path(downloads, artifact);
    let record = |cached: bool| Fetched {
        what: what.to_owned(),
        url: artifact.url.clone(),
        sha256: artifact.sha256.clone(),
        size: artifact.size,
        licence: licence.map(str::to_owned),
        cached,
    };
    if target.is_file() {
        if verify(&target, artifact)? {
            return Ok((target, record(true)));
        }
        // Something changed it: fetch again.
        let _ = std::fs::remove_file(&target);
    }
    std::fs::create_dir_all(downloads).map_err(|error| {
        Failure::new(
            Code::StepFailed,
            format!("cannot create {}: {error}", downloads.display()),
        )
    })?;
    let partial = downloads.join(format!("{}.part-{}", artifact.sha256, std::process::id()));
    let Some(partial_text) = partial.to_str() else {
        return Err(Failure::new(
            Code::InputInvalid,
            "the download directory's path is not valid UTF-8",
        ));
    };
    let invocation = Invocation {
        purpose: format!("download {what}"),
        program: curl.to_path_buf(),
        args: curl_arguments(&artifact.url, partial_text, artifact.size),
        env: vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())],
        cwd: None,
        timeout_seconds: FETCH_SECONDS,
    };
    let outcome = runner.run(&invocation).map_err(|error| {
        Failure::new(
            Code::ProgramMissing,
            format!("curl cannot be started ({}): {error}", curl.display()),
        )
    });
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(failure) => {
            let _ = std::fs::remove_file(&partial);
            return Err(failure);
        }
    };
    if !outcome.succeeded() {
        let _ = std::fs::remove_file(&partial);
        return Err(Failure::new(
            Code::FetchFailed,
            format!(
                "the download of {what} from {} did not complete: {}",
                artifact.url,
                if outcome.timed_out {
                    "it took too long"
                } else {
                    outcome.last_line()
                }
            ),
        ));
    }
    match verify(&partial, artifact) {
        Ok(true) => {}
        Ok(false) => {
            let _ = std::fs::remove_file(&partial);
            return Err(Failure::new(
                Code::FetchMismatch,
                format!(
                    "{what} from {} is not the file TUR pinned (SHA-256 {}, {} bytes): it was deleted and nothing in it was used",
                    artifact.url, artifact.sha256, artifact.size
                ),
            ));
        }
        Err(failure) => {
            let _ = std::fs::remove_file(&partial);
            return Err(failure);
        }
    }
    std::fs::rename(&partial, &target).map_err(|error| {
        let _ = std::fs::remove_file(&partial);
        Failure::new(
            Code::StepFailed,
            format!("cannot keep the download in {}: {error}", target.display()),
        )
    })?;
    Ok((target, record(false)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Outcome, RecordingRunner};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tur-rehost-fetch-{}-{}-{label}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn artifact_for(bytes: &[u8]) -> Artifact {
        let digest = Sha256::digest(bytes);
        Artifact {
            url: "https://example.org/file.zip".to_owned(),
            sha256: hex(&digest),
            size: bytes.len() as u64,
            hash_source: "test".to_owned(),
            npm_integrity: None,
        }
    }

    /// A runner standing in for curl: writes `bytes` to the `--output`.
    fn curl_writing(bytes: &'static [u8]) -> RecordingRunner {
        RecordingRunner::new(move |invocation| {
            let at = invocation
                .args
                .iter()
                .position(|argument| argument == "--output")
                .unwrap();
            std::fs::write(&invocation.args[at + 1], bytes).unwrap();
            Outcome {
                status: Some(0),
                timed_out: false,
                output: String::new(),
            }
        })
    }

    #[test]
    fn sha256_of_the_empty_string_is_the_published_value() {
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_pinned_file_is_kept_under_its_digest() {
        let downloads = scratch("ok");
        let artifact = artifact_for(b"the runtime");
        let mut runner = curl_writing(b"the runtime");
        let (path, record) = fetch(
            &mut runner,
            Path::new("/usr/bin/curl"),
            &downloads,
            &artifact,
            "a runtime",
            Some("MIT"),
        )
        .unwrap();
        assert_eq!(path, downloads.join(&artifact.sha256));
        assert!(!record.cached);
        assert_eq!(std::fs::read(&path).unwrap(), b"the runtime");
        // curl is told to read no configuration and to speak HTTPS only.
        let call = &runner.calls[0];
        assert_eq!(call.args[0], "--disable");
        assert!(call.args.windows(2).any(|pair| pair == ["--proto", "=https"]));
        assert!(call.args.windows(2).any(|pair| pair == ["--proto-redir", "=https"]));
        // A second fetch finds it, checks it and fetches nothing.
        let mut idle = RecordingRunner::succeeding();
        let (_, again) = fetch(
            &mut idle,
            Path::new("/usr/bin/curl"),
            &downloads,
            &artifact,
            "a runtime",
            None,
        )
        .unwrap();
        assert!(again.cached);
        assert!(idle.calls.is_empty());
        let _ = std::fs::remove_dir_all(&downloads);
    }

    #[test]
    fn a_file_that_is_not_the_pinned_one_is_deleted() {
        let downloads = scratch("mismatch");
        let artifact = artifact_for(b"the runtime");
        let mut runner = curl_writing(b"something else");
        let failure = fetch(
            &mut runner,
            Path::new("/usr/bin/curl"),
            &downloads,
            &artifact,
            "a runtime",
            None,
        )
        .unwrap_err();
        assert_eq!(failure.code, Code::FetchMismatch);
        assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&downloads);
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Making sure a model is on this computer, checked, in one step: the part
//! of `tur-ai-local ensure` that does not start the engine.
//!
//! [`fetch_model`] looks at what the models folder already has, checks
//! that the disk has room for the rest, downloads each file from the
//! publisher's URL in the catalogue, going on from any partial file an
//! earlier attempt left, checks the size and the SHA-256 digest of every
//! file before it gets its final name, and writes the record of
//! verification. It tells what it does as [`EnsureEvent`]s, which the
//! command line prints as one JSON object per line. A model that is already
//! downloaded and verified is reported as such and nothing is fetched.
//!
//! Nothing here runs unless the person asked for the model: `ensure` is a
//! command, and the assistant's service calls it only when the person
//! chooses a local model.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::GIB;
use crate::SCHEMA_VERSION;
use crate::catalogue::Model;
use crate::codes::AiLocalCode;
use crate::download::{self, DownloadError, Scheme};
use crate::serve::State;
use crate::store::{self, InstallState, Store};

/// The space kept free on the disk beside a model's files.
pub const DISK_RESERVE: u64 = GIB;

/// The program that reports the free space of a file system. Never looked
/// up along a search path.
pub const DF: &str = "/usr/bin/df";

/// The name of the stop request of `ensure`, in the runtime folder.
pub const CANCEL_FILE: &str = "ensure-cancel";

/// One step of `ensure`, as it is printed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnsureEvent {
    /// The format version of the document.
    pub schema_version: u32,
    /// Always `ensure-event`.
    pub kind: &'static str,
    /// `plan`, `progress`, `verified`, `installed`, `already-present`,
    /// `stopping`, `starting`, `ready`, `failed` or `cancelled`.
    pub event: &'static str,
    /// The model.
    pub model: String,
    /// The code of an outcome.
    pub code: Option<AiLocalCode>,
    /// In plain words.
    pub message: String,
    /// The file this is about.
    pub file: Option<String>,
    /// The bytes downloaded and checked so far, of the whole model.
    pub bytes_done: Option<u64>,
    /// The bytes of the whole model.
    pub bytes_total: Option<u64>,
    /// The bytes still to be downloaded, in a plan.
    pub bytes_to_download: Option<u64>,
    /// The free space of the disk, in a plan; null when it could not be read.
    pub free_bytes: Option<u64>,
    /// The served model, when it is ready.
    pub state: Option<State>,
}

impl EnsureEvent {
    /// An event with nothing but its name, the model and the message.
    pub fn new(event: &'static str, model: &str, message: impl Into<String>) -> EnsureEvent {
        EnsureEvent {
            schema_version: SCHEMA_VERSION,
            kind: "ensure-event",
            event,
            model: model.to_owned(),
            code: None,
            message: message.into(),
            file: None,
            bytes_done: None,
            bytes_total: None,
            bytes_to_download: None,
            free_bytes: None,
            state: None,
        }
    }

    /// With a code.
    #[must_use]
    pub fn with_code(mut self, code: AiLocalCode) -> EnsureEvent {
        self.code = Some(code);
        self
    }
}

/// Why a model could not be made ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureError {
    /// The code.
    pub code: AiLocalCode,
    /// What exactly went wrong.
    pub detail: String,
}

impl EnsureError {
    /// An error with this code.
    pub fn new(code: AiLocalCode, detail: impl Into<String>) -> EnsureError {
        EnsureError {
            code,
            detail: detail.into(),
        }
    }
}

/// How [`fetch_model`] reaches the publisher and the disk.
pub struct FetchOptions<'a> {
    /// The HTTP client.
    pub agent: &'a ureq::Agent,
    /// Which addresses may be downloaded from: https only, except in tests.
    pub scheme: Scheme,
    /// The free space of the file system that holds a path; `None` when it
    /// cannot be read.
    pub free_space: &'a dyn Fn(&Path) -> Option<u64>,
    /// Whether the person asked to stop.
    pub cancelled: &'a dyn Fn() -> bool,
}

/// The nearest folder of `path` that exists: the models folder, or the
/// folder it will be created in.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
}

/// The free space of the file system that holds `path`, from `df`, in
/// bytes. `None` when `df` is not there or says something else.
pub fn free_space(path: &Path) -> Option<u64> {
    let folder = existing_ancestor(path)?;
    let output = Command::new(DF)
        .args(["--output=avail", "-B1", "--"])
        .arg(&folder)
        .env_clear()
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_df(&String::from_utf8_lossy(&output.stdout))
}

/// The number `df --output=avail -B1` printed under its heading.
pub fn parse_df(text: &str) -> Option<u64> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    lines.next()?;
    lines.next()?.trim().parse().ok()
}

/// Download and check every file of `model` that the models folder does not
/// have yet, as described in the module documentation.
///
/// # Errors
///
/// `TUR-AI-LOCAL-028` when the disk has no room; the download's codes
/// (`-008` to `-011`); `-029` when the person asked to stop. What was
/// received of a file is kept after a broken connection and a stop, and
/// deleted after a size or digest that does not match.
pub fn fetch_model(
    store: &Store,
    model: &Model,
    options: &FetchOptions<'_>,
    emit: &mut dyn FnMut(EnsureEvent),
) -> Result<(), EnsureError> {
    let storage = |path: &Path, error: &std::io::Error| {
        EnsureError::new(
            AiLocalCode::StorageFailed,
            format!("{}: {error}", path.display()),
        )
    };
    let dir = store.model_dir(&model.id).ok_or_else(|| {
        EnsureError::new(AiLocalCode::UnknownModel, format!("{:?} is not a model id", model.id))
    })?;
    let total = model.download_bytes();
    let (state, _) = store.state(model);
    match state {
        InstallState::Installed => {
            let mut event = EnsureEvent::new(
                "already-present",
                &model.id,
                format!(
                    "{} is already on this computer and was checked when it was downloaded.",
                    model.name
                ),
            )
            .with_code(AiLocalCode::Installed);
            event.bytes_done = Some(total);
            event.bytes_total = Some(total);
            emit(event);
            return Ok(());
        }
        InstallState::Damaged => {
            // A record that does not match: what is there cannot be trusted.
            store.remove(&model.id).map_err(|error| storage(&dir, &error))?;
        }
        InstallState::Absent => {}
    }
    store::create_private_dir(&dir).map_err(|error| storage(&dir, &error))?;

    // What is there: whole files and partial ones.
    let mut present: u64 = 0;
    for file in model.files() {
        let destination = dir.join(&file.name);
        if destination.is_file() {
            present += std::fs::metadata(&destination).map_or(0, |metadata| metadata.len());
        } else {
            present += download::partial_bytes(&destination).min(file.size_bytes);
        }
    }
    let present = present.min(total);
    let to_download = total - present;
    let free = (options.free_space)(&dir);
    let mut plan = EnsureEvent::new(
        "plan",
        &model.id,
        format!(
            "{} downloads {} from {}{}.",
            model.name,
            crate::human_bytes(to_download),
            model.repository,
            if present > 0 {
                format!(", going on from {} already here", crate::human_bytes(present))
            } else {
                String::new()
            }
        ),
    );
    plan.bytes_done = Some(present);
    plan.bytes_total = Some(total);
    plan.bytes_to_download = Some(to_download);
    plan.free_bytes = free;
    emit(plan);
    if let Some(free) = free
        && free < to_download.saturating_add(DISK_RESERVE)
    {
        return Err(EnsureError::new(
            AiLocalCode::NoDiskSpace,
            format!(
                "{} to download and {} kept free, and {} is free on the disk of {}",
                crate::human_bytes(to_download),
                crate::human_bytes(DISK_RESERVE),
                crate::human_bytes(free),
                dir.display()
            ),
        ));
    }

    let mut done_before: u64 = 0;
    for file in model.files() {
        let destination = dir.join(&file.name);
        if destination.is_file() {
            // A whole file from an attempt that stopped before the record
            // was written: checked again, kept only if it matches.
            match download::check_existing(&destination, file) {
                Ok(()) => {
                    done_before += file.size_bytes;
                    continue;
                }
                Err(_) => {
                    std::fs::remove_file(&destination)
                        .map_err(|error| storage(&destination, &error))?;
                }
            }
        }
        let step = (total / 100).max(1 << 20);
        let mut next = 0u64;
        let name = file.name.clone();
        let mut progress = |bytes: u64| {
            let done = done_before + bytes;
            if done >= next || bytes == file.size_bytes {
                let mut event = EnsureEvent::new(
                    "progress",
                    &model.id,
                    format!(
                        "{} of {} ({}%).",
                        crate::human_bytes(done),
                        crate::human_bytes(total),
                        done.saturating_mul(100) / total.max(1)
                    ),
                );
                event.file = Some(name.clone());
                event.bytes_done = Some(done);
                event.bytes_total = Some(total);
                emit(event);
                next = done.saturating_add(step);
            }
        };
        let result = download::download_resumable(
            options.agent,
            options.scheme,
            file,
            &destination,
            options.cancelled,
            &mut progress,
        );
        if let Err(error) = result {
            let code = error.code();
            return Err(match error {
                DownloadError::Cancelled => EnsureError::new(
                    code,
                    format!(
                        "{}: stopped after {} of {}; the partial file is kept",
                        file.name,
                        crate::human_bytes(done_before + download::partial_bytes(&destination)),
                        crate::human_bytes(total)
                    ),
                ),
                other => EnsureError::new(code, format!("{}: {other}", file.name)),
            });
        }
        done_before += file.size_bytes;
        let mut verified = EnsureEvent::new(
            "verified",
            &model.id,
            format!(
                "{} has the size and the SHA-256 digest the catalogue gives.",
                file.name
            ),
        );
        verified.file = Some(file.name.clone());
        verified.bytes_done = Some(done_before);
        verified.bytes_total = Some(total);
        emit(verified);
    }
    store
        .write_verified(model, store::now_seconds())
        .map_err(|error| storage(&dir, &error))?;
    let mut installed = EnsureEvent::new(
        "installed",
        &model.id,
        AiLocalCode::Installed.entry().user_message,
    )
    .with_code(AiLocalCode::Installed);
    installed.bytes_done = Some(total);
    installed.bytes_total = Some(total);
    emit(installed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_free_space_is_read_from_df() {
        assert_eq!(parse_df("    Avail\n123456789\n"), Some(123_456_789));
        assert_eq!(parse_df("Avail\n"), None);
        assert_eq!(parse_df("Avail\nlots\n"), None);
        // On this machine df answers for the root folder.
        if Path::new(DF).is_file() {
            assert!(free_space(Path::new("/nonexistent/below/root")).is_some());
        }
    }

    #[test]
    fn only_https_unless_a_test_allows_loopback_http() {
        let https = "https://huggingface.co/x/resolve/r/m.gguf";
        assert!(Scheme::HttpsOnly.allows(https));
        assert!(!Scheme::HttpsOnly.allows("http://127.0.0.1:8080/m.gguf"));
        assert!(Scheme::AllowLoopbackHttp.allows("http://127.0.0.1:8080/m.gguf"));
        assert!(Scheme::AllowLoopbackHttp.allows("http://[::1]:8080/m.gguf"));
        assert!(Scheme::AllowLoopbackHttp.allows("http://localhost/m.gguf"));
        assert!(!Scheme::AllowLoopbackHttp.allows("http://example.org/m.gguf"));
        assert!(!Scheme::AllowLoopbackHttp.allows("http://10.0.0.1/m.gguf"));
        assert!(!Scheme::AllowLoopbackHttp.allows("ftp://127.0.0.1/m.gguf"));
    }
}

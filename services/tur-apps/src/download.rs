// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The one place this component uses the network itself.
//!
//! Two things are fetched, both over `https` only, both through the system's
//! `curl` (ADR-0019 says why `curl` and not an HTTP library linked in):
//!
//! * an installer the catalogue pins ([`fetch_installer`]): only a
//!   [`tur_catalogue::Installer`], whose URL, size and SHA-256 the catalogue
//!   holds, can be fetched. The file is written beside its final name,
//!   checked against the size and the digest, and only then given its name;
//!   a file that does not match is deleted, and nothing in it has run;
//! * a description of a Flatpak application from Flathub's public API
//!   ([`fetch_flathub_summary`]), for the sizes the plan shows.
//!
//! What `curl` is asked to do, always: ignore every configuration file
//! (`-q`, first); `https` only, on the first request and on every redirect;
//! at most ten redirects; fail on an HTTP error; stop at the expected size;
//! time limits. Flatpak's own downloads are Flatpak's, not this module's.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use tur_catalogue::Installer;

use crate::codes::{Code, Failure};
use crate::digest::{Hashed, verify};
use crate::exec::spawn;

/// The address of Flathub's public API.
pub const FLATHUB_API: &str = "https://flathub.org/api/v2";

/// The largest answer accepted from Flathub's API, in bytes.
pub const MAX_API_BYTES: u64 = 1024 * 1024;

/// How often a running download is looked at for progress.
const POLL: Duration = Duration::from_millis(500);

/// The options every `curl` run gets, before its URL and output.
fn common_arguments() -> Vec<String> {
    [
        // Must come first: no ~/.curlrc or any other configuration file.
        "-q",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--location",
        "--max-redirs",
        "10",
        "--fail",
        "--no-progress-meter",
        "--connect-timeout",
        "30",
        "--retry",
        "2",
    ]
    .iter()
    .map(|argument| (*argument).to_owned())
    .collect()
}

/// The `curl` command line that downloads `url` to `output`, stopping at
/// `max_bytes`, and prints where it ended up and the HTTP status.
pub fn installer_arguments(url: &str, output: &Path, max_bytes: u64) -> Vec<String> {
    let mut arguments = common_arguments();
    arguments.extend([
        "--max-time".to_owned(),
        "3600".to_owned(),
        "--max-filesize".to_owned(),
        max_bytes.to_string(),
        "--output".to_owned(),
        output.display().to_string(),
        "--write-out".to_owned(),
        "%{url_effective}\\n%{http_code}\\n".to_owned(),
        "--".to_owned(),
        url.to_owned(),
    ]);
    arguments
}

/// The `curl` command line that prints the answer of an API address.
pub fn api_arguments(url: &str) -> Vec<String> {
    let mut arguments = common_arguments();
    arguments.extend([
        "--max-time".to_owned(),
        "60".to_owned(),
        "--max-filesize".to_owned(),
        MAX_API_BYTES.to_string(),
        "--".to_owned(),
        url.to_owned(),
    ]);
    arguments
}

/// What a finished, checked download is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The file, under its own name, checked.
    pub path: PathBuf,
    /// Its digest and size.
    pub hashed: Hashed,
    /// The address the file was finally fetched from, after redirects.
    pub final_url: Option<String>,
}

/// Creates a new directory readable by the user only, at `base` or, when
/// that exists, at `base-1`, `base-2` and so on. An existing directory is
/// never used: someone else may have made it.
pub fn private_directory(base: &Path) -> Result<PathBuf, Failure> {
    use std::os::unix::fs::DirBuilderExt;
    for attempt in 0..100u32 {
        let candidate = if attempt == 0 {
            base.to_path_buf()
        } else {
            PathBuf::from(format!("{}-{attempt}", base.display()))
        };
        match fs::DirBuilder::new().mode(0o700).create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(Failure::new(
                    Code::StepFailed,
                    format!("cannot create {}: {error}", candidate.display()),
                ));
            }
        }
    }
    Err(Failure::new(
        Code::StepFailed,
        format!("cannot create a new directory beside {}", base.display()),
    ))
}

/// Downloads the installer the catalogue pins into a new private directory
/// made from `base` (see [`private_directory`]), checks it, and returns it.
///
/// `on_progress` is called about twice a second with the bytes written so
/// far. A failure removes the directory: nothing is left.
pub fn fetch_installer(
    curl: &Path,
    installer: &Installer,
    base: &Path,
    on_progress: &mut dyn FnMut(u64),
) -> Result<Fetched, Failure> {
    let name = installer.file_name();
    if name.is_empty() || name.starts_with('.') || name.contains('/') {
        return Err(Failure::new(
            Code::NotVerified,
            format!("{} does not end in a file name", installer.url),
        ));
    }
    let directory = private_directory(base)?;
    let fetched = fetch_into(curl, installer, &directory, name, on_progress);
    if fetched.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    fetched
}

fn fetch_into(
    curl: &Path,
    installer: &Installer,
    directory: &Path,
    name: &str,
    on_progress: &mut dyn FnMut(u64),
) -> Result<Fetched, Failure> {
    let target = directory.join(name);
    let partial = directory.join(format!(".{name}.part"));

    let mut command = Command::new(curl);
    command
        .args(installer_arguments(
            &installer.url,
            &partial,
            installer.size,
        ))
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(&mut command).map_err(|error| {
        Failure::new(
            Code::ProgramMissing,
            format!(
                "curl is needed to download the installer, and cannot be started ({}): {error}",
                curl.display()
            ),
        )
    })?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if crate::cancel::requested() {
                    // Cancelled: curl is one program, and stops at once.
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&partial);
                    return Err(crate::cancel::cancelled(
                        "The download was stopped, and what had arrived was deleted.",
                    ));
                }
                if let Ok(metadata) = fs::metadata(&partial) {
                    on_progress(metadata.len());
                }
                thread::sleep(POLL);
            }
            Err(_) => break,
        }
    }
    let output = child.wait_with_output().map_err(|error| {
        Failure::new(Code::DownloadFailed, format!("lost track of curl: {error}"))
    })?;
    if let Ok(metadata) = fs::metadata(&partial) {
        on_progress(metadata.len());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let final_url = stdout
        .lines()
        .next()
        .filter(|line| !line.is_empty())
        .map(str::to_owned);
    if !output.status.success() {
        let _ = fs::remove_file(&partial);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = output
            .status
            .code()
            .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"));
        return Err(Failure::new(
            Code::DownloadFailed,
            format!(
                "the download of {} stopped ({status}): {}. Nothing was installed.",
                installer.url,
                stderr.trim()
            ),
        ));
    }
    let hashed = match verify(&partial, &installer.sha256, installer.size) {
        Ok(hashed) => hashed,
        Err(mut failure) => {
            let _ = fs::remove_file(&partial);
            failure.message = format!(
                "the file downloaded from {} is not the one the catalogue pins: {}",
                installer.url, failure.message
            );
            return Err(failure);
        }
    };
    fs::rename(&partial, &target).map_err(|error| {
        let _ = fs::remove_file(&partial);
        Failure::new(
            Code::StepFailed,
            format!("cannot move the download into place: {error}"),
        )
    })?;
    Ok(Fetched {
        path: target,
        hashed,
        final_url,
    })
}

/// What Flathub's API says about an application or a runtime: the sizes
/// of its current release, in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlathubSummary {
    /// The download size of the stable branch.
    pub download_size: Option<u64>,
    /// The installed size of the stable branch.
    pub installed_size: Option<u64>,
    /// The runtime the application needs, as `name/arch/branch`.
    pub runtime: Option<String>,
}

/// Reads `/summary/<id>` of Flathub's API for one branch.
pub fn parse_flathub_summary(json: &str, branch: &str) -> Option<FlathubSummary> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let in_branch = |key: &str| {
        value
            .get("branches")
            .and_then(|branches| branches.get(branch))
            .and_then(|found| found.get(key))
            .and_then(serde_json::Value::as_u64)
    };
    let top = |key: &str| value.get(key).and_then(serde_json::Value::as_u64);
    Some(FlathubSummary {
        download_size: in_branch("download_size").or_else(|| top("download_size")),
        installed_size: in_branch("installed_size").or_else(|| top("installed_size")),
        runtime: value
            .get("metadata")
            .and_then(|metadata| metadata.get("runtime"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

/// Whether `id` may be put into an API address: a Flatpak identifier.
fn is_flatpak_id(id: &str) -> bool {
    id.split('.').count() >= 3
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

/// Asks Flathub's API for the sizes of an application or a runtime.
/// `None` when the answer cannot be had; the plan then says the size is not
/// known.
pub fn fetch_flathub_summary(curl: &Path, id: &str, branch: &str) -> Option<FlathubSummary> {
    if !is_flatpak_id(id) {
        return None;
    }
    let url = format!("{FLATHUB_API}/summary/{id}");
    let mut command = Command::new(curl);
    command
        .args(api_arguments(&url))
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn(&mut command).ok()?.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_flathub_summary(&String::from_utf8_lossy(&output.stdout), branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_ignores_its_configuration_and_speaks_https_only() {
        let arguments = installer_arguments(
            "https://example.org/a.exe",
            Path::new("/tmp/x/.a.exe.part"),
            1234,
        );
        assert_eq!(arguments[0], "-q", "-q must be the first argument");
        let joined = arguments.join(" ");
        for expected in [
            "--proto =https",
            "--proto-redir =https",
            "--max-redirs 10",
            "--fail",
            "--max-filesize 1234",
            "--output /tmp/x/.a.exe.part",
            "-- https://example.org/a.exe",
        ] {
            assert!(joined.contains(expected), "{expected} in {joined}");
        }
        // The URL comes after "--", so it is never read as an option.
        assert_eq!(arguments[arguments.len() - 2], "--");
        let api = api_arguments("https://flathub.org/api/v2/summary/org.example.App");
        assert_eq!(api[0], "-q");
        assert!(api.join(" ").contains("--max-filesize 1048576"));
    }

    #[test]
    fn flathub_s_summary_gives_sizes_in_bytes() {
        let json = r#"{"arches":["x86_64"],"branch":"stable","download_size":1,
            "installed_size":2,
            "branches":{"stable":{"download_size":1146880,"installed_size":2523136}},
            "metadata":{"runtime":"org.freedesktop.Platform/x86_64/26.08","sdk":"x"}}"#;
        let summary = parse_flathub_summary(json, "stable").unwrap();
        assert_eq!(summary.download_size, Some(1_146_880));
        assert_eq!(summary.installed_size, Some(2_523_136));
        assert_eq!(
            summary.runtime.as_deref(),
            Some("org.freedesktop.Platform/x86_64/26.08")
        );
        let other = parse_flathub_summary(json, "beta").unwrap();
        assert_eq!(other.download_size, Some(1));
        assert!(parse_flathub_summary("not json", "stable").is_none());
    }

    #[test]
    fn only_a_flatpak_identifier_goes_into_an_address() {
        assert!(is_flatpak_id("net.sourceforge.Klavaro"));
        assert!(!is_flatpak_id("org.example"));
        assert!(!is_flatpak_id("org.example.App/../x"));
        assert!(!is_flatpak_id("org.example.App?x=1"));
    }

    #[test]
    fn a_download_that_is_not_the_pinned_file_is_deleted() {
        // A stand-in for curl that writes four bytes to its --output.
        let directory =
            std::env::temp_dir().join(format!("tur-apps-download-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let fake = directory.join("curl");
        std::fs::write(
            &fake,
            "#!/bin/sh\nwhile [ \"$1\" != \"--output\" ]; do shift; done\nprintf 'evil' > \"$2\"\necho https://example.org/a.exe\necho 200\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let installer = Installer {
            url: "https://example.org/a.exe".to_owned(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
            size: 4,
            version: "1".to_owned(),
            licence: "MIT".to_owned(),
            licence_url: "https://example.org/licence".to_owned(),
            hash_source: "test".to_owned(),
            checked: "2026-11-12".to_owned(),
            portable: false,
        };
        let target = directory.join("downloads");
        let mut progress = Vec::new();
        let failure = fetch_installer(&fake, &installer, &target, &mut |bytes| {
            progress.push(bytes)
        })
        .unwrap_err();
        assert_eq!(failure.code, Code::NotVerified);
        assert!(failure.message.contains("not the one the catalogue pins"));
        // The private directory went with the file.
        assert!(!target.exists());
        // The last progress report is what was written: four bytes.
        assert!(progress.iter().all(|bytes| *bytes <= 4));
    }

    #[test]
    fn a_private_directory_is_new_and_the_user_s_alone() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("tur-apps-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let first = private_directory(&base).unwrap();
        assert_eq!(first, base);
        let mode = std::fs::metadata(&first).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // One that exists is never used again.
        let second = private_directory(&base).unwrap();
        assert_ne!(second, first);
        assert!(second.display().to_string().ends_with("-1"));
        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }
}

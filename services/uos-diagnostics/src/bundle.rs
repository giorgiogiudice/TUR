// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Building, writing and verifying a support bundle (specification §29).
//!
//! A bundle is a zstd-compressed tar archive of regular files:
//!
//! ```text
//! manifest.json            what the bundle is, its files and what is missing
//! README.txt               what this is, for whoever opens it
//! redaction-report.json    how many identifiers were replaced, per category
//! system/ boot/ hardware/ storage/ services/   the collected files
//! checksums.sha256         SHA-256 of every other member, sha256sum format
//! ```
//!
//! Members are written in that order, with the collected files sorted by
//! path, fixed ownership and mode, and the creation time as modification
//! time, so the same input always gives the same archive.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::collectors::{CollectOptions, Collector, run_collectors};
use crate::error::{DiagError, ErrorKind};
use crate::host::Host;
use crate::identity::gather_identity;
use crate::manifest::{
    COLLECTOR_NAME, COLLECTOR_VERSION, CollectorInfo, FileEntry, MANIFEST_SCHEMA_VERSION, Manifest,
    format_utc,
};
use crate::redact::{KnownIdentifiers, Profile, RedactionReport, Redactor};

/// The manifest's path inside a bundle.
pub const MANIFEST_PATH: &str = "manifest.json";
/// The read-me's path inside a bundle.
pub const README_PATH: &str = "README.txt";
/// The redaction report's path inside a bundle.
pub const REDACTION_REPORT_PATH: &str = "redaction-report.json";
/// The checksum list's path inside a bundle.
pub const CHECKSUMS_PATH: &str = "checksums.sha256";

/// zstd compression level. The library default; bundles are mostly text.
const COMPRESSION_LEVEL: i32 = 3;
/// The largest decompressed archive `verify-bundle` will open: 2 GiB.
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// One archive member: its path and its content.
pub type Member = (String, Vec<u8>);

/// What to collect and how.
#[derive(Debug, Clone)]
pub struct CollectRequest<'a> {
    /// The redaction profile.
    pub profile: Profile,
    /// Leave out everything about earlier boots.
    pub since_boot: bool,
    /// Creation time, in seconds since the Unix epoch.
    pub created_epoch_seconds: u64,
    /// The collectors to run.
    pub collectors: &'a [Collector],
    /// Names of this machine and its users, for the redactor.
    pub known: KnownIdentifiers,
    /// Size limit for one collector's output, in bytes.
    pub max_bytes: usize,
}

/// A bundle that has been built in memory.
#[derive(Debug, Clone)]
pub struct BuiltBundle {
    /// The compressed archive, ready to be written to a file.
    pub archive: Vec<u8>,
    /// The manifest that is inside the archive.
    pub manifest: Manifest,
    /// The redaction report that is inside the archive.
    pub report: RedactionReport,
}

/// The result of a successful verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The bundle's manifest.
    pub manifest: Manifest,
    /// How many members were checked against a recorded SHA-256.
    pub files_verified: usize,
}

/// Lower-case hexadecimal SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for byte in digest.iter().copied() {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    hex
}

fn length(data: &[u8]) -> u64 {
    u64::try_from(data.len()).unwrap_or(u64::MAX)
}

fn internal(error: impl std::fmt::Display) -> DiagError {
    DiagError::new(ErrorKind::Internal, error.to_string())
}

fn damaged(error: impl std::fmt::Display) -> DiagError {
    DiagError::new(ErrorKind::ArchiveDamaged, error.to_string())
}

/// Packs `members`, in the order given, into a zstd-compressed tar archive.
///
/// Every member is a regular file with mode 0644, owner and group 0, and
/// `mtime` as its modification time. The zstd frame carries a content
/// checksum, so damage to the file is detected when it is read.
pub fn pack(members: &[Member], mtime: u64) -> Result<Vec<u8>, DiagError> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in members {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(mtime);
        header.set_size(length(data));
        builder
            .append_data(&mut header, path, data.as_slice())
            .map_err(internal)?;
    }
    let tarball = builder.into_inner().map_err(internal)?;

    let mut encoder =
        zstd::stream::write::Encoder::new(Vec::new(), COMPRESSION_LEVEL).map_err(internal)?;
    encoder.include_checksum(true).map_err(internal)?;
    encoder.write_all(&tarball).map_err(internal)?;
    encoder.finish().map_err(internal)
}

/// Whether `path` is one `uos-diag` could have written: relative, with no
/// empty, `.` or `..` component and no backslash.
fn is_safe_member_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Unpacks an archive written by [`pack`] into its members, in archive order.
///
/// Fails if the data is not a complete zstd stream holding a tar archive, or
/// if a member is anything but a regular file with a safe, unique path.
pub fn unpack(archive: &[u8]) -> Result<Vec<Member>, DiagError> {
    let decoder = zstd::stream::read::Decoder::new(archive).map_err(damaged)?;
    let mut tarball = Vec::new();
    decoder
        .take(MAX_UNPACKED_BYTES + 1)
        .read_to_end(&mut tarball)
        .map_err(damaged)?;
    if length(&tarball) > MAX_UNPACKED_BYTES {
        return Err(DiagError::new(
            ErrorKind::BundleTooLarge,
            format!("the archive expands to more than {MAX_UNPACKED_BYTES} bytes"),
        ));
    }

    let mut reader = tar::Archive::new(tarball.as_slice());
    let mut members = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in reader.entries().map_err(damaged)? {
        let mut entry = entry.map_err(damaged)?;
        let path = entry
            .path()
            .map_err(damaged)?
            .to_string_lossy()
            .into_owned();
        if !entry.header().entry_type().is_file() {
            return Err(DiagError::new(
                ErrorKind::UnsafeMember,
                format!("{path} is not a regular file"),
            ));
        }
        if !is_safe_member_path(&path) {
            return Err(DiagError::new(
                ErrorKind::UnsafeMember,
                format!("{path} is not a plain relative path"),
            ));
        }
        if !seen.insert(path.clone()) {
            return Err(DiagError::new(
                ErrorKind::UnsafeMember,
                format!("{path} appears more than once"),
            ));
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(damaged)?;
        members.push((path, data));
    }
    Ok(members)
}

/// Renders `checksums.sha256` for `members`: one `<sha256>  <path>` line per
/// member, sorted by path, as `sha256sum --check` reads it.
pub fn render_checksums(members: &[Member]) -> String {
    let mut lines: Vec<String> = members
        .iter()
        .map(|(path, data)| {
            let digest = sha256_hex(data);
            format!("{digest}  {path}\n")
        })
        .collect();
    lines.sort_by(|a, b| a[66..].cmp(&b[66..]));
    lines.concat()
}

fn readme(profile: Profile) -> String {
    let profile_name = profile.as_str();
    let profile_note = match profile {
        Profile::Default => {
            "User names, host names, serial numbers, MAC addresses, public IP addresses,\n\
             UUIDs, Wi-Fi network names, the machine ID and labelled secrets were replaced\n\
             by placeholders such as <mac-1>. The same placeholder always stands for the\n\
             same value within this bundle."
        }
        Profile::Technical => {
            "This bundle was made with the technical profile. It KEEPS hardware serial\n\
             numbers, MAC addresses and disk UUIDs, which identify this computer and its\n\
             devices. User names, host names, public IP addresses, Wi-Fi network names,\n\
             the machine ID and labelled secrets were still replaced by placeholders.\n\
             Share it only with someone you trust with those identifiers."
        }
    };
    format!(
        "UOS support bundle\n\
         ==================\n\
         \n\
         This archive was created by uos-diag on this computer, at your request.\n\
         Nothing was uploaded or sent anywhere. It leaves this computer only if you\n\
         copy or send it yourself.\n\
         \n\
         Contents\n\
         --------\n\
         manifest.json          what this bundle is, the files in it with their\n\
         \x20                      SHA-256, and what could not be collected and why\n\
         redaction-report.json  how many identifiers were replaced, per category\n\
         system/                kernel, operating-system release, failed units\n\
         boot/                  system journal, kernel messages, crash-store listing\n\
         hardware/              PCI, USB, disks, graphics, audio, network, firmware\n\
         storage/               filesystems and mounts\n\
         services/              state of UOS services\n\
         checksums.sha256       SHA-256 of every other file in this archive\n\
         \n\
         It does not contain your documents, browser data, passwords, keys or\n\
         anything else from a home directory. System logs can still mention what\n\
         you were doing; read the files before you share them.\n\
         \n\
         Redaction profile: {profile_name}\n\
         {profile_note}\n\
         Redaction follows fixed rules and cannot recognise every personal detail.\n\
         \n\
         To check that the bundle is intact:\n\
         \n\
         \x20   uos-diag verify-bundle <this file>\n"
    )
}

fn to_pretty_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, DiagError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(internal)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Runs the collectors, redacts what they return and builds the archive.
///
/// A collector that fails is recorded in the manifest's `missing` list and
/// never fails the bundle. The only errors are internal ones.
pub fn build_bundle(
    host: &dyn Host,
    request: &CollectRequest<'_>,
) -> Result<BuiltBundle, DiagError> {
    let identity = gather_identity(host);
    let options = CollectOptions {
        since_boot: request.since_boot,
        max_bytes: request.max_bytes,
    };
    let (mut collected, mut missing) = run_collectors(host, request.collectors, &options);
    collected.sort_by(|a, b| a.path.cmp(&b.path));
    missing.sort_by(|a, b| a.path.cmp(&b.path));

    let mut known = request.known.clone();
    if let Some(boot_id) = &identity.boot_id {
        known.preserved_ids.push(boot_id.clone());
    }
    let mut redactor = Redactor::new(request.profile, &known);
    for item in &collected {
        redactor.learn_document(item.format, &item.text);
    }
    let mut data_members: Vec<Member> = Vec::with_capacity(collected.len());
    for item in &collected {
        let text = redactor.redact_document(item.format, &item.text);
        data_members.push((item.path.clone(), text.into_bytes()));
    }
    for entry in &mut missing {
        entry.detail = redactor.redact(&entry.detail);
    }
    let report = redactor.report();

    let mut listed: Vec<Member> = vec![
        (README_PATH.to_owned(), readme(request.profile).into_bytes()),
        (REDACTION_REPORT_PATH.to_owned(), to_pretty_json(&report)?),
    ];
    listed.extend(data_members);

    let mut files: Vec<FileEntry> = listed
        .iter()
        .map(|(path, data)| FileEntry {
            path: path.clone(),
            sha256: sha256_hex(data),
            size: length(data),
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        collector: CollectorInfo {
            name: COLLECTOR_NAME.to_owned(),
            version: COLLECTOR_VERSION.to_owned(),
        },
        created_utc: format_utc(request.created_epoch_seconds),
        uos: identity.uos,
        boot_id: identity.boot_id,
        architecture: identity.architecture,
        kernel_release: identity.kernel_release,
        installer_session_id: None,
        redaction_profile: request.profile,
        since_boot: request.since_boot,
        files,
        missing,
    };

    let mut members: Vec<Member> = Vec::with_capacity(listed.len() + 2);
    members.push((MANIFEST_PATH.to_owned(), to_pretty_json(&manifest)?));
    members.extend(listed);
    let checksums = render_checksums(&members);
    members.push((CHECKSUMS_PATH.to_owned(), checksums.into_bytes()));

    let archive = pack(&members, request.created_epoch_seconds)?;
    Ok(BuiltBundle {
        archive,
        manifest,
        report,
    })
}

/// Writes a bundle to a new file that only its owner can read.
///
/// Never overwrites: an existing file at `path` is an error. If writing
/// fails part-way, the partial file is removed.
pub fn write_bundle(path: &Path, archive: &[u8]) -> Result<(), DiagError> {
    let shown = path.display();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            let kind = if error.kind() == io::ErrorKind::AlreadyExists {
                ErrorKind::OutputExists
            } else {
                ErrorKind::OutputWrite
            };
            DiagError::new(kind, format!("{shown}: {error}"))
        })?;
    let written = file.write_all(archive).and_then(|()| file.sync_all());
    if let Err(error) = written {
        drop(file);
        // Best effort: the write error is the one worth reporting.
        let _ = fs::remove_file(path);
        return Err(DiagError::new(
            ErrorKind::OutputWrite,
            format!("{shown}: {error}"),
        ));
    }
    Ok(())
}

/// Parses `checksums.sha256` into path → SHA-256.
fn parse_checksums(text: &str) -> Result<BTreeMap<&str, &str>, DiagError> {
    let invalid = |detail: String| DiagError::new(ErrorKind::MetadataInvalid, detail);
    let mut sums = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let Some((digest, path)) = line.split_once("  ") else {
            return Err(invalid(format!(
                "{CHECKSUMS_PATH} line {number} is not '<sha256>  <path>'"
            )));
        };
        let valid_digest = digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid_digest || path.is_empty() {
            return Err(invalid(format!(
                "{CHECKSUMS_PATH} line {number} is not '<sha256>  <path>'"
            )));
        }
        if sums.insert(path, digest).is_some() {
            return Err(invalid(format!("{CHECKSUMS_PATH} lists {path} twice")));
        }
    }
    Ok(sums)
}

/// Verifies a bundle held in memory.
///
/// Checks, in this order: that the archive decompresses and unpacks into
/// regular files with safe, unique paths; that `manifest.json` and
/// `checksums.sha256` are present and well-formed and the manifest's schema
/// version is supported; that every member matches its SHA-256 in
/// `checksums.sha256` and, for listed files, its SHA-256 and size in the
/// manifest; and that the three file sets agree exactly.
pub fn verify_archive(archive: &[u8]) -> Result<Verified, DiagError> {
    let members = unpack(archive)?;
    let by_path: BTreeMap<&str, &[u8]> = members
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();

    let required = |path: &str| {
        by_path.get(path).copied().ok_or_else(|| {
            DiagError::new(
                ErrorKind::MemberMissing,
                format!("{path} is not in the archive"),
            )
        })
    };
    let manifest_bytes = required(MANIFEST_PATH)?;
    let checksum_bytes = required(CHECKSUMS_PATH)?;

    let invalid = |detail: String| DiagError::new(ErrorKind::MetadataInvalid, detail);
    let manifest_value: Value = serde_json::from_slice(manifest_bytes)
        .map_err(|error| invalid(format!("{MANIFEST_PATH}: {error}")))?;
    let version = manifest_value.get("schema_version").and_then(Value::as_u64);
    if version != Some(u64::from(MANIFEST_SCHEMA_VERSION)) {
        let found = match manifest_value.get("schema_version") {
            Some(value) => value.to_string(),
            None => "nothing".to_owned(),
        };
        return Err(DiagError::new(
            ErrorKind::SchemaUnsupported,
            format!(
                "{MANIFEST_PATH} has schema_version {found}; this build supports \
                 {MANIFEST_SCHEMA_VERSION}"
            ),
        ));
    }
    let manifest: Manifest = serde_json::from_value(manifest_value)
        .map_err(|error| invalid(format!("{MANIFEST_PATH}: {error}")))?;

    let checksum_text = std::str::from_utf8(checksum_bytes)
        .map_err(|error| invalid(format!("{CHECKSUMS_PATH}: {error}")))?;
    let sums = parse_checksums(checksum_text)?;

    let mismatch = |detail: String| DiagError::new(ErrorKind::ChecksumMismatch, detail);
    let unlisted = |detail: String| DiagError::new(ErrorKind::FileListMismatch, detail);

    let mut files_verified = 0;
    for (path, expected) in &sums {
        let Some(data) = by_path.get(path) else {
            return Err(unlisted(format!(
                "{path} is listed in {CHECKSUMS_PATH} but is not in the archive"
            )));
        };
        let actual = sha256_hex(data);
        if actual != *expected {
            return Err(mismatch(format!(
                "{path}: {CHECKSUMS_PATH} records {expected}, the file has {actual}"
            )));
        }
        files_verified += 1;
    }
    for path in by_path.keys() {
        if *path != CHECKSUMS_PATH && !sums.contains_key(path) {
            return Err(unlisted(format!(
                "{path} is in the archive but not listed in {CHECKSUMS_PATH}"
            )));
        }
    }

    let mut listed = BTreeSet::new();
    for file in &manifest.files {
        let path = file.path.as_str();
        let Some(data) = by_path.get(path) else {
            return Err(unlisted(format!(
                "{path} is listed in {MANIFEST_PATH} but is not in the archive"
            )));
        };
        let actual = sha256_hex(data);
        if actual != file.sha256 {
            let expected = &file.sha256;
            return Err(mismatch(format!(
                "{path}: {MANIFEST_PATH} records {expected}, the file has {actual}"
            )));
        }
        let actual_size = length(data);
        if actual_size != file.size {
            let expected = file.size;
            return Err(mismatch(format!(
                "{path}: {MANIFEST_PATH} records {expected} bytes, the file has {actual_size}"
            )));
        }
        if !listed.insert(path) {
            return Err(unlisted(format!(
                "{path} is listed twice in {MANIFEST_PATH}"
            )));
        }
    }
    for path in by_path.keys() {
        if *path != MANIFEST_PATH && *path != CHECKSUMS_PATH && !listed.contains(path) {
            return Err(unlisted(format!(
                "{path} is in the archive but not listed in {MANIFEST_PATH}"
            )));
        }
    }

    Ok(Verified {
        manifest,
        files_verified,
    })
}

/// Verifies the bundle in the file at `path`. See [`verify_archive`].
pub fn verify_file(path: &Path) -> Result<Verified, DiagError> {
    let archive = fs::read(path).map_err(|error| {
        let shown = path.display();
        DiagError::new(ErrorKind::BundleRead, format!("{shown}: {error}"))
    })?;
    verify_archive(&archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::COLLECTORS;
    use crate::host::FakeHost;
    use crate::manifest::MissingReason;

    const EPOCH: u64 = 1_700_000_000;

    fn member(path: &str, data: &str) -> Member {
        (path.to_owned(), data.as_bytes().to_vec())
    }

    fn host() -> FakeHost {
        FakeHost::new()
            .with_command(
                "uname",
                "Linux workstation-7 6.17.1-300.fc45.x86_64 #1 SMP x86_64 GNU/Linux\n",
            )
            .with_file(
                "/proc/cmdline",
                "root=UUID=6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10 quiet\n",
            )
            .with_file("/proc/sys/kernel/osrelease", "6.17.1-300.fc45.x86_64\n")
            .with_file(
                "/proc/sys/kernel/random/boot_id",
                "0f0e0d0c-0b0a-4909-8807-060504030201\n",
            )
            .with_file(
                "/usr/lib/uos/release",
                "UOS_VERSION=0.1.0\nUOS_BUILD_ID=20261112.g0123456789ab\n",
            )
            .with_failing_command(
                "dmesg",
                1,
                "dmesg: read kernel buffer failed: Operation not permitted for alice",
            )
    }

    fn request(profile: Profile) -> CollectRequest<'static> {
        CollectRequest {
            profile,
            since_boot: false,
            created_epoch_seconds: EPOCH,
            collectors: COLLECTORS,
            known: KnownIdentifiers {
                usernames: vec!["alice".to_owned()],
                hostnames: vec!["workstation-7".to_owned()],
                machine_ids: Vec::new(),
                preserved_ids: Vec::new(),
            },
            max_bytes: 1 << 20,
        }
    }

    /// Replaces one member of a built bundle and packs it again.
    fn repack(archive: &[u8], change: impl Fn(&mut Vec<Member>)) -> Vec<u8> {
        let mut members = unpack(archive).expect("the bundle unpacks");
        change(&mut members);
        pack(&members, EPOCH).expect("the members pack")
    }

    fn kind_of(result: Result<Verified, DiagError>) -> ErrorKind {
        result.expect_err("verification must fail").kind()
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn pack_and_unpack_round_trip_in_order() {
        let members = vec![
            member("manifest.json", "{}"),
            member("system/kernel.txt", "Linux\n"),
            member("boot/pstore/listing.txt", ""),
        ];
        let archive = pack(&members, EPOCH).expect("members pack");
        assert_eq!(unpack(&archive).expect("archive unpacks"), members);
        assert_eq!(pack(&members, EPOCH).expect("members pack"), archive);
    }

    #[test]
    fn unpack_rejects_what_is_not_a_bundle() {
        assert_eq!(
            unpack(b"this is not zstd")
                .expect_err("not an archive")
                .kind(),
            ErrorKind::ArchiveDamaged
        );
        let archive = pack(&[member("system/kernel.txt", "Linux\n")], EPOCH).expect("members pack");
        let truncated = &archive[..archive.len() - 5];
        assert_eq!(
            unpack(truncated).expect_err("truncated archive").kind(),
            ErrorKind::ArchiveDamaged
        );
        for path in ["../escape.txt", "/etc/passwd", "a//b", "./a", "a\\b"] {
            assert!(!is_safe_member_path(path), "{path}");
        }
        assert!(is_safe_member_path("boot/pstore/listing.txt"));
    }

    #[test]
    fn checksums_are_sorted_by_path_in_sha256sum_format() {
        let text = render_checksums(&[member("b.txt", "abc"), member("a.txt", "")]);
        assert_eq!(
            text,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.txt\n\
             ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  b.txt\n"
        );
        let parsed = parse_checksums(&text).expect("the list parses");
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed["a.txt"],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn malformed_checksum_lists_are_rejected() {
        let digest = "0".repeat(64);
        for text in [
            "no separator\n".to_owned(),
            format!("{digest} one-space.txt\n"),
            "abc  short-digest.txt\n".to_owned(),
            format!("{}  upper.txt\n", "A".repeat(64)),
            format!("{digest}  \n"),
            format!("{digest}  twice.txt\n{digest}  twice.txt\n"),
        ] {
            let error = parse_checksums(&text).expect_err("the list is malformed");
            assert_eq!(error.kind(), ErrorKind::MetadataInvalid, "{text}");
        }
    }

    #[test]
    fn a_built_bundle_has_the_documented_layout_and_verifies() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        let members = unpack(&built.archive).expect("the bundle unpacks");
        let paths: Vec<&str> = members.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "manifest.json",
                "README.txt",
                "redaction-report.json",
                "system/kernel-cmdline.txt",
                "system/kernel.txt",
                "system/uos-release.txt",
                "checksums.sha256",
            ]
        );

        let verified = verify_archive(&built.archive).expect("the bundle verifies");
        assert_eq!(verified.manifest, built.manifest);
        assert_eq!(verified.files_verified, 6);

        let manifest = &built.manifest;
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.created_utc, "2023-11-14T22:13:20Z");
        assert_eq!(
            manifest.uos.build_id.as_deref(),
            Some("20261112.g0123456789ab")
        );
        assert_eq!(
            manifest.boot_id.as_deref(),
            Some("0f0e0d0c-0b0a-4909-8807-060504030201")
        );
        assert_eq!(
            manifest.kernel_release.as_deref(),
            Some("6.17.1-300.fc45.x86_64")
        );
        assert_eq!(manifest.redaction_profile, Profile::Default);
        assert_eq!(manifest.collector.name, "uos-diag");
        assert_eq!(manifest.installer_session_id, None);
        let listed: Vec<&str> = manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(
            listed,
            [
                "README.txt",
                "redaction-report.json",
                "system/kernel-cmdline.txt",
                "system/kernel.txt",
                "system/uos-release.txt",
            ]
        );
        assert_eq!(manifest.missing.len(), COLLECTORS.len() - 3);
        let dmesg = manifest
            .missing
            .iter()
            .find(|entry| entry.path == "boot/kernel-ring-buffer.txt")
            .expect("the failed dmesg is recorded");
        assert_eq!(dmesg.reason, MissingReason::NonZeroExit);
        assert_eq!(
            dmesg.detail,
            "exit code 1: dmesg: read kernel buffer failed: Operation not permitted for <user-1>"
        );
    }

    #[test]
    fn nothing_in_a_default_bundle_carries_a_known_identifier() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        for (path, data) in unpack(&built.archive).expect("the bundle unpacks") {
            let text = String::from_utf8(data).expect("members are UTF-8");
            for original in ["alice", "workstation-7", "6f1c2a9e"] {
                assert!(!text.contains(original), "{original} is in {path}");
            }
        }
        let uuids = &built.report.categories["uuid"];
        assert_eq!((uuids.distinct_values, uuids.replacements), (1, 1));
    }

    #[test]
    fn the_technical_profile_is_recorded_and_keeps_disk_uuids() {
        let built = build_bundle(&host(), &request(Profile::Technical)).expect("the bundle builds");
        assert_eq!(built.manifest.redaction_profile, Profile::Technical);
        let members = unpack(&built.archive).expect("the bundle unpacks");
        let text_of = |wanted: &str| {
            let (_, data) = members
                .iter()
                .find(|(path, _)| path.as_str() == wanted)
                .expect("the member exists");
            String::from_utf8_lossy(data).into_owned()
        };
        assert!(
            text_of("system/kernel-cmdline.txt").contains("6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10")
        );
        assert!(text_of("system/kernel.txt").contains("<host-1>"));
        assert!(text_of("README.txt").contains("technical profile"));
        assert!(text_of("manifest.json").contains("\"redaction_profile\": \"technical\""));
        verify_archive(&built.archive).expect("the bundle verifies");
    }

    #[test]
    fn building_twice_gives_the_same_bytes() {
        let first = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        let second = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        assert_eq!(first.archive, second.archive);
    }

    #[test]
    fn a_bundle_from_a_host_with_nothing_still_builds_and_verifies() {
        let built =
            build_bundle(&FakeHost::new(), &request(Profile::Default)).expect("the bundle builds");
        assert_eq!(built.manifest.missing.len(), COLLECTORS.len());
        assert_eq!(built.manifest.files.len(), 2);
        assert_eq!(built.manifest.boot_id, None);
        assert_eq!(built.manifest.uos.build_id, None);
        verify_archive(&built.archive).expect("the bundle verifies");
    }

    #[test]
    fn since_boot_is_recorded_and_excludes_earlier_boots() {
        let mut since_boot = request(Profile::Default);
        since_boot.since_boot = true;
        let host = host().with_command("journalctl", "{\"MESSAGE\":\"hello\"}\n");
        let built = build_bundle(&host, &since_boot).expect("the bundle builds");
        assert!(built.manifest.since_boot);
        let paths: Vec<&str> = built
            .manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert!(paths.contains(&"boot/current-journal.jsonl"));
        assert!(!paths.contains(&"boot/previous-journal.jsonl"));
        let excluded: Vec<&str> = built
            .manifest
            .missing
            .iter()
            .filter(|entry| entry.reason == MissingReason::ExcludedByOption)
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(
            excluded,
            ["boot/previous-journal.jsonl", "boot/pstore/listing.txt"]
        );
    }

    #[test]
    fn a_changed_file_fails_with_checksum_mismatch() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        let tampered = repack(&built.archive, |members| {
            let (_, data) = members
                .iter_mut()
                .find(|(path, _)| path.as_str() == "system/kernel.txt")
                .expect("the member exists");
            data[0] ^= 0x01;
        });
        let error = verify_archive(&tampered).expect_err("verification must fail");
        assert_eq!(error.kind(), ErrorKind::ChecksumMismatch);
        assert_eq!(error.code().to_string(), "UOS-DIAG-BUNDLE-007");
        assert!(
            error.detail().contains("system/kernel.txt"),
            "{}",
            error.detail()
        );
    }

    #[test]
    fn a_file_changed_together_with_its_checksum_line_still_fails() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");
        let tampered = repack(&built.archive, |members| {
            for (path, data) in members.iter_mut() {
                if path.as_str() == "system/kernel.txt" {
                    data.extend_from_slice(b"extra\n");
                }
            }
            let listed: Vec<Member> = members
                .iter()
                .filter(|(path, _)| path.as_str() != CHECKSUMS_PATH)
                .cloned()
                .collect();
            let checksums = render_checksums(&listed).into_bytes();
            for (path, data) in members.iter_mut() {
                if path.as_str() == CHECKSUMS_PATH {
                    data.clone_from(&checksums);
                }
            }
        });
        // checksums.sha256 now agrees with the file, but the manifest does not.
        assert_eq!(
            kind_of(verify_archive(&tampered)),
            ErrorKind::ChecksumMismatch
        );
    }

    #[test]
    fn added_and_removed_files_fail_with_file_list_mismatch() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");

        let added = repack(&built.archive, |members| {
            members.push(member("system/extra.txt", "surprise"));
        });
        assert_eq!(kind_of(verify_archive(&added)), ErrorKind::FileListMismatch);

        let removed = repack(&built.archive, |members| {
            members.retain(|(path, _)| path.as_str() != "system/kernel.txt");
        });
        assert_eq!(
            kind_of(verify_archive(&removed)),
            ErrorKind::FileListMismatch
        );
    }

    #[test]
    fn missing_or_unreadable_metadata_fails_with_its_own_code() {
        let built = build_bundle(&host(), &request(Profile::Default)).expect("the bundle builds");

        let no_manifest = repack(&built.archive, |members| {
            members.retain(|(path, _)| path.as_str() != MANIFEST_PATH);
        });
        assert_eq!(
            kind_of(verify_archive(&no_manifest)),
            ErrorKind::MemberMissing
        );

        let no_checksums = repack(&built.archive, |members| {
            members.retain(|(path, _)| path.as_str() != CHECKSUMS_PATH);
        });
        assert_eq!(
            kind_of(verify_archive(&no_checksums)),
            ErrorKind::MemberMissing
        );

        let replace_manifest = |content: &'static str| {
            repack(&built.archive, |members| {
                for (path, data) in members.iter_mut() {
                    if path.as_str() == MANIFEST_PATH {
                        *data = content.as_bytes().to_vec();
                    }
                }
            })
        };
        assert_eq!(
            kind_of(verify_archive(&replace_manifest("not json"))),
            ErrorKind::MetadataInvalid
        );
        assert_eq!(
            kind_of(verify_archive(&replace_manifest("{\"schema_version\": 2}"))),
            ErrorKind::SchemaUnsupported
        );
        assert_eq!(
            kind_of(verify_archive(&replace_manifest("{\"files\": []}"))),
            ErrorKind::SchemaUnsupported
        );
        assert_eq!(
            kind_of(verify_archive(&replace_manifest("{\"schema_version\": 1}"))),
            ErrorKind::MetadataInvalid
        );
    }

    #[test]
    fn a_non_regular_member_is_refused() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        builder
            .append_link(&mut header, "system/kernel.txt", "/etc/shadow")
            .expect("the link is appended");
        let tarball = builder.into_inner().expect("the tar is finished");
        let archive = zstd::stream::encode_all(tarball.as_slice(), 3).expect("the tar compresses");
        assert_eq!(kind_of(verify_archive(&archive)), ErrorKind::UnsafeMember);
    }
}

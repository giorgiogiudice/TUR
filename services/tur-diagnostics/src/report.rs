// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Problem reports (ADR-0024).
//!
//! A problem report is an ordinary support bundle that carries one more
//! member, `report.json`: what happened, in words, with an error code and a
//! component where they are known. It is prepared on this computer, shown in
//! full, and sent to TUR's own receiver only when the person confirms it.
//!
//! * [`prepare`] collects a bundle for a report, with a smaller journal than
//!   `tur-diag collect` and always with the `default` redaction profile. The
//!   report's own text is redacted together with the collected files.
//! * [`inspect_archive`] checks that a file is a prepared report that a
//!   receiver will accept. The receiver runs the same checks.
//! * [`load_config`] reads the receiver's address. It is empty by default,
//!   and nothing can be sent until TUR has a receiver.
//! * [`send_prepared`] hands the file to `curl` and reads the receipt. It is
//!   called only by `tur-diag report send`, after the person has confirmed;
//!   nothing here retries in the background.
//! * [`SentRecord`] is the local list of sent reports, which Settings reads.
//! * [`request_from_boot_health`] turns a failed update recorded by boot
//!   health into a report proposal.
//!
//! Nothing in this module prints or logs a field's value in an error: error
//! details name the field, never its content.

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tur_core::ErrorCode;

use crate::bundle::{
    CollectRequest, ExtraDocument, MANIFEST_PATH, Member, REDACTION_REPORT_PATH, build_bundle_with,
    sha256_hex, verify_members,
};
use crate::collectors::{COLLECTORS, Collector, Source};
use crate::error::{DiagError, ErrorKind};
use crate::host::{Host, Limits, RunError};
use crate::manifest::Manifest;
use crate::redact::{Format, KnownIdentifiers, Profile, RedactionReport};

/// The path of the report inside a bundle.
pub const REPORT_PATH: &str = "report.json";
/// The version of `report.json` this build writes and accepts.
pub const REPORT_SCHEMA_VERSION: u32 = 1;
/// The largest bundle a receiver accepts, as sent: 8 MiB.
pub const MAX_UPLOAD_BYTES: u64 = 8 * 1024 * 1024;
/// The largest a bundle may expand to: 64 MiB.
pub const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024;
/// The size limit for one collector's output in a report: 8 MiB.
pub const REPORT_COLLECTOR_MAX_BYTES: usize = 8 * 1024 * 1024;
/// The longest title a person or P@x may give, in characters.
pub const MAX_TITLE_CHARS: usize = 120;
/// The longest description a person or P@x may give, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 4000;
/// The longest boot health notice text carried in a report, in characters.
pub const MAX_NOTICE_CHARS: usize = 1000;
/// The media type of an upload.
pub const CONTENT_TYPE: &str = "application/vnd.tur.diag-bundle+zstd";
/// The receiver's path for uploads, below its base address.
pub const UPLOAD_PATH: &str = "/v1/reports";
/// How long the receiver keeps a bundle, as the person is told.
pub const RETENTION_DAYS: u32 = 90;

/// The configuration files that name the receiver, in the order they are
/// read. The first that exists is used; an empty `RECEIVER_URL` in it means
/// that no receiver is configured.
pub const CONFIG_PATHS: [&str; 2] = [
    "/etc/tur/problem-reports.conf",
    "/usr/lib/tur/problem-reports.conf",
];

/// The components a report may name. A fixed list, so that nothing personal
/// can travel in a field that groups reports.
pub const COMPONENTS: &[&str] = &[
    "android",
    "apps",
    "audio",
    "boot",
    "catalogue",
    "desktop",
    "device",
    "diag",
    "display",
    "install",
    "network",
    "other",
    "pax",
    "runtime",
    "settings",
    "storage",
    "update",
    "windows",
];

/// The notice kinds of boot health (boot-health-and-rollback.md, §5.11).
pub const NOTICE_KINDS: &[&str] = &["fallback", "not-applied", "refused", "unhealthy"];

/// The journal arguments of a report: the newest 5 000 entries of each boot,
/// instead of the 50 000 and 20 000 of `tur-diag collect`, so that a report
/// stays well below [`MAX_UPLOAD_BYTES`].
const REPORT_CURRENT_JOURNAL: &[&str] =
    &["--boot=0", "--output=json", "--no-pager", "--lines=5000"];
const REPORT_PREVIOUS_JOURNAL: &[&str] =
    &["--boot=-1", "--output=json", "--no-pager", "--lines=5000"];

/// What kind of problem a report describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportKind {
    /// An update failed, was not applied, or was refused.
    FailedUpdate,
    /// The system started with a failed health check.
    BootProblem,
    /// A program or service crashed.
    Crash,
    /// A repair P@x or the person tried did not finish.
    RepairFailed,
    /// A problem the person describes.
    Request,
}

impl ReportKind {
    /// Every kind, in declaration order.
    pub const ALL: [ReportKind; 5] = [
        ReportKind::FailedUpdate,
        ReportKind::BootProblem,
        ReportKind::Crash,
        ReportKind::RepairFailed,
        ReportKind::Request,
    ];

    /// The kind as it appears in `report.json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReportKind::FailedUpdate => "failed-update",
            ReportKind::BootProblem => "boot-problem",
            ReportKind::Crash => "crash",
            ReportKind::RepairFailed => "repair-failed",
            ReportKind::Request => "request",
        }
    }
}

/// Who wrote the report's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparedBy {
    /// The person, on the command line or in Settings.
    Person,
    /// P@x, for the person to read and send.
    Pax,
}

impl PreparedBy {
    /// The value as it appears in `report.json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PreparedBy::Person => "person",
            PreparedBy::Pax => "pax",
        }
    }
}

/// What boot health recorded, when a report is made from its notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootHealthEvidence {
    /// The notice kind: `fallback`, `not-applied`, `refused` or `unhealthy`.
    pub notice_kind: String,
    /// The notice's error code, if it is a valid TUR code.
    pub error_code: Option<String>,
    /// The notice's text, as the person was shown it.
    pub text: String,
    /// The first required check that failed, where the notice names it.
    pub failed_check: Option<String>,
    /// The version of the image that was rejected, where boot health has it.
    pub rejected_version: Option<String>,
}

/// The content of `report.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRequest {
    /// Version of this document's format.
    pub schema_version: u32,
    /// What kind of problem this is.
    pub kind: ReportKind,
    /// One line that names the problem.
    pub title: String,
    /// What happened, in words. May be empty.
    pub description: String,
    /// The TUR error code involved, if one is known.
    pub error_code: Option<String>,
    /// The component concerned, from [`COMPONENTS`].
    pub component: String,
    /// The TUR version the problem is in, when it is not the running one
    /// (for a failed update: the version that failed).
    pub affected_version: Option<String>,
    /// Who wrote the text.
    pub prepared_by: PreparedBy,
    /// What boot health recorded, for a report made from its notice.
    pub boot_health: Option<BootHealthEvidence>,
}

/// How strictly [`ReportRequest::validate`] applies the length limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Text as a person or P@x gave it: the limits as documented.
    Input,
    /// Text as it arrives in a bundle. Redaction can make a text longer (a
    /// pseudonym may be longer than what it replaces), so twice the limits.
    Received,
}

impl ReportRequest {
    /// Checks every field. The message names the field and the rule, never
    /// the value.
    pub fn validate(&self, strictness: Strictness) -> Result<(), String> {
        let factor = match strictness {
            Strictness::Input => 1,
            Strictness::Received => 2,
        };
        if self.schema_version != REPORT_SCHEMA_VERSION {
            return Err(format!(
                "schema_version is {}; this build accepts {REPORT_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.title.trim().is_empty() {
            return Err("title is empty".to_owned());
        }
        check_text("title", &self.title, MAX_TITLE_CHARS * factor, false)?;
        check_text(
            "description",
            &self.description,
            MAX_DESCRIPTION_CHARS * factor,
            true,
        )?;
        if let Some(code) = &self.error_code
            && !is_error_code(code)
        {
            return Err("error_code is not a TUR error code (TUR-FAMILY-...-NNN)".to_owned());
        }
        if !COMPONENTS.contains(&self.component.as_str()) {
            return Err(format!(
                "component is not one of: {}",
                COMPONENTS.join(", ")
            ));
        }
        if let Some(version) = &self.affected_version
            && !is_version(version)
        {
            return Err(
                "affected_version is not a version (letters, digits, . + _ ~ -; at most 64)"
                    .to_owned(),
            );
        }
        if let Some(evidence) = &self.boot_health {
            if !NOTICE_KINDS.contains(&evidence.notice_kind.as_str()) {
                return Err("boot_health.notice_kind is not a boot health notice kind".to_owned());
            }
            if let Some(code) = &evidence.error_code
                && !is_error_code(code)
            {
                return Err("boot_health.error_code is not a TUR error code".to_owned());
            }
            check_text(
                "boot_health.text",
                &evidence.text,
                MAX_NOTICE_CHARS * factor,
                true,
            )?;
            if let Some(check) = &evidence.failed_check
                && !is_check_id(check)
            {
                return Err("boot_health.failed_check is not a check identifier".to_owned());
            }
            if let Some(version) = &evidence.rejected_version
                && !is_version(version)
            {
                return Err("boot_health.rejected_version is not a version".to_owned());
            }
        }
        Ok(())
    }
}

fn check_text(field: &str, text: &str, max_chars: usize, multi_line: bool) -> Result<(), String> {
    if text.chars().count() > max_chars {
        return Err(format!("{field} is longer than {max_chars} characters"));
    }
    let allowed = |c: char| multi_line && (c == '\n' || c == '\t');
    if text.chars().any(|c| c.is_control() && !allowed(c)) {
        return Err(format!("{field} has a control character"));
    }
    Ok(())
}

/// Whether `code` is a valid TUR error code of at most 64 characters.
pub fn is_error_code(code: &str) -> bool {
    code.len() <= 64 && code.parse::<ErrorCode>().is_ok()
}

/// Whether `value` looks like a TUR version or build identifier: 1 to 64
/// characters, starting with a letter or digit, then letters, digits and
/// `. + _ ~ -`.
pub fn is_version(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+_~-".contains(byte))
}

/// Whether `value` is a health check identifier such as `R3` or `X:name`.
fn is_check_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":_.-".contains(&byte))
}

/// The duplicate signature of a report: error code, component and version,
/// hashed. Reports with the same signature are grouped on one issue.
///
/// ```
/// let a = tur_diag::report::signature(Some("TUR-BOOT-GATE-003"), "update", "0.2.0");
/// assert_eq!(a.len(), 16);
/// assert_eq!(a, tur_diag::report::signature(Some("TUR-BOOT-GATE-003"), "update", "0.2.0"));
/// assert_ne!(a, tur_diag::report::signature(None, "update", "0.2.0"));
/// ```
pub fn signature(error_code: Option<&str>, component: &str, version: &str) -> String {
    let code = error_code.unwrap_or("none");
    let text = format!("tur-report-signature-1\n{code}\n{component}\n{version}");
    sha256_hex(text.as_bytes())[..16].to_owned()
}

/// The version a report is about: the one it names, else the version the
/// bundle's manifest records, else `unknown`.
pub fn affected_version(report: &ReportRequest, manifest: &Manifest) -> String {
    report
        .affected_version
        .clone()
        .or_else(|| manifest.tur.version.clone().filter(|v| is_version(v)))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The collectors of a report: those of `tur-diag collect`, with a smaller
/// journal.
pub fn report_collectors() -> Vec<Collector> {
    COLLECTORS
        .iter()
        .map(|collector| {
            let args = match collector.bundle_path().as_str() {
                "boot/current-journal.jsonl" => Some(REPORT_CURRENT_JOURNAL),
                "boot/previous-journal.jsonl" => Some(REPORT_PREVIOUS_JOURNAL),
                _ => None,
            };
            match (args, collector.source) {
                (Some(args), Source::Command { program, .. }) => Collector {
                    source: Source::Command { program, args },
                    ..*collector
                },
                _ => *collector,
            }
        })
        .collect()
}

/// A report that has been prepared in memory.
#[derive(Debug, Clone)]
pub struct PreparedReport {
    /// The bundle, ready to be written to a file or sent.
    pub archive: Vec<u8>,
    /// What [`inspect_archive`] found in it.
    pub inspected: InspectedReport,
    /// The redaction report inside the bundle.
    pub redaction: RedactionReport,
}

/// A bundle that has been checked as a problem report.
#[derive(Debug, Clone)]
pub struct InspectedReport {
    /// The bundle's manifest.
    pub manifest: Manifest,
    /// `report.json` as it stands in the bundle, after redaction.
    pub report: ReportRequest,
    /// The size of the bundle as sent, in bytes.
    pub bytes: u64,
    /// The total size of its members, in bytes.
    pub unpacked_bytes: u64,
    /// The bundle's SHA-256.
    pub sha256: String,
    /// The members, as verified.
    pub members: Vec<Member>,
}

/// Reads `report.json` from verified members, and checks it as received.
pub fn read_report(members: &[Member]) -> Result<ReportRequest, String> {
    let Some((_, data)) = members.iter().find(|(path, _)| path == REPORT_PATH) else {
        return Err(format!("{REPORT_PATH} is not in the bundle"));
    };
    let report: ReportRequest = serde_json::from_slice(data)
        .map_err(|error| format!("{REPORT_PATH} is not a report: {}", error_category(&error)))?;
    report
        .validate(Strictness::Received)
        .map_err(|problem| format!("{REPORT_PATH}: {problem}"))?;
    Ok(report)
}

/// The kind of a JSON error without the text near it, which could quote a
/// value.
fn error_category(error: &serde_json::Error) -> String {
    let category = match error.classify() {
        serde_json::error::Category::Io => "unreadable",
        serde_json::error::Category::Syntax => "not valid JSON",
        serde_json::error::Category::Data => "a field is missing, unknown or of the wrong type",
        serde_json::error::Category::Eof => "cut short",
    };
    format!(
        "{category} (line {}, column {})",
        error.line(),
        error.column()
    )
}

/// Checks that `archive` is a problem report a receiver will accept: within
/// the size limits, verifying as a bundle, made with the `default` profile,
/// and carrying a valid `report.json`.
pub fn inspect_archive(archive: &[u8]) -> Result<InspectedReport, DiagError> {
    let bytes = u64::try_from(archive.len()).unwrap_or(u64::MAX);
    if bytes > MAX_UPLOAD_BYTES {
        return Err(DiagError::new(
            ErrorKind::ReportTooLarge,
            format!("the bundle is {bytes} bytes; the limit is {MAX_UPLOAD_BYTES}"),
        ));
    }
    let (verified, members) = verify_members(archive, MAX_UNPACKED_BYTES).map_err(|error| {
        if error.kind() == ErrorKind::BundleTooLarge {
            DiagError::new(ErrorKind::ReportTooLarge, error.detail().to_owned())
        } else {
            DiagError::new(
                ErrorKind::ReportNotPrepared,
                format!("{}: {}", error.code(), error.detail()),
            )
        }
    })?;
    if verified.manifest.redaction_profile != Profile::Default {
        return Err(DiagError::new(
            ErrorKind::ReportNotPrepared,
            "the bundle was made with the technical profile, which keeps serial numbers, MAC \
             addresses and disk UUIDs; reports use the default profile",
        ));
    }
    let report = read_report(&members)
        .map_err(|problem| DiagError::new(ErrorKind::ReportNotPrepared, problem))?;
    let unpacked_bytes = members
        .iter()
        .map(|(_, data)| u64::try_from(data.len()).unwrap_or(u64::MAX))
        .sum();
    Ok(InspectedReport {
        manifest: verified.manifest,
        report,
        bytes,
        unpacked_bytes,
        sha256: sha256_hex(archive),
        members,
    })
}

/// Collects a bundle for `request` and checks it as a receiver would.
///
/// The bundle always uses the `default` redaction profile. `request` is
/// checked first, with the limits for input.
pub fn prepare(
    host: &dyn Host,
    request: &ReportRequest,
    since_boot: bool,
    created_epoch_seconds: u64,
    known: KnownIdentifiers,
) -> Result<PreparedReport, DiagError> {
    request
        .validate(Strictness::Input)
        .map_err(|problem| DiagError::new(ErrorKind::ReportInvalid, problem))?;
    let collectors = report_collectors();
    let collect = CollectRequest {
        profile: Profile::Default,
        since_boot,
        created_epoch_seconds,
        collectors: &collectors,
        known,
        max_bytes: REPORT_COLLECTOR_MAX_BYTES,
    };
    // One line, so that it is redacted as one JSON document with every
    // string decoded first (Format::JsonLines).
    let line = serde_json::to_string(request)
        .map_err(|error| DiagError::new(ErrorKind::Internal, error.to_string()))?;
    let extras = [ExtraDocument {
        path: REPORT_PATH.to_owned(),
        format: Format::JsonLines,
        text: format!("{line}\n"),
    }];
    let built = build_bundle_with(host, &collect, &extras)?;
    let inspected = inspect_archive(&built.archive)?;
    Ok(PreparedReport {
        archive: built.archive,
        inspected,
        redaction: built.report,
    })
}

// ---------------------------------------------------------------- preview

/// One file of the bundle, in a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewFile {
    /// Path inside the bundle.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
}

/// One item that was not collected, in a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewMissing {
    /// The path the item would have had.
    pub path: String,
    /// Why it is absent or incomplete.
    pub reason: String,
}

/// The bundle part of a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewBundle {
    /// Size as sent, in bytes.
    pub bytes: u64,
    /// Total size of the files, in bytes.
    pub unpacked_bytes: u64,
    /// SHA-256 of the bundle.
    pub sha256: String,
    /// Always `default` for a report.
    pub redaction_profile: Profile,
    /// Every file in the bundle, with its size.
    pub files: Vec<PreviewFile>,
    /// What could not be collected.
    pub missing: Vec<PreviewMissing>,
    /// How many identifiers were replaced, per category.
    pub identifiers_replaced: BTreeMap<String, u64>,
}

/// What `tur-diag report prepare` shows: exactly what would be sent.
/// The format is `schemas/report-preview.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preview {
    /// Version of this document's format.
    pub schema_version: u32,
    /// Always `false`: preparing never sends.
    pub sent: bool,
    /// The configured receiver, or `null` when none is configured.
    pub receiver: Option<String>,
    /// Where the prepared bundle was saved, or `null` for a preview only.
    pub prepared_file: Option<String>,
    /// `report.json` exactly as it is in the bundle.
    pub report: ReportRequest,
    /// The bundle that goes with it.
    pub bundle: PreviewBundle,
    /// What happens next, in words.
    pub notice: String,
}

/// The notice every preview carries.
pub const PREVIEW_NOTICE: &str = "Nothing has been sent. The report is sent only when you \
     confirm it, to TUR's configured receiver. The receiver removes personal data again, files a \
     short summary for TUR's developers in a private tracker, keeps the full bundle encrypted for \
     90 days and then deletes it.";

/// Builds the preview of a prepared report.
pub fn preview(
    prepared: &PreparedReport,
    receiver: Option<&str>,
    prepared_file: Option<&Path>,
) -> Preview {
    let inspected = &prepared.inspected;
    let files = inspected
        .members
        .iter()
        .map(|(path, data)| PreviewFile {
            path: path.clone(),
            size: u64::try_from(data.len()).unwrap_or(u64::MAX),
        })
        .collect();
    let missing = inspected
        .manifest
        .missing
        .iter()
        .map(|entry| PreviewMissing {
            path: entry.path.clone(),
            reason: entry.reason.as_str().to_owned(),
        })
        .collect();
    let identifiers_replaced = prepared
        .redaction
        .categories
        .iter()
        .map(|(name, category)| (name.clone(), category.replacements))
        .collect();
    Preview {
        schema_version: 1,
        sent: false,
        receiver: receiver.map(str::to_owned),
        prepared_file: prepared_file.map(|path| path.display().to_string()),
        report: inspected.report.clone(),
        bundle: PreviewBundle {
            bytes: inspected.bytes,
            unpacked_bytes: inspected.unpacked_bytes,
            sha256: inspected.sha256.clone(),
            redaction_profile: inspected.manifest.redaction_profile,
            files,
            missing,
            identifiers_replaced,
        },
        notice: PREVIEW_NOTICE.to_owned(),
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>()
}

/// Renders a preview as text for a person.
pub fn render_preview(preview: &Preview) -> String {
    let report = &preview.report;
    let bundle = &preview.bundle;
    let mut out = String::new();
    out.push_str("Problem report: prepared, NOT sent\n\n");
    out.push_str(&format!("Title:        {}\n", report.title));
    out.push_str(&format!("Kind:         {}\n", report.kind.as_str()));
    out.push_str(&format!("Component:    {}\n", report.component));
    out.push_str(&format!(
        "Error code:   {}\n",
        report.error_code.as_deref().unwrap_or("none")
    ));
    if let Some(version) = &report.affected_version {
        out.push_str(&format!("Version:      {version}\n"));
    }
    out.push_str(&format!("Prepared by:  {}\n", report.prepared_by.as_str()));
    if !report.description.is_empty() {
        out.push_str("Description:\n");
        out.push_str(&indent(&report.description));
    }
    if let Some(evidence) = &report.boot_health {
        out.push_str(&format!(
            "From boot health: notice '{}', code {}\n",
            evidence.notice_kind,
            evidence.error_code.as_deref().unwrap_or("none")
        ));
        out.push_str(&indent(&evidence.text));
    }
    out.push_str(&format!(
        "\nThe diagnostic bundle that goes with it: {} bytes ({} bytes unpacked), {} files, \
         redaction profile {}\n",
        bundle.bytes,
        bundle.unpacked_bytes,
        bundle.files.len(),
        bundle.redaction_profile.as_str()
    ));
    for file in &bundle.files {
        out.push_str(&format!("    {:<40} {:>10} bytes\n", file.path, file.size));
    }
    if !bundle.missing.is_empty() {
        out.push_str(&format!(
            "Not collected: {} items (reasons are in {MANIFEST_PATH})\n",
            bundle.missing.len()
        ));
    }
    let replaced: Vec<String> = bundle
        .identifiers_replaced
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(name, count)| format!("{name} {count}"))
        .collect();
    if replaced.is_empty() {
        out.push_str("Identifiers replaced: none recognised\n");
    } else {
        out.push_str(&format!(
            "Identifiers replaced ({REDACTION_REPORT_PATH}): {}\n",
            replaced.join(", ")
        ));
    }
    out.push('\n');
    match &preview.receiver {
        Some(receiver) => out.push_str(&format!("Receiver:     {receiver}\n")),
        None => out.push_str("Receiver:     none configured, so this report cannot be sent yet\n"),
    }
    match &preview.prepared_file {
        Some(file) => {
            out.push_str(&format!("Saved as:     {file}\n"));
            out.push_str(&format!(
                "Read any file of it with:  tar --zstd -xOf {file} {REPORT_PATH}\n"
            ));
            if let Some(receiver) = &preview.receiver {
                out.push_str(&format!(
                    "Send it with:              tur-diag report send --to {receiver} {file}\n"
                ));
            }
        }
        None => out.push_str("Preview only: nothing was saved.\n"),
    }
    out.push('\n');
    out.push_str(&preview.notice);
    out.push('\n');
    out
}

// ----------------------------------------------------------- boot health

/// Runs `tur-boot-health status --json` and returns what it printed.
pub fn read_boot_health(host: &dyn Host) -> Result<String, DiagError> {
    let limits = Limits {
        timeout: Duration::from_secs(20),
        max_bytes: 1024 * 1024,
    };
    let nothing = |detail: String| DiagError::new(ErrorKind::ReportNothingFromBootHealth, detail);
    let output = host
        .run("tur-boot-health", &["status", "--json"], limits)
        .map_err(|error| nothing(format!("tur-boot-health could not be run: {error:?}")))?;
    if !output.success() {
        return Err(nothing(format!(
            "tur-boot-health status exited with {:?}",
            output.exit_code
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| nothing("tur-boot-health printed text that is not UTF-8".to_owned()))
}

/// Turns boot health's status (`tur-boot-health status --json`) into a
/// report proposal, from its pending notice.
///
/// A `fallback` (an update failed and the previous version was restored),
/// `not-applied` or `refused` notice is a failed update; `unhealthy` is a
/// boot problem. Unknown members are ignored, as the status format asks of
/// its readers.
pub fn request_from_boot_health(
    status_json: &str,
    prepared_by: PreparedBy,
) -> Result<ReportRequest, DiagError> {
    let nothing =
        |detail: &str| DiagError::new(ErrorKind::ReportNothingFromBootHealth, detail.to_owned());
    let status: Value = serde_json::from_str(status_json)
        .map_err(|_| nothing("tur-boot-health status --json did not print JSON"))?;
    let notice = status
        .get("notice")
        .filter(|notice| notice.is_object())
        .ok_or_else(|| nothing("boot health has no pending notice"))?;
    let notice_kind = notice
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| NOTICE_KINDS.contains(kind))
        .ok_or_else(|| nothing("the notice has no known kind"))?;
    let text: String = notice
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(MAX_NOTICE_CHARS)
        .collect();
    let error_code = notice
        .get("error_code")
        .and_then(Value::as_str)
        .filter(|code| is_error_code(code))
        .map(str::to_owned);
    let failed_check = notice
        .get("details")
        .and_then(|details| details.get("failed_check"))
        .and_then(Value::as_str)
        .filter(|check| is_check_id(check))
        .map(str::to_owned);
    let event_id = notice.get("event_id").and_then(Value::as_str);
    let rejected_version = status
        .get("rejected")
        .and_then(|rejected| rejected.get("images"))
        .and_then(Value::as_array)
        .and_then(|images| {
            images
                .iter()
                .find(|image| image.get("event_id").and_then(Value::as_str) == event_id)
        })
        .and_then(|image| image.get("version"))
        .and_then(Value::as_str)
        .filter(|version| is_version(version))
        .map(str::to_owned);

    let (kind, component, title) = match notice_kind {
        "fallback" => (
            ReportKind::FailedUpdate,
            "update",
            "An update failed to start and the previous version was restored",
        ),
        "not-applied" => (
            ReportKind::FailedUpdate,
            "update",
            "An update was not applied",
        ),
        "refused" => (
            ReportKind::FailedUpdate,
            "update",
            "An update could not be tried",
        ),
        _ => (
            ReportKind::BootProblem,
            "boot",
            "The system started with a failed health check",
        ),
    };
    let mut description = format!("Boot health recorded a '{notice_kind}' notice.");
    if let Some(check) = &failed_check {
        description.push_str(&format!(
            " The first required check that failed was {check}."
        ));
    }
    Ok(ReportRequest {
        schema_version: REPORT_SCHEMA_VERSION,
        kind,
        title: title.to_owned(),
        description,
        error_code: error_code.clone(),
        component: component.to_owned(),
        affected_version: rejected_version.clone(),
        prepared_by,
        boot_health: Some(BootHealthEvidence {
            notice_kind: notice_kind.to_owned(),
            error_code,
            text,
            failed_check,
            rejected_version,
        }),
    })
}

// --------------------------------------------------------- configuration

/// Where problem reports may be sent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportConfig {
    /// The receiver's base address, without a trailing `/`; `None` when no
    /// receiver is configured.
    pub receiver_url: Option<String>,
    /// The file it was read from, if any.
    pub source: Option<PathBuf>,
}

/// Reads the configuration: the file named, or else the first of
/// [`CONFIG_PATHS`] that exists. No file at all means no receiver.
pub fn load_config(explicit: Option<&Path>) -> Result<ReportConfig, DiagError> {
    let invalid = |path: &Path, detail: String| {
        DiagError::new(
            ErrorKind::ReportConfigInvalid,
            format!("{}: {detail}", path.display()),
        )
    };
    let candidates: Vec<PathBuf> = match explicit {
        Some(path) => vec![path.to_path_buf()],
        None => CONFIG_PATHS.iter().map(PathBuf::from).collect(),
    };
    for path in candidates {
        match fs::read_to_string(&path) {
            Ok(text) => {
                let receiver_url = parse_config(&text).map_err(|detail| invalid(&path, detail))?;
                return Ok(ReportConfig {
                    receiver_url,
                    source: Some(path),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && explicit.is_none() => {}
            Err(error) => return Err(invalid(&path, error.to_string())),
        }
    }
    Ok(ReportConfig::default())
}

/// Parses `KEY=VALUE` lines. Blank lines and lines starting with `#` are
/// ignored; `RECEIVER_URL` is the only key.
pub fn parse_config(text: &str) -> Result<Option<String>, String> {
    let mut receiver = None;
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {number} is not KEY=VALUE"));
        };
        match key.trim() {
            "RECEIVER_URL" => {
                let value = value.trim().trim_matches('"');
                receiver = if value.is_empty() {
                    None
                } else {
                    Some(
                        normalise_receiver_url(value)
                            .map_err(|problem| format!("line {number}: RECEIVER_URL {problem}"))?,
                    )
                };
            }
            other => return Err(format!("line {number}: unknown key {other}")),
        }
    }
    Ok(receiver)
}

/// Checks a receiver address and returns it without a trailing `/`.
///
/// It must be `https://host[:port][/path]`, without a user name, query or
/// fragment. `http://` is accepted only for this computer itself
/// (`127.0.0.1`, `[::1]`, `localhost`), for tests.
pub fn normalise_receiver_url(url: &str) -> Result<String, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| "is not an address".to_owned())?;
    if url.chars().any(|c| c.is_control() || c.is_whitespace())
        || url.contains(['?', '#', '@', '\\'])
    {
        return Err("has spaces, a user name, a query or a fragment".to_owned());
    }
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let host = match authority.strip_prefix('[') {
        Some(inner) => {
            let (address, after) = inner
                .split_once(']')
                .ok_or_else(|| "has an unclosed '['".to_owned())?;
            if !(after.is_empty() || after.starts_with(':')) {
                return Err("has a malformed host".to_owned());
            }
            format!("[{address}]")
        }
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
            .to_owned(),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-[]:".contains(&byte))
    {
        return Err("has no valid host".to_owned());
    }
    if let Some((_, port)) = authority.rsplit_once(':')
        && !authority.ends_with(']')
        && (port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("has a port that is not a number".to_owned());
    }
    let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]");
    match scheme {
        "https" => {}
        "http" if loopback => {}
        "http" => return Err("must use https".to_owned()),
        _ => return Err("must start with https://".to_owned()),
    }
    Ok(format!(
        "{scheme}://{authority}{}",
        path.trim_end_matches('/')
    ))
}

// ------------------------------------------------------------ local state

/// The directory for prepared and sent reports: `$XDG_STATE_HOME/tur/reports`,
/// or `$HOME/.local/state/tur/reports`. Relative values are ignored.
pub fn default_state_dir() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute("XDG_STATE_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
        .map(|base| base.join("tur/reports"))
}

/// The subdirectory for prepared reports.
pub const PREPARED_DIR: &str = "prepared";
/// The subdirectory for records of sent reports.
pub const SENT_DIR: &str = "sent";

fn records_error(path: &Path, error: impl std::fmt::Display) -> DiagError {
    DiagError::new(
        ErrorKind::ReportRecordsUnavailable,
        format!("{}: {error}", path.display()),
    )
}

/// Creates `directory` and its parents, readable only by its owner.
pub fn ensure_private_dir(directory: &Path) -> Result<(), DiagError> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|error| records_error(directory, error))
}

/// The file name for a prepared report.
pub fn prepared_file_name(prepared: &PreparedReport) -> String {
    let created: String = prepared
        .inspected
        .manifest
        .created_utc
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let digest = &prepared.inspected.sha256[..12];
    format!("report-{created}-{digest}.tar.zst")
}

/// A record of a sent report, kept in `sent/<report-id>.json`.
/// The format is `schemas/report-record.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentRecord {
    /// Version of this document's format.
    pub schema_version: u32,
    /// The receiver's identifier for the report.
    pub report_id: String,
    /// When it was sent, as `YYYY-MM-DDTHH:MM:SSZ`.
    pub sent_utc: String,
    /// The receiver it was sent to.
    pub receiver: String,
    /// Where its state can be read. Holds the status token: kept in this
    /// file, which only its owner can read, and never listed.
    pub status_url: String,
    /// The title it was sent with.
    pub title: String,
    /// Its kind.
    pub kind: ReportKind,
    /// Its component.
    pub component: String,
    /// Its error code.
    pub error_code: Option<String>,
    /// SHA-256 of the bundle that was sent.
    pub bundle_sha256: String,
    /// Size of the bundle that was sent.
    pub bundle_bytes: u64,
    /// The last state the receiver gave: `received`, `filed` or `fixed`.
    pub state: String,
    /// The version that fixes it, once the state is `fixed`.
    pub fixed_in: Option<String>,
    /// When the state was last read, as `YYYY-MM-DDTHH:MM:SSZ`.
    pub state_checked_utc: Option<String>,
}

/// Whether `id` has the form of a receiver's report identifier.
pub fn is_report_id(id: &str) -> bool {
    id.len() == 18
        && id.starts_with("R-")
        && id[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Writes `record` to `sent/<report-id>.json`, readable only by its owner,
/// replacing an earlier record of the same report.
pub fn write_record(state_dir: &Path, record: &SentRecord) -> Result<PathBuf, DiagError> {
    if !is_report_id(&record.report_id) {
        return Err(DiagError::new(
            ErrorKind::Internal,
            "a record with a malformed report identifier",
        ));
    }
    let directory = state_dir.join(SENT_DIR);
    ensure_private_dir(&directory)?;
    let path = directory.join(format!("{}.json", record.report_id));
    let temporary = directory.join(format!(".{}.json.new", record.report_id));
    let mut text = serde_json::to_vec_pretty(record)
        .map_err(|error| DiagError::new(ErrorKind::Internal, error.to_string()))?;
    text.push(b'\n');
    let _ = fs::remove_file(&temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| file.write_all(&text).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temporary, &path));
    if let Err(error) = written {
        let _ = fs::remove_file(temporary);
        return Err(records_error(&path, error));
    }
    Ok(path)
}

/// Reads every record in `sent/`, newest first. A file that cannot be
/// parsed is skipped and counted.
pub fn read_records(state_dir: &Path) -> Result<(Vec<SentRecord>, usize), DiagError> {
    let directory = state_dir.join(SENT_DIR);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => return Err(records_error(&directory, error)),
    };
    let mut records = Vec::new();
    let mut unreadable = 0;
    for entry in entries {
        let path = entry
            .map_err(|error| records_error(&directory, error))?
            .path();
        let is_record = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".json") && !name.starts_with('.'));
        if !is_record {
            continue;
        }
        match fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice::<SentRecord>(&data).ok())
        {
            Some(record) => records.push(record),
            None => unreadable += 1,
        }
    }
    records.sort_by(|a, b| {
        b.sent_utc
            .cmp(&a.sent_utc)
            .then_with(|| a.report_id.cmp(&b.report_id))
    });
    Ok((records, unreadable))
}

/// A sent report as `tur-diag report list` shows it: everything in the
/// record except where its state is read, which holds the status token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListedReport {
    /// The receiver's identifier.
    pub report_id: String,
    /// When it was sent.
    pub sent_utc: String,
    /// The receiver.
    pub receiver: String,
    /// The title.
    pub title: String,
    /// The kind.
    pub kind: ReportKind,
    /// The component.
    pub component: String,
    /// The error code.
    pub error_code: Option<String>,
    /// The last known state.
    pub state: String,
    /// The version that fixes it.
    pub fixed_in: Option<String>,
    /// When the state was last read.
    pub state_checked_utc: Option<String>,
}

impl From<&SentRecord> for ListedReport {
    fn from(record: &SentRecord) -> Self {
        ListedReport {
            report_id: record.report_id.clone(),
            sent_utc: record.sent_utc.clone(),
            receiver: record.receiver.clone(),
            title: record.title.clone(),
            kind: record.kind,
            component: record.component.clone(),
            error_code: record.error_code.clone(),
            state: record.state.clone(),
            fixed_in: record.fixed_in.clone(),
            state_checked_utc: record.state_checked_utc.clone(),
        }
    }
}

/// A prepared report that has not been sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListedPrepared {
    /// The file name in `prepared/`. The directory is not shown: it is in
    /// the home directory and names the account.
    pub name: String,
    /// Its size in bytes.
    pub bytes: u64,
}

/// What `tur-diag report list` shows. The format is the `list` definition
/// of `schemas/report-record.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportList {
    /// Version of this document's format.
    pub schema_version: u32,
    /// Whether the directory of reports exists and could be read. It is
    /// `false` where no report was ever prepared, and inside P@x's read-only
    /// sandbox, which hides the home directory.
    pub directory_found: bool,
    /// Sent reports, newest first.
    pub sent: Vec<ListedReport>,
    /// Prepared reports that have not been sent, by name.
    pub prepared: Vec<ListedPrepared>,
    /// Record files that could not be read.
    pub unreadable_records: usize,
}

/// Lists sent and prepared reports under `state_dir`.
pub fn list_reports(state_dir: Option<&Path>) -> Result<ReportList, DiagError> {
    let Some(state_dir) = state_dir.filter(|dir| dir.is_dir()) else {
        return Ok(ReportList {
            schema_version: 1,
            directory_found: false,
            sent: Vec::new(),
            prepared: Vec::new(),
            unreadable_records: 0,
        });
    };
    let (records, unreadable_records) = read_records(state_dir)?;
    let mut prepared = Vec::new();
    let directory = state_dir.join(PREPARED_DIR);
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".tar.zst") || name.starts_with('.') {
                    continue;
                }
                let bytes = entry.metadata().map_or(0, |metadata| metadata.len());
                prepared.push(ListedPrepared { name, bytes });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(records_error(&directory, error)),
    }
    prepared.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(ReportList {
        schema_version: 1,
        directory_found: true,
        sent: records.iter().map(ListedReport::from).collect(),
        prepared,
        unreadable_records,
    })
}

/// Renders a list for a person.
pub fn render_list(list: &ReportList) -> String {
    let mut out = String::new();
    if !list.directory_found {
        out.push_str("No problem reports have been prepared or sent from this account.\n");
        return out;
    }
    if list.sent.is_empty() {
        out.push_str("Sent reports: none\n");
    } else {
        out.push_str("Sent reports, newest first:\n");
        for report in &list.sent {
            let state = match &report.fixed_in {
                Some(version) => format!("fixed in {version}"),
                None => report.state.clone(),
            };
            out.push_str(&format!(
                "  {}  {}  {:<10} {}  ({})\n",
                report.report_id, report.sent_utc, report.component, report.title, state
            ));
        }
    }
    if !list.prepared.is_empty() {
        out.push_str("Prepared, not sent:\n");
        for prepared in &list.prepared {
            out.push_str(&format!("  {}  {} bytes\n", prepared.name, prepared.bytes));
        }
    }
    if list.unreadable_records > 0 {
        out.push_str(&format!(
            "{} record files could not be read.\n",
            list.unreadable_records
        ));
    }
    out
}

// ---------------------------------------------------------------- sending

/// The receiver's answer to an upload. See `schemas/report-receiver.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// Version of the receiver's answer format.
    pub schema_version: u32,
    /// The report's identifier.
    pub report_id: String,
    /// The token that reads the report's state.
    pub status_token: String,
    /// Where the state can be read.
    pub status_url: String,
    /// The state: `received`.
    pub state: String,
    /// How long the receiver keeps the bundle.
    pub retention_days: u32,
}

/// The state of a sent report, as the receiver gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportState {
    /// Version of the receiver's answer format.
    pub schema_version: u32,
    /// `received`, `filed` or `fixed`.
    pub state: String,
    /// The version that fixes it, when the state is `fixed`.
    pub fixed_in: Option<String>,
}

/// The receiver's answer to a request it refused.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Refusal {
    code: String,
    message: String,
}

/// An HTTP answer, as curl gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    /// The HTTP status code.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

/// How long curl may take for an upload.
const UPLOAD_SECONDS: u64 = 300;

/// Runs curl with `args` and splits its output into body and status.
fn curl(host: &dyn Host, url: &str, args: &[String]) -> Result<HttpReply, DiagError> {
    let unreachable = |detail: String| DiagError::new(ErrorKind::ReportUnreachable, detail);
    let protocol = if url.starts_with("https://") {
        "=https"
    } else {
        "=http"
    };
    let user_agent = format!("tur-diag/{}", crate::manifest::COLLECTOR_VERSION);
    let max_time = UPLOAD_SECONDS.to_string();
    let fixed: [&str; 16] = [
        "--silent",
        "--show-error",
        "--proto",
        protocol,
        "--max-redirs",
        "0",
        "--connect-timeout",
        "20",
        "--max-time",
        max_time.as_str(),
        "--user-agent",
        user_agent.as_str(),
        "--header",
        "Accept: application/json",
        "--write-out",
        "\\n%{http_code}",
    ];
    let mut line: Vec<String> = fixed.iter().map(|text| (*text).to_owned()).collect();
    line.extend(args.iter().cloned());
    // The address was checked by normalise_receiver_url (or came from a
    // record written after such a check): it starts with http(s)://, so
    // curl cannot read it as an option.
    line.push(url.to_owned());
    let refs: Vec<&str> = line.iter().map(String::as_str).collect();
    let limits = Limits {
        timeout: Duration::from_secs(UPLOAD_SECONDS + 30),
        max_bytes: 1024 * 1024,
    };
    let output = host
        .run("curl", &refs, limits)
        .map_err(|error| match error {
            RunError::NotFound => unreachable("curl is not installed".to_owned()),
            RunError::TimedOut => unreachable("the receiver did not answer in time".to_owned()),
            other => unreachable(format!("curl could not be run: {other:?}")),
        })?;
    if !output.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first: String = stderr
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        return Err(unreachable(format!(
            "curl exited with {:?}: {first}",
            output.exit_code
        )));
    }
    let text = output.stdout;
    let split = text.iter().rposition(|byte| *byte == b'\n');
    let (body, code) = match split {
        Some(index) => (text[..index].to_vec(), &text[index + 1..]),
        None => (Vec::new(), &text[..]),
    };
    let status = std::str::from_utf8(code)
        .ok()
        .and_then(|code| code.trim().parse::<u16>().ok())
        .filter(|status| *status >= 100)
        .ok_or_else(|| unreachable("the receiver gave no HTTP status".to_owned()))?;
    Ok(HttpReply { status, body })
}

/// The error for an answer that is not a success.
fn refused_or_unreachable(reply: &HttpReply) -> DiagError {
    match serde_json::from_slice::<Refusal>(&reply.body) {
        Ok(refusal) if refusal.code.starts_with("TUR-") => {
            let message: String = refusal.message.chars().take(300).collect();
            DiagError::new(
                ErrorKind::ReportRefused,
                format!("HTTP {} {}: {message}", reply.status, refusal.code),
            )
        }
        _ => DiagError::new(
            ErrorKind::ReportUnreachable,
            format!(
                "the receiver answered HTTP {} without a TUR code",
                reply.status
            ),
        ),
    }
}

/// Sends the prepared report in `file` to `receiver` and returns the
/// receipt. The caller has checked the file with [`inspect_archive`] and
/// obtained the person's confirmation.
pub fn send_prepared(host: &dyn Host, receiver: &str, file: &Path) -> Result<Receipt, DiagError> {
    let url = format!("{receiver}{UPLOAD_PATH}");
    let data = format!("@{}", file.display());
    let args = vec![
        "--request".to_owned(),
        "POST".to_owned(),
        "--header".to_owned(),
        format!("Content-Type: {CONTENT_TYPE}"),
        "--data-binary".to_owned(),
        data,
    ];
    let reply = curl(host, &url, &args)?;
    if !(200..300).contains(&reply.status) {
        return Err(refused_or_unreachable(&reply));
    }
    let not_a_receipt = |detail: &str| {
        DiagError::new(
            ErrorKind::ReportUnreachable,
            format!("the receiver's answer is not a receipt: {detail}"),
        )
    };
    let receipt: Receipt =
        serde_json::from_slice(&reply.body).map_err(|_| not_a_receipt("not the expected JSON"))?;
    if !is_report_id(&receipt.report_id) {
        return Err(not_a_receipt("malformed report identifier"));
    }
    let token_ok = receipt.status_token.len() == 64
        && receipt
            .status_token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !token_ok {
        return Err(not_a_receipt("malformed status token"));
    }
    let expected = format!("{receiver}{UPLOAD_PATH}/{}", receipt.status_token);
    if receipt.status_url != expected {
        return Err(not_a_receipt("the status address is not on the receiver"));
    }
    Ok(receipt)
}

/// Reads the state of a sent report from its receiver.
pub fn fetch_state(host: &dyn Host, record: &SentRecord) -> Result<ReportState, DiagError> {
    let expected = format!("{}{UPLOAD_PATH}/", record.receiver);
    if normalise_receiver_url(&record.receiver).as_deref() != Ok(record.receiver.as_str())
        || !record.status_url.starts_with(&expected)
    {
        return Err(DiagError::new(
            ErrorKind::ReportRecordsUnavailable,
            format!("the record of {} has an unusable address", record.report_id),
        ));
    }
    let reply = curl(host, &record.status_url, &[])?;
    if reply.status != 200 {
        return Err(refused_or_unreachable(&reply));
    }
    let state: ReportState = serde_json::from_slice(&reply.body).map_err(|_| {
        DiagError::new(
            ErrorKind::ReportUnreachable,
            "the receiver's answer is not a report state",
        )
    })?;
    if !matches!(state.state.as_str(), "received" | "filed" | "fixed") {
        return Err(DiagError::new(
            ErrorKind::ReportUnreachable,
            "the receiver gave an unknown state",
        ));
    }
    Ok(state)
}

#[cfg(test)]
mod tests;

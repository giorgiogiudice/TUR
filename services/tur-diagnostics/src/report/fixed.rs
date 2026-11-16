// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! "The problem you reported is fixed": the link from a sent report to the
//! update that fixes it, on this computer.
//!
//! When a report is sent, `tur-diag` keeps where it came from beside the
//! record ([`SentOrigin`], `origins/<report-id>.json`): its error code, its
//! duplicate signature, and the TUR version and build it was made on. The
//! record itself is not extended, so that an older `tur-diag` (after a
//! rollback) still reads every record.
//!
//! Each TUR image lists the codes a change in it fixes
//! (`/usr/share/tur/heal/fixes.json`). [`fixed_reports`] names every sent
//! report whose code that list names, made on another build than the one
//! running: the update that fixes it is installed. It also names every
//! report the receiver says is fixed, which covers reports without a code.
//!
//! For a fault of the self-healing catalogue the healing service goes
//! further: it checks the fault again after the update and says "fixed" only
//! when it is gone (self-healing.md, section 7). Such reports are marked
//! `rechecked_by_healing`, so that the one who tells the person can leave
//! them to the healing service's notice.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SentRecord, ensure_private_dir, is_report_id};
use crate::error::{DiagError, ErrorKind};
use crate::heal::{Fixes, Running};

/// The subdirectory for the origins of sent reports.
pub const ORIGINS_DIR: &str = "origins";

/// Where a sent report came from. Format: `schemas/report-origin.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentOrigin {
    /// Version of this document's format.
    pub schema_version: u32,
    /// The receiver's identifier for the report.
    pub report_id: String,
    /// The report's error code.
    pub error_code: Option<String>,
    /// The duplicate signature the receiver files it under.
    pub signature: String,
    /// The TUR version the report was made on, as its bundle records it.
    pub version: Option<String>,
    /// The TUR build the report was made on, as its bundle records it.
    pub build_id: Option<String>,
}

fn origins_error(path: &Path, error: impl std::fmt::Display) -> DiagError {
    DiagError::new(
        ErrorKind::ReportRecordsUnavailable,
        format!("{}: {error}", path.display()),
    )
}

/// Writes `origin` to `origins/<report-id>.json`, readable only by its owner.
pub fn write_origin(state_dir: &Path, origin: &SentOrigin) -> Result<PathBuf, DiagError> {
    if !is_report_id(&origin.report_id) {
        return Err(DiagError::new(
            ErrorKind::Internal,
            "an origin with a malformed report identifier",
        ));
    }
    let directory = state_dir.join(ORIGINS_DIR);
    ensure_private_dir(&directory)?;
    let path = directory.join(format!("{}.json", origin.report_id));
    let temporary = directory.join(format!(".{}.json.new", origin.report_id));
    let mut text = serde_json::to_vec_pretty(origin)
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
        return Err(origins_error(&path, error));
    }
    Ok(path)
}

/// Reads the origin of the report `report_id`, if one was kept.
pub fn read_origin(state_dir: &Path, report_id: &str) -> Option<SentOrigin> {
    if !is_report_id(report_id) {
        return None;
    }
    let path = state_dir
        .join(ORIGINS_DIR)
        .join(format!("{report_id}.json"));
    fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<SentOrigin>(&data).ok())
        .filter(|origin| origin.schema_version == 1 && origin.report_id == report_id)
}

/// How a report is known to be fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixedBy {
    /// The running image lists the report's code as fixed, and the report was
    /// made on another build.
    Update,
    /// The receiver said so (`tur-diag report status`).
    Receiver,
}

/// One sent report that is fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedReport {
    /// The receiver's identifier.
    pub report_id: String,
    /// The title it was sent with.
    pub title: String,
    /// Its error code.
    pub error_code: Option<String>,
    /// When it was sent.
    pub sent_utc: String,
    /// The version it was made on, where known.
    pub reported_on: Option<String>,
    /// The version that fixes it: the running one for an update, the
    /// receiver's word otherwise.
    pub fixed_in: Option<String>,
    /// How this is known.
    pub how: FixedBy,
    /// What the fix changed, from the image's list.
    pub summary: Option<String>,
    /// The change that fixed it, from the image's list.
    pub change: Option<String>,
    /// Whether the healing service checks the fault again and tells the
    /// person itself: the code is one of its catalogue.
    pub rechecked_by_healing: bool,
    /// The sentence for the person.
    pub text: String,
}

/// What `tur-diag report fixed` prints. Format: `schemas/report-fixed.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedList {
    /// Version of this document's format.
    pub schema_version: u32,
    /// The TUR version running.
    pub running_version: Option<String>,
    /// The TUR build running.
    pub running_build_id: Option<String>,
    /// The sent reports that are fixed, newest first.
    pub fixed: Vec<FixedReport>,
}

/// The sent reports that are fixed, given the records and their origins,
/// the running image's list of fixes and what is running. `catalogue_codes`
/// are the codes of the self-healing catalogue.
pub fn fixed_reports(
    records: &[SentRecord],
    origins: &[Option<SentOrigin>],
    fixes: &Fixes,
    running: &Running,
    catalogue_codes: &[String],
) -> FixedList {
    let mut fixed = Vec::new();
    for (record, origin) in records.iter().zip(origins) {
        let listed = record
            .error_code
            .as_deref()
            .and_then(|code| fixes.fixes.iter().find(|fix| fix.code == code));
        let other_build = match (origin, &running.build_id) {
            (Some(origin), Some(running_build)) => {
                origin.build_id.as_deref() != Some(running_build.as_str())
            }
            _ => false,
        };
        let reported_on = origin.as_ref().and_then(|origin| origin.version.clone());
        let rechecked_by_healing = record
            .error_code
            .as_ref()
            .is_some_and(|code| catalogue_codes.contains(code));
        let (how, fixed_in, summary, change) = match listed {
            Some(fix) if other_build => (
                FixedBy::Update,
                running.version.clone(),
                Some(fix.summary.clone()),
                fix.change.clone(),
            ),
            _ if record.state == "fixed" => {
                (FixedBy::Receiver, record.fixed_in.clone(), None, None)
            }
            _ => continue,
        };
        let version = fixed_in.as_deref().unwrap_or("a later version");
        let text = match how {
            FixedBy::Update => format!(
                "The problem you reported is fixed: \"{}\" ({}) is addressed by TUR {version}, \
                 which this computer now runs.",
                record.title, record.report_id
            ),
            FixedBy::Receiver => format!(
                "The problem you reported is fixed: TUR's developers fixed \"{}\" ({}) in TUR \
                 {version}.",
                record.title, record.report_id
            ),
        };
        fixed.push(FixedReport {
            report_id: record.report_id.clone(),
            title: record.title.clone(),
            error_code: record.error_code.clone(),
            sent_utc: record.sent_utc.clone(),
            reported_on,
            fixed_in,
            how,
            summary,
            change,
            rechecked_by_healing,
            text,
        });
    }
    FixedList {
        schema_version: 1,
        running_version: running.version.clone(),
        running_build_id: running.build_id.clone(),
        fixed,
    }
}

/// Renders the list for a person.
pub fn render_fixed(list: &FixedList) -> String {
    if list.fixed.is_empty() {
        return "None of the reports sent from this account is known to be fixed yet.\n"
            .to_owned();
    }
    let mut out = String::new();
    for report in &list.fixed {
        out.push_str(&report.text);
        out.push('\n');
        if let Some(summary) = &report.summary {
            out.push_str(&format!("  What changed: {summary}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heal::engine::Fix;
    use crate::report::ReportKind;

    const ID: &str = "R-0123456789abcdef";
    const OTHER: &str = "R-fedcba9876543210";

    fn record(id: &str, code: Option<&str>, state: &str) -> SentRecord {
        SentRecord {
            schema_version: 1,
            report_id: id.to_owned(),
            sent_utc: "2026-11-16T10:00:00Z".to_owned(),
            receiver: "https://reports.tur.example".to_owned(),
            status_url: "https://reports.tur.example/v1/reports/x".to_owned(),
            title: "No sound after resume".to_owned(),
            kind: ReportKind::Request,
            component: "audio".to_owned(),
            error_code: code.map(str::to_owned),
            bundle_sha256: "0".repeat(64),
            bundle_bytes: 1,
            state: state.to_owned(),
            fixed_in: (state == "fixed").then(|| "0.2.1".to_owned()),
            state_checked_utc: None,
        }
    }

    fn origin(id: &str, code: Option<&str>, build: &str) -> SentOrigin {
        SentOrigin {
            schema_version: 1,
            report_id: id.to_owned(),
            error_code: code.map(str::to_owned),
            signature: "0123456789abcdef".to_owned(),
            version: Some("0.2.0".to_owned()),
            build_id: Some(build.to_owned()),
        }
    }

    fn fixes(code: &str) -> Fixes {
        Fixes {
            schema_version: 1,
            about: String::new(),
            fixes: vec![Fix {
                code: code.to_owned(),
                summary: "The sound comes back after resume.".to_owned(),
                change: Some("giorgiogiudice/TUR#200".to_owned()),
            }],
        }
    }

    fn running(build: &str) -> Running {
        Running {
            build_id: Some(build.to_owned()),
            version: Some("0.3.0".to_owned()),
            boot_id: None,
        }
    }

    #[test]
    fn a_code_the_running_image_fixes_is_fixed_once_the_build_changed() {
        let records = [record(ID, Some("TUR-AUDIO-RESUME-001"), "filed")];
        let origins = [Some(origin(ID, Some("TUR-AUDIO-RESUME-001"), "b1"))];
        let fixes = fixes("TUR-AUDIO-RESUME-001");

        let same_build = fixed_reports(&records, &origins, &fixes, &running("b1"), &[]);
        assert!(same_build.fixed.is_empty(), "made on the build that lists it");

        let updated = fixed_reports(&records, &origins, &fixes, &running("b2"), &[]);
        assert_eq!(updated.fixed.len(), 1);
        let fixed = &updated.fixed[0];
        assert_eq!(fixed.how, FixedBy::Update);
        assert_eq!(fixed.fixed_in.as_deref(), Some("0.3.0"));
        assert_eq!(fixed.reported_on.as_deref(), Some("0.2.0"));
        assert!(!fixed.rechecked_by_healing);
        assert!(fixed.text.starts_with("The problem you reported is fixed"));
        assert!(fixed.text.contains(ID));
    }

    #[test]
    fn without_an_origin_only_the_receiver_can_say_fixed() {
        let records = [
            record(ID, Some("TUR-AUDIO-RESUME-001"), "filed"),
            record(OTHER, None, "fixed"),
        ];
        let origins = [None, None];
        let list = fixed_reports(
            &records,
            &origins,
            &fixes("TUR-AUDIO-RESUME-001"),
            &running("b2"),
            &[],
        );
        assert_eq!(list.fixed.len(), 1);
        assert_eq!(list.fixed[0].report_id, OTHER);
        assert_eq!(list.fixed[0].how, FixedBy::Receiver);
        assert_eq!(list.fixed[0].fixed_in.as_deref(), Some("0.2.1"));
    }

    #[test]
    fn a_catalogue_code_is_left_to_the_healing_service() {
        let code = "TUR-DIAG-CRASH-001";
        let list = fixed_reports(
            &[record(ID, Some(code), "filed")],
            &[Some(origin(ID, Some(code), "b1"))],
            &fixes(code),
            &running("b2"),
            &[code.to_owned()],
        );
        assert!(list.fixed[0].rechecked_by_healing);
    }

    #[test]
    fn origins_are_kept_privately_and_read_back() {
        let scratch =
            std::env::temp_dir().join(format!("tur-diag-origins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        let kept = origin(ID, Some("TUR-AUDIO-RESUME-001"), "b1");
        let path = write_origin(&scratch, &kept).expect("written");
        let mode = fs::metadata(&path).expect("metadata").permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        assert_eq!(read_origin(&scratch, ID), Some(kept));
        assert_eq!(read_origin(&scratch, OTHER), None);
        assert_eq!(read_origin(&scratch, "../x"), None);
        let _ = fs::remove_dir_all(&scratch);
    }
}

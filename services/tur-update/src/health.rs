// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What boot health knows, as far as the update manager needs it.
//!
//! `tur-boot-health` (ADR-0014) decides whether a start was good, counts the
//! attempts of a new version and makes a fallback permanent. The update
//! manager must respect what it recorded (boot-health-and-rollback.md,
//! section 5.10): an image that failed its trial is not staged again
//! automatically; nothing is staged where attempts cannot be counted; and
//! nothing is staged or reordered while a trial is in progress.
//!
//! This module reads the document `tur-boot-health status --json` prints
//! (`boot-health-status.schema.json`, schema version 1). It depends on no
//! code of that program: the document is the interface. Members it does not
//! use are passed over, as that schema asks of every reader.

use serde::{Deserialize, Serialize};

string_enum! {
    /// The health gate's verdict for the running boot.
    pub enum Verdict {
        /// Every check passed.
        Passed => "passed",
        /// Every required check passed; a non-blocking one did not.
        Degraded => "degraded",
        /// A required check failed.
        Failed => "failed",
    }
}

/// An image that failed its start-up trial on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RejectedImage {
    /// The manifest digest, `sha256:…`.
    pub digest: String,
    /// The reference it was fetched from.
    pub image: String,
    /// Its version label.
    pub version: String,
    /// The fallback event that rejected it.
    pub event_id: String,
}

/// What the user is to be told about an earlier start, until acknowledged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// `fallback`, `not-applied`, `refused` or `unhealthy`.
    pub kind: String,
    /// The text, owned by the code below.
    pub text: String,
    /// The `TUR-BOOT-*` code.
    pub error_code: String,
    /// Whether the user has dismissed it.
    pub acknowledged: bool,
}

/// The boot-health facts a decision uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootHealth {
    /// Boot attempts can be counted on this installation.
    pub counting_usable: bool,
    /// A new version is on trial: a trial record or a boot counter exists.
    pub trial_in_progress: bool,
    /// The gate's verdict for this boot. `None` while it has not finished.
    pub verdict: Option<Verdict>,
    /// The images that failed their trial.
    pub rejected: Vec<RejectedImage>,
    /// What the user is to be told, if anything is pending.
    pub notice: Option<Notice>,
}

impl BootHealth {
    /// Whether an image with this digest failed its trial here.
    pub fn is_rejected(&self, digest: &str) -> bool {
        self.rejected.iter().any(|image| image.digest == digest)
    }

    /// A boot-health state in which nothing holds an update back. For tests
    /// and examples.
    pub fn healthy() -> BootHealth {
        BootHealth {
            counting_usable: true,
            trial_in_progress: false,
            verdict: Some(Verdict::Passed),
            rejected: Vec::new(),
            notice: None,
        }
    }
}

#[derive(Deserialize)]
struct RawRejectedImage {
    #[serde(default)]
    digest: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    event_id: String,
}

#[derive(Deserialize)]
struct RawRejected {
    schema_version: u32,
    #[serde(default)]
    images: Vec<RawRejectedImage>,
}

#[derive(Deserialize)]
struct RawCounting {
    usable: bool,
    #[serde(default)]
    boot_counter: Option<String>,
}

#[derive(Deserialize)]
struct RawVerdict {
    result: String,
}

#[derive(Deserialize)]
struct RawNotice {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    acknowledged: bool,
}

#[derive(Deserialize)]
struct RawStatus {
    schema_version: u32,
    counting: RawCounting,
    #[serde(default)]
    verdict: Option<RawVerdict>,
    #[serde(default)]
    trial: Option<serde_json::Value>,
    rejected: RawRejected,
    #[serde(default)]
    notice: Option<RawNotice>,
}

/// The schema version of the boot-health documents this module reads.
pub const BOOT_HEALTH_SCHEMA_VERSION: u32 = 1;

fn rejected_images(raw: RawRejected) -> Result<Vec<RejectedImage>, String> {
    if raw.schema_version != BOOT_HEALTH_SCHEMA_VERSION {
        return Err(format!(
            "the rejected list has schema version {}, not {BOOT_HEALTH_SCHEMA_VERSION}",
            raw.schema_version
        ));
    }
    Ok(raw
        .images
        .into_iter()
        .filter(|image| !image.digest.is_empty())
        .map(|image| RejectedImage {
            digest: image.digest,
            image: image.image,
            version: image.version,
            event_id: image.event_id,
        })
        .collect())
}

/// Reads the state file `/var/lib/tur/boot-health/rejected.json`, which has
/// the shape of the `rejected` member of the status document.
///
/// # Errors
///
/// The text is not that document.
pub fn parse_rejected(text: &str) -> Result<Vec<RejectedImage>, String> {
    let raw: RawRejected = serde_json::from_str(text).map_err(|error| error.to_string())?;
    rejected_images(raw)
}

/// Reads the document `tur-boot-health status --json` prints.
///
/// # Errors
///
/// The text is not JSON, is not schema version 1, lacks the `counting` or
/// `rejected` member, or names a verdict this code does not know. Nothing is
/// guessed: an update is not staged on a document that could not be read.
pub fn parse_boot_health(text: &str) -> Result<BootHealth, String> {
    let raw: RawStatus = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if raw.schema_version != BOOT_HEALTH_SCHEMA_VERSION {
        return Err(format!(
            "schema version {}, not {BOOT_HEALTH_SCHEMA_VERSION}",
            raw.schema_version
        ));
    }
    let verdict = match raw.verdict {
        None => None,
        Some(verdict) => Some(
            Verdict::from_text(&verdict.result)
                .ok_or_else(|| format!("unknown verdict {:?}", verdict.result))?,
        ),
    };
    let trial_recorded = raw.trial.as_ref().is_some_and(|trial| !trial.is_null());
    Ok(BootHealth {
        counting_usable: raw.counting.usable,
        trial_in_progress: trial_recorded || raw.counting.boot_counter.is_some(),
        verdict,
        rejected: rejected_images(raw.rejected)?,
        notice: raw
            .notice
            .filter(|notice| !notice.acknowledged)
            .map(|notice| Notice {
                kind: notice.kind,
                text: notice.text,
                error_code: notice.error_code,
                acknowledged: notice.acknowledged,
            }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{"schema_version":1,
        "counting":{"usable":true,"contract":"1","layout":"ext4","environment_block":"valid",
                    "boot_counter":null},
        "verdict":{"result":"degraded"},"trial":null,
        "rejected":{"schema_version":1,"images":[]},"notice":null}"#;

    #[test]
    fn an_ordinary_boot_holds_nothing_back() {
        let health = parse_boot_health(MINIMAL).unwrap();
        assert!(health.counting_usable);
        assert!(!health.trial_in_progress);
        assert_eq!(health.verdict, Some(Verdict::Degraded));
        assert!(health.rejected.is_empty());
        assert!(health.notice.is_none());
        assert!(!health.is_rejected("sha256:aaa"));
    }

    #[test]
    fn a_trial_is_a_record_or_a_counter() {
        let with_counter = MINIMAL.replace(r#""boot_counter":null"#, r#""boot_counter":"2""#);
        assert!(parse_boot_health(&with_counter).unwrap().trial_in_progress);
        let with_record = MINIMAL.replace(r#""trial":null"#, r#""trial":{"phase":"trial"}"#);
        assert!(parse_boot_health(&with_record).unwrap().trial_in_progress);
    }

    #[test]
    fn the_gate_may_not_have_finished() {
        let pending = MINIMAL.replace(r#""verdict":{"result":"degraded"}"#, r#""verdict":null"#);
        assert_eq!(parse_boot_health(&pending).unwrap().verdict, None);
        let failed = MINIMAL.replace("degraded", "failed");
        assert_eq!(
            parse_boot_health(&failed).unwrap().verdict,
            Some(Verdict::Failed)
        );
    }

    #[test]
    fn rejected_images_and_the_notice_are_read() {
        let text = MINIMAL
            .replace(
                r#""images":[]"#,
                r#""images":[{"digest":"sha256:bad","image":"example.org/tur:stable",
                              "version":"0.4.0","event_id":"e1"},{"digest":"","image":"x"}]"#,
            )
            .replace(
                r#""notice":null"#,
                r#""notice":{"kind":"fallback","text":"The update did not start.",
                             "error_code":"TUR-BOOT-TRIAL-003","acknowledged":false}"#,
            );
        let health = parse_boot_health(&text).unwrap();
        assert_eq!(health.rejected.len(), 1);
        assert!(health.is_rejected("sha256:bad"));
        assert!(!health.is_rejected("sha256:good"));
        let notice = health.notice.unwrap();
        assert_eq!(notice.kind, "fallback");
        assert_eq!(notice.error_code, "TUR-BOOT-TRIAL-003");
        // A notice the user dismissed is not pending.
        let acknowledged = text.replace(r#""acknowledged":false"#, r#""acknowledged":true"#);
        assert!(parse_boot_health(&acknowledged).unwrap().notice.is_none());
    }

    #[test]
    fn the_state_file_has_the_shape_of_the_rejected_member() {
        let list = parse_rejected(
            r#"{"schema_version":1,"images":[{"digest":"sha256:bad","image":"i","version":"v",
                "event_id":"e"}]}"#,
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].version, "v");
        assert!(parse_rejected(r#"{"schema_version":2,"images":[]}"#).is_err());
        assert!(parse_rejected("[]").is_err());
    }

    #[test]
    fn a_document_that_cannot_be_trusted_is_refused() {
        for text in [
            "",
            "not json",
            "{}",
            r#"{"schema_version":1}"#,
            r#"{"schema_version":1,"counting":{"usable":true}}"#,
        ] {
            assert!(parse_boot_health(text).is_err(), "{text:?}");
        }
        let other_version = MINIMAL.replacen(r#""schema_version":1"#, r#""schema_version":2"#, 1);
        assert!(
            parse_boot_health(&other_version)
                .unwrap_err()
                .contains("schema version 2")
        );
        let odd_verdict = MINIMAL.replace("degraded", "splendid");
        assert!(
            parse_boot_health(&odd_verdict)
                .unwrap_err()
                .contains("splendid")
        );
        let odd_list = MINIMAL.replace(
            r#""rejected":{"schema_version":1"#,
            r#""rejected":{"schema_version":7"#,
        );
        assert!(parse_boot_health(&odd_list).is_err());
    }

    #[test]
    fn healthy_is_what_it_says() {
        let health = BootHealth::healthy();
        assert!(health.counting_usable && !health.trial_in_progress);
        assert_eq!(health.verdict, Some(Verdict::Passed));
    }
}

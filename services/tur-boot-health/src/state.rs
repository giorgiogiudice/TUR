// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The state this program keeps under `/var/lib/tur/boot-health` (design
//! section 5.2). `/var` is shared by every deployment, so the version that
//! reads a file is often not the version that wrote it. Every file carries
//! `schema_version: 1`, and a reader ignores members it does not know.

use crate::json::{self, Value};
use crate::model::Deployment;

/// The schema version of every state file.
pub(crate) const SCHEMA_VERSION: i64 = 1;

/// File names, relative to the state directory.
pub(crate) const TRIAL_FILE: &str = "trial.json";
pub(crate) const HISTORY_FILE: &str = "history.jsonl";
pub(crate) const REJECTED_FILE: &str = "rejected.json";
pub(crate) const NOTICE_FILE: &str = "notice.json";

/// How many history events are kept.
pub(crate) const HISTORY_LIMIT: usize = 200;

/// Where a trial stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Armed; the new deployment is being tried.
    Trial,
    /// The previous deployment has begun to make the fallback permanent
    /// (design section 5.9).
    Reverting,
}

impl Phase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Phase::Trial => "trial",
            Phase::Reverting => "reverting",
        }
    }
}

/// One start of the new deployment that reached this program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attempt {
    pub(crate) boot_id: String,
    /// `tur.boot_tries_left` of that boot, if GRUB passed one.
    pub(crate) tries_left: Option<i64>,
    /// `pending` until the gate decides, then `passed` or `failed`.
    pub(crate) verdict: String,
    /// The first required check that failed, for example `R5`.
    pub(crate) failed_check: Option<String>,
    /// Its error code.
    pub(crate) error_code: Option<String>,
    /// What the check reported.
    pub(crate) detail: Option<String>,
}

impl Attempt {
    fn to_json(&self) -> Value {
        Value::object(vec![
            ("boot_id", Value::from(self.boot_id.as_str())),
            (
                "tries_left",
                self.tries_left.map(Value::Int).unwrap_or(Value::Null),
            ),
            ("verdict", Value::from(self.verdict.as_str())),
            ("failed_check", Value::opt_str(self.failed_check.as_deref())),
            ("error_code", Value::opt_str(self.error_code.as_deref())),
            ("detail", Value::opt_str(self.detail.as_deref())),
        ])
    }

    fn from_json(value: &Value) -> Attempt {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Attempt {
            boot_id: value.str_or_empty("boot_id"),
            tries_left: value.get("tries_left").and_then(Value::as_i64),
            verdict: value.str_or_empty("verdict"),
            failed_check: text("failed_check"),
            error_code: text("error_code"),
            detail: text("detail"),
        }
    }
}

/// The trial record, `trial.json`: information about the trial in progress.
/// GRUB's environment block, not this file, is the authority for whether a
/// trial is in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrialRecord {
    /// Identifies the trial in the history, the notice and the evidence.
    pub(crate) event_id: String,
    pub(crate) phase: Phase,
    pub(crate) attempts_allowed: i64,
    /// The boot that armed the trial.
    pub(crate) armed_boot_id: String,
    /// Wall-clock seconds when it was armed. Informational: no decision uses it.
    pub(crate) armed_at: i64,
    /// The kernel arguments the trial boots get.
    pub(crate) trial_args: String,
    /// The deployment that was running when the trial was armed. Unknown when
    /// the record had to be made after the fact.
    pub(crate) previous: Option<Deployment>,
    /// The deployment under trial.
    pub(crate) new: Option<Deployment>,
    pub(crate) attempts: Vec<Attempt>,
    /// GRUB did not save the counter: attempts are counted here instead
    /// (design section 5.6).
    pub(crate) grub_cannot_save: bool,
    /// Why the fallback began, once it has: `attempts` or `loader`.
    pub(crate) reason: Option<String>,
}

impl TrialRecord {
    pub(crate) fn to_json(&self) -> Value {
        let deployment = |deployment: &Option<Deployment>| match deployment {
            Some(deployment) => deployment.to_json(),
            None => Value::Null,
        };
        Value::object(vec![
            ("schema_version", Value::Int(SCHEMA_VERSION)),
            ("event_id", Value::from(self.event_id.as_str())),
            ("phase", Value::from(self.phase.as_str())),
            ("attempts_allowed", Value::Int(self.attempts_allowed)),
            ("armed_boot_id", Value::from(self.armed_boot_id.as_str())),
            ("armed_at", Value::Int(self.armed_at)),
            ("trial_args", Value::from(self.trial_args.as_str())),
            ("previous", deployment(&self.previous)),
            ("new", deployment(&self.new)),
            (
                "attempts",
                Value::Array(self.attempts.iter().map(Attempt::to_json).collect()),
            ),
            ("grub_cannot_save", Value::Bool(self.grub_cannot_save)),
            ("reason", Value::opt_str(self.reason.as_deref())),
        ])
    }

    /// Reads `trial.json`. `None` for text that is not a version 1 record.
    pub(crate) fn parse(text: &str) -> Option<TrialRecord> {
        let value = json::parse(text).ok()?;
        if value.get("schema_version").and_then(Value::as_i64) != Some(SCHEMA_VERSION) {
            return None;
        }
        let phase = match value.get("phase").and_then(Value::as_str) {
            Some("trial") => Phase::Trial,
            Some("reverting") => Phase::Reverting,
            _ => return None,
        };
        let deployment = |key: &str| value.get(key).and_then(Deployment::from_record);
        Some(TrialRecord {
            event_id: value.str_or_empty("event_id"),
            phase,
            attempts_allowed: value
                .get("attempts_allowed")
                .and_then(Value::as_i64)
                .unwrap_or(3),
            armed_boot_id: value.str_or_empty("armed_boot_id"),
            armed_at: value.get("armed_at").and_then(Value::as_i64).unwrap_or(0),
            trial_args: value.str_or_empty("trial_args"),
            previous: deployment("previous"),
            new: deployment("new"),
            attempts: value
                .get("attempts")
                .and_then(Value::as_array)
                .map(|items| items.iter().map(Attempt::from_json).collect())
                .unwrap_or_default(),
            grub_cannot_save: value
                .get("grub_cannot_save")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// The attempt recorded for a boot, if that boot reached this program.
    pub(crate) fn attempt_mut(&mut self, boot_id: &str) -> Option<&mut Attempt> {
        self.attempts
            .iter_mut()
            .find(|attempt| attempt.boot_id == boot_id)
    }

    /// The first failed check among the attempts, with its code and detail.
    pub(crate) fn first_failure(&self) -> Option<&Attempt> {
        self.attempts
            .iter()
            .find(|attempt| attempt.failed_check.is_some())
    }
}

/// Adds one event to the history text and keeps the newest
/// [`HISTORY_LIMIT`] lines. Lines that are not JSON are dropped.
pub(crate) fn history_append(existing: &str, event: &Value) -> String {
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| json::parse(line).is_ok())
        .map(str::to_owned)
        .collect();
    lines.push(event.to_compact());
    let skip = lines.len().saturating_sub(HISTORY_LIMIT);
    let mut out = String::new();
    for line in lines.into_iter().skip(skip) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The events of the history text, oldest first.
pub(crate) fn history_events(existing: &str) -> Vec<Value> {
    existing
        .lines()
        .filter_map(|line| json::parse(line).ok())
        .collect()
}

/// An image that failed its trial. The updater must not stage it again unless
/// a person asks (design section 5.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejected {
    pub(crate) digest: String,
    pub(crate) image: String,
    pub(crate) version: String,
    pub(crate) event_id: String,
}

/// Reads `rejected.json`. Text that cannot be read is an empty list.
pub(crate) fn rejected_parse(text: &str) -> Vec<Rejected> {
    let Ok(value) = json::parse(text) else {
        return Vec::new();
    };
    value
        .get("images")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| Rejected {
                    digest: item.str_or_empty("digest"),
                    image: item.str_or_empty("image"),
                    version: item.str_or_empty("version"),
                    event_id: item.str_or_empty("event_id"),
                })
                .filter(|item| !item.digest.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The text of `rejected.json`.
pub(crate) fn rejected_to_json(list: &[Rejected]) -> Value {
    Value::object(vec![
        ("schema_version", Value::Int(SCHEMA_VERSION)),
        (
            "images",
            Value::Array(
                list.iter()
                    .map(|item| {
                        Value::object(vec![
                            ("digest", Value::from(item.digest.as_str())),
                            ("image", Value::from(item.image.as_str())),
                            ("version", Value::from(item.version.as_str())),
                            ("event_id", Value::from(item.event_id.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// Adds an image to the rejected list, once.
pub(crate) fn rejected_add(list: &mut Vec<Rejected>, entry: Rejected) {
    if entry.digest.is_empty() || list.iter().any(|item| item.digest == entry.digest) {
        return;
    }
    list.push(entry);
}

/// What the user is told (design section 5.11).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Notice {
    pub(crate) event_id: String,
    /// `fallback`, `not-applied`, `refused` or `unhealthy`.
    pub(crate) kind: String,
    pub(crate) text: String,
    pub(crate) error_code: String,
    pub(crate) boot_id: String,
    /// Further facts: versions, attempts, the failed check, the evidence path.
    pub(crate) details: Vec<(String, Value)>,
}

impl Notice {
    pub(crate) fn to_json(&self) -> Value {
        Value::object(vec![
            ("schema_version", Value::Int(SCHEMA_VERSION)),
            ("event_id", Value::from(self.event_id.as_str())),
            ("kind", Value::from(self.kind.as_str())),
            ("text", Value::from(self.text.as_str())),
            ("error_code", Value::from(self.error_code.as_str())),
            ("boot_id", Value::from(self.boot_id.as_str())),
            ("acknowledged", Value::Bool(false)),
            ("details", Value::Object(self.details.clone())),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment(checksum: &str, serial: i64) -> Deployment {
        Deployment {
            image: "localhost/tur:ci".to_owned(),
            transport: "registry".to_owned(),
            digest: format!("sha256:{checksum}"),
            version: "0.1.0".to_owned(),
            checksum: checksum.to_owned(),
            serial,
            stateroot: "default".to_owned(),
            download_only: false,
            build_id: Some("20261112.gabc".to_owned()),
        }
    }

    fn record() -> TrialRecord {
        TrialRecord {
            event_id: "1700000000-abcd1234".to_owned(),
            phase: Phase::Trial,
            attempts_allowed: 3,
            armed_boot_id: "abcd1234".to_owned(),
            armed_at: 1_700_000_000,
            trial_args: "panic=30".to_owned(),
            previous: Some(deployment("1111", 0)),
            new: Some(deployment("2222", 0)),
            attempts: vec![Attempt {
                boot_id: "b1".to_owned(),
                tries_left: Some(2),
                verdict: "failed".to_owned(),
                failed_check: Some("R5".to_owned()),
                error_code: Some("TUR-BOOT-HEALTH-005".to_owned()),
                detail: Some("unit is failed".to_owned()),
            }],
            grub_cannot_save: false,
            reason: None,
        }
    }

    #[test]
    fn the_trial_record_reads_back_what_it_wrote() {
        let record = record();
        let text = record.to_json().to_pretty();
        assert_eq!(TrialRecord::parse(&text), Some(record.clone()));
        let mut reverting = record;
        reverting.phase = Phase::Reverting;
        reverting.previous = None;
        reverting.reason = Some("attempts".to_owned());
        assert_eq!(
            TrialRecord::parse(&reverting.to_json().to_compact()),
            Some(reverting)
        );
    }

    #[test]
    fn a_record_of_another_version_is_not_read() {
        assert_eq!(TrialRecord::parse("{}"), None);
        assert_eq!(TrialRecord::parse("garbage"), None);
        assert_eq!(
            TrialRecord::parse(r#"{"schema_version": 2, "phase": "trial"}"#),
            None
        );
        assert_eq!(
            TrialRecord::parse(r#"{"schema_version": 1, "phase": "paused"}"#),
            None
        );
    }

    #[test]
    fn a_reader_ignores_members_it_does_not_know() {
        let text = r#"{"schema_version": 1, "phase": "trial", "event_id": "e", "future": [1, 2]}"#;
        let record = TrialRecord::parse(text).unwrap();
        assert_eq!(record.event_id, "e");
        assert_eq!(record.attempts_allowed, 3);
        assert!(record.previous.is_none());
        assert!(record.attempts.is_empty());
    }

    #[test]
    fn attempts_are_found_by_boot_id() {
        let mut record = record();
        assert!(record.attempt_mut("b1").is_some());
        assert!(record.attempt_mut("other").is_none());
        assert_eq!(
            record.first_failure().and_then(|a| a.failed_check.clone()),
            Some("R5".to_owned())
        );
    }

    #[test]
    fn the_history_keeps_the_newest_events() {
        let mut text = String::from("not json\n");
        for number in 0..(HISTORY_LIMIT as i64 + 5) {
            text = history_append(&text, &Value::object(vec![("n", Value::Int(number))]));
        }
        let events = history_events(&text);
        assert_eq!(events.len(), HISTORY_LIMIT);
        assert_eq!(events[0].get("n").and_then(Value::as_i64), Some(5));
        assert_eq!(
            events[HISTORY_LIMIT - 1].get("n").and_then(Value::as_i64),
            Some(HISTORY_LIMIT as i64 + 4)
        );
    }

    #[test]
    fn an_image_is_rejected_once() {
        let mut list = rejected_parse("garbage");
        assert!(list.is_empty());
        let entry = Rejected {
            digest: "sha256:bad".to_owned(),
            image: "localhost/tur:ci".to_owned(),
            version: "0.2.0".to_owned(),
            event_id: "e1".to_owned(),
        };
        rejected_add(&mut list, entry.clone());
        rejected_add(&mut list, entry.clone());
        rejected_add(
            &mut list,
            Rejected {
                digest: String::new(),
                ..entry.clone()
            },
        );
        assert_eq!(list.len(), 1);
        let text = rejected_to_json(&list).to_pretty();
        assert_eq!(rejected_parse(&text), vec![entry]);
    }

    #[test]
    fn a_notice_is_unacknowledged_when_written() {
        let notice = Notice {
            event_id: "e1".to_owned(),
            kind: "fallback".to_owned(),
            text: "The update did not start correctly.".to_owned(),
            error_code: "TUR-BOOT-TRIAL-003".to_owned(),
            boot_id: "b".to_owned(),
            details: vec![("attempts".to_owned(), Value::Int(3))],
        };
        let value = notice.to_json();
        assert_eq!(
            value.get("acknowledged").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            value.path(&["details", "attempts"]).and_then(Value::as_i64),
            Some(3)
        );
    }
}

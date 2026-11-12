// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the receiver does, without HTTP: accept a report, answer a status
//! request, file what is due, sweep what has expired, and the
//! administrator's two actions (mark fixed, decrypt a bundle).

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use serde::Serialize;
use tur_diag::bundle::sha256_hex;
use tur_diag::report::{CONTENT_TYPE, affected_version, is_version, read_report, signature};
use tur_diag::{ErrorKind, Profile, verify_members};

use crate::clock::Clock;
use crate::codes::{ErrorBody, Refusal};
use crate::config::Config;
use crate::crypto::{Keyring, Purpose, hex, random_bytes};
use crate::log::Log;
use crate::ratelimit::{RateLimiter, Verdict};
use crate::store::{Filing, Locked, Record, State, Store, is_hex64};
use crate::summary::{self, Facts, Summary};
use crate::tracker::{Tracker, TrackerError};

/// An answer to an HTTP request: status, JSON body, and how long to wait
/// before trying again, for a rate limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The HTTP status.
    pub status: u16,
    /// The JSON body.
    pub body: String,
    /// Seconds for `Retry-After`.
    pub retry_after: Option<u64>,
}

/// The receipt for an accepted report (`schemas/report-receiver.schema.json`).
#[derive(Debug, Serialize)]
struct ReceiptBody<'a> {
    schema_version: u32,
    report_id: &'a str,
    status_token: &'a str,
    status_url: String,
    state: &'static str,
    retention_days: u64,
}

/// The state of a report (`schemas/report-receiver.schema.json`).
#[derive(Debug, Serialize)]
struct StateBody<'a> {
    schema_version: u32,
    state: &'static str,
    fixed_in: Option<&'a str>,
}

/// What one round of filing did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FilingRound {
    /// Reports filed.
    pub filed: usize,
    /// Reports whose filing failed; the round stops at the first.
    pub failed: usize,
}

/// What one sweep deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SweepRound {
    /// Bundles past their retention.
    pub bundles_deleted: usize,
    /// Records past their retention.
    pub records_deleted: usize,
    /// Files left by an interrupted write.
    pub orphans_deleted: usize,
}

/// The receiver.
#[derive(Debug)]
pub struct Receiver {
    config: Config,
    store: Store,
    keyring: Keyring,
    clock: Arc<dyn Clock>,
    log: Log,
    uploads: RateLimiter,
    lookups: RateLimiter,
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    random_bytes(&mut buffer)?;
    Ok(hex(&buffer))
}

impl Receiver {
    /// A receiver with the given keys, clock and log.
    pub fn new(config: Config, keyring: Keyring, clock: Arc<dyn Clock>, log: Log) -> io::Result<Self> {
        let store = Store::open(&config.data_dir)?;
        let uploads = RateLimiter::new(
            config.limits.uploads_per_address_per_hour,
            Some(config.limits.uploads_per_hour),
        );
        let lookups = RateLimiter::new(config.limits.lookups_per_address_per_hour, None);
        Ok(Receiver {
            config,
            store,
            keyring,
            clock,
            log,
            uploads,
            lookups,
        })
    }

    /// A receiver whose keys are read from the configured key directory.
    pub fn open(config: Config, clock: Arc<dyn Clock>, log: Log) -> Result<Self, String> {
        let keyring = Keyring::load(&config.keys_dir, &config.current_key)?;
        Receiver::new(config, keyring, clock, log)
            .map_err(|error| format!("data directory: {error}"))
    }

    /// The configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The log.
    pub fn log(&self) -> &Log {
        &self.log
    }

    /// The store, for tests and the administrator.
    pub fn store(&self) -> &Store {
        &self.store
    }

    fn now(&self) -> u64 {
        self.clock.now()
    }

    fn lock(&self) -> io::Result<Locked> {
        self.store.lock()
    }

    /// Refuses a request, and records the code (nothing else) in the log.
    pub fn refuse(&self, refusal: Refusal, detail_code: Option<String>) -> Answer {
        self.log.info(
            "refused",
            &[
                ("code", refusal.code()),
                ("detail", detail_code.as_deref().unwrap_or("-")),
            ],
        );
        Answer {
            status: refusal.status(),
            body: json(&ErrorBody::new(refusal, detail_code)),
            retry_after: None,
        }
    }

    fn limited(&self, refusal: Refusal, retry_after: u64) -> Answer {
        let mut answer = self.refuse(refusal, None);
        answer.retry_after = Some(retry_after);
        answer
    }

    /// Counts an upload from `client` against the rate limits. `None` when
    /// it may go ahead, else the refusal to send. Called before the body is
    /// read.
    pub fn admit_upload(&self, client: IpAddr) -> Option<Answer> {
        match self.uploads.check(client, self.now()) {
            Verdict::Allowed => None,
            Verdict::AddressLimited(after) => Some(self.limited(Refusal::AddressLimit, after)),
            Verdict::GlobalLimited(after) => Some(self.limited(Refusal::GlobalLimit, after)),
        }
    }

    /// Accepts an uploaded bundle, or refuses it.
    pub fn submit(&self, content_type: Option<&str>, body: &[u8]) -> Answer {
        let media = content_type
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase());
        if media.as_deref() != Some(CONTENT_TYPE) {
            return self.refuse(Refusal::UnsupportedMediaType, None);
        }
        let limits = &self.config.limits;
        if u64::try_from(body.len()).unwrap_or(u64::MAX) > limits.max_upload_bytes {
            return self.refuse(Refusal::TooLarge, None);
        }
        let (verified, members) = match verify_members(body, limits.max_unpacked_bytes) {
            Ok(verified) => verified,
            Err(error) if error.kind() == ErrorKind::BundleTooLarge => {
                return self.refuse(Refusal::TooLarge, Some(error.code().to_string()));
            }
            Err(error) => {
                return self.refuse(Refusal::NotABundle, Some(error.code().to_string()));
            }
        };
        if verified.manifest.redaction_profile != Profile::Default {
            return self.refuse(Refusal::TechnicalProfile, None);
        }
        let Ok(report) = read_report(&members) else {
            return self.refuse(Refusal::NotAReport, None);
        };
        match self.store_report(body, &verified.manifest, &report, &members) {
            Ok(answer) => answer,
            Err(problem) => {
                self.log.warn("store-failed", &[("reason", problem.as_str())]);
                self.refuse(Refusal::Internal, None)
            }
        }
    }

    fn store_report(
        &self,
        archive: &[u8],
        manifest: &tur_diag::Manifest,
        report: &tur_diag::report::ReportRequest,
        members: &[tur_diag::bundle::Member],
    ) -> Result<Answer, String> {
        let now = self.now();
        let version = affected_version(report, manifest);
        let signature = signature(report.error_code.as_deref(), &report.component, &version);
        let report_id = format!("R-{}", random_hex(8)?);
        let token = random_hex(32)?;
        let bundle_expires_at = now.saturating_add(self.config.retention_seconds());
        let facts = Facts {
            report_id: &report_id,
            received_at: now,
            bundle_expires_at,
            signature: &signature,
            version: &version,
        };
        let summary = summary::build(manifest, report, members, &facts);
        let summary_bytes = serde_json::to_vec(&summary).map_err(|error| error.to_string())?;
        let sealed_bundle = self.keyring.seal(Purpose::Bundle, &report_id, archive)?;
        let sealed_summary = self.keyring.seal(Purpose::Summary, &report_id, &summary_bytes)?;
        let record = Record {
            schema_version: 1,
            report_id: report_id.clone(),
            token_sha256: sha256_hex(token.as_bytes()),
            received_at: now,
            bundle_expires_at,
            record_expires_at: now.saturating_add(self.config.status_retention_seconds()),
            bundle_deleted_at: None,
            key_id: self.keyring.current().to_owned(),
            signature: signature.clone(),
            error_code: report.error_code.clone(),
            component: report.component.clone(),
            version: version.clone(),
            state: State::Received,
            filing: Filing::Pending,
            issue: None,
            attempts: 0,
            next_attempt_at: now,
            last_failure: None,
            fixed_in: None,
        };
        {
            let lock = self.lock().map_err(|error| error.to_string())?;
            if self
                .store
                .record(&lock, &report_id)
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Err("report id collision".to_owned());
            }
            self.store
                .put_report(&lock, &record, &sealed_bundle, &sealed_summary)
                .map_err(|error| error.to_string())?;
        }
        let bytes = archive.len().to_string();
        let scrubbed = summary.server_replacements.to_string();
        self.log.info(
            "received",
            &[
                ("report", report_id.as_str()),
                ("signature", signature.as_str()),
                ("bytes", bytes.as_str()),
                ("scrubbed", scrubbed.as_str()),
            ],
        );
        let receipt = ReceiptBody {
            schema_version: 1,
            report_id: &report_id,
            status_token: &token,
            status_url: format!(
                "{}{}/{token}",
                self.config.public_url,
                tur_diag::report::UPLOAD_PATH
            ),
            state: State::Received.as_str(),
            retention_days: self.config.retention_days,
        };
        Ok(Answer {
            status: 202,
            body: json(&receipt),
            retry_after: None,
        })
    }

    /// Answers a status request: the state of one report, nothing else.
    pub fn status(&self, client: IpAddr, token: &str) -> Answer {
        match self.lookups.check(client, self.now()) {
            Verdict::Allowed => {}
            Verdict::AddressLimited(after) | Verdict::GlobalLimited(after) => {
                return self.limited(Refusal::LookupLimit, after);
            }
        }
        if !is_hex64(token) {
            return self.refuse(Refusal::UnknownToken, None);
        }
        let found = self.lock().and_then(|lock| {
            let Some(id) = self.store.report_for_token(&lock, &sha256_hex(token.as_bytes()))? else {
                return Ok(None);
            };
            self.store.record(&lock, &id)
        });
        match found {
            Ok(Some(record)) => Answer {
                status: 200,
                body: json(&StateBody {
                    schema_version: 1,
                    state: record.state.as_str(),
                    fixed_in: record.fixed_in.as_deref(),
                }),
                retry_after: None,
            },
            Ok(None) => self.refuse(Refusal::UnknownToken, None),
            Err(_) => self.refuse(Refusal::Internal, None),
        }
    }

    fn backoff(&self, attempts: u32) -> u64 {
        let timing = &self.config.timing;
        let doublings = attempts.saturating_sub(1).min(30);
        timing
            .retry_initial_seconds
            .saturating_mul(1u64 << doublings)
            .min(timing.retry_max_seconds)
    }

    /// Files every report whose time has come, oldest first. Stops at the
    /// first failure: the tracker is probably down, and the others wait for
    /// the next round.
    pub fn file_due(&self, tracker: &dyn Tracker) -> FilingRound {
        let now = self.now();
        let mut round = FilingRound::default();
        let due: Vec<String> = match self.lock().and_then(|lock| self.store.records(&lock)) {
            Ok(records) => records
                .into_iter()
                .filter(|record| record.filing == Filing::Pending && record.next_attempt_at <= now)
                .map(|record| record.report_id)
                .collect(),
            Err(_) => {
                self.log.warn("store-unreadable", &[]);
                return round;
            }
        };
        for id in due {
            match self.file_one(tracker, &id, now) {
                Ok(true) => round.filed += 1,
                Ok(false) => {}
                Err(()) => {
                    round.failed += 1;
                    break;
                }
            }
        }
        round
    }

    /// Files one report. `Ok(true)` when filed, `Ok(false)` when there was
    /// nothing to do, `Err(())` when filing failed and was rescheduled.
    fn file_one(&self, tracker: &dyn Tracker, id: &str, now: u64) -> Result<bool, ()> {
        let prepared = self.lock().and_then(|lock| {
            let Some(record) = self.store.record(&lock, id)? else {
                return Ok(None);
            };
            if record.filing != Filing::Pending {
                return Ok(None);
            }
            let sealed = self.store.summary(&lock, id)?;
            let issue = self.store.signatures(&lock)?.get(&record.signature).copied();
            Ok(Some((record, sealed, issue)))
        });
        let (record, sealed, issue) = match prepared {
            Ok(Some(found)) => found,
            Ok(None) => return Ok(false),
            Err(_) => {
                self.log.warn("store-unreadable", &[("report", id)]);
                return Err(());
            }
        };
        let summary: Option<Summary> = sealed
            .and_then(|sealed| self.keyring.open(Purpose::Summary, id, &sealed).ok())
            .and_then(|plain| serde_json::from_slice(&plain).ok());
        let Some(summary) = summary else {
            self.reschedule(id, now, "summary-unreadable");
            return Err(());
        };
        let labels = &self.config.github.labels;
        let result = match issue {
            Some(number) => match tracker.comment(number, &summary.comment_body) {
                Ok(()) => Ok((number, Filing::Comment)),
                Err(TrackerError::Gone) => {
                    self.forget_signature(&record.signature);
                    tracker
                        .create_issue(&summary.title, &summary.issue_body, labels)
                        .map(|number| (number, Filing::Issue))
                }
                Err(error) => Err(error),
            },
            None => tracker
                .create_issue(&summary.title, &summary.issue_body, labels)
                .map(|number| (number, Filing::Issue)),
        };
        match result {
            Ok((number, filing)) => {
                let saved = self.lock().and_then(|lock| {
                    let Some(mut current) = self.store.record(&lock, id)? else {
                        return Ok(());
                    };
                    current.filing = filing;
                    current.issue = Some(number);
                    if current.state == State::Received {
                        current.state = State::Filed;
                    }
                    current.last_failure = None;
                    self.store.write_record(&lock, &current)?;
                    if filing == Filing::Issue {
                        let mut signatures = self.store.signatures(&lock)?;
                        signatures.insert(current.signature.clone(), number);
                        self.store.write_signatures(&lock, &signatures)?;
                    }
                    self.store.remove_summary(&lock, id)?;
                    Ok(())
                });
                if saved.is_err() {
                    self.log.warn("store-failed", &[("report", id)]);
                }
                let how = match filing {
                    Filing::Comment => "comment",
                    _ => "issue",
                };
                let issue = number.to_string();
                self.log.info(
                    "filed",
                    &[
                        ("report", id),
                        ("signature", record.signature.as_str()),
                        ("issue", issue.as_str()),
                        ("as", how),
                    ],
                );
                Ok(true)
            }
            Err(TrackerError::Unavailable(reason)) => {
                self.reschedule(id, now, &reason);
                Err(())
            }
            Err(TrackerError::Gone) => {
                self.reschedule(id, now, "gone");
                Err(())
            }
        }
    }

    fn forget_signature(&self, signature: &str) {
        let forgotten = self.lock().and_then(|lock| {
            let mut signatures = self.store.signatures(&lock)?;
            signatures.remove(signature);
            self.store.write_signatures(&lock, &signatures)
        });
        if forgotten.is_err() {
            self.log.warn("store-failed", &[("signature", signature)]);
        }
    }

    fn reschedule(&self, id: &str, now: u64, reason: &str) {
        let result = self.lock().and_then(|lock| {
            let Some(mut record) = self.store.record(&lock, id)? else {
                return Ok(None);
            };
            record.attempts = record.attempts.saturating_add(1);
            let wait = self.backoff(record.attempts);
            record.next_attempt_at = now.saturating_add(wait);
            record.last_failure = Some(reason.to_owned());
            self.store.write_record(&lock, &record)?;
            Ok(Some((record.attempts, wait)))
        });
        match result {
            Ok(Some((attempts, wait))) => {
                let attempts = attempts.to_string();
                let wait = wait.to_string();
                self.log.warn(
                    "filing-failed",
                    &[
                        ("report", id),
                        ("reason", reason),
                        ("attempts", attempts.as_str()),
                        ("retry_in", wait.as_str()),
                    ],
                );
            }
            Ok(None) => {}
            Err(_) => self.log.warn("store-failed", &[("report", id)]),
        }
    }

    /// Deletes bundles past the retention period, records past theirs, and
    /// files left by an interrupted write.
    pub fn sweep(&self) -> io::Result<SweepRound> {
        let now = self.now();
        let mut round = SweepRound::default();
        let lock = self.lock()?;
        for mut record in self.store.records(&lock)? {
            if now >= record.record_expires_at {
                self.store.remove_report(&lock, &record)?;
                round.records_deleted += 1;
                continue;
            }
            if record.bundle_deleted_at.is_none() && now >= record.bundle_expires_at {
                self.store.remove_bundle(&lock, &record.report_id)?;
                record.bundle_deleted_at = Some(now);
                self.store.write_record(&lock, &record)?;
                round.bundles_deleted += 1;
            }
        }
        round.orphans_deleted = self.store.remove_orphans(&lock)?;
        drop(lock);
        let bundles = round.bundles_deleted.to_string();
        let records = round.records_deleted.to_string();
        let orphans = round.orphans_deleted.to_string();
        self.log.info(
            "swept",
            &[
                ("bundles", bundles.as_str()),
                ("records", records.as_str()),
                ("orphans", orphans.as_str()),
            ],
        );
        Ok(round)
    }

    /// Marks every report filed under `issue` as fixed in `version`.
    pub fn mark_fixed(&self, issue: u64, version: &str) -> Result<usize, String> {
        if !is_version(version) {
            return Err("the version must be letters, digits and . + _ ~ -".to_owned());
        }
        let lock = self.lock().map_err(|error| error.to_string())?;
        let mut marked = 0;
        for mut record in self.store.records(&lock).map_err(|error| error.to_string())? {
            if record.issue == Some(issue) {
                record.state = State::Fixed;
                record.fixed_in = Some(version.to_owned());
                self.store
                    .write_record(&lock, &record)
                    .map_err(|error| error.to_string())?;
                marked += 1;
            }
        }
        drop(lock);
        let issue = issue.to_string();
        let reports = marked.to_string();
        self.log.info(
            "marked-fixed",
            &[
                ("issue", issue.as_str()),
                ("version", version),
                ("reports", reports.as_str()),
            ],
        );
        Ok(marked)
    }

    /// The raw bundle of a report, decrypted, for a maintainer.
    pub fn decrypt(&self, report_id: &str) -> Result<Vec<u8>, String> {
        let lock = self.lock().map_err(|error| error.to_string())?;
        let record = self
            .store
            .record(&lock, report_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("there is no report {report_id}"))?;
        if record.bundle_deleted_at.is_some() {
            return Err(format!(
                "the bundle of {report_id} was deleted after the retention period"
            ));
        }
        let sealed = self
            .store
            .bundle(&lock, report_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("the bundle of {report_id} is missing"))?;
        drop(lock);
        self.log.info("decrypted", &[("report", report_id)]);
        self.keyring.open(Purpose::Bundle, report_id, &sealed)
    }
}

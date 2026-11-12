// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What P@x used today, and the limit of one day.
//!
//! A task has its own limit, kept by the provider layer's meter. The limit
//! of a day is across tasks, so it needs a record that outlives them: the
//! spending record, `$XDG_STATE_HOME/tur/pax/spending.json`. It holds, for
//! each day, the tokens the providers reported and what they cost at the
//! configured prices. It holds no request, no answer and no key.
//!
//! How the limit is enforced (addendum A sections 7 and 10):
//!
//! * [`TaskBudget`] is the task loop's [`Budget`]. Before every request to
//!   the provider the loop asks whether the day's limit has been reached;
//!   if it has, nothing is sent and the task stops with
//!   `TUR-AI-SPEND-001`.
//! * After every response the loop reports what the task has used so far,
//!   and the difference to the last report is added to the day and written
//!   to the file at once. A task that is cut off, cancelled or crashes has
//!   been counted up to its last response.
//! * The limit acts between two responses, because a provider reports usage
//!   at the end of one. A day can pass its limit by the cost of one
//!   response, which the output limit bounds.
//! * **If the record cannot be written, the day counts as used up.** A
//!   limit that cannot be kept is treated as reached.
//!
//! A day is a calendar day in the time zone the computer is set to. If the
//! clock or the zone is changed, "today" is whatever day the clock then
//! shows: usage is never moved between days, and a day that comes round
//! again finds its earlier total.
//!
//! The amounts are estimates from the provider's reported token counts and
//! the prices in the model list. The provider's invoice is the authority.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tur_pax_memory::{Clock, Timestamp};
use tur_pax_providers::Usage;

use crate::faults::{Fault, TaskCode};
use crate::orchestrator::Budget;
use crate::settings::{DayLimit, write_private};

/// The schema version of the spending record.
pub const SPENDING_SCHEMA_VERSION: u32 = 1;

/// How many days the record keeps.
pub const DAYS_KEPT: usize = 62;

/// What was used on one day.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayUse {
    /// How many responses were counted.
    #[serde(default)]
    pub responses: u64,
    /// Every input token.
    #[serde(default)]
    pub input_tokens: u64,
    /// Every output token.
    #[serde(default)]
    pub output_tokens: u64,
    /// The part of the input that was read from a provider's cache.
    #[serde(default)]
    pub cached_input_tokens: u64,
    /// What the tokens cost at the configured prices. It counts only the
    /// responses whose model had a known price.
    #[serde(default)]
    pub cost: f64,
    /// The currency of the cost, once a priced response was counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

impl DayUse {
    /// Input and output tokens together.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Whether this much use reaches a limit.
    pub fn reaches(&self, limit: &DayLimit) -> bool {
        limit.cost.is_some_and(|most| self.cost >= most)
            || limit.tokens.is_some_and(|most| self.total_tokens() >= most)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    days: BTreeMap<String, DayUse>,
}

/// The spending record.
#[derive(Debug, Clone, PartialEq)]
pub struct Ledger {
    path: PathBuf,
    days: BTreeMap<String, DayUse>,
}

fn storage(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceStorage, detail)
}

fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

impl Ledger {
    /// Load the record. A file that does not exist is a record with nothing
    /// in it.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the file exists and is not a spending record.
    /// The caller then has no limit it can keep, and starts no task.
    pub fn load(path: &Path) -> Result<Self, Fault> {
        let days = match fs::read_to_string(path) {
            Ok(text) => {
                let record: Record = serde_json::from_str(&text).map_err(|error| {
                    storage(format!(
                        "{}: not a spending record, at line {} column {}",
                        path.display(),
                        error.line(),
                        error.column()
                    ))
                })?;
                let valid = record.schema_version == SPENDING_SCHEMA_VERSION
                    && record.days.iter().all(|(day, used)| {
                        is_day(day) && used.cost.is_finite() && used.cost >= 0.0
                    });
                if !valid {
                    return Err(storage(format!(
                        "{}: not a spending record of this version",
                        path.display()
                    )));
                }
                record.days
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(storage(format!("{}: {error}", path.display()))),
        };
        Ok(Ledger {
            path: path.to_path_buf(),
            days,
        })
    }

    /// What was used on a day, written `YYYY-MM-DD`.
    pub fn day(&self, day: &str) -> DayUse {
        self.days.get(day).cloned().unwrap_or_default()
    }

    /// What was used in a month, written `YYYY-MM`.
    pub fn month(&self, month: &str) -> DayUse {
        let mut total = DayUse::default();
        for (day, used) in &self.days {
            if day.starts_with(month) && day.as_bytes().get(month.len()) == Some(&b'-') {
                total.responses = total.responses.saturating_add(used.responses);
                total.input_tokens = total.input_tokens.saturating_add(used.input_tokens);
                total.output_tokens = total.output_tokens.saturating_add(used.output_tokens);
                total.cached_input_tokens = total
                    .cached_input_tokens
                    .saturating_add(used.cached_input_tokens);
                total.cost += used.cost;
                if total.currency.is_none() {
                    total.currency.clone_from(&used.currency);
                }
            }
        }
        total
    }

    /// Add use to a day and write the record.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the day is not a date or the record cannot be
    /// written. The use stays counted in memory either way.
    pub fn add(
        &mut self,
        day: &str,
        usage: &Usage,
        cost: Option<f64>,
        currency: Option<&str>,
        responses: u64,
    ) -> Result<(), Fault> {
        if !is_day(day) {
            return Err(storage("the day of the use is not a date"));
        }
        let used = self.days.entry(day.to_owned()).or_default();
        used.responses = used.responses.saturating_add(responses);
        used.input_tokens = used.input_tokens.saturating_add(usage.input_tokens);
        used.output_tokens = used.output_tokens.saturating_add(usage.output_tokens);
        used.cached_input_tokens = used
            .cached_input_tokens
            .saturating_add(usage.cached_input_tokens);
        if let Some(cost) = cost.filter(|cost| cost.is_finite() && *cost > 0.0) {
            used.cost += cost;
            if used.currency.is_none() {
                used.currency = currency.map(str::to_owned);
            }
        }
        // The oldest days go.
        while self.days.len() > DAYS_KEPT {
            self.days.pop_first();
        }
        self.save()
    }

    fn save(&self) -> Result<(), Fault> {
        let record = Record {
            schema_version: SPENDING_SCHEMA_VERSION,
            days: self.days.clone(),
        };
        let text = serde_json::to_string_pretty(&record)
            .map_err(|_| storage("the spending record could not be written out"))?;
        write_private(&self.path, text.as_bytes())
    }
}

/// The day a clock shows in a time zone, as `YYYY-MM-DD`. A zone that is
/// not known reads as UTC.
pub fn day_of(now: Timestamp, zone: &str) -> String {
    now.wall_clock_in(zone)
        .or_else(|| now.wall_clock_in("UTC"))
        .map_or_else(|| "1970-01-01".to_owned(), |(date, _)| date)
}

/// Tells the zone the computer is set to. It is asked each time, so that a
/// change of zone is seen.
pub type Zone = Arc<dyn Fn() -> String + Send + Sync>;

/// The day's limit for one task: the task loop's [`Budget`].
pub struct TaskBudget {
    ledger: Arc<Mutex<Ledger>>,
    limit: DayLimit,
    clock: Arc<dyn Clock + Send + Sync>,
    zone: Zone,
    currency: Option<String>,
    /// What of this task has been added to the day.
    counted: Usage,
    counted_cost: f64,
    /// The record could not be written: the day counts as used up.
    failed: bool,
}

impl TaskBudget {
    /// The budget of one task.
    pub fn new(
        ledger: Arc<Mutex<Ledger>>,
        limit: DayLimit,
        clock: Arc<dyn Clock + Send + Sync>,
        zone: Zone,
        currency: Option<String>,
    ) -> Self {
        TaskBudget {
            ledger,
            limit,
            clock,
            zone,
            currency,
            counted: Usage::default(),
            counted_cost: 0.0,
            failed: false,
        }
    }

    fn today(&self) -> String {
        day_of(self.clock.now(), &(self.zone)())
    }

    /// Whether the record could not be written at some point.
    pub fn failed(&self) -> bool {
        self.failed
    }
}

impl Budget for TaskBudget {
    fn exhausted(&mut self) -> bool {
        if self.failed {
            return true;
        }
        let today = self.today();
        let ledger = self
            .ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ledger.day(&today).reaches(&self.limit)
    }

    fn used(&mut self, usage: Usage, cost: Option<f64>) {
        let more = Usage {
            input_tokens: usage.input_tokens.saturating_sub(self.counted.input_tokens),
            output_tokens: usage
                .output_tokens
                .saturating_sub(self.counted.output_tokens),
            cached_input_tokens: usage
                .cached_input_tokens
                .saturating_sub(self.counted.cached_input_tokens),
        };
        let total_cost = cost.unwrap_or(0.0);
        let more_cost = (total_cost - self.counted_cost).max(0.0);
        self.counted = usage;
        self.counted_cost = total_cost;
        let today = self.today();
        let mut ledger = self
            .ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let written = ledger.add(
            &today,
            &more,
            cost.map(|_| more_cost),
            self.currency.as_deref(),
            1,
        );
        if written.is_err() {
            self.failed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TestClock(AtomicU64);

    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.load(Ordering::SeqCst))
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("tur-pax-spending-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory.join("spending.json")
    }

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: 0,
        }
    }

    fn zone(name: &'static str) -> Zone {
        Arc::new(move || name.to_owned())
    }

    // 2026-11-12T10:00:00Z.
    const NOON: u64 = 1_794_477_600;

    #[test]
    fn use_is_added_to_its_day_and_survives_a_restart() {
        let path = scratch("add");
        let mut ledger = Ledger::load(&path).expect("an empty record");
        assert_eq!(ledger.day("2026-11-12"), DayUse::default());
        ledger
            .add("2026-11-12", &usage(100, 20), Some(0.5), Some("USD"), 1)
            .expect("written");
        ledger
            .add("2026-11-12", &usage(50, 5), Some(0.25), Some("USD"), 1)
            .expect("written");
        ledger
            .add("2026-11-13", &usage(7, 3), None, None, 1)
            .expect("written");
        ledger
            .add("2026-12-01", &usage(1, 1), Some(1.0), Some("USD"), 1)
            .expect("written");

        let read = Ledger::load(&path).expect("read back");
        assert_eq!(read, ledger);
        let day = read.day("2026-11-12");
        assert_eq!(day.responses, 2);
        assert_eq!(day.input_tokens, 150);
        assert_eq!(day.output_tokens, 25);
        assert_eq!(day.total_tokens(), 175);
        assert!((day.cost - 0.75).abs() < 1e-9);
        assert_eq!(day.currency.as_deref(), Some("USD"));
        // A day with use whose price was not known has tokens and no cost.
        let unpriced = read.day("2026-11-13");
        assert_eq!(unpriced.total_tokens(), 10);
        assert_eq!(unpriced.cost, 0.0);
        assert_eq!(unpriced.currency, None);
        // The month adds its days up, and only its own.
        let month = read.month("2026-11");
        assert_eq!(month.total_tokens(), 185);
        assert!((month.cost - 0.75).abs() < 1e-9);
        assert_eq!(read.month("2026-1").total_tokens(), 0);

        assert!(ledger.add("today", &usage(1, 1), None, None, 1).is_err());
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    #[test]
    fn a_record_that_cannot_be_used_is_refused() {
        let path = scratch("damaged");
        fs::create_dir_all(path.parent().expect("a directory")).expect("created");
        for text in [
            "",
            "{",
            "[]",
            r#"{"schema_version": 2, "days": {}}"#,
            r#"{"schema_version": 1}"#,
            r#"{"schema_version": 1, "days": {"yesterday": {}}}"#,
            r#"{"schema_version": 1, "days": {"2026-11-12": {"cost": -1}}}"#,
            r#"{"schema_version": 1, "days": {"2026-11-12": {"key": "x"}}}"#,
        ] {
            fs::write(&path, text).expect("written");
            let fault = Ledger::load(&path).expect_err(text);
            assert_eq!(fault.code.as_str(), "TUR-AI-SERVICE-009", "{text}");
        }
        fs::write(
            &path,
            r#"{"schema_version": 1, "days": {"2026-11-12": {}}}"#,
        )
        .expect("written");
        assert_eq!(
            Ledger::load(&path).expect("read").day("2026-11-12"),
            DayUse::default()
        );
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    #[test]
    fn only_the_newest_days_are_kept() {
        let path = scratch("kept");
        let mut ledger = Ledger::load(&path).expect("an empty record");
        for day in 0..(DAYS_KEPT + 10) {
            let date = format!("2026-{:02}-{:02}", 1 + day / 28, 1 + day % 28);
            ledger
                .add(&date, &usage(1, 0), None, None, 1)
                .expect("written");
        }
        assert_eq!(ledger.days.len(), DAYS_KEPT);
        assert_eq!(ledger.day("2026-01-01"), DayUse::default());
        assert_eq!(ledger.day("2026-03-16").input_tokens, 1);
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    #[test]
    fn the_limit_of_a_day_is_reached_at_the_limit_in_cost_or_in_tokens() {
        let used = DayUse {
            input_tokens: 600,
            output_tokens: 400,
            cost: 2.0,
            ..DayUse::default()
        };
        let limit = |cost: Option<f64>, tokens: Option<u64>| DayLimit { cost, tokens };
        assert!(!used.reaches(&limit(None, None)));
        assert!(!used.reaches(&limit(Some(2.01), None)));
        assert!(used.reaches(&limit(Some(2.0), None)));
        assert!(used.reaches(&limit(Some(1.0), None)));
        assert!(!used.reaches(&limit(None, Some(1001))));
        assert!(used.reaches(&limit(None, Some(1000))));
        assert!(used.reaches(&limit(Some(100.0), Some(10))));
        assert!(!DayUse::default().reaches(&limit(Some(0.01), Some(1))));
    }

    #[test]
    fn a_task_counts_towards_the_day_response_by_response() {
        let path = scratch("budget");
        let ledger = Arc::new(Mutex::new(Ledger::load(&path).expect("an empty record")));
        let clock = Arc::new(TestClock(AtomicU64::new(NOON)));
        let limit = DayLimit {
            cost: Some(1.0),
            tokens: None,
        };
        let mut budget = TaskBudget::new(
            Arc::clone(&ledger),
            limit,
            clock.clone(),
            zone("Europe/Rome"),
            Some("USD".to_owned()),
        );
        assert!(!budget.exhausted());
        // The meter reports totals for the task; the day gets the difference.
        budget.used(usage(100, 10), Some(0.4));
        assert!(!budget.exhausted());
        budget.used(usage(250, 30), Some(0.9));
        assert!(!budget.exhausted());
        budget.used(usage(300, 40), Some(1.1));
        assert!(budget.exhausted());
        assert!(!budget.failed());
        let day = ledger.lock().expect("the record").day("2026-11-12");
        assert_eq!(day.input_tokens, 300);
        assert_eq!(day.output_tokens, 40);
        assert_eq!(day.responses, 3);
        assert!((day.cost - 1.1).abs() < 1e-9);

        // Another task on the same day starts where the day stands: it may
        // send nothing.
        let mut second = TaskBudget::new(
            Arc::clone(&ledger),
            limit,
            clock.clone(),
            zone("Europe/Rome"),
            Some("USD".to_owned()),
        );
        assert!(second.exhausted());

        // The next day is a new day. 2026-11-12T23:30:00Z is already the
        // 13th in Rome, and still the 12th in New York.
        clock.0.store(NOON + 13 * 3600 + 1800, Ordering::SeqCst);
        assert!(!second.exhausted());
        let mut elsewhere = TaskBudget::new(
            Arc::clone(&ledger),
            limit,
            clock.clone(),
            zone("America/New_York"),
            None,
        );
        assert!(elsewhere.exhausted());

        // The record on disk is what a restarted service would find.
        let read = Ledger::load(&path).expect("read back");
        assert!(read.day("2026-11-12").reaches(&limit));
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    #[test]
    fn a_clock_set_back_or_a_changed_zone_moves_nothing_between_days() {
        let path = scratch("clock");
        let ledger = Arc::new(Mutex::new(Ledger::load(&path).expect("an empty record")));
        let clock = Arc::new(TestClock(AtomicU64::new(NOON)));
        let limit = DayLimit {
            cost: None,
            tokens: Some(1000),
        };
        let mut budget =
            TaskBudget::new(Arc::clone(&ledger), limit, clock.clone(), zone("UTC"), None);
        budget.used(usage(900, 200), None);
        assert!(budget.exhausted());
        // The clock is set back a day: yesterday has its own, empty, total.
        clock.0.store(NOON - 86_400, Ordering::SeqCst);
        assert!(!budget.exhausted());
        budget.used(usage(950, 200), None);
        // Set forward again: today's total is what it was.
        clock.0.store(NOON, Ordering::SeqCst);
        assert!(budget.exhausted());
        let record = ledger.lock().expect("the record");
        assert_eq!(record.day("2026-11-12").total_tokens(), 1100);
        assert_eq!(record.day("2026-11-11").total_tokens(), 50);
        drop(record);

        // A zone nobody knows reads as UTC rather than as no day at all.
        assert_eq!(day_of(Timestamp(NOON), "Mars/Olympus"), "2026-11-12");
        assert_eq!(day_of(Timestamp(NOON), "Pacific/Kiritimati"), "2026-11-13");
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    #[test]
    fn a_record_that_cannot_be_written_uses_the_day_up() {
        // The record's directory is a file: nothing can be written there.
        let blocker =
            std::env::temp_dir().join(format!("tur-pax-spending-block-{}", std::process::id()));
        let _ = fs::remove_dir_all(&blocker);
        let _ = fs::remove_file(&blocker);
        fs::write(&blocker, "not a directory").expect("written");
        let path = blocker.join("spending.json");
        let ledger = Arc::new(Mutex::new(Ledger {
            path,
            days: BTreeMap::new(),
        }));
        let clock = Arc::new(TestClock(AtomicU64::new(NOON)));
        let limit = DayLimit {
            cost: Some(100.0),
            tokens: None,
        };
        let mut budget = TaskBudget::new(ledger, limit, clock, zone("UTC"), None);
        assert!(!budget.exhausted());
        budget.used(usage(1, 1), Some(0.001));
        assert!(budget.failed());
        assert!(budget.exhausted());
        let _ = fs::remove_file(&blocker);
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The trial of a new deployment: arming it, classifying each boot, making a
//! fallback permanent, marking a boot good and recording a failed gate
//! (design sections 5.5 to 5.9 and 6.4).
//!
//! Two rules shape everything here. The version under trial is not trusted
//! to repair anything: it may say "good", and nothing else. And GRUB's
//! environment block, not any file, is the authority for whether a trial is
//! in progress; the order of writes below keeps every interrupted sequence
//! on a path that ends on a working version.

use crate::codes;
use crate::config::GateConfig;
use crate::host::{Entry, Host};
use crate::json::{self, Value};
use crate::model::{BootArg, BootcStatus, Deployment, EnvBlock, KernelArgs, VAR_COUNTER, VAR_TRIAL_ARGS};
use crate::state::{
    self, Attempt, HISTORY_FILE, NOTICE_FILE, Notice, Phase, REJECTED_FILE, Rejected, TRIAL_FILE,
    TrialRecord,
};

/// Journal message IDs. Fixed, so that `tur-diag` can select these entries
/// without parsing text (specification §5.5).
pub(crate) const MSG_CLASSIFIED: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e01";
pub(crate) const MSG_TRIAL: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e02";
pub(crate) const MSG_FALLBACK: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e03";
pub(crate) const MSG_VERDICT: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e04";
pub(crate) const MSG_STAGE: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e05";
pub(crate) const MSG_PROBLEM: &str = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e06";

/// Runtime files under `/run/tur`.
pub(crate) const RUN_BOOT: &str = "boot-health/boot.json";
pub(crate) const RUN_VERDICT: &str = "boot-health/verdict.json";
/// Exists when this boot is a trial that GRUB could not announce on the
/// kernel command line. The restart units test for it (design section 5.4).
pub(crate) const RUN_TRIAL_FLAG: &str = "boot-trial";

/// The counter value a fallback uses while it changes the boot order
/// (design section 5.9, step 3).
const REVERTING_COUNTER: &str = "2";
/// What GRUB leaves in the block once the attempts are used up.
const EXHAUSTED_COUNTER: &str = "-1";

/// What kind of boot this is (design section 5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Classification {
    Normal,
    Manual,
    Trial,
    Stale,
    Fallback,
    Rejected,
    Inconsistent,
    ManualRetry,
    /// A combination the table does not list. Nothing is changed.
    Unlisted,
}

impl Classification {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Classification::Normal => "normal",
            Classification::Manual => "manual",
            Classification::Trial => "trial",
            Classification::Stale => "stale",
            Classification::Fallback => "fallback",
            Classification::Rejected => "rejected",
            Classification::Inconsistent => "inconsistent",
            Classification::ManualRetry => "manual-retry",
            Classification::Unlisted => "unlisted",
        }
    }
}

/// Which deployment of the trial record is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    /// The deployment under trial.
    New,
    /// The deployment that armed the trial.
    Previous,
    /// Neither of the two the record names.
    Other,
    /// The record does not say, or there is no record.
    Unknown,
}

/// The facts a boot is classified from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    /// `tur.boot=` on the kernel command line.
    pub(crate) arg: BootArg,
    /// The phase of the trial record, if there is a record.
    pub(crate) record: Option<Phase>,
    /// The booted deployment is the one the bootloader starts by default.
    pub(crate) default: bool,
    pub(crate) booted: Which,
    /// The environment block holds a counter.
    pub(crate) counter: bool,
}

/// Where a fallback starts or resumes (design section 5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FallbackFrom {
    /// From step 1.
    Start,
    /// From step 3: the record already says `reverting`.
    Counter,
    /// From step 5: the boot order has already changed.
    Confirm,
}

/// What to do about a boot: one row of the table in design section 5.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Row 1.
    Normal,
    /// Row 2.
    Manual,
    /// Row 3.
    Trial,
    /// Row 3 with a record that names another deployment as the new one.
    TrialOther,
    /// Row 4.
    Stale,
    /// Rows 5 to 8. `loader` is true for row 5.
    Fallback { from: FallbackFrom, loader: bool },
    /// Row 9.
    Rejected,
    /// Row 10.
    TrialUnknown,
    /// Row 11.
    TrialRecovered,
    /// Row 12.
    Inconsistent,
    /// Row 13.
    ManualRetry,
    /// Not in the table.
    Unlisted,
}

impl Decision {
    pub(crate) fn classification(self) -> Classification {
        match self {
            Decision::Normal => Classification::Normal,
            Decision::Manual => Classification::Manual,
            Decision::Trial
            | Decision::TrialOther
            | Decision::TrialUnknown
            | Decision::TrialRecovered => Classification::Trial,
            Decision::Stale => Classification::Stale,
            Decision::Fallback { .. } => Classification::Fallback,
            Decision::Rejected => Classification::Rejected,
            Decision::Inconsistent => Classification::Inconsistent,
            Decision::ManualRetry => Classification::ManualRetry,
            Decision::Unlisted => Classification::Unlisted,
        }
    }
}

/// The table of design section 5.6.
pub(crate) fn decide(facts: &Facts) -> Decision {
    use BootArg as A;
    use Which as W;
    match facts.record {
        Some(Phase::Reverting) => match (facts.booted, facts.default) {
            (W::New, _) => Decision::Rejected,
            (W::Previous, false) => Decision::Fallback {
                from: FallbackFrom::Counter,
                loader: false,
            },
            (W::Previous, true) => Decision::Fallback {
                from: FallbackFrom::Confirm,
                loader: false,
            },
            _ => Decision::Unlisted,
        },
        Some(Phase::Trial) => match (facts.arg, facts.default, facts.booted) {
            (A::Trial, true, W::New) => Decision::Trial,
            (A::Trial, true, W::Previous) => Decision::Stale,
            // The default deployment is one the record does not know: an
            // update staged after the trial was armed. Count it.
            (A::Trial, true, W::Other) => Decision::TrialOther,
            (A::Trial, true, W::Unknown) => Decision::TrialUnknown,
            (A::Trial, false, W::Previous) => Decision::Fallback {
                from: FallbackFrom::Start,
                loader: true,
            },
            (A::Fallback, false, W::Previous | W::Unknown) => Decision::Fallback {
                from: FallbackFrom::Start,
                loader: false,
            },
            (A::None, true, W::New) => Decision::TrialRecovered,
            (A::Fallback, true, W::New) => Decision::ManualRetry,
            _ => Decision::Unlisted,
        },
        None => match (facts.arg, facts.default) {
            (A::None, _) if facts.counter => Decision::Inconsistent,
            (A::None, true) => Decision::Normal,
            (A::None, false) => Decision::Manual,
            (A::Trial, true) => Decision::TrialUnknown,
            (A::Fallback, false) => Decision::Fallback {
                from: FallbackFrom::Start,
                loader: false,
            },
            _ => Decision::Unlisted,
        },
    }
}

/// Which of the record's two deployments is booted.
pub(crate) fn which(record: Option<&TrialRecord>, booted: Option<&Deployment>) -> Which {
    let (Some(record), Some(booted)) = (record, booted) else {
        return Which::Unknown;
    };
    if let Some(new) = &record.new
        && new.same_as(booted)
    {
        return Which::New;
    }
    if let Some(previous) = &record.previous
        && previous.same_as(booted)
    {
        return Which::Previous;
    }
    if record.new.is_some() && record.previous.is_some() {
        Which::Other
    } else {
        Which::Unknown
    }
}

// ---- small helpers ------------------------------------------------------------

pub(crate) fn load_record(host: &dyn Host) -> Option<TrialRecord> {
    host.read_state(TRIAL_FILE)
        .and_then(|text| TrialRecord::parse(&text))
}

fn save_record(host: &mut dyn Host, record: &TrialRecord) -> Result<(), String> {
    host.write_state(TRIAL_FILE, &record.to_json().to_pretty())
}

fn new_event_id(host: &dyn Host) -> String {
    let boot = host.boot_id();
    let short: String = boot.chars().take(8).collect();
    format!("{}-{short}", host.now_unix())
}

/// A journal entry with the fields every entry of this program carries.
pub(crate) fn log(
    host: &mut dyn Host,
    message_id: &'static str,
    priority: u8,
    message: String,
    mut fields: Vec<(String, String)>,
) {
    fields.push(("TUR_BOOT_ID".to_owned(), host.boot_id()));
    fields.push(("TUR_BUILD_ID".to_owned(), host.build_id()));
    host.journal(&Entry {
        message_id,
        priority,
        message,
        fields,
    });
}

fn problem(host: &mut dyn Host, code: &tur_core::ErrorCode, message: String) {
    host.console(&format!("{code}: {message}"));
    log(
        host,
        MSG_PROBLEM,
        3,
        format!("{code}: {message}"),
        vec![("TUR_ERROR_CODE".to_owned(), code.to_string())],
    );
}

/// Adds an event to the history. Best effort: a history that cannot be
/// written never stops a state change.
pub(crate) fn history(
    host: &mut dyn Host,
    event: &str,
    event_id: &str,
    mut members: Vec<(&str, Value)>,
) {
    let mut all: Vec<(&str, Value)> = vec![
        ("schema_version", Value::Int(state::SCHEMA_VERSION)),
        ("event", Value::from(event)),
        ("event_id", Value::from(event_id)),
        ("boot_id", Value::from(host.boot_id())),
        ("build_id", Value::from(host.build_id())),
        ("at", Value::Int(host.now_unix())),
    ];
    all.append(&mut members);
    let existing = host.read_state(HISTORY_FILE).unwrap_or_default();
    let text = state::history_append(&existing, &Value::object(all));
    if let Err(error) = host.write_state(HISTORY_FILE, &text) {
        host.console(&format!("the history could not be written: {error}"));
    }
}

fn write_notice(host: &mut dyn Host, notice: &Notice) {
    if let Err(error) = host.write_state(NOTICE_FILE, &notice.to_json().to_pretty()) {
        host.console(&format!("the notice could not be written: {error}"));
    }
    log(
        host,
        MSG_TRIAL,
        4,
        notice.text.clone(),
        vec![
            ("TUR_ERROR_CODE".to_owned(), notice.error_code.clone()),
            ("TUR_NOTICE".to_owned(), notice.kind.clone()),
            ("TUR_EVENT_ID".to_owned(), notice.event_id.clone()),
        ],
    );
}

fn counter_value(env: &Result<EnvBlock, String>) -> Option<String> {
    env.as_ref()
        .ok()
        .and_then(|block| block.get(VAR_COUNTER).cloned())
}

fn deployment_json(deployment: Option<&Deployment>) -> Value {
    match deployment {
        Some(deployment) => deployment.to_json(),
        None => Value::Null,
    }
}

fn label_of(deployment: Option<&Deployment>) -> String {
    match deployment {
        Some(deployment) => deployment.label(),
        None => "an unknown version".to_owned(),
    }
}

/// What `classify` leaves in `/run` for the gate, `mark-good`,
/// `record-failure` and `status`.
pub(crate) fn boot_info(
    host: &dyn Host,
    classification: Classification,
    args: &KernelArgs,
    booted: Option<&Deployment>,
    event_id: Option<&str>,
) -> Value {
    Value::object(vec![
        ("schema_version", Value::Int(state::SCHEMA_VERSION)),
        ("boot_id", Value::from(host.boot_id())),
        ("classification", Value::from(classification.as_str())),
        ("boot_argument", Value::from(args.boot.as_str())),
        (
            "tries_left",
            args.tries_left.map(Value::Int).unwrap_or(Value::Null),
        ),
        ("contract", Value::opt_str(args.contract.as_deref())),
        (
            "deployment",
            Value::opt_str(booted.map(Deployment::name).as_deref()),
        ),
        (
            "image_digest",
            Value::opt_str(booted.map(|deployment| deployment.digest.as_str())),
        ),
        ("build_id", Value::from(host.build_id())),
        ("event_id", Value::opt_str(event_id)),
    ])
}

/// The classification `classify` recorded for this boot, if it ran.
pub(crate) fn recorded_classification(host: &dyn Host) -> Option<String> {
    let text = host.read_run(RUN_BOOT)?;
    let value = json::parse(&text).ok()?;
    if value.str_or_empty("boot_id") != host.boot_id() {
        return None;
    }
    Some(value.str_or_empty("classification"))
}

// ---- arm (design section 5.5) ---------------------------------------------------

/// Arms a trial for a staged deployment. Runs at shutdown, immediately before
/// OSTree finalises the deployment. Never fails: shutdown is never blocked.
pub(crate) fn arm(host: &mut dyn Host, config: &GateConfig) {
    let status = match host.bootc_status() {
        Ok(status) => status,
        Err(error) => {
            host.console(&format!("arm: nothing done, bootc status failed: {error}"));
            return;
        }
    };
    let Some(staged) = status.staged.clone() else {
        return;
    };
    if staged.download_only {
        host.console("arm: the staged deployment is download-only; nothing to arm");
        return;
    }
    let event_id = new_event_id(host);
    let args = KernelArgs::parse(&host.cmdline());
    let refusal = if !args.contract_ok() {
        Some("the installed GRUB configuration does not count boot attempts".to_owned())
    } else {
        match host.layout() {
            Ok(layout) if layout.counting_supported() => None,
            Ok(layout) => Some(format!(
                "GRUB cannot be relied on to write its environment block on {}",
                layout.describe()
            )),
            Err(error) => Some(format!("the filesystem under /boot/grub2 is unknown: {error}")),
        }
    };
    if let Some(reason) = refusal {
        refuse(host, &event_id, &status, &staged, &reason);
        return;
    }

    // The record first, then the block: a counter without a record still
    // runs a correct trial, a record without a counter protects nothing.
    let record = TrialRecord {
        event_id: event_id.clone(),
        phase: Phase::Trial,
        attempts_allowed: config.attempts,
        armed_boot_id: host.boot_id(),
        armed_at: host.now_unix(),
        trial_args: config.trial_args.clone(),
        previous: status.booted.clone(),
        new: Some(staged.clone()),
        attempts: Vec::new(),
        grub_cannot_save: false,
        reason: None,
    };
    if let Err(error) = save_record(host, &record) {
        host.console(&format!("arm: the trial record could not be written: {error}"));
    }
    let attempts = config.attempts.to_string();
    let written = host.env_set(&[
        (VAR_TRIAL_ARGS, config.trial_args.as_str()),
        (VAR_COUNTER, attempts.as_str()),
    ]);
    if let Err(error) = written {
        // An unprotected trial is the worst outcome: do not apply the update.
        let _ = host.remove_state(TRIAL_FILE);
        refuse(
            host,
            &event_id,
            &status,
            &staged,
            &format!("the boot counter could not be written: {error}"),
        );
        return;
    }
    host.console(&format!(
        "armed: boot_counter={} for {} (event {event_id})",
        config.attempts,
        staged.label()
    ));
    history(
        host,
        "armed",
        &event_id,
        vec![
            ("attempts_allowed", Value::Int(config.attempts)),
            ("previous", deployment_json(status.booted.as_ref())),
            ("new", staged.to_json()),
        ],
    );
    log(
        host,
        MSG_TRIAL,
        5,
        format!(
            "Boot-attempt counting armed for {}: {} attempts",
            staged.label(),
            config.attempts
        ),
        vec![
            ("TUR_EVENT_ID".to_owned(), event_id),
            ("TUR_TRIAL_EVENT".to_owned(), "armed".to_owned()),
            ("TUR_IMAGE_DIGEST".to_owned(), staged.digest.clone()),
        ],
    );
    host.sync_journal();
}

fn refuse(
    host: &mut dyn Host,
    event_id: &str,
    status: &BootcStatus,
    staged: &Deployment,
    reason: &str,
) {
    let locked = host.lock_finalization();
    let text = format!(
        "The update was not applied: this installation cannot count boot attempts. TUR is \
         still on {}.",
        label_of(status.booted.as_ref())
    );
    let mut details = vec![
        ("reason".to_owned(), Value::from(reason)),
        ("refused".to_owned(), staged.to_json()),
        ("finalization_locked".to_owned(), Value::Bool(locked.is_ok())),
    ];
    if let Err(error) = &locked {
        details.push(("lock_error".to_owned(), Value::from(error.as_str())));
    }
    let boot_id = host.boot_id();
    write_notice(
        host,
        &Notice {
            event_id: event_id.to_owned(),
            kind: "refused".to_owned(),
            text,
            error_code: codes::TRIAL_REFUSED.to_string(),
            boot_id,
            details,
        },
    );
    history(
        host,
        "refused",
        event_id,
        vec![
            ("reason", Value::from(reason)),
            ("finalization_locked", Value::Bool(locked.is_ok())),
        ],
    );
    host.console(&format!(
        "{}: update refused: {reason}",
        codes::TRIAL_REFUSED
    ));
    host.sync_journal();
}

// ---- classify (design section 5.6) ------------------------------------------------

/// Classifies this boot and acts on the cases that need action.
pub(crate) fn classify(host: &mut dyn Host, config: &GateConfig) -> Classification {
    let args = KernelArgs::parse(&host.cmdline());
    let mut status = host.bootc_status();
    let mut tries = 0;
    while status.is_err() && tries < 3 {
        host.sleep_ms(2000);
        status = host.bootc_status();
        tries += 1;
    }
    let env = host.env_list();
    let mut record = load_record(host);
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            problem(
                host,
                &codes::STATE_MISMATCH,
                format!("this boot could not be classified, bootc status failed: {error}"),
            );
            return finish(host, Classification::Unlisted, &args, None, None);
        }
    };
    let default = match host.booted_is_default() {
        // bootc must agree: rollbackQueued is true exactly when the booted
        // deployment is not the default one.
        Ok(default) if default != status.rollback_queued => default,
        Ok(default) => {
            problem(
                host,
                &codes::STATE_MISMATCH,
                format!(
                    "ostree says the booted deployment is {}the default, bootc says \
                     rollbackQueued={}; nothing was changed",
                    if default { "" } else { "not " },
                    status.rollback_queued
                ),
            );
            return finish(
                host,
                Classification::Unlisted,
                &args,
                status.booted.as_ref(),
                None,
            );
        }
        Err(error) => {
            problem(
                host,
                &codes::STATE_MISMATCH,
                format!("the boot order could not be read: {error}; nothing was changed"),
            );
            return finish(
                host,
                Classification::Unlisted,
                &args,
                status.booted.as_ref(),
                None,
            );
        }
    };
    let facts = Facts {
        arg: args.boot,
        record: record.as_ref().map(|record| record.phase),
        default,
        booted: which(record.as_ref(), status.booted.as_ref()),
        counter: counter_value(&env).is_some(),
    };
    let decision = decide(&facts);
    host.console(&format!(
        "classify: tur.boot={} tries_left={:?} record={} default={} booted={:?} counter={:?} -> {}",
        args.boot.as_str(),
        args.tries_left,
        facts
            .record
            .map(Phase::as_str)
            .unwrap_or("none"),
        default,
        facts.booted,
        counter_value(&env),
        decision.classification().as_str()
    ));

    match decision {
        Decision::Normal | Decision::Manual | Decision::ManualRetry => {}
        Decision::Trial | Decision::TrialOther | Decision::TrialUnknown => {
            let mut current = match (decision, record.take()) {
                (Decision::Trial, Some(record)) => record,
                (Decision::TrialOther, Some(mut record)) => {
                    record.new = status.booted.clone();
                    record
                }
                (_, existing) => {
                    // Row 10: armed, but nothing says by whom or for what.
                    let mut record = existing.unwrap_or(TrialRecord {
                        event_id: new_event_id(host),
                        phase: Phase::Trial,
                        attempts_allowed: config.attempts,
                        armed_boot_id: String::new(),
                        armed_at: 0,
                        trial_args: String::new(),
                        previous: None,
                        new: None,
                        attempts: Vec::new(),
                        grub_cannot_save: false,
                        reason: None,
                    });
                    record.new = status.booted.clone();
                    if record.previous.is_none() {
                        record.previous = status.rollback.clone();
                    }
                    record
                }
            };
            note_attempt(host, &mut current, &args, &env);
            record = Some(current);
        }
        Decision::TrialRecovered => {
            if let Some(mut current) = record.take() {
                recover_block(host, &mut current, &env);
                record = Some(current);
            }
        }
        Decision::Stale => {
            if let Some(current) = record.take() {
                stale(host, &current, &status);
            }
        }
        Decision::Fallback { from, loader } => {
            fall_back(host, config, record.take(), &status, from, loader);
        }
        Decision::Rejected => {
            // An interrupted fallback started the failed version again. It
            // must not run, and it cannot be marked good.
            if !default {
                let _ = host.env_unset(&[VAR_COUNTER]);
            }
            host.console("classify: this version was rejected; restarting");
            host.sync_journal();
            if let Err(error) = host.reboot() {
                host.console(&format!("classify: the restart failed: {error}"));
            }
        }
        Decision::Inconsistent => {
            let removed = host.env_unset(&[VAR_COUNTER, VAR_TRIAL_ARGS]);
            problem(
                host,
                &codes::STATE_STRAY_COUNTER,
                format!(
                    "a boot counter existed without a trial; {}",
                    match removed {
                        Ok(()) => "removed".to_owned(),
                        Err(error) => format!("removing it failed: {error}"),
                    }
                ),
            );
        }
        Decision::Unlisted => {
            problem(
                host,
                &codes::STATE_MISMATCH,
                format!(
                    "tur.boot={}, record {}, default {}, booted {:?}: not a known state; \
                     nothing was changed",
                    args.boot.as_str(),
                    facts.record.map(Phase::as_str).unwrap_or("none"),
                    default,
                    facts.booted
                ),
            );
        }
    }
    let event_id = record.as_ref().map(|record| record.event_id.clone());
    finish(
        host,
        decision.classification(),
        &args,
        status.booted.as_ref(),
        event_id.as_deref(),
    )
}

fn finish(
    host: &mut dyn Host,
    classification: Classification,
    args: &KernelArgs,
    booted: Option<&Deployment>,
    event_id: Option<&str>,
) -> Classification {
    let info = boot_info(host, classification, args, booted, event_id);
    if let Err(error) = host.write_run(RUN_BOOT, &info.to_pretty()) {
        host.console(&format!("classify: {RUN_BOOT} could not be written: {error}"));
    }
    log(
        host,
        MSG_CLASSIFIED,
        6,
        format!("Boot classified as {}", classification.as_str()),
        vec![
            ("TUR_BOOT_MODE".to_owned(), classification.as_str().to_owned()),
            (
                "TUR_BOOT_TRIES_LEFT".to_owned(),
                args.tries_left
                    .map(|tries| tries.to_string())
                    .unwrap_or_default(),
            ),
            (
                "TUR_DEPLOYMENT".to_owned(),
                booted.map(Deployment::name).unwrap_or_default(),
            ),
            (
                "TUR_IMAGE_DIGEST".to_owned(),
                booted
                    .map(|deployment| deployment.digest.clone())
                    .unwrap_or_default(),
            ),
            (
                "TUR_EVENT_ID".to_owned(),
                event_id.unwrap_or_default().to_owned(),
            ),
        ],
    );
    if classification != Classification::Normal {
        host.sync_journal();
    }
    classification
}

/// Rows 3 and 10: record this attempt, and notice when GRUB did not save the
/// counter it announced.
fn note_attempt(
    host: &mut dyn Host,
    record: &mut TrialRecord,
    args: &KernelArgs,
    env: &Result<EnvBlock, String>,
) {
    let boot_id = host.boot_id();
    if record.attempt_mut(&boot_id).is_none() {
        record.attempts.push(Attempt {
            boot_id: boot_id.clone(),
            tries_left: args.tries_left,
            verdict: "pending".to_owned(),
            failed_check: None,
            error_code: None,
            detail: None,
        });
    }
    let stored = counter_value(env).and_then(|text| text.parse::<i64>().ok());
    if args.tries_left.is_some() && stored != args.tries_left && !record.grub_cannot_save {
        record.grub_cannot_save = true;
        problem(
            host,
            &codes::TRIAL_COUNTED_IN_USERSPACE,
            format!(
                "GRUB announced {:?} tries left but the environment block holds {:?}: GRUB \
                 could not save the counter, so attempts are counted here",
                args.tries_left, stored
            ),
        );
    }
    if let Err(error) = save_record(host, record) {
        host.console(&format!("classify: the trial record could not be written: {error}"));
    }
    history(
        host,
        "attempt",
        &record.event_id.clone(),
        vec![
            (
                "tries_left",
                args.tries_left.map(Value::Int).unwrap_or(Value::Null),
            ),
            ("attempt", Value::Int(record.attempts.len() as i64)),
        ],
    );
}

/// Row 11: the block was unreadable to GRUB, so GRUB counted nothing and
/// passed no trial arguments. Put the counter back and carry on as a trial.
fn recover_block(host: &mut dyn Host, record: &mut TrialRecord, env: &Result<EnvBlock, String>) {
    if env.is_err() {
        let created = host.env_create();
        problem(
            host,
            &codes::STATE_BLOCK_RECREATED,
            format!(
                "GRUB's environment block was invalid during a trial; {}",
                match created {
                    Ok(()) => "recreated".to_owned(),
                    Err(error) => format!("recreating it failed: {error}"),
                }
            ),
        );
    }
    let used = record.attempts.len() as i64 + 1;
    let remaining = (record.attempts_allowed - used).max(0);
    let remaining_text = remaining.to_string();
    let written = host.env_set(&[
        (VAR_TRIAL_ARGS, record.trial_args.as_str()),
        (VAR_COUNTER, remaining_text.as_str()),
    ]);
    if let Err(error) = written {
        host.console(&format!("classify: the counter could not be restored: {error}"));
    }
    if let Err(error) = host.write_run(RUN_TRIAL_FLAG, "trial\n") {
        host.console(&format!("classify: {RUN_TRIAL_FLAG} could not be written: {error}"));
    }
    let boot_id = host.boot_id();
    if record.attempt_mut(&boot_id).is_none() {
        record.attempts.push(Attempt {
            boot_id,
            tries_left: Some(remaining),
            verdict: "pending".to_owned(),
            failed_check: None,
            error_code: None,
            detail: None,
        });
    }
    if let Err(error) = save_record(host, record) {
        host.console(&format!("classify: the trial record could not be written: {error}"));
    }
    history(
        host,
        "attempt",
        &record.event_id.clone(),
        vec![
            ("tries_left", Value::Int(remaining)),
            ("attempt", Value::Int(record.attempts.len() as i64)),
            ("counter_restored", Value::Bool(true)),
        ],
    );
}

/// Row 4: the trial was armed but the update was never finalised.
fn stale(host: &mut dyn Host, record: &TrialRecord, status: &BootcStatus) {
    if let Err(error) = host.env_unset(&[VAR_COUNTER, VAR_TRIAL_ARGS]) {
        host.console(&format!("classify: the counter could not be removed: {error}"));
    }
    let _ = host.remove_state(TRIAL_FILE);
    let text = format!(
        "The update was not applied. TUR is still on {}.",
        label_of(status.booted.as_ref())
    );
    let boot_id = host.boot_id();
    write_notice(
        host,
        &Notice {
            event_id: record.event_id.clone(),
            kind: "not-applied".to_owned(),
            text,
            error_code: codes::TRIAL_NOT_APPLIED.to_string(),
            boot_id,
            details: vec![
                ("running".to_owned(), deployment_json(status.booted.as_ref())),
                ("not_applied".to_owned(), deployment_json(record.new.as_ref())),
            ],
        },
    );
    history(
        host,
        "stale",
        &record.event_id,
        vec![("not_applied", deployment_json(record.new.as_ref()))],
    );
}

// ---- fallback (design section 5.9) ---------------------------------------------------

/// Makes GRUB's choice of the previous deployment permanent. Run by the
/// previous deployment, never by the one that failed.
fn fall_back(
    host: &mut dyn Host,
    config: &GateConfig,
    record: Option<TrialRecord>,
    status: &BootcStatus,
    from: FallbackFrom,
    loader: bool,
) {
    let mut from = from;
    // Steps 1 and 2: the picture must match, then say so durably.
    let mut record = match record {
        Some(record) => record,
        None => TrialRecord {
            event_id: new_event_id(host),
            phase: Phase::Trial,
            attempts_allowed: config.attempts,
            armed_boot_id: String::new(),
            armed_at: 0,
            trial_args: String::new(),
            previous: None,
            new: None,
            attempts: Vec::new(),
            grub_cannot_save: false,
            reason: None,
        },
    };
    if from == FallbackFrom::Start {
        if !status.rollback_queued {
            problem(
                host,
                &codes::STATE_MISMATCH,
                "a fallback was announced but the booted deployment is the default; nothing \
                 was changed"
                    .to_owned(),
            );
            return;
        }
        if record.previous.is_none() {
            record.previous = status.booted.clone();
        }
        if record.new.is_none() {
            record.new = status.rollback.clone();
        }
        record.phase = Phase::Reverting;
        record.reason = Some(if loader { "loader" } else { "attempts" }.to_owned());
        if let Err(error) = save_record(host, &record) {
            host.console(&format!("fallback: the trial record could not be written: {error}"));
        }
        host.console("fallback: step 2 done, record says reverting");
        from = FallbackFrom::Counter;
    }
    if from == FallbackFrom::Counter {
        // Step 3. A positive counter means "start the first entry and count
        // it", which is safe before and after the order changes.
        if let Err(error) = host.env_set(&[(VAR_COUNTER, REVERTING_COUNTER)]) {
            problem(
                host,
                &codes::STATE_ROLLBACK_FAILED,
                format!("the counter could not be made positive: {error}; the boot order was not changed"),
            );
            return;
        }
        host.console("fallback: step 3 done, boot_counter=2");
        // Step 4.
        if let Err(error) = host.rollback() {
            let still_second = matches!(host.booted_is_default(), Ok(false));
            if still_second {
                // The order is unchanged: put back what GRUB left, so that it
                // keeps starting this version.
                let _ = host.env_set(&[(VAR_COUNTER, EXHAUSTED_COUNTER)]);
            }
            problem(
                host,
                &codes::STATE_ROLLBACK_FAILED,
                format!("bootc rollback failed: {error}"),
            );
            return;
        }
        host.console("fallback: step 4 done, boot order changed");
    }
    // Step 5.
    match host.booted_is_default() {
        Ok(true) => {}
        Ok(false) => {
            problem(
                host,
                &codes::STATE_ROLLBACK_FAILED,
                "bootc rollback reported success but the booted deployment is still not the \
                 default"
                    .to_owned(),
            );
            return;
        }
        Err(error) => {
            problem(
                host,
                &codes::STATE_ROLLBACK_FAILED,
                format!("the boot order could not be confirmed: {error}"),
            );
            return;
        }
    }
    // Step 6.
    if let Err(error) = host.env_unset(&[VAR_COUNTER, VAR_TRIAL_ARGS]) {
        problem(
            host,
            &codes::STATE_ROLLBACK_FAILED,
            format!("the counter could not be removed after the rollback: {error}"),
        );
        return;
    }
    host.console("fallback: step 6 done, counter removed");
    // Step 7.
    let loader = record.reason.as_deref() == Some("loader");
    let code = if loader {
        codes::TRIAL_FALLBACK_LOADER
    } else {
        codes::TRIAL_FALLBACK
    };
    let reached = record.attempts.len() as i64;
    let failure = record.first_failure().cloned();
    if let Some(new) = &record.new {
        let mut list = state::rejected_parse(&host.read_state(REJECTED_FILE).unwrap_or_default());
        state::rejected_add(
            &mut list,
            Rejected {
                digest: new.digest.clone(),
                image: new.image.clone(),
                version: new.version.clone(),
                event_id: record.event_id.clone(),
            },
        );
        if let Err(error) =
            host.write_state(REJECTED_FILE, &state::rejected_to_json(&list).to_pretty())
        {
            host.console(&format!("fallback: the rejected list could not be written: {error}"));
        }
    }
    let how = if loader {
        "could not be started by the boot loader".to_owned()
    } else {
        format!(
            "did not start correctly after {} attempts",
            record.attempts_allowed
        )
    };
    let text = format!(
        "The update to TUR {} {how}. TUR has restored the previous version, {}. Your files \
         were not changed. System settings changed while the failed version was running were \
         not kept. The update will not be tried again automatically.",
        label_of(record.new.as_ref()),
        label_of(record.previous.as_ref())
    );
    let failed_check = failure.as_ref().and_then(|attempt| attempt.failed_check.clone());
    let failed_code = failure.as_ref().and_then(|attempt| attempt.error_code.clone());
    let failed_detail = failure.as_ref().and_then(|attempt| attempt.detail.clone());
    let details: Vec<(String, Value)> = vec![
        ("reason".to_owned(), Value::opt_str(record.reason.as_deref())),
        ("attempts_allowed".to_owned(), Value::Int(record.attempts_allowed)),
        ("attempts_that_reached_userspace".to_owned(), Value::Int(reached)),
        ("first_failed_check".to_owned(), Value::opt_str(failed_check.as_deref())),
        ("first_failed_check_code".to_owned(), Value::opt_str(failed_code.as_deref())),
        ("first_failed_check_detail".to_owned(), Value::opt_str(failed_detail.as_deref())),
        ("restored".to_owned(), deployment_json(record.previous.as_ref())),
        ("failed".to_owned(), deployment_json(record.new.as_ref())),
        (
            "attempt_boot_ids".to_owned(),
            Value::Array(
                record
                    .attempts
                    .iter()
                    .map(|attempt| Value::from(attempt.boot_id.as_str()))
                    .collect(),
            ),
        ),
        ("armed_boot_id".to_owned(), Value::from(record.armed_boot_id.as_str())),
        (
            "evidence".to_owned(),
            Value::from(format!(
                "/var/lib/tur/boot-health/evidence/{}",
                record.event_id
            )),
        ),
    ];
    let boot_id = host.boot_id();
    // An interrupted step 7 is done again: say it in the history once.
    let already = state::history_events(&host.read_state(HISTORY_FILE).unwrap_or_default())
        .iter()
        .any(|event| {
            event.str_or_empty("event") == "fallback"
                && event.str_or_empty("event_id") == record.event_id
        });
    if !already {
        history(
            host,
            "fallback",
            &record.event_id,
            details
                .iter()
                .map(|(name, value)| (name.as_str(), value.clone()))
                .collect(),
        );
    }
    write_notice(
        host,
        &Notice {
            event_id: record.event_id.clone(),
            kind: "fallback".to_owned(),
            text: text.clone(),
            error_code: code.to_string(),
            boot_id,
            details,
        },
    );
    log(
        host,
        MSG_FALLBACK,
        3,
        format!("{code}: {text}"),
        vec![
            ("TUR_ERROR_CODE".to_owned(), code.to_string()),
            ("TUR_EVENT_ID".to_owned(), record.event_id.clone()),
            (
                "TUR_FAILED_IMAGE_DIGEST".to_owned(),
                record
                    .new
                    .as_ref()
                    .map(|new| new.digest.clone())
                    .unwrap_or_default(),
            ),
            (
                "TUR_FAILED_DEPLOYMENT".to_owned(),
                record.new.as_ref().map(Deployment::name).unwrap_or_default(),
            ),
            ("TUR_ATTEMPTS_REACHED_USERSPACE".to_owned(), reached.to_string()),
            ("TUR_FALLBACK_REASON".to_owned(), record.reason.clone().unwrap_or_default()),
            ("TUR_FAILED_CHECK".to_owned(), failed_check.unwrap_or_default()),
        ],
    );
    if let Err(error) = host.remove_state(TRIAL_FILE) {
        host.console(&format!("fallback: the trial record could not be removed: {error}"));
    }
    host.console(&format!(
        "fallback: step 7 done, {} restored (event {})",
        label_of(record.previous.as_ref()),
        record.event_id
    ));
    host.sync_journal();
}

// ---- mark-good (design section 6.4) ---------------------------------------------------

/// Whether the gate's verdict for this boot lets it be marked good.
fn verdict_passed(host: &dyn Host) -> Result<(), String> {
    let Some(text) = host.read_run(RUN_VERDICT) else {
        return Err("the gate has left no verdict for this boot".to_owned());
    };
    let value = json::parse(&text).map_err(|error| format!("the verdict is unreadable: {error}"))?;
    if value.str_or_empty("boot_id") != host.boot_id() {
        return Err("the verdict is from another boot".to_owned());
    }
    match value.str_or_empty("result").as_str() {
        "passed" | "degraded" => Ok(()),
        other => Err(format!("the gate's verdict is {other:?}")),
    }
}

/// Marks this boot good: removes the counter, ends the trial. Refuses unless
/// the gate passed in this boot.
pub(crate) fn mark_good(host: &mut dyn Host) -> Result<(), String> {
    verdict_passed(host)?;
    let args = KernelArgs::parse(&host.cmdline());
    let classification = match recorded_classification(host) {
        Some(classification) => classification,
        // `classify` did not run. The kernel command line still says whether
        // GRUB counted this boot.
        None if args.boot == BootArg::Trial => "trial".to_owned(),
        None => "unknown".to_owned(),
    };
    let in_trial = classification == "trial" || classification == "manual-retry";
    if in_trial {
        // The block is the authority: remove the counter first.
        host.env_unset(&[VAR_COUNTER, VAR_TRIAL_ARGS])?;
        host.console("mark-good: counter removed");
        let record = load_record(host);
        let event_id = record
            .as_ref()
            .map(|record| record.event_id.clone())
            .unwrap_or_default();
        host.remove_state(TRIAL_FILE)?;
        history(
            host,
            "marked-good",
            &event_id,
            vec![
                (
                    "attempts_used",
                    Value::Int(record.as_ref().map(|r| r.attempts.len() as i64).unwrap_or(0)),
                ),
                (
                    "kept",
                    deployment_json(record.as_ref().and_then(|record| record.new.as_ref())),
                ),
                ("classification", Value::from(classification.as_str())),
            ],
        );
        log(
            host,
            MSG_TRIAL,
            5,
            "The new version passed its boot health gate and is kept".to_owned(),
            vec![
                ("TUR_EVENT_ID".to_owned(), event_id),
                ("TUR_TRIAL_EVENT".to_owned(), "marked-good".to_owned()),
            ],
        );
    }
    let uptime_usec = host.uptime_ms() * 1000;
    log(
        host,
        MSG_STAGE,
        6,
        "Boot stage completed: boot marked successful".to_owned(),
        vec![
            ("TUR_BOOT_STAGE".to_owned(), "BOOT-150".to_owned()),
            ("TUR_BOOT_STAGE_STATE".to_owned(), "succeeded".to_owned()),
            ("TUR_BOOT_MODE".to_owned(), classification.clone()),
            ("TUR_STAGE_MONOTONIC_USEC".to_owned(), uptime_usec.to_string()),
        ],
    );
    host.console(&format!("mark-good: BOOT-150 recorded ({classification})"));
    if in_trial {
        host.sync_journal();
    }
    Ok(())
}

// ---- record-failure (design sections 5.6 and 6.1) -----------------------------------------

/// Records that the gate failed. In a trial the restart that follows is plain
/// systemd; outside a trial the user is told and the system stays up.
pub(crate) fn record_failure(host: &mut dyn Host) {
    let args = KernelArgs::parse(&host.cmdline());
    let verdict = host
        .read_run(RUN_VERDICT)
        .and_then(|text| json::parse(&text).ok())
        .filter(|value| value.str_or_empty("boot_id") == host.boot_id());
    let (check, code, detail) = match &verdict {
        Some(value) => (
            value.str_or_empty("failed_check"),
            value.str_or_empty("error_code"),
            value.str_or_empty("failed_detail"),
        ),
        None => (
            "gate".to_owned(),
            codes::HEALTH_GATE_DIED.to_string(),
            "the gate left no verdict: it was killed, timed out or could not run".to_owned(),
        ),
    };
    let classification = recorded_classification(host).unwrap_or_else(|| "unknown".to_owned());
    let trial_flag = host.read_run(RUN_TRIAL_FLAG).is_some();
    let will_restart = args.boot == BootArg::Trial || trial_flag;
    host.console(&format!(
        "record-failure: check {check} failed ({code}): {detail}; classification \
         {classification}; restart follows: {will_restart}"
    ));
    let mut event_id = String::new();
    if let Some(mut record) = load_record(host)
        && record.phase == Phase::Trial
        && (classification == "trial" || classification == "manual-retry" || will_restart)
    {
        event_id = record.event_id.clone();
        let boot_id = host.boot_id();
        if record.attempt_mut(&boot_id).is_none() {
            record.attempts.push(Attempt {
                boot_id: boot_id.clone(),
                tries_left: args.tries_left,
                verdict: "pending".to_owned(),
                failed_check: None,
                error_code: None,
                detail: None,
            });
        }
        if let Some(attempt) = record.attempt_mut(&boot_id) {
            attempt.verdict = "failed".to_owned();
            attempt.failed_check = Some(check.clone());
            attempt.error_code = Some(code.clone());
            attempt.detail = Some(detail.clone());
        }
        let exhausted = record.attempts.len() as i64 >= record.attempts_allowed;
        if record.grub_cannot_save && exhausted && will_restart {
            // The one exception to "the version under trial repairs nothing":
            // GRUB cannot count, so it will never choose the previous
            // version. Userspace is alive, so it does (design section 5.6).
            record.phase = Phase::Reverting;
            record.reason = Some("attempts".to_owned());
            let _ = save_record(host, &record);
            match host.rollback() {
                Ok(()) => host.console(
                    "record-failure: attempts used up and GRUB cannot count; boot order changed",
                ),
                Err(error) => problem(
                    host,
                    &codes::STATE_ROLLBACK_FAILED,
                    format!("bootc rollback from the failing version failed: {error}"),
                ),
            }
        } else if let Err(error) = save_record(host, &record) {
            host.console(&format!(
                "record-failure: the trial record could not be written: {error}"
            ));
        }
    }
    history(
        host,
        "gate-failed",
        &event_id,
        vec![
            ("failed_check", Value::from(check.as_str())),
            ("error_code", Value::from(code.as_str())),
            ("detail", Value::from(detail.as_str())),
            ("classification", Value::from(classification.as_str())),
            ("restart_follows", Value::Bool(will_restart)),
        ],
    );
    if !will_restart {
        let event = new_event_id(host);
        let boot_id = host.boot_id();
        write_notice(
            host,
            &Notice {
                event_id: event,
                kind: "unhealthy".to_owned(),
                text: "TUR started, but some checks failed. Diagnostics are available."
                    .to_owned(),
                error_code: code.clone(),
                boot_id,
                details: vec![
                    ("first_failed_check".to_owned(), Value::from(check.as_str())),
                    ("first_failed_check_detail".to_owned(), Value::from(detail.as_str())),
                ],
            },
        );
    }
    log(
        host,
        MSG_VERDICT,
        3,
        format!("Boot health gate failed: {check} ({code}): {detail}"),
        vec![
            ("TUR_ERROR_CODE".to_owned(), code),
            ("TUR_FAILED_CHECK".to_owned(), check),
            ("TUR_BOOT_MODE".to_owned(), classification),
            ("TUR_RESTART_FOLLOWS".to_owned(), will_restart.to_string()),
        ],
    );
    host.sync_journal();
}

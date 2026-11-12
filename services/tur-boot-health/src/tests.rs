// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The trial state machine and the gate, run against a machine in memory.
//!
//! These tests follow the design document: every row of the classification
//! table (section 5.6), every row of the interruption table (5.9), the order
//! of writes (5.2), the sequences of a good and a bad boot (5.7, 5.8), the
//! loops that must not happen (5.10) and the required checks (6.2).

use crate::config::GateConfig;
use crate::fake::{At, FakeHost};
use crate::gate::{self, CheckState};
use crate::host::Host;
use crate::json::{self, Value};
use crate::model::{BootArg, VAR_COUNTER, VAR_TRIAL_ARGS};
use crate::state::{self, HISTORY_FILE, NOTICE_FILE, Phase, REJECTED_FILE, TRIAL_FILE};
use crate::status;
use crate::trial::{
    self, Classification, Decision, Facts, FallbackFrom, RUN_BOOT, RUN_TRIAL_FLAG, Which, arm,
    classify, decide, mark_good, record_failure,
};

fn config() -> GateConfig {
    GateConfig::default()
}

/// A clean restart: the trial is armed if something is staged, OSTree
/// finalises, GRUB runs.
fn restart(host: &mut FakeHost) {
    arm(host, &config());
    host.finalize();
    host.grub();
}

/// The userspace of a boot whose gate fails.
fn failing_boot(host: &mut FakeHost) -> Classification {
    let classification = classify(host, &config());
    if classification == Classification::Trial || classification == Classification::ManualRetry {
        host.set_verdict("failed", Some("R5"));
        record_failure(host);
    }
    classification
}

/// The userspace of a boot whose gate passes.
fn good_boot(host: &mut FakeHost) -> Classification {
    let classification = classify(host, &config());
    host.set_verdict("passed", None);
    mark_good(host).expect("mark-good");
    classification
}

fn events(host: &FakeHost) -> Vec<String> {
    state::history_events(&host.read_state(HISTORY_FILE).unwrap_or_default())
        .iter()
        .map(|event| event.str_or_empty("event"))
        .collect()
}

fn last_event(host: &FakeHost, name: &str) -> Value {
    state::history_events(&host.read_state(HISTORY_FILE).unwrap_or_default())
        .into_iter()
        .rev()
        .find(|event| event.str_or_empty("event") == name)
        .unwrap_or_else(|| panic!("no {name} event in {:?}", events(host)))
}

fn notice(host: &FakeHost) -> Value {
    json::parse(&host.read_state(NOTICE_FILE).expect("a notice")).expect("notice JSON")
}

fn rejected(host: &FakeHost) -> Vec<String> {
    state::rejected_parse(&host.read_state(REJECTED_FILE).unwrap_or_default())
        .into_iter()
        .map(|item| item.digest)
        .collect()
}

/// A machine whose update to `new` has just been finalised and is about to
/// have its first attempt started by GRUB.
fn updated() -> FakeHost {
    let mut host = FakeHost::new();
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
    host.stage("new", "2.0");
    restart(&mut host);
    host
}

/// Asserts the end state after a fallback: the previous version runs and is
/// the default, nothing is left of the trial, and the failed image is
/// rejected.
fn assert_restored(host: &FakeHost) {
    assert_eq!(host.booted_checksum(), "old", "the previous version runs");
    assert_eq!(host.default_checksum(), "old", "and is the default");
    assert_eq!(host.counter(), None, "no counter is left");
    let env = host.env.as_ref().expect("a valid block");
    assert!(
        !env.contains_key(VAR_TRIAL_ARGS),
        "no trial arguments are left"
    );
    assert!(
        host.read_state(TRIAL_FILE).is_none(),
        "no trial record is left"
    );
    assert_eq!(rejected(host), vec!["sha256:new".to_owned()]);
    assert_eq!(notice(host).str_or_empty("kind"), "fallback");
}

// ---- the classification table, design section 5.6 ---------------------------------

fn facts(
    arg: BootArg,
    record: Option<Phase>,
    default: bool,
    booted: Which,
    counter: bool,
) -> Facts {
    Facts {
        arg,
        record,
        default,
        booted,
        counter,
    }
}

#[test]
fn every_row_of_the_classification_table() {
    use BootArg as A;
    use Which as W;
    let trial = Some(Phase::Trial);
    let reverting = Some(Phase::Reverting);
    let start = |loader: bool| Decision::Fallback {
        from: FallbackFrom::Start,
        loader,
    };
    let rows: Vec<(&str, Facts, Decision, Classification)> = vec![
        (
            "1",
            facts(A::None, None, true, W::Unknown, false),
            Decision::Normal,
            Classification::Normal,
        ),
        (
            "2",
            facts(A::None, None, false, W::Unknown, false),
            Decision::Manual,
            Classification::Manual,
        ),
        (
            "3",
            facts(A::Trial, trial, true, W::New, true),
            Decision::Trial,
            Classification::Trial,
        ),
        (
            "4",
            facts(A::Trial, trial, true, W::Previous, true),
            Decision::Stale,
            Classification::Stale,
        ),
        (
            "4, when GRUB acted on a block from before the trial was armed",
            facts(A::None, trial, true, W::Previous, true),
            Decision::Stale,
            Classification::Stale,
        ),
        (
            "5",
            facts(A::Trial, trial, false, W::Previous, true),
            start(true),
            Classification::Fallback,
        ),
        (
            "6",
            facts(A::Fallback, trial, false, W::Previous, true),
            start(false),
            Classification::Fallback,
        ),
        (
            "6, record without identities",
            facts(A::Fallback, trial, false, W::Unknown, true),
            start(false),
            Classification::Fallback,
        ),
        (
            "6, record missing",
            facts(A::Fallback, None, false, W::Unknown, true),
            start(false),
            Classification::Fallback,
        ),
        (
            "7",
            facts(A::Fallback, reverting, false, W::Previous, true),
            Decision::Fallback {
                from: FallbackFrom::Counter,
                loader: false,
            },
            Classification::Fallback,
        ),
        (
            "7, any argument",
            facts(A::Trial, reverting, false, W::Previous, true),
            Decision::Fallback {
                from: FallbackFrom::Counter,
                loader: false,
            },
            Classification::Fallback,
        ),
        (
            "8",
            facts(A::Trial, reverting, true, W::Previous, true),
            Decision::Fallback {
                from: FallbackFrom::Confirm,
                loader: false,
            },
            Classification::Fallback,
        ),
        (
            "8, any argument",
            facts(A::None, reverting, true, W::Previous, false),
            Decision::Fallback {
                from: FallbackFrom::Confirm,
                loader: false,
            },
            Classification::Fallback,
        ),
        (
            "9, default",
            facts(A::Trial, reverting, true, W::New, true),
            Decision::Rejected,
            Classification::Rejected,
        ),
        (
            "9, not default",
            facts(A::Fallback, reverting, false, W::New, true),
            Decision::Rejected,
            Classification::Rejected,
        ),
        (
            "10",
            facts(A::Trial, None, true, W::Unknown, true),
            Decision::TrialUnknown,
            Classification::Trial,
        ),
        (
            "11",
            facts(A::None, trial, true, W::New, false),
            Decision::TrialRecovered,
            Classification::Trial,
        ),
        (
            "12",
            facts(A::None, None, true, W::Unknown, true),
            Decision::Inconsistent,
            Classification::Inconsistent,
        ),
        (
            "12, not default",
            facts(A::None, None, false, W::Unknown, true),
            Decision::Inconsistent,
            Classification::Inconsistent,
        ),
        (
            "13",
            facts(A::Fallback, trial, true, W::New, true),
            Decision::ManualRetry,
            Classification::ManualRetry,
        ),
    ];
    for (row, facts, decision, classification) in rows {
        assert_eq!(decide(&facts), decision, "row {row}");
        assert_eq!(decide(&facts).classification(), classification, "row {row}");
    }
}

#[test]
fn combinations_the_table_does_not_list_change_nothing() {
    use BootArg as A;
    use Which as W;
    let trial = Some(Phase::Trial);
    let reverting = Some(Phase::Reverting);
    for facts in [
        facts(A::Trial, None, false, W::Unknown, true),
        facts(A::Fallback, None, true, W::Unknown, true),
        facts(A::Fallback, trial, false, W::New, true),
        facts(A::Fallback, trial, false, W::Other, true),
        facts(A::Trial, trial, false, W::New, true),
        facts(A::None, trial, false, W::Previous, false),
        facts(A::Fallback, reverting, false, W::Unknown, true),
        facts(A::None, reverting, true, W::Other, false),
    ] {
        assert_eq!(decide(&facts), Decision::Unlisted, "{facts:?}");
    }
    // Acted out: nothing is written to the block or the boot order.
    let mut host = updated();
    host.cmdline = host.cmdline.replace("tur.boot=trial", "tur.boot=fallback");
    host.state.remove(TRIAL_FILE);
    host.booted = 0;
    let before = host.ops.len();
    assert_eq!(classify(&mut host, &config()), Classification::Unlisted);
    assert_eq!(host.ops.len(), before, "{:?}", host.ops);
    assert!(
        host.journal
            .iter()
            .any(|entry| entry.message.contains("TUR-BOOT-STATE-001"))
    );
}

#[test]
fn a_default_deployment_the_record_does_not_know_is_still_counted() {
    // An update staged after the trial was armed replaces the staged one.
    let facts = facts(BootArg::Trial, Some(Phase::Trial), true, Which::Other, true);
    assert_eq!(decide(&facts), Decision::TrialOther);
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    host.stage("newer", "2.1");
    host.finalize();
    host.grub();
    assert_eq!(classify(&mut host, &config()), Classification::Trial);
    let record = trial::load_record(&host).expect("a record");
    assert_eq!(record.new.expect("new").checksum, "newer");
    assert_eq!(record.attempts.len(), 1);
}

// ---- sequence on a good boot, design section 5.7 -------------------------------------

#[test]
fn a_good_update_is_kept_and_marked_good() {
    let mut host = updated();
    // GRUB counted the first attempt before it started the kernel.
    assert_eq!(host.counter().as_deref(), Some("2"));
    assert!(host.cmdline.contains(
        "tur.boot_contract=1 tur.boot=trial tur.boot_tries_left=2 panic=30 rd.shell=0 \
         rd.emergency=reboot systemd.watchdog_sec=180"
    ));
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(good_boot(&mut host), Classification::Trial);
    assert_eq!(host.counter(), None);
    assert!(!host.env.as_ref().unwrap().contains_key(VAR_TRIAL_ARGS));
    assert!(host.read_state(TRIAL_FILE).is_none());
    assert_eq!(events(&host), vec!["armed", "attempt", "marked-good"]);
    assert!(host.read_state(NOTICE_FILE).is_none());
    assert!(rejected(&host).is_empty());
    assert!(host.journal.iter().any(|entry| {
        entry
            .fields
            .contains(&("TUR_BOOT_STAGE".to_owned(), "BOOT-150".to_owned()))
    }));
    // The next boot is ordinary: GRUB finds no counter and writes nothing.
    let block = host.env.clone();
    host.grub();
    assert_eq!(host.env, block);
    assert!(!host.cmdline.contains("tur.boot="));
    assert!(host.cmdline.contains("tur.boot_contract=1"));
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
    assert_eq!(host.booted_checksum(), "new");
    // The previous version is kept as the rollback deployment.
    let status = host.bootc_status().unwrap();
    assert_eq!(status.rollback.unwrap().checksum, "old");
}

#[test]
fn nothing_is_armed_without_a_staged_deployment() {
    let mut host = FakeHost::new();
    restart(&mut host);
    assert_eq!(host.counter(), None);
    assert!(host.ops.is_empty(), "{:?}", host.ops);
    host.stage("new", "2.0");
    host.staged.as_mut().unwrap().download_only = true;
    arm(&mut host, &config());
    assert_eq!(host.counter(), None);
    assert!(host.ops.is_empty(), "{:?}", host.ops);
}

// ---- order of writes, design section 5.2 -----------------------------------------------

#[test]
fn arming_writes_the_record_before_the_block_in_one_call() {
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    assert!(host.op_index("write_state trial.json") < host.op_index("env_set"));
    assert_eq!(
        host.ops_matching("env_set"),
        vec![
            "env_set tur_trial_args=panic=30 rd.shell=0 rd.emergency=reboot \
             systemd.watchdog_sec=180 boot_counter=3"
                .to_owned()
        ]
    );
    let record = trial::load_record(&host).expect("a record");
    assert_eq!(record.phase, Phase::Trial);
    assert_eq!(record.attempts_allowed, 3);
    assert_eq!(record.previous.unwrap().checksum, "old");
    assert_eq!(record.new.unwrap().checksum, "new");
    assert_eq!(record.armed_boot_id, host.boot_id());
}

#[test]
fn a_fallback_makes_the_counter_positive_then_changes_the_order_then_removes_it() {
    let mut host = updated();
    for _ in 0..3 {
        assert_eq!(failing_boot(&mut host), Classification::Trial);
        host.grub();
    }
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    let reverting = host.op_index("write_state trial.json");
    let positive = host.op_index("env_set boot_counter=2");
    let order = host.op_index("rollback");
    let removed = host.op_index("env_unset boot_counter tur_trial_args");
    let rejected_at = host.op_index("write_state rejected.json");
    let record_gone = host.op_index("remove_state trial.json");
    assert!(reverting < positive, "{:?}", host.ops);
    assert!(positive < order, "{:?}", host.ops);
    assert!(order < removed, "{:?}", host.ops);
    assert!(removed < rejected_at, "{:?}", host.ops);
    assert!(rejected_at < record_gone, "{:?}", host.ops);
    assert_restored(&host);
}

#[test]
fn marking_good_removes_the_counter_before_the_record() {
    let mut host = updated();
    host.ops.clear();
    good_boot(&mut host);
    assert!(
        host.op_index("env_unset boot_counter tur_trial_args")
            < host.op_index("remove_state trial.json"),
        "{:?}",
        host.ops
    );
}

// ---- sequences on a bad boot, design section 5.8 ------------------------------------------

#[test]
fn a_failing_gate_gets_three_attempts_and_then_the_previous_version() {
    let mut host = updated();
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(crate::model::KernelArgs::parse(&host.cmdline).tries_left);
        assert_eq!(host.booted_checksum(), "new");
        assert_eq!(failing_boot(&mut host), Classification::Trial);
        host.grub();
    }
    assert_eq!(seen, vec![Some(2), Some(1), Some(0)]);
    // The fourth boot is the fallback: GRUB starts the second entry.
    assert!(host.cmdline.contains("tur.boot=fallback"));
    assert_eq!(host.counter().as_deref(), Some("-1"));
    assert_eq!(host.booted_checksum(), "old");
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
    let event = last_event(&host, "fallback");
    assert_eq!(event.str_or_empty("reason"), "attempts");
    assert_eq!(
        event.get("attempts_allowed").and_then(Value::as_i64),
        Some(3)
    );
    assert_eq!(
        event
            .get("attempts_that_reached_userspace")
            .and_then(Value::as_i64),
        Some(3)
    );
    assert_eq!(event.str_or_empty("first_failed_check"), "R5");
    assert_eq!(
        event.str_or_empty("first_failed_check_code"),
        "TUR-BOOT-HEALTH-005"
    );
    assert_eq!(
        event
            .path(&["restored", "checksum"])
            .and_then(Value::as_str),
        Some("old")
    );
    assert_eq!(
        event.path(&["failed", "checksum"]).and_then(Value::as_str),
        Some("new")
    );
    assert_eq!(
        event
            .get("attempt_boot_ids")
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(3)
    );
    let notice = notice(&host);
    assert_eq!(notice.str_or_empty("error_code"), "TUR-BOOT-TRIAL-003");
    let text = notice.str_or_empty("text");
    assert!(text.contains(
        "The update to TUR 2.0 (build build-new) did not start correctly after 3 attempts."
    ));
    assert!(text.contains("TUR has restored the previous version, 1.0 (build build-old)."));
    assert!(text.contains("Your files were not changed."));
    assert!(text.contains("The update will not be tried again automatically."));
    assert!(host.journal.iter().any(|entry| {
        entry.message_id == trial::MSG_FALLBACK
            && entry.fields.contains(&(
                "TUR_FAILED_IMAGE_DIGEST".to_owned(),
                "sha256:new".to_owned(),
            ))
    }));
}

#[test]
fn an_update_that_never_reaches_userspace_is_abandoned_by_the_counter_alone() {
    // A kernel panic or an unmountable root: GRUB counts, nothing else runs.
    let mut host = updated();
    let mut sequence = vec![host.cmdline.clone()];
    for _ in 0..3 {
        host.grub();
        sequence.push(host.cmdline.clone());
    }
    assert!(sequence[0].contains("tur.boot_tries_left=2"));
    assert!(sequence[1].contains("tur.boot_tries_left=1"));
    assert!(sequence[2].contains("tur.boot_tries_left=0"));
    assert!(sequence[3].contains("tur.boot=fallback"));
    assert_eq!(host.booted_checksum(), "old");
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
    let event = last_event(&host, "fallback");
    assert_eq!(
        event
            .get("attempts_that_reached_userspace")
            .and_then(Value::as_i64),
        Some(0)
    );
    assert!(event.get("first_failed_check").unwrap().is_null());
}

#[test]
fn a_kernel_grub_cannot_start_falls_back_in_the_same_boot() {
    let mut host = updated();
    host.loader_fell_back();
    assert!(
        host.cmdline
            .contains("tur.boot=trial tur.boot_tries_left=2")
    );
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
    assert_eq!(
        last_event(&host, "fallback").str_or_empty("reason"),
        "loader"
    );
    let notice = notice(&host);
    assert_eq!(notice.str_or_empty("error_code"), "TUR-BOOT-TRIAL-004");
    assert!(
        notice
            .str_or_empty("text")
            .contains("could not be started by the boot loader")
    );
}

#[test]
fn an_update_that_was_never_finalised_is_not_rolled_back() {
    // greenboot issue 219: a failed finalisation must not undo a healthy host.
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    host.staged = None;
    host.grub();
    assert!(
        host.cmdline
            .contains("tur.boot=trial tur.boot_tries_left=2")
    );
    assert_eq!(host.booted_checksum(), "old");
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Stale);
    assert!(host.ops_matching("rollback").is_empty(), "{:?}", host.ops);
    assert_eq!(host.counter(), None);
    assert!(host.read_state(TRIAL_FILE).is_none());
    assert!(rejected(&host).is_empty());
    let notice = notice(&host);
    assert_eq!(notice.str_or_empty("kind"), "not-applied");
    assert_eq!(notice.str_or_empty("error_code"), "TUR-BOOT-TRIAL-001");
    assert!(
        notice
            .str_or_empty("text")
            .starts_with("The update was not applied. TUR is still on 1.0")
    );
    host.grub();
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
}

#[test]
fn power_lost_between_arming_and_finalisation_leaves_the_update_not_applied() {
    // Measured in CI: GRUB reads the filesystem without replaying its
    // journal, so after a power cut it can act on the environment block as
    // it was before the last write. Here that is the block before arming.
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    host.staged = None;
    let written = host.env.clone();
    host.env = Some(Default::default());
    host.grub();
    host.env = written;
    assert!(!host.cmdline.contains("tur.boot="), "GRUB saw no counter");
    assert_eq!(host.counter().as_deref(), Some("3"), "the block holds one");
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Stale);
    assert!(host.ops_matching("rollback").is_empty(), "{:?}", host.ops);
    assert_eq!(host.counter(), None);
    assert!(host.read_state(TRIAL_FILE).is_none());
    assert_eq!(notice(&host).str_or_empty("kind"), "not-applied");
    host.grub();
    assert!(!host.cmdline.contains("tur.boot="));
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
    assert_eq!(host.booted_checksum(), "old");
}

#[test]
fn a_failing_gate_outside_a_trial_tells_the_user_and_restarts_nothing() {
    let mut host = FakeHost::new();
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
    host.set_verdict("failed", Some("R6"));
    record_failure(&mut host);
    assert!(!host.reboot_requested);
    let notice = notice(&host);
    assert_eq!(notice.str_or_empty("kind"), "unhealthy");
    assert_eq!(
        notice.str_or_empty("text"),
        "TUR started, but some checks failed. Diagnostics are available."
    );
    assert_eq!(
        last_event(&host, "gate-failed")
            .get("restart_follows")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(host.counter(), None);
}

#[test]
fn a_gate_that_left_no_verdict_is_recorded_as_such() {
    let mut host = updated();
    assert_eq!(classify(&mut host, &config()), Classification::Trial);
    record_failure(&mut host);
    let record = trial::load_record(&host).expect("a record");
    assert_eq!(record.attempts[0].verdict, "failed");
    assert_eq!(
        record.attempts[0].error_code.as_deref(),
        Some("TUR-BOOT-HEALTH-030")
    );
    assert!(
        host.read_state(NOTICE_FILE).is_none(),
        "a trial restarts; it leaves no notice"
    );
}

// ---- the interruption table, design section 5.9 ------------------------------------------

/// Boots until the machine is stable on the previous version, with the
/// failed version failing as before whenever it is started.
fn converge(host: &mut FakeHost) -> u32 {
    for boots in 1..=12 {
        host.grub();
        let classification = if host.booted_checksum() == "new" {
            failing_boot(host)
        } else {
            classify(host, &config())
        };
        if host.booted_checksum() == "old"
            && host.default_checksum() == "old"
            && host.counter().is_none()
            && host.read_state(TRIAL_FILE).is_none()
            && classification != Classification::Rejected
        {
            return boots;
        }
    }
    panic!("the machine did not converge: ops {:?}", host.ops);
}

/// A machine in the fallback boot, before `classify` has run.
fn at_fallback() -> FakeHost {
    let mut host = updated();
    for _ in 0..3 {
        failing_boot(&mut host);
        host.grub();
    }
    assert!(host.cmdline.contains("tur.boot=fallback"));
    host
}

#[test]
fn power_lost_at_any_write_of_the_fallback_still_ends_on_the_previous_version() {
    // How many writes a complete fallback makes.
    let mut complete = at_fallback();
    complete.ops.clear();
    classify(&mut complete, &config());
    let writes = complete.ops.len();
    assert!(writes >= 6, "{:?}", complete.ops);
    for cut in 0..writes {
        let mut host = at_fallback();
        host.power = Some(cut);
        classify(&mut host, &config());
        assert!(host.dead, "the power was to fail after {cut} writes");
        let boots = converge(&mut host);
        assert_restored(&host);
        assert!(
            boots <= 4,
            "cut after {cut} writes: {boots} boots, ops {:?}",
            host.ops
        );
        // No loop afterwards: two more restarts are ordinary boots.
        for _ in 0..2 {
            host.grub();
            assert!(!host.cmdline.contains("tur.boot="));
            assert_eq!(classify(&mut host, &config()), Classification::Normal);
            assert_eq!(host.booted_checksum(), "old");
        }
    }
}

#[test]
fn power_lost_after_step_3_costs_two_short_starts_of_the_failed_version() {
    let mut host = at_fallback();
    // Writes: the record (step 2), the counter (step 3), then the rollback.
    host.power = Some(2);
    classify(&mut host, &config());
    assert!(host.dead);
    assert_eq!(host.counter().as_deref(), Some("2"));
    assert_eq!(host.default_checksum(), "new", "the order was not changed");
    assert_eq!(trial::load_record(&host).unwrap().phase, Phase::Reverting);
    // GRUB counts and starts the first entry: still the failed version.
    host.grub();
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(classify(&mut host, &config()), Classification::Rejected);
    assert!(host.reboot_requested, "row 9 restarts at once");
    assert_eq!(
        host.counter().as_deref(),
        Some("1"),
        "the default version keeps its counter"
    );
    host.grub();
    assert_eq!(classify(&mut host, &config()), Classification::Rejected);
    // The counter is used up: GRUB selects the previous version again and
    // row 7 resumes from step 3.
    host.grub();
    assert!(host.cmdline.contains("tur.boot=fallback"));
    assert_eq!(host.booted_checksum(), "old");
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
}

#[test]
fn power_lost_after_step_4_resumes_at_step_5() {
    let mut host = at_fallback();
    host.power = Some(3);
    classify(&mut host, &config());
    assert!(host.dead);
    assert_eq!(host.default_checksum(), "old", "the order was changed");
    assert_eq!(host.counter().as_deref(), Some("2"));
    // GRUB counts and starts the first entry: now the previous version.
    host.grub();
    assert!(
        host.cmdline
            .contains("tur.boot=trial tur.boot_tries_left=1")
    );
    assert_eq!(host.booted_checksum(), "old");
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert!(
        host.ops_matching("rollback").is_empty(),
        "row 8 does not roll back again"
    );
    assert!(host.ops_matching("env_set").is_empty(), "{:?}", host.ops);
    assert_restored(&host);
}

#[test]
fn power_lost_three_times_between_steps_4_and_6_still_converges() {
    let mut host = at_fallback();
    host.power = Some(3);
    classify(&mut host, &config());
    // Two more starts of the previous version, each cut before step 6.
    for expected in ["1", "0"] {
        host.grub();
        assert_eq!(host.booted_checksum(), "old");
        assert_eq!(host.counter().as_deref(), Some(expected));
        host.power = Some(0);
        classify(&mut host, &config());
        assert!(host.dead);
    }
    // The counter has run out on starts of the previous version: GRUB now
    // starts the second entry, which is the failed version.
    host.grub();
    assert!(host.cmdline.contains("tur.boot=fallback"));
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(classify(&mut host, &config()), Classification::Rejected);
    assert!(host.reboot_requested);
    assert_eq!(
        host.counter(),
        None,
        "row 9 removes the counter when it is not the default"
    );
    // The previous version then finishes.
    host.grub();
    assert_eq!(host.booted_checksum(), "old");
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
}

#[test]
fn a_failed_rollback_leaves_grub_on_the_previous_version() {
    let mut host = at_fallback();
    host.rollback_fails = true;
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_eq!(
        host.counter().as_deref(),
        Some("-1"),
        "what GRUB left is put back"
    );
    assert_eq!(host.default_checksum(), "new");
    assert!(
        host.journal
            .iter()
            .any(|entry| entry.message.contains("TUR-BOOT-STATE-004"))
    );
    // The next boot is a fallback boot again, and the rollback is retried.
    host.grub();
    assert_eq!(host.booted_checksum(), "old");
    host.rollback_fails = false;
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
}

// ---- avoiding a rollback loop, design section 5.10 ------------------------------------------

#[test]
fn nothing_brings_the_failed_version_back_after_a_fallback() {
    let mut host = at_fallback();
    classify(&mut host, &config());
    assert_restored(&host);
    for _ in 0..3 {
        // Clean restarts: nothing is staged, so nothing is armed.
        restart(&mut host);
        assert!(!host.cmdline.contains("tur.boot="));
        assert_eq!(host.booted_checksum(), "old");
        assert_eq!(classify(&mut host, &config()), Classification::Normal);
        assert_eq!(host.counter(), None);
    }
    assert_eq!(rejected(&host), vec!["sha256:new".to_owned()]);
}

#[test]
fn a_person_can_retry_the_failed_version_from_the_menu() {
    // Row 13: a fallback boot in which a person chose the first entry.
    let mut host = at_fallback();
    host.booted = 0;
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(classify(&mut host, &config()), Classification::ManualRetry);
    assert_eq!(
        host.counter().as_deref(),
        Some("-1"),
        "classify changes nothing"
    );
    // The gate fails: nothing restarts, and the next boot is a fallback again.
    host.set_verdict("failed", Some("R5"));
    record_failure(&mut host);
    assert!(!host.reboot_requested);
    host.grub();
    assert!(host.cmdline.contains("tur.boot=fallback"));
    // Chosen again, and this time the gate passes: the version is kept.
    host.booted = 0;
    assert_eq!(good_boot(&mut host), Classification::ManualRetry);
    assert_eq!(host.counter(), None);
    assert!(host.read_state(TRIAL_FILE).is_none());
    host.grub();
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(classify(&mut host, &config()), Classification::Normal);
}

// ---- refusing an unprotected trial, design section 5.5 ------------------------------------------

#[test]
fn an_update_is_refused_where_attempts_cannot_be_counted() {
    // No fragment in grub.cfg: the contract version is not on the command line.
    let mut host = FakeHost::new();
    host.cmdline = "root=UUID=1 rw $tur_boot_args".to_owned();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    assert!(host.finalization_locked);
    assert!(host.ops_matching("env_set").is_empty());
    assert_eq!(host.counter(), None);
    let found = notice(&host);
    assert_eq!(found.str_or_empty("kind"), "refused");
    assert_eq!(found.str_or_empty("error_code"), "TUR-BOOT-TRIAL-002");
    assert!(
        found.str_or_empty("text").starts_with(
            "The update was not applied: this installation cannot count boot attempts."
        )
    );
    host.finalize();
    host.grub();
    assert_eq!(host.booted_checksum(), "old");

    // A filesystem GRUB cannot be relied on to write.
    let mut host = FakeHost::new();
    host.layout.fstype = "btrfs".to_owned();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    assert!(host.finalization_locked);
    assert_eq!(host.counter(), None);

    // The block cannot be written at all.
    let mut host = FakeHost::new();
    host.env_write_fails = true;
    host.stage("new", "2.0");
    arm(&mut host, &config());
    assert!(host.finalization_locked);
    assert!(
        host.read_state(TRIAL_FILE).is_none(),
        "no record without a counter"
    );
    assert_eq!(notice(&host).str_or_empty("kind"), "refused");
}

// ---- lost or stray state, design sections 5.6 and 5.12 ---------------------------------------------

#[test]
fn a_block_grub_could_not_read_is_recreated_and_the_trial_goes_on() {
    // Row 11. The block became invalid after arming: GRUB loaded no
    // variables, counted nothing and passed no trial arguments.
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    host.finalize();
    host.env = None;
    host.grub();
    assert!(!host.cmdline.contains("tur.boot="));
    assert_eq!(host.booted_checksum(), "new");
    assert_eq!(classify(&mut host, &config()), Classification::Trial);
    assert_eq!(
        host.counter().as_deref(),
        Some("2"),
        "three allowed, this boot is the first"
    );
    assert!(
        host.read_run(RUN_TRIAL_FLAG).is_some(),
        "the restart units need the flag"
    );
    assert!(
        host.journal
            .iter()
            .any(|entry| entry.message.contains("TUR-BOOT-STATE-003"))
    );
    // The gate fails: the remaining attempts are counted by GRUB as usual.
    host.set_verdict("failed", Some("R5"));
    record_failure(&mut host);
    assert_eq!(
        last_event(&host, "gate-failed")
            .get("restart_follows")
            .and_then(Value::as_bool),
        Some(true)
    );
    for _ in 0..2 {
        host.grub();
        assert_eq!(failing_boot(&mut host), Classification::Trial);
    }
    host.grub();
    assert!(host.cmdline.contains("tur.boot=fallback"));
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
}

#[test]
fn power_lost_while_marking_good_costs_at_most_one_more_trial_boot() {
    for cut in 0..3 {
        let mut host = updated();
        assert_eq!(classify(&mut host, &config()), Classification::Trial);
        host.set_verdict("passed", None);
        host.power = Some(cut);
        let _ = mark_good(&mut host);
        host.grub();
        assert_eq!(host.booted_checksum(), "new", "cut after {cut} writes");
        let classification = good_boot(&mut host);
        assert!(
            classification == Classification::Trial || classification == Classification::Normal,
            "cut after {cut} writes: {classification:?}"
        );
        assert_eq!(host.counter(), None);
        assert!(host.read_state(TRIAL_FILE).is_none());
        host.grub();
        assert_eq!(classify(&mut host, &config()), Classification::Normal);
        assert_eq!(host.booted_checksum(), "new");
        assert!(rejected(&host).is_empty());
    }
}

#[test]
fn a_counter_without_a_record_is_a_trial_with_unknown_identities() {
    // Row 10: armed by hand, or the record was lost.
    let mut host = FakeHost::new();
    host.env
        .as_mut()
        .unwrap()
        .insert(VAR_COUNTER.to_owned(), "2".to_owned());
    host.grub();
    assert!(
        host.cmdline
            .contains("tur.boot=trial tur.boot_tries_left=1")
    );
    assert_eq!(good_boot(&mut host), Classification::Trial);
    assert_eq!(host.counter(), None);
    assert_eq!(events(&host), vec!["attempt", "marked-good"]);
}

#[test]
fn a_stray_counter_is_removed() {
    // Row 12: a counter that GRUB did not act on and no record explains.
    let mut host = FakeHost::new();
    host.env
        .as_mut()
        .unwrap()
        .insert(VAR_COUNTER.to_owned(), "3".to_owned());
    assert_eq!(classify(&mut host, &config()), Classification::Inconsistent);
    assert_eq!(host.counter(), None);
    assert!(
        host.journal
            .iter()
            .any(|entry| entry.message.contains("TUR-BOOT-STATE-002"))
    );
}

#[test]
fn a_person_choosing_the_other_entry_outside_a_trial_changes_nothing() {
    // Row 2.
    let mut host = updated();
    good_boot(&mut host);
    host.grub();
    host.booted = 1;
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Manual);
    assert!(host.ops.is_empty(), "{:?}", host.ops);
    assert_eq!(host.default_checksum(), "new");
}

#[test]
fn when_grub_cannot_save_the_counter_userspace_counts_and_ends_the_trial() {
    // greenboot issue 220: GRUB reads the block but its writes are lost, so
    // the counter never reaches zero.
    let mut host = FakeHost::new();
    host.stage("new", "2.0");
    arm(&mut host, &config());
    host.finalize();
    host.grub_cannot_save = true;
    for attempt in 1..=3 {
        host.grub();
        assert!(
            host.cmdline
                .contains("tur.boot=trial tur.boot_tries_left=2")
        );
        assert_eq!(host.counter().as_deref(), Some("3"));
        assert_eq!(failing_boot(&mut host), Classification::Trial);
        let record = trial::load_record(&host).expect("a record");
        assert!(record.grub_cannot_save);
        assert_eq!(record.attempts.len(), attempt);
    }
    // The third failure changed the boot order from the failing version.
    assert_eq!(host.ops_matching("rollback").len(), 1);
    assert_eq!(host.default_checksum(), "old");
    assert!(
        host.journal
            .iter()
            .any(|entry| entry.message.contains("TUR-BOOT-TRIAL-005"))
    );
    host.grub();
    assert_eq!(host.booted_checksum(), "old");
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert_restored(&host);
    assert_eq!(
        host.ops_matching("rollback").len(),
        1,
        "row 8 does not roll back again"
    );
}

#[test]
fn a_boot_is_not_marked_good_without_a_passing_verdict() {
    let mut host = updated();
    assert_eq!(classify(&mut host, &config()), Classification::Trial);
    assert!(mark_good(&mut host).is_err(), "no verdict");
    host.set_verdict("failed", Some("R3"));
    assert!(mark_good(&mut host).is_err(), "a failed verdict");
    assert_eq!(host.counter().as_deref(), Some("2"));
    host.set_verdict("passed", None);
    host.boots += 1;
    assert!(mark_good(&mut host).is_err(), "a verdict from another boot");
    host.boots -= 1;
    host.set_verdict("degraded", None);
    assert!(mark_good(&mut host).is_ok(), "degraded is not failed");
    assert_eq!(host.counter(), None);
}

#[test]
fn a_trial_is_marked_good_even_if_classify_never_ran() {
    let mut host = updated();
    assert!(host.read_run(RUN_BOOT).is_none());
    host.set_verdict("passed", None);
    mark_good(&mut host).expect("mark-good");
    assert_eq!(host.counter(), None);
}

#[test]
fn a_boot_that_cannot_be_classified_changes_nothing() {
    let mut host = updated();
    host.bootc_fails = true;
    host.ops.clear();
    assert_eq!(classify(&mut host, &config()), Classification::Unlisted);
    assert!(host.ops.is_empty(), "{:?}", host.ops);
    assert_eq!(
        host.counter().as_deref(),
        Some("2"),
        "the trial is still counted"
    );
}

// ---- the gate, design section 6.2 ----------------------------------------------------------------

fn check<'a>(verdict: &'a gate::Verdict, id: &str) -> &'a gate::CheckResult {
    verdict
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check {id}"))
}

#[test]
fn the_gate_passes_on_a_healthy_boot_to_the_greeter() {
    let mut host = FakeHost::new();
    let verdict = gate::run_gate(&mut host, &config(), "normal");
    assert_eq!(verdict.result, "passed");
    for id in ["R1", "R2", "R3", "R4", "R6", "R7", "R8"] {
        assert_eq!(check(&verdict, id).state, CheckState::Passed, "{id}");
    }
    // No TUR service is required yet.
    assert_eq!(check(&verdict, "R5").state, CheckState::Skipped);
    // The seat's session is held for 15 seconds.
    assert!(check(&verdict, "R8").duration_ms >= 15_000);
    let stages: Vec<String> = host
        .journal
        .iter()
        .flat_map(|entry| entry.fields.iter())
        .filter(|(name, _)| name == "TUR_BOOT_STAGE")
        .map(|(_, value)| value.clone())
        .collect();
    assert_eq!(
        stages,
        vec!["BOOT-080", "BOOT-130", "BOOT-110", "BOOT-120", "BOOT-140"]
    );
}

#[test]
fn the_gate_accepts_the_first_run_wizard_as_a_session() {
    // First boot: no user exists; the display manager logs a system user in
    // to a session of class "user".
    let mut host = FakeHost::new();
    host.session.as_mut().unwrap().class = "user".to_owned();
    assert_eq!(
        gate::run_gate(&mut host, &config(), "normal").result,
        "passed"
    );
}

#[test]
fn a_failed_unit_that_is_not_required_only_degrades_the_boot() {
    // mcelog.service fails on every AMD boot.
    let mut host = FakeHost::new();
    host.failed = vec!["mcelog.service".to_owned()];
    let verdict = gate::run_gate(&mut host, &config(), "normal");
    assert_eq!(verdict.result, "degraded");
    assert!(verdict.passed());
    assert_eq!(check(&verdict, "W1").detail, "mcelog.service");
    assert!(verdict.first_failure().is_none());
}

#[test]
fn a_required_unit_that_fails_fails_the_gate_at_once() {
    let mut host = FakeHost::new();
    host.files.insert(
        "/usr/lib/tur/boot-health/required-units.d/storage.conf".to_owned(),
        "# the storage manager\ntur-storage.service\n".to_owned(),
    );
    host.set_unit("tur-storage.service", "ActiveState", "failed");
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.result, "failed");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R5");
    assert_eq!(failure.state, CheckState::Failed);
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-005"));
    assert!(failure.detail.contains("tur-storage.service is failed"));
    assert!(
        failure.duration_ms < 2_000,
        "a failed unit is not waited for"
    );
    // The checks after it are not run.
    assert!(verdict.checks.iter().all(|check| check.id != "R6"));
}

#[test]
fn a_required_unit_that_never_starts_runs_out_of_time() {
    let mut host = FakeHost::new();
    host.files.insert(
        "/usr/lib/tur/boot-health/required-units.d/storage.conf".to_owned(),
        "tur-storage.service\n".to_owned(),
    );
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R5");
    assert_eq!(failure.state, CheckState::Timeout);
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-020"));
    assert!(failure.duration_ms >= 60_000);
}

#[test]
fn a_required_unit_that_starts_late_passes() {
    let mut host = FakeHost::new();
    host.files.insert(
        "/usr/lib/tur/boot-health/required-units.d/storage.conf".to_owned(),
        "tur-storage.service\n".to_owned(),
    );
    host.set_unit("tur-storage.service", "ActiveState", "activating");
    host.timeline.push(At {
        uptime_ms: 25_000,
        unit: "tur-storage.service",
        property: "ActiveState",
        value: "active",
    });
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.result, "passed");
    assert_eq!(check(&verdict, "R5").state, CheckState::Passed);
}

#[test]
fn a_boot_that_stalls_before_its_default_target_fails_after_the_limit() {
    let mut host = FakeHost::new();
    host.set_unit("default.target", "ActiveState", "inactive");
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R3");
    assert_eq!(failure.state, CheckState::Timeout);
    assert!(failure.duration_ms >= 240_000);
    assert!(failure.duration_ms < 245_000);
    assert!(failure.detail.contains("graphical.target is inactive"));
}

#[test]
fn a_display_manager_that_crash_loops_fails_the_gate() {
    // Seen from outside the unit is active most of the time; its restart
    // count is what moves.
    let mut host = FakeHost::new();
    host.timeline.push(At {
        uptime_ms: 12_000,
        unit: "display-manager.service",
        property: "NRestarts",
        value: "3",
    });
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R8");
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-008"));
    assert!(failure.detail.contains("restarts 0 became"));

    // No session ever appears on the seat.
    let mut host = FakeHost::new();
    host.session = None;
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R7");
    assert_eq!(failure.state, CheckState::Timeout);
    assert!(failure.duration_ms >= 90_000);

    // A session without a compositor.
    let mut host = FakeHost::new();
    host.compositor = None;
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.first_failure().expect("a failure").id, "R7");

    // A text console is not a graphical session.
    let mut host = FakeHost::new();
    host.session.as_mut().unwrap().kind = "tty".to_owned();
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.first_failure().expect("a failure").id, "R7");
}

#[test]
fn a_headless_default_target_skips_the_display_checks() {
    let mut host = FakeHost::new();
    host.set_unit("default.target", "Id", "multi-user.target");
    host.session = None;
    host.set_unit("display-manager.service", "ActiveState", "inactive");
    let verdict = gate::run_gate(&mut host, &config(), "normal");
    assert_eq!(verdict.result, "passed");
    for id in ["R6", "R7", "R8"] {
        assert_eq!(check(&verdict, id).state, CheckState::Skipped, "{id}");
    }
}

#[test]
fn the_root_and_the_critical_paths_are_checked_first() {
    let mut host = FakeHost::new();
    host.root = Some(("ext4".to_owned(), "/dev/vda3".to_owned()));
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R1");
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-001"));

    let mut host = FakeHost::new();
    host.unreadable.insert("/usr/lib/tur/release".to_owned());
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R2");
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-002"));

    let mut host = FakeHost::new();
    host.var_writable = false;
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.first_failure().expect("a failure").id, "R2");
}

#[test]
fn a_core_service_that_failed_fails_the_gate() {
    let mut host = FakeHost::new();
    host.set_unit("systemd-logind.service", "ActiveState", "failed");
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "R4");
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-004"));
}

#[test]
fn additional_checks_from_the_image_are_run_without_a_shell() {
    let directory = "/usr/lib/tur/boot-health/checks.d";
    // A required check that exits 1 fails the gate.
    let mut host = FakeHost::new();
    host.files.insert(
        format!("{directory}/broken.toml"),
        "exec = \"/usr/libexec/broken\"\nrequired = true\n".to_owned(),
    );
    host.check_status
        .insert("/usr/libexec/broken".to_owned(), Ok(1));
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    let failure = verdict.first_failure().expect("a failure");
    assert_eq!(failure.id, "X:broken");
    assert_eq!(failure.code.as_deref(), Some("TUR-BOOT-HEALTH-021"));
    assert!(failure.detail.contains("exited 1"));
    // It runs after R1 to R8 have passed.
    assert_eq!(check(&verdict, "R8").state, CheckState::Passed);

    // A wanted check that fails only degrades the boot.
    let mut host = FakeHost::new();
    host.files.insert(
        format!("{directory}/optional.toml"),
        "exec = \"/usr/libexec/optional\"\n".to_owned(),
    );
    host.check_status
        .insert("/usr/libexec/optional".to_owned(), Ok(3));
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.result, "degraded");

    // One that passes changes nothing.
    let mut host = FakeHost::new();
    host.files.insert(
        format!("{directory}/fine.toml"),
        "exec = \"/usr/libexec/fine\"\nrequired = true\n".to_owned(),
    );
    host.check_status
        .insert("/usr/libexec/fine".to_owned(), Ok(0));
    assert_eq!(
        gate::run_gate(&mut host, &config(), "trial").result,
        "passed"
    );

    // A file that cannot be understood is a failure, not ignored.
    let mut host = FakeHost::new();
    host.files.insert(
        format!("{directory}/typo.toml"),
        "exe = \"/bin/true\"\n".to_owned(),
    );
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.first_failure().expect("a failure").id, "X:typo");

    // A check that cannot be run, or runs out of time, has failed.
    let mut host = FakeHost::new();
    host.files.insert(
        format!("{directory}/missing.toml"),
        "exec = \"/usr/libexec/missing\"\nrequired = true\n".to_owned(),
    );
    let verdict = gate::run_gate(&mut host, &config(), "trial");
    assert_eq!(verdict.first_failure().expect("a failure").id, "X:missing");
}

#[test]
fn the_gate_writes_its_verdict_and_refuses_a_rejected_version() {
    let mut host = FakeHost::new();
    classify(&mut host, &config());
    assert!(gate::gate(&mut host, Ok(config())));
    assert!(mark_good(&mut host).is_ok());

    let mut host = FakeHost::new();
    classify(&mut host, &config());
    host.session = None;
    assert!(!gate::gate(&mut host, Ok(config())));
    assert!(mark_good(&mut host).is_err());
    record_failure(&mut host);
    let event = last_event(&host, "gate-failed");
    assert_eq!(event.str_or_empty("failed_check"), "R7");
    assert_eq!(event.str_or_empty("error_code"), "TUR-BOOT-HEALTH-020");

    // A configuration that cannot be read fails the gate.
    let mut host = FakeHost::new();
    classify(&mut host, &config());
    assert!(!gate::gate(
        &mut host,
        Err("line 3: unknown key".to_owned())
    ));

    // Row 9: the version being replaced is never marked good.
    let mut host = at_fallback();
    host.power = Some(2);
    classify(&mut host, &config());
    host.grub();
    assert_eq!(classify(&mut host, &config()), Classification::Rejected);
    assert!(!gate::gate(&mut host, Ok(config())));
    assert!(mark_good(&mut host).is_err());
}

#[test]
fn the_gate_passes_in_the_boot_that_made_the_fallback_permanent() {
    // `bootc rollback` re-points the links the kernel's ostree= argument goes
    // through. Resolved again after it, the argument names the failed
    // version, and R1 failed in the first CI run of the scenarios.
    let mut host = at_fallback();
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    assert!(host.links_swapped);
    assert!(gate::gate(&mut host, Ok(config())));
    assert_eq!(notice(&host).str_or_empty("kind"), "fallback");
    // Without `classify` the gate resolves the argument itself.
    let mut host = FakeHost::new();
    assert_eq!(gate::run_gate(&mut host, &config(), "unknown").result, "passed");
}

#[test]
fn a_failed_gate_does_not_replace_the_notice_of_a_fallback() {
    let mut host = at_fallback();
    assert_eq!(classify(&mut host, &config()), Classification::Fallback);
    host.session = None;
    assert!(!gate::gate(&mut host, Ok(config())));
    record_failure(&mut host);
    assert!(!host.reboot_requested);
    assert_eq!(notice(&host).str_or_empty("kind"), "fallback");
    assert_eq!(last_event(&host, "gate-failed").str_or_empty("failed_check"), "R7");
}

#[test]
fn counting_that_is_not_usable_is_reported_by_the_gate() {
    let mut host = FakeHost::new();
    host.layout.plain_partition = false;
    let verdict = gate::run_gate(&mut host, &config(), "normal");
    assert_eq!(verdict.result, "degraded");
    assert_eq!(check(&verdict, "W10").state, CheckState::Failed);
    let mut host = FakeHost::new();
    host.watchdog = None;
    let verdict = gate::run_gate(&mut host, &config(), "normal");
    assert_eq!(verdict.result, "passed");
    assert_eq!(check(&verdict, "W12").state, CheckState::Skipped);
}

// ---- status, the notice and the evidence ------------------------------------------------------------

#[test]
fn status_reports_the_trial_the_rejected_images_and_the_notice() {
    let mut host = updated();
    classify(&mut host, &config());
    let value = status::status_json(&mut host);
    assert_eq!(value.get("schema_version").and_then(Value::as_i64), Some(1));
    assert_eq!(
        value
            .path(&["boot", "classification"])
            .and_then(Value::as_str),
        Some("trial")
    );
    assert_eq!(
        value.path(&["counting", "usable"]).and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .path(&["counting", "boot_counter"])
            .and_then(Value::as_str),
        Some("2")
    );
    assert_eq!(
        value.path(&["trial", "phase"]).and_then(Value::as_str),
        Some("trial")
    );
    assert!(status::status_text(&value).contains("yes, phase trial"));

    let mut host = at_fallback();
    classify(&mut host, &config());
    let value = status::status_json(&mut host);
    assert!(value.get("trial").unwrap().is_null());
    assert_eq!(
        value
            .path(&["rejected", "images"])
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(1)
    );
    assert_eq!(
        value.path(&["notice", "kind"]).and_then(Value::as_str),
        Some("fallback")
    );
    let text = status::status_text(&value);
    assert!(text.contains("Rejected images:"));
    assert!(text.contains("sha256:new"));
    assert!(text.contains("TUR-BOOT-TRIAL-003"));
    assert!(text.contains("tur-boot-health acknowledge"));
    assert!(status::codes_text().contains("TUR-BOOT-TRIAL-003  Fallback"));

    // Acknowledging clears the notice, once.
    assert_eq!(status::acknowledge(&mut host), Ok(true));
    assert!(host.read_state(NOTICE_FILE).is_none());
    assert_eq!(status::acknowledge(&mut host), Ok(false));
    assert_eq!(
        events(&host).last().map(String::as_str),
        Some("acknowledged")
    );
    // The rejected list is not cleared by acknowledging.
    assert_eq!(rejected(&host), vec!["sha256:new".to_owned()]);
}

#[test]
fn the_journals_of_the_failed_attempts_are_exported_once() {
    let mut host = updated();
    let armed = "boot0001".to_owned();
    let mut boots = vec![armed.clone()];
    for _ in 0..3 {
        boots.push(host.boot_id());
        failing_boot(&mut host);
        host.grub();
    }
    boots.push(host.boot_id());
    host.journal_boot_ids = boots.clone();
    classify(&mut host, &config());
    assert_restored(&host);
    let done = status::export_evidence(&mut host).expect("export");
    assert!(done.contains("3 boots"), "{done}");
    assert_eq!(host.exported, boots[1..4].to_vec());
    let event_id = last_event(&host, "fallback").str_or_empty("event_id");
    let summary = json::parse(
        &host
            .read_state(&format!("evidence/{event_id}/summary.json"))
            .expect("a summary"),
    )
    .unwrap();
    assert_eq!(
        summary
            .get("journals")
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(3)
    );
    assert_eq!(
        summary.path(&["event", "reason"]).and_then(Value::as_str),
        Some("attempts")
    );
    // A second run does nothing.
    let again = status::export_evidence(&mut host).expect("export");
    assert!(again.contains("already exists"), "{again}");
    assert_eq!(host.exported.len(), 3);

    // Without a fallback there is nothing to export.
    let mut host = FakeHost::new();
    assert!(
        status::export_evidence(&mut host)
            .unwrap()
            .contains("nothing to export")
    );
}

#[test]
fn an_attempt_that_wrote_a_journal_without_reaching_classify_is_exported_too() {
    // Emergency mode: journald ran, this program did not.
    let mut host = updated();
    let armed = "boot0001".to_owned();
    let mut boots = vec![armed];
    for _ in 0..3 {
        boots.push(host.boot_id());
        host.grub();
    }
    boots.push(host.boot_id());
    host.journal_boot_ids = boots.clone();
    classify(&mut host, &config());
    assert_restored(&host);
    status::export_evidence(&mut host).expect("export");
    assert_eq!(host.exported, boots[1..4].to_vec());
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The compatibility autopilot (`docs/architecture/compat-autopilot.md`):
//! when a Windows application fails under Wine, try known remedies in
//! throwaway copies of its prefix, keep the first that works, and record a
//! measured result.
//!
//! This module is the part that decides and records, and touches nothing:
//!
//! * [`CheckSpec`] says what "works" means for an application: a window of
//!   its own, an exit status, text in its output; [`evaluate`] holds one
//!   run against it.
//! * [`candidates`] chooses, for a failure class and the hints of the
//!   evidence, which remedies of the [`Catalogue`] are tried and in which
//!   order, and says why each of the others is not.
//! * [`search`] runs the baseline, the single remedies and then bounded
//!   pairs through a [`TrialRunner`], stopping at the first that passes.
//!   The real runner (`crate::autopilot_run`) works on copies only; tests
//!   use a scripted one.
//! * [`AutopilotResult`] is the measured result
//!   (`schemas/remedy-result.schema.json`), [`catalogue_record`] the
//!   shareable compatibility record in the catalogue's own format, and
//!   [`AppliedRemedies`] what is kept beside an application once the person
//!   applies a result (`schemas/remedy-applied.schema.json`).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::failure::{
    Diagnosis, Ended, Evidence, FailureClass, Hint, StaticFacts, dll_base, reports_fatal,
};
use crate::metadata::RuntimeKind;
use crate::profile::is_app_id;
use crate::remedy::{Catalogue, EnvironmentValue, RemedyInstance, Requirement, Resolved, instance};
use crate::runtime_cmd::check_arguments;
use crate::sha256::sha256_hex;
use crate::timefmt::Timestamp;

/// The version of the check format.
pub const CHECK_SCHEMA_VERSION: u32 = 1;
/// The version of the result format.
pub const RESULT_SCHEMA_VERSION: u32 = 1;
/// The version of the applied-remedies format.
pub const APPLIED_SCHEMA_VERSION: u32 = 1;
/// How many remedy attempts a search makes unless told otherwise. The
/// baseline run is not counted.
pub const DEFAULT_MAX_ATTEMPTS: usize = 12;
/// The most attempts a search may be allowed.
pub const MAX_ATTEMPTS_LIMIT: usize = 64;
/// How many remedies of a combination are tried together at most.
pub const MAX_COMBINATION: usize = 2;
/// The largest result file read back.
pub const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;
/// The largest check or applied-remedies file read.
pub const MAX_SMALL_FILE_BYTES: usize = 256 * 1024;

/// What a program's exit has to be for a run to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExitRule {
    /// The program exits by itself with status 0.
    Zero,
    /// The program exits with 0, or is still running when it is stopped.
    ZeroOrRunning,
    /// Any exit, or none.
    Any,
}

/// What "works" means for one application (`schemas/remedy-check.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    /// [`CHECK_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Whether a window of the program must appear.
    pub window: bool,
    /// The X11 class the window has; by default the program's file name in
    /// lower case, which is what Wine sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_class: Option<String>,
    /// What the exit must be.
    pub exit: ExitRule,
    /// How long one run may take.
    pub timeout_seconds: u64,
    /// How long the program must stay up after its window appeared.
    pub settle_seconds: u64,
    /// Texts the output must contain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_contains: Vec<String>,
    /// Texts the output must not contain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_lacks: Vec<String>,
    /// Arguments the program is started with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
}

impl Default for CheckSpec {
    /// A window of the program appears within 60 seconds, the program stays
    /// up for 3 seconds or exits with 0, and nothing crashes.
    fn default() -> Self {
        CheckSpec {
            schema_version: CHECK_SCHEMA_VERSION,
            window: true,
            window_class: None,
            exit: ExitRule::ZeroOrRunning,
            timeout_seconds: 60,
            settle_seconds: 3,
            output_contains: Vec::new(),
            output_lacks: Vec::new(),
            arguments: Vec::new(),
        }
    }
}

impl CheckSpec {
    /// Parses and validates a check.
    ///
    /// # Errors
    ///
    /// A message when the text is not a valid check.
    pub fn parse(text: &str) -> Result<CheckSpec, String> {
        if text.len() > MAX_SMALL_FILE_BYTES {
            return Err("the check file is too large".to_owned());
        }
        let check: CheckSpec = serde_json::from_str(text).map_err(|error| error.to_string())?;
        check.validate()?;
        Ok(check)
    }

    /// Checks the rules the format cannot express.
    ///
    /// # Errors
    ///
    /// A message naming the first rule that does not hold.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != CHECK_SCHEMA_VERSION {
            return Err(format!("schema_version must be {CHECK_SCHEMA_VERSION}"));
        }
        if !(1..=600).contains(&self.timeout_seconds) {
            return Err("timeout_seconds must be 1 to 600".to_owned());
        }
        if self.settle_seconds > 60 || self.settle_seconds >= self.timeout_seconds {
            return Err("settle_seconds must be at most 60 and less than the timeout".to_owned());
        }
        if let Some(class) = &self.window_class
            && !crate::remedy::is_program_name(class)
        {
            return Err("window_class must be a program's file name".to_owned());
        }
        for text in self.output_contains.iter().chain(&self.output_lacks) {
            if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
                return Err(
                    "an output text is empty, too long or holds a control character".to_owned(),
                );
            }
        }
        if self.output_contains.len() + self.output_lacks.len() > 32 {
            return Err("at most 32 output texts".to_owned());
        }
        check_arguments(&self.arguments).map_err(|error| error.to_string())?;
        Ok(())
    }

    /// The window class looked for when the program is `exe`.
    pub fn class_for(&self, exe: &str) -> String {
        self.window_class
            .clone()
            .unwrap_or_else(|| exe.to_lowercase())
    }
}

/// What one run of the program did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRun {
    /// The exit status, when the program exited by itself.
    pub exit_status: Option<i32>,
    /// How the run ended.
    pub ended: Ended,
    /// Whether a window of the program appeared.
    pub window_seen: bool,
}

/// What does not hold of `check` in one run. Empty when the run passes.
pub fn evaluate(check: &CheckSpec, run: &RawRun, output: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(line) = output.lines().find(|line| reports_fatal(line)) {
        let line: String = line.trim().chars().take(300).collect();
        problems.push(format!("Wine reported a failure: {line}"));
    }
    if check.window && !run.window_seen {
        problems.push("no window of the program appeared".to_owned());
    }
    match (check.exit, run.ended, run.exit_status) {
        (ExitRule::Any, _, _)
        | (ExitRule::Zero | ExitRule::ZeroOrRunning, Ended::Exited, Some(0))
        | (ExitRule::ZeroOrRunning, Ended::Stopped | Ended::TimedOut, _) => {}
        (_, Ended::Exited, Some(status)) => {
            problems.push(format!("the program exited with status {status}"));
        }
        (_, Ended::Exited, None) => {
            problems.push("the program's exit status is unknown".to_owned())
        }
        (ExitRule::Zero, Ended::Stopped | Ended::TimedOut, _) => {
            problems.push("the program did not exit by itself in time".to_owned());
        }
    }
    for text in &check.output_contains {
        if !output.contains(text.as_str()) {
            problems.push(format!("the output does not contain {text:?}"));
        }
    }
    for text in &check.output_lacks {
        if output.contains(text.as_str()) {
            problems.push(format!("the output contains {text:?}"));
        }
    }
    problems
}

/// What the machine offers the remedies that need something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Abilities {
    /// An X11 display is there.
    pub x11_display: bool,
    /// The file system holding the copies can fold case.
    pub case_folding: bool,
    /// `/dev/ntsync` opens and the installed Wine uses it.
    pub ntsync: bool,
    /// The DLLs this Wine implements, by base name in lower case.
    pub builtin_dlls: BTreeSet<String>,
    /// The font files found on the machine, of those the catalogue names.
    pub font_files: BTreeSet<String>,
}

/// A remedy chosen to be tried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// The remedy with its parameters.
    pub instance: RemedyInstance,
    /// Its group.
    pub group: String,
    /// Whether a hint brought it forward.
    pub hinted: bool,
    /// Why it is tried.
    pub reason: String,
    /// Its order in the catalogue.
    pub order: u32,
}

/// A remedy that addresses the failure and is not tried, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassedOver {
    /// The remedy.
    pub remedy: String,
    /// Why it is not tried.
    pub reason: String,
}

/// What would be tried for one failure, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatePlan {
    /// The failure class.
    pub class: FailureClass,
    /// The hints the plan used.
    pub hints: Vec<Hint>,
    /// The remedies to try, first first.
    pub candidates: Vec<Candidate>,
    /// The remedies for the class that are not tried.
    pub passed_over: Vec<PassedOver>,
}

fn hint_names(hints: &[Hint]) -> String {
    hints
        .iter()
        .map(|hint| hint.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Chooses the remedies for a failure and orders them: those a hint
/// brought forward first, then by their order in the catalogue.
#[allow(clippy::too_many_arguments)]
pub fn candidates(
    catalogue: &Catalogue,
    class: FailureClass,
    dll: Option<&str>,
    hints: &[Hint],
    exe: &str,
    runtime: RuntimeKind,
    facts: &StaticFacts,
    abilities: &Abilities,
) -> CandidatePlan {
    let mut chosen = Vec::new();
    let mut passed_over = Vec::new();
    let mut skip = |remedy: &str, reason: String| {
        passed_over.push(PassedOver {
            remedy: remedy.to_owned(),
            reason,
        });
    };
    for remedy in &catalogue.remedies {
        if !remedy.addresses.contains(&class) {
            continue;
        }
        if !remedy.runtimes.contains(&runtime) {
            skip(
                &remedy.id,
                format!(
                    "it is for {}",
                    remedy
                        .runtimes
                        .iter()
                        .map(|r| r.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            continue;
        }
        if !remedy.automatic {
            skip(&remedy.id, "it is not tried automatically".to_owned());
            continue;
        }
        let matched: Vec<Hint> = remedy
            .hints
            .iter()
            .copied()
            .filter(|hint| hints.contains(hint))
            .collect();
        if remedy.needs_hint && matched.is_empty() {
            skip(
                &remedy.id,
                format!(
                    "it is tried only when the evidence shows {}; it does not",
                    hint_names(&remedy.hints)
                ),
            );
            continue;
        }
        let takes_dll = remedy.parameter.is_some();
        if takes_dll && dll.is_none() {
            skip(&remedy.id, "the evidence names no DLL".to_owned());
            continue;
        }
        let dll_name = dll.map(dll_base).unwrap_or_default();
        let missing = remedy.requires.iter().find_map(|requirement| {
            let met = match requirement {
                Requirement::X11Display => abilities.x11_display,
                Requirement::WineBuiltinDll => abilities.builtin_dlls.contains(&dll_name),
                Requirement::DllBesideProgram => facts
                    .dlls_beside_program
                    .iter()
                    .any(|beside| dll_base(beside) == dll_name),
                Requirement::CaseFoldingFileSystem => abilities.case_folding,
                Requirement::NtsyncDevice => abilities.ntsync,
                Requirement::FontFile => remedy
                    .font_file
                    .as_ref()
                    .is_some_and(|file| abilities.font_files.contains(file)),
            };
            (!met).then_some(*requirement)
        });
        if let Some(requirement) = missing {
            let reason = match requirement {
                Requirement::X11Display => "there is no X11 display".to_owned(),
                Requirement::WineBuiltinDll => {
                    format!("this Wine has no implementation of {dll_name}.dll")
                }
                Requirement::DllBesideProgram => {
                    format!("{dll_name}.dll is not beside the program")
                }
                Requirement::CaseFoldingFileSystem => {
                    "the file system holding the copies cannot fold case".to_owned()
                }
                Requirement::NtsyncDevice => {
                    "the ntsync device is not in use here (no /dev/ntsync, or Wine older than 11.0)"
                        .to_owned()
                }
                Requirement::FontFile => format!(
                    "{} is not on this machine",
                    remedy.font_file.as_deref().unwrap_or("the font")
                ),
            };
            skip(&remedy.id, reason);
            continue;
        }
        let reason = if matched.is_empty() {
            format!("it addresses {class}")
        } else {
            format!(
                "it addresses {class}, and the evidence shows {}",
                hint_names(&matched)
            )
        };
        chosen.push(Candidate {
            instance: instance(remedy, exe, if takes_dll { dll } else { None }),
            group: remedy.group.clone(),
            hinted: !matched.is_empty(),
            reason,
            order: remedy.order,
        });
    }
    chosen.sort_by_key(|candidate| (!candidate.hinted, candidate.order));
    CandidatePlan {
        class,
        hints: hints.to_vec(),
        candidates: chosen,
        passed_over,
    }
}

/// Time spent on one attempt, in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timings {
    /// Making the throwaway copy.
    pub snapshot_seconds: f64,
    /// Setting the remedies in the copy.
    pub remedy_seconds: f64,
    /// Running the program and watching it.
    pub run_seconds: f64,
}

/// The log of one attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogFile {
    /// Its name in the result's directory.
    pub file: String,
    /// Its SHA-256.
    pub sha256: String,
    /// Its size.
    pub bytes: u64,
}

/// What a [`TrialRunner`] reports for one attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct Trial {
    /// Why the remedies could not be set up in the copy, when they could
    /// not. The program was then not run.
    pub not_applicable: Option<String>,
    /// What the run did.
    pub run: RawRun,
    /// What the program and Wine printed, bounded.
    pub output: String,
    /// Where the time went.
    pub timings: Timings,
    /// The log, when one was written.
    pub log: Option<LogFile>,
}

/// Runs the program in a throwaway copy of its prefix with some remedies.
pub trait TrialRunner {
    /// Makes a fresh copy, sets `remedies` in it, runs the program as the
    /// check says, and discards the copy. Attempt 0 is the baseline, with no
    /// remedy.
    fn run(&mut self, number: usize, remedies: &[Resolved]) -> Trial;
}

/// How an attempt went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptOutcome {
    /// The check held.
    Passed,
    /// The check did not hold.
    Failed,
    /// The remedies could not be set up; the program was not run.
    NotApplicable,
}

/// A failure as a result records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureRecord {
    /// The class.
    pub class: FailureClass,
    /// Its code.
    pub code: String,
    /// One sentence.
    pub summary: String,
    /// The DLL the evidence names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dll: Option<String>,
    /// The function the evidence names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The Microsoft component, for `microsoft-component-missing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// The hints.
    pub hints: Vec<Hint>,
    /// The lines of evidence.
    pub evidence: Vec<String>,
}

impl From<&Diagnosis> for FailureRecord {
    fn from(diagnosis: &Diagnosis) -> Self {
        FailureRecord {
            class: diagnosis.class,
            code: diagnosis.code.to_owned(),
            summary: diagnosis.summary.clone(),
            dll: diagnosis.dll.clone(),
            function: diagnosis.function.clone(),
            component: diagnosis.component.clone(),
            hints: diagnosis.hints.clone(),
            evidence: diagnosis.evidence.clone(),
        }
    }
}

/// One attempt: the baseline (number 0) or remedies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    /// 0 for the baseline, then 1, 2, ...
    pub number: usize,
    /// The remedies tried together.
    pub remedies: Vec<RemedyInstance>,
    /// How it went.
    pub outcome: AttemptOutcome,
    /// What failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureRecord>,
    /// The parts of the check that did not hold.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    /// The program's exit status, when it exited by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<i32>,
    /// How the run ended.
    pub ended: Ended,
    /// Whether a window of the program appeared.
    pub window_seen: bool,
    /// Why the remedies could not be set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Where the time went.
    pub timings: Timings,
    /// The attempt's log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<LogFile>,
}

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The application passed its check unchanged; nothing was tried.
    PassedWithoutRemedy,
    /// A remedy, or a combination, made it pass.
    Fixed,
    /// Nothing that was tried made it pass.
    NotFixed,
    /// The failure needs something TUR cannot supply.
    Unsuppliable,
}

impl Outcome {
    /// The outcome's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Outcome::PassedWithoutRemedy => "passed-without-remedy",
            Outcome::Fixed => "fixed",
            Outcome::NotFixed => "not-fixed",
            Outcome::Unsuppliable => "unsuppliable",
        }
    }
}

/// The bounds of a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most remedy attempts, the baseline not counted.
    pub max_attempts: usize,
    /// Whether pairs are tried when no single remedy passes.
    pub combinations: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            combinations: true,
        }
    }
}

/// What a search needs to know besides the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct SearchContext<'a> {
    /// What "works" means.
    pub check: &'a CheckSpec,
    /// What is known of the program.
    pub facts: &'a StaticFacts,
    /// What the machine offers.
    pub abilities: &'a Abilities,
    /// The runtime.
    pub runtime: RuntimeKind,
}

/// What a search found.
#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    /// Every attempt, the baseline first.
    pub attempts: Vec<Attempt>,
    /// The failure of the unchanged application.
    pub failure: Option<FailureRecord>,
    /// What was chosen to be tried for it.
    pub plan: Option<CandidatePlan>,
    /// The remedies that made the check pass.
    pub found: Vec<RemedyInstance>,
    /// How it ended.
    pub outcome: Outcome,
    /// What a reader should know.
    pub notes: Vec<String>,
}

/// One attempt through the runner, held against the check.
fn attempt(
    catalogue: &Catalogue,
    context: &SearchContext<'_>,
    runner: &mut dyn TrialRunner,
    number: usize,
    instances: &[RemedyInstance],
) -> (Attempt, Option<Diagnosis>) {
    let mut resolved = Vec::new();
    for instance in instances {
        match catalogue.resolve(instance) {
            Ok(found) => resolved.push(found),
            Err(problem) => {
                return (
                    Attempt {
                        number,
                        remedies: instances.to_vec(),
                        outcome: AttemptOutcome::NotApplicable,
                        failure: None,
                        problems: Vec::new(),
                        exit_status: None,
                        ended: Ended::Exited,
                        window_seen: false,
                        note: Some(problem),
                        timings: Timings::default(),
                        log: None,
                    },
                    None,
                );
            }
        }
    }
    let trial = runner.run(number, &resolved);
    let mut record = Attempt {
        number,
        remedies: instances.to_vec(),
        outcome: AttemptOutcome::Failed,
        failure: None,
        problems: Vec::new(),
        exit_status: trial.run.exit_status,
        ended: trial.run.ended,
        window_seen: trial.run.window_seen,
        note: trial.not_applicable.clone(),
        timings: trial.timings,
        log: trial.log.clone(),
    };
    if trial.not_applicable.is_some() {
        record.outcome = AttemptOutcome::NotApplicable;
        return (record, None);
    }
    let problems = evaluate(context.check, &trial.run, &trial.output);
    if problems.is_empty() {
        record.outcome = AttemptOutcome::Passed;
        return (record, None);
    }
    let evidence = Evidence {
        exit_status: trial.run.exit_status,
        ended: trial.run.ended,
        window_required: context.check.window,
        window_seen: trial.run.window_seen,
        output: trial.output,
        check_problems: problems.clone(),
    };
    let builtin = |dll: &str| context.abilities.builtin_dlls.contains(&dll_base(dll));
    let diagnosis =
        crate::failure::diagnose(&evidence, context.facts, &builtin, &catalogue.families());
    record.failure = Some(FailureRecord::from(&diagnosis));
    record.problems = problems;
    (record, Some(diagnosis))
}

fn labels(instances: &[RemedyInstance]) -> Vec<String> {
    let mut labels: Vec<String> = instances.iter().map(|i| i.label.clone()).collect();
    labels.sort();
    labels
}

/// Runs the baseline, then single remedies, then bounded combinations,
/// and stops at the first attempt whose check holds.
///
/// Combinations are tried in two ways, both bounded by
/// [`Limits::max_attempts`]: a remedy that changed the failure into another
/// class has made progress, and the remedies for the new class are tried
/// together with it; then pairs of the first four single remedies from
/// different groups. At most [`MAX_COMBINATION`] remedies are combined.
pub fn search(
    catalogue: &Catalogue,
    context: &SearchContext<'_>,
    limits: Limits,
    runner: &mut dyn TrialRunner,
) -> Search {
    let mut attempts = Vec::new();
    let mut notes = Vec::new();
    let (baseline, diagnosis) = attempt(catalogue, context, runner, 0, &[]);
    let baseline_passed = baseline.outcome == AttemptOutcome::Passed;
    let baseline_applicable = baseline.outcome != AttemptOutcome::NotApplicable;
    attempts.push(baseline);
    let done = |attempts, failure, plan, found, outcome, notes| Search {
        attempts,
        failure,
        plan,
        found,
        outcome,
        notes,
    };
    if baseline_passed {
        notes.push("The application passed its check unchanged; no remedy was tried.".to_owned());
        return done(
            attempts,
            None,
            None,
            Vec::new(),
            Outcome::PassedWithoutRemedy,
            notes,
        );
    }
    let Some(diagnosis) = diagnosis else {
        if !baseline_applicable {
            notes.push("The unchanged copy could not be set up, so nothing was tried.".to_owned());
        }
        return done(attempts, None, None, Vec::new(), Outcome::NotFixed, notes);
    };
    let failure = FailureRecord::from(&diagnosis);
    if diagnosis.class.is_unsuppliable() {
        notes.push(format!(
            "{}. No remedy can supply it: TUR never installs Microsoft components. The Windows \
             guest route runs the application on real Windows.",
            diagnosis.summary
        ));
        return done(
            attempts,
            Some(failure),
            None,
            Vec::new(),
            Outcome::Unsuppliable,
            notes,
        );
    }
    let exe = context.facts.exe_name.clone();
    let plan = candidates(
        catalogue,
        diagnosis.class,
        diagnosis.dll.as_deref(),
        &diagnosis.hints,
        &exe,
        context.runtime,
        context.facts,
        context.abilities,
    );
    let mut tried: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut number = 1;
    let mut progress: Vec<(Candidate, Diagnosis)> = Vec::new();
    let mut failed_singles: Vec<Candidate> = Vec::new();

    for candidate in &plan.candidates {
        if number > limits.max_attempts {
            notes.push(format!(
                "The limit of {} attempts was reached.",
                limits.max_attempts
            ));
            break;
        }
        let set = vec![candidate.instance.clone()];
        tried.insert(labels(&set));
        let (record, found) = attempt(catalogue, context, runner, number, &set);
        number += 1;
        let outcome = record.outcome;
        attempts.push(record);
        match outcome {
            AttemptOutcome::Passed => {
                return done(
                    attempts,
                    Some(failure),
                    Some(plan.clone()),
                    set,
                    Outcome::Fixed,
                    notes,
                );
            }
            AttemptOutcome::Failed => {
                failed_singles.push(candidate.clone());
                if let Some(next) = found
                    && next.class != diagnosis.class
                {
                    notes.push(format!(
                        "{} changed the failure from {} to {}.",
                        candidate.instance.label, diagnosis.class, next.class
                    ));
                    progress.push((candidate.clone(), next));
                }
            }
            AttemptOutcome::NotApplicable => {}
        }
    }

    if limits.combinations && MAX_COMBINATION >= 2 {
        let mut pairs: Vec<Vec<RemedyInstance>> = Vec::new();
        // Progress first: the remedies for the new failure, on top of the
        // one that got the application past the first.
        for (first, next) in &progress {
            if next.class.is_unsuppliable() {
                continue;
            }
            let follow = candidates(
                catalogue,
                next.class,
                next.dll.as_deref(),
                &next.hints,
                &exe,
                context.runtime,
                context.facts,
                context.abilities,
            );
            for second in follow.candidates.iter().take(6) {
                if second.group != first.group {
                    pairs.push(vec![first.instance.clone(), second.instance.clone()]);
                }
            }
        }
        // Then pairs of the first single remedies.
        let top: Vec<&Candidate> = failed_singles.iter().take(4).collect();
        for (index, first) in top.iter().enumerate() {
            for second in top.iter().skip(index + 1) {
                if first.group != second.group {
                    pairs.push(vec![first.instance.clone(), second.instance.clone()]);
                }
            }
        }
        for set in pairs {
            if !tried.insert(labels(&set)) {
                continue;
            }
            if number > limits.max_attempts {
                notes.push(format!(
                    "The limit of {} attempts was reached before every combination was tried.",
                    limits.max_attempts
                ));
                break;
            }
            let (record, _) = attempt(catalogue, context, runner, number, &set);
            number += 1;
            let passed = record.outcome == AttemptOutcome::Passed;
            attempts.push(record);
            if passed {
                return done(
                    attempts,
                    Some(failure),
                    Some(plan.clone()),
                    set,
                    Outcome::Fixed,
                    notes,
                );
            }
        }
    }
    if plan.candidates.is_empty() {
        notes.push(format!(
            "No remedy in the catalogue is tried for {}.",
            diagnosis.class
        ));
    }
    done(
        attempts,
        Some(failure),
        Some(plan),
        Vec::new(),
        Outcome::NotFixed,
        notes,
    )
}

/// The SHA-256 of the evidence: every attempt as the result records it,
/// logs by their digests.
pub fn evidence_digest(attempts: &[Attempt]) -> String {
    // Times are digested in whole milliseconds: a number written and read
    // back as JSON may come back one unit in the last place off, and the
    // digest must survive that.
    fn canonical(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Number(number) if number.is_f64() => {
                let milliseconds = (number.as_f64().unwrap_or(0.0) * 1000.0).round();
                serde_json::Value::String(format!("{milliseconds:.0}ms"))
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.into_iter().map(canonical).collect())
            }
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            ),
            other => other,
        }
    }
    let value = canonical(serde_json::to_value(attempts).unwrap_or_default());
    sha256_hex(value.to_string().as_bytes())
}

/// How the throwaway copies were made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotInfo {
    /// `reflink` (copy-on-write) or `copy`.
    pub method: String,
    /// How long the first copy took.
    pub seconds: f64,
    /// The size of the copied prefix and private home.
    pub bytes: u64,
    /// The number of files in it.
    pub files: u64,
}

/// The real prefix before and after the search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixFingerprints {
    /// The fingerprint before the first copy.
    pub before: String,
    /// The fingerprint after the last attempt.
    pub after: String,
    /// Whether they are the same.
    pub unchanged: bool,
}

/// What TUR cannot supply, and the route that runs the application anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unsuppliable {
    /// The component.
    pub component: String,
    /// The DLL the evidence names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dll: Option<String>,
    /// The route suggested instead: `windows-guest`.
    pub route: String,
    /// In plain language.
    pub explanation: String,
}

/// What was known about the program before it was run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactsRecord {
    /// The program's file name.
    pub exe_name: String,
    /// The resource languages, as hexadecimal language identifiers.
    pub resource_languages: Vec<String>,
    /// The DLLs beside the program.
    pub dlls_beside_program: Vec<String>,
    /// Names on drive C: that differ only in case.
    pub case_collisions: Vec<String>,
}

impl From<&StaticFacts> for FactsRecord {
    fn from(facts: &StaticFacts) -> Self {
        FactsRecord {
            exe_name: facts.exe_name.clone(),
            resource_languages: facts
                .resource_languages
                .iter()
                .map(|language| format!("{language:04x}"))
                .collect(),
            dlls_beside_program: facts.dlls_beside_program.clone(),
            case_collisions: facts.case_collisions.clone(),
        }
    }
}

/// The measured result of one autopilot run
/// (`schemas/remedy-result.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutopilotResult {
    /// [`RESULT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `<app-id>@<time>`.
    pub result_id: String,
    /// The application.
    pub app_id: String,
    /// Its name.
    pub app_name: String,
    /// Its version, as recorded at installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    /// The SHA-256 of the file it was installed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer_sha256: Option<String>,
    /// The program that was run, as a Windows path.
    pub main_executable_windows: String,
    /// The runtime.
    pub runtime: RuntimeKind,
    /// Its version token.
    pub runtime_version: String,
    /// Everything it reported.
    pub runtime_build: String,
    /// When the run began.
    pub started_at: Timestamp,
    /// When it ended.
    pub finished_at: Timestamp,
    /// How it ended.
    pub outcome: Outcome,
    /// What "works" meant.
    pub check: CheckSpec,
    /// Whether the copies shared the host's network. Granted directories
    /// are never shown to a copy.
    pub trials_network: bool,
    /// How the copies were made.
    pub snapshot: SnapshotInfo,
    /// What was known about the program.
    pub facts: FactsRecord,
    /// The failure of the unchanged application.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureRecord>,
    /// What was chosen to be tried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CandidatePlan>,
    /// Every attempt, the baseline first.
    pub attempts: Vec<Attempt>,
    /// The remedies that made the check pass.
    pub found: Vec<RemedyInstance>,
    /// What TUR cannot supply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsuppliable: Option<Unsuppliable>,
    /// The real prefix before and after.
    pub real_prefix: PrefixFingerprints,
    /// The SHA-256 of the attempts.
    pub evidence_sha256: String,
    /// The whole run, in seconds.
    pub seconds: f64,
    /// What a reader should know.
    pub notes: Vec<String>,
}

/// Whether `text` is a result identifier, and the application it is for.
pub fn parse_result_id(text: &str) -> Option<(&str, &str)> {
    let (app_id, stamp) = text.split_once('@')?;
    let stamp_ok = stamp.len() == 16
        && stamp.bytes().enumerate().all(|(index, byte)| match index {
            8 => byte == b'T',
            15 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    (is_app_id(app_id) && stamp_ok).then_some((app_id, stamp))
}

impl AutopilotResult {
    /// Parses a result and checks it belongs to `result_id`.
    ///
    /// # Errors
    ///
    /// A message when the text is not a valid result or is another one's.
    pub fn parse(text: &str, result_id: &str) -> Result<AutopilotResult, String> {
        if text.len() > MAX_RESULT_BYTES {
            return Err("the result file is too large".to_owned());
        }
        let result: AutopilotResult =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        if result.schema_version != RESULT_SCHEMA_VERSION {
            return Err(format!("schema_version must be {RESULT_SCHEMA_VERSION}"));
        }
        if result.result_id != result_id {
            return Err(format!("the file is the result {}", result.result_id));
        }
        let Some((app_id, _)) = parse_result_id(result_id) else {
            return Err("the identifier is not a result identifier".to_owned());
        };
        if result.app_id != app_id {
            return Err("the result names another application".to_owned());
        }
        if result.evidence_sha256 != evidence_digest(&result.attempts) {
            return Err("the evidence does not match its digest".to_owned());
        }
        Ok(result)
    }

    /// The result as JSON, ending with a newline.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// A string as TOML writes it: in double quotes, escaped.
fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(character));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// The compatibility record a fixed result gives, in the catalogue's own
/// format (`catalogue/README.md`): one `[[editions.records]]` table with
/// `reported_by = "tur-autopilot"`, to be appended to the Windows edition
/// of the application's entry. `None` unless a remedy was found.
pub fn catalogue_record(result: &AutopilotResult) -> Option<String> {
    if result.outcome != Outcome::Fixed || result.found.is_empty() {
        return None;
    }
    let date: String = result.finished_at.rfc3339().chars().take(10).collect();
    let remedies: Vec<String> = result
        .found
        .iter()
        .map(|found| toml_string(&found.label))
        .collect();
    let failure = result.failure.as_ref().map_or_else(String::new, |failure| {
        format!("failed as {} ({}) unchanged; ", failure.class, failure.code)
    });
    let checked = if result.check.window {
        "a window of its own"
    } else {
        "its exit status and output"
    };
    let notes = format!(
        "tur-runtime compatibility autopilot, result {}: {failure}passed TUR's check ({checked}) \
         in a throwaway copy of its prefix with these settings, after {} attempts. {}. Measured \
         once, on one machine; not a rating.",
        result.result_id,
        result.attempts.len().saturating_sub(1),
        result.runtime_build
    );
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# A compatibility record written by tur-runtime's compatibility autopilot\n\
         # ({}). Append it to the Windows edition of the application's catalogue\n\
         # entry; `tur-catalogue validate` checks it (catalogue/README.md).",
        result.result_id
    );
    let _ = writeln!(text, "[[editions.records]]");
    let _ = writeln!(text, "route = \"wine\"");
    let _ = writeln!(text, "rating = \"unknown\"");
    let _ = writeln!(text, "reported_by = \"tur-autopilot\"");
    if let Some(version) = &result.app_version {
        let _ = writeln!(text, "version_tested = {}", toml_string(version));
    }
    let _ = writeln!(text, "date = \"{date}\"");
    let _ = writeln!(text, "remedies = [{}]", remedies.join(", "));
    let _ = writeln!(
        text,
        "runtime_version = {}",
        toml_string(&result.runtime_version)
    );
    let _ = writeln!(text, "evidence_sha256 = \"{}\"", result.evidence_sha256);
    let _ = writeln!(text, "notes = {}", toml_string(&notes));
    Some(text)
}

/// What is kept beside an application once a result is applied
/// (`schemas/remedy-applied.schema.json`): the remedies a launch carries,
/// and the backup that undoes the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedRemedies {
    /// [`APPLIED_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The application.
    pub app_id: String,
    /// The result that was applied.
    pub result_id: String,
    /// When.
    pub applied_at: Timestamp,
    /// The backup taken first; restoring it undoes the change.
    pub backup: String,
    /// The remedies.
    pub remedies: Vec<RemedyInstance>,
    /// The variables every launch of the application gets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<EnvironmentValue>,
    /// Whether `/dev/ntsync` is left out of the application's sandbox.
    #[serde(default)]
    pub hide_ntsync: bool,
    /// Whether drive C: was case-insensitive before.
    pub case_folding_before: bool,
}

impl AppliedRemedies {
    /// Parses the record kept beside `app_id`.
    ///
    /// # Errors
    ///
    /// A message when the text is not valid or is another application's.
    pub fn parse(text: &str, app_id: &str) -> Result<AppliedRemedies, String> {
        if text.len() > MAX_SMALL_FILE_BYTES {
            return Err("the file is too large".to_owned());
        }
        let applied: AppliedRemedies =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        if applied.schema_version != APPLIED_SCHEMA_VERSION {
            return Err(format!("schema_version must be {APPLIED_SCHEMA_VERSION}"));
        }
        if applied.app_id != app_id {
            return Err("the file names another application".to_owned());
        }
        let pairs: Vec<(String, String)> = applied
            .environment
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect();
        crate::runtime_cmd::check_extra_environment(&pairs).map_err(|error| error.to_string())?;
        if !crate::prefix::is_backup_reason(applied.backup.rsplit('-').next().unwrap_or("")) {
            return Err("the backup is not a backup name".to_owned());
        }
        Ok(applied)
    }

    /// The record as JSON, ending with a newline.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// The environment as name and value pairs.
    pub fn environment_pairs(&self) -> Vec<(String, String)> {
        self.environment
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::*;

    const CHECK_SCHEMA: &str = include_str!("../../../schemas/remedy-check.schema.json");
    const RESULT_SCHEMA: &str = include_str!("../../../schemas/remedy-result.schema.json");
    const APPLIED_SCHEMA: &str = include_str!("../../../schemas/remedy-applied.schema.json");

    fn keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .unwrap_or_else(|| panic!("not an object: {value}"))
            .keys()
            .cloned()
            .collect()
    }

    /// A runner whose program passes when every remedy of one of `cures`
    /// is set, and otherwise prints what `failing` says for the remedies it
    /// was given.
    struct Scripted<'a> {
        cures: Vec<Vec<&'a str>>,
        failing: &'a dyn Fn(&[Resolved]) -> String,
        calls: Vec<Vec<String>>,
    }

    impl TrialRunner for Scripted<'_> {
        fn run(&mut self, _number: usize, remedies: &[Resolved]) -> Trial {
            let ids: Vec<String> = remedies
                .iter()
                .map(|remedy| remedy.instance.label.clone())
                .collect();
            self.calls.push(ids.clone());
            let cured = self
                .cures
                .iter()
                .any(|cure| cure.iter().all(|id| ids.iter().any(|have| have == id)));
            let output = if cured {
                "TUR-CHECK-PASS\n".to_owned()
            } else {
                (self.failing)(remedies)
            };
            Trial {
                not_applicable: None,
                run: RawRun {
                    exit_status: Some(if cured { 0 } else { 3 }),
                    ended: Ended::Exited,
                    window_seen: false,
                },
                output,
                timings: Timings {
                    snapshot_seconds: 0.5,
                    remedy_seconds: 1.0,
                    run_seconds: 2.0,
                },
                log: None,
            }
        }
    }

    fn console_check() -> CheckSpec {
        CheckSpec {
            window: false,
            exit: ExitRule::Zero,
            timeout_seconds: 30,
            output_contains: vec!["TUR-CHECK-PASS".to_owned()],
            ..CheckSpec::default()
        }
    }

    fn facts() -> StaticFacts {
        StaticFacts {
            exe_name: "winver.exe".to_owned(),
            ..StaticFacts::default()
        }
    }

    fn abilities() -> Abilities {
        Abilities {
            x11_display: true,
            builtin_dlls: ["d3dx9_43", "comctl32"]
                .iter()
                .map(|n| (*n).to_owned())
                .collect(),
            ..Abilities::default()
        }
    }

    fn run_search(
        cures: Vec<Vec<&str>>,
        failing: &dyn Fn(&[Resolved]) -> String,
        facts: &StaticFacts,
        limits: Limits,
    ) -> (Search, Vec<Vec<String>>) {
        let catalogue = Catalogue::shipped();
        let check = console_check();
        let abilities = abilities();
        let context = SearchContext {
            check: &check,
            facts,
            abilities: &abilities,
            runtime: RuntimeKind::Wine,
        };
        let mut runner = Scripted {
            cures,
            failing,
            calls: Vec::new(),
        };
        let found = search(&catalogue, &context, limits, &mut runner);
        (found, runner.calls)
    }

    #[test]
    fn a_version_check_is_fixed_by_the_version_remedy_first() {
        let failing = |_: &[Resolved]| "This program requires Windows 11 or later.\n".to_owned();
        let (found, calls) = run_search(
            vec![vec!["windows-version-win11"]],
            &failing,
            &facts(),
            Limits::default(),
        );
        assert_eq!(found.outcome, Outcome::Fixed);
        assert_eq!(found.found[0].label, "windows-version-win11");
        assert_eq!(found.found[0].parameters["exe"], "winver.exe");
        // The baseline, then the hinted remedy: two runs.
        assert_eq!(calls.len(), 2);
        assert!(calls[0].is_empty());
        assert_eq!(found.attempts.len(), 2);
        assert_eq!(found.attempts[0].outcome, AttemptOutcome::Failed);
        let failure = found.failure.unwrap();
        assert_eq!(failure.class, FailureClass::ExitedWithError);
        assert!(failure.hints.contains(&Hint::WindowsVersion));
    }

    #[test]
    fn a_broken_dll_is_answered_with_wine_s_own() {
        let failing = |_: &[Resolved]| {
            "0024:err:module:LdrInitializeThunk \"d3dx9_43.dll\" failed to initialize, aborting\n"
                .to_owned()
        };
        let (found, calls) = run_search(
            vec![vec!["dll-builtin:d3dx9_43"]],
            &failing,
            &facts(),
            Limits::default(),
        );
        assert_eq!(found.outcome, Outcome::Fixed);
        assert_eq!(found.found[0].label, "dll-builtin:d3dx9_43");
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn a_missing_microsoft_component_is_not_tried_at_all() {
        let failing = |_: &[Resolved]| {
            "err:module:import_dll Library mfc140u.dll (which is needed by L\"x\") not found\n"
                .to_owned()
        };
        let (found, calls) = run_search(Vec::new(), &failing, &facts(), Limits::default());
        assert_eq!(found.outcome, Outcome::Unsuppliable);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            found.failure.unwrap().class,
            FailureClass::MicrosoftComponentMissing
        );
    }

    #[test]
    fn progress_is_combined_with_the_remedy_for_the_next_failure() {
        // The program needs Wine's own d3dx9_43 and then Windows 11.
        let failing = |remedies: &[Resolved]| {
            let has = |label: &str| remedies.iter().any(|r| r.instance.label == label);
            if has("dll-builtin:d3dx9_43") {
                "This program requires Windows 11 or later.\n".to_owned()
            } else {
                "err:module:LdrInitializeThunk \"d3dx9_43.dll\" failed to initialize, aborting\n"
                    .to_owned()
            }
        };
        let (found, calls) = run_search(
            vec![vec!["dll-builtin:d3dx9_43", "windows-version-win11"]],
            &failing,
            &facts(),
            Limits::default(),
        );
        assert_eq!(found.outcome, Outcome::Fixed);
        let mut labels: Vec<&str> = found.found.iter().map(|f| f.label.as_str()).collect();
        labels.sort_unstable();
        assert_eq!(labels, ["dll-builtin:d3dx9_43", "windows-version-win11"]);
        assert!(calls.len() <= 1 + DEFAULT_MAX_ATTEMPTS);
        assert!(
            found
                .notes
                .iter()
                .any(|note| note.contains("changed the failure"))
        );
    }

    #[test]
    fn the_attempt_limit_is_kept() {
        let failing = |_: &[Resolved]| "This program requires Windows 11 or later.\n".to_owned();
        let limits = Limits {
            max_attempts: 2,
            combinations: true,
        };
        let (found, calls) = run_search(Vec::new(), &failing, &facts(), limits);
        assert_eq!(found.outcome, Outcome::NotFixed);
        assert_eq!(calls.len(), 3);
    }

    #[test]
    fn candidates_say_why_a_remedy_is_not_tried() {
        let catalogue = Catalogue::shipped();
        let plan = candidates(
            &catalogue,
            FailureClass::Crash,
            None,
            &[Hint::LocaleJa],
            "a.exe",
            RuntimeKind::Wine,
            &facts(),
            &Abilities::default(),
        );
        assert_eq!(plan.candidates[0].instance.label, "locale-ja");
        assert!(plan.candidates[0].hinted);
        let reason = |id: &str| {
            plan.passed_over
                .iter()
                .find(|passed| passed.remedy == id)
                .map(|passed| passed.reason.clone())
                .unwrap_or_default()
        };
        assert!(reason("virtual-desktop-1024x768").contains("X11"));
        assert!(reason("dll-builtin").contains("names no DLL"));
        assert!(reason("renderer-gl").contains("direct3d"));
        assert!(reason("proton-no-esync").contains("proton"));
        assert!(reason("ntsync-off").contains("ntsync"));
        assert!(
            !plan
                .candidates
                .iter()
                .any(|candidate| candidate.instance.id == "drive-c-case-insensitive")
        );
    }

    #[test]
    fn a_run_is_held_against_the_check() {
        let check = console_check();
        let exited = |status| RawRun {
            exit_status: Some(status),
            ended: Ended::Exited,
            window_seen: false,
        };
        assert!(evaluate(&check, &exited(0), "x\nTUR-CHECK-PASS\n").is_empty());
        assert_eq!(evaluate(&check, &exited(0), "nothing").len(), 1);
        assert_eq!(evaluate(&check, &exited(2), "TUR-CHECK-PASS").len(), 1);
        let crashed = evaluate(
            &check,
            &exited(0),
            "TUR-CHECK-PASS\nwine: Unhandled page fault on read access, starting debugger...",
        );
        assert!(crashed[0].contains("Wine reported"));
        let window = CheckSpec::default();
        let stopped = RawRun {
            exit_status: None,
            ended: Ended::Stopped,
            window_seen: true,
        };
        assert!(evaluate(&window, &stopped, "").is_empty());
        let timed_out = RawRun {
            window_seen: false,
            ended: Ended::TimedOut,
            ..stopped
        };
        assert_eq!(
            evaluate(&window, &timed_out, ""),
            ["no window of the program appeared"]
        );
    }

    #[test]
    fn checks_are_validated() {
        assert!(CheckSpec::default().validate().is_ok());
        let text = serde_json::to_string(&console_check()).unwrap();
        assert_eq!(CheckSpec::parse(&text).unwrap(), console_check());
        let mut slow = console_check();
        slow.timeout_seconds = 0;
        assert!(slow.validate().is_err());
        let mut odd = console_check();
        odd.window_class = Some("../x".to_owned());
        assert!(odd.validate().is_err());
        assert!(CheckSpec::parse("{\"schema_version\":1,\"colour\":1}").is_err());
        assert_eq!(CheckSpec::default().class_for("7zFM.exe"), "7zfm.exe");
    }

    fn result() -> AutopilotResult {
        let failing = |_: &[Resolved]| "This program requires Windows 11 or later.\n".to_owned();
        let (found, _) = run_search(
            vec![vec!["windows-version-win11"]],
            &failing,
            &facts(),
            Limits::default(),
        );
        AutopilotResult {
            schema_version: RESULT_SCHEMA_VERSION,
            result_id: "tur.test.winver@20261113T101500Z".to_owned(),
            app_id: "tur.test.winver".to_owned(),
            app_name: "Version \"check\"".to_owned(),
            app_version: Some("1.0".to_owned()),
            installer_sha256: Some("ab".repeat(32)),
            main_executable_windows: "C:\\Program Files\\winver\\winver.exe".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: "wine-9.0".to_owned(),
            runtime_build: "wine-9.0 (Ubuntu 9.0~repack-4build3)".to_owned(),
            started_at: Timestamp::from_unix(1_794_565_000),
            finished_at: Timestamp::from_unix(1_794_565_100),
            outcome: found.outcome,
            check: console_check(),
            trials_network: false,
            snapshot: SnapshotInfo {
                method: "copy".to_owned(),
                seconds: 0.8,
                bytes: 1000,
                files: 10,
            },
            facts: FactsRecord::from(&facts()),
            failure: found.failure,
            plan: found.plan,
            evidence_sha256: evidence_digest(&found.attempts),
            attempts: found.attempts,
            found: found.found,
            unsuppliable: None,
            real_prefix: PrefixFingerprints {
                before: "a".repeat(64),
                after: "a".repeat(64),
                unchanged: true,
            },
            seconds: 5.5,
            notes: found.notes,
        }
    }

    #[test]
    fn a_result_survives_a_round_trip_and_tampering_is_seen() {
        let result = result();
        let text = result.to_json();
        let back = AutopilotResult::parse(&text, &result.result_id).unwrap();
        assert_eq!(back, result);
        assert!(AutopilotResult::parse(&text, "tur.test.other@20261113T101500Z").is_err());
        let mut tampered = result.clone();
        tampered.attempts[1].outcome = AttemptOutcome::Passed;
        tampered.attempts[0].outcome = AttemptOutcome::Passed;
        assert!(AutopilotResult::parse(&tampered.to_json(), &result.result_id).is_err());
        assert_eq!(
            parse_result_id("tur.test.winver@20261113T101500Z"),
            Some(("tur.test.winver", "20261113T101500Z"))
        );
        assert_eq!(parse_result_id("tur.test.winver@2026"), None);
        assert_eq!(parse_result_id("../x@20261113T101500Z"), None);
    }

    #[test]
    fn a_fixed_result_gives_a_catalogue_record() {
        let result = result();
        let record = catalogue_record(&result).unwrap();
        assert!(record.contains("[[editions.records]]\n"));
        assert!(record.contains("reported_by = \"tur-autopilot\"\n"));
        assert!(record.contains("rating = \"unknown\"\n"));
        assert!(record.contains("remedies = [\"windows-version-win11\"]\n"));
        assert!(record.contains("runtime_version = \"wine-9.0\"\n"));
        assert!(record.contains(&format!("evidence_sha256 = \"{}\"", result.evidence_sha256)));
        assert!(record.contains("date = \"2026-11-13\""));
        assert!(record.contains("version_tested = \"1.0\""));
        let mut unfixed = result;
        unfixed.outcome = Outcome::NotFixed;
        assert!(catalogue_record(&unfixed).is_none());
        assert_eq!(toml_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn the_schemas_and_the_rust_types_name_the_same_fields() {
        let check: Value = serde_json::from_str(CHECK_SCHEMA).unwrap();
        let mut full_check = serde_json::to_value(console_check()).unwrap();
        full_check["window_class"] = Value::from("a.exe");
        full_check["output_lacks"] = serde_json::json!(["x"]);
        full_check["arguments"] = serde_json::json!(["--x"]);
        assert_eq!(keys(&full_check), keys(&check["properties"]));

        let schema: Value = serde_json::from_str(RESULT_SCHEMA).unwrap();
        let mut written = serde_json::to_value(result()).unwrap();
        written["unsuppliable"] = serde_json::to_value(Unsuppliable {
            component: "x".to_owned(),
            dll: None,
            route: "windows-guest".to_owned(),
            explanation: "y".to_owned(),
        })
        .unwrap();
        assert_eq!(keys(&written), keys(&schema["properties"]));
        let attempt = &written["attempts"][0];
        let mut attempt = attempt.clone();
        attempt["note"] = Value::from("n");
        attempt["log"] = serde_json::to_value(LogFile {
            file: "attempt-0.log".to_owned(),
            sha256: "0".repeat(64),
            bytes: 1,
        })
        .unwrap();
        assert_eq!(
            keys(&attempt),
            keys(&schema["$defs"]["attempt"]["properties"])
        );
        let mut failure = written["failure"].clone();
        failure["dll"] = Value::from("a.dll");
        failure["function"] = Value::from("f");
        failure["component"] = Value::from("c");
        assert_eq!(
            keys(&failure),
            keys(&schema["$defs"]["failure"]["properties"])
        );
        assert_eq!(
            keys(&written["plan"]["candidates"][0]),
            keys(&schema["$defs"]["candidate"]["properties"])
        );
        assert_eq!(
            keys(&written["found"][0]),
            keys(&schema["$defs"]["remedy_instance"]["properties"])
        );

        let applied: Value = serde_json::from_str(APPLIED_SCHEMA).unwrap();
        let record = AppliedRemedies {
            schema_version: APPLIED_SCHEMA_VERSION,
            app_id: "tur.test.winver".to_owned(),
            result_id: "tur.test.winver@20261113T101500Z".to_owned(),
            applied_at: Timestamp::from_unix(1_794_565_200),
            backup: "20261113T102000Z-autopilot".to_owned(),
            remedies: Vec::new(),
            environment: vec![EnvironmentValue {
                name: "LC_ALL".to_owned(),
                value: "ja_JP.UTF-8".to_owned(),
            }],
            hide_ntsync: false,
            case_folding_before: false,
        };
        assert_eq!(
            keys(&serde_json::to_value(&record).unwrap()),
            keys(&applied["properties"])
        );
        let text = record.to_json();
        assert_eq!(
            AppliedRemedies::parse(&text, "tur.test.winver").unwrap(),
            record
        );
        assert!(AppliedRemedies::parse(&text, "tur.test.other").is_err());
        let mut hostile = record;
        hostile.environment[0].name = "LD_PRELOAD".to_owned();
        assert!(AppliedRemedies::parse(&hostile.to_json(), "tur.test.winver").is_err());

        for schema in [&check, &schema, &applied] {
            assert_eq!(schema["additionalProperties"], Value::Bool(false));
        }
        let _ = BTreeMap::<String, String>::new();
    }
}

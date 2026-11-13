// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The fault catalogue: what self-healing looks for and what it may do.
//!
//! The catalogue is data (`heal/catalogue.json`, format
//! `schemas/heal-catalogue.schema.json`), compiled into the program. An
//! entry names a check of one of a fixed set of kinds ([`Check`]) and
//! remedies of a fixed set of kinds ([`RemedyKind`]): no entry can express a
//! command line, a script or a download. [`Catalogue::validate`] holds every
//! entry to the rules of the design (docs/architecture/self-healing.md,
//! section 3), and a test holds the shipped catalogue to them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tur_core::ErrorCode;

use crate::report::{COMPONENTS, ReportKind};

/// The catalogue format this build reads.
pub const CATALOGUE_SCHEMA_VERSION: u32 = 1;

/// The catalogue shipped with this build.
pub const BUILT_IN: &str = include_str!("../../heal/catalogue.json");

/// The whole catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// The format version, [`CATALOGUE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// What the file is, for a person reading it.
    pub about: String,
    /// The faults, in the order they are checked.
    pub faults: Vec<Fault>,
}

/// One fault TUR can notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    /// The stable error code, for example `TUR-NETWORK-TIME-001`.
    pub code: String,
    /// A short title a person understands.
    pub title: String,
    /// The component concerned, from the problem-report list.
    pub component: String,
    /// What is wrong and what it does, in plain words.
    pub explanation: String,
    /// How the fault is detected. The repair is verified by the same check.
    pub check: Check,
    /// What may be done about it, in order of preference.
    pub remedies: Vec<Remedy>,
    /// How long a repair may take to show, in seconds: the check is run
    /// again once a second until it passes or this time is up.
    pub verify_seconds: u64,
    /// When a problem report is offered.
    pub report: ReportPolicy,
    /// The kind of the report, when one is offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_kind: Option<ReportKind>,
    /// What the person is told.
    pub notice: NoticeText,
    /// Where the check and the remedies were verified.
    pub sources: Vec<Source>,
}

/// A detection check. Each kind is implemented in `checks.rs` and does one
/// thing; an entry only gives it its parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Check {
    /// One of these systemd units is in the failed state.
    UnitFailed {
        /// The units watched.
        units: Vec<String>,
    },
    /// Setting the clock from the network is possible and switched off.
    NetworkTimeOff,
    /// The resolver service runs, and `path` is neither a link to one of
    /// its files nor a file that names only its stub.
    ResolverNotManaged {
        /// The resolver's unit.
        service: String,
        /// The resolver configuration, `/etc/resolv.conf`.
        path: String,
        /// Link targets that mean the resolver manages the file.
        managed_targets: Vec<String>,
        /// The address of the resolver's local stub.
        stub_address: String,
        /// Where the remedy points the link.
        restore_target: String,
    },
    /// Little free space where `path` lies.
    FreeSpaceLow {
        /// The directory whose file system is measured.
        path: String,
        /// Less free than this many bytes is a fault.
        min_free_bytes: u64,
        /// Less free than this share of the file system, in percent, is a
        /// fault.
        min_free_percent: u64,
    },
    /// A program of TUR's crashed during this boot.
    TurProgramCrashed {
        /// Path prefixes of TUR's programs.
        prefixes: Vec<String>,
        /// The component of each program, by its file name.
        components: BTreeMap<String, String>,
    },
}

impl Check {
    /// The kind's name, as written in the catalogue.
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Check::UnitFailed { .. } => "unit-failed",
            Check::NetworkTimeOff => "network-time-off",
            Check::ResolverNotManaged { .. } => "resolver-not-managed",
            Check::FreeSpaceLow { .. } => "free-space-low",
            Check::TurProgramCrashed { .. } => "tur-program-crashed",
        }
    }
}

/// One way of repairing a fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remedy {
    /// Its name, unique within the fault.
    pub id: String,
    /// What it does: one of a fixed set of operations.
    pub kind: RemedyKind,
    /// Its risk class (ai-control-layer.md, section 5): 0 observes, 1 is
    /// reversible, 2 is consequential and needs the person's approval.
    pub class: u8,
    /// Whether TUR's healing service applies it by itself when it finds
    /// the fault. Only a class 0 or 1 remedy with an undo may be.
    pub automatic: bool,
    /// How often it may be applied in one boot. Default 1 for the service,
    /// 3 for requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_boot: Option<u32>,
    /// For `vacuum-journal`: how much of the logs is kept, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_bytes: Option<u64>,
    /// What it changes, in plain words.
    pub changes: String,
    /// Whether it can be undone, or changes nothing lasting.
    pub reversible: bool,
    /// How it is undone, in plain words.
    pub undo: String,
}

/// The operations a remedy can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemedyKind {
    /// Clear the failed state of the failed units the check found and start
    /// each once.
    RestartUnits,
    /// `timedatectl set-ntp true`; undone with `false`.
    SetNetworkTime,
    /// Keep a copy of the resolver configuration, then point it at the
    /// resolver; undone by putting the copy back.
    RestoreResolverLink,
    /// `journalctl --rotate --vacuum-size=…`. Cannot be undone.
    VacuumJournal,
}

impl RemedyKind {
    /// Whether the operation can be undone or changes nothing lasting.
    pub const fn reversible(self) -> bool {
        !matches!(self, RemedyKind::VacuumJournal)
    }

    /// The check kind the operation answers.
    pub const fn answers(self) -> &'static str {
        match self {
            RemedyKind::RestartUnits => "unit-failed",
            RemedyKind::SetNetworkTime => "network-time-off",
            RemedyKind::RestoreResolverLink => "resolver-not-managed",
            RemedyKind::VacuumJournal => "free-space-low",
        }
    }
}

/// When a problem report is offered for a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportPolicy {
    /// Never: the cause is not in TUR.
    Never,
    /// When the remedies did not repair it.
    WhenUnrepaired,
    /// Whenever it is found: the cause is in TUR and nothing is safe to try.
    Always,
}

/// What the person is told, with `{subjects}` standing for what the check
/// found (units, programs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoticeText {
    /// When it is found and waits for the person.
    pub found: String,
    /// When a remedy repaired it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repaired: Option<String>,
    /// When the remedies did not repair it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unrepaired: Option<String>,
    /// When a report is offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
}

/// Where something in an entry was verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// What was verified.
    pub what: String,
    /// Where: an `https` address.
    #[serde(rename = "where")]
    pub location: String,
    /// When it was read, `YYYY-MM-DD`.
    pub read: String,
}

impl Catalogue {
    /// Parses a catalogue. It is not validated: see [`Catalogue::validate`].
    ///
    /// # Errors
    ///
    /// The text is not a catalogue of this format.
    pub fn from_json(text: &str) -> Result<Catalogue, String> {
        serde_json::from_str(text).map_err(|error| error.to_string())
    }

    /// The catalogue compiled into this build, validated.
    ///
    /// # Errors
    ///
    /// The built-in catalogue does not parse or breaks a rule. The tests
    /// prove it does neither, so a released build cannot reach this.
    pub fn built_in() -> Result<Catalogue, String> {
        let catalogue = Catalogue::from_json(BUILT_IN)?;
        let problems = catalogue.validate();
        if problems.is_empty() {
            Ok(catalogue)
        } else {
            Err(problems.join("; "))
        }
    }

    /// The entry for `code`.
    pub fn fault(&self, code: &str) -> Option<&Fault> {
        self.faults.iter().find(|fault| fault.code == code)
    }

    /// Every rule the catalogue breaks, empty when it breaks none.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema_version != CATALOGUE_SCHEMA_VERSION {
            problems.push(format!(
                "schema_version is {}, this build reads {CATALOGUE_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.faults.is_empty() {
            problems.push("the catalogue lists no fault".to_owned());
        }
        let mut codes = BTreeSet::new();
        for fault in &self.faults {
            if !codes.insert(fault.code.as_str()) {
                problems.push(format!("{} is listed twice", fault.code));
            }
            problems.extend(
                fault
                    .problems()
                    .into_iter()
                    .map(|problem| format!("{}: {problem}", fault.code)),
            );
        }
        problems
    }
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

fn is_remedy_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    id.len() <= 64
        && matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

impl Fault {
    /// The remedy named `id`.
    pub fn remedy(&self, id: &str) -> Option<&Remedy> {
        self.remedies.iter().find(|remedy| remedy.id == id)
    }

    /// The remedy the service applies by itself, if there is one.
    pub fn automatic_remedy(&self) -> Option<&Remedy> {
        self.remedies.iter().find(|remedy| remedy.automatic)
    }

    /// Every rule this entry breaks.
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        match self.code.parse::<ErrorCode>() {
            Ok(code) if code.as_str().starts_with("TUR-DIAG-HEAL-") => {
                problems.push("TUR-DIAG-HEAL-* codes are tur-diag's own errors".to_owned());
            }
            Ok(_) => {}
            Err(error) => problems.push(format!("the code does not parse: {error}")),
        }
        if !COMPONENTS.contains(&self.component.as_str()) {
            problems.push(format!("{} is not a report component", self.component));
        }
        for (field, text) in [("title", &self.title), ("explanation", &self.explanation)] {
            if blank(text) {
                problems.push(format!("{field} is empty"));
            }
        }
        if self.title.chars().count() > 120 {
            problems.push("the title is longer than 120 characters".to_owned());
        }
        if blank(&self.notice.found) {
            problems.push("notice.found is empty".to_owned());
        }
        if self.verify_seconds > 120 {
            problems.push("verify_seconds is above 120".to_owned());
        }
        self.check_problems(&mut problems);

        let mut ids = BTreeSet::new();
        for remedy in &self.remedies {
            let at = format!("remedy {}", remedy.id);
            if !is_remedy_id(&remedy.id) {
                problems.push(format!("{at}: the id is not lower-case words joined by -"));
            }
            if !ids.insert(remedy.id.as_str()) {
                problems.push(format!("{at}: listed twice"));
            }
            if remedy.kind.answers() != self.check.kind_name() {
                problems.push(format!(
                    "{at}: a {:?} remedy does not answer a {} check",
                    remedy.kind,
                    self.check.kind_name()
                ));
            }
            if remedy.class > 2 {
                problems.push(format!(
                    "{at}: class {} is not offered: a system-critical operation is never a \
                     catalogue remedy",
                    remedy.class
                ));
            }
            if remedy.reversible != remedy.kind.reversible() {
                problems.push(format!(
                    "{at}: reversible says {}, the operation is {}",
                    remedy.reversible,
                    if remedy.kind.reversible() {
                        "reversible"
                    } else {
                        "irreversible"
                    }
                ));
            }
            if remedy.class <= 1 && !remedy.reversible {
                problems.push(format!(
                    "{at}: class {} must be reversible; an irreversible remedy is class 2",
                    remedy.class
                ));
            }
            if remedy.automatic && (remedy.class > 1 || !remedy.reversible) {
                problems.push(format!(
                    "{at}: only a reversible remedy of class 0 or 1 may be automatic"
                ));
            }
            if remedy.kind == RemedyKind::VacuumJournal && remedy.keep_bytes.is_none() {
                problems.push(format!("{at}: keep_bytes is required"));
            }
            if remedy.kind != RemedyKind::VacuumJournal && remedy.keep_bytes.is_some() {
                problems.push(format!("{at}: keep_bytes means nothing here"));
            }
            if remedy.max_per_boot == Some(0) {
                problems.push(format!("{at}: max_per_boot must be at least 1"));
            }
            if blank(&remedy.changes) || blank(&remedy.undo) {
                problems.push(format!("{at}: changes and undo must say something"));
            }
        }
        if self
            .remedies
            .iter()
            .filter(|remedy| remedy.automatic)
            .count()
            > 1
        {
            problems.push("more than one automatic remedy".to_owned());
        }
        if !self.remedies.is_empty() && self.notice.repaired.as_deref().is_none_or(blank) {
            problems.push("notice.repaired is needed when there is a remedy".to_owned());
        }
        if self.remedies.is_empty() && self.report != ReportPolicy::Always {
            problems.push(
                "a fault without a remedy must offer a report: report must be \"always\""
                    .to_owned(),
            );
        }
        match (self.report, self.report_kind) {
            (ReportPolicy::Never, Some(_)) => {
                problems.push("report_kind is given but no report is offered".to_owned());
            }
            (ReportPolicy::WhenUnrepaired | ReportPolicy::Always, None) => {
                problems.push("report_kind is needed when a report is offered".to_owned());
            }
            _ => {}
        }
        if self.report != ReportPolicy::Never && self.notice.report.as_deref().is_none_or(blank) {
            problems.push("notice.report is needed when a report is offered".to_owned());
        }
        if self.sources.is_empty() {
            problems.push("no source says where the check and remedies were verified".to_owned());
        }
        for source in &self.sources {
            if !source.location.starts_with("https://") || blank(&source.what) {
                problems.push(format!(
                    "source {:?} is not an https address",
                    source.location
                ));
            }
            if !is_date(&source.read) {
                problems.push(format!(
                    "source {:?}: read is not YYYY-MM-DD",
                    source.location
                ));
            }
        }
        problems
    }

    fn check_problems(&self, problems: &mut Vec<String>) {
        match &self.check {
            Check::UnitFailed { units } => {
                if units.is_empty() {
                    problems.push("unit-failed names no unit".to_owned());
                }
                for unit in units {
                    let valid = unit.ends_with(".service")
                        && !unit.starts_with('-')
                        && unit
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.@:".contains(&b));
                    if !valid {
                        problems.push(format!("{unit:?} is not a service unit name"));
                    }
                }
            }
            Check::NetworkTimeOff => {}
            Check::ResolverNotManaged {
                service,
                path,
                managed_targets,
                stub_address,
                restore_target,
            } => {
                if !service.ends_with(".service") || !path.starts_with('/') {
                    problems.push("resolver-not-managed needs a service and a path".to_owned());
                }
                if !managed_targets.contains(restore_target) {
                    problems.push(
                        "restore_target must be one of managed_targets, or the repair could \
                         never verify"
                            .to_owned(),
                    );
                }
                if stub_address.parse::<std::net::IpAddr>().is_err() {
                    problems.push("stub_address is not an address".to_owned());
                }
            }
            Check::FreeSpaceLow {
                path,
                min_free_bytes,
                min_free_percent,
            } => {
                if !path.starts_with('/') || (*min_free_bytes == 0 && *min_free_percent == 0) {
                    problems.push("free-space-low needs a path and a limit".to_owned());
                }
                if *min_free_percent > 50 {
                    problems.push("min_free_percent above 50".to_owned());
                }
            }
            Check::TurProgramCrashed {
                prefixes,
                components,
            } => {
                if prefixes.is_empty() || prefixes.iter().any(|prefix| !prefix.starts_with('/')) {
                    problems.push("tur-program-crashed needs absolute path prefixes".to_owned());
                }
                for (program, component) in components {
                    if !COMPONENTS.contains(&component.as_str()) {
                        problems.push(format!("{program}: {component} is not a report component"));
                    }
                }
            }
        }
    }
}

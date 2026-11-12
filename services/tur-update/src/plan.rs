// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Plans: the exact commands an operation would run, and what they would do.
//!
//! **Nothing here runs a command.** A plan is data. It is made only for a
//! decision that says the operation may proceed; for any other decision the
//! plan has no steps, and says so.
//!
//! Every command a plan can name is in [`COMMANDS`], with its program by
//! full path, its options and what it changes. The `update` workflow holds
//! that table against the programs in the base image: each program must be
//! there, and each option must be one its `--help` describes.
//!
//! The commands and their meaning were read from the documentation and
//! source of bootc 1.17.1 and OSTree 2026.6
//! (`docs/architecture/update-manager.md`, section 3).

use serde::Serialize;

use crate::conditions::Conditions;
use crate::policy::{
    Decision, Requester, decide_pin, decide_rollback, decide_stage, decide_unpin,
};
use crate::probe::STATUS_PROGRAM;
use crate::state::{Slot, SystemState, UpdateKnowledge};

string_enum! {
    /// An operation that changes the system, and so is only planned.
    pub enum Operation {
        /// Download the known update, stage it, and release it.
        Stage => "stage",
        /// Go back to the previous version.
        Rollback => "rollback",
        /// Keep a version.
        Pin => "pin",
        /// Release a kept version.
        Unpin => "unpin",
    }
}

string_enum! {
    /// What a command changes.
    pub enum Changes {
        /// Nothing: it only reads.
        Nothing => "nothing",
        /// Only the cached result of a check. No deployment changes.
        UpdateMetadata => "update-metadata",
        /// Deployments, the start-up order, or what is kept.
        System => "system",
    }
}

/// The program that manages deployments.
pub const BOOTC: &str = STATUS_PROGRAM;

/// The program that pins deployments. bootc has no command for it.
pub const OSTREE: &str = "/usr/bin/ostree";

/// A command of another program that this component names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExternalCommand {
    /// A short name for the command.
    pub id: &'static str,
    /// The program, by full path.
    pub program: &'static str,
    /// The subcommand, one word or more.
    pub subcommand: &'static [&'static str],
    /// The options.
    pub options: &'static [&'static str],
    /// The values its one operand may take; empty if it takes none.
    pub operands: &'static [&'static str],
    /// What it changes.
    pub changes: Changes,
    /// Whether this program ever runs it. True for one command only.
    pub run_by_this_program: bool,
    /// What it is for.
    pub purpose: &'static str,
}

impl ExternalCommand {
    /// The argument vector, with the program first.
    pub fn argv(&self, operand: Option<&str>) -> Vec<String> {
        let mut argv = vec![self.program.to_owned()];
        argv.extend(self.subcommand.iter().map(|word| (*word).to_owned()));
        argv.extend(self.options.iter().map(|option| (*option).to_owned()));
        argv.extend(operand.map(str::to_owned));
        argv
    }
}

/// Reads the deployments. The one command this program runs.
pub const READ_STATUS: ExternalCommand = ExternalCommand {
    id: "read-status",
    program: BOOTC,
    subcommand: &["status"],
    options: &["--format=json"],
    operands: &[],
    changes: Changes::Nothing,
    run_by_this_program: true,
    purpose: "Print the deployments as JSON. The only command this program runs, and only when \
              it is asked about this machine with --live.",
};

/// Checks for an update.
pub const CHECK: ExternalCommand = ExternalCommand {
    id: "check",
    program: BOOTC,
    subcommand: &["upgrade"],
    options: &["--check"],
    operands: &[],
    changes: Changes::UpdateMetadata,
    run_by_this_program: false,
    purpose: "Fetch the manifest and configuration of the image the system follows, a few \
              kilobytes, and keep them as the cached update. No deployment changes.",
};

/// Downloads and stages the update, held.
pub const STAGE_HELD: ExternalCommand = ExternalCommand {
    id: "stage-held",
    program: BOOTC,
    subcommand: &["upgrade"],
    options: &["--download-only", "--quiet"],
    operands: &[],
    changes: Changes::System,
    run_by_this_program: false,
    purpose: "Download the image and stage it beside the running system, held: it is not used \
              at the next restart, and a restart discards it.",
};

/// Releases the held deployment.
pub const RELEASE: ExternalCommand = ExternalCommand {
    id: "release",
    program: BOOTC,
    subcommand: &["upgrade"],
    options: &["--from-downloaded"],
    operands: &[],
    changes: Changes::System,
    run_by_this_program: false,
    purpose: "Release the held deployment without fetching anything: it is finalised at the \
              next shutdown and started at the next boot.",
};

/// Makes the previous deployment the default.
pub const ROLLBACK: ExternalCommand = ExternalCommand {
    id: "rollback",
    program: BOOTC,
    subcommand: &["rollback"],
    options: &[],
    operands: &[],
    changes: Changes::System,
    run_by_this_program: false,
    purpose: "Make the rollback deployment first in the start-up order. A staged deployment is \
              discarded. Run a second time, it swaps the order back.",
};

/// Pins a deployment.
pub const PIN: ExternalCommand = ExternalCommand {
    id: "pin",
    program: OSTREE,
    subcommand: &["admin", "pin"],
    options: &[],
    operands: &["booted", "rollback"],
    changes: Changes::System,
    run_by_this_program: false,
    purpose: "Pin a deployment: it is never removed by garbage collection.",
};

/// Unpins a deployment.
pub const UNPIN: ExternalCommand = ExternalCommand {
    id: "unpin",
    program: OSTREE,
    subcommand: &["admin", "pin"],
    options: &["--unpin"],
    operands: &["booted", "rollback"],
    changes: Changes::System,
    run_by_this_program: false,
    purpose: "Undo a pin: the deployment is removed like any other when it is no longer the \
              booted or the previous one.",
};

/// Every command of another program this component names, anywhere.
pub const COMMANDS: [ExternalCommand; 7] =
    [READ_STATUS, CHECK, STAGE_HELD, RELEASE, ROLLBACK, PIN, UNPIN];

/// How many attempts a new version gets (ADR-0014).
pub const TRIAL_ATTEMPTS: u8 = 3;

/// One command of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The name of the command in [`COMMANDS`].
    pub id: &'static str,
    /// The argument vector, with the program first.
    pub command: Vec<String>,
    /// What the command changes.
    pub changes: Changes,
    /// What this step does, in this plan.
    pub effect: String,
}

fn step(command: &ExternalCommand, operand: Option<&str>, effect: String) -> Step {
    Step {
        id: command.id,
        command: command.argv(operand),
        changes: command.changes,
        effect,
    }
}

/// What an operation would do. Nothing in it has been done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The operation.
    pub operation: Operation,
    /// The decision of the policy for it.
    pub decision: Decision,
    /// The commands, in order. Empty unless the decision is to proceed.
    pub steps: Vec<Step>,
    /// What the user would notice, in plain sentences.
    pub effects: Vec<String>,
    /// Whether the change takes effect only at a restart.
    pub restart_required: bool,
    /// Always false: this program runs no step of any plan.
    pub executed: bool,
}

impl Plan {
    fn new(operation: Operation, decision: Decision) -> Plan {
        Plan {
            operation,
            decision,
            steps: Vec::new(),
            effects: Vec::new(),
            restart_required: false,
            executed: false,
        }
    }
}

/// The plan for downloading and staging the known update.
pub fn plan_stage(state: &SystemState, conditions: &Conditions, requester: Requester) -> Plan {
    let mut plan = Plan::new(
        Operation::Stage,
        decide_stage(state, conditions, requester),
    );
    if !plan.decision.proceeds() {
        return plan;
    }
    let current = state.booted.label();
    // What is staged: the known update, or the held deployment to release.
    let held = state.staged.as_ref().filter(|staged| {
        staged.download_only
            && match &state.update {
                UpdateKnowledge::Available(image) => staged.digest() == Some(image.digest.as_str()),
                UpdateKnowledge::NotChecked | UpdateKnowledge::UpToDate => true,
            }
    });
    let (new, digest) = match (held, state.update.image()) {
        (Some(staged), _) => (staged.label(), staged.digest().map(str::to_owned)),
        (None, Some(image)) => (image.label(), Some(image.digest.clone())),
        // Not reachable: the decision proceeds only for one of the two.
        (None, None) => return plan,
    };
    let expected = match &digest {
        Some(digest) => format!("the digest {digest}"),
        None => "what was expected".to_owned(),
    };
    if held.is_none() {
        plan.steps.push(step(
            &STAGE_HELD,
            None,
            format!(
                "Downloads {new} and stages it beside {current}, held. The running system is \
                 not changed."
            ),
        ));
    }
    plan.steps.push(step(
        &READ_STATUS,
        None,
        format!(
            "Reads back what is staged. TUR compares it with {expected} and with the list of \
             versions that failed to start; if it is anything else, the next step is not run \
             and the staged version stays held."
        ),
    ));
    plan.steps.push(step(
        &RELEASE,
        None,
        format!("Releases {new}: it is used at the next restart. Nothing is fetched."),
    ));

    if held.is_none() {
        plan.effects.push(format!(
            "{new} is downloaded and stored beside the version in use. {current} keeps running \
             and is not changed."
        ));
    } else {
        plan.effects
            .push(format!("{new} is already on this computer. Nothing is downloaded."));
    }
    plan.effects.push(format!(
        "At the next restart the computer starts {new}. It gets {TRIAL_ATTEMPTS} attempts to \
         start properly; if it does not, TUR returns to {current} by itself."
    ));
    plan.effects.push(format!(
        "{current} stays on this computer as the previous version. Files, applications and \
         settings carry over to {new}."
    ));
    if let Some(previous) = &state.rollback {
        if previous.pinned {
            plan.effects.push(format!(
                "{}, the previous version now, stays: TUR was asked to keep it.",
                previous.label()
            ));
        } else {
            plan.effects.push(format!(
                "{}, the previous version now, is removed when {new} is applied.",
                previous.label()
            ));
        }
    }
    if let Some(staged) = &state.staged
        && held.is_none()
    {
        plan.effects.push(format!(
            "The update that was waiting, {}, is replaced.",
            staged.label()
        ));
    }
    plan.restart_required = true;
    plan
}

/// The plan for going back to the previous version.
pub fn plan_rollback(state: &SystemState, conditions: &Conditions) -> Plan {
    let mut plan = Plan::new(Operation::Rollback, decide_rollback(state, conditions));
    if !plan.decision.proceeds() {
        return plan;
    }
    let Some(previous) = &state.rollback else {
        // Not reachable: the decision proceeds only with a previous version.
        return plan;
    };
    let previous = previous.label();
    let current = state.booted.label();
    plan.steps.push(step(
        &ROLLBACK,
        None,
        format!(
            "Makes {previous} the first in the start-up order; {current} becomes the previous \
             version. Nothing is downloaded and the running system is not changed."
        ),
    ));
    plan.effects.push(format!(
        "At the next restart the computer starts {previous}. Until then nothing changes."
    ));
    plan.effects.push(format!(
        "{current} stays on this computer as the previous version, and you can return to it."
    ));
    plan.effects.push(
        "Files in home folders and installed applications are not changed.".to_owned(),
    );
    plan.effects.push(format!(
        "System settings changed since {previous} was last in use go back to what they were \
         then: each version has its own /etc, and going back does not merge it."
    ));
    if let Some(staged) = &state.staged {
        plan.effects.push(format!(
            "The update waiting to be used, {}, is discarded.",
            staged.label()
        ));
    }
    plan.effects.push(format!(
        "{previous} is started without counted attempts: it has run on this computer before."
    ));
    plan.restart_required = true;
    plan
}

fn operand_for(slot: Slot) -> Option<&'static str> {
    match slot {
        Slot::Booted => Some("booted"),
        Slot::Rollback => Some("rollback"),
        Slot::Staged | Slot::Other => None,
    }
}

/// The plan for keeping a version.
pub fn plan_pin(state: &SystemState, slot: Slot) -> Plan {
    let mut plan = Plan::new(Operation::Pin, decide_pin(state, slot));
    let (true, Some(deployment), Some(operand)) = (
        plan.decision.proceeds(),
        state.deployment(slot),
        operand_for(slot),
    ) else {
        return plan;
    };
    let version = deployment.label();
    plan.steps.push(step(
        &PIN,
        Some(operand),
        format!("Pins {version}, the {slot} deployment."),
    ));
    plan.effects.push(format!(
        "{version} is never removed when updates are installed, until it is released."
    ));
    plan.effects
        .push("It uses disk space for as long as it is kept.".to_owned());
    plan.effects
        .push("Updates continue as before.".to_owned());
    plan
}

/// The plan for releasing a kept version.
pub fn plan_unpin(state: &SystemState, slot: Slot) -> Plan {
    let mut plan = Plan::new(Operation::Unpin, decide_unpin(state, slot));
    let (true, Some(deployment), Some(operand)) = (
        plan.decision.proceeds(),
        state.deployment(slot),
        operand_for(slot),
    ) else {
        return plan;
    };
    let version = deployment.label();
    plan.steps.push(step(
        &UNPIN,
        Some(operand),
        format!("Unpins {version}, the {slot} deployment."),
    ));
    plan.effects
        .push(format!("{version} is treated like any other version again."));
    plan.effects.push(match slot {
        Slot::Rollback => "It is removed when the next update is applied.".to_owned(),
        _ => "It becomes the previous version at the next update, and is removed by the one \
              after that."
            .to_owned(),
    });
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::STATUS_ARGUMENTS;

    #[test]
    fn the_command_table_is_the_whole_vocabulary() {
        let ids: Vec<&str> = COMMANDS.iter().map(|command| command.id).collect();
        assert_eq!(
            ids,
            [
                "read-status",
                "check",
                "stage-held",
                "release",
                "rollback",
                "pin",
                "unpin"
            ]
        );
        for command in COMMANDS {
            // A full path, never looked up.
            assert!(
                command.program == "/usr/bin/bootc" || command.program == "/usr/bin/ostree",
                "{}",
                command.id
            );
            assert!(!command.subcommand.is_empty(), "{}", command.id);
            for option in command.options {
                assert!(option.starts_with("--"), "{}: {option}", command.id);
            }
            assert!(command.purpose.ends_with('.'), "{}", command.id);
        }
        // Exactly one command is ever run by this program, and it reads.
        let run: Vec<&ExternalCommand> = COMMANDS
            .iter()
            .filter(|command| command.run_by_this_program)
            .collect();
        assert_eq!(run.len(), 1);
        assert_eq!(run[0].changes, Changes::Nothing);
        assert_eq!(run[0].argv(None), ["/usr/bin/bootc", "status", "--format=json"]);
        assert_eq!(run[0].program, STATUS_PROGRAM);
        assert_eq!(
            [run[0].subcommand, run[0].options].concat(),
            STATUS_ARGUMENTS
        );
        // Nothing else reads only.
        for command in COMMANDS.iter().filter(|command| !command.run_by_this_program) {
            assert_ne!(command.changes, Changes::Nothing, "{}", command.id);
        }
        // No command restarts the machine: `--apply` is never named.
        for command in COMMANDS {
            assert!(!command.options.contains(&"--apply"), "{}", command.id);
            assert!(!command.options.contains(&"--soft-reboot"), "{}", command.id);
        }
    }

    #[test]
    fn argument_vectors_are_spelled_out() {
        assert_eq!(CHECK.argv(None), ["/usr/bin/bootc", "upgrade", "--check"]);
        assert_eq!(
            STAGE_HELD.argv(None),
            ["/usr/bin/bootc", "upgrade", "--download-only", "--quiet"]
        );
        assert_eq!(
            RELEASE.argv(None),
            ["/usr/bin/bootc", "upgrade", "--from-downloaded"]
        );
        assert_eq!(ROLLBACK.argv(None), ["/usr/bin/bootc", "rollback"]);
        assert_eq!(
            PIN.argv(Some("booted")),
            ["/usr/bin/ostree", "admin", "pin", "booted"]
        );
        assert_eq!(
            UNPIN.argv(Some("rollback")),
            ["/usr/bin/ostree", "admin", "pin", "--unpin", "rollback"]
        );
        assert_eq!(operand_for(Slot::Staged), None);
        assert_eq!(operand_for(Slot::Other), None);
        assert_eq!(TRIAL_ATTEMPTS, 3);
    }
}

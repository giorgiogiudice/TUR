// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `TUR-AI-*` error codes of the orchestrator, and the [`Fault`] that
//! carries one.
//!
//! The policy engine's codes are in [`crate::codes`]. These are the codes of
//! everything around it: the tool registry, the adapters, the read-only
//! sandbox and the task loop. They are declared the same way, in one table,
//! with the fields specification section 29.3 requires. The two tables are
//! kept apart so that the engine's registry, and the tests that pin it, are
//! not touched by this layer.

use std::fmt;

use tur_core::ErrorCode;

use crate::codes::{Problem, Severity};

/// The registry entry of one orchestrator code (specification section 29.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRegistryEntry {
    /// The code this entry describes.
    pub code: TaskCode,
    /// The code as text, for example `TUR-AI-TASK-001`.
    pub text: &'static str,
    /// One sentence stating what the code means.
    pub title: &'static str,
    /// How serious the condition is.
    pub severity: Severity,
    /// What this component observed in order to report the code.
    pub detection: &'static str,
    /// What the user is told, in plain terms.
    pub user_message: &'static str,
    /// What a developer needs to know.
    pub explanation: &'static str,
    /// What can be done next.
    pub recovery: &'static str,
}

macro_rules! task_code_registry {
    ($(
        $variant:ident => $code:literal, $severity:ident,
            title: $title:literal,
            detection: $detection:literal,
            user_message: $user_message:literal,
            explanation: $explanation:literal,
            recovery: $recovery:literal;
    )+) => {
        /// Every `TUR-AI-*` error code the orchestrator can report.
        ///
        /// The documentation of each variant is its registry title. The
        /// discriminants index [`TASK_REGISTRY`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum TaskCode {
            $(
                #[doc = $title]
                $variant,
            )+
        }

        /// The registry: one entry for every code, in registry order.
        pub const TASK_REGISTRY: &[TaskRegistryEntry] = &[
            $(
                TaskRegistryEntry {
                    code: TaskCode::$variant,
                    text: $code,
                    title: $title,
                    severity: Severity::$severity,
                    detection: $detection,
                    user_message: $user_message,
                    explanation: $explanation,
                    recovery: $recovery,
                },
            )+
        ];

        impl TaskCode {
            /// The code as text, for example `TUR-AI-TASK-001`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( TaskCode::$variant => $code, )+
                }
            }

            /// The code as a validated [`ErrorCode`]. Each code is checked
            /// when this crate is compiled.
            pub fn error_code(self) -> ErrorCode {
                match self {
                    $( TaskCode::$variant => const { ErrorCode::from_static($code) }, )+
                }
            }
        }
    };
}

task_code_registry! {
    RegistryDuplicateService => "TUR-AI-REGISTRY-001", Error,
        title: "Two tool manifests declare the same service, or one declares a reserved service.",
        detection: "Loading the tool set found a service name a second time, or found the name the assistant keeps for its own built-in tools.",
        user_message: "Two components describe themselves to the assistant under the same name, so the assistant cannot use its tools.",
        explanation: "With two manifests for one service it would not be defined which class applies to a tool. The whole tool set is rejected rather than the second manifest being ignored.",
        recovery: "Remove the duplicate manifest from the manifest directory.";
    RegistryDuplicateTool => "TUR-AI-REGISTRY-002", Error,
        title: "Two services declare a tool with the same name.",
        detection: "Loading the tool set found one tool name in two manifests, or a tool with the name of a built-in tool.",
        user_message: "Two components offer the assistant an operation under the same name, so the assistant cannot use its tools.",
        explanation: "A model names a tool by its bare name. Two tools with one name could not be told apart, and one could stand for the other.",
        recovery: "Rename one of the tools in the manifest of the service that owns it.";
    AdapterParse => "TUR-AI-ADAPTER-001", Error,
        title: "The tool adapter file is not valid JSON of the expected shape.",
        detection: "Parsing an adapter file failed: the text is not JSON, a required field is missing, a field has the wrong type, a field is unknown or repeated, or a value is outside its permitted range.",
        user_message: "A description of how the assistant performs an operation could not be read, so the assistant cannot use its tools.",
        explanation: "The whole tool set is rejected. The detail names the position in the file.",
        recovery: "Correct the file against the adapter format and load the tool set again.";
    AdapterVersion => "TUR-AI-ADAPTER-002", Error,
        title: "The tool adapter file has an unsupported format version.",
        detection: "The file states a format version this build does not read.",
        user_message: "A description of how the assistant performs an operation is in a format this version of the system does not understand.",
        explanation: "No attempt is made to read a file of another version: a field could mean something else.",
        recovery: "Update the system or the adapter file so that both use the same version.";
    AdapterUnknownTool => "TUR-AI-ADAPTER-003", Error,
        title: "An adapter names a service or a tool that no manifest declares, or names a tool twice.",
        detection: "An adapter file names a service with no loaded manifest, a tool its manifest does not declare, or a tool that already has an adapter.",
        user_message: "A description of how the assistant performs an operation does not match the operations that exist, so the assistant cannot use its tools.",
        explanation: "An adapter never adds a tool. The class and the input rules of a tool come from the reviewed manifest only.",
        recovery: "Make the adapter name a tool of the manifest, exactly, and once.";
    AdapterArgument => "TUR-AI-ADAPTER-004", Error,
        title: "An adapter argument does not fit the tool it is for.",
        detection: "An argument refers to an input the tool does not declare, to an input of a kind that cannot be placed on a command line, or is not one of the permitted forms.",
        user_message: "A description of how the assistant performs an operation is not consistent, so the assistant cannot use its tools.",
        explanation: "Every value taken from a tool input is checked against the manifest when the adapter is loaded, so that nothing unchecked can reach a command line later.",
        recovery: "Refer only to inputs the tool declares, with a text, whole number or true-or-false kind as the form requires.";
    AdapterProgram => "TUR-AI-ADAPTER-005", Error,
        title: "An adapter names a program or a path that is not a plain absolute path.",
        detection: "A program or a path in an adapter file is relative, has a dot or dot-dot component, or holds a control character.",
        user_message: "A description of how the assistant performs an operation names a program in a way that is not valid, so the assistant cannot use its tools.",
        explanation: "A program is never looked up along a search path, so that a program planted under a familiar name elsewhere cannot be run instead.",
        recovery: "Name the program by its full path.";
    SandboxUnavailable => "TUR-AI-SANDBOX-001", Error,
        title: "The sandbox could not be started, so nothing was run.",
        detection: "The sandbox program is not installed, or it reported that it could not set up the sandbox.",
        user_message: "The assistant could not start the protected environment it runs things in, so it ran nothing.",
        explanation: "There is no way to run a tool program or a command outside the sandbox. A failure to build it is a failure to run.",
        recovery: "Check that bubblewrap is installed and that user namespaces are permitted for it.";
    SandboxGrantRefused => "TUR-AI-SANDBOX-002", Error,
        title: "A folder cannot be shown to the commands the assistant runs.",
        detection: "A path given for a task is not a plain absolute path, does not exist, is reached through a link, is a protected or hidden location, belongs to the system or to another person, or contains the home folder.",
        user_message: "That folder cannot be shared with the assistant, because it holds or contains things that are kept from it.",
        explanation: "Keys, keyrings, password stores, browser profiles, the runtime directory and other accounts are never visible in the sandbox, whatever is granted.",
        recovery: "Grant a more specific folder that holds only what the task needs.";
    SandboxCommandInvalid => "TUR-AI-SANDBOX-003", Warning,
        title: "The command is not one that can be run.",
        detection: "A command has no program, too many or too long arguments, or an argument with a character that cannot be passed to a program.",
        user_message: "The assistant proposed a command that is not valid. Nothing was run.",
        explanation: "The command is refused before a sandbox is built for it.",
        recovery: "None is needed. The assistant may propose a valid command.";
    SandboxTimedOut => "TUR-AI-SANDBOX-004", Notice,
        title: "The command ran longer than its time limit and was stopped.",
        detection: "The sandboxed process had not ended when its wall-clock limit passed, and was killed with everything it had started.",
        user_message: "Something the assistant ran took too long and was stopped.",
        explanation: "Output produced before the limit is kept, within the size bound.",
        recovery: "None is needed. The assistant may try a narrower command.";
    TaskStepLimit => "TUR-AI-TASK-001", Notice,
        title: "The task reached its limit on the number of steps and was stopped.",
        detection: "The model was about to be asked for one more response than the task allows.",
        user_message: "The assistant stopped because the task was taking more steps than it is allowed.",
        explanation: "A step is one response of the model. The limit bounds a model that never finishes.",
        recovery: "Ask again with a narrower request.";
    TaskTimeLimit => "TUR-AI-TASK-002", Notice,
        title: "The task reached its time limit and was stopped.",
        detection: "The wall-clock time allowed for the task had passed when the task was about to take its next step. Time spent paused counts.",
        user_message: "The assistant stopped because the task was taking longer than it is allowed.",
        explanation: "The limit is checked between steps; a single response and a single command have their own time limits.",
        recovery: "Ask again with a narrower request.";
    TaskCancelled => "TUR-AI-TASK-003", Notice,
        title: "The task was cancelled.",
        detection: "A cancellation was seen between two steps.",
        user_message: "The assistant stopped because the task was cancelled.",
        explanation: "Nothing further is proposed, decided or run after a cancellation is seen.",
        recovery: "None is needed.";
    TaskApprovalUnavailable => "TUR-AI-TASK-004", Notice,
        title: "The action needs the user's approval, and no way to give one exists yet.",
        detection: "The policy engine decided that the action must be confirmed, authenticated or held for the user's return.",
        user_message: "P@x would need your approval for this, which is not available yet.",
        explanation: "The trusted confirmation path is not built. An approval typed into the conversation is not an approval, so the action does not run and the model is told why.",
        recovery: "Do the step yourself.";
    TaskClassUnavailable => "TUR-AI-TASK-005", Warning,
        title: "The action is above the classes the assistant may perform in this release.",
        detection: "The policy engine allowed an action it treated as consequential or system-critical.",
        user_message: "The assistant is not able to do that in this version. Nothing was done.",
        explanation: "A second check behind the policy engine. Without the trusted confirmation path the engine refuses these actions itself, so this code should never be seen.",
        recovery: "Report it: the policy engine and the orchestrator disagree.";
    TaskTooManyCalls => "TUR-AI-TASK-006", Warning,
        title: "One response proposed more actions than a response may.",
        detection: "A response of the model held more tool calls than the task allows in one response.",
        user_message: "The assistant proposed too many actions at once. The extra ones were not done.",
        explanation: "The calls within the limit are decided one by one. The others are refused without being decided.",
        recovery: "None is needed.";
    TaskResponseDiscarded => "TUR-AI-TASK-007", Notice,
        title: "A response of the model was incomplete and was discarded whole.",
        detection: "A response ended in a refusal, in an error or at its output limit.",
        user_message: "The assistant's answer was cut off or withdrawn, so nothing in it was acted on.",
        explanation: "Actions proposed before the response broke off are not decided and not run.",
        recovery: "Ask again.";
    TaskToolFailed => "TUR-AI-TASK-008", Notice,
        title: "A tool program or a command ended with a failure.",
        detection: "The sandboxed process ended with an exit status that is not one of the statuses declared as success.",
        user_message: "Something the assistant ran reported a failure.",
        explanation: "The output, within its size bound and after filtering, is still returned to the model, marked as a failure.",
        recovery: "None is needed. The assistant may read the output and try something else.";
    OutputWithheld => "TUR-AI-OUTPUT-001", Notice,
        title: "Part of a result looked like a credential and was withheld from the provider.",
        detection: "The output filter found text with the form of a password, a key, a token, a payment card number or a one-time code in what a tool or a command returned.",
        user_message: "Part of what the assistant read looked like a password or a key, so it was not sent to the AI provider.",
        explanation: "The audit trail records that something was withheld and of what kind, never the text itself.",
        recovery: "None is needed.";
}

impl TaskCode {
    /// The registry entry of the code.
    pub const fn entry(self) -> &'static TaskRegistryEntry {
        &TASK_REGISTRY[self as usize]
    }

    /// What the user is told, in plain terms.
    pub const fn user_message(self) -> &'static str {
        self.entry().user_message
    }
}

impl fmt::Display for TaskCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing that went wrong in the orchestrator, with its stable code.
///
/// A fault carries either a code of this module or a code of the policy
/// engine, so that a caller handles both the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The stable error code.
    pub code: ErrorCode,
    /// What exactly is wrong, for a person reading a log or a terminal. It
    /// never repeats a value the model supplied.
    pub detail: String,
}

impl Fault {
    /// A fault with a code of the orchestrator.
    pub fn new(code: TaskCode, detail: impl Into<String>) -> Self {
        Fault {
            code: code.error_code(),
            detail: detail.into(),
        }
    }
}

impl From<Problem> for Fault {
    fn from(problem: Problem) -> Self {
        Fault {
            code: problem.code.error_code(),
            detail: problem.detail,
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Fault {}

/// Convert the problems of the policy engine's loaders into faults.
pub fn faults(problems: Vec<Problem>) -> Vec<Fault> {
    problems.into_iter().map(Fault::from).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::Family;

    use super::*;
    use crate::codes::REGISTRY;

    #[test]
    fn every_code_is_a_valid_ai_code_with_a_complete_entry() {
        for (index, entry) in TASK_REGISTRY.iter().enumerate() {
            let text = entry.text;
            let parsed: ErrorCode = text.parse().unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(parsed.family(), Family::Ai, "{text}");
            assert_eq!(entry.code as usize, index, "{text}");
            assert_eq!(entry.code.as_str(), text);
            assert_eq!(entry.code.error_code(), parsed);
            assert_eq!(entry.code.to_string(), text);
            assert_eq!(entry.code.entry(), entry);
            for (name, field) in [
                ("title", entry.title),
                ("detection", entry.detection),
                ("user_message", entry.user_message),
                ("explanation", entry.explanation),
                ("recovery", entry.recovery),
            ] {
                assert!(field.len() >= 10, "{text}: {name} is too short");
                assert!(field.ends_with('.'), "{text}: {name} must be a sentence");
                assert!(
                    field.chars().all(|c| matches!(c, ' '..='~')),
                    "{text}: {name} must be printable ASCII"
                );
            }
            for jargon in ["class ", "TUR-", "D-Bus", "schema", "JSON", "_"] {
                assert!(
                    !entry.user_message.contains(jargon),
                    "{text}: the user message contains {jargon:?}"
                );
            }
        }
    }

    #[test]
    fn no_code_collides_with_another_or_with_the_policy_engine() {
        let mut texts: BTreeSet<&str> = REGISTRY.iter().map(|entry| entry.text).collect();
        let engine = texts.len();
        let mut titles = BTreeSet::new();
        for entry in TASK_REGISTRY {
            assert!(texts.insert(entry.text), "duplicate code {}", entry.text);
            assert!(titles.insert(entry.title), "duplicate title {}", entry.text);
        }
        assert_eq!(texts.len(), engine + TASK_REGISTRY.len());
    }

    #[test]
    fn a_fault_shows_its_code_then_its_detail() {
        let fault = Fault::new(TaskCode::TaskStepLimit, "after 3 steps");
        assert_eq!(fault.to_string(), "TUR-AI-TASK-001: after 3 steps");
        let from_engine = Fault::from(Problem::new(crate::codes::Code::ToolUnknown, "no such tool"));
        assert_eq!(from_engine.to_string(), "TUR-AI-TOOL-001: no such tool");
        assert_eq!(
            TaskCode::TaskApprovalUnavailable.user_message(),
            "P@x would need your approval for this, which is not available yet."
        );
    }
}

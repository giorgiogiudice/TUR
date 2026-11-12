// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `TUR-AI-*` error codes of the policy layer.
//!
//! Every code this component reports is declared once, in the registry table in
//! this file, with the fields specification section 29.3 requires: title,
//! severity, detection criteria, user message, technical explanation and
//! recovery action. The meaning of a released code never changes.

use std::fmt;

use tur_core::ErrorCode;

/// How serious a registered condition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    /// The assistant did not do something because of a setting or of the
    /// situation it is in. Nothing is wrong.
    Notice,
    /// The assistant proposed something it must never do, or proposed it in a
    /// form that is not accepted. The proposal did not run. Worth a look if it
    /// recurs: it may be the trace of an attempt to mislead the assistant.
    Warning,
    /// A file or an input is invalid. Nothing was decided.
    Error,
    /// A safeguard the assistant depends on is not working. The assistant must
    /// not continue until it is.
    Critical,
}

impl Severity {
    /// Every severity, in ascending order.
    pub const ALL: [Severity; 4] = [
        Severity::Notice,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
    ];

    /// The severity as lower-case text.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Notice => "notice",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The registry entry of one error code (specification section 29.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The code this entry describes.
    pub code: Code,
    /// The code as text, for example `TUR-AI-POLICY-001`.
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

// Declares the `Code` enum, the `REGISTRY` table and the lookups from one list,
// so a code cannot exist without a complete registry entry.
macro_rules! code_registry {
    ($(
        $variant:ident => $code:literal, $severity:ident,
            title: $title:literal,
            detection: $detection:literal,
            user_message: $user_message:literal,
            explanation: $explanation:literal,
            recovery: $recovery:literal;
    )+) => {
        /// Every `TUR-AI-*` error code this component can report.
        ///
        /// The documentation of each variant is its registry title. The
        /// discriminants index [`REGISTRY`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Code {
            $(
                #[doc = $title]
                $variant,
            )+
        }

        /// The registry: one entry for every code, in registry order.
        pub const REGISTRY: &[RegistryEntry] = &[
            $(
                RegistryEntry {
                    code: Code::$variant,
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

        impl Code {
            /// The code as text, for example `TUR-AI-POLICY-001`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Code::$variant => $code, )+
                }
            }

            /// One sentence stating what the code means.
            pub const fn title(self) -> &'static str {
                match self {
                    $( Code::$variant => $title, )+
                }
            }

            /// The code as a validated [`ErrorCode`].
            ///
            /// Each code is checked when this crate is compiled: one that is
            /// not a valid TUR error code stops the build.
            pub fn error_code(self) -> ErrorCode {
                match self {
                    $( Code::$variant => const { ErrorCode::from_static($code) }, )+
                }
            }
        }
    };
}

code_registry! {
    ManifestParse => "TUR-AI-MANIFEST-001", Error,
        title: "The tool manifest is not valid JSON of the expected shape.",
        detection: "Parsing the manifest failed: the text is not JSON, a required field is missing, a field has the wrong type, a field is unknown, or a field or a key inside an input schema is repeated.",
        user_message: "A component's description of what the assistant may do with it could not be read, so the assistant cannot use that component.",
        explanation: "The manifest is rejected as a whole and none of its tools is offered. The detail names the position in the file.",
        recovery: "Correct the manifest against schemas/agent-tool-manifest.schema.json and validate it with `tur-pax manifest validate`.";
    ManifestVersion => "TUR-AI-MANIFEST-002", Error,
        title: "The tool manifest has an unsupported schema version.",
        detection: "The manifest's schema_version is not the version this build reads.",
        user_message: "A component describes itself in a format this version of the system does not understand, so the assistant cannot use that component.",
        explanation: "No attempt is made to read a manifest of another version: a field could mean something else.",
        recovery: "Update the system or the component so that both use the same manifest version.";
    ManifestService => "TUR-AI-MANIFEST-003", Error,
        title: "The tool manifest names an invalid service.",
        detection: "The service field is not a D-Bus well-known name of at least two elements of ASCII letters, digits and underscores.",
        user_message: "A component's description names the component in a way that is not valid, so the assistant cannot use it.",
        explanation: "The service name is written to the audit log and used to address the service, so it is restricted to a safe character set.",
        recovery: "Use the service's D-Bus well-known name, for example org.tur.Storage1.";
    ManifestNoTools => "TUR-AI-MANIFEST-004", Error,
        title: "The tool manifest declares no tools.",
        detection: "The tools list is empty.",
        user_message: "A component's description offers the assistant nothing to do.",
        explanation: "An empty manifest is more likely a truncated file than an intention, so it is an error.",
        recovery: "Declare at least one tool, or do not ship a manifest.";
    ManifestToolName => "TUR-AI-MANIFEST-005", Error,
        title: "A tool name is outside the permitted character set.",
        detection: "A tool name does not match ^[a-z][a-z0-9_]{0,63}$.",
        user_message: "A component names one of its operations in a way that is not valid, so the assistant cannot use the component.",
        explanation: "Tool names are given to the model and written to the audit log. The restriction rules out look-alike characters, case tricks and separators.",
        recovery: "Rename the tool using lower-case ASCII letters, digits and underscores, starting with a letter.";
    ManifestDuplicateTool => "TUR-AI-MANIFEST-006", Error,
        title: "A tool name is declared more than once.",
        detection: "Two tools in one manifest have the same name.",
        user_message: "A component describes the same operation twice, so the assistant cannot use the component.",
        explanation: "With two declarations it would not be defined which class and which input schema apply.",
        recovery: "Give every tool of the manifest its own name.";
    ManifestToolField => "TUR-AI-MANIFEST-007", Error,
        title: "A tool description or method is invalid.",
        detection: "A description is blank, longer than 1024 characters or contains a control character; or a method is not a D-Bus member name.",
        user_message: "A component describes one of its operations in a way that is not valid, so the assistant cannot use the component.",
        explanation: "The description is given to the model and may be shown to the user; the method is used to call the service.",
        recovery: "Write a description of 1 to 1024 characters without control characters, and name the D-Bus method exactly.";
    ManifestClassRange => "TUR-AI-MANIFEST-008", Error,
        title: "A tool class is outside the range 0 to 3.",
        detection: "A tool's class is an integer below 0 or above 4.",
        user_message: "A component does not say how much care one of its operations needs, so the assistant cannot use the component.",
        explanation: "The class decides whether the tool runs, is asked about or needs authentication. There is no default.",
        recovery: "Declare the class of the tool: 0 observe, 1 reversible, 2 consequential, 3 system-critical.";
    ManifestClassProhibited => "TUR-AI-MANIFEST-009", Error,
        title: "A tool declares class 4; a prohibited operation is not offered as a tool.",
        detection: "A tool's class is 4.",
        user_message: "A component offers the assistant an operation the assistant must never perform, so the assistant cannot use the component.",
        explanation: "Class 4 operations are refused in every circumstance. Offering one as a tool would only invite the model to ask for it.",
        recovery: "Remove the tool from the manifest.";
    ManifestInputSchema => "TUR-AI-MANIFEST-010", Error,
        title: "A tool input schema is not a closed object schema in the supported subset.",
        detection: "An input schema is not an object schema, leaves an object open to undeclared properties at some depth, uses a keyword outside the supported subset, or uses a keyword with a value of the wrong kind.",
        user_message: "A component describes the inputs of one of its operations in a way the assistant cannot check, so the assistant cannot use the component.",
        explanation: "The agent checks the arguments of every call against the schema and enforces exactly the supported subset. A constraint it would ignore is rejected rather than accepted and not enforced.",
        recovery: "Restrict the schema to the subset listed in docs/architecture/agent-policy.md and set additionalProperties to false in every object schema.";
    ManifestMethodClass => "TUR-AI-MANIFEST-011", Error,
        title: "A method is declared with two different classes.",
        detection: "Two tools of one manifest call the same method and declare different classes.",
        user_message: "A component says two different things about how much care one of its operations needs, so the assistant cannot use the component.",
        explanation: "One method reached under two classes would let the lower class stand for the higher: a destructive operation could be offered a second time as a harmless tool.",
        recovery: "Declare one class for the method, or give the less dangerous variant a method of its own in the service.";
    ToolUnknown => "TUR-AI-TOOL-001", Warning,
        title: "The tool is not declared in the manifest.",
        detection: "A call names a tool the service's manifest does not declare. Names are compared exactly.",
        user_message: "The assistant tried to use an operation that does not exist. Nothing was done.",
        explanation: "The model can only call declared tools. A name that differs in case or by a look-alike character is a different name.",
        recovery: "None is needed. If it recurs, check that the manifest given to the model is the one the service ships.";
    ToolArguments => "TUR-AI-TOOL-002", Warning,
        title: "The tool arguments do not match the declared input schema.",
        detection: "The proposed arguments have an undeclared property, lack a required one, or have a value of the wrong type or outside the declared bounds; or the arguments repeat a key.",
        user_message: "The assistant tried to use an operation with inputs that are not valid. Nothing was done.",
        explanation: "The call is not built, so it is never decided or run. The detail names the position and the expectation and never repeats a value.",
        recovery: "None is needed. The assistant may propose the call again with valid arguments.";
    PolicyFileParse => "TUR-AI-POLICYFILE-001", Error,
        title: "The user policy file is not valid JSON of the expected shape.",
        detection: "Parsing the policy file failed: the text is not JSON, a required field is missing, a field has the wrong type, a field is unknown or a field is repeated.",
        user_message: "Your settings for the assistant could not be read. Until they can, the assistant only observes and changes nothing.",
        explanation: "The caller must fall back to UserPolicy::fail_closed(), which is observe-only. A damaged file is never read as the default policy.",
        recovery: "Open the assistant's page in system settings and save the settings again.";
    PolicyFileVersion => "TUR-AI-POLICYFILE-002", Error,
        title: "The user policy file has an unsupported schema version.",
        detection: "The policy file's schema_version is not the version this build reads.",
        user_message: "Your settings for the assistant are in a format this version of the system does not understand. Until that is resolved, the assistant only observes and changes nothing.",
        explanation: "The caller must fall back to UserPolicy::fail_closed().",
        recovery: "Update the system, or save the settings again from system settings.";
    PolicyFileApplication => "TUR-AI-POLICYFILE-003", Error,
        title: "The user policy file names an invalid application.",
        detection: "An application identifier in the policy file is empty, longer than 255 characters, has a space at either end, or contains a character that is not printable ASCII.",
        user_message: "Your settings for the assistant name an application in a way that is not valid. Until that is corrected, the assistant only observes and changes nothing.",
        explanation: "An identifier with a look-alike or invisible character would never equal the identifier the compositor reports, so the exclusion or sensitivity it stands for would silently not apply.",
        recovery: "Remove the entry in system settings and choose the application from the list again.";
    PolicyFileRelaxation => "TUR-AI-POLICYFILE-004", Error,
        title: "A relaxation names a class other than 2, or repeats an application.",
        detection: "A relaxation's class is an integer other than 2, or two relaxations name the same application, ignoring ASCII case and a trailing .desktop.",
        user_message: "Your settings for the assistant relax a rule in a way that is not possible. Until that is corrected, the assistant only observes and changes nothing.",
        explanation: "Only the class 2 handling of accessibility and screen actions in a named application can be relaxed. Classes 3 and 4 cannot, and class 1 needs no relaxation.",
        recovery: "Remove the relaxation in system settings.";
    PolicyProhibited => "TUR-AI-POLICY-001", Warning,
        title: "The action is prohibited for the assistant (class 4).",
        detection: "The proposed action is one of the agent operations of class 4: reading a keyring secret, changing policy, permissions or standing approvals, disabling the indicator or the audit log, operating the trusted path, or enabling APFS write.",
        user_message: "The assistant is never allowed to do that. Nothing was done.",
        explanation: "No context, policy or approval changes this decision.",
        recovery: "If you want the change, make it yourself in system settings.";
    PolicyMalformedAction => "TUR-AI-POLICY-002", Warning,
        title: "The proposed action is incomplete and cannot be classified.",
        detection: "A tool call without a service or a tool name; an accessibility or screen action without an application; a command without a program; or a typed operation whose target does not have the shape of its type or whose content is missing or surplus.",
        user_message: "The assistant proposed something that is not a complete action. Nothing was done.",
        explanation: "An action that cannot be classified is not asked about either: there would be nothing definite to show.",
        recovery: "None is needed. The assistant may propose a complete action.";
    PolicyExcludedApplication => "TUR-AI-POLICY-003", Notice,
        title: "The target application is excluded.",
        detection: "An accessibility or screen action targets an application on the user's list of excluded applications or on the built-in list of password managers, keyrings and password prompts.",
        user_message: "The assistant does not look at or act in that application.",
        explanation: "Exclusion covers observation too. The built-in list applies whatever the user policy says.",
        recovery: "Do that step yourself. An application you excluded can be taken off your list in system settings.";
    PolicySessionLocked => "TUR-AI-POLICY-004", Notice,
        title: "Accessibility and screen actions are not available while the session is locked.",
        detection: "An accessibility or screen action was proposed while the session is locked.",
        user_message: "The assistant does not look at or use the screen while the session is locked.",
        explanation: "An attended task is paused by the lock. An unattended task goes on with tools and read-only commands only.",
        recovery: "Unlock the session.";
    PolicyObserveOnly => "TUR-AI-POLICY-005", Notice,
        title: "The user policy is observe-only; only class 0 actions are available.",
        detection: "The policy mode is observe_only and the action is above class 0. The fail-closed policy used for an unreadable policy file is observe-only as well.",
        user_message: "The assistant is set to observe only, so it changes nothing.",
        explanation: "If the mode was not chosen by the user, look for a TUR-AI-POLICYFILE code reported earlier.",
        recovery: "Choose another mode in system settings, or do the step yourself.";
    PolicyTrustedPathUnavailable => "TUR-AI-POLICY-006", Notice,
        title: "The trusted confirmation path is not available; the assistant is limited to classes 0 and 1.",
        detection: "The context reports that the five compositor guarantees of the trusted confirmation path do not all hold, and the action is class 2 or 3.",
        user_message: "The assistant cannot ask for your approval in a way that is safe on this system, so it does only things that can be undone.",
        explanation: "Without the trusted path a confirmation could be given by the assistant itself or imitated by an application, so nothing that needs one runs, and no authentication is requested.",
        recovery: "Do the step yourself. The limit is lifted when the compositor provides the trusted path.";
    PolicySystemCriticalUnattended => "TUR-AI-POLICY-007", Notice,
        title: "A system-critical action needs a person present to authenticate.",
        detection: "A class 3 action was proposed in an unattended task or while the session is locked.",
        user_message: "That needs you to authenticate in person, and nobody is there to do it. Nothing was done.",
        explanation: "Class 3 is refused, not deferred, when nobody is present: it is never queued for later.",
        recovery: "Ask the assistant again while you are at the machine.";
    PolicySystemCriticalAsCommand => "TUR-AI-POLICY-008", Warning,
        title: "A system-critical operation is not available as a privileged request; it exists only as a tool of the owning service.",
        detection: "A command in the privileged sandbox declares a system-critical or unclassified operation type, or a typed operation names a protected target.",
        user_message: "The assistant tried to change a part of the system that is never changed this way. Nothing was done.",
        explanation: "Partitioning, formatting, encryption, the bootloader, firmware, OS update and rollback, device hand-off and security policy are class 3. A repair session never covers them, and no authentication is offered for the request: the operation is reached only through the class 3 tool of the service that owns it.",
        recovery: "If the change is wanted, use the settings page or the tool of the service that owns it; it will ask you to authenticate.";
    PolicyPrivilegedUnattended => "TUR-AI-POLICY-009", Notice,
        title: "A privileged request needs a person present and an open repair session.",
        detection: "A command in the privileged sandbox or a typed operation was proposed in an unattended task or while the session is locked.",
        user_message: "That needs administrator rights, which the assistant has only while you are present and have opened a repair session. Nothing was done.",
        explanation: "An unattended task has no repair session and cannot open one. A lock closes the repair session.",
        recovery: "Ask the assistant again while you are at the machine.";
    PolicyPrivilegedWindow => "TUR-AI-POLICY-010", Warning,
        title: "The target window belongs to a process with administrator rights.",
        detection: "An accessibility or screen action was proposed while the context reports that the target window belongs to a process running with administrator rights.",
        user_message: "The assistant does not look at or act in a window that runs with administrator rights. Nothing was done.",
        explanation: "A click in such a window can have a class 3 effect with no prompt, so the window is a protected surface. Observation is refused as well.",
        recovery: "Do that step yourself, or ask the assistant to make the change through a repair session.";
    AuditUnavailable => "TUR-AI-AUDIT-001", Critical,
        title: "The audit record could not be written, so the action was not released.",
        detection: "The audit sink returned an error for the record of a decision.",
        user_message: "The assistant could not record what it was about to do, so it did nothing.",
        explanation: "An action that leaves no record does not run, whatever the engine decided. The decision returned to the caller is this refusal.",
        recovery: "Check that the journal is running and has space, then try again. If it persists, export a diagnostic bundle.";
    InputUnreadable => "TUR-AI-INPUT-001", Error,
        title: "An input file could not be read.",
        detection: "Opening or reading a file named on the command line failed.",
        user_message: "A file the command needs could not be read.",
        explanation: "Nothing was decided. The exit status is 1, never the status of a decision.",
        recovery: "Check the path and the permissions of the file.";
    InputInvalid => "TUR-AI-INPUT-002", Error,
        title: "A command-line value is not valid.",
        detection: "A value given on the command line is not valid JSON of the expected shape, or repeats a key.",
        user_message: "A value given to the command is not valid.",
        explanation: "Nothing was decided. The exit status is 1, never the status of a decision.",
        recovery: "Correct the value and run the command again.";
}

impl Code {
    /// The registry entry of the code.
    pub const fn entry(self) -> &'static RegistryEntry {
        &REGISTRY[self as usize]
    }

    /// How serious the condition is.
    pub const fn severity(self) -> Severity {
        self.entry().severity
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One problem found while loading or checking input, with its stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The stable error code.
    pub code: Code,
    /// What exactly is wrong, for a person reading a log or a terminal.
    pub detail: String,
}

impl Problem {
    /// A problem with the given code and detail.
    pub fn new(code: Code, detail: impl Into<String>) -> Self {
        Problem {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Problem {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_ai_code() {
        assert!(!REGISTRY.is_empty());
        for (index, entry) in REGISTRY.iter().enumerate() {
            let text = entry.text;
            let parsed: ErrorCode = text.parse().unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(parsed.family(), Family::Ai, "{text}");
            // The discriminant of the code is its position in the registry.
            assert_eq!(entry.code as usize, index, "{text}");
            assert_eq!(entry.code.entry(), entry, "{text}");
            assert_eq!(entry.code.as_str(), text);
            assert_eq!(entry.code.title(), entry.title);
            assert_eq!(entry.code.severity(), entry.severity);
            assert_eq!(entry.code.error_code(), parsed);
            assert_eq!(entry.code.to_string(), text);
        }
    }

    #[test]
    fn codes_and_titles_are_unique() {
        let mut texts = BTreeSet::new();
        let mut titles = BTreeSet::new();
        let mut variants = BTreeSet::new();
        for entry in REGISTRY {
            assert!(texts.insert(entry.text), "duplicate code {}", entry.text);
            assert!(
                titles.insert(entry.title),
                "duplicate title for {}",
                entry.text
            );
            assert!(
                variants.insert(entry.code),
                "duplicate variant for {}",
                entry.text
            );
        }
    }

    #[test]
    fn every_entry_has_every_field_of_the_registry() {
        for entry in REGISTRY {
            let fields = [
                ("title", entry.title),
                ("detection", entry.detection),
                ("user_message", entry.user_message),
                ("explanation", entry.explanation),
                ("recovery", entry.recovery),
            ];
            for (name, text) in fields {
                assert!(
                    text.len() >= 10,
                    "{}: {name} is missing or too short",
                    entry.text
                );
                assert!(
                    text.ends_with('.'),
                    "{}: {name} must be a sentence",
                    entry.text
                );
                assert_eq!(text.trim(), text, "{}: {name}", entry.text);
                assert!(
                    text.chars().all(|c| matches!(c, ' '..='~')),
                    "{}: {name} must be printable ASCII",
                    entry.text
                );
            }
            // The user message is for a person who has not read the design:
            // no class numbers, no code names, no identifiers.
            for jargon in ["class ", "TUR-", "polkit", "D-Bus", "schema", "JSON", "_"] {
                assert!(
                    !entry.user_message.contains(jargon),
                    "{}: the user message contains {jargon:?}",
                    entry.text
                );
            }
        }
    }

    #[test]
    fn severities_follow_the_kind_of_condition() {
        for entry in REGISTRY {
            let text = entry.text;
            let expected: &[Severity] = if text.starts_with("TUR-AI-AUDIT-") {
                &[Severity::Critical]
            } else if text.starts_with("TUR-AI-MANIFEST-")
                || text.starts_with("TUR-AI-POLICYFILE-")
                || text.starts_with("TUR-AI-INPUT-")
            {
                &[Severity::Error]
            } else if text.starts_with("TUR-AI-TOOL-") {
                &[Severity::Warning]
            } else {
                // A refusal of the engine is never an error of the system.
                assert!(text.starts_with("TUR-AI-POLICY-"), "{text}");
                &[Severity::Notice, Severity::Warning]
            };
            assert!(expected.contains(&entry.severity), "{text}");
        }
        let names: BTreeSet<&str> = Severity::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(names.len(), Severity::ALL.len());
        for pair in Severity::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        assert_eq!(Severity::Critical.to_string(), "critical");
    }

    #[test]
    fn problem_displays_code_then_detail() {
        let problem = Problem::new(Code::ToolUnknown, "no tool named x");
        assert_eq!(problem.to_string(), "TUR-AI-TOOL-001: no tool named x");
    }
}

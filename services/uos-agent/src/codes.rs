// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The `UOS-AI-*` error codes of the policy layer.
//!
//! Every code this component reports is declared once, in the registry table in
//! this file. The meaning of a released code never changes (specification
//! section 29.3).

use std::fmt;

use uos_core::ErrorCode;

// Declares the `Code` enum, the `REGISTRY` table and the lookups from one list,
// so a code cannot exist without a registry entry.
macro_rules! code_registry {
    ($( $variant:ident => $code:literal, $title:literal; )+) => {
        /// Every `UOS-AI-*` error code this component can report.
        ///
        /// The documentation of each variant is its registry title.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Code {
            $(
                #[doc = $title]
                $variant,
            )+
        }

        /// The registry: every code with its text and its title, in registry order.
        pub const REGISTRY: &[(Code, &str, &str)] = &[
            $( (Code::$variant, $code, $title), )+
        ];

        impl Code {
            /// The code as text, for example `UOS-AI-POLICY-001`.
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
        }
    };
}

code_registry! {
    ManifestParse => "UOS-AI-MANIFEST-001", "The tool manifest is not valid JSON of the expected shape.";
    ManifestVersion => "UOS-AI-MANIFEST-002", "The tool manifest has an unsupported schema version.";
    ManifestService => "UOS-AI-MANIFEST-003", "The tool manifest names an invalid service.";
    ManifestNoTools => "UOS-AI-MANIFEST-004", "The tool manifest declares no tools.";
    ManifestToolName => "UOS-AI-MANIFEST-005", "A tool name is outside the permitted character set.";
    ManifestDuplicateTool => "UOS-AI-MANIFEST-006", "A tool name is declared more than once.";
    ManifestToolField => "UOS-AI-MANIFEST-007", "A tool description or method is invalid.";
    ManifestClassRange => "UOS-AI-MANIFEST-008", "A tool class is outside the range 0 to 3.";
    ManifestClassProhibited => "UOS-AI-MANIFEST-009", "A tool declares class 4; a prohibited operation is not offered as a tool.";
    ManifestInputSchema => "UOS-AI-MANIFEST-010", "A tool input schema is not a closed object schema in the supported subset.";
    ToolUnknown => "UOS-AI-TOOL-001", "The tool is not declared in the manifest.";
    ToolArguments => "UOS-AI-TOOL-002", "The tool arguments do not match the declared input schema.";
    PolicyFileParse => "UOS-AI-POLICYFILE-001", "The user policy file is not valid JSON of the expected shape.";
    PolicyFileVersion => "UOS-AI-POLICYFILE-002", "The user policy file has an unsupported schema version.";
    PolicyFileApplication => "UOS-AI-POLICYFILE-003", "The user policy file names an invalid application.";
    PolicyFileRelaxation => "UOS-AI-POLICYFILE-004", "A relaxation names a class other than 1 or 2, or repeats an application.";
    PolicyProhibited => "UOS-AI-POLICY-001", "The action is prohibited for the assistant (class 4).";
    PolicyMalformedAction => "UOS-AI-POLICY-002", "The proposed action is incomplete and cannot be classified.";
    PolicyExcludedApplication => "UOS-AI-POLICY-003", "The target application is excluded by the user.";
    PolicySessionLocked => "UOS-AI-POLICY-004", "Accessibility and screen actions are not available while the session is locked.";
    PolicyObserveOnly => "UOS-AI-POLICY-005", "The user policy is observe-only; only class 0 actions are available.";
    PolicyTrustedPathUnavailable => "UOS-AI-POLICY-006", "The trusted confirmation path is not available; the assistant is limited to classes 0 and 1.";
    PolicySystemCriticalUnattended => "UOS-AI-POLICY-007", "A system-critical action needs a person present to authenticate.";
    PolicySystemCriticalAsCommand => "UOS-AI-POLICY-008", "A system-critical operation is not available as a command; it exists only as a tool of the owning service.";
    PolicyPrivilegedUnattended => "UOS-AI-POLICY-009", "A privileged command needs a person present and an open repair session.";
    AuditUnavailable => "UOS-AI-AUDIT-001", "The audit record could not be written, so the action was not released.";
    InputUnreadable => "UOS-AI-INPUT-001", "An input file could not be read.";
    InputInvalid => "UOS-AI-INPUT-002", "A command-line value is not valid.";
}

impl Code {
    /// The code as a validated [`ErrorCode`].
    ///
    /// # Panics
    ///
    /// Never for a registered code: a unit test parses every registry entry.
    pub fn error_code(self) -> ErrorCode {
        self.as_str()
            .parse()
            .expect("every registry entry is a valid UOS error code")
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

    use uos_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_ai_code() {
        assert!(!REGISTRY.is_empty());
        for (code, text, title) in REGISTRY {
            let parsed: ErrorCode = text.parse().unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(parsed.family(), Family::Ai, "{text}");
            assert_eq!(code.as_str(), *text);
            assert_eq!(code.title(), *title);
            assert_eq!(code.error_code(), parsed);
            assert!(title.ends_with('.'), "{text}: title must be a sentence");
        }
    }

    #[test]
    fn codes_and_titles_are_unique() {
        let mut texts = BTreeSet::new();
        let mut titles = BTreeSet::new();
        let mut variants = BTreeSet::new();
        for (code, text, title) in REGISTRY {
            assert!(texts.insert(*text), "duplicate code {text}");
            assert!(titles.insert(*title), "duplicate title for {text}");
            assert!(variants.insert(*code), "duplicate variant for {text}");
        }
    }

    #[test]
    fn problem_displays_code_then_detail() {
        let problem = Problem::new(Code::ToolUnknown, "no tool named x");
        assert_eq!(problem.to_string(), "UOS-AI-TOOL-001: no tool named x");
    }
}

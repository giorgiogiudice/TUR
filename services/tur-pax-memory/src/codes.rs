// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `TUR-AI-MEMORY-*` and `TUR-AI-SCHEDULE-*` error codes.
//!
//! Every code this component reports is declared once, in the registry table in
//! this file. The meaning of a released code never changes (specification
//! section 29.3). The codes keep the `TUR-` prefix: the registry and
//! `tur-core` are renamed with the rest of the project, not here.

use std::fmt;

use tur_core::ErrorCode;

// Declares the `Code` enum, the `REGISTRY` table and the lookups from one list,
// so a code cannot exist without a registry entry.
macro_rules! code_registry {
    ($( $variant:ident => $code:literal, $title:literal; )+) => {
        /// Every error code this component can report.
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
            /// The code as text, for example `TUR-AI-MEMORY-001`.
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
    MemoryOff => "TUR-AI-MEMORY-001", "Memory is turned off; nothing is written or recalled.";
    MemoryNotFound => "TUR-AI-MEMORY-002", "No memory entry has that identifier.";
    MemoryInvalidEntry => "TUR-AI-MEMORY-003", "The memory entry is outside the limits on text, tags, application, provenance or procedure.";
    MemoryNotPending => "TUR-AI-MEMORY-004", "The memory entry is not waiting for confirmation.";
    MemoryNotConfirmed => "TUR-AI-MEMORY-005", "The operation is the user's to make, and the confirmation given does not cover it.";
    MemoryDamaged => "TUR-AI-MEMORY-006", "A file in the memory store is damaged; it was left in place.";
    MemoryStore => "TUR-AI-MEMORY-007", "The memory store could not be read or written.";
    MemoryFull => "TUR-AI-MEMORY-008", "The memory store holds the largest number of entries it accepts.";
    MemoryInvalidId => "TUR-AI-MEMORY-009", "The text is not a memory entry identifier.";
    MemorySecretPassword => "TUR-AI-MEMORY-SECRET-001", "Refused: the text looks like it holds a password, passphrase or PIN.";
    MemorySecretKnownToken => "TUR-AI-MEMORY-SECRET-002", "Refused: the text holds what looks like an API key or access token of a known form.";
    MemorySecretHighEntropy => "TUR-AI-MEMORY-SECRET-003", "Refused: the text holds a long random-looking string that may be a key or token.";
    MemorySecretPrivateKey => "TUR-AI-MEMORY-SECRET-004", "Refused: the text holds a private-key block.";
    MemorySecretPaymentCard => "TUR-AI-MEMORY-SECRET-005", "Refused: the text holds what looks like a payment-card number.";
    MemorySecretOneTimeCode => "TUR-AI-MEMORY-SECRET-006", "Refused: the text holds what looks like a one-time code.";
    ScheduleNotFound => "TUR-AI-SCHEDULE-001", "No schedule or run record has that identifier.";
    ScheduleInvalidProposal => "TUR-AI-SCHEDULE-002", "The proposed schedule is outside the limits on its name, message, request or approvals.";
    ScheduleInvalidTrigger => "TUR-AI-SCHEDULE-003", "The trigger is not valid.";
    ScheduleTooFrequent => "TUR-AI-SCHEDULE-004", "The schedule would fire more often than the limit allows.";
    ScheduleLimit => "TUR-AI-SCHEDULE-005", "The largest number of schedules already exists.";
    ScheduleNotConfirmed => "TUR-AI-SCHEDULE-006", "Creating or changing a schedule is the user's act, and the confirmation given does not cover it.";
    ScheduleDamaged => "TUR-AI-SCHEDULE-007", "A file in the schedule store is damaged; it was left in place.";
    ScheduleStore => "TUR-AI-SCHEDULE-008", "The schedule store could not be read or written.";
    ScheduleInvalidApproval => "TUR-AI-SCHEDULE-009", "A standing approval does not name one service, one tool and its arguments in the accepted form.";
    ScheduleSecret => "TUR-AI-SCHEDULE-010", "Refused: the schedule text or an approval argument looks like it holds a secret.";
    ScheduleInvalidRun => "TUR-AI-SCHEDULE-011", "The run record is not consistent.";
    ScheduleInvalidUnit => "TUR-AI-SCHEDULE-012", "The systemd unit cannot be rendered from these values.";
    ScheduleInvalidId => "TUR-AI-SCHEDULE-013", "The text is not a schedule or run identifier.";
    InputInvalid => "TUR-AI-MEMORY-INPUT-001", "A command-line value is not valid.";
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
            .expect("every registry entry is a valid error code")
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One refusal or failure, with its stable code.
///
/// The detail is for a person reading a terminal or a log. It never contains
/// the text of a memory entry or of a refused secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The stable error code.
    pub code: Code,
    /// What exactly is wrong.
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
        for (code, text, title) in REGISTRY {
            let parsed: ErrorCode = text.parse().unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(parsed.family(), Family::Ai, "{text}");
            assert_eq!(code.as_str(), *text);
            assert_eq!(code.title(), *title);
            assert_eq!(code.error_code(), parsed);
            assert!(title.ends_with('.'), "{text}: title must be a sentence");
            assert!(
                text.starts_with("TUR-AI-MEMORY-") || text.starts_with("TUR-AI-SCHEDULE-"),
                "{text}"
            );
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
        let problem = Problem::new(Code::MemoryNotFound, "no entry m-00000001");
        assert_eq!(
            problem.to_string(),
            "TUR-AI-MEMORY-002: no entry m-00000001"
        );
    }
}

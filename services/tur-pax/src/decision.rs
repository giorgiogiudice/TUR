// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the policy engine decides.

use tur_core::ErrorCode;

use crate::class::ActionClass;
use crate::codes::Code;

/// The outcome of a decision.
///
/// Outcomes have a total strictness order, [`Verdict::strictness`]:
/// allow, ask, authenticate, defer, refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The action may run now.
    Allow,
    /// The user must approve this one action first.
    Ask {
        /// The text to show the user. It states the exact action.
        prompt: String,
    },
    /// A person must authenticate on the trusted confirmation path.
    Authenticate {
        /// The text to show on the trusted path.
        prompt: String,
    },
    /// Nobody can be asked now; the action is held for the user's return.
    Defer,
    /// The action does not run.
    Refuse,
}

impl Verdict {
    /// The position in the strictness order: 0 for allow up to 4 for refuse.
    ///
    /// The order is by what must happen before the action can run: nothing; a
    /// confirmation; an authentication on the trusted path; the user's return
    /// (the action cannot run in this context at all); never.
    pub const fn strictness(&self) -> u8 {
        match self {
            Verdict::Allow => 0,
            Verdict::Ask { .. } => 1,
            Verdict::Authenticate { .. } => 2,
            Verdict::Defer => 3,
            Verdict::Refuse => 4,
        }
    }

    /// The verdict as text: `allow`, `ask`, `authenticate`, `defer` or `refuse`.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Ask { .. } => "ask",
            Verdict::Authenticate { .. } => "authenticate",
            Verdict::Defer => "defer",
            Verdict::Refuse => "refuse",
        }
    }
}

/// Why an action is refused. Every refusal has a `TUR-AI-*` error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The action is class 4.
    Prohibited,
    /// The action is incomplete and cannot be classified.
    MalformedAction,
    /// The target application is excluded by the user.
    ExcludedApplication,
    /// An accessibility or screen action was proposed on a window that belongs
    /// to a process with administrator rights.
    PrivilegedWindow,
    /// An accessibility or screen action was proposed while the session is locked.
    SessionLocked,
    /// The user policy is observe-only and the action is above class 0.
    ObserveOnly,
    /// The trusted path is not available and the action is above class 1.
    TrustedPathUnavailable,
    /// A class 3 action was proposed with nobody present to authenticate.
    SystemCriticalUnattended,
    /// A system-critical operation was proposed as a privileged request: a
    /// command in the privileged sandbox, or a typed operation.
    SystemCriticalAsCommand,
    /// A privileged request was proposed with nobody present.
    PrivilegedUnattended,
    /// The audit record could not be written.
    AuditUnavailable,
}

impl Refusal {
    /// Every refusal.
    pub const ALL: [Refusal; 11] = [
        Refusal::Prohibited,
        Refusal::MalformedAction,
        Refusal::ExcludedApplication,
        Refusal::PrivilegedWindow,
        Refusal::SessionLocked,
        Refusal::ObserveOnly,
        Refusal::TrustedPathUnavailable,
        Refusal::SystemCriticalUnattended,
        Refusal::SystemCriticalAsCommand,
        Refusal::PrivilegedUnattended,
        Refusal::AuditUnavailable,
    ];

    /// The error code of the refusal.
    pub const fn code(self) -> Code {
        match self {
            Refusal::Prohibited => Code::PolicyProhibited,
            Refusal::MalformedAction => Code::PolicyMalformedAction,
            Refusal::ExcludedApplication => Code::PolicyExcludedApplication,
            Refusal::PrivilegedWindow => Code::PolicyPrivilegedWindow,
            Refusal::SessionLocked => Code::PolicySessionLocked,
            Refusal::ObserveOnly => Code::PolicyObserveOnly,
            Refusal::TrustedPathUnavailable => Code::PolicyTrustedPathUnavailable,
            Refusal::SystemCriticalUnattended => Code::PolicySystemCriticalUnattended,
            Refusal::SystemCriticalAsCommand => Code::PolicySystemCriticalAsCommand,
            Refusal::PrivilegedUnattended => Code::PolicyPrivilegedUnattended,
            Refusal::AuditUnavailable => Code::AuditUnavailable,
        }
    }

    /// The machine-readable name of the refusal.
    pub const fn as_str(self) -> &'static str {
        match self {
            Refusal::Prohibited => "prohibited",
            Refusal::MalformedAction => "malformed_action",
            Refusal::ExcludedApplication => "excluded_application",
            Refusal::PrivilegedWindow => "privileged_window",
            Refusal::SessionLocked => "session_locked",
            Refusal::ObserveOnly => "observe_only",
            Refusal::TrustedPathUnavailable => "trusted_path_unavailable",
            Refusal::SystemCriticalUnattended => "system_critical_unattended",
            Refusal::SystemCriticalAsCommand => "system_critical_as_command",
            Refusal::PrivilegedUnattended => "privileged_unattended",
            Refusal::AuditUnavailable => "audit_unavailable",
        }
    }
}

/// The machine-readable reason for a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Allowed: class 0 reads state only.
    ObservePermitted,
    /// Allowed: class 1 can be undone.
    ReversiblePermitted,
    /// Allowed: unattended class 2 tool call that exactly matches a standing approval.
    StandingApprovalMatched,
    /// Allowed: class 2 UI action in an application the user relaxed.
    RelaxedByUserPolicy,
    /// Allowed: typed privileged operation inside an open repair session.
    RepairSessionOpen,
    /// Ask: class 2 needs the user's confirmation.
    ConsequentialNeedsConfirmation,
    /// Ask: the user policy asks before every action.
    AskEveryActionMode,
    /// Ask: a free-text privileged command is approved one at a time, inside
    /// an open repair session too.
    PrivilegedCommandNeedsApproval,
    /// Authenticate: class 3 needs its own authentication on the trusted path.
    SystemCriticalNeedsAuthentication,
    /// Authenticate: a privileged request needs an open repair session.
    RepairSessionRequired,
    /// Defer: unattended class 2 action without a matching standing approval.
    UnattendedWithoutStandingApproval,
    /// Defer: unattended while the user policy asks before every action.
    UnattendedAskEveryActionMode,
    /// Defer: while the session is locked an unattended task runs tools and
    /// read-only commands only.
    UnattendedCommandWhileLocked,
    /// Defer: an attended task is paused while the session is locked.
    PausedWhileLocked,
    /// Defer: an unattended task records only observed memory entries; an
    /// entry on the user's word waits for the user.
    UnattendedMemoryNotObserved,
    /// Refused, for the stated reason.
    Refused(Refusal),
}

impl Reason {
    /// Every reason that is not a refusal. The refusals are [`Refusal::ALL`].
    pub const ALL_BUT_REFUSALS: [Reason; 15] = [
        Reason::ObservePermitted,
        Reason::ReversiblePermitted,
        Reason::StandingApprovalMatched,
        Reason::RelaxedByUserPolicy,
        Reason::RepairSessionOpen,
        Reason::ConsequentialNeedsConfirmation,
        Reason::AskEveryActionMode,
        Reason::PrivilegedCommandNeedsApproval,
        Reason::SystemCriticalNeedsAuthentication,
        Reason::RepairSessionRequired,
        Reason::UnattendedWithoutStandingApproval,
        Reason::UnattendedAskEveryActionMode,
        Reason::UnattendedCommandWhileLocked,
        Reason::PausedWhileLocked,
        Reason::UnattendedMemoryNotObserved,
    ];

    /// The machine-readable name of the reason.
    pub const fn as_str(self) -> &'static str {
        match self {
            Reason::ObservePermitted => "observe_permitted",
            Reason::ReversiblePermitted => "reversible_permitted",
            Reason::StandingApprovalMatched => "standing_approval_matched",
            Reason::RelaxedByUserPolicy => "relaxed_by_user_policy",
            Reason::RepairSessionOpen => "repair_session_open",
            Reason::ConsequentialNeedsConfirmation => "consequential_needs_confirmation",
            Reason::AskEveryActionMode => "ask_every_action_mode",
            Reason::PrivilegedCommandNeedsApproval => "privileged_command_needs_approval",
            Reason::SystemCriticalNeedsAuthentication => "system_critical_needs_authentication",
            Reason::RepairSessionRequired => "repair_session_required",
            Reason::UnattendedWithoutStandingApproval => "unattended_without_standing_approval",
            Reason::PausedWhileLocked => "paused_while_locked",
            Reason::UnattendedAskEveryActionMode => "unattended_ask_every_action_mode",
            Reason::UnattendedCommandWhileLocked => "unattended_command_while_locked",
            Reason::UnattendedMemoryNotObserved => "unattended_memory_not_observed",
            Reason::Refused(refusal) => refusal.as_str(),
        }
    }
}

/// The decision for one proposed action.
///
/// A decision can only be produced by the policy engine. There is no input
/// that turns an `ask` or an `authenticate` into an `allow`: the confirmation
/// or authentication happens outside the engine, for that one action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    verdict: Verdict,
    class: ActionClass,
    reason: Reason,
}

impl Decision {
    pub(crate) fn allow(class: ActionClass, reason: Reason) -> Self {
        Decision {
            verdict: Verdict::Allow,
            class,
            reason,
        }
    }

    pub(crate) fn ask(class: ActionClass, reason: Reason, prompt: String) -> Self {
        Decision {
            verdict: Verdict::Ask { prompt },
            class,
            reason,
        }
    }

    pub(crate) fn authenticate(class: ActionClass, reason: Reason, prompt: String) -> Self {
        Decision {
            verdict: Verdict::Authenticate { prompt },
            class,
            reason,
        }
    }

    pub(crate) fn defer(class: ActionClass, reason: Reason) -> Self {
        Decision {
            verdict: Verdict::Defer,
            class,
            reason,
        }
    }

    pub(crate) fn refuse(class: ActionClass, refusal: Refusal) -> Self {
        Decision {
            verdict: Verdict::Refuse,
            class,
            reason: Reason::Refused(refusal),
        }
    }

    /// The outcome.
    pub fn verdict(&self) -> &Verdict {
        &self.verdict
    }

    /// The class the action was treated as, after every rule that can raise it.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// The machine-readable reason.
    pub fn reason(&self) -> Reason {
        self.reason
    }

    /// Whether the action may run now.
    pub fn is_allow(&self) -> bool {
        matches!(self.verdict, Verdict::Allow)
    }

    /// The position of the outcome in the strictness order. See
    /// [`Verdict::strictness`].
    pub fn strictness(&self) -> u8 {
        self.verdict.strictness()
    }

    /// The text to show the user, for `ask` and `authenticate`.
    pub fn prompt(&self) -> Option<&str> {
        match &self.verdict {
            Verdict::Ask { prompt } | Verdict::Authenticate { prompt } => Some(prompt.as_str()),
            Verdict::Allow | Verdict::Defer | Verdict::Refuse => None,
        }
    }

    /// The registry code, present exactly when the action is refused.
    pub fn code(&self) -> Option<Code> {
        match self.reason {
            Reason::Refused(refusal) => Some(refusal.code()),
            _ => None,
        }
    }

    /// The `TUR-AI-*` error code, present exactly when the action is refused.
    pub fn error_code(&self) -> Option<ErrorCode> {
        self.code().map(Code::error_code)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn strictness_is_a_total_order_over_the_verdicts() {
        let verdicts = [
            Verdict::Allow,
            Verdict::Ask {
                prompt: String::new(),
            },
            Verdict::Authenticate {
                prompt: String::new(),
            },
            Verdict::Defer,
            Verdict::Refuse,
        ];
        for (index, verdict) in verdicts.iter().enumerate() {
            assert_eq!(usize::from(verdict.strictness()), index);
        }
        let names: BTreeSet<&str> = verdicts.iter().map(Verdict::as_str).collect();
        assert_eq!(names.len(), verdicts.len());
    }

    #[test]
    fn every_refusal_has_its_own_code_and_name() {
        let codes: BTreeSet<Code> = Refusal::ALL.iter().map(|refusal| refusal.code()).collect();
        assert_eq!(codes.len(), Refusal::ALL.len());
        let names: BTreeSet<&str> = Refusal::ALL
            .iter()
            .map(|refusal| refusal.as_str())
            .collect();
        assert_eq!(names.len(), Refusal::ALL.len());
    }

    #[test]
    fn every_reason_has_its_own_name() {
        let mut names: BTreeSet<&str> = Reason::ALL_BUT_REFUSALS
            .iter()
            .map(|reason| reason.as_str())
            .collect();
        assert_eq!(names.len(), Reason::ALL_BUT_REFUSALS.len());
        for refusal in Refusal::ALL {
            assert!(
                names.insert(Reason::Refused(refusal).as_str()),
                "{refusal:?}"
            );
        }
        for name in names {
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
        }
    }

    #[test]
    fn only_a_refusal_carries_an_error_code() {
        let refused = Decision::refuse(ActionClass::Prohibited, Refusal::Prohibited);
        assert_eq!(refused.code(), Some(Code::PolicyProhibited));
        assert_eq!(
            refused.error_code().map(|code| code.to_string()),
            Some("TUR-AI-POLICY-001".to_owned())
        );
        assert_eq!(refused.prompt(), None);

        let allowed = Decision::allow(ActionClass::Observe, Reason::ObservePermitted);
        assert_eq!(allowed.error_code(), None);
        assert!(allowed.is_allow());

        let asked = Decision::ask(
            ActionClass::Consequential,
            Reason::ConsequentialNeedsConfirmation,
            "text".to_owned(),
        );
        assert_eq!(asked.prompt(), Some("text"));
        assert_eq!(asked.error_code(), None);
        assert!(!asked.is_allow());
    }
}

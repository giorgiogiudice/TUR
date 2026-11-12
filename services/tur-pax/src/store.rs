// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The policy store, and the only way a standing approval reaches the engine.
//!
//! The policy file, the standing approvals and the repair-session record are
//! owned by a separate system component (addendum A section 17, item 5).
//! P@x reads them through it and cannot write them. For standing approvals
//! the addendum goes further (section 18, item 1): a schedule and its
//! approvals may not live anywhere P@x's own process can write, because a
//! compromised P@x could then give itself an approval. **Until the component
//! that owns them exists, standing approvals are not honoured: an unattended
//! class 2 action is always deferred.**
//!
//! This module makes that structural instead of a rule callers must remember:
//!
//! * [`PolicyStore`] is the interface of that component, as far as the engine
//!   needs it.
//! * [`HeldApprovals`] is what a [`crate::Context`] carries. Its content is
//!   private. It can be empty ([`HeldApprovals::none`]), or it can be read
//!   from a policy store ([`HeldApprovals::read`]). There is no third way: no
//!   conversion from a list, no deserialisation, no public field.
//! * **Nothing in this crate implements [`PolicyStore`]**, outside its tests.
//!   The component does not exist yet. So no code path of the shipped
//!   programs can hand the engine an approval: not the task loop, not
//!   `tur-pax policy check`, not a file or an option. A test
//!   (`tests/invariants.rs`,
//!   `no_shipped_code_can_hand_the_engine_a_standing_approval`) fails if an
//!   implementation appears in the crate's sources.
//!
//! When the component is built, its client implements [`PolicyStore`] here,
//! and that change is the reviewed step that turns standing approvals on.
//!
//! A list of approvals cannot be turned into what the engine honours:
//!
//! ```compile_fail
//! use tur_pax::{HeldApprovals, StandingApproval};
//!
//! let written_by_anyone: Vec<StandingApproval> = Vec::new();
//! // The field is private: this does not compile.
//! let forged = HeldApprovals { approvals: written_by_anyone };
//! ```
//!
//! ```compile_fail
//! use tur_pax::{HeldApprovals, StandingApproval};
//!
//! let written_by_anyone: Vec<StandingApproval> = Vec::new();
//! // There is no conversion either: this does not compile.
//! let forged: HeldApprovals = written_by_anyone.into();
//! ```

use crate::action::ToolCall;
use crate::context::StandingApproval;

/// The system component that owns the policy file, the standing approvals
/// and the repair-session record (addendum A section 17, item 5, and
/// section 18, item 1).
///
/// It is a process of its own, which P@x's process cannot write to; it changes
/// what it holds only on the user's confirmation, in the trusted session. An
/// implementation of this trait is a client of that component.
///
/// **No implementation exists outside tests.** See the module documentation.
pub trait PolicyStore {
    /// The standing approvals the store holds for a schedule: the ones the
    /// user granted when confirming it, and has not revoked.
    ///
    /// `schedule` is the store's identifier of the schedule that started the
    /// task. A store that does not know it returns nothing.
    fn standing_approvals(&self, schedule: &str) -> Vec<StandingApproval>;
}

/// The standing approvals of the schedule that started a task, as a policy
/// store holds them.
///
/// This is the only form in which a standing approval reaches the engine.
/// See the module documentation for why it cannot be built from a list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeldApprovals {
    approvals: Vec<StandingApproval>,
}

impl HeldApprovals {
    /// No approvals. This is what every caller has while no policy store
    /// exists, and what an attended task has always.
    pub const fn none() -> Self {
        HeldApprovals {
            approvals: Vec::new(),
        }
    }

    /// The approvals a policy store holds for a schedule.
    pub fn read(store: &dyn PolicyStore, schedule: &str) -> Self {
        HeldApprovals {
            approvals: store.standing_approvals(schedule),
        }
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.approvals.is_empty()
    }

    /// How many there are.
    pub fn len(&self) -> usize {
        self.approvals.len()
    }

    /// The approvals, to be read.
    pub fn iter(&self) -> std::slice::Iter<'_, StandingApproval> {
        self.approvals.iter()
    }

    /// Whether one of the approvals covers exactly this call
    /// ([`StandingApproval::matches`]).
    pub fn covers(&self, call: &ToolCall) -> bool {
        self.approvals.iter().any(|approval| approval.matches(call))
    }
}

impl<'a> IntoIterator for &'a HeldApprovals {
    type Item = &'a StandingApproval;
    type IntoIter = std::slice::Iter<'a, StandingApproval>;

    fn into_iter(self) -> Self::IntoIter {
        self.approvals.iter()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::*;
    use crate::class::ActionClass;

    /// A stand-in for the component that does not exist yet.
    struct TestStore(Vec<(&'static str, StandingApproval)>);

    impl PolicyStore for TestStore {
        fn standing_approvals(&self, schedule: &str) -> Vec<StandingApproval> {
            self.0
                .iter()
                .filter(|(owner, _)| *owner == schedule)
                .map(|(_, approval)| approval.clone())
                .collect()
        }
    }

    fn arguments(to: &str) -> Map<String, Value> {
        json!({ "to": to }).as_object().cloned().unwrap_or_default()
    }

    fn approval(to: &str) -> StandingApproval {
        StandingApproval {
            service: "org.tur.Test1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: arguments(to),
        }
    }

    fn call(to: &str) -> ToolCall {
        ToolCall {
            service: "org.tur.Test1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: arguments(to),
            declared_class: ActionClass::Consequential,
        }
    }

    #[test]
    fn without_a_store_there_is_nothing_to_honour() {
        for held in [HeldApprovals::none(), HeldApprovals::default()] {
            assert!(held.is_empty());
            assert_eq!(held.len(), 0);
            assert_eq!(held.iter().count(), 0);
            assert!(!held.covers(&call("a@example.org")));
        }
        assert_eq!(HeldApprovals::none(), HeldApprovals::default());
    }

    #[test]
    fn approvals_are_read_from_the_store_for_one_schedule_only() {
        let store = TestStore(vec![
            ("weekly-report", approval("a@example.org")),
            ("weekly-report", approval("b@example.org")),
            ("monthly-report", approval("c@example.org")),
        ]);
        let held = HeldApprovals::read(&store, "weekly-report");
        assert_eq!(held.len(), 2);
        assert!(!held.is_empty());
        assert!(held.covers(&call("a@example.org")));
        assert!(held.covers(&call("b@example.org")));
        // Another schedule's approval is not this schedule's.
        assert!(!held.covers(&call("c@example.org")));
        // A near miss is not covered.
        assert!(!held.covers(&call("A@example.org")));
        assert_eq!((&held).into_iter().count(), 2);

        // A schedule the store does not know has nothing.
        assert!(HeldApprovals::read(&store, "unknown").is_empty());
        assert!(HeldApprovals::read(&store, "").is_empty());
        assert!(HeldApprovals::read(&TestStore(Vec::new()), "weekly-report").is_empty());
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x's local memory, and its reminders and scheduled tasks.
//!
//! This crate is the storage, the rules and the schedule arithmetic behind
//! sections 16.1, 16.2 and 5.6 of the AI control layer design (addendum A),
//! as a library and a small command-line tool. It talks to no model, opens no
//! network connection and uses no D-Bus. The design document is
//! `docs/architecture/pax-memory-and-schedules.md`.
//!
//! # The two rules the types carry
//!
//! **Memory is data, never authority.** Recall returns a [`RecalledNotes`],
//! whose only rendering frames every entry as a quoted note that is not an
//! instruction. No type of the memory half converts into a type of the
//! schedule half:
//!
//! ```compile_fail
//! use tur_pax_memory::{ApprovalRequest, MemoryEntry};
//!
//! // A remembered entry is not a request for approval, and there is no
//! // conversion that makes it one.
//! fn launder(entry: MemoryEntry) -> ApprovalRequest {
//!     entry.into()
//! }
//! ```
//!
//! A recalled note does not hand out its bare text:
//!
//! ```compile_fail
//! use tur_pax_memory::Recalled;
//!
//! fn bare(note: &Recalled) -> &str {
//!     note.text()
//! }
//! ```
//!
//! **A proposal is not a schedule.** The assistant can build a [`Proposal`].
//! A [`Schedule`], and the [`StandingApproval`]s inside it, exist only after
//! [`ScheduleBook::create`] was given a [`UserConfirmation`] for that exact
//! proposal, and this library has no type that implements
//! [`UserConfirmation`]. There is no other constructor:
//!
//! ```compile_fail
//! use tur_pax_memory::{Proposal, Schedule};
//!
//! fn skip_the_user(proposal: Proposal) -> Schedule {
//!     proposal.into()
//! }
//! ```
//!
//! ```compile_fail
//! use tur_pax_memory::Schedule;
//!
//! // A schedule cannot be deserialised outside this crate either.
//! fn from_text(text: &str) -> Schedule {
//!     serde_json::from_str(text).unwrap()
//! }
//! ```
//!
//! ```compile_fail
//! use tur_pax_memory::{ApprovalRequest, StandingApproval};
//!
//! fn approve_myself(request: ApprovalRequest) -> StandingApproval {
//!     StandingApproval::granted(request).unwrap()
//! }
//! ```
//!
//! The same code with the user in it compiles. The confirmation type is the
//! caller's; here the test stands in for the settings interface:
//!
//! ```
//! use tur_pax_memory::{
//!     MemoryStore, MissedRunPolicy, Proposal, ProposedAction, ScheduleBook, Timestamp, Trigger,
//!     UserConfirmation,
//! };
//!
//! struct Shown(Proposal);
//!
//! impl UserConfirmation<Proposal> for Shown {
//!     fn covers(&self, proposal: &Proposal) -> bool {
//!         self.0 == *proposal
//!     }
//!     fn confirmed_at(&self) -> Timestamp {
//!         Timestamp(1_000)
//!     }
//! }
//!
//! let proposal = Proposal {
//!     name: "Bins".to_owned(),
//!     action: ProposedAction::Reminder { message: "Take the bins out.".to_owned() },
//!     trigger: Trigger::Once { at: Timestamp(5_000) },
//!     missed_runs: MissedRunPolicy::RunOnce,
//!     proposed_by_task: None,
//! };
//! let mut book = ScheduleBook::new(MemoryStore::new());
//!
//! // The user was shown this proposal: a schedule is created.
//! let shown = Shown(proposal.clone());
//! let schedule = book.create(proposal.clone(), &shown).unwrap();
//! assert_eq!(schedule.name(), "Bins");
//!
//! // The user was shown something else: nothing is created.
//! let mut other = proposal.clone();
//! other.name = "Something else".to_owned();
//! assert!(book.create(other, &shown).is_err());
//! ```

#![forbid(unsafe_code)]

pub mod approval;
pub mod cli;
pub mod codes;
pub mod confirm;
pub mod entry;
pub mod fs;
pub mod ids;
mod json;
pub mod memory;
pub mod recall;
pub mod schedule;
pub mod secret;
pub mod store;
pub mod systemd;
#[cfg(test)]
mod testing;
pub mod time;
pub mod trigger;

pub use approval::{ApprovalRequest, StandingApproval};
pub use codes::{Code, Problem};
pub use confirm::UserConfirmation;
pub use entry::{
    EntryKind, EntryState, MemoryEntry, NewEntry, Procedure, ProcedureStep, Provenance, Trust,
};
pub use fs::{CrashPoint, FileSystem, MemFs, StdFs};
pub use ids::{EntryId, RunId, ScheduleId};
pub use memory::{DamagedFile, Filter, Listing, Memory, MemorySwitch};
pub use recall::{RecallBudget, Recalled, RecalledNotes};
pub use schedule::{
    ActionRecord, DeferReason, DeferredAction, NewRun, Proposal, ProposedAction, RunListing,
    RunOutcome, RunRecord, Schedule, ScheduleBook, ScheduleEdit, ScheduleListing, ScheduledAction,
};
pub use secret::SecretKind;
pub use store::{Collection, FileStore, MemoryStore, Store, StoreError, default_store_dir};
pub use time::{Clock, ManualClock, SystemClock, Timestamp};
pub use trigger::{
    IntervalUnit, LocalTime, MissedRunPolicy, NextRun, ShortMonth, SystemEvent, Trigger, Weekday,
    event_may_fire, next_run,
};

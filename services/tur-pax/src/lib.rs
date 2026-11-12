// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The TUR AI control layer: the tool manifest, the policy engine, the audit
//! record, and the orchestrator that runs a task for P@x.
//!
//! The first half of this crate decides what the assistant may do. It
//! contains no model, no network code and no D-Bus code. The design is the AI
//! control layer addendum (`docs/architecture/ai-control-layer.md`); the
//! behaviour implemented here is described in
//! `docs/architecture/agent-policy.md`.
//!
//! The second half is the orchestrator, described in
//! `docs/architecture/pax-orchestrator.md`: the task loop
//! ([`orchestrator`]), the tool registry and its adapters ([`registry`],
//! [`adapter`]), the read-only sandbox ([`sandbox`], [`exec`]), the output
//! filter ([`redact`]), the audit trail of a task ([`trail`]) and the text
//! interface ([`textui`]). It reaches a model only through
//! `tur-pax-providers`, and it runs nothing outside the sandbox.
//!
//! The central item is [`decide`], a pure function from a proposed [`Action`],
//! its [`Context`], the [`UserPolicy`] and the current time to a [`Decision`].
//! Nothing the model says can make a decision more permissive.
//!
//! The decision to enforce the assistant's authority this way is recorded in
//! `docs/adr/0012-assistant-policy-model.md`; the first independent review of
//! this crate is `docs/reviews/policy-engine-review-1.md`.
//!
//! ```
//! use tur_pax::{Action, ActionClass, Context, Timestamp, ToolManifest, UserPolicy, decide};
//!
//! let manifest = ToolManifest::from_json(
//!     r#"{
//!         "schema_version": 1,
//!         "service": "org.tur.Storage1",
//!         "tools": [{
//!             "name": "storage_format",
//!             "description": "Format a volume.",
//!             "class": 3,
//!             "method": "Format",
//!             "input_schema": { "type": "object", "additionalProperties": false }
//!         }]
//!     }"#,
//! )
//! .unwrap();
//! let call = manifest.tool_call("storage_format", serde_json::Map::new()).unwrap();
//!
//! let mut context = Context::attended();
//! context.trusted_session_proven = true;
//! let decision = decide(&Action::Tool(call), &context, &UserPolicy::default(), Timestamp(0));
//!
//! assert_eq!(decision.class(), ActionClass::SystemCritical);
//! assert!(!decision.is_allow());
//! ```

#![forbid(unsafe_code)]

pub mod action;
pub mod adapter;
pub mod audit;
pub mod class;
pub mod cli;
pub mod codes;
pub mod context;
pub mod decision;
pub mod engine;
pub mod exec;
pub mod faults;
pub mod manifest;
pub mod orchestrator;
pub mod patterns;
pub mod policy;
pub mod redact;
pub mod registry;
pub mod sandbox;
pub mod store;
pub mod strict;
pub mod textui;
pub mod trail;

pub use action::{
    Action, AgentOperation, CommandAction, OperationContent, PrivilegedOperation,
    PrivilegedRequestKind, ProtectedTarget, Sandbox, TargetRule, ToolCall, TypedOperation,
    TypedOperationKind, TypedOperationSpec, UiAction, UiActionKind, WorkingContext,
};
pub use audit::{
    ActionOutcome, ActionResult, AuditMeta, AuditOutcome, AuditRecord, AuditSink, AuditWriteError,
    MemorySink, OutputWithheld, decide_and_record,
};
pub use class::ActionClass;
pub use codes::{Code, Problem, RegistryEntry, Severity};
pub use context::{Context, Presence, RepairSession, StandingApproval, Timestamp, WindowOwner};
pub use decision::{Decision, Reason, Refusal, Verdict};
pub use engine::decide;
pub use manifest::{ToolManifest, ToolSpec};
pub use policy::{Evidence, KnownApplication, Mode, Relaxation, RelaxationLevel, UserPolicy};
pub use store::{HeldApprovals, PolicyStore};

pub use exec::{BwrapExecutor, ExecOutput, ExecStatus, Executor};
pub use faults::{Fault, TaskCode};
pub use orchestrator::{
    Control, NoControl, NoObserver, Observer, Orchestrator, ProposalReport, ReportItem,
    SharedControl, Signal, Situation, SystemTaskClock, TaskClock, TaskLimits, TaskOutcome,
    TaskReport, TaskRequest,
};
pub use registry::{COMMAND_TOOL, ToolFiles, ToolRegistry, read_tool_sets};
pub use sandbox::{Grant, GrantRefusal, GrantRules, Limits, SandboxPlan};
pub use trail::{EventKind, FileTrail, MemoryTrail, Trail, TrailEntry, TrailEvent};

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The UOS AI control layer, milestone A0: the tool manifest, the policy engine
//! and the audit record.
//!
//! This crate decides what the assistant may do. It contains no model, no
//! network code and no D-Bus code. The design is the AI control layer addendum
//! (`docs/architecture/ai-control-layer.md`); the behaviour implemented here is
//! described in `docs/architecture/agent-policy.md`.
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
//! use uos_agent::{Action, ActionClass, Context, Timestamp, ToolManifest, UserPolicy, decide};
//!
//! let manifest = ToolManifest::from_json(
//!     r#"{
//!         "schema_version": 1,
//!         "service": "org.uos.Storage1",
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
//! context.trusted_path_available = true;
//! let decision = decide(&Action::Tool(call), &context, &UserPolicy::default(), Timestamp(0));
//!
//! assert_eq!(decision.class(), ActionClass::SystemCritical);
//! assert!(!decision.is_allow());
//! ```

#![forbid(unsafe_code)]

pub mod action;
pub mod audit;
pub mod class;
pub mod cli;
pub mod codes;
pub mod context;
pub mod decision;
pub mod engine;
pub mod manifest;
pub mod patterns;
pub mod policy;
pub mod strict;

pub use action::{
    Action, AgentOperation, CommandAction, OperationContent, PrivilegedOperation,
    PrivilegedRequestKind, ProtectedTarget, Sandbox, TargetRule, ToolCall, TypedOperation,
    TypedOperationKind, TypedOperationSpec, UiAction, UiActionKind, WorkingContext,
};
pub use audit::{
    AuditMeta, AuditOutcome, AuditRecord, AuditSink, AuditWriteError, MemorySink, decide_and_record,
};
pub use class::ActionClass;
pub use codes::{Code, Problem, RegistryEntry, Severity};
pub use context::{Context, Presence, RepairSession, StandingApproval, Timestamp, WindowOwner};
pub use decision::{Decision, Reason, Refusal, Verdict};
pub use engine::decide;
pub use manifest::{ToolManifest, ToolSpec};
pub use policy::{Mode, Relaxation, RelaxationLevel, UserPolicy};

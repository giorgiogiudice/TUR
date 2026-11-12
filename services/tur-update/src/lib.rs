// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The TUR update manager, first slice.
//!
//! This crate reads the state of the system from `bootc status`, decides what
//! an update manager should do next, and prints the plan. **It changes
//! nothing.** No command that stages, releases, rolls back or pins a
//! deployment is run by any code here; the plans name those commands and
//! their effects, for a later slice to carry out.
//!
//! * [`bootc`]: the document `bootc status --format=json` prints, as bootc
//!   1.17.1 defines it, and nothing more lenient.
//! * [`state`]: the typed model built from it: the versions on this
//!   computer and what is known about an update.
//! * [`health`]: what `tur-boot-health status --json` says about earlier
//!   starts: rejected images, a trial in progress, whether attempts can be
//!   counted.
//! * [`conditions`]: everything else a decision depends on: power, network,
//!   free space, a repair session, the user's settings.
//! * [`policy`]: the decisions, as pure functions.
//! * [`plan`]: the commands a decision would lead to, and their effects.
//! * [`probe`]: the one place a program is started, and the one program:
//!   `bootc status --format=json`.
//! * [`codes`]: the `TUR-UPDATE-*` error-code registry (specification §29.3).
//! * [`cli`]: the `tur-update` command line.
//!
//! The design is in `docs/architecture/update-manager.md`.

#![forbid(unsafe_code)]

/// Defines a field-less enum with a stable text form.
///
/// The generated type has `all()`, `as_str()` and `from_text()`, implements
/// `Display`, and serialises as its text form.
macro_rules! string_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $text:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant
            ),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const fn all() -> &'static [$name] {
                &[$($name::$variant),+]
            }

            /// The stable text form used in JSON output and on the command line.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            /// Parses the stable text form. Returns `None` for anything else.
            pub fn from_text(text: &str) -> Option<$name> {
                $name::all().iter().copied().find(|value| value.as_str() == text)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

pub mod bootc;
pub mod cli;
pub mod codes;
pub mod conditions;
pub mod health;
pub mod plan;
pub mod policy;
pub mod probe;
pub mod state;

pub use codes::{CodeEntry, Severity, UpdateCode};
pub use conditions::{Conditions, Limits, Metered, Network, Power, Settings};
pub use health::{BootHealth, Notice, RejectedImage, Verdict};
pub use plan::{Changes, ExternalCommand, Operation, Plan, Step};
pub use policy::{Action, Decision, NextStep, Outcome, Requester};
pub use state::{Deployment, Image, Signature, Slot, SystemState, UpdateKnowledge, UpdateState};

/// Version of the JSON documents this crate emits
/// (`schemas/update-report.schema.json`).
pub const SCHEMA_VERSION: u32 = 1;

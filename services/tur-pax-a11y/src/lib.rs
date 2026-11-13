// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x's accessibility bridge: the one program that reads and operates
//! other applications through the accessibility tree (AT-SPI) for P@x.
//!
//! It runs as a child of the P@x task loop, outside the sandbox the loop
//! runs commands in, because it needs the session bus and the
//! accessibility bus, which the sandbox denies (AI control layer design,
//! section 19, item 1). The task loop gives it typed requests over a pipe
//! (`tur_pax::interface`); a model never reaches a bus. Every request it
//! carries out was decided by the policy engine in the task loop, and is
//! decided again here, on what the bus says now.
//!
//! The design is `docs/architecture/pax-accessibility.md`.
//!
//! | Module | What it holds |
//! |---|---|
//! | [`atspi`] | The AT-SPI interfaces over D-Bus: roles, states, calls |
//! | [`identity`] | Who an application is: its identifiers and its owner |
//! | [`tree`] | Windows and elements, reading within a budget, finding |
//! | [`overlay`] | P@x's indicator and pointer, and the stop that comes with them |
//! | [`act`] | Carrying out one operation on one element |
//! | [`serve`] | The protocol with the task loop |
//! | [`survey`] | What each application exposes, for the coverage table |

#![forbid(unsafe_code)]

pub mod act;
pub mod atspi;
pub mod identity;
pub mod overlay;
pub mod serve;
pub mod survey;
pub mod tree;

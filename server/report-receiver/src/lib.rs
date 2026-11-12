// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! TUR's problem report receiver (ADR-0024).
//!
//! The receiver accepts problem reports that TUR computers send with the
//! person's consent (`tur-diag report send`), and files a cleaned summary of
//! each in a private tracker. The design is in
//! `docs/architecture/problem-reports.md`; in short:
//!
//! * [`receiver::Receiver::submit`] accepts only a bundle that verifies with
//!   `tur-diag`'s own checks ([`tur_diag::verify_members`]), within the size
//!   limits, made with the `default` redaction profile and carrying a valid
//!   `report.json`. Everything else is refused with a stable
//!   `TUR-DIAG-RECEIVER-*` code ([`codes`]).
//! * [`scrub`] removes personal data a second time, with rules of its own,
//!   from everything that goes into a summary ([`summary`]).
//! * [`store`] keeps the raw bundle encrypted ([`crypto`]) for the retention
//!   period, then the sweep deletes it.
//! * [`tracker`] files the summary through the GitHub REST API, grouping
//!   duplicates by signature, and retries while the tracker is down.
//! * [`http`] is the HTTP/1 front, behind a TLS-terminating proxy.
//! * [`log`] writes events whose values are restricted to identifiers and
//!   numbers, so that free text cannot reach the log.
//!
//! Time comes from a [`clock::Clock`], so retention can be tested with a
//! clock the test sets.

#![forbid(unsafe_code)]

pub mod clock;
pub mod codes;
pub mod config;
pub mod crypto;
pub mod http;
pub mod log;
pub mod ratelimit;
pub mod receiver;
pub mod scrub;
pub mod store;
pub mod summary;
pub mod testing;
pub mod tracker;

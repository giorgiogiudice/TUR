// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Types shared by every UOS component.
//!
//! This crate is deliberately small and has no dependencies. It defines the
//! vocabulary components must agree on: stable error codes.

#![forbid(unsafe_code)]

pub mod error_code;

pub use error_code::{ErrorCode, ErrorCodeParseError, Family};

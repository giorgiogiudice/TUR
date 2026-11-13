// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! TUR local AI, first slice (ADR-0021, `docs/architecture/local-ai.md`).
//!
//! The service runs AI models on the person's own computer and serves them
//! through an OpenAI-compatible API on the loopback interface, so that P@x
//! and ordinary applications can use them like a cloud service. Nothing runs
//! and nothing is downloaded until the person asks.
//!
//! * [`hardware`]: memory, processor features and graphics devices, read
//!   from `/proc`, `/sys` and, if it is installed, `vulkaninfo --summary`.
//! * [`catalogue`]: the curated list of models (`catalogue/*.toml`,
//!   `schemas/local-model.schema.json`) and its rules.
//! * [`fit`]: whether a model fits this computer, and which one to
//!   recommend.
//! * [`store`]: the downloaded models under `$XDG_DATA_HOME/tur/models`.
//! * [`download`]: fetching a model from its publisher, or importing a file,
//!   with its size and SHA-256 checked before it can be used.
//! * [`ensure`]: a chosen model downloaded, resumed, checked and put in
//!   place in one step, with its progress as events.
//! * [`engine`]: the inference engine (llama.cpp's `llama-server`), the
//!   address it may listen on and its command line.
//! * [`serve`]: starting the engine as a child process, stopping it when it
//!   is idle or asked to, and its state for `status`.
//! * [`codes`]: the `TUR-AI-LOCAL-*` error-code registry.
//! * [`cli`]: the `tur-ai-local` command line.

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

pub mod catalogue;
pub mod cli;
pub mod codes;
pub mod download;
pub mod engine;
pub mod ensure;
pub mod fit;
pub mod hardware;
pub mod serve;
pub mod store;

pub use codes::{AiLocalCode, CodeEntry, Severity};

/// Version of the JSON documents this crate emits
/// (`schemas/ai-local-report.schema.json`).
pub const SCHEMA_VERSION: u32 = 1;

/// One gibibyte, in bytes.
pub const GIB: u64 = 1024 * 1024 * 1024;

/// Bytes as a person reads them: "1.6 GB". Decimal units, one decimal place.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_as_a_person_reads_them() {
        assert_eq!(human_bytes(0), "0 bytes");
        assert_eq!(human_bytes(999), "999 bytes");
        assert_eq!(human_bytes(1_000), "1.0 kB");
        assert_eq!(human_bytes(639_446_688), "639.4 MB");
        assert_eq!(human_bytes(2_260_000_000), "2.3 GB");
        assert_eq!(human_bytes(4_009_860 * 1024), "4.1 GB");
    }
}

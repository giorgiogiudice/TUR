// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The typed error returned when a file cannot be inspected.

use std::fmt;
use std::path::Path;

use serde::Serialize;

use crate::codes::Code;

/// The container format an error or a finding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormatFamily {
    /// ELF, the Linux executable format.
    Elf,
    /// PE/COFF, the Windows executable format.
    Pe,
    /// Mach-O, the macOS executable format.
    MachO,
    /// A macOS `.app` bundle directory.
    AppBundle,
    /// A zip archive, including Android packages.
    Zip,
    /// An OLE compound file, including Windows Installer packages.
    Ole,
    /// A script with an interpreter line.
    Script,
    /// No format was identified.
    Unknown,
}

impl FormatFamily {
    /// A label for messages, for example `Mach-O`.
    pub const fn label(self) -> &'static str {
        match self {
            FormatFamily::Elf => "ELF",
            FormatFamily::Pe => "PE",
            FormatFamily::MachO => "Mach-O",
            FormatFamily::AppBundle => "application bundle",
            FormatFamily::Zip => "zip",
            FormatFamily::Ole => "OLE compound",
            FormatFamily::Script => "script",
            FormatFamily::Unknown => "input",
        }
    }
}

/// Why an input could not be inspected.
///
/// A file whose format is simply not recognised is not an error: it is
/// reported as [`crate::Detected::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectError {
    /// The path could not be opened or read.
    Io {
        /// The path, or an empty string for in-memory input.
        path: String,
        /// The operating system's description of the failure.
        message: String,
    },
    /// The format was recognised, but the file ends before a structure that
    /// its headers say is there.
    Truncated {
        /// The recognised format.
        format: FormatFamily,
        /// The structure that is missing.
        detail: &'static str,
    },
    /// The format was recognised, but a header holds a value that cannot be
    /// valid.
    Malformed {
        /// The recognised format.
        format: FormatFamily,
        /// What is wrong.
        detail: String,
    },
    /// The file asks for more work than one inspection is allowed to do.
    LimitExceeded {
        /// The recognised format, if any.
        format: FormatFamily,
        /// Which limit was hit.
        detail: String,
    },
}

impl InspectError {
    /// The stable TUR error code for this failure.
    pub fn code(&self) -> Code {
        match self {
            InspectError::Io { .. } => Code::Unreadable,
            InspectError::LimitExceeded { .. } => Code::LimitExceeded,
            InspectError::Truncated { format, .. } | InspectError::Malformed { format, .. } => {
                match format {
                    FormatFamily::Pe => Code::WindowsDamaged,
                    FormatFamily::MachO | FormatFamily::AppBundle => Code::DarwinDamaged,
                    _ => Code::Damaged,
                }
            }
        }
    }

    /// Fills in the path of an I/O error raised below the level that knows it.
    pub(crate) fn with_path(self, path: &Path) -> Self {
        match self {
            InspectError::Io { message, .. } => InspectError::Io {
                path: path.display().to_string(),
                message,
            },
            other => other,
        }
    }

    /// An I/O error for `path`.
    pub(crate) fn io(path: &Path, error: &std::io::Error) -> Self {
        InspectError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InspectError::Io { path, message } if path.is_empty() => {
                write!(f, "cannot read the input: {message}")
            }
            InspectError::Io { path, message } => write!(f, "cannot read {path}: {message}"),
            InspectError::Truncated { format, detail } => {
                let label = format.label();
                write!(f, "the {label} file is truncated: {detail} is missing")
            }
            InspectError::Malformed { format, detail } => {
                let label = format.label();
                write!(f, "the {label} file is malformed: {detail}")
            }
            InspectError::LimitExceeded { format, detail } => {
                let label = format.label();
                write!(f, "the {label} file exceeds an inspection limit: {detail}")
            }
        }
    }
}

impl std::error::Error for InspectError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_follow_the_format_family() {
        let pe = InspectError::Truncated {
            format: FormatFamily::Pe,
            detail: "COFF file header",
        };
        assert_eq!(pe.code(), Code::WindowsDamaged);
        let macho = InspectError::Malformed {
            format: FormatFamily::MachO,
            detail: "x".to_owned(),
        };
        assert_eq!(macho.code(), Code::DarwinDamaged);
        let elf = InspectError::Truncated {
            format: FormatFamily::Elf,
            detail: "file header",
        };
        assert_eq!(elf.code(), Code::Damaged);
        let limit = InspectError::LimitExceeded {
            format: FormatFamily::Zip,
            detail: "x".to_owned(),
        };
        assert_eq!(limit.code(), Code::LimitExceeded);
    }

    #[test]
    fn messages_name_the_format_and_the_path() {
        let error = InspectError::Truncated {
            format: FormatFamily::MachO,
            detail: "file header",
        };
        assert_eq!(
            error.to_string(),
            "the Mach-O file is truncated: file header is missing"
        );
        let io = InspectError::Io {
            path: String::new(),
            message: "boom".to_owned(),
        }
        .with_path(Path::new("/tmp/x"));
        assert_eq!(io.to_string(), "cannot read /tmp/x: boom");
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Stable `UOS-DIAG-*` error codes and the two-layer error they are reported with.
//!
//! Every failure `uos-diag` can report has an entry in the registry below
//! (specification §29.3) and is shown with a user message and a technical
//! detail (specification §6.3). The meaning of a released code never changes.

use std::fmt;

use uos_core::ErrorCode;

/// How serious a registered error is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// The requested operation did not complete.
    Error,
}

impl Severity {
    /// The severity as lower-case text.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
        }
    }
}

/// Every failure `uos-diag` reports. Each kind maps to exactly one stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ErrorKind {
    /// The bundle could not be written to the requested output path.
    OutputWrite,
    /// The requested output path already exists.
    OutputExists,
    /// The bundle to verify could not be read from disk.
    BundleRead,
    /// The archive is not a readable zstd-compressed tar stream.
    ArchiveDamaged,
    /// The archive has a member that a UOS bundle never contains.
    UnsafeMember,
    /// `manifest.json` or `checksums.sha256` is absent.
    MemberMissing,
    /// `manifest.json` or `checksums.sha256` cannot be parsed.
    MetadataInvalid,
    /// The manifest declares a schema version this build does not know.
    SchemaUnsupported,
    /// A file's content does not match its recorded SHA-256 or size.
    ChecksumMismatch,
    /// The files present and the files listed are not the same set.
    FileListMismatch,
    /// The bundle expands beyond the size this build will verify.
    BundleTooLarge,
    /// A defect in `uos-diag` itself.
    Internal,
}

/// A registry entry for one error code (specification §29.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The kind this entry describes.
    pub kind: ErrorKind,
    /// The stable code, for example `UOS-DIAG-BUNDLE-007`.
    pub code: &'static str,
    /// A short title.
    pub title: &'static str,
    /// How serious the error is.
    pub severity: Severity,
    /// What `uos-diag` observed in order to raise it.
    pub detection: &'static str,
    /// What the user is told happened.
    pub user_message: &'static str,
    /// What a developer needs to know.
    pub explanation: &'static str,
    /// What the user can do next.
    pub recovery: &'static str,
}

impl ErrorKind {
    /// Every kind, in registry order.
    pub const ALL: [ErrorKind; 12] = [
        ErrorKind::OutputWrite,
        ErrorKind::OutputExists,
        ErrorKind::BundleRead,
        ErrorKind::ArchiveDamaged,
        ErrorKind::UnsafeMember,
        ErrorKind::MemberMissing,
        ErrorKind::MetadataInvalid,
        ErrorKind::SchemaUnsupported,
        ErrorKind::ChecksumMismatch,
        ErrorKind::FileListMismatch,
        ErrorKind::BundleTooLarge,
        ErrorKind::Internal,
    ];

    /// The registry entry for this kind.
    pub const fn entry(self) -> RegistryEntry {
        match self {
            ErrorKind::OutputWrite => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-COLLECT-001",
                title: "Bundle could not be written",
                severity: Severity::Error,
                detection: "Creating, writing or syncing the output file returned an I/O error.",
                user_message: "The support bundle could not be saved.",
                explanation: "The output file could not be created or written. A partial file, \
                              if one was created, has been removed.",
                recovery: "Check that the destination exists, is writable and has free space, \
                           then run the command again.",
            },
            ErrorKind::OutputExists => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-COLLECT-002",
                title: "Output file already exists",
                severity: Severity::Error,
                detection: "The output path named an existing file.",
                user_message: "A file with that name already exists, and it was left untouched.",
                explanation: "uos-diag never overwrites an existing file.",
                recovery: "Choose a different file name, or move the existing file away first.",
            },
            ErrorKind::BundleRead => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-001",
                title: "Bundle could not be read",
                severity: Severity::Error,
                detection: "Opening or reading the bundle file returned an I/O error.",
                user_message: "The support bundle could not be opened.",
                explanation: "The file is absent, unreadable, or on media that returned an error.",
                recovery: "Check the path and the permissions on the file, then try again.",
            },
            ErrorKind::ArchiveDamaged => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-002",
                title: "Archive is damaged",
                severity: Severity::Error,
                detection: "zstd decompression (including its content checksum) or tar parsing \
                            failed.",
                user_message: "The support bundle is damaged or is not a UOS support bundle.",
                explanation: "The file is not a complete zstd-compressed tar archive. It was \
                              probably truncated or altered while being copied.",
                recovery: "Copy the bundle again from where it was created, or collect a new one.",
            },
            ErrorKind::UnsafeMember => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-003",
                title: "Archive has an unexpected member",
                severity: Severity::Error,
                detection: "A member is not a regular file, has an absolute path or a path with \
                            '.' or '..' components, or appears twice.",
                user_message: "The support bundle has contents that uos-diag never produces.",
                explanation: "uos-diag writes regular files with relative paths, each exactly \
                              once. Anything else means the archive came from another tool or \
                              was altered.",
                recovery: "Do not extract this file. Collect a new bundle with uos-diag.",
            },
            ErrorKind::MemberMissing => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-004",
                title: "Required member is missing",
                severity: Severity::Error,
                detection: "manifest.json or checksums.sha256 is not in the archive.",
                user_message: "The support bundle is incomplete.",
                explanation: "Every bundle carries manifest.json and checksums.sha256. Without \
                              them the contents cannot be verified.",
                recovery: "Collect a new bundle with uos-diag.",
            },
            ErrorKind::MetadataInvalid => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-005",
                title: "Bundle metadata is invalid",
                severity: Severity::Error,
                detection: "manifest.json is not valid JSON of the expected shape, or a line of \
                            checksums.sha256 is not '<sha256>  <path>'.",
                user_message: "The support bundle's description of itself cannot be read.",
                explanation: "The manifest or the checksum list does not follow the bundle \
                              format.",
                recovery: "Collect a new bundle with uos-diag.",
            },
            ErrorKind::SchemaUnsupported => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-006",
                title: "Manifest schema version is not supported",
                severity: Severity::Error,
                detection: "manifest.json has a schema_version other than the one this build \
                            understands.",
                user_message: "The support bundle was made by a different version of uos-diag.",
                explanation: "This build verifies manifest schema version 1 only.",
                recovery: "Verify the bundle with the version of uos-diag that created it, or a \
                           newer one.",
            },
            ErrorKind::ChecksumMismatch => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-007",
                title: "Checksum mismatch",
                severity: Severity::Error,
                detection: "The SHA-256 or size of a member differs from the value recorded in \
                            checksums.sha256 or manifest.json.",
                user_message: "The support bundle has changed since it was created.",
                explanation: "At least one file no longer matches its recorded checksum.",
                recovery: "Copy the bundle again from where it was created, or collect a new one.",
            },
            ErrorKind::FileListMismatch => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-008",
                title: "File list mismatch",
                severity: Severity::Error,
                detection: "A file is listed in manifest.json or checksums.sha256 but absent \
                            from the archive, or present in the archive but not listed.",
                user_message: "The support bundle has had files added or removed.",
                explanation: "The set of files in the archive differs from the set the manifest \
                              and the checksum list record.",
                recovery: "Collect a new bundle with uos-diag.",
            },
            ErrorKind::BundleTooLarge => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-BUNDLE-009",
                title: "Bundle is too large to verify",
                severity: Severity::Error,
                detection: "The decompressed archive exceeds the verification size limit.",
                user_message: "The support bundle is larger than uos-diag will open.",
                explanation: "Verification is done in memory and refuses archives that expand \
                              beyond a fixed limit, so that a crafted file cannot exhaust memory.",
                recovery: "Collect a new bundle with uos-diag; its bundles stay below the limit.",
            },
            ErrorKind::Internal => RegistryEntry {
                kind: self,
                code: "UOS-DIAG-INTERNAL-001",
                title: "Internal error",
                severity: Severity::Error,
                detection: "Serialising or archiving data that uos-diag itself produced failed.",
                user_message: "uos-diag hit an internal error and stopped.",
                explanation: "This is a defect in uos-diag, not a problem with the computer.",
                recovery: "Report this message, including the technical detail, to the UOS \
                           developers.",
            },
        }
    }

    /// The stable code as text, for example `UOS-DIAG-BUNDLE-007`.
    pub const fn code_str(self) -> &'static str {
        self.entry().code
    }

    /// The stable code as a validated [`ErrorCode`].
    ///
    /// # Panics
    ///
    /// Panics if a registry entry holds a malformed code. The registry tests
    /// parse every entry, so a released build cannot reach this.
    pub fn code(self) -> ErrorCode {
        self.code_str()
            .parse()
            .expect("the registry tests prove that every registered code parses")
    }
}

/// The whole registry, in registry order.
pub fn registry() -> Vec<RegistryEntry> {
    ErrorKind::ALL.into_iter().map(ErrorKind::entry).collect()
}

/// An error reported by `uos-diag`: a registered kind plus what exactly went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagError {
    kind: ErrorKind,
    detail: String,
}

impl DiagError {
    /// Creates an error of `kind` with a technical `detail`.
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        DiagError {
            kind,
            detail: detail.into(),
        }
    }

    /// The registered kind.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The stable error code.
    pub fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The message for the user.
    pub fn user_message(&self) -> &'static str {
        self.kind.entry().user_message
    }

    /// The technical detail: which file, which value, which underlying error.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for DiagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entry = self.kind.entry();
        writeln!(f, "{}", entry.code)?;
        writeln!(f, "{}", entry.user_message)?;
        writeln!(f)?;
        writeln!(f, "Action:")?;
        writeln!(f, "{}", entry.recovery)?;
        writeln!(f)?;
        writeln!(f, "Technical detail:")?;
        writeln!(f, "{}", entry.explanation)?;
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for DiagError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uos_core::Family;

    use super::*;

    #[test]
    fn every_code_parses_and_belongs_to_the_diag_family() {
        for entry in registry() {
            let code: ErrorCode = entry
                .code
                .parse()
                .unwrap_or_else(|error| panic!("{} does not parse: {error}", entry.code));
            assert_eq!(code.family(), Family::Diag, "{}", entry.code);
            assert!(entry.code.starts_with("UOS-DIAG-"), "{}", entry.code);
            assert_eq!(entry.kind.code(), code);
        }
    }

    #[test]
    fn codes_are_unique_and_cover_every_kind() {
        let entries = registry();
        let codes: BTreeSet<&str> = entries.iter().map(|entry| entry.code).collect();
        assert_eq!(codes.len(), entries.len(), "two kinds share a code");
        let kinds: BTreeSet<ErrorKind> = entries.iter().map(|entry| entry.kind).collect();
        assert_eq!(kinds.len(), ErrorKind::ALL.len());
        for kind in ErrorKind::ALL {
            assert_eq!(kind.entry().kind, kind);
        }
    }

    #[test]
    fn every_entry_is_filled_in() {
        for entry in registry() {
            for (field, text) in [
                ("title", entry.title),
                ("detection", entry.detection),
                ("user_message", entry.user_message),
                ("explanation", entry.explanation),
                ("recovery", entry.recovery),
            ] {
                assert!(
                    !text.trim().is_empty(),
                    "{} has an empty {field}",
                    entry.code
                );
            }
            assert_eq!(entry.severity.as_str(), "error");
        }
    }

    #[test]
    fn released_codes_keep_their_meaning() {
        // A released code never changes meaning (specification §29.3). Changing
        // one of these pairs is a breaking change, not a refactoring.
        let pinned = [
            (ErrorKind::OutputWrite, "UOS-DIAG-COLLECT-001"),
            (ErrorKind::OutputExists, "UOS-DIAG-COLLECT-002"),
            (ErrorKind::BundleRead, "UOS-DIAG-BUNDLE-001"),
            (ErrorKind::ArchiveDamaged, "UOS-DIAG-BUNDLE-002"),
            (ErrorKind::UnsafeMember, "UOS-DIAG-BUNDLE-003"),
            (ErrorKind::MemberMissing, "UOS-DIAG-BUNDLE-004"),
            (ErrorKind::MetadataInvalid, "UOS-DIAG-BUNDLE-005"),
            (ErrorKind::SchemaUnsupported, "UOS-DIAG-BUNDLE-006"),
            (ErrorKind::ChecksumMismatch, "UOS-DIAG-BUNDLE-007"),
            (ErrorKind::FileListMismatch, "UOS-DIAG-BUNDLE-008"),
            (ErrorKind::BundleTooLarge, "UOS-DIAG-BUNDLE-009"),
            (ErrorKind::Internal, "UOS-DIAG-INTERNAL-001"),
        ];
        assert_eq!(pinned.len(), ErrorKind::ALL.len());
        for (kind, code) in pinned {
            assert_eq!(kind.code_str(), code);
        }
    }

    #[test]
    fn display_has_both_layers() {
        let error = DiagError::new(
            ErrorKind::ChecksumMismatch,
            "system/kernel.txt: sha256 differs",
        );
        let text = error.to_string();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("UOS-DIAG-BUNDLE-007"));
        assert_eq!(lines.next(), Some(error.user_message()));
        assert!(text.contains("Action:"));
        assert!(text.contains("Technical detail:"));
        assert!(text.ends_with("system/kernel.txt: sha256 differs"));
        assert_eq!(error.kind(), ErrorKind::ChecksumMismatch);
        assert_eq!(error.detail(), "system/kernel.txt: sha256 differs");
        assert_eq!(error.code().to_string(), "UOS-DIAG-BUNDLE-007");
    }
}

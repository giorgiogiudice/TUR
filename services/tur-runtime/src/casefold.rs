// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Case-insensitive prefixes: drive C: on a directory whose file system
//! folds the case of names.
//!
//! Windows finds `File.txt` when asked for `FILE.TXT`; a Linux file system
//! compares names byte for byte. Wine bridges the two in user space: when a
//! name is not found as given, it reads the whole directory and compares
//! every entry without regard to case (`find_file_in_dir` in Wine's
//! `dlls/ntdll/unix/file.c`). ext4 can make the comparison itself, per
//! directory, when the file system was made with the `casefold` feature and
//! the directory carries the `F` attribute (`EXT4_CASEFOLD_FL`). Wine reads
//! that attribute (`get_dir_case_sensitivity_stat`, the same file): in such
//! a directory the kernel finds the name in any case, and Wine skips its
//! directory scan for a name that is not there.
//!
//! The attribute can only be set on an empty directory, and every directory
//! created inside one inherits it. So `drive_c` is created empty and marked
//! before Wine writes anything into it; everything Wine and the application
//! put on drive C: is then case-insensitive. The rest of the prefix (the
//! registry files, `dosdevices`) stays as Wine expects it.
//!
//! Nothing here changes a file system. Whether the file system holding the
//! prefix has the `casefold` feature is decided when it is made, by whoever
//! makes it; here the attribute is asked for, and a file system that cannot
//! give it is an outcome, not a failure: drive C: is then an ordinary
//! directory, Wine emulates case-insensitivity as it always has, and the
//! reason is recorded.
//!
//! The attribute is set and read with e2fsprogs' `chattr +F` and `lsattr -d`
//! (this crate forbids `unsafe`, and the ioctl has no safe wrapper in the
//! standard library). Both run outside the sandbox, on a directory of the
//! runtime manager's own, with `LC_ALL=C` so that their messages can be
//! classified.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::app_record::{as_text, is_text};
use crate::codes::Code;
use crate::plan::CommandSpec;
use crate::runtime_cmd::RUNTIME_PATH;

/// Characters of a tool's message kept in a record.
const MAX_DETAIL_CHARS: usize = 500;

/// Whether a new prefix's drive C: is to be made case-insensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaseFolding {
    /// Where the file system can: `chattr +F` on the empty `drive_c`.
    Try,
    /// Never: drive C: is an ordinary directory, and Wine emulates
    /// case-insensitivity as it does on any Linux file system.
    Off,
}

/// What a prefix gets when neither `--casefold` nor `--no-casefold` is
/// given. Decided from the measurements in `docs/reports/windows-runtime-3.md`;
/// the reasoning is in `docs/architecture/runtime-manager.md`.
pub const DEFAULT_CASE_FOLDING: CaseFolding = CaseFolding::Off;

impl CaseFolding {
    /// The choice made by the command-line options.
    pub fn from_options(casefold: bool, no_casefold: bool) -> Self {
        if no_casefold {
            CaseFolding::Off
        } else if casefold {
            CaseFolding::Try
        } else {
            DEFAULT_CASE_FOLDING
        }
    }
}

/// Why a prefix's drive C: is, or is not, case-insensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaseFoldReason {
    /// It is: the file system folds the case of names in it.
    Casefolded,
    /// It was not asked for (`--no-casefold`, or the default).
    NotRequested,
    /// The file system refused the attribute as not supported: an ext4 file
    /// system made without the `casefold` feature, or a file system that
    /// has no such attribute at all.
    NoCasefoldFeature,
    /// The file system has no inode attributes that `chattr` can set.
    NoAttributes,
    /// The user may not change the directory's attributes.
    NotPermitted,
    /// The directory was not empty, so its attributes could not change.
    NotEmpty,
    /// The file system is mounted read-only.
    ReadOnly,
    /// `chattr` is not installed.
    ToolMissing,
    /// Anything else; the detail says what.
    Failed,
}

impl CaseFoldReason {
    /// Every reason, in schema order.
    pub const ALL: [CaseFoldReason; 9] = [
        CaseFoldReason::Casefolded,
        CaseFoldReason::NotRequested,
        CaseFoldReason::NoCasefoldFeature,
        CaseFoldReason::NoAttributes,
        CaseFoldReason::NotPermitted,
        CaseFoldReason::NotEmpty,
        CaseFoldReason::ReadOnly,
        CaseFoldReason::ToolMissing,
        CaseFoldReason::Failed,
    ];

    /// The code reported when case folding was asked for and not obtained.
    /// Neither stops anything: the prefix is created, case-sensitive.
    pub const fn code(self) -> Option<Code> {
        match self {
            CaseFoldReason::Casefolded | CaseFoldReason::NotRequested => None,
            CaseFoldReason::NoCasefoldFeature | CaseFoldReason::NoAttributes => {
                Some(Code::CaseFoldUnsupported)
            }
            CaseFoldReason::NotPermitted
            | CaseFoldReason::NotEmpty
            | CaseFoldReason::ReadOnly
            | CaseFoldReason::ToolMissing
            | CaseFoldReason::Failed => Some(Code::CaseFoldFailed),
        }
    }

    /// The reason in plain language.
    pub const fn describe(self) -> &'static str {
        match self {
            CaseFoldReason::Casefolded => {
                "drive C: is case-insensitive: the file system folds case"
            }
            CaseFoldReason::NotRequested => {
                "drive C: was not made case-insensitive: it was not asked for"
            }
            CaseFoldReason::NoCasefoldFeature => {
                "the file system holding the prefix cannot fold case (an ext4 file system made without the casefold feature, or another file system)"
            }
            CaseFoldReason::NoAttributes => {
                "the file system holding the prefix has no attributes chattr can set"
            }
            CaseFoldReason::NotPermitted => {
                "the attributes of the prefix's drive C: may not be changed by this user"
            }
            CaseFoldReason::NotEmpty => {
                "drive C: was not empty, and only an empty directory can be made case-insensitive"
            }
            CaseFoldReason::ReadOnly => "the file system holding the prefix is read-only",
            CaseFoldReason::ToolMissing => "chattr (e2fsprogs) is not installed",
            CaseFoldReason::Failed => "chattr could not make drive C: case-insensitive",
        }
    }
}

/// What became of a request to make a prefix's drive C: case-insensitive.
/// Kept in the application record and reported by `prefix create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseFoldRecord {
    /// Whether it was asked for.
    pub requested: bool,
    /// Whether drive C: is case-insensitive.
    pub casefolded: bool,
    /// Why, or why not.
    pub reason: CaseFoldReason,
    /// What `chattr` said when it refused, without control characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl CaseFoldRecord {
    /// The record of a prefix for which case folding was not asked for.
    pub fn not_requested() -> Self {
        CaseFoldRecord {
            requested: false,
            casefolded: false,
            reason: CaseFoldReason::NotRequested,
            detail: None,
        }
    }

    /// The record of an attempt that ended for `reason`.
    pub fn attempted(reason: CaseFoldReason, detail: Option<&str>) -> Self {
        CaseFoldRecord {
            requested: true,
            casefolded: reason == CaseFoldReason::Casefolded,
            reason,
            detail: detail.and_then(|text| as_text(text, MAX_DETAIL_CHARS)),
        }
    }

    /// Whether the fields agree with one another and the detail is
    /// storable.
    pub fn is_consistent(&self) -> bool {
        let reason_fits = match self.reason {
            CaseFoldReason::Casefolded => self.requested && self.casefolded,
            CaseFoldReason::NotRequested => !self.requested && !self.casefolded,
            _ => self.requested && !self.casefolded,
        };
        reason_fits
            && self
                .detail
                .as_deref()
                .is_none_or(|text| is_text(text, MAX_DETAIL_CHARS))
    }

    /// One line for people: the state, and why.
    pub fn summary(&self) -> String {
        let mut line = if self.casefolded {
            "case-insensitive (ext4 case folding)".to_owned()
        } else {
            format!(
                "case-sensitive; Wine emulates case-insensitivity ({})",
                self.reason.describe()
            )
        };
        if let Some(detail) = &self.detail {
            line.push_str(": ");
            line.push_str(detail);
        }
        line
    }
}

/// The command that gives `path` the case-folding attribute, or `None` when
/// the path cannot be passed as text.
pub fn chattr_command(chattr: &Path, path: &Path) -> Option<CommandSpec> {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".to_owned(), RUNTIME_PATH.to_owned());
    // The messages are classified; they must not be translated.
    environment.insert("LC_ALL".to_owned(), "C".to_owned());
    Some(CommandSpec {
        program: chattr.to_str()?.to_owned(),
        arguments: vec!["+F".to_owned(), "--".to_owned(), path.to_str()?.to_owned()],
        environment,
        working_directory: None,
    })
}

/// Classifies how `chattr +F` ended: its exit status and what it printed.
///
/// `chattr` reports an error as `chattr: <strerror> while setting flags on
/// <path>`; under `LC_ALL=C` the text of the error is the C library's.
pub fn classify(status: i32, output: &str) -> CaseFoldRecord {
    if status == 0 {
        return CaseFoldRecord::attempted(CaseFoldReason::Casefolded, None);
    }
    let reason = if output.contains("Operation not supported") {
        CaseFoldReason::NoCasefoldFeature
    } else if output.contains("Inappropriate ioctl for device") {
        CaseFoldReason::NoAttributes
    } else if output.contains("Operation not permitted") || output.contains("Permission denied") {
        CaseFoldReason::NotPermitted
    } else if output.contains("Directory not empty") {
        CaseFoldReason::NotEmpty
    } else if output.contains("Read-only file system") {
        CaseFoldReason::ReadOnly
    } else {
        CaseFoldReason::Failed
    };
    let detail = output
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("chattr exited with status {status}"));
    CaseFoldRecord::attempted(reason, Some(&detail))
}

/// Reads `lsattr -d` output for one directory: whether its attributes
/// include `F`. `None` when the output is not one line of attributes.
pub fn parse_lsattr(output: &str) -> Option<bool> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    let attributes = line.split_whitespace().next()?;
    let well_formed = !attributes.is_empty()
        && attributes
            .chars()
            .all(|character| character == '-' || character.is_ascii_alphabetic());
    well_formed.then(|| attributes.contains('F'))
}

/// Whether the directory at `path` is case-insensitive, as `lsattr -d`
/// reports it. `None` when that cannot be told: no such directory, a file
/// system without attributes, or `lsattr` failing.
pub fn probe(lsattr: &Path, path: &Path) -> Option<bool> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir() {
        return None;
    }
    let output = Command::new(lsattr)
        .arg("-d")
        .arg("--")
        .arg(path)
        .env_clear()
        .env("PATH", RUNTIME_PATH)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_lsattr(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_choose_and_the_default_fills_in() {
        assert_eq!(CaseFolding::from_options(false, true), CaseFolding::Off);
        assert_eq!(CaseFolding::from_options(true, false), CaseFolding::Try);
        assert_eq!(
            CaseFolding::from_options(false, false),
            DEFAULT_CASE_FOLDING
        );
    }

    #[test]
    fn chattr_messages_are_classified() {
        let cases = [
            (0, "", CaseFoldReason::Casefolded),
            (
                1,
                "chattr: Operation not supported while setting flags on /p/drive_c\n",
                CaseFoldReason::NoCasefoldFeature,
            ),
            (
                1,
                "chattr: Inappropriate ioctl for device while reading flags on /p/drive_c\n",
                CaseFoldReason::NoAttributes,
            ),
            (
                1,
                "chattr: Operation not permitted while setting flags on /p/drive_c\n",
                CaseFoldReason::NotPermitted,
            ),
            (
                1,
                "chattr: Permission denied while trying to stat /p/drive_c\n",
                CaseFoldReason::NotPermitted,
            ),
            (
                1,
                "chattr: Directory not empty while setting flags on /p/drive_c\n",
                CaseFoldReason::NotEmpty,
            ),
            (
                1,
                "chattr: Read-only file system while setting flags on /p/drive_c\n",
                CaseFoldReason::ReadOnly,
            ),
            (
                1,
                "chattr: Invalid argument while setting flags on /p/drive_c\n",
                CaseFoldReason::Failed,
            ),
            (1, "", CaseFoldReason::Failed),
        ];
        for (status, output, reason) in cases {
            let record = classify(status, output);
            assert_eq!(record.reason, reason, "{output:?}");
            assert!(record.requested);
            assert_eq!(record.casefolded, reason == CaseFoldReason::Casefolded);
            assert!(record.is_consistent(), "{record:?}");
            if status == 0 {
                assert_eq!(record.detail, None);
            } else {
                let detail = record.detail.unwrap();
                assert!(!detail.contains('\n'));
                if output.is_empty() {
                    assert_eq!(detail, "chattr exited with status 1");
                } else {
                    assert!(output.starts_with(&detail), "{detail}");
                }
            }
        }
        // A message with control characters is stored without them.
        let record = classify(1, "chattr: \u{1b}[31mweird while setting flags\n");
        assert!(!record.detail.unwrap().contains('\u{1b}'));
    }

    #[test]
    fn codes_follow_the_reason() {
        assert_eq!(CaseFoldReason::Casefolded.code(), None);
        assert_eq!(CaseFoldReason::NotRequested.code(), None);
        assert_eq!(
            CaseFoldReason::NoCasefoldFeature.code(),
            Some(Code::CaseFoldUnsupported)
        );
        assert_eq!(
            CaseFoldReason::NoAttributes.code(),
            Some(Code::CaseFoldUnsupported)
        );
        for reason in [
            CaseFoldReason::NotPermitted,
            CaseFoldReason::NotEmpty,
            CaseFoldReason::ReadOnly,
            CaseFoldReason::ToolMissing,
            CaseFoldReason::Failed,
        ] {
            assert_eq!(reason.code(), Some(Code::CaseFoldFailed), "{reason:?}");
        }
        for reason in CaseFoldReason::ALL {
            assert!(!reason.describe().is_empty());
        }
    }

    #[test]
    fn records_are_consistent_or_refused() {
        assert!(CaseFoldRecord::not_requested().is_consistent());
        let folded = CaseFoldRecord::attempted(CaseFoldReason::Casefolded, None);
        assert!(folded.casefolded && folded.is_consistent());
        assert!(folded.summary().starts_with("case-insensitive"));
        let refused = CaseFoldRecord::attempted(
            CaseFoldReason::NoCasefoldFeature,
            Some("chattr: Operation not supported while setting flags on /p"),
        );
        assert!(!refused.casefolded && refused.is_consistent());
        assert!(refused.summary().contains("Operation not supported"));
        let contradictions = [
            CaseFoldRecord {
                casefolded: true,
                ..CaseFoldRecord::not_requested()
            },
            CaseFoldRecord {
                casefolded: false,
                ..folded.clone()
            },
            CaseFoldRecord {
                casefolded: true,
                ..refused.clone()
            },
            CaseFoldRecord {
                requested: false,
                ..refused.clone()
            },
            CaseFoldRecord {
                detail: Some(String::new()),
                ..refused.clone()
            },
            CaseFoldRecord {
                detail: Some("x".repeat(MAX_DETAIL_CHARS + 1)),
                ..refused
            },
        ];
        for record in contradictions {
            assert!(!record.is_consistent(), "{record:?}");
        }
    }

    #[test]
    fn the_command_sets_the_attribute_in_the_c_locale() {
        let command =
            chattr_command(Path::new("/usr/bin/chattr"), Path::new("/d/p/drive_c")).unwrap();
        assert_eq!(command.program, "/usr/bin/chattr");
        assert_eq!(command.arguments, ["+F", "--", "/d/p/drive_c"]);
        assert_eq!(command.environment["LC_ALL"], "C");
        assert_eq!(command.environment.len(), 2);
    }

    #[test]
    fn lsattr_output_is_read() {
        assert_eq!(
            parse_lsattr("--------------F------- /d/p/drive_c\n"),
            Some(true)
        );
        assert_eq!(
            parse_lsattr("--------------e------- /d/p/drive_c\n"),
            Some(false)
        );
        assert_eq!(parse_lsattr("----i---------e--F---- /a b/c\n"), Some(true));
        assert_eq!(parse_lsattr(""), None);
        assert_eq!(
            parse_lsattr("lsattr: Operation not supported While reading flags on /x"),
            None
        );
    }
}

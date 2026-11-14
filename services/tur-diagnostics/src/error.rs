// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Stable `TUR-DIAG-*` error codes and the two-layer error they are reported with.
//!
//! Every failure `tur-diag` can report has an entry in the registry below
//! (specification §29.3) and is shown with a user message and a technical
//! detail (specification §6.3). The meaning of a released code never changes.

use std::fmt;

use tur_core::ErrorCode;

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

/// Every failure `tur-diag` reports. Each kind maps to exactly one stable code.
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
    /// The archive has a member that a TUR bundle never contains.
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
    /// A defect in `tur-diag` itself.
    Internal,
    /// A problem report's fields are missing or not acceptable.
    ReportInvalid,
    /// No problem-report receiver is configured.
    ReportNoReceiver,
    /// `--to` names a receiver other than the configured one.
    ReportWrongReceiver,
    /// Sending was not confirmed.
    ReportNotConfirmed,
    /// The file to send is not a prepared problem report.
    ReportNotPrepared,
    /// The receiver could not be reached.
    ReportUnreachable,
    /// The receiver answered, and refused the report.
    ReportRefused,
    /// The problem-report configuration file cannot be used.
    ReportConfigInvalid,
    /// Boot health has no failed update or failed start to report.
    ReportNothingFromBootHealth,
    /// The local list of reports cannot be read or written.
    ReportRecordsUnavailable,
    /// No sent report has the given identifier.
    ReportUnknown,
    /// The prepared report is larger than a receiver accepts.
    ReportTooLarge,
    /// No such fault or remedy in the self-healing catalogue.
    HealUnknown,
    /// The fault a remedy was asked for is not present.
    HealNotPresent,
    /// A class 2 remedy was asked for without administrator rights.
    HealNeedsApproval,
    /// A class 2 remedy was not confirmed.
    HealNotConfirmed,
    /// A remedy was asked for too often in one boot.
    HealTooOften,
    /// A detection check could not be made.
    HealCheckUnavailable,
    /// There is no applied remedy to undo, or it cannot be undone.
    HealNothingToUndo,
    /// No problem report is offered for this fault now.
    HealNoReport,
    /// The self-healing state or data directory cannot be used.
    HealStateUnavailable,
    /// A request to the healing service was not understood.
    HealRequestInvalid,
    /// The healing service's socket could not be reached.
    HealServiceUnreachable,
}

/// A registry entry for one error code (specification §29.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The kind this entry describes.
    pub kind: ErrorKind,
    /// The stable code, for example `TUR-DIAG-BUNDLE-007`.
    pub code: &'static str,
    /// A short title.
    pub title: &'static str,
    /// How serious the error is.
    pub severity: Severity,
    /// What `tur-diag` observed in order to raise it.
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
    pub const ALL: [ErrorKind; 35] = [
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
        ErrorKind::ReportInvalid,
        ErrorKind::ReportNoReceiver,
        ErrorKind::ReportWrongReceiver,
        ErrorKind::ReportNotConfirmed,
        ErrorKind::ReportNotPrepared,
        ErrorKind::ReportUnreachable,
        ErrorKind::ReportRefused,
        ErrorKind::ReportConfigInvalid,
        ErrorKind::ReportNothingFromBootHealth,
        ErrorKind::ReportRecordsUnavailable,
        ErrorKind::ReportUnknown,
        ErrorKind::ReportTooLarge,
        ErrorKind::HealUnknown,
        ErrorKind::HealNotPresent,
        ErrorKind::HealNeedsApproval,
        ErrorKind::HealNotConfirmed,
        ErrorKind::HealTooOften,
        ErrorKind::HealCheckUnavailable,
        ErrorKind::HealNothingToUndo,
        ErrorKind::HealNoReport,
        ErrorKind::HealStateUnavailable,
        ErrorKind::HealRequestInvalid,
        ErrorKind::HealServiceUnreachable,
    ];

    /// The registry entry for this kind.
    pub const fn entry(self) -> RegistryEntry {
        match self {
            ErrorKind::OutputWrite => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-COLLECT-001",
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
                code: "TUR-DIAG-COLLECT-002",
                title: "Output file already exists",
                severity: Severity::Error,
                detection: "The output path named an existing file.",
                user_message: "A file with that name already exists, and it was left untouched.",
                explanation: "tur-diag never overwrites an existing file.",
                recovery: "Choose a different file name, or move the existing file away first.",
            },
            ErrorKind::BundleRead => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-001",
                title: "Bundle could not be read",
                severity: Severity::Error,
                detection: "Opening or reading the bundle file returned an I/O error.",
                user_message: "The support bundle could not be opened.",
                explanation: "The file is absent, unreadable, or on media that returned an error.",
                recovery: "Check the path and the permissions on the file, then try again.",
            },
            ErrorKind::ArchiveDamaged => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-002",
                title: "Archive is damaged",
                severity: Severity::Error,
                detection: "zstd decompression (including its content checksum) or tar parsing \
                            failed.",
                user_message: "The support bundle is damaged or is not a TUR support bundle.",
                explanation: "The file is not a complete zstd-compressed tar archive. It was \
                              probably truncated or altered while being copied.",
                recovery: "Copy the bundle again from where it was created, or collect a new one.",
            },
            ErrorKind::UnsafeMember => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-003",
                title: "Archive has an unexpected member",
                severity: Severity::Error,
                detection: "A member is not a regular file, has an absolute path or a path with \
                            '.' or '..' components, or appears twice.",
                user_message: "The support bundle has contents that tur-diag never produces.",
                explanation: "tur-diag writes regular files with relative paths, each exactly \
                              once. Anything else means the archive came from another tool or \
                              was altered.",
                recovery: "Do not extract this file. Collect a new bundle with tur-diag.",
            },
            ErrorKind::MemberMissing => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-004",
                title: "Required member is missing",
                severity: Severity::Error,
                detection: "manifest.json or checksums.sha256 is not in the archive.",
                user_message: "The support bundle is incomplete.",
                explanation: "Every bundle carries manifest.json and checksums.sha256. Without \
                              them the contents cannot be verified.",
                recovery: "Collect a new bundle with tur-diag.",
            },
            ErrorKind::MetadataInvalid => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-005",
                title: "Bundle metadata is invalid",
                severity: Severity::Error,
                detection: "manifest.json is not valid JSON of the expected shape, or a line of \
                            checksums.sha256 is not '<sha256>  <path>'.",
                user_message: "The support bundle's description of itself cannot be read.",
                explanation: "The manifest or the checksum list does not follow the bundle \
                              format.",
                recovery: "Collect a new bundle with tur-diag.",
            },
            ErrorKind::SchemaUnsupported => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-006",
                title: "Manifest schema version is not supported",
                severity: Severity::Error,
                detection: "manifest.json has a schema_version other than the one this build \
                            understands.",
                user_message: "The support bundle was made by a different version of tur-diag.",
                explanation: "This build verifies manifest schema version 1 only.",
                recovery: "Verify the bundle with the version of tur-diag that created it, or a \
                           newer one.",
            },
            ErrorKind::ChecksumMismatch => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-007",
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
                code: "TUR-DIAG-BUNDLE-008",
                title: "File list mismatch",
                severity: Severity::Error,
                detection: "A file is listed in manifest.json or checksums.sha256 but absent \
                            from the archive, or present in the archive but not listed.",
                user_message: "The support bundle has had files added or removed.",
                explanation: "The set of files in the archive differs from the set the manifest \
                              and the checksum list record.",
                recovery: "Collect a new bundle with tur-diag.",
            },
            ErrorKind::BundleTooLarge => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-BUNDLE-009",
                title: "Bundle is too large to verify",
                severity: Severity::Error,
                detection: "The decompressed archive exceeds the verification size limit.",
                user_message: "The support bundle is larger than tur-diag will open.",
                explanation: "Verification is done in memory and refuses archives that expand \
                              beyond a fixed limit, so that a crafted file cannot exhaust memory.",
                recovery: "Collect a new bundle with tur-diag; its bundles stay below the limit.",
            },
            ErrorKind::Internal => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-INTERNAL-001",
                title: "Internal error",
                severity: Severity::Error,
                detection: "Serialising or archiving data that tur-diag itself produced failed.",
                user_message: "tur-diag hit an internal error and stopped.",
                explanation: "This is a defect in tur-diag, not a problem with the computer.",
                recovery: "Report this message, including the technical detail, to the TUR \
                           developers.",
            },
            ErrorKind::ReportInvalid => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-001",
                title: "Problem report is not valid",
                severity: Severity::Error,
                detection: "A field of the problem report is missing, too long, has control \
                            characters, or is not one of the accepted values.",
                user_message: "The problem report could not be prepared as given.",
                explanation: "A report needs a title and a component from the fixed list; an \
                              error code must be a TUR code; texts have length limits.",
                recovery: "Correct the value named in the technical detail and prepare the \
                           report again.",
            },
            ErrorKind::ReportNoReceiver => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-002",
                title: "No problem-report receiver is configured",
                severity: Severity::Error,
                detection: "RECEIVER_URL is empty or absent in every problem-report \
                            configuration file.",
                user_message: "TUR has no address to send problem reports to yet. Nothing was \
                               sent.",
                explanation: "Sending is possible only once TUR's receiver is configured in \
                              /etc/tur/problem-reports.conf or /usr/lib/tur/problem-reports.conf. \
                              The prepared report stays on this computer.",
                recovery: "Keep the prepared report, or share it yourself. It can be sent once \
                           a receiver is configured.",
            },
            ErrorKind::ReportWrongReceiver => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-003",
                title: "Not TUR's configured receiver",
                severity: Severity::Error,
                detection: "The address given with --to differs from the configured \
                            RECEIVER_URL.",
                user_message: "Problem reports go only to TUR's configured receiver. Nothing was \
                               sent.",
                explanation: "tur-diag refuses to send a report to any other address, so that a \
                              mistyped or suggested address cannot receive it.",
                recovery: "Give the configured receiver's address with --to, exactly as \
                           configured.",
            },
            ErrorKind::ReportNotConfirmed => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-004",
                title: "Sending was not confirmed",
                severity: Severity::Error,
                detection: "Neither --yes was given nor was 'yes' typed at the question.",
                user_message: "The report was not sent, because sending was not confirmed.",
                explanation: "A report is sent only after the person confirms it, with --yes or \
                              by typing yes when asked. Without a terminal to ask on, --yes is \
                              required.",
                recovery: "Read what will be sent, then run the command again and confirm.",
            },
            ErrorKind::ReportNotPrepared => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-005",
                title: "Not a prepared problem report",
                severity: Severity::Error,
                detection: "The file is not a bundle that verifies, or it has no valid \
                            report.json, or it was made with the technical profile.",
                user_message: "This file is not a problem report prepared by tur-diag.",
                explanation: "Only a bundle made by 'tur-diag report prepare' can be sent: it \
                              must verify, carry report.json and use the default redaction \
                              profile.",
                recovery: "Prepare the report with 'tur-diag report prepare' and send that file.",
            },
            ErrorKind::ReportUnreachable => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-006",
                title: "Receiver could not be reached",
                severity: Severity::Error,
                detection: "curl failed, timed out, or the receiver's answer was not a \
                            receipt.",
                user_message: "The report could not be delivered. It was not sent, and it is \
                               kept on this computer.",
                explanation: "The network, the receiver or curl failed before the receiver \
                              acknowledged the report. Nothing is retried in the background.",
                recovery: "Check the network connection and send the same file again later.",
            },
            ErrorKind::ReportRefused => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-007",
                title: "Receiver refused the report",
                severity: Severity::Error,
                detection: "The receiver answered with an error and a TUR-DIAG-RECEIVER code.",
                user_message: "TUR's receiver did not accept the report. It is kept on this \
                               computer.",
                explanation: "The technical detail carries the receiver's code and message, \
                              for example a size or rate limit.",
                recovery: "Follow the receiver's message; for a rate limit, try again later.",
            },
            ErrorKind::ReportConfigInvalid => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-008",
                title: "Problem-report configuration is not usable",
                severity: Severity::Error,
                detection: "The configuration file cannot be read, has a line that is not \
                            KEY=VALUE, or RECEIVER_URL is not an https address.",
                user_message: "The problem-report settings of this computer are not valid. \
                               Nothing was sent.",
                explanation: "RECEIVER_URL must be https://host[:port][/path] without user \
                              name, query or fragment; plain http is accepted only for this \
                              computer itself (testing).",
                recovery: "Correct the file named in the technical detail.",
            },
            ErrorKind::ReportNothingFromBootHealth => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-009",
                title: "Nothing to report from boot health",
                severity: Severity::Error,
                detection: "tur-boot-health status shows no pending notice, or cannot be read.",
                user_message: "There is no failed update or failed start to report.",
                explanation: "A report from boot health is made from its pending notice. There \
                              is none, or tur-boot-health could not be run.",
                recovery: "Describe the problem yourself with --title and --component instead.",
            },
            ErrorKind::ReportRecordsUnavailable => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-010",
                title: "Report list cannot be used",
                severity: Severity::Error,
                detection: "The directory for prepared and sent reports cannot be found, \
                            created, read or written.",
                user_message: "tur-diag cannot keep its list of problem reports.",
                explanation: "Prepared and sent reports are kept under $XDG_STATE_HOME/tur/\
                              reports (by default ~/.local/state/tur/reports).",
                recovery: "Check that the home directory is writable, or name a directory with \
                           --state-dir.",
            },
            ErrorKind::ReportUnknown => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-011",
                title: "No such sent report",
                severity: Severity::Error,
                detection: "No record of a sent report has the identifier given.",
                user_message: "There is no sent report with that identifier on this computer.",
                explanation: "Only reports sent from this account are listed.",
                recovery: "Run 'tur-diag report list' to see the identifiers.",
            },
            ErrorKind::ReportTooLarge => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-REPORT-012",
                title: "Report is too large",
                severity: Severity::Error,
                detection: "The prepared bundle is larger than 8 MiB, or expands to more than \
                            64 MiB.",
                user_message: "The problem report is larger than TUR's receiver accepts.",
                explanation: "Receivers accept bundles up to the limits given. A report collects \
                              less of the journal than 'tur-diag collect', so this is rare.",
                recovery: "Prepare the report again with --since-boot, or share a bundle made \
                           with 'tur-diag collect' yourself.",
            },
            ErrorKind::HealUnknown => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-001",
                title: "Unknown fault or remedy",
                severity: Severity::Error,
                detection: "The code or the remedy's name is not in the catalogue compiled into tur-diag.",
                user_message: "TUR does not know that problem or that repair.",
                explanation: "The catalogue is compiled into tur-diag; names are case-sensitive.",
                recovery: "Run 'tur-diag heal catalogue' to see every fault and its remedies.",
            },
            ErrorKind::HealNotPresent => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-002",
                title: "Problem not present",
                severity: Severity::Error,
                detection: "The fault's detection check passed when the remedy was asked for.",
                user_message: "That problem is not present now. Nothing was changed.",
                explanation: "A remedy is applied only while its fault's check finds the fault, so that a request cannot change a working system.",
                recovery: "Run 'tur-diag heal status' to see the problems TUR has found.",
            },
            ErrorKind::HealNeedsApproval => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-003",
                title: "Needs your approval as administrator",
                severity: Severity::Error,
                detection: "The remedy is class 2 (it cannot be undone) and the request came through the healing service's socket or from an account without administrator rights.",
                user_message: "This repair cannot be undone, so only you can apply it, with administrator rights. P@x cannot.",
                explanation: "Class 2 remedies are refused on the socket that P@x and ordinary accounts use (ADR-0027).",
                recovery: "Read what the repair changes with 'tur-diag heal explain CODE', then run 'sudo tur-diag heal apply CODE REMEDY' and confirm.",
            },
            ErrorKind::HealNotConfirmed => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-004",
                title: "Repair not confirmed",
                severity: Severity::Error,
                detection: "Neither --yes was given nor 'yes' typed at the question.",
                user_message: "The repair was not confirmed. Nothing was changed.",
                explanation: "A remedy that cannot be undone is applied only after the person confirms it.",
                recovery: "Run the command again and confirm, or give --yes after reading what it changes.",
            },
            ErrorKind::HealTooOften => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-005",
                title: "Tried too often",
                severity: Severity::Error,
                detection: "The remedy was applied as often in this boot as the catalogue or the request limit allows.",
                user_message: "This repair has already been tried several times since the computer started.",
                explanation: "Repeating a repair that does not hold does not help; the limit also stops a loop.",
                recovery: "Restart the computer, or send a report with 'tur-diag heal report CODE' if TUR offers one.",
            },
            ErrorKind::HealCheckUnavailable => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-006",
                title: "Check could not be made",
                severity: Severity::Error,
                detection: "The program a check runs is missing, failed, or printed something unexpected.",
                user_message: "TUR could not check for this problem, so it changed nothing.",
                explanation: "Checks run systemctl, timedatectl, df and coredumpctl; the detail names the one that failed.",
                recovery: "Run the command again as root; if it still fails, send a report with 'tur-diag report prepare'.",
            },
            ErrorKind::HealNothingToUndo => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-007",
                title: "Nothing to undo",
                severity: Severity::Error,
                detection: "No application of that remedy to that fault is recorded that has not been undone, the remedy changed nothing lasting, or it is irreversible, or putting back the kept copy failed.",
                user_message: "There is nothing to undo for that repair.",
                explanation: "The state file records each application and its undo.",
                recovery: "Run 'tur-diag heal status' to see what was applied.",
            },
            ErrorKind::HealNoReport => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-008",
                title: "No report offered",
                severity: Severity::Error,
                detection: "The fault is not in a state in which TUR offers a report (its cause is not in TUR, or it was repaired).",
                user_message: "TUR offers no report for this problem now.",
                explanation: "Reports are offered when a fault's cause is in TUR and nothing safe repairs it.",
                recovery: "You can still describe any problem yourself with 'tur-diag report prepare --title ... --component ...'.",
            },
            ErrorKind::HealStateUnavailable => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-009",
                title: "Self-healing state unavailable",
                severity: Severity::Error,
                detection: "The state file could not be read, parsed or written, or the data directory or its lock could not be used.",
                user_message: "TUR cannot keep its record of problems and repairs.",
                explanation: "The state is /var/log/tur/heal/state.json; copies and the lock are in /var/lib/tur/heal.",
                recovery: "Check the free space and that the directories exist; they are created by systemd-tmpfiles.",
            },
            ErrorKind::HealRequestInvalid => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-010",
                title: "Request not understood",
                severity: Severity::Error,
                detection: "The request was not one JSON object of the request format, or named a tool the service does not answer.",
                user_message: "The healing service did not understand the request. Nothing was changed.",
                explanation: "Requests are one line of JSON: schema_version, tool (heal_apply_remedy or heal_undo_remedy) and arguments (code, remedy).",
                recovery: "Use 'tur-diag heal apply' or 'tur-diag heal undo'.",
            },
            ErrorKind::HealServiceUnreachable => RegistryEntry {
                kind: self,
                code: "TUR-DIAG-HEAL-011",
                title: "Healing service not reachable",
                severity: Severity::Error,
                detection: "Connecting to /run/tur/heal.socket failed, or no answer came back.",
                user_message: "TUR's healing service did not answer. Nothing was changed.",
                explanation: "tur-heal.socket starts a service for each request.",
                recovery: "Check 'systemctl status tur-heal.socket', or run the command with administrator rights.",
            },
        }
    }

    /// The stable code as text, for example `TUR-DIAG-BUNDLE-007`.
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

/// An error reported by `tur-diag`: a registered kind plus what exactly went wrong.
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

    use tur_core::Family;

    use super::*;

    #[test]
    fn every_code_parses_and_belongs_to_the_diag_family() {
        for entry in registry() {
            let code: ErrorCode = entry
                .code
                .parse()
                .unwrap_or_else(|error| panic!("{} does not parse: {error}", entry.code));
            assert_eq!(code.family(), Family::Diag, "{}", entry.code);
            assert!(entry.code.starts_with("TUR-DIAG-"), "{}", entry.code);
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
            (ErrorKind::OutputWrite, "TUR-DIAG-COLLECT-001"),
            (ErrorKind::OutputExists, "TUR-DIAG-COLLECT-002"),
            (ErrorKind::BundleRead, "TUR-DIAG-BUNDLE-001"),
            (ErrorKind::ArchiveDamaged, "TUR-DIAG-BUNDLE-002"),
            (ErrorKind::UnsafeMember, "TUR-DIAG-BUNDLE-003"),
            (ErrorKind::MemberMissing, "TUR-DIAG-BUNDLE-004"),
            (ErrorKind::MetadataInvalid, "TUR-DIAG-BUNDLE-005"),
            (ErrorKind::SchemaUnsupported, "TUR-DIAG-BUNDLE-006"),
            (ErrorKind::ChecksumMismatch, "TUR-DIAG-BUNDLE-007"),
            (ErrorKind::FileListMismatch, "TUR-DIAG-BUNDLE-008"),
            (ErrorKind::BundleTooLarge, "TUR-DIAG-BUNDLE-009"),
            (ErrorKind::Internal, "TUR-DIAG-INTERNAL-001"),
            (ErrorKind::ReportInvalid, "TUR-DIAG-REPORT-001"),
            (ErrorKind::ReportNoReceiver, "TUR-DIAG-REPORT-002"),
            (ErrorKind::ReportWrongReceiver, "TUR-DIAG-REPORT-003"),
            (ErrorKind::ReportNotConfirmed, "TUR-DIAG-REPORT-004"),
            (ErrorKind::ReportNotPrepared, "TUR-DIAG-REPORT-005"),
            (ErrorKind::ReportUnreachable, "TUR-DIAG-REPORT-006"),
            (ErrorKind::ReportRefused, "TUR-DIAG-REPORT-007"),
            (ErrorKind::ReportConfigInvalid, "TUR-DIAG-REPORT-008"),
            (
                ErrorKind::ReportNothingFromBootHealth,
                "TUR-DIAG-REPORT-009",
            ),
            (ErrorKind::ReportRecordsUnavailable, "TUR-DIAG-REPORT-010"),
            (ErrorKind::ReportUnknown, "TUR-DIAG-REPORT-011"),
            (ErrorKind::ReportTooLarge, "TUR-DIAG-REPORT-012"),
            (ErrorKind::HealUnknown, "TUR-DIAG-HEAL-001"),
            (ErrorKind::HealNotPresent, "TUR-DIAG-HEAL-002"),
            (ErrorKind::HealNeedsApproval, "TUR-DIAG-HEAL-003"),
            (ErrorKind::HealNotConfirmed, "TUR-DIAG-HEAL-004"),
            (ErrorKind::HealTooOften, "TUR-DIAG-HEAL-005"),
            (ErrorKind::HealCheckUnavailable, "TUR-DIAG-HEAL-006"),
            (ErrorKind::HealNothingToUndo, "TUR-DIAG-HEAL-007"),
            (ErrorKind::HealNoReport, "TUR-DIAG-HEAL-008"),
            (ErrorKind::HealStateUnavailable, "TUR-DIAG-HEAL-009"),
            (ErrorKind::HealRequestInvalid, "TUR-DIAG-HEAL-010"),
            (ErrorKind::HealServiceUnreachable, "TUR-DIAG-HEAL-011"),
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
        assert_eq!(lines.next(), Some("TUR-DIAG-BUNDLE-007"));
        assert_eq!(lines.next(), Some(error.user_message()));
        assert!(text.contains("Action:"));
        assert!(text.contains("Technical detail:"));
        assert!(text.ends_with("system/kernel.txt: sha256 differs"));
        assert_eq!(error.kind(), ErrorKind::ChecksumMismatch);
        assert_eq!(error.detail(), "system/kernel.txt: sha256 differs");
        assert_eq!(error.code().to_string(), "TUR-DIAG-BUNDLE-007");
    }
}

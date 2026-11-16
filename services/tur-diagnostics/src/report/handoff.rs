// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The hand-over of a report from P@x to the person.
//!
//! P@x can prepare a report and show it (class 0), and never send one. When
//! P@x has previewed a report, the preview carries a [`Handoff`]: the one
//! command with which the person reads the same report in full and sends it
//! by typing `yes` (`tur-diag report submit`). The command collects the
//! logs again and shows everything before it asks, so what is sent is what
//! the person read, whatever P@x said about it.
//!
//! Two barriers keep P@x from sending through its command tool, where the
//! policy engine allows a read-only command whatever its text: its sandbox
//! has no network, and `tur-diag report send` and `submit` refuse to run in
//! it at all ([`in_pax_sandbox`]).

use serde::Serialize;

use super::{MAX_DESCRIPTION_CHARS, MAX_TITLE_CHARS, PreparedBy, ReportRequest};
use crate::host::Host;

/// The host name of P@x's read-only sandbox. `tur-pax` gives every sandboxed
/// command its own UTS namespace with this name (`--hostname`), and the
/// command cannot change it: it has no capabilities there. A test of this
/// component holds the two names together.
pub const PAX_SANDBOX_HOSTNAME: &str = "tur-pax-sandbox";

/// Where the kernel gives the host name of the current UTS namespace.
const HOSTNAME_FILE: &str = "/proc/sys/kernel/hostname";

/// Whether this process runs in P@x's read-only sandbox.
pub fn in_pax_sandbox(host: &dyn Host) -> bool {
    host.read_file(HOSTNAME_FILE, 256)
        .is_ok_and(|content| String::from_utf8_lossy(&content.data).trim() == PAX_SANDBOX_HOSTNAME)
}

/// The words a hand-over gives the person.
pub const HANDOFF_TEXT: &str = "Only you can send this report: P@x prepared it and cannot send \
     it. To read it in full and send it, run the command below in a terminal. It collects the \
     logs again, shows you everything that would be sent, and sends the report only if you type \
     yes. Nothing has been sent.";

/// How the person reads and sends a report P@x previewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handoff {
    /// Who sends: always `person`.
    pub by: String,
    /// The command, one argument per item.
    pub argv: Vec<String>,
    /// The same command, quoted for a shell.
    pub command: String,
    /// What to tell the person.
    pub text: String,
}

/// The hand-over for a report, from its `report.json` as it stands in the
/// bundle (after redaction). Only a report P@x prepared gets one.
pub fn handoff_for(report: &ReportRequest) -> Option<Handoff> {
    if report.prepared_by != PreparedBy::Pax {
        return None;
    }
    let title: String = report.title.chars().take(MAX_TITLE_CHARS).collect();
    let mut argv: Vec<String> = ["tur-diag", "report", "submit", "--prepared-by=pax"]
        .iter()
        .map(|word| (*word).to_owned())
        .collect();
    if report.boot_health.is_some() {
        argv.push("--from-boot-health".to_owned());
    }
    argv.push(format!("--kind={}", report.kind.as_str()));
    argv.push(format!("--title={title}"));
    argv.push(format!("--component={}", report.component));
    if let Some(code) = &report.error_code {
        argv.push(format!("--error-code={code}"));
    }
    if !report.description.trim().is_empty() {
        let description: String = report
            .description
            .chars()
            .take(MAX_DESCRIPTION_CHARS)
            .collect();
        argv.push(format!("--description={description}"));
    }
    if report.component == "pax" {
        argv.push("--include-pax-activity".to_owned());
    }
    let command = argv
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    Some(Handoff {
        by: "person".to_owned(),
        argv,
        command,
        text: HANDOFF_TEXT.to_owned(),
    })
}

/// `text` quoted for a POSIX shell: as it is when it has only characters a
/// shell never interprets, otherwise in single quotes.
pub fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./=:@%+,-".contains(c));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;
    use crate::report::{REPORT_SCHEMA_VERSION, ReportKind};

    fn report(by: PreparedBy) -> ReportRequest {
        ReportRequest {
            schema_version: REPORT_SCHEMA_VERSION,
            kind: ReportKind::Request,
            title: "No sound after resume".to_owned(),
            description: "It's silent; <user-1> restarted.".to_owned(),
            error_code: Some("TUR-AUDIO-RESUME-001".to_owned()),
            component: "audio".to_owned(),
            affected_version: None,
            prepared_by: by,
            boot_health: None,
        }
    }

    #[test]
    fn only_a_report_pax_prepared_is_handed_over() {
        assert_eq!(handoff_for(&report(PreparedBy::Person)), None);
        let handoff = handoff_for(&report(PreparedBy::Pax)).expect("a hand-over");
        assert_eq!(handoff.by, "person");
        assert_eq!(
            handoff.argv,
            [
                "tur-diag",
                "report",
                "submit",
                "--prepared-by=pax",
                "--kind=request",
                "--title=No sound after resume",
                "--component=audio",
                "--error-code=TUR-AUDIO-RESUME-001",
                "--description=It's silent; <user-1> restarted.",
            ]
        );
        // The person's command submits; it never sends without asking.
        assert!(!handoff.argv.iter().any(|argument| argument == "--yes"));
        assert!(!handoff.argv.iter().any(|argument| argument == "send"));
        assert_eq!(
            handoff.command,
            "tur-diag report submit --prepared-by=pax --kind=request \
             '--title=No sound after resume' --component=audio \
             --error-code=TUR-AUDIO-RESUME-001 \
             '--description=It'\\''s silent; <user-1> restarted.'"
        );
    }

    #[test]
    fn a_report_about_pax_brings_pax_activity() {
        let mut about_pax = report(PreparedBy::Pax);
        about_pax.component = "pax".to_owned();
        about_pax.description = String::new();
        let handoff = handoff_for(&about_pax).expect("a hand-over");
        assert_eq!(
            handoff.argv.last().map(String::as_str),
            Some("--include-pax-activity")
        );
        assert!(
            !handoff
                .argv
                .iter()
                .any(|argument| argument.starts_with("--description"))
        );
    }

    #[test]
    fn quoting_survives_a_shell() {
        assert_eq!(shell_quote("plain-word_1.0"), "plain-word_1.0");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("`x`"), "'`x`'");
    }

    #[test]
    fn the_sandbox_is_known_by_its_host_name() {
        let inside = FakeHost::new().with_file(HOSTNAME_FILE, "tur-pax-sandbox\n");
        assert!(in_pax_sandbox(&inside));
        let outside = FakeHost::new().with_file(HOSTNAME_FILE, "workstation-7\n");
        assert!(!in_pax_sandbox(&outside));
        assert!(!in_pax_sandbox(&FakeHost::new()));
    }
}

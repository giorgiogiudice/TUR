// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The summary that is filed: never the bundle, only a few facts and short
//! excerpts, every one of them passed through the receiver's own scrubber
//! after it has learnt from the whole bundle.
//!
//! What goes in:
//!
//! * fields that are checked against a fixed form and need no scrubbing:
//!   the signature, error code, component, version, kind, who prepared it,
//!   the architecture and the build identifier (anything that does not have
//!   the expected form is shown as `unknown`);
//! * scrubbed: the report's title and description, boot health's notice
//!   text, the kernel release, up to 20 lines of `system/failed-units.txt`,
//!   and the newest 15 journal messages of priority 3 (error) or more urgent
//!   from each of the current and the previous boot;
//! * counts: files, bytes, items not collected, and how many identifiers
//!   the client and the receiver replaced.
//!
//! Every text from the report is put inside a fenced code block, so that
//! nothing in it is rendered as Markdown: no mention notifies anyone, no
//! link or image is followed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tur_diag::bundle::{Member, REDACTION_REPORT_PATH};
use tur_diag::manifest::{Manifest, format_utc};
use tur_diag::report::{ReportRequest, is_version};

use crate::scrub::Scrubber;

/// Lines of failed units shown.
pub const MAX_FAILED_UNITS: usize = 20;
/// Journal messages shown per boot.
pub const MAX_JOURNAL_LINES: usize = 15;
/// The longest line shown, in characters, after scrubbing.
pub const MAX_LINE_CHARS: usize = 300;
/// The longest description shown in a comment, in characters.
pub const MAX_COMMENT_DESCRIPTION: usize = 1500;
/// The longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// What is filed for one report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// The issue's title, for the first report of a signature.
    pub title: String,
    /// The issue's body, for the first report of a signature.
    pub issue_body: String,
    /// The comment, for a later report of the same signature.
    pub comment_body: String,
    /// How many replacements the receiver's scrubber made.
    pub server_replacements: u64,
}

/// Facts about the report that the receiver decided.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The report's identifier.
    pub report_id: &'a str,
    /// When it was received.
    pub received_at: u64,
    /// When its bundle will be deleted.
    pub bundle_expires_at: u64,
    /// Its duplicate signature.
    pub signature: &'a str,
    /// The version it is about.
    pub version: &'a str,
}

fn member<'a>(members: &'a [Member], path: &str) -> Option<&'a [u8]> {
    members
        .iter()
        .find(|(name, _)| name == path)
        .map(|(_, data)| data.as_slice())
}

/// Cuts `text` to `max` characters.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(max).collect();
        cut.push('…');
        cut
    }
}

/// Makes text safe inside a fenced block of four backticks: no run of three
/// or more backticks remains, and no control character but line breaks.
fn fence_safe(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    while out.contains("```") {
        out = out.replace("```", "'''");
    }
    out
}

fn fenced(text: &str) -> String {
    format!("````text\n{}\n````\n", fence_safe(text.trim_end()))
}

/// Makes a value safe inside an inline code span in a table cell.
fn inline(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == '`' || c == '|' { '\'' } else { c })
        .collect()
}

/// A value checked against a form, or `unknown`.
fn checked(value: Option<&str>, valid: impl Fn(&str) -> bool) -> String {
    value
        .filter(|value| valid(value))
        .map_or_else(|| "unknown".to_owned(), str::to_owned)
}

fn is_architecture(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The message of a journal entry: a string, or an array of bytes.
fn journal_message(entry: &Value) -> Option<String> {
    match entry.get("MESSAGE")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(bytes) => {
            let bytes: Vec<u8> = bytes
                .iter()
                .filter_map(|byte| byte.as_u64().and_then(|b| u8::try_from(b).ok()))
                .collect();
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        _ => None,
    }
}

fn journal_priority(entry: &Value) -> Option<u64> {
    match entry.get("PRIORITY")? {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// The newest error messages of one journal file, oldest first.
fn journal_errors(data: Option<&[u8]>) -> Vec<String> {
    let Some(data) = data else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(data);
    let mut lines: Vec<String> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| journal_priority(entry).is_some_and(|priority| priority <= 3))
        .filter_map(|entry| {
            let message = journal_message(&entry)?;
            let source = ["SYSLOG_IDENTIFIER", "_SYSTEMD_UNIT", "_COMM"]
                .iter()
                .find_map(|key| entry.get(*key).and_then(Value::as_str))
                .unwrap_or("?");
            Some(format!("{source}: {}", message.replace('\n', " ")))
        })
        .collect();
    let skip = lines.len().saturating_sub(MAX_JOURNAL_LINES);
    lines.split_off(skip)
}

fn failed_units(data: Option<&[u8]>) -> Vec<String> {
    let Some(data) = data else {
        return Vec::new();
    };
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_FAILED_UNITS)
        .map(str::to_owned)
        .collect()
}

/// How many identifiers the client replaced, from `redaction-report.json`.
fn client_replacements(members: &[Member]) -> u64 {
    member(members, REDACTION_REPORT_PATH)
        .and_then(|data| serde_json::from_slice::<Value>(data).ok())
        .and_then(|report| {
            report.get("categories").and_then(Value::as_object).map(|categories| {
                categories
                    .values()
                    .filter_map(|category| category.get("replacements").and_then(Value::as_u64))
                    .sum()
            })
        })
        .unwrap_or(0)
}

/// Scrubs the lines as one text, so that a rule that spans lines (a
/// private key block) still applies, then cuts each line. Cutting after
/// scrubbing means a cut can never shorten a secret below what its rule
/// recognises.
fn scrub_lines(scrubber: &mut Scrubber, lines: &[String]) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    scrubber
        .scrub(&lines.join("\n"))
        .lines()
        .map(|line| cut(line, MAX_LINE_CHARS))
        .collect()
}

/// Builds the summary of a verified report.
pub fn build(
    manifest: &Manifest,
    report: &ReportRequest,
    members: &[Member],
    facts: &Facts<'_>,
) -> Summary {
    let mut scrubber = Scrubber::new();
    scrubber.learn(&report.title);
    scrubber.learn(&report.description);
    if let Some(evidence) = &report.boot_health {
        scrubber.learn(&evidence.text);
    }
    for (_, data) in members {
        scrubber.learn(&String::from_utf8_lossy(data));
    }

    let title = scrubber.scrub(&report.title);
    let description = scrubber.scrub(&report.description);
    let notice = report
        .boot_health
        .as_ref()
        .map(|evidence| scrubber.scrub(&evidence.text));
    let kernel = manifest
        .kernel_release
        .as_deref()
        .map_or_else(|| "unknown".to_owned(), |kernel| cut(&scrubber.scrub(kernel), 80));
    let units = scrub_lines(
        &mut scrubber,
        &failed_units(member(members, "system/failed-units.txt")),
    );
    let current = scrub_lines(
        &mut scrubber,
        &journal_errors(member(members, "boot/current-journal.jsonl")),
    );
    let previous = scrub_lines(
        &mut scrubber,
        &journal_errors(member(members, "boot/previous-journal.jsonl")),
    );

    let code = report.error_code.as_deref().unwrap_or("none");
    let architecture = checked(Some(manifest.architecture.as_str()), is_architecture);
    let build_id = checked(manifest.tur.build_id.as_deref(), is_version);
    let received = format_utc(facts.received_at);
    let expires = format_utc(facts.bundle_expires_at);
    let received_day = &received[..10];
    let expires_day = &expires[..10];

    let one_line_title: String = title
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            '@' => '＠',
            '`' => '\'',
            other => other,
        })
        .collect();
    let issue_title = cut(
        &format!(
            "[{code}] {} {}: {}",
            report.component,
            facts.version,
            one_line_title.trim()
        ),
        MAX_TITLE_CHARS,
    );

    let mut body = String::new();
    body.push_str(&format!(
        "<!-- tur-report-signature: {} -->\n",
        facts.signature
    ));
    body.push_str(
        "A problem report sent from a TUR computer with the person's consent. Personal data \
         was removed on the computer and again on the receiver. This is a summary; the bundle \
         itself is not here.\n\n",
    );
    body.push_str("| | |\n|---|---|\n");
    let rows = [
        ("Signature", format!("`{}` (error code, component, version)", inline(facts.signature))),
        ("Error code", format!("`{}`", inline(code))),
        ("Component", format!("`{}`", inline(&report.component))),
        ("Version", format!("`{}`", inline(facts.version))),
        ("Kind", format!("`{}`", report.kind.as_str())),
        ("Prepared by", format!("`{}`", report.prepared_by.as_str())),
        ("Architecture", format!("`{}`", inline(&architecture))),
        ("Kernel", format!("`{}`", inline(&kernel))),
        ("TUR build", format!("`{}`", inline(&build_id))),
        (
            "Report",
            format!("`{}`, received {received_day}", inline(facts.report_id)),
        ),
    ];
    for (name, value) in rows {
        body.push_str(&format!("| {name} | {value} |\n"));
    }
    body.push_str("\n### What happened\n\n");
    let mut what = title.clone();
    if !description.trim().is_empty() {
        what.push_str("\n\n");
        what.push_str(&description);
    }
    body.push_str(&fenced(&what));
    if let (Some(evidence), Some(notice)) = (&report.boot_health, &notice) {
        body.push_str("\n### Boot health\n\n");
        let mut text = format!(
            "notice: {}, code {}",
            evidence.notice_kind,
            evidence.error_code.as_deref().unwrap_or("none")
        );
        if let Some(check) = &evidence.failed_check {
            text.push_str(&format!(", first failed check {check}"));
        }
        if let Some(version) = &evidence.rejected_version {
            text.push_str(&format!(", rejected version {version}"));
        }
        text.push('\n');
        text.push_str(notice);
        body.push_str(&fenced(&text));
    }
    if !units.is_empty() {
        body.push_str("\n### Failed units\n\n");
        body.push_str(&fenced(&units.join("\n")));
    }
    for (heading, lines) in [
        ("Errors in the journal of this boot", &current),
        ("Errors in the journal of the previous boot", &previous),
    ] {
        if !lines.is_empty() {
            body.push_str(&format!(
                "\n### {heading} (newest {MAX_JOURNAL_LINES} at most, oldest first)\n\n"
            ));
            body.push_str(&fenced(&lines.join("\n")));
        }
    }
    let unpacked: u64 = members
        .iter()
        .map(|(_, data)| u64::try_from(data.len()).unwrap_or(u64::MAX))
        .sum();
    let server = scrubber.replacements();
    body.push_str(&format!(
        "\n### Bundle\n\n{} files, {unpacked} bytes unpacked; {} items could not be collected. \
         The computer replaced {} identifiers and the receiver {server} more. The bundle is kept \
         encrypted on the receiver as report `{}` until {expires_day}, then deleted.\n",
        members.len(),
        manifest.missing.len(),
        client_replacements(members),
        inline(facts.report_id),
    ));

    let mut comment = format!(
        "Another report with this signature: `{}`, received {received_day}; kind `{}`, \
         prepared by `{}`, build `{}`, architecture `{}`.\n\n",
        inline(facts.report_id),
        report.kind.as_str(),
        report.prepared_by.as_str(),
        inline(&build_id),
        inline(&architecture),
    );
    let mut what = title;
    if !description.trim().is_empty() {
        what.push_str("\n\n");
        what.push_str(&cut(&description, MAX_COMMENT_DESCRIPTION));
    }
    comment.push_str(&fenced(&what));
    if let Some(line) = current.last() {
        comment.push_str("\nNewest error in the journal of this boot:\n\n");
        comment.push_str(&fenced(line));
    }

    Summary {
        title: issue_title,
        issue_body: body,
        comment_body: comment,
        server_replacements: scrubber.replacements(),
    }
}

#[cfg(test)]
mod tests {
    use tur_diag::Profile;
    use tur_diag::report::{PreparedBy, REPORT_SCHEMA_VERSION, ReportKind};

    use super::*;
    use crate::testing;

    fn report(title: &str, description: &str) -> ReportRequest {
        ReportRequest {
            schema_version: REPORT_SCHEMA_VERSION,
            kind: ReportKind::Request,
            title: title.to_owned(),
            description: description.to_owned(),
            error_code: Some("TUR-AUDIO-RESUME-001".to_owned()),
            component: "audio".to_owned(),
            affected_version: None,
            prepared_by: PreparedBy::Person,
            boot_health: None,
        }
    }

    fn facts() -> Facts<'static> {
        Facts {
            report_id: "R-0123456789abcdef",
            received_at: 1_793_000_000,
            bundle_expires_at: 1_793_000_000 + 90 * 86_400,
            signature: "0011223344556677",
            version: "0.2.0",
        }
    }

    #[test]
    fn the_summary_has_the_facts_and_the_evidence_and_no_markdown_escapes() {
        let report = report(
            "No sound ```` @everyone",
            "It stopped.\n```\n![x](https://tracker.example/p.png)",
        );
        let members = testing::members(
            Some(&serde_json::to_string(&report).unwrap()),
            &[
                (
                    "boot/current-journal.jsonl",
                    "{\"MESSAGE\":\"Failed to start audio.service\",\"PRIORITY\":\"3\",\"SYSLOG_IDENTIFIER\":\"systemd\"}\n\
                     {\"MESSAGE\":\"just info\",\"PRIORITY\":\"6\"}\n\
                     {\"MESSAGE\":[104,105],\"PRIORITY\":2}\n",
                ),
                ("system/failed-units.txt", "audio.service loaded failed failed Audio\n"),
            ],
            Profile::Default,
        );
        let manifest = testing::manifest(&members);
        let summary = build(&manifest, &report, &members, &facts());
        assert!(summary.title.starts_with("[TUR-AUDIO-RESUME-001] audio 0.2.0: No sound"));
        assert!(!summary.title.contains('@'));
        assert!(!summary.title.contains('`'));
        let body = &summary.issue_body;
        assert!(body.contains("tur-report-signature: 0011223344556677"));
        assert!(body.contains("| Error code | `TUR-AUDIO-RESUME-001` |"));
        assert!(body.contains("| Architecture | `x86_64` |"));
        assert!(body.contains("systemd: Failed to start audio.service"));
        assert!(body.contains("?: hi"));
        assert!(!body.contains("just info"));
        assert!(body.contains("audio.service loaded failed failed Audio"));
        assert!(body.contains("until 2027-"));
        // The person's text cannot leave its code block.
        let fences = body.matches("````").count();
        assert_eq!(fences % 2, 0);
        for line in body.lines() {
            if line.contains("tracker.example") {
                assert!(!line.starts_with("````"));
            }
        }
        assert!(!body.contains("```\n!["));
        assert!(summary.comment_body.contains("Another report with this signature"));
        assert!(summary.comment_body.contains("R-0123456789abcdef"));
    }

    #[test]
    fn values_of_the_wrong_form_are_shown_as_unknown() {
        let report = report("t", "");
        let members = testing::members(Some(&serde_json::to_string(&report).unwrap()), &[], Profile::Default);
        let mut manifest = testing::manifest(&members);
        manifest.architecture = "x86_64 | evil `code`".to_owned();
        manifest.tur.build_id = Some("../../etc".to_owned());
        let summary = build(&manifest, &report, &members, &facts());
        assert!(summary.issue_body.contains("| Architecture | `unknown` |"));
        assert!(summary.issue_body.contains("| TUR build | `unknown` |"));
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Plain-text reports for the command line.
//!
//! The text is for people. Programs should use the JSON output instead; the
//! wording here may change.

use std::fmt::Write;

use crate::catalogue::Statistics;
use crate::model::{Entry, Rating, Record, Reporter};
use crate::route::{Outlook, RatingSource, RouteOption, Severity};
use crate::search::SearchHit;

/// One line summarising a valid catalogue.
pub fn render_statistics(statistics: &Statistics) -> String {
    format!(
        "valid: {} applications, {} editions, {} compatibility records ({} rated by external sources, {} rated by TUR tests, {} unknown)",
        statistics.applications,
        statistics.editions,
        statistics.records,
        statistics.rated_by_external_sources,
        statistics.rated_by_tur_tests,
        statistics.unknown
    )
}

/// The hits of a search, one per line, best first.
pub fn render_hits(hits: &[SearchHit<'_>]) -> String {
    let mut out = String::new();
    for hit in hits {
        let entry = hit.entry;
        let _ = write!(
            out,
            "{}  {} ({}, {})",
            entry.id,
            entry.name,
            entry.category.as_str(),
            entry.publisher
        );
        if hit.matched != entry.name {
            let _ = write!(out, "  [matched \"{}\"]", hit.matched);
        }
        out.push('\n');
    }
    out
}

/// A rating and where it comes from, in one line. The wording always says
/// who reported it, and says so plainly when it was not TUR.
pub fn describe_rating(rating: Rating, source: &RatingSource) -> String {
    match source {
        RatingSource::Unrated => {
            "unknown (the catalogue holds no report; TUR has not tested it)".to_owned()
        }
        RatingSource::External {
            name,
            url,
            source_rating,
            version_tested,
            date,
        } => {
            let mut text = format!("{}, reported by {name}", rating.as_str());
            if let Some(words) = source_rating {
                let _ = write!(text, " as \"{words}\"");
            }
            if let Some(version) = version_tested {
                let _ = write!(text, " for version {version}");
            }
            let _ = write!(text, ", checked {date}; not tested by TUR ({url})");
            text
        }
        RatingSource::TurTest {
            report,
            version_tested,
            date,
        } => {
            let mut text = format!("{}, tested by TUR on {date}", rating.as_str());
            if let Some(version) = version_tested {
                let _ = write!(text, " with version {version}");
            }
            let _ = write!(text, " ({report})");
            text
        }
    }
}

fn describe_record(record: &Record) -> String {
    if record.reported_by == Reporter::TurAutopilot {
        let mut text = format!(
            "not a rating: passed TUR's compatibility autopilot check on {} with {}",
            record.date,
            record
                .runtime_version
                .as_deref()
                .unwrap_or("an unstated runtime")
        );
        if let Some(version) = &record.version_tested {
            let _ = write!(text, " for version {version}");
        }
        let _ = write!(text, " and these settings: {}", record.remedies.join(", "));
        return text;
    }
    let source = match record.reported_by {
        Reporter::Nobody | Reporter::TurAutopilot => RatingSource::Unrated,
        Reporter::External => RatingSource::External {
            name: record.source_name.clone().unwrap_or_default(),
            url: record.source_url.clone().unwrap_or_default(),
            source_rating: record.source_rating.clone(),
            version_tested: record.version_tested.clone(),
            date: record.date.clone(),
        },
        Reporter::TurTest => RatingSource::TurTest {
            report: record.test_report.clone().unwrap_or_default(),
            version_tested: record.version_tested.clone(),
            date: record.date.clone(),
        },
    };
    describe_rating(record.rating, &source)
}

/// Everything the catalogue holds about one application.
pub fn render_entry(entry: &Entry) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} ({})", entry.name, entry.id);
    let _ = writeln!(out, "Publisher: {}", entry.publisher);
    let _ = writeln!(out, "Category: {}", entry.category.as_str());
    if !entry.aliases.is_empty() {
        let _ = writeln!(out, "Also known as: {}", entry.aliases.join(", "));
    }
    if let Some(summary) = &entry.summary {
        let _ = writeln!(out, "Summary: {summary}");
    }
    if let Some(homepage) = &entry.homepage {
        let _ = writeln!(out, "Homepage: {homepage}");
    }
    for (index, edition) in entry.editions.iter().enumerate() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}. {}", index + 1, edition.describe());
        if let Some(licence) = edition.licence {
            let _ = writeln!(out, "   Licence: {}", licence.as_str());
        }
        if let Some(packaging) = edition.packaging {
            let _ = writeln!(out, "   Packaged by: {}", packaging.as_str());
        }
        if edition.primary {
            let _ = writeln!(out, "   The publisher's primary form of the application.");
        }
        if !edition.requirements.is_empty() {
            let labels: Vec<&str> = edition
                .requirements
                .iter()
                .map(|requirement| requirement.label())
                .collect();
            let _ = writeln!(out, "   Requires: {}", labels.join("; "));
        }
        if let Some(installer) = &edition.installer {
            let _ = writeln!(
                out,
                "   Installer: version {}, {} bytes, from {} (SHA-256 {}; licence {}; checked {})",
                installer.version,
                installer.size,
                installer.url,
                installer.sha256,
                installer.licence,
                installer.checked
            );
        }
        if let Some(notes) = &edition.notes {
            let _ = writeln!(out, "   Notes: {notes}");
        }
        if edition.records.is_empty() {
            let _ = writeln!(out, "   Compatibility: no record; every route is unknown.");
        }
        for record in &edition.records {
            let _ = writeln!(
                out,
                "   Compatibility, {}: {}",
                record.route.as_str(),
                describe_record(record)
            );
            if let Some(notes) = &record.notes {
                let _ = writeln!(out, "      {notes}");
            }
        }
        for reference in &edition.references {
            let _ = writeln!(
                out,
                "   Checked {}: {} ({})",
                reference.checked, reference.title, reference.url
            );
        }
    }
    out
}

fn outlook_text(outlook: Outlook) -> &'static str {
    match outlook {
        Outlook::Clear => "nothing known stands in the way",
        Outlook::Doubtful => "doubtful: see the caveats",
        Outlook::Blocked => "cannot work on this machine",
    }
}

fn severity_text(severity: Severity) -> &'static str {
    match severity {
        Severity::Note => "note",
        Severity::Warning => "warning",
        Severity::Obstacle => "obstacle",
        Severity::Blocker => "blocker",
    }
}

/// The routes for one application, in ladder order, each with its rating,
/// reason, caveats and prerequisites.
pub fn render_routes(entry: &Entry, options: &[RouteOption]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} ({}), category {}",
        entry.name,
        entry.id,
        entry.category.as_str()
    );
    for (index, option) in options.iter().enumerate() {
        let _ = writeln!(out);
        let mut marks = String::new();
        if option.recommended {
            marks.push_str("  [recommended]");
        }
        if option.pinned {
            marks.push_str("  [pinned by you]");
        }
        let _ = writeln!(out, "{}. {}{marks}", index + 1, option.route.label());
        if let Some(edition) = &option.edition {
            let _ = writeln!(out, "   Runs: {edition}");
            let _ = writeln!(
                out,
                "   Rating: {}",
                describe_rating(option.rating, &option.rating_source)
            );
            let _ = writeln!(out, "   Outlook: {}", outlook_text(option.outlook));
        }
        if let Some(settings) = &option.settings {
            let _ = writeln!(
                out,
                "   Works with these settings (TUR's autopilot, {} with {}; not a rating): {}",
                settings.date,
                settings.runtime_version,
                settings.remedies.join(", ")
            );
        }
        let _ = writeln!(out, "   Why: {}", option.reason);
        if let Some(alternative) = &option.alternative {
            let _ = writeln!(out, "   Least-bad alternative: {}", alternative.text);
        }
        if !option.caveats.is_empty() {
            let _ = writeln!(out, "   Before you choose:");
            for caveat in &option.caveats {
                let _ = writeln!(
                    out,
                    "     - [{}] {}",
                    severity_text(caveat.severity),
                    caveat.text
                );
            }
        }
        if !option.prerequisites.is_empty() {
            let _ = writeln!(out, "   Needs first:");
            for prerequisite in &option.prerequisites {
                let _ = writeln!(out, "     - {}", prerequisite.text);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{HostFacts, Preferences};
    use crate::model::parse_entry;
    use crate::route::{RouteRequest, route};
    use crate::search::search;

    const ENTRY: &str = r#"
schema_version = 1
id = "org.example.suite"
name = "Example Suite"
aliases = ["ExSuite"]
publisher = "Example Corp"
category = "productivity"
summary = "An office suite."
homepage = "https://example.org/"

[[editions]]
kind = "windows"
obtain = "publisher-page"
download_page = "https://example.org/download"
licence = "proprietary"
requirements = ["online-account"]

[[editions.records]]
route = "wine"
rating = "broken"
reported_by = "external"
source_name = "WineHQ AppDB"
source_url = "https://appdb.winehq.org/objectManager.php?sClass=application&iId=1"
source_rating = "Garbage"
version_tested = "2026"
date = "2026-11-12"
notes = "Installer fails."

[[editions.references]]
title = "Download page"
url = "https://example.org/download"
checked = "2026-11-12"

[[editions]]
kind = "web"
url = "https://app.example.org/"
"#;

    #[test]
    fn an_external_rating_is_never_described_as_tur_s_own() {
        let source = RatingSource::External {
            name: "ProtonDB".to_owned(),
            url: "https://www.protondb.com/app/1".to_owned(),
            source_rating: Some("gold".to_owned()),
            version_tested: None,
            date: "2026-11-12".to_owned(),
        };
        let text = describe_rating(Rating::Usable, &source);
        assert_eq!(
            text,
            "usable, reported by ProtonDB as \"gold\", checked 2026-11-12; not tested by TUR (https://www.protondb.com/app/1)"
        );
        let unrated = describe_rating(Rating::Unknown, &RatingSource::Unrated);
        assert!(unrated.starts_with("unknown"));
        assert!(unrated.contains("TUR has not tested it"));
        let own = RatingSource::TurTest {
            report: "docs/reports/compat/x.md".to_owned(),
            version_tested: Some("1.2".to_owned()),
            date: "2026-11-12".to_owned(),
        };
        assert_eq!(
            describe_rating(Rating::Good, &own),
            "good, tested by TUR on 2026-11-12 with version 1.2 (docs/reports/compat/x.md)"
        );
    }

    #[test]
    fn show_lists_editions_requirements_records_and_references() {
        let entry = parse_entry(ENTRY).unwrap();
        let text = render_entry(&entry);
        for expected in [
            "Example Suite (org.example.suite)",
            "Publisher: Example Corp",
            "Category: productivity",
            "Also known as: ExSuite",
            "1. Windows edition: the publisher's page https://example.org/download",
            "Licence: proprietary",
            "Requires: an online account",
            "Compatibility, wine: broken, reported by WineHQ AppDB as \"Garbage\" for version 2026, checked 2026-11-12; not tested by TUR",
            "Installer fails.",
            "Checked 2026-11-12: Download page (https://example.org/download)",
            "2. Web edition: https://app.example.org/",
            "Compatibility: no record; every route is unknown.",
        ] {
            assert!(
                text.contains(expected),
                "missing \"{expected}\" in:\n{text}"
            );
        }
    }

    #[test]
    fn route_report_shows_rating_source_reason_caveats_and_prerequisites() {
        let entry = parse_entry(ENTRY).unwrap();
        let options = route(
            &RouteRequest::new(&entry),
            &HostFacts::default(),
            &Preferences::default(),
        );
        let text = render_routes(&entry, &options);
        for expected in [
            "Example Suite (org.example.suite), category productivity",
            "1. Wine\n",
            "   Rating: broken, reported by WineHQ AppDB as \"Garbage\"",
            "not tested by TUR",
            "   Outlook: doubtful: see the caveats",
            "2. Windows guest (virtual machine)  [recommended]\n",
            "     - [warning] Needs about 16 GB of memory while the Windows guest runs.",
            "   Needs first:",
            "     - Wine must be installed.",
            "     - You must supply a Windows licence: TUR does not include one.",
            "3. Web application\n",
            "     - [note] Needs an online account with its publisher.",
        ] {
            assert!(
                text.contains(expected),
                "missing \"{expected}\" in:\n{text}"
            );
        }
    }

    #[test]
    fn hits_show_the_alias_that_matched() {
        let entries = vec![parse_entry(ENTRY).unwrap()];
        let text = render_hits(&search(&entries, "exsuite"));
        assert_eq!(
            text,
            "org.example.suite  Example Suite (productivity, Example Corp)  [matched \"ExSuite\"]\n"
        );
        let text = render_hits(&search(&entries, "example suite"));
        assert_eq!(
            text,
            "org.example.suite  Example Suite (productivity, Example Corp)\n"
        );
    }

    #[test]
    fn statistics_line_counts_what_was_and_was_not_rated() {
        let statistics = Statistics {
            applications: 2,
            editions: 5,
            records: 4,
            rated_by_external_sources: 3,
            rated_by_tur_tests: 0,
            unknown: 1,
        };
        assert_eq!(
            render_statistics(&statistics),
            "valid: 2 applications, 5 editions, 4 compatibility records (3 rated by external sources, 0 rated by TUR tests, 1 unknown)"
        );
    }
}

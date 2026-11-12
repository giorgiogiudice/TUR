// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Loading a catalogue directory.
//!
//! A catalogue is one flat directory with one TOML file per application,
//! named `<application ID>.toml`. A `README.md` and files whose name starts
//! with a dot are ignored; anything else in the directory is an error, so a
//! misplaced file cannot go unnoticed.
//!
//! Loading validates. A catalogue value therefore always holds valid
//! entries, and every caller (search, the router, the command line) can rely
//! on that.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Problem};
use crate::date::Date;
use crate::model::{Entry, Rating, Reporter, parse_entry};
use crate::validate::validate_catalogue;

/// The largest entry file, in bytes.
pub const MAX_ENTRY_BYTES: usize = 256 * 1024;

/// The most entries a catalogue may hold.
pub const MAX_ENTRIES: usize = 20_000;

/// A validated catalogue, ordered by application ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    entries: Vec<Entry>,
}

/// Counts over a catalogue, for the `validate` report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Statistics {
    /// Applications and application classes.
    pub applications: usize,
    /// Editions over all applications.
    pub editions: usize,
    /// Compatibility records over all editions.
    pub records: usize,
    /// Records that carry a rating reported by an external source.
    pub rated_by_external_sources: usize,
    /// Records that carry a rating from TUR's own tests.
    pub rated_by_tur_tests: usize,
    /// Records whose rating is unknown.
    pub unknown: usize,
}

impl Catalogue {
    /// Loads and validates every entry in `directory`.
    ///
    /// `today` is the day records must not be dated after. On failure every
    /// problem found is returned, not only the first.
    pub fn load(directory: &Path, today: Date) -> Result<Catalogue, Vec<Problem>> {
        let listing = std::fs::read_dir(directory).map_err(|error| {
            vec![Problem::general(
                Code::Unreadable,
                format!(
                    "cannot read the catalogue directory {}: {error}",
                    directory.display()
                ),
            )]
        })?;

        let mut problems = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for item in listing {
            let path = match item {
                Ok(item) => item.path(),
                Err(error) => {
                    problems.push(Problem::general(
                        Code::Unreadable,
                        format!("cannot list {}: {error}", directory.display()),
                    ));
                    continue;
                }
            };
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                problems.push(Problem::general(
                    Code::EntryInvalid,
                    "a file name in the catalogue is not valid UTF-8",
                ));
                continue;
            };
            let name = name.to_owned();
            if name.starts_with('.') || name == "README.md" {
                continue;
            }
            if path.is_dir() {
                problems.push(Problem::in_file(
                    Code::EntryInvalid,
                    &name,
                    "is a directory; the catalogue is one flat directory of .toml files",
                ));
            } else if !name.ends_with(".toml") {
                problems.push(Problem::in_file(
                    Code::EntryInvalid,
                    &name,
                    "is not a .toml file; the catalogue holds one .toml file per application",
                ));
            } else {
                files.push((name, path));
            }
        }
        files.sort();
        if files.len() > MAX_ENTRIES {
            problems.push(Problem::general(
                Code::LimitExceeded,
                format!("the catalogue holds more than {MAX_ENTRIES} entries"),
            ));
            return Err(problems);
        }
        if files.is_empty() && problems.is_empty() {
            problems.push(Problem::general(
                Code::Unreadable,
                format!("{} holds no catalogue entries", directory.display()),
            ));
        }

        let mut entries: Vec<(String, Entry)> = Vec::new();
        for (name, path) in files {
            let text = match read_entry_file(&path) {
                Ok(text) => text,
                Err((code, message)) => {
                    problems.push(Problem::in_file(code, &name, message));
                    continue;
                }
            };
            match parse_entry(&text) {
                Ok(entry) => entries.push((name, entry)),
                Err(message) => problems.push(Problem::in_file(Code::EntrySyntax, &name, message)),
            }
        }
        problems.extend(validate_catalogue(&entries, today));
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Catalogue::sorted(
            entries.into_iter().map(|(_, entry)| entry).collect(),
        ))
    }

    /// Builds a catalogue from entries already in memory, validating them
    /// as [`Catalogue::load`] does. Each element is the file name the entry
    /// would have and the entry.
    pub fn from_entries(
        entries: Vec<(String, Entry)>,
        today: Date,
    ) -> Result<Catalogue, Vec<Problem>> {
        let problems = validate_catalogue(&entries, today);
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Catalogue::sorted(
            entries.into_iter().map(|(_, entry)| entry).collect(),
        ))
    }

    fn sorted(mut entries: Vec<Entry>) -> Catalogue {
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Catalogue { entries }
    }

    /// The entries, ordered by application ID.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with the application ID `id`.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalogue holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Counts applications, editions and records.
    pub fn statistics(&self) -> Statistics {
        let mut statistics = Statistics {
            applications: self.entries.len(),
            editions: 0,
            records: 0,
            rated_by_external_sources: 0,
            rated_by_tur_tests: 0,
            unknown: 0,
        };
        for edition in self.entries.iter().flat_map(|entry| entry.editions.iter()) {
            statistics.editions += 1;
            for record in &edition.records {
                statistics.records += 1;
                if record.rating == Rating::Unknown {
                    statistics.unknown += 1;
                } else if record.reported_by == Reporter::TurTest {
                    statistics.rated_by_tur_tests += 1;
                } else {
                    statistics.rated_by_external_sources += 1;
                }
            }
        }
        statistics
    }

    /// Checks that every record claiming a TUR test points at a report file
    /// that exists under `repository_root`.
    ///
    /// The validator has already checked that the reference is present and
    /// is a path under `docs/reports/` or `tests/`; this checks the file.
    pub fn missing_test_reports(&self, repository_root: &Path) -> Vec<Problem> {
        let mut problems = Vec::new();
        for entry in &self.entries {
            let records = entry
                .editions
                .iter()
                .flat_map(|edition| edition.records.iter())
                .filter(|record| record.reported_by == Reporter::TurTest);
            for record in records {
                let Some(report) = &record.test_report else {
                    continue;
                };
                if !repository_root.join(report).is_file() {
                    problems.push(Problem::in_file(
                        Code::UnsourcedRating,
                        &format!("{}.toml", entry.id),
                        format!(
                            "a {} record claims TUR's own test, and its report {report} does not exist in the repository",
                            record.route.as_str()
                        ),
                    ));
                }
            }
        }
        problems
    }
}

/// Reads an entry file, refusing one larger than an entry may be before
/// reading all of it.
fn read_entry_file(path: &Path) -> Result<String, (Code, String)> {
    let limit = u64::try_from(MAX_ENTRY_BYTES).unwrap_or(u64::MAX);
    let file =
        File::open(path).map_err(|error| (Code::Unreadable, format!("cannot be read: {error}")))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| (Code::Unreadable, format!("cannot be read: {error}")))?;
    if bytes.len() > MAX_ENTRY_BYTES {
        return Err((
            Code::LimitExceeded,
            format!("is larger than the {MAX_ENTRY_BYTES} bytes an entry may be"),
        ));
    }
    String::from_utf8(bytes).map_err(|_| (Code::EntrySyntax, "is not valid UTF-8".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// A scratch directory removed on drop.
    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let process = std::process::id();
            let path =
                std::env::temp_dir().join(format!("tur-catalogue-{process}-{unique}-{label}"));
            std::fs::create_dir_all(&path).unwrap();
            Scratch { path }
        }

        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.path.join(name), text).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn today() -> Date {
        Date::parse("2026-11-12").unwrap()
    }

    fn entry_text(id: &str, name: &str) -> String {
        format!(
            "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\npublisher = \"Example\"\ncategory = \"utility\"\n\n[[editions]]\nkind = \"web\"\nurl = \"https://{id}.example.org/\"\n"
        )
    }

    fn codes(result: Result<Catalogue, Vec<Problem>>) -> Vec<Code> {
        result
            .unwrap_err()
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn loads_a_directory_in_application_id_order() {
        let scratch = Scratch::new("load");
        scratch.write("org.example.b.toml", &entry_text("org.example.b", "Beta"));
        scratch.write("org.example.a.toml", &entry_text("org.example.a", "Alpha"));
        scratch.write("README.md", "How entries are written.");
        scratch.write(".hidden", "ignored");
        let catalogue = Catalogue::load(&scratch.path, today()).unwrap();
        let ids: Vec<&str> = catalogue
            .entries()
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, ["org.example.a", "org.example.b"]);
        assert_eq!(catalogue.len(), 2);
        assert!(!catalogue.is_empty());
        assert_eq!(catalogue.get("org.example.b").unwrap().name, "Beta");
        assert!(catalogue.get("org.example.c").is_none());
        let statistics = catalogue.statistics();
        assert_eq!(statistics.applications, 2);
        assert_eq!(statistics.editions, 2);
        assert_eq!(statistics.records, 0);
    }

    #[test]
    fn a_missing_or_empty_directory_is_unreadable() {
        let scratch = Scratch::new("empty");
        assert_eq!(
            codes(Catalogue::load(&scratch.path.join("absent"), today())),
            [Code::Unreadable]
        );
        scratch.write("README.md", "Nothing else.");
        assert_eq!(
            codes(Catalogue::load(&scratch.path, today())),
            [Code::Unreadable]
        );
    }

    #[test]
    fn a_file_that_is_not_an_entry_is_an_error() {
        let scratch = Scratch::new("stray");
        scratch.write("org.example.a.toml", &entry_text("org.example.a", "Alpha"));
        scratch.write("notes.txt", "stray");
        std::fs::create_dir_all(scratch.path.join("games")).unwrap();
        let problems = Catalogue::load(&scratch.path, today()).unwrap_err();
        let mut found: Vec<(Code, Option<String>)> = problems
            .into_iter()
            .map(|problem| (problem.code, problem.file))
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                (Code::EntryInvalid, Some("games".to_owned())),
                (Code::EntryInvalid, Some("notes.txt".to_owned())),
            ]
        );
    }

    #[test]
    fn reports_every_bad_entry_with_its_file_and_code() {
        let scratch = Scratch::new("bad");
        scratch.write("org.example.a.toml", &entry_text("org.example.a", "Alpha"));
        scratch.write("org.example.syntax.toml", "this is not TOML");
        scratch.write(
            "org.example.field.toml",
            &entry_text("org.example.field", "Field").replace("publisher", "colour = 1\npublisher"),
        );
        scratch.write(
            "org.example.other.toml",
            &entry_text("org.example.renamed", "Renamed"),
        );
        scratch.write(
            "org.example.url.toml",
            &entry_text("org.example.url", "Url").replace("https://", "http://"),
        );
        scratch.write(
            "org.example.twin.toml",
            &entry_text("org.example.twin", "ALPHA"),
        );
        std::fs::write(
            scratch.path.join("org.example.bytes.toml"),
            [0xffu8, 0xfe, 0x00],
        )
        .unwrap();
        std::fs::write(
            scratch.path.join("org.example.big.toml"),
            vec![b'#'; MAX_ENTRY_BYTES + 1],
        )
        .unwrap();

        let problems = Catalogue::load(&scratch.path, today()).unwrap_err();
        let mut found: Vec<(String, Code)> = problems
            .into_iter()
            .map(|problem| (problem.file.unwrap(), problem.code))
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                ("org.example.big.toml".to_owned(), Code::LimitExceeded),
                ("org.example.bytes.toml".to_owned(), Code::EntrySyntax),
                ("org.example.field.toml".to_owned(), Code::EntrySyntax),
                ("org.example.other.toml".to_owned(), Code::EntryInvalid),
                ("org.example.syntax.toml".to_owned(), Code::EntrySyntax),
                ("org.example.twin.toml".to_owned(), Code::AliasCollision),
                ("org.example.url.toml".to_owned(), Code::EntryInvalid),
            ]
        );
    }

    #[test]
    fn a_tur_test_must_point_at_a_report_that_exists() {
        let scratch = Scratch::new("reports");
        let text = format!(
            "{}\n[[editions.records]]\nroute = \"web\"\nrating = \"good\"\nreported_by = \"tur-test\"\ntest_report = \"docs/reports/compat/alpha.md\"\nversion_tested = \"1.0\"\ndate = \"2026-11-12\"\n",
            entry_text("org.example.a", "Alpha")
        );
        let catalogue_dir = scratch.path.join("catalogue");
        std::fs::create_dir_all(&catalogue_dir).unwrap();
        std::fs::write(catalogue_dir.join("org.example.a.toml"), text).unwrap();
        let catalogue = Catalogue::load(&catalogue_dir, today()).unwrap();
        assert_eq!(catalogue.statistics().rated_by_tur_tests, 1);

        let missing = catalogue.missing_test_reports(&scratch.path);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].code, Code::UnsourcedRating);
        assert_eq!(missing[0].file.as_deref(), Some("org.example.a.toml"));
        assert!(missing[0].message.contains("docs/reports/compat/alpha.md"));

        let reports = scratch.path.join("docs/reports/compat");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(reports.join("alpha.md"), "# Test report").unwrap();
        assert!(catalogue.missing_test_reports(&scratch.path).is_empty());
    }

    #[test]
    fn from_entries_validates_like_load() {
        let good = parse_entry(&entry_text("org.example.a", "Alpha")).unwrap();
        let catalogue = Catalogue::from_entries(
            vec![("org.example.a.toml".to_owned(), good.clone())],
            today(),
        )
        .unwrap();
        assert_eq!(catalogue.len(), 1);

        let mut bad = good;
        bad.schema_version = 9;
        let result = Catalogue::from_entries(vec![("org.example.a.toml".to_owned(), bad)], today());
        assert_eq!(codes(result), [Code::EntryInvalid]);
    }
}

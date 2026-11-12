// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Validation of catalogue entries.
//!
//! Parsing (in [`crate::model`]) refuses unknown fields and wrong types.
//! This module checks everything else: identifiers, text, URLs, dates, which
//! fields an edition of each kind may carry, that a compatibility record
//! names a route its edition can use, and the honesty rule.
//!
//! # The honesty rule
//!
//! A rating other than `unknown` may be entered only if it is attributed:
//!
//! * to a named external source, with the URL that was checked and the
//!   source's own rating in its own words; or
//! * to TUR's own test, with the path of the test report in the repository.
//!
//! A record that claims a TUR test without a report reference is refused
//! with [`Code::UnsourcedRating`]. Whether the report file exists is checked
//! separately, by [`crate::catalogue::Catalogue::missing_test_reports`],
//! because that needs the repository.

use std::collections::{BTreeMap, BTreeSet};

use crate::codes::{Code, Problem};
use crate::date::{Date, MIN_YEAR};
use crate::model::{
    AndroidStore, Edition, EditionKind, Entry, EntryKind, Obtain, Rating, Record, Reference,
    Reporter, SCHEMA_VERSION,
};
use crate::search::normalise;
use crate::url::{check_https_url, is_direct_download};

/// The longest application ID, in bytes.
pub const MAX_ID_BYTES: usize = 255;
/// The longest name, alias, publisher, source name or reference title, in
/// characters.
pub const MAX_NAME_CHARS: usize = 200;
/// The longest summary, in characters.
pub const MAX_SUMMARY_CHARS: usize = 300;
/// The longest free-text note, in characters.
pub const MAX_NOTES_CHARS: usize = 1000;
/// The longest version or source-rating text, in characters.
pub const MAX_SHORT_CHARS: usize = 100;
/// The most aliases an entry may have.
pub const MAX_ALIASES: usize = 32;
/// The most editions an entry may have.
pub const MAX_EDITIONS: usize = 16;
/// The most records or references an edition may have.
pub const MAX_RECORDS: usize = 32;

const FLATPAK_ONLY: &[EditionKind] = &[EditionKind::Flatpak];
const LINUX_PACKAGES: &[EditionKind] = &[EditionKind::Flatpak, EditionKind::LinuxNative];
const OBTAINED: &[EditionKind] = &[
    EditionKind::LinuxNative,
    EditionKind::Windows,
    EditionKind::Macos,
];
const WITH_PACKAGE: &[EditionKind] = &[EditionKind::LinuxNative, EditionKind::Android];
const LINUX_NATIVE_ONLY: &[EditionKind] = &[EditionKind::LinuxNative];
const WITH_DOWNLOAD_PAGE: &[EditionKind] = &[
    EditionKind::LinuxNative,
    EditionKind::Windows,
    EditionKind::Macos,
    EditionKind::Android,
];
const ANDROID_ONLY: &[EditionKind] = &[EditionKind::Android];
const WEB_ONLY: &[EditionKind] = &[EditionKind::Web];
const WINDOWS_ONLY: &[EditionKind] = &[EditionKind::Windows];

/// The largest installer an entry may pin, in bytes: 4 GiB.
pub const MAX_INSTALLER_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The file endings a pinned installer may have: a program, a Windows
/// Installer package, or an archive of a portable application.
const INSTALLER_ENDINGS: [&str; 3] = [".exe", ".msi", ".zip"];

/// The findings of one entry: a code and a message each.
struct Findings {
    list: Vec<(Code, String)>,
}

impl Findings {
    fn add(&mut self, code: Code, message: impl Into<String>) {
        self.list.push((code, message.into()));
    }

    fn invalid(&mut self, message: impl Into<String>) {
        self.add(Code::EntryInvalid, message);
    }
}

/// Validates one entry on its own.
///
/// `today` is the day records and references must not be dated after; one
/// day of slack is allowed for time zones. Returns every problem found, each
/// with its code; an empty list means the entry is valid.
pub fn validate_entry(entry: &Entry, today: Date) -> Vec<(Code, String)> {
    let mut findings = Findings { list: Vec::new() };
    let class = entry.entry_kind == EntryKind::Class;

    if entry.schema_version != SCHEMA_VERSION {
        findings.invalid(format!(
            "schema_version is {}; this reader knows version {SCHEMA_VERSION} only",
            entry.schema_version
        ));
    }
    if !is_application_id(&entry.id) {
        findings.invalid(format!(
            "id \"{}\" is not a lower-case, dot-separated identifier of at least two segments",
            printable(&entry.id)
        ));
    }
    check_text(&mut findings, "name", &entry.name, MAX_NAME_CHARS);
    check_text(&mut findings, "publisher", &entry.publisher, MAX_NAME_CHARS);
    if let Some(summary) = &entry.summary {
        check_text(&mut findings, "summary", summary, MAX_SUMMARY_CHARS);
    }
    if let Some(homepage) = &entry.homepage {
        check_url(&mut findings, "homepage", homepage);
    }
    check_names(&mut findings, entry);

    if entry.editions.is_empty() {
        findings.invalid("an entry needs at least one edition");
    }
    if entry.editions.len() > MAX_EDITIONS {
        findings.add(
            Code::LimitExceeded,
            format!("an entry may have at most {MAX_EDITIONS} editions"),
        );
    }
    let mut identities = BTreeSet::new();
    for (index, edition) in entry.editions.iter().enumerate() {
        let label = format!("edition {} ({})", index + 1, edition.kind.as_str());
        check_edition(&mut findings, &label, edition, class, today);
        if let Some(identity) = edition_identity(edition)
            && !identities.insert(identity)
        {
            findings.invalid(format!("{label} repeats an earlier edition"));
        }
    }
    findings.list
}

/// Validates a whole catalogue: every entry on its own, then the rules
/// across entries.
///
/// Each element is the file name an entry was read from and the entry. The
/// rules across entries are: the file is named `<id>.toml`; application IDs
/// are unique; and no name or alias, compared through its normalised key,
/// belongs to two applications.
pub fn validate_catalogue(entries: &[(String, Entry)], today: Date) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut ids: BTreeMap<&str, &str> = BTreeMap::new();
    let mut names: BTreeMap<String, &str> = BTreeMap::new();

    for (file, entry) in entries {
        for (code, message) in validate_entry(entry, today) {
            problems.push(Problem::in_file(code, file, message));
        }
        let expected = format!("{}.toml", entry.id);
        if *file != expected {
            problems.push(Problem::in_file(
                Code::EntryInvalid,
                file,
                format!(
                    "the file of application {} must be named {expected}",
                    entry.id
                ),
            ));
        }
        match ids.get(entry.id.as_str()) {
            Some(first) => problems.push(Problem::in_file(
                Code::DuplicateId,
                file,
                format!("application ID {} is already used by {first}", entry.id),
            )),
            None => {
                ids.insert(&entry.id, file);
            }
        }
        let own: BTreeSet<String> = std::iter::once(&entry.name)
            .chain(entry.aliases.iter())
            .map(|name| normalise(name))
            .filter(|key| !key.is_empty())
            .collect();
        for key in own {
            match names.get(&key) {
                Some(first) if *first != entry.id => problems.push(Problem::in_file(
                    Code::AliasCollision,
                    file,
                    format!(
                        "a name or alias that reads \"{key}\" is already used by application {first}"
                    ),
                )),
                Some(_) => {}
                None => {
                    names.insert(key, &entry.id);
                }
            }
        }
    }
    problems
}

/// Checks the name and aliases of one entry against each other.
fn check_names(findings: &mut Findings, entry: &Entry) {
    if entry.aliases.len() > MAX_ALIASES {
        findings.add(
            Code::LimitExceeded,
            format!("an entry may have at most {MAX_ALIASES} aliases"),
        );
    }
    let name_key = normalise(&entry.name);
    if name_key.is_empty() {
        findings.invalid("name has no letter or digit to search by");
    }
    let mut seen = BTreeSet::new();
    seen.insert(name_key);
    for alias in &entry.aliases {
        check_text(findings, "alias", alias, MAX_NAME_CHARS);
        let key = normalise(alias);
        if key.is_empty() {
            findings.invalid(format!(
                "alias \"{}\" has no letter or digit to search by",
                printable(alias)
            ));
        } else if !seen.insert(key) {
            findings.add(
                Code::AliasCollision,
                format!(
                    "alias \"{}\" reads the same as the name or an earlier alias of this entry",
                    printable(alias)
                ),
            );
        }
    }
}

/// Checks one edition.
fn check_edition(
    findings: &mut Findings,
    label: &str,
    edition: &Edition,
    class: bool,
    today: Date,
) {
    let kind = edition.kind;
    let fields: [(&str, bool, &[EditionKind]); 12] = [
        ("installer", edition.installer.is_some(), WINDOWS_ONLY),
        ("flatpak_id", edition.flatpak_id.is_some(), FLATPAK_ONLY),
        ("remote", edition.remote.is_some(), FLATPAK_ONLY),
        ("obtain", edition.obtain.is_some(), OBTAINED),
        ("package", edition.package.is_some(), WITH_PACKAGE),
        (
            "distribution",
            edition.distribution.is_some(),
            LINUX_NATIVE_ONLY,
        ),
        ("store_id", edition.store_id.is_some(), OBTAINED),
        (
            "download_page",
            edition.download_page.is_some(),
            WITH_DOWNLOAD_PAGE,
        ),
        ("stores", !edition.stores.is_empty(), ANDROID_ONLY),
        ("url", edition.url.is_some(), WEB_ONLY),
        ("primary", edition.primary, WEB_ONLY),
        ("packaging", edition.packaging.is_some(), LINUX_PACKAGES),
    ];
    for (field, present, kinds) in fields {
        if present && !kinds.contains(&kind) {
            findings.invalid(format!(
                "{label}: field {field} is not used by a {} edition",
                kind.as_str()
            ));
        }
    }

    match kind {
        EditionKind::Flatpak => {
            match &edition.flatpak_id {
                Some(id) if !is_flatpak_id(id) => findings.invalid(format!(
                    "{label}: flatpak_id \"{}\" is not a Flatpak application ID",
                    printable(id)
                )),
                None if !class => findings.invalid(format!("{label}: flatpak_id is required")),
                _ => {}
            }
            if let Some(remote) = &edition.remote
                && !is_slug(remote)
            {
                findings.invalid(format!(
                    "{label}: remote \"{}\" is not a lower-case name",
                    printable(remote)
                ));
            }
        }
        EditionKind::LinuxNative => {
            let allowed = [
                Obtain::FedoraPackage,
                Obtain::OtherDistributionPackage,
                Obtain::Steam,
                Obtain::PublisherPage,
            ];
            check_obtain(findings, label, edition, class, &allowed);
        }
        EditionKind::Windows => {
            let allowed = [
                Obtain::Steam,
                Obtain::Epic,
                Obtain::Gog,
                Obtain::PublisherPage,
                Obtain::MicrosoftStore,
            ];
            check_obtain(findings, label, edition, class, &allowed);
        }
        EditionKind::Macos => {
            let allowed = [Obtain::MacAppStore, Obtain::PublisherPage, Obtain::Steam];
            check_obtain(findings, label, edition, class, &allowed);
        }
        EditionKind::Android => check_android(findings, label, edition, class),
        EditionKind::Web => match &edition.url {
            Some(url) => check_url(findings, &format!("{label}: url"), url),
            None if !class => findings.invalid(format!("{label}: url is required")),
            None => {}
        },
    }

    if let Some(page) = &edition.download_page {
        let what = format!("{label}: download_page");
        check_url(findings, &what, page);
        if is_direct_download(page) {
            findings.invalid(format!(
                "{what} points at a file to download; it must be a page for a person to read"
            ));
        }
    }
    if let Some(installer) = &edition.installer
        && kind == EditionKind::Windows
    {
        check_installer(findings, label, edition, installer, today);
    }
    if let Some(notes) = &edition.notes {
        check_text(findings, &format!("{label}: notes"), notes, MAX_NOTES_CHARS);
    }

    let mut requirements = BTreeSet::new();
    for requirement in &edition.requirements {
        if !requirements.insert(*requirement) {
            findings.invalid(format!(
                "{label}: requirement {} is listed twice",
                requirement.as_str()
            ));
        }
    }

    if edition.records.len() > MAX_RECORDS || edition.references.len() > MAX_RECORDS {
        findings.add(
            Code::LimitExceeded,
            format!("{label}: at most {MAX_RECORDS} records and {MAX_RECORDS} references"),
        );
    }
    for (index, record) in edition.records.iter().enumerate() {
        let what = format!("{label}, record {}", index + 1);
        check_record(findings, &what, edition, record, today);
    }
    for (index, reference) in edition.references.iter().enumerate() {
        let what = format!("{label}, reference {}", index + 1);
        check_reference(findings, &what, reference, today);
    }
}

/// Checks where a native Linux, Windows or macOS edition is obtained, and
/// the identifier fields that go with each source.
fn check_obtain(
    findings: &mut Findings,
    label: &str,
    edition: &Edition,
    class: bool,
    allowed: &[Obtain],
) {
    let Some(obtain) = edition.obtain else {
        if !class {
            findings.invalid(format!("{label}: obtain is required"));
        }
        return;
    };
    if !allowed.contains(&obtain) {
        findings.invalid(format!(
            "{label}: obtain \"{}\" is not possible for a {} edition",
            obtain.as_str(),
            edition.kind.as_str()
        ));
        return;
    }
    // (field, present, required, permitted)
    let package = obtain == Obtain::FedoraPackage || obtain == Obtain::OtherDistributionPackage;
    let other_distribution = obtain == Obtain::OtherDistributionPackage;
    let publisher_page = obtain == Obtain::PublisherPage;
    let rules = [
        ("package", edition.package.is_some(), package, package),
        (
            "distribution",
            edition.distribution.is_some(),
            other_distribution,
            other_distribution,
        ),
        (
            "store_id",
            edition.store_id.is_some(),
            obtain.uses_store_id(),
            obtain.uses_store_id(),
        ),
        (
            "download_page",
            edition.download_page.is_some(),
            publisher_page,
            publisher_page || other_distribution,
        ),
    ];
    for (field, present, required, permitted) in rules {
        if present && !permitted {
            findings.invalid(format!(
                "{label}: field {field} is not used when the edition is obtained from {}",
                obtain.label()
            ));
        }
        if required && !present && !class {
            findings.invalid(format!(
                "{label}: field {field} is required when the edition is obtained from {}",
                obtain.label()
            ));
        }
    }
    if let Some(package) = &edition.package
        && !is_package_name(package)
    {
        findings.invalid(format!(
            "{label}: package \"{}\" is not a package name",
            printable(package)
        ));
    }
    if let Some(distribution) = &edition.distribution
        && !is_slug(distribution)
    {
        findings.invalid(format!(
            "{label}: distribution \"{}\" is not a lower-case name",
            printable(distribution)
        ));
    }
    if let Some(store_id) = &edition.store_id {
        let digits_only = store_id.bytes().all(|byte| byte.is_ascii_digit());
        if !is_store_id(store_id) || (obtain == Obtain::Steam && !digits_only) {
            findings.invalid(format!(
                "{label}: store_id \"{}\" is not an identifier in {}",
                printable(store_id),
                obtain.label()
            ));
        }
    }
}

/// Checks the pinned installer of a Windows edition (ADR-0019).
///
/// The catalogue names a file to download in this one place, so the rules
/// are strict: only for an edition obtained from the publisher's page and
/// marked open source; the location is an `https` URL that ends in the name
/// of a program, a package or an archive; the digest is a whole SHA-256;
/// and the licence, where it was read, where the digest comes from and the
/// day all of it was checked are stated.
fn check_installer(
    findings: &mut Findings,
    label: &str,
    edition: &Edition,
    installer: &crate::model::Installer,
    today: Date,
) {
    let what = format!("{label}: installer");
    if edition.obtain != Some(Obtain::PublisherPage) {
        findings.invalid(format!(
            "{what} is only possible when the edition is obtained from the publisher's page"
        ));
    }
    if edition.licence != Some(crate::model::Licence::OpenSource) {
        findings.invalid(format!(
            "{what} needs licence = \"open-source\" on the edition: TUR downloads only software that may be freely redistributed, and never a proprietary application"
        ));
    }

    let url_what = format!("{what}: url");
    check_url(findings, &url_what, &installer.url);
    let lower = installer.url.to_ascii_lowercase();
    let plain = !installer.url.contains(['?', '#']);
    let name = installer.file_name();
    let name_valid = name
        .bytes()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'));
    if !plain
        || !name_valid
        || !INSTALLER_ENDINGS
            .iter()
            .any(|ending| lower.ends_with(ending))
    {
        findings.invalid(format!(
            "{url_what} must end in the name of the file, a .exe, .msi or .zip, with no query or fragment"
        ));
    }
    if installer.portable && !lower.ends_with(".exe") {
        findings.invalid(format!(
            "{what}: portable is for a single program, a .exe; an archive is unpacked without it"
        ));
    }

    let digest_valid = installer.sha256.len() == 64
        && installer
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !digest_valid {
        findings.invalid(format!(
            "{what}: sha256 is not 64 lower-case hexadecimal digits"
        ));
    }
    if installer.size == 0 {
        findings.invalid(format!("{what}: size is zero"));
    }
    if installer.size > MAX_INSTALLER_BYTES {
        findings.add(
            Code::LimitExceeded,
            format!("{what}: size is larger than {MAX_INSTALLER_BYTES} bytes"),
        );
    }
    check_text(
        findings,
        &format!("{what}: version"),
        &installer.version,
        MAX_SHORT_CHARS,
    );
    check_text(
        findings,
        &format!("{what}: licence"),
        &installer.licence,
        MAX_NAME_CHARS,
    );
    check_url(
        findings,
        &format!("{what}: licence_url"),
        &installer.licence_url,
    );
    check_text(
        findings,
        &format!("{what}: hash_source"),
        &installer.hash_source,
        MAX_NAME_CHARS,
    );
    check_date(
        findings,
        &format!("{what}: checked"),
        &installer.checked,
        today,
    );
}

/// Checks the fields of an Android edition.
fn check_android(findings: &mut Findings, label: &str, edition: &Edition, class: bool) {
    match &edition.package {
        Some(package) if !is_android_package(package) => findings.invalid(format!(
            "{label}: package \"{}\" is not an Android package name",
            printable(package)
        )),
        None if !class => findings.invalid(format!("{label}: package is required")),
        _ => {}
    }
    if edition.stores.is_empty() && !class {
        findings.invalid(format!("{label}: stores must name at least one store"));
    }
    let mut stores = BTreeSet::new();
    for store in &edition.stores {
        if !stores.insert(*store) {
            findings.invalid(format!("{label}: store {} is listed twice", store.as_str()));
        }
    }
    let from_page = edition.stores.contains(&AndroidStore::PublisherPage);
    if from_page && edition.download_page.is_none() {
        findings.invalid(format!(
            "{label}: download_page is required when stores lists publisher-page"
        ));
    }
    if !from_page && edition.download_page.is_some() {
        findings.invalid(format!(
            "{label}: download_page is only used when stores lists publisher-page"
        ));
    }
}

/// Checks one compatibility record, including the honesty rule.
fn check_record(
    findings: &mut Findings,
    what: &str,
    edition: &Edition,
    record: &Record,
    today: Date,
) {
    if !edition.routes().contains(&record.route) {
        let possible: Vec<&str> = edition
            .routes()
            .iter()
            .map(|route| route.as_str())
            .collect();
        findings.add(
            Code::RouteMismatch,
            format!(
                "{what}: route {} cannot run this edition; possible: {}",
                record.route.as_str(),
                possible.join(", ")
            ),
        );
    }
    check_date(findings, &format!("{what}: date"), &record.date, today);
    if let Some(version) = &record.version_tested {
        check_text(
            findings,
            &format!("{what}: version_tested"),
            version,
            MAX_SHORT_CHARS,
        );
    }
    if let Some(notes) = &record.notes {
        check_text(findings, &format!("{what}: notes"), notes, MAX_NOTES_CHARS);
    }
    if let Some(rating) = &record.source_rating {
        check_text(
            findings,
            &format!("{what}: source_rating"),
            rating,
            MAX_SHORT_CHARS,
        );
    }
    if let Some(name) = &record.source_name {
        check_text(
            findings,
            &format!("{what}: source_name"),
            name,
            MAX_NAME_CHARS,
        );
    }
    if let Some(url) = &record.source_url {
        check_url(findings, &format!("{what}: source_url"), url);
    }

    let has_source = record.source_name.is_some()
        || record.source_url.is_some()
        || record.source_rating.is_some();
    match record.reported_by {
        Reporter::Nobody => {
            if record.rating != Rating::Unknown {
                findings.add(
                    Code::UnsourcedRating,
                    format!(
                        "{what}: rating {} has no source; only unknown may be entered without one",
                        record.rating.as_str()
                    ),
                );
            }
            if has_source || record.test_report.is_some() {
                findings.invalid(format!(
                    "{what}: a record reported by nobody carries no source or test report"
                ));
            }
        }
        Reporter::External => {
            if record.source_name.is_none() || record.source_url.is_none() {
                findings.add(
                    Code::UnsourcedRating,
                    format!("{what}: an external record needs source_name and source_url"),
                );
            }
            if record.rating != Rating::Unknown && record.source_rating.is_none() {
                findings.add(
                    Code::UnsourcedRating,
                    format!(
                        "{what}: an external rating needs source_rating, the source's own words"
                    ),
                );
            }
            if record.test_report.is_some() {
                findings.invalid(format!(
                    "{what}: test_report belongs to TUR's own tests, not to an external record"
                ));
            }
            if let Some(name) = &record.source_name
                && claims_tur(name)
            {
                findings.add(
                    Code::UnsourcedRating,
                    format!(
                        "{what}: TUR is not an external source; a TUR test is reported_by = \"tur-test\" with a test report"
                    ),
                );
            }
        }
        Reporter::TurTest => {
            match &record.test_report {
                None => findings.add(
                    Code::UnsourcedRating,
                    format!("{what}: a TUR test needs test_report, the path of its report"),
                ),
                Some(path) if !is_report_path(path) => findings.add(
                    Code::UnsourcedRating,
                    format!(
                        "{what}: test_report \"{}\" is not a report path under docs/reports/ or tests/",
                        printable(path)
                    ),
                ),
                Some(_) => {}
            }
            if record.version_tested.is_none() {
                findings.invalid(format!("{what}: a TUR test states version_tested"));
            }
            if has_source {
                findings.invalid(format!(
                    "{what}: a TUR test carries no external source fields"
                ));
            }
        }
    }
}

/// Checks one reference.
fn check_reference(findings: &mut Findings, what: &str, reference: &Reference, today: Date) {
    check_text(
        findings,
        &format!("{what}: title"),
        &reference.title,
        MAX_NAME_CHARS,
    );
    check_url(findings, &format!("{what}: url"), &reference.url);
    check_date(
        findings,
        &format!("{what}: checked"),
        &reference.checked,
        today,
    );
}

/// What makes two editions of one entry the same edition, or `None` when the
/// edition carries no identifier (as in a class entry).
fn edition_identity(edition: &Edition) -> Option<String> {
    let identifier = match edition.kind {
        EditionKind::Flatpak => edition.flatpak_id.clone(),
        EditionKind::Android => edition.package.clone(),
        EditionKind::Web => edition.url.clone(),
        EditionKind::LinuxNative | EditionKind::Windows | EditionKind::Macos => {
            let obtain = edition.obtain?;
            let detail = edition
                .store_id
                .as_deref()
                .or(edition.package.as_deref())
                .or(edition.download_page.as_deref())?;
            Some(format!("{}:{detail}", obtain.as_str()))
        }
    }?;
    Some(format!("{}|{identifier}", edition.kind.as_str()))
}

/// Checks a piece of text shown to people: not empty, no control
/// characters, no leading or trailing space, within `max` characters.
fn check_text(findings: &mut Findings, what: &str, text: &str, max: usize) {
    if text.trim().is_empty() {
        findings.invalid(format!("{what} is empty"));
        return;
    }
    if text.trim() != text {
        findings.invalid(format!("{what} starts or ends with a space"));
    }
    if text.chars().any(char::is_control) {
        findings.invalid(format!("{what} holds a control character"));
    }
    if text.chars().count() > max {
        findings.add(
            Code::LimitExceeded,
            format!("{what} is longer than {max} characters"),
        );
    }
}

fn check_url(findings: &mut Findings, what: &str, url: &str) {
    if let Err(reason) = check_https_url(url) {
        findings.invalid(format!("{what} is not an https URL: {reason}"));
    }
}

fn check_date(findings: &mut Findings, what: &str, text: &str, today: Date) {
    let Some(date) = Date::parse(text) else {
        findings.invalid(format!(
            "{what} \"{}\" is not a date written YYYY-MM-DD",
            printable(text)
        ));
        return;
    };
    if date.year() < MIN_YEAR {
        findings.invalid(format!("{what} {date} is before {MIN_YEAR}"));
    }
    if date.days_since_epoch() > today.days_since_epoch() + 1 {
        findings.invalid(format!("{what} {date} is in the future"));
    }
}

/// A value for an error message: control characters removed, and cut short.
fn printable(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect()
}

/// Whether a source name is TUR itself under some spelling.
fn claims_tur(name: &str) -> bool {
    let key = normalise(name);
    // "uos" is the project's former name: a record must not claim it either.
    key == "tur" || key == "uos" || key.starts_with("turtest") || key == "theunifiedrealm"
}

fn is_application_id(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_ID_BYTES {
        return false;
    }
    let mut segments = 0usize;
    for segment in text.split('.') {
        let mut bytes = segment.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
            return false;
        }
        if !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-') {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}

fn is_flatpak_id(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_ID_BYTES {
        return false;
    }
    let mut segments = 0usize;
    for segment in text.split('.') {
        let mut bytes = segment.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return false;
        }
        if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return false;
        }
        segments += 1;
    }
    segments >= 3
}

fn is_android_package(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_ID_BYTES {
        return false;
    }
    let mut segments = 0usize;
    for segment in text.split('.') {
        let mut bytes = segment.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        if !first.is_ascii_alphabetic() {
            return false;
        }
        if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}

fn is_package_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    text.len() <= 128
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'.' | b'_' | b'-')
        })
}

fn is_slug(text: &str) -> bool {
    let mut bytes = text.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    text.len() <= 64
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_store_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Whether `text` is the repository path of a test report: a relative path
/// under `docs/reports/` or `tests/`, without `.` or `..` segments, ending
/// in `.md` or `.json`.
pub fn is_report_path(text: &str) -> bool {
    if text.len() > 255 || !(text.starts_with("docs/reports/") || text.starts_with("tests/")) {
        return false;
    }
    if !(text.ends_with(".md") || text.ends_with(".json")) {
        return false;
    }
    text.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Licence, Packaging, Requirement, Route};

    const NONE: [Code; 0] = [];

    fn today() -> Date {
        Date::parse("2026-11-12").unwrap()
    }

    fn flatpak() -> Edition {
        let mut edition = Edition::new(EditionKind::Flatpak);
        edition.flatpak_id = Some("org.example.App".to_owned());
        edition
    }

    fn windows() -> Edition {
        let mut edition = Edition::new(EditionKind::Windows);
        edition.obtain = Some(Obtain::Steam);
        edition.store_id = Some("12345".to_owned());
        edition
    }

    fn entry(editions: Vec<Edition>) -> Entry {
        Entry {
            schema_version: SCHEMA_VERSION,
            id: "org.example.app".to_owned(),
            entry_kind: EntryKind::Application,
            name: "Example".to_owned(),
            aliases: vec!["Sample".to_owned()],
            publisher: "Example Project".to_owned(),
            category: Category::Utility,
            summary: Some("An example.".to_owned()),
            homepage: Some("https://example.org/".to_owned()),
            editions,
        }
    }

    fn external(route: Route, rating: Rating) -> Record {
        Record {
            route,
            rating,
            reported_by: Reporter::External,
            source_name: Some("ProtonDB".to_owned()),
            source_url: Some("https://www.protondb.com/app/12345".to_owned()),
            source_rating: Some("Gold".to_owned()),
            test_report: None,
            version_tested: None,
            date: "2026-11-12".to_owned(),
            notes: None,
        }
    }

    fn unknown(route: Route) -> Record {
        Record {
            route,
            rating: Rating::Unknown,
            reported_by: Reporter::Nobody,
            source_name: None,
            source_url: None,
            source_rating: None,
            test_report: None,
            version_tested: None,
            date: "2026-11-12".to_owned(),
            notes: None,
        }
    }

    fn tur_test(route: Route, rating: Rating) -> Record {
        Record {
            route,
            rating,
            reported_by: Reporter::TurTest,
            source_name: None,
            source_url: None,
            source_rating: None,
            test_report: Some("docs/reports/compat/example-1.md".to_owned()),
            version_tested: Some("1.2.3".to_owned()),
            date: "2026-11-12".to_owned(),
            notes: None,
        }
    }

    fn codes(entry: &Entry) -> Vec<Code> {
        validate_entry(entry, today())
            .into_iter()
            .map(|(code, _)| code)
            .collect()
    }

    fn messages(entry: &Entry) -> String {
        validate_entry(entry, today())
            .into_iter()
            .map(|(_, message)| message)
            .collect::<Vec<String>>()
            .join("\n")
    }

    fn with_record(record: Record) -> Entry {
        let mut edition = windows();
        edition.records.push(record);
        entry(vec![edition])
    }

    #[test]
    fn a_complete_entry_is_valid() {
        let mut first = flatpak();
        first.remote = Some("flathub".to_owned());
        first.licence = Some(Licence::OpenSource);
        first.packaging = Some(Packaging::Publisher);
        first.requirements = vec![Requirement::OnlineAccount];
        first.records = vec![unknown(Route::Flatpak)];
        first.references = vec![Reference {
            title: "Flathub listing".to_owned(),
            url: "https://flathub.org/apps/org.example.App".to_owned(),
            checked: "2026-11-12".to_owned(),
        }];
        let mut second = windows();
        second.records = vec![
            external(Route::Proton, Rating::Usable),
            unknown(Route::Wine),
            tur_test(Route::WindowsGuest, Rating::Good),
        ];
        let mut web = Edition::new(EditionKind::Web);
        web.url = Some("https://app.example.org/".to_owned());
        web.primary = true;
        let mut android = Edition::new(EditionKind::Android);
        android.package = Some("org.example.app".to_owned());
        android.stores = vec![AndroidStore::FDroid, AndroidStore::PublisherPage];
        android.download_page = Some("https://example.org/android".to_owned());
        let mut debian = Edition::new(EditionKind::LinuxNative);
        debian.obtain = Some(Obtain::OtherDistributionPackage);
        debian.distribution = Some("debian".to_owned());
        debian.package = Some("example-app".to_owned());
        debian.download_page = Some("https://example.org/linux".to_owned());
        let mut mac = Edition::new(EditionKind::Macos);
        mac.obtain = Some(Obtain::PublisherPage);
        mac.download_page = Some("https://example.org/mac".to_owned());
        let entry = entry(vec![first, second, web, android, debian, mac]);
        let found = validate_entry(&entry, today());
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_tur_test_claim_without_a_report_reference_is_refused() {
        let mut record = tur_test(Route::Wine, Rating::Good);
        record.test_report = None;
        let entry = with_record(record);
        assert_eq!(codes(&entry), [Code::UnsourcedRating]);
        assert!(messages(&entry).contains("test_report"));
    }

    #[test]
    fn a_tur_test_report_reference_must_be_a_repository_report_path() {
        for path in [
            "report.md",
            "/docs/reports/x.md",
            "docs/reports/../../etc/passwd.md",
            "docs/reports/x.txt",
            "docs/reports/",
            "docs/reports//x.md",
            "https://example.org/report.md",
            "docs/reports/a b.md",
            "src/x.md",
        ] {
            let mut record = tur_test(Route::Wine, Rating::Good);
            record.test_report = Some(path.to_owned());
            assert_eq!(
                codes(&with_record(record)),
                [Code::UnsourcedRating],
                "{path}"
            );
        }
        for path in ["docs/reports/x.md", "tests/compat/2026/elden-ring.json"] {
            let mut record = tur_test(Route::Wine, Rating::Good);
            record.test_report = Some(path.to_owned());
            assert_eq!(codes(&with_record(record)), NONE, "{path}");
        }
    }

    #[test]
    fn a_rating_without_a_source_is_refused_unless_it_is_unknown() {
        for rating in [Rating::Good, Rating::Usable, Rating::Poor, Rating::Broken] {
            let mut record = unknown(Route::Wine);
            record.rating = rating;
            assert_eq!(codes(&with_record(record)), [Code::UnsourcedRating]);
        }
        assert_eq!(codes(&with_record(unknown(Route::Wine))), NONE);
    }

    #[test]
    fn an_external_rating_needs_a_name_a_url_and_the_sources_own_rating() {
        let mut record = external(Route::Proton, Rating::Good);
        record.source_url = None;
        assert_eq!(codes(&with_record(record)), [Code::UnsourcedRating]);

        let mut record = external(Route::Proton, Rating::Good);
        record.source_name = None;
        assert_eq!(codes(&with_record(record)), [Code::UnsourcedRating]);

        let mut record = external(Route::Proton, Rating::Good);
        record.source_rating = None;
        assert_eq!(codes(&with_record(record)), [Code::UnsourcedRating]);

        // An external source may also say that it does not know.
        let mut record = external(Route::Proton, Rating::Unknown);
        record.source_rating = None;
        assert_eq!(codes(&with_record(record)), NONE);

        let mut record = external(Route::Proton, Rating::Good);
        record.source_url = Some("http://www.protondb.com/app/1".to_owned());
        assert_eq!(codes(&with_record(record)), [Code::EntryInvalid]);
    }

    #[test]
    fn tur_cannot_be_entered_as_an_external_source() {
        for name in [
            "TUR",
            "tur",
            "T.U.R.",
            "TUR test",
            "TUR",
            "The Unified Realm",
        ] {
            let mut record = external(Route::Proton, Rating::Good);
            record.source_name = Some(name.to_owned());
            assert_eq!(
                codes(&with_record(record)),
                [Code::UnsourcedRating],
                "{name}"
            );
        }
    }

    #[test]
    fn source_fields_must_match_the_reporter() {
        let mut record = unknown(Route::Wine);
        record.source_name = Some("ProtonDB".to_owned());
        assert_eq!(codes(&with_record(record)), [Code::EntryInvalid]);

        let mut record = external(Route::Proton, Rating::Good);
        record.test_report = Some("docs/reports/x.md".to_owned());
        assert_eq!(codes(&with_record(record)), [Code::EntryInvalid]);

        let mut record = tur_test(Route::Wine, Rating::Good);
        record.source_name = Some("ProtonDB".to_owned());
        assert_eq!(codes(&with_record(record)), [Code::EntryInvalid]);

        let mut record = tur_test(Route::Wine, Rating::Good);
        record.version_tested = None;
        assert_eq!(codes(&with_record(record)), [Code::EntryInvalid]);
    }

    #[test]
    fn a_record_must_name_a_route_its_edition_can_use() {
        for route in [
            Route::Flatpak,
            Route::AndroidRuntime,
            Route::Web,
            Route::Darling,
            Route::NoRoute,
        ] {
            assert_eq!(
                codes(&with_record(unknown(route))),
                [Code::RouteMismatch],
                "{}",
                route.as_str()
            );
        }
        for route in [Route::Proton, Route::Wine, Route::WindowsGuest] {
            assert_eq!(codes(&with_record(unknown(route))), NONE);
        }
        let mut edition = flatpak();
        edition.records.push(unknown(Route::Native));
        assert_eq!(codes(&entry(vec![edition])), [Code::RouteMismatch]);

        let mut fedora = Edition::new(EditionKind::LinuxNative);
        fedora.obtain = Some(Obtain::FedoraPackage);
        fedora.package = Some("example".to_owned());
        fedora.records.push(unknown(Route::DistroContainer));
        assert_eq!(codes(&entry(vec![fedora])), [Code::RouteMismatch]);
    }

    #[test]
    fn dates_must_be_real_recent_and_not_in_the_future() {
        for (date, valid) in [
            ("2026-11-12", true),
            ("2026-11-13", true),
            ("2026-11-14", false),
            ("2027-01-01", false),
            ("2026-02-30", false),
            ("1999-12-31", false),
            ("12 November 2026", false),
            ("", false),
        ] {
            let mut record = unknown(Route::Wine);
            record.date = date.to_owned();
            let found = codes(&with_record(record));
            assert_eq!(found.is_empty(), valid, "{date}: {found:?}");
        }
        let mut edition = flatpak();
        edition.references.push(Reference {
            title: "Listing".to_owned(),
            url: "https://example.org/".to_owned(),
            checked: "2030-01-01".to_owned(),
        });
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);
    }

    #[test]
    fn urls_must_be_https_and_well_formed() {
        let mut bad_homepage = entry(vec![flatpak()]);
        bad_homepage.homepage = Some("http://example.org/".to_owned());
        assert_eq!(codes(&bad_homepage), [Code::EntryInvalid]);

        let mut web = Edition::new(EditionKind::Web);
        web.url = Some("example.org".to_owned());
        assert_eq!(codes(&entry(vec![web])), [Code::EntryInvalid]);

        let mut edition = flatpak();
        edition.references.push(Reference {
            title: "Listing".to_owned(),
            url: "https://user:pw@example.org/".to_owned(),
            checked: "2026-11-12".to_owned(),
        });
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);
    }

    #[test]
    fn a_download_page_must_not_be_a_direct_download() {
        let mut edition = Edition::new(EditionKind::Windows);
        edition.obtain = Some(Obtain::PublisherPage);
        edition.download_page = Some("https://example.org/files/setup.exe".to_owned());
        let entry = entry(vec![edition]);
        assert_eq!(codes(&entry), [Code::EntryInvalid]);
        assert!(messages(&entry).contains("a page for a person to read"));
    }

    #[test]
    fn identifiers_are_checked() {
        for id in [
            "Example",
            "org.Example.app",
            "single",
            "org..app",
            "org.example.",
            ".org",
            "org.exa mple",
            "",
        ] {
            let mut bad = entry(vec![flatpak()]);
            bad.id = id.to_owned();
            assert_eq!(codes(&bad), [Code::EntryInvalid], "{id}");
        }
        for id in [
            "org.example.app",
            "tur.class.android-banking",
            "io.7zip.app_2",
        ] {
            let mut good = entry(vec![flatpak()]);
            good.id = id.to_owned();
            assert_eq!(codes(&good), NONE, "{id}");
        }
        assert!(is_flatpak_id("md.obsidian.Obsidian"));
        assert!(is_flatpak_id("com.github.IsmaelMartinez.teams_for_linux"));
        assert!(!is_flatpak_id("org.example"));
        assert!(!is_flatpak_id("org.7zip.App"));
        assert!(!is_flatpak_id("org.exa mple.App"));
        assert!(is_android_package("com.miHoYo.GenshinImpact"));
        assert!(is_android_package("us.zoom.videomeetings"));
        assert!(!is_android_package("whatsapp"));
        assert!(!is_android_package("com.2fa.app"));
        assert!(!is_android_package("com.example.my-app"));
        assert!(is_package_name("7zip"));
        assert!(is_package_name("libstdc++6"));
        assert!(!is_package_name("Firefox"));
        assert!(!is_package_name(""));
        assert!(is_slug("debian"));
        assert!(!is_slug("Debian GNU/Linux"));
        assert!(is_store_id("1245620"));
        assert!(!is_store_id("12 45"));
    }

    #[test]
    fn a_steam_identifier_is_a_number() {
        let mut edition = windows();
        edition.store_id = Some("elden-ring".to_owned());
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);
        let mut edition = windows();
        edition.obtain = Some(Obtain::Epic);
        edition.store_id = Some("elden-ring".to_owned());
        assert_eq!(codes(&entry(vec![edition])), NONE);
    }

    #[test]
    fn an_edition_carries_only_the_fields_of_its_kind() {
        let mut edition = flatpak();
        edition.url = Some("https://example.org/".to_owned());
        assert!(messages(&entry(vec![edition])).contains("field url is not used by a flatpak"));

        let mut edition = flatpak();
        edition.primary = true;
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);

        let mut edition = windows();
        edition.package = Some("example".to_owned());
        assert!(messages(&entry(vec![edition])).contains("field package is not used by a windows"));

        let mut edition = windows();
        edition.download_page = Some("https://example.org/download".to_owned());
        assert!(
            messages(&entry(vec![edition]))
                .contains("not used when the edition is obtained from Steam")
        );

        let mut edition = Edition::new(EditionKind::Web);
        edition.url = Some("https://example.org/".to_owned());
        edition.stores = vec![AndroidStore::GooglePlay];
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);

        let mut edition = windows();
        edition.packaging = Some(Packaging::Community);
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);
    }

    #[test]
    fn an_edition_needs_the_identifiers_of_its_kind() {
        let cases: Vec<(Edition, &str)> = vec![
            (Edition::new(EditionKind::Flatpak), "flatpak_id is required"),
            (Edition::new(EditionKind::Web), "url is required"),
            (Edition::new(EditionKind::Windows), "obtain is required"),
            (Edition::new(EditionKind::Android), "package is required"),
            (
                {
                    let mut edition = Edition::new(EditionKind::Windows);
                    edition.obtain = Some(Obtain::Steam);
                    edition
                },
                "field store_id is required",
            ),
            (
                {
                    let mut edition = Edition::new(EditionKind::LinuxNative);
                    edition.obtain = Some(Obtain::OtherDistributionPackage);
                    edition.package = Some("example".to_owned());
                    edition
                },
                "field distribution is required",
            ),
            (
                {
                    let mut edition = Edition::new(EditionKind::Macos);
                    edition.obtain = Some(Obtain::FedoraPackage);
                    edition.package = Some("example".to_owned());
                    edition
                },
                "is not possible for a macos edition",
            ),
            (
                {
                    let mut edition = Edition::new(EditionKind::Android);
                    edition.package = Some("org.example.app".to_owned());
                    edition.stores = vec![AndroidStore::PublisherPage];
                    edition
                },
                "download_page is required",
            ),
        ];
        for (edition, expected) in cases {
            let text = messages(&entry(vec![edition]));
            assert!(
                text.contains(expected),
                "expected \"{expected}\" in: {text}"
            );
        }
    }

    #[test]
    fn a_class_entry_may_leave_identifiers_out() {
        let mut edition = Edition::new(EditionKind::Android);
        edition.requirements = vec![Requirement::PlayIntegrity];
        let mut class = entry(vec![edition]);
        class.entry_kind = EntryKind::Class;
        assert_eq!(codes(&class), NONE);
        class.entry_kind = EntryKind::Application;
        assert!(!codes(&class).is_empty());
    }

    #[test]
    fn names_and_aliases_are_checked() {
        let mut bad = entry(vec![flatpak()]);
        bad.name = " ".to_owned();
        assert!(codes(&bad).contains(&Code::EntryInvalid));

        let mut bad = entry(vec![flatpak()]);
        bad.name = "Exa\u{1b}[31mmple".to_owned();
        assert_eq!(codes(&bad), [Code::EntryInvalid]);

        let mut bad = entry(vec![flatpak()]);
        bad.aliases = vec!["EXAMPLE!".to_owned()];
        assert_eq!(codes(&bad), [Code::AliasCollision]);

        let mut bad = entry(vec![flatpak()]);
        bad.aliases = vec!["Sample".to_owned(), "sam-ple".to_owned()];
        assert_eq!(codes(&bad), [Code::AliasCollision]);

        let mut bad = entry(vec![flatpak()]);
        bad.aliases = vec!["!!!".to_owned()];
        assert_eq!(codes(&bad), [Code::EntryInvalid]);

        let mut bad = entry(vec![flatpak()]);
        bad.name = "x".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(codes(&bad), [Code::LimitExceeded]);
    }

    #[test]
    fn other_entry_rules() {
        let mut bad = entry(vec![flatpak()]);
        bad.schema_version = 2;
        assert_eq!(codes(&bad), [Code::EntryInvalid]);

        assert_eq!(codes(&entry(Vec::new())), [Code::EntryInvalid]);

        assert_eq!(
            codes(&entry(vec![flatpak(), flatpak()])),
            [Code::EntryInvalid]
        );

        let mut edition = flatpak();
        edition.requirements = vec![Requirement::HeavyGpu, Requirement::HeavyGpu];
        assert_eq!(codes(&entry(vec![edition])), [Code::EntryInvalid]);

        let many: Vec<Edition> = (0..=MAX_EDITIONS)
            .map(|number| {
                let mut edition = flatpak();
                edition.flatpak_id = Some(format!("org.example.App{number}"));
                edition
            })
            .collect();
        assert_eq!(codes(&entry(many)), [Code::LimitExceeded]);
    }

    fn named(id: &str, name: &str, aliases: &[&str]) -> (String, Entry) {
        let mut entry = entry(vec![flatpak()]);
        entry.id = id.to_owned();
        entry.name = name.to_owned();
        entry.aliases = aliases.iter().map(|alias| (*alias).to_owned()).collect();
        (format!("{id}.toml"), entry)
    }

    fn catalogue_codes(entries: &[(String, Entry)]) -> Vec<Code> {
        validate_catalogue(entries, today())
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn application_ids_must_be_unique() {
        let first = named("org.example.one", "One", &[]);
        let mut second = named("org.example.one", "Two", &[]);
        second.0 = "copy.toml".to_owned();
        let problems = validate_catalogue(&[first, second], today());
        let found: Vec<Code> = problems.iter().map(|problem| problem.code).collect();
        assert_eq!(found, [Code::EntryInvalid, Code::DuplicateId]);
        assert_eq!(problems[1].file.as_deref(), Some("copy.toml"));
        assert!(problems[1].message.contains("org.example.one.toml"));
    }

    #[test]
    fn names_and_aliases_must_not_collide_across_applications() {
        // Alias against name, name against name, alias against alias; each
        // compared through the normalised key.
        let cases = [
            (
                named("org.example.one", "One", &[]),
                named("org.example.two", "Two", &["ONE"]),
            ),
            (
                named("org.example.one", "One App", &[]),
                named("org.example.two", "one-app", &[]),
            ),
            (
                named("org.example.one", "One", &["Uno"]),
                named("org.example.two", "Two", &["U.N.O."]),
            ),
        ];
        for (first, second) in cases {
            assert_eq!(catalogue_codes(&[first, second]), [Code::AliasCollision]);
        }
        let fine = [
            named("org.example.one", "One", &["Uno"]),
            named("org.example.two", "Two", &["Dos"]),
        ];
        assert_eq!(catalogue_codes(&fine), NONE);
    }

    fn pinned() -> crate::model::Installer {
        crate::model::Installer {
            url: "https://downloads.example.org/1.2/example-1.2-setup.exe".to_owned(),
            sha256: "9b697f65ecc2d9b0152f5ee96d72dde522a7827690349d9307935dacf40f42ec".to_owned(),
            size: 1_659_704,
            version: "1.2".to_owned(),
            licence: "MIT".to_owned(),
            licence_url: "https://example.org/licence".to_owned(),
            hash_source: "The publisher's sha256sums file".to_owned(),
            checked: "2026-11-12".to_owned(),
            portable: false,
        }
    }

    fn from_the_publisher(installer: crate::model::Installer) -> Edition {
        let mut edition = Edition::new(EditionKind::Windows);
        edition.obtain = Some(Obtain::PublisherPage);
        edition.download_page = Some("https://example.org/download".to_owned());
        edition.licence = Some(Licence::OpenSource);
        edition.installer = Some(installer);
        edition
    }

    #[test]
    fn a_pinned_installer_of_free_software_is_valid() {
        let found = validate_entry(&entry(vec![from_the_publisher(pinned())]), today());
        assert!(found.is_empty(), "{found:?}");

        let mut msi = pinned();
        msi.url = "https://downloads.example.org/1.2/Example-1.2.MSI".to_owned();
        assert_eq!(codes(&entry(vec![from_the_publisher(msi)])), NONE);

        let mut single = pinned();
        single.portable = true;
        assert_eq!(codes(&entry(vec![from_the_publisher(single)])), NONE);
    }

    #[test]
    fn a_proprietary_application_never_carries_an_installer_to_download() {
        // "Never download proprietary software": an edition that is marked
        // proprietary, or not marked at all, may not name a file to fetch.
        let mut proprietary = from_the_publisher(pinned());
        proprietary.licence = Some(Licence::Proprietary);
        let text = messages(&entry(vec![proprietary.clone()]));
        assert_eq!(codes(&entry(vec![proprietary])), [Code::EntryInvalid]);
        assert!(text.contains("never a proprietary application"), "{text}");

        let mut unmarked = from_the_publisher(pinned());
        unmarked.licence = None;
        assert_eq!(codes(&entry(vec![unmarked])), [Code::EntryInvalid]);
    }

    #[test]
    fn an_installer_belongs_to_a_windows_edition_from_the_publisher_s_page() {
        let mut from_a_store = from_the_publisher(pinned());
        from_a_store.obtain = Some(Obtain::Steam);
        from_a_store.download_page = None;
        from_a_store.store_id = Some("12345".to_owned());
        let text = messages(&entry(vec![from_a_store]));
        assert!(
            text.contains("obtained from the publisher's page"),
            "{text}"
        );

        let mut linux = flatpak();
        linux.licence = Some(Licence::OpenSource);
        linux.installer = Some(pinned());
        let text = messages(&entry(vec![linux]));
        assert!(
            text.contains("field installer is not used by a flatpak"),
            "{text}"
        );
    }

    #[test]
    fn an_installer_s_location_digest_and_size_are_checked() {
        for url in [
            "http://downloads.example.org/example-setup.exe",
            "https://downloads.example.org/download",
            "https://downloads.example.org/example-setup.exe?mirror=2",
            "https://downloads.example.org/example-setup.exe#top",
            "https://downloads.example.org/example.dmg",
            "https://downloads.example.org/.exe",
            "https://user@downloads.example.org/example-setup.exe",
        ] {
            let mut installer = pinned();
            installer.url = url.to_owned();
            let found = codes(&entry(vec![from_the_publisher(installer)]));
            assert!(
                !found.is_empty() && found.iter().all(|code| *code == Code::EntryInvalid),
                "{url}: {found:?}"
            );
        }
        for digest in [
            "",
            "9b697f65",
            "9B697F65ECC2D9B0152F5EE96D72DDE522A7827690349D9307935DACF40F42EC",
            "zb697f65ecc2d9b0152f5ee96d72dde522a7827690349d9307935dacf40f42ec",
            "9b697f65ecc2d9b0152f5ee96d72dde522a7827690349d9307935dacf40f42ec0",
        ] {
            let mut installer = pinned();
            installer.sha256 = digest.to_owned();
            let text = messages(&entry(vec![from_the_publisher(installer)]));
            assert!(text.contains("sha256 is not 64"), "{digest}: {text}");
        }

        let mut empty = pinned();
        empty.size = 0;
        assert!(messages(&entry(vec![from_the_publisher(empty)])).contains("size is zero"));
        let mut huge = pinned();
        huge.size = MAX_INSTALLER_BYTES + 1;
        assert_eq!(
            codes(&entry(vec![from_the_publisher(huge)])),
            [Code::LimitExceeded]
        );

        let mut archive = pinned();
        archive.url = "https://downloads.example.org/example-1.2.zip".to_owned();
        assert_eq!(
            codes(&entry(vec![from_the_publisher(archive.clone())])),
            NONE
        );
        archive.portable = true;
        assert!(messages(&entry(vec![from_the_publisher(archive)])).contains("single program"));
    }

    #[test]
    fn an_installer_states_its_licence_its_digest_s_source_and_the_day() {
        let mut installer = pinned();
        installer.licence = " ".to_owned();
        assert!(messages(&entry(vec![from_the_publisher(installer)])).contains("licence is empty"));

        let mut installer = pinned();
        installer.licence_url = "ftp://example.org/licence".to_owned();
        assert!(
            messages(&entry(vec![from_the_publisher(installer)])).contains("licence_url is not")
        );

        let mut installer = pinned();
        installer.hash_source = String::new();
        assert!(
            messages(&entry(vec![from_the_publisher(installer)])).contains("hash_source is empty")
        );

        let mut installer = pinned();
        installer.checked = "2031-01-01".to_owned();
        assert!(messages(&entry(vec![from_the_publisher(installer)])).contains("in the future"));

        let mut installer = pinned();
        installer.version = String::new();
        assert!(messages(&entry(vec![from_the_publisher(installer)])).contains("version is empty"));
    }

    #[test]
    fn the_file_is_named_after_the_application_id() {
        let mut misplaced = named("org.example.one", "One", &[]);
        misplaced.0 = "one.toml".to_owned();
        let problems = validate_catalogue(&[misplaced], today());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, Code::EntryInvalid);
        assert!(problems[0].message.contains("org.example.one.toml"));
    }

    #[test]
    fn entry_problems_are_reported_with_their_file() {
        let mut bad = named("org.example.one", "One", &[]);
        bad.1.editions[0].records.push({
            let mut record = tur_test(Route::Flatpak, Rating::Good);
            record.test_report = None;
            record
        });
        let problems = validate_catalogue(&[bad], today());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, Code::UnsourcedRating);
        assert_eq!(problems[0].file.as_deref(), Some("org.example.one.toml"));
    }
}

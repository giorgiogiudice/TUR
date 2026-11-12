// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Redaction of personal and machine identifiers (specification §6.5).
//!
//! Every text file passes through a [`Redactor`] before it enters a bundle.
//! Each distinct identifier is replaced by a stable pseudonym such as
//! `<mac-1>` or `<user-2>`, so that the same value reads the same everywhere
//! in one bundle and correlations survive, while the value itself does not.
//!
//! Redaction works in two passes over the whole bundle. [`Redactor::learn`]
//! finds values that are only recognisable in context (a user name in a home
//! path, a serial number after its label, an SSID after `SSID=`) and
//! remembers them. [`Redactor::redact`] then replaces those values wherever
//! they appear, with or without their context, together with everything that
//! is recognisable by shape alone (MAC addresses, IP addresses, UUIDs).
//!
//! When a rule has to guess, it guesses towards redacting. The rules and their
//! known gaps are listed in `docs/architecture/diagnostics.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the `redaction-report.json` format.
pub const REDACTION_REPORT_SCHEMA_VERSION: u32 = 1;

/// The longest value an SSID can have, in bytes.
const SSID_MAX_BYTES: usize = 32;

/// The nil UUID, which identifies nothing.
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// GPT partition-type GUIDs. They name a kind of partition, not a disk, and
/// are the same on every machine, so they stay readable.
const WELL_KNOWN_UUIDS: &[&str] = &[
    "c12a7328-f81f-11d2-ba4b-00a0c93ec93b", // EFI system partition
    "21686148-6449-6e6f-744e-656564454649", // BIOS boot
    "0fc63daf-8483-4772-8e79-3d69d8477de4", // Linux filesystem data
    "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f", // Linux swap
    "e6d6d379-f507-44c2-a23c-238f2a3df928", // Linux LVM
    "a19d880f-05fc-4d3b-a006-743f0f84911e", // Linux RAID
    "933ac7e1-2eb4-4f13-b844-0e14e2aef915", // Linux /home
    "4f68bce3-e8cd-4db1-96e7-fbcaf984b709", // Linux root (x86-64)
    "bc13c2ff-59e6-4262-a352-b275fd6f7172", // Extended boot loader
    "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", // Microsoft basic data
    "e3c9e316-0b5c-4db8-817d-f92df00215ae", // Microsoft reserved
    "de94bba4-06d1-4d40-a16a-bfd50179d6ac", // Windows recovery environment
];

/// Directory prefixes whose next path component is a user name.
const USER_PATH_PREFIXES: &[&str] = &["/home/", "/run/media/"];

/// How much a bundle reveals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Redact every category. The profile for bundles that leave the machine.
    Default,
    /// Keep serial numbers, MAC addresses and disk UUIDs for a developer who
    /// needs to tell devices apart. Everything else is still redacted.
    Technical,
}

impl Profile {
    /// Every profile.
    pub const ALL: [Profile; 2] = [Profile::Default, Profile::Technical];

    /// The profile's name as used on the command line and in the manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            Profile::Default => "default",
            Profile::Technical => "technical",
        }
    }

    /// Whether this profile redacts `category`.
    pub const fn redacts(self, category: Category) -> bool {
        match self {
            Profile::Default => true,
            Profile::Technical => {
                !matches!(category, Category::Serial | Category::Mac | Category::Uuid)
            }
        }
    }
}

/// How a collected file is structured, which decides how it is redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Free text. Redacted as one string.
    Text,
    /// One JSON document per line, as written by `journalctl -o json`. Each
    /// string value is decoded, redacted and encoded again, so that JSON
    /// escapes can neither hide an identifier nor be broken by a pseudonym.
    JsonLines,
}

/// A kind of identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// Login names and real names of people, including in home paths.
    Username,
    /// The machine's host name.
    Hostname,
    /// Hardware serial numbers and world-wide names.
    Serial,
    /// MAC and Bluetooth addresses.
    Mac,
    /// Public IPv4 addresses.
    Ipv4,
    /// Public IPv6 addresses, and local ones that embed a MAC address.
    Ipv6,
    /// Disk, partition, filesystem and other UUIDs.
    Uuid,
    /// Wi-Fi network names.
    Ssid,
    /// The systemd machine ID.
    MachineId,
    /// Values labelled as passwords, tokens or keys.
    Secret,
}

impl Category {
    /// Every category, in report order.
    pub const ALL: [Category; 10] = [
        Category::Username,
        Category::Hostname,
        Category::Serial,
        Category::Mac,
        Category::Ipv4,
        Category::Ipv6,
        Category::Uuid,
        Category::Ssid,
        Category::MachineId,
        Category::Secret,
    ];

    /// The category's name in `redaction-report.json`.
    pub const fn name(self) -> &'static str {
        match self {
            Category::Username => "username",
            Category::Hostname => "hostname",
            Category::Serial => "serial_number",
            Category::Mac => "mac_address",
            Category::Ipv4 => "ipv4_address",
            Category::Ipv6 => "ipv6_address",
            Category::Uuid => "uuid",
            Category::Ssid => "wifi_ssid",
            Category::MachineId => "machine_id",
            Category::Secret => "secret",
        }
    }

    /// The word used in pseudonyms: `mac` gives `<mac-1>`, `<mac-2>`, ...
    pub const fn tag(self) -> &'static str {
        match self {
            Category::Username => "user",
            Category::Hostname => "host",
            Category::Serial => "serial",
            Category::Mac => "mac",
            Category::Ipv4 => "ipv4",
            Category::Ipv6 => "ipv6",
            Category::Uuid => "uuid",
            Category::Ssid => "ssid",
            Category::MachineId => "machine-id",
            Category::Secret => "secret",
        }
    }

    /// Which candidate wins when two cover exactly the same text: the more
    /// specific knowledge (a known machine ID) before the more general shape
    /// (any UUID).
    const fn rank(self) -> u8 {
        match self {
            Category::MachineId => 0,
            Category::Secret => 1,
            Category::Username => 2,
            Category::Hostname => 3,
            Category::Ssid => 4,
            Category::Serial => 5,
            Category::Mac => 6,
            Category::Uuid => 7,
            Category::Ipv6 => 8,
            Category::Ipv4 => 9,
        }
    }

    /// The shortest value of this category that is replaced wherever it
    /// appears, not only next to its label. Shorter values are too likely to
    /// be ordinary words or numbers.
    const fn min_literal_len(self) -> usize {
        match self {
            Category::Username | Category::Hostname => 2,
            Category::Ssid => 4,
            Category::Serial | Category::Uuid => 8,
            Category::MachineId => 32,
            Category::Mac | Category::Ipv4 | Category::Ipv6 | Category::Secret => usize::MAX,
        }
    }
}

/// Identifiers of this machine that are known before any file is read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownIdentifiers {
    /// Login names and real names of the people who use the machine.
    pub usernames: Vec<String>,
    /// The machine's host names.
    pub hostnames: Vec<String>,
    /// The systemd machine ID.
    pub machine_ids: Vec<String>,
    /// UUIDs that must stay readable, such as the boot ID the manifest records.
    pub preserved_ids: Vec<String>,
}

/// Counts for one category in `redaction-report.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryReport {
    /// Whether the bundle's profile redacts this category.
    pub redacted: bool,
    /// The word used in this category's pseudonyms.
    pub pseudonym_prefix: String,
    /// How many different values were replaced.
    pub distinct_values: u64,
    /// How many replacements were made in total.
    pub replacements: u64,
}

/// The content of `redaction-report.json`: counts only, never a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionReport {
    /// Version of this document's format.
    pub schema_version: u32,
    /// The profile that was applied.
    pub profile: Profile,
    /// Counts for every category, by category name.
    pub categories: BTreeMap<String, CategoryReport>,
}

/// A piece of text to replace.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    category: Category,
    /// The value in canonical form. Equal keys get the same pseudonym.
    key: String,
}

impl Span {
    fn new(start: usize, end: usize, category: Category, raw: &str) -> Self {
        Span {
            start,
            end,
            category,
            key: normalise_key(category, raw),
        }
    }
}

/// A value that is replaced wherever it stands as a whole word.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Literal {
    category: Category,
    needle: String,
    fold_case: bool,
}

/// What may stand directly before a label for it to count.
#[derive(Debug, Clone, Copy)]
enum Left {
    /// Anything: the label may be the end of a longer one, as in `ID_SERIAL`.
    Any,
    /// Not a letter or digit.
    Boundary,
    /// Not a letter or digit, or this one letter (given in lower case).
    BoundaryOr(u8),
}

/// How far an unquoted value extends.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// To the next white space or closing punctuation.
    Token,
    /// To the end of the line if that is at most this many bytes, else a token.
    LineOrToken(usize),
    /// To the end of the line.
    Line,
}

/// A rule for values that are recognisable only by the label before them.
struct KeyRule {
    /// The label, in lower case.
    key: &'static str,
    /// Endings the label may have, in lower case, longest first.
    suffixes: &'static [&'static str],
    /// Characters that separate label and value.
    separators: &'static [u8],
    left: Left,
    shape: Shape,
    category: Category,
    /// Whether a candidate value is plausible for this category.
    valid: fn(&str) -> bool,
}

const fn secret_rule(key: &'static str, left: Left) -> KeyRule {
    KeyRule {
        key,
        suffixes: &[],
        separators: b":=",
        left,
        shape: Shape::Token,
        category: Category::Secret,
        valid: valid_secret,
    }
}

const KEY_RULES: &[KeyRule] = &[
    KeyRule {
        key: "serial",
        suffixes: &[" number", "_number", "number", "_short", " no", "_no", "no"],
        separators: b":=",
        left: Left::Any,
        shape: Shape::Token,
        category: Category::Serial,
        valid: valid_serial,
    },
    KeyRule {
        key: "wwn",
        suffixes: &[],
        separators: b":=",
        left: Left::Boundary,
        shape: Shape::Token,
        category: Category::Serial,
        valid: valid_serial,
    },
    KeyRule {
        key: "uuid",
        suffixes: &[],
        separators: b":=/",
        left: Left::Any,
        shape: Shape::Token,
        category: Category::Uuid,
        valid: valid_keyed_uuid,
    },
    KeyRule {
        key: "ssid",
        suffixes: &[],
        separators: b":=",
        left: Left::BoundaryOr(b'e'),
        shape: Shape::LineOrToken(SSID_MAX_BYTES),
        category: Category::Ssid,
        valid: valid_ssid,
    },
    KeyRule {
        key: "machine_id",
        suffixes: &[],
        separators: b":=",
        left: Left::Any,
        shape: Shape::Token,
        category: Category::MachineId,
        valid: valid_machine_id,
    },
    KeyRule {
        key: "machine-id",
        suffixes: &[],
        separators: b":=",
        left: Left::Any,
        shape: Shape::Token,
        category: Category::MachineId,
        valid: valid_machine_id,
    },
    secret_rule("password", Left::Any),
    secret_rule("passwd", Left::Any),
    secret_rule("passphrase", Left::Any),
    secret_rule("psk", Left::Boundary),
    secret_rule("token", Left::Any),
    secret_rule("secret", Left::Any),
    secret_rule("api_key", Left::Any),
    secret_rule("api-key", Left::Any),
    secret_rule("apikey", Left::Any),
    KeyRule {
        key: "authorization",
        suffixes: &[],
        separators: b":=",
        left: Left::Any,
        shape: Shape::Line,
        category: Category::Secret,
        valid: valid_secret,
    },
];

/// Replaces identifiers with stable pseudonyms and counts what it replaced.
#[derive(Debug, Clone)]
pub struct Redactor {
    profile: Profile,
    literals: Vec<Literal>,
    /// Lower-case UUIDs that stay readable.
    preserved: BTreeSet<String>,
    /// The pseudonym number of every value replaced so far.
    assigned: BTreeMap<(Category, String), u64>,
    distinct: BTreeMap<Category, u64>,
    replacements: BTreeMap<Category, u64>,
}

impl Redactor {
    /// Creates a redactor for `profile` that already knows `known`.
    pub fn new(profile: Profile, known: &KnownIdentifiers) -> Self {
        let mut preserved: BTreeSet<String> = WELL_KNOWN_UUIDS
            .iter()
            .map(|uuid| (*uuid).to_owned())
            .collect();
        for id in &known.preserved_ids {
            preserved.insert(normalise_key(Category::Uuid, id.trim()));
        }
        let mut redactor = Redactor {
            profile,
            literals: Vec::new(),
            preserved,
            assigned: BTreeMap::new(),
            distinct: BTreeMap::new(),
            replacements: BTreeMap::new(),
        };
        for name in &known.usernames {
            redactor.add_literal(Category::Username, name);
        }
        for name in &known.hostnames {
            redactor.add_hostname(name);
        }
        for id in &known.machine_ids {
            redactor.add_machine_id(id);
        }
        redactor
    }

    /// The profile this redactor applies.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Remembers `value` so that it is replaced wherever it stands as a word.
    fn add_literal(&mut self, category: Category, value: &str) {
        let value = value.trim();
        if !self.profile.redacts(category)
            || value.len() < category.min_literal_len()
            || is_generic(category, value)
        {
            return;
        }
        let fold_case = matches!(
            category,
            Category::Hostname | Category::Uuid | Category::MachineId
        );
        let needle = if fold_case {
            value.to_ascii_lowercase()
        } else {
            value.to_owned()
        };
        let known = self
            .literals
            .iter()
            .any(|literal| literal.category == category && literal.needle == needle);
        if !known {
            self.literals.push(Literal {
                category,
                needle,
                fold_case,
            });
        }
    }

    /// Remembers a host name, and its first label when it is fully qualified.
    fn add_hostname(&mut self, name: &str) {
        let name = name.trim();
        self.add_literal(Category::Hostname, name);
        if let Some((label, _)) = name.split_once('.') {
            self.add_literal(Category::Hostname, label);
        }
    }

    /// Remembers a machine ID in both its plain and its dashed spelling.
    fn add_machine_id(&mut self, id: &str) {
        let id = id.trim();
        if !valid_machine_id(id) {
            return;
        }
        self.add_literal(Category::MachineId, id);
        let dashed = format!(
            "{}-{}-{}-{}-{}",
            &id[..8],
            &id[8..12],
            &id[12..16],
            &id[16..20],
            &id[20..]
        );
        self.add_literal(Category::MachineId, &dashed);
    }

    /// First pass: remembers the values in `text` that are recognisable only
    /// in context, so that [`Redactor::redact`] finds them out of context too.
    pub fn learn(&mut self, text: &str) {
        let lower = text.to_ascii_lowercase();
        let mut spans = Vec::new();
        self.find_in_context(text, &lower, &mut spans);
        for span in spans {
            let value = &text[span.start..span.end];
            // A UUID in standard form is found by its shape wherever it is.
            let found_by_shape = span.category == Category::Uuid
                && uuid_at(value.as_bytes(), 0) == Some(value.len());
            if !found_by_shape {
                self.add_literal(span.category, value);
            }
        }
    }

    /// Second pass: returns `text` with every identifier replaced.
    pub fn redact(&mut self, text: &str) -> String {
        let spans = resolve(self.scan(text));
        if spans.is_empty() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for span in spans {
            out.push_str(&text[cursor..span.start]);
            let pseudonym = self.pseudonym(span.category, &span.key);
            out.push_str(&pseudonym);
            cursor = span.end;
        }
        out.push_str(&text[cursor..]);
        out
    }

    /// First pass over a whole file of the given format.
    pub fn learn_document(&mut self, format: Format, text: &str) {
        match format {
            Format::Text => self.learn(text),
            Format::JsonLines => {
                for line in text.lines() {
                    match serde_json::from_str::<Value>(line) {
                        Ok(value) => self.learn_value(None, &value),
                        Err(_) => self.learn(line),
                    }
                }
            }
        }
    }

    /// Second pass over a whole file of the given format.
    ///
    /// A [`Format::JsonLines`] file comes back with one compact JSON document
    /// per line and object keys in sorted order. Byte arrays, which
    /// `journalctl` writes for messages that are not plain text, are decoded
    /// to strings so that they are redacted like any other message. A line
    /// that is not valid JSON is redacted as plain text.
    pub fn redact_document(&mut self, format: Format, text: &str) -> String {
        match format {
            Format::Text => self.redact(text),
            Format::JsonLines => {
                let mut out = String::with_capacity(text.len());
                for line in text.lines() {
                    match serde_json::from_str::<Value>(line) {
                        Ok(mut value) => {
                            self.redact_value(None, &mut value);
                            out.push_str(&value.to_string());
                        }
                        Err(_) => out.push_str(&self.redact(line)),
                    }
                    out.push('\n');
                }
                out
            }
        }
    }

    /// The counts of what has been replaced so far.
    pub fn report(&self) -> RedactionReport {
        let categories = Category::ALL
            .into_iter()
            .map(|category| {
                let report = CategoryReport {
                    redacted: self.profile.redacts(category),
                    pseudonym_prefix: category.tag().to_owned(),
                    distinct_values: self.distinct.get(&category).copied().unwrap_or(0),
                    replacements: self.replacements.get(&category).copied().unwrap_or(0),
                };
                (category.name().to_owned(), report)
            })
            .collect();
        RedactionReport {
            schema_version: REDACTION_REPORT_SCHEMA_VERSION,
            profile: self.profile,
            categories,
        }
    }

    fn learn_value(&mut self, key: Option<&str>, value: &Value) {
        match value {
            Value::String(text) => self.learn_field(key, text),
            Value::Array(items) => match decode_byte_array(items) {
                Some(decoded) => self.learn_field(key, &decoded),
                None => {
                    for item in items {
                        self.learn_value(key, item);
                    }
                }
            },
            Value::Object(map) => {
                for (name, item) in map {
                    self.learn_value(Some(name.as_str()), item);
                }
            }
            _ => {}
        }
    }

    fn learn_field(&mut self, key: Option<&str>, text: &str) {
        match key {
            Some("_HOSTNAME") => self.add_hostname(text),
            Some("_MACHINE_ID") => self.add_machine_id(text),
            _ => self.learn(text),
        }
    }

    fn redact_value(&mut self, key: Option<&str>, value: &mut Value) {
        let decoded = match &*value {
            Value::Array(items) => decode_byte_array(items),
            _ => None,
        };
        if let Some(decoded) = decoded {
            *value = Value::String(self.redact_field(key, &decoded));
            return;
        }
        match value {
            Value::String(text) => {
                let redacted = self.redact_field(key, text);
                *text = redacted;
            }
            Value::Array(items) => {
                for item in items {
                    self.redact_value(key, item);
                }
            }
            Value::Object(map) => {
                for (name, item) in map.iter_mut() {
                    self.redact_value(Some(name.as_str()), item);
                }
            }
            _ => {}
        }
    }

    /// Redacts one decoded JSON string. Journal fields that hold nothing but
    /// an identifier are replaced whole, whatever their content looks like.
    fn redact_field(&mut self, key: Option<&str>, text: &str) -> String {
        let whole = match key {
            Some("_HOSTNAME") => Some(Category::Hostname),
            Some("_MACHINE_ID") => Some(Category::MachineId),
            _ => None,
        };
        match whole {
            Some(category) if !text.trim().is_empty() && !is_generic(category, text.trim()) => {
                let key = normalise_key(category, text.trim());
                self.pseudonym(category, &key)
            }
            _ => self.redact(text),
        }
    }

    /// The pseudonym for `key`, allocating the next number on first sight.
    fn pseudonym(&mut self, category: Category, key: &str) -> String {
        *self.replacements.entry(category).or_insert(0) += 1;
        let next = self.distinct.get(&category).copied().unwrap_or(0) + 1;
        let index = *self
            .assigned
            .entry((category, key.to_owned()))
            .or_insert(next);
        if index == next {
            self.distinct.insert(category, next);
        }
        let tag = category.tag();
        format!("<{tag}-{index}>")
    }

    /// Every candidate replacement in `text`, overlaps included.
    fn scan(&self, text: &str) -> Vec<Span> {
        let lower = text.to_ascii_lowercase();
        let mut spans = Vec::new();
        self.find_literals(text, &lower, &mut spans);
        self.find_in_context(text, &lower, &mut spans);
        if self.profile.redacts(Category::Mac) {
            find_macs(text, &mut spans);
        }
        if self.profile.redacts(Category::Uuid) {
            self.find_uuids(text, &mut spans);
        }
        find_ipv4(text, &mut spans);
        self.find_ipv6(text, &mut spans);
        spans
    }

    fn find_literals(&self, text: &str, lower: &str, spans: &mut Vec<Span>) {
        let bytes = text.as_bytes();
        for literal in &self.literals {
            let haystack = if literal.fold_case { lower } else { text };
            for (start, matched) in haystack.match_indices(literal.needle.as_str()) {
                let end = start + matched.len();
                let before = start == 0 || !is_word_byte(bytes[start - 1]);
                let after = end == bytes.len() || !is_word_byte(bytes[end]);
                if before && after {
                    spans.push(Span::new(start, end, literal.category, &text[start..end]));
                }
            }
        }
    }

    /// Values recognisable by what stands before them.
    fn find_in_context(&self, text: &str, lower: &str, spans: &mut Vec<Span>) {
        find_user_paths(text, spans);
        find_uid_mentions(text, spans);
        for rule in KEY_RULES {
            if self.profile.redacts(rule.category) {
                self.find_keyed(text, lower, rule, spans);
            }
        }
    }

    fn find_keyed(&self, text: &str, lower: &str, rule: &KeyRule, spans: &mut Vec<Span>) {
        let bytes = lower.as_bytes();
        for (position, _) in lower.match_indices(rule.key) {
            if !left_allows(rule.left, bytes, position) {
                continue;
            }
            let Some((start, end)) = keyed_value(bytes, position + rule.key.len(), rule) else {
                continue;
            };
            let value = &text[start..end];
            if !(rule.valid)(value) {
                continue;
            }
            let span = Span::new(start, end, rule.category, value);
            if rule.category == Category::Uuid && self.preserved.contains(&span.key) {
                continue;
            }
            spans.push(span);
        }
    }

    fn find_uuids(&self, text: &str, spans: &mut Vec<Span>) {
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match uuid_at(bytes, index) {
                Some(end) => {
                    let span = Span::new(index, end, Category::Uuid, &text[index..end]);
                    if span.key != NIL_UUID && !self.preserved.contains(&span.key) {
                        spans.push(span);
                    }
                    index = end;
                }
                None => index += 1,
            }
        }
    }

    fn find_ipv6(&self, text: &str, spans: &mut Vec<Span>) {
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if !is_ipv6_byte(bytes[index]) {
                index += 1;
                continue;
            }
            let run_start = index;
            while index < bytes.len() && is_ipv6_byte(bytes[index]) {
                index += 1;
            }
            let mut start = run_start;
            let mut end = index;
            // A full stop or a single colon after the address is punctuation.
            while end > start && bytes[end - 1] == b'.' {
                end -= 1;
            }
            if end > start + 1 && bytes[end - 1] == b':' && bytes[end - 2] != b':' {
                end -= 1;
            }
            // A single colon before it is a separator, as in "addr:2a00::1".
            if bytes[start] == b':' && bytes.get(start + 1) != Some(&b':') {
                start += 1;
            }
            if start >= end
                || (start > 0 && is_word_byte(bytes[start - 1]))
                || (end < bytes.len() && is_word_byte(bytes[end]))
            {
                continue;
            }
            let candidate = &text[start..end];
            let colons = candidate.bytes().filter(|byte| *byte == b':').count();
            if colons < 2 || !candidate.bytes().any(|byte| byte.is_ascii_digit()) {
                continue;
            }
            let Ok(address) = candidate.parse::<Ipv6Addr>() else {
                continue;
            };
            if self.ipv6_is_identifying(&address) {
                spans.push(Span::new(start, end, Category::Ipv6, &address.to_string()));
            }
        }
    }

    /// Whether an IPv6 address says something about this machine or network.
    fn ipv6_is_identifying(&self, address: &Ipv6Addr) -> bool {
        if address.is_unspecified() || address.is_loopback() || address.is_multicast() {
            return false;
        }
        if let Some(mapped) = address.to_ipv4_mapped() {
            return ipv4_is_public(mapped.octets());
        }
        let segments = address.segments();
        let link_local = (segments[0] & 0xffc0) == 0xfe80;
        let unique_local = (segments[0] & 0xfe00) == 0xfc00;
        if link_local || unique_local {
            // Readable, unless the interface identifier is a MAC address in
            // EUI-64 form (xxxx:xxff:fexx:xxxx) and MAC addresses are redacted.
            let embeds_mac = (segments[5] & 0x00ff) == 0x00ff && (segments[6] & 0xff00) == 0xfe00;
            return embeds_mac && self.profile.redacts(Category::Mac);
        }
        let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
        !documentation
    }
}

/// Puts a value in the form in which equal values compare equal.
fn normalise_key(category: Category, raw: &str) -> String {
    match category {
        // The short and the fully qualified name of a machine are one value.
        Category::Hostname => {
            let lower = raw.to_ascii_lowercase();
            match lower.split_once('.') {
                Some((label, _)) if !label.is_empty() => label.to_owned(),
                _ => lower,
            }
        }
        Category::Mac => raw.to_ascii_lowercase().replace('-', ":"),
        Category::Uuid => raw.to_ascii_lowercase().replace("\\x2d", "-"),
        Category::MachineId => raw.to_ascii_lowercase().replace('-', ""),
        _ => raw.to_owned(),
    }
}

/// Values that look like an identifier of the category but identify nobody.
fn is_generic(category: Category, value: &str) -> bool {
    match category {
        Category::Username => matches!(value, "root" | "nobody"),
        Category::Hostname => matches!(
            value.to_ascii_lowercase().as_str(),
            "localhost" | "localhost.localdomain" | "localhost6" | "fedora" | "linux" | "(none)"
        ),
        _ => false,
    }
}

/// Whether a byte belongs to a word, for whole-word matching. Bytes of
/// non-ASCII characters count as word bytes.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Sorts candidate replacements and removes overlaps, never leaving part of a
/// candidate unreplaced: a candidate inside another is dropped, and one that
/// starts inside another is cut to begin where the other ends.
fn resolve(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then(b.end.cmp(&a.end))
            .then(a.category.rank().cmp(&b.category.rank()))
    });
    let mut resolved: Vec<Span> = Vec::with_capacity(spans.len());
    let mut covered = 0;
    for mut span in spans {
        if span.end <= covered {
            continue;
        }
        if span.start < covered {
            span.start = covered;
        }
        covered = span.end;
        resolved.push(span);
    }
    resolved
}

/// Decodes a JSON array of byte values, as `journalctl -o json` writes for
/// fields that are not valid or printable UTF-8.
fn decode_byte_array(items: &[Value]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        let byte = item.as_u64().and_then(|number| u8::try_from(number).ok())?;
        bytes.push(byte);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_user_name_end(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'/' | b'"'
                | b'\''
                | b':'
                | b','
                | b';'
                | b'('
                | b')'
                | b'<'
                | b'>'
                | b'|'
                | b'\\'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'='
                | b'*'
                | b'?'
                | b'&'
        )
}

/// User names as the path component after `/home/` (which also covers
/// `/var/home/`) and `/run/media/`.
fn find_user_paths(text: &str, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    for &prefix in USER_PATH_PREFIXES {
        for (position, _) in text.match_indices(prefix) {
            let start = position + prefix.len();
            if !bytes.get(start).is_some_and(|byte| is_word_byte(*byte)) {
                continue;
            }
            let mut end = start;
            while end < bytes.len() && !is_user_name_end(bytes[end]) {
                end += 1;
            }
            while end > start && bytes[end - 1] == b'.' {
                end -= 1;
            }
            let name = &text[start..end];
            if !name.is_empty() && !is_generic(Category::Username, name) {
                spans.push(Span::new(start, end, Category::Username, name));
            }
        }
    }
}

/// User names as written by PAM and others: `alice(uid=1000)`. Only UIDs of
/// ordinary accounts count; system accounts are the same on every machine.
fn find_uid_mentions(text: &str, spans: &mut Vec<Span>) {
    const MARKER: &str = "(uid=";
    let bytes = text.as_bytes();
    for (position, _) in text.match_indices(MARKER) {
        let digits_start = position + MARKER.len();
        let mut digits_end = digits_start;
        while digits_end < bytes.len() && bytes[digits_end].is_ascii_digit() {
            digits_end += 1;
        }
        if bytes.get(digits_end) != Some(&b')') {
            continue;
        }
        let Ok(uid) = text[digits_start..digits_end].parse::<u32>() else {
            continue;
        };
        if !(1000..60_000).contains(&uid) {
            continue;
        }
        let mut start = position;
        while start > 0
            && (is_word_byte(bytes[start - 1]) || matches!(bytes[start - 1], b'.' | b'-'))
        {
            start -= 1;
        }
        let name = &text[start..position];
        if !name.is_empty() && !is_generic(Category::Username, name) {
            spans.push(Span::new(start, position, Category::Username, name));
        }
    }
}

fn left_allows(left: Left, bytes: &[u8], position: usize) -> bool {
    let Some(previous) = position.checked_sub(1).map(|index| bytes[index]) else {
        return true;
    };
    match left {
        Left::Any => true,
        Left::Boundary => !previous.is_ascii_alphanumeric(),
        Left::BoundaryOr(letter) => !previous.is_ascii_alphanumeric() || previous == letter,
    }
}

fn skip_blanks(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    index
}

fn is_value_end(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b',' | b';' | b')' | b']' | b'}' | b'"' | b'\'' | b'<' | b'>' | b'|'
        )
}

/// Finds the value that follows a label ending at `index`.
///
/// Accepts `label: value`, `label=value`, `"label": "value"`, and a quoted
/// value directly after the label (`SSID 'name'`). Returns the byte range of
/// the value without its quotes.
fn keyed_value(bytes: &[u8], mut index: usize, rule: &KeyRule) -> Option<(usize, usize)> {
    let len = bytes.len();
    for suffix in rule.suffixes {
        if bytes[index..].starts_with(suffix.as_bytes()) {
            index += suffix.len();
            break;
        }
    }
    // The closing quote of a quoted label, as in JSON.
    if index < len && matches!(bytes[index], b'"' | b'\'') {
        index += 1;
    }
    index = skip_blanks(bytes, index);
    let separated = index < len && rule.separators.contains(&bytes[index]);
    if separated {
        index = skip_blanks(bytes, index + 1);
    }
    if index >= len {
        return None;
    }

    let quote = bytes[index];
    if quote == b'"' || quote == b'\'' {
        let start = index + 1;
        let mut end = start;
        while end < len && bytes[end] != quote && bytes[end] != b'\n' {
            end += 1;
        }
        if end >= len || bytes[end] != quote {
            return None;
        }
        return (end > start).then_some((start, end));
    }
    if !separated {
        return None;
    }

    let start = index;
    let mut token_end = start;
    while token_end < len && !is_value_end(bytes[token_end]) {
        token_end += 1;
    }
    let mut line_end = start;
    while line_end < len && !matches!(bytes[line_end], b'\n' | b'\r') {
        line_end += 1;
    }
    while line_end > start && matches!(bytes[line_end - 1], b' ' | b'\t') {
        line_end -= 1;
    }
    let end = match rule.shape {
        Shape::Token => token_end,
        Shape::Line => line_end,
        Shape::LineOrToken(limit) => {
            if line_end - start <= limit {
                line_end
            } else {
                token_end
            }
        }
    };
    (end > start).then_some((start, end))
}

fn valid_serial(value: &str) -> bool {
    (4..=64).contains(&value.len())
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_keyed_uuid(value: &str) -> bool {
    (8..=40).contains(&value.len())
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn valid_ssid(value: &str) -> bool {
    value.len() <= 64
        && !matches!(
            value,
            "" | "--" | "*" | "any" | "off/any" | "(null)" | "(hidden)"
        )
}

fn valid_machine_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_secret(value: &str) -> bool {
    !value.bytes().all(|byte| byte == b'*')
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "none" | "(null)" | "true" | "false" | "yes" | "no" | "required" | "optional"
        )
}

fn is_hex_pair(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_hexdigit())
        && bytes
            .get(index + 1)
            .is_some_and(|byte| byte.is_ascii_hexdigit())
}

/// If a MAC-like address starts at `start`, returns where it ends.
///
/// A MAC-like address is six or more pairs of hexadecimal digits separated
/// consistently by `:` or by `-`. Longer runs (EUI-64, InfiniBand, DUID)
/// embed or are hardware addresses and are treated the same.
fn mac_at(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if !is_hex_pair(bytes, start) {
        return None;
    }
    let separator = *bytes.get(start + 2)?;
    if separator != b':' && separator != b'-' {
        return None;
    }
    let mut groups = 1;
    let mut end = start + 2;
    while bytes.get(end) == Some(&separator)
        && is_hex_pair(bytes, end + 1)
        && !bytes
            .get(end + 3)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        groups += 1;
        end += 3;
    }
    if groups < 6 {
        return None;
    }
    Some(end)
}

/// Addresses that every machine has: all zeros and broadcast.
fn is_placeholder_mac(key: &str) -> bool {
    key.split(':').all(|group| group == "00") || key.split(':').all(|group| group == "ff")
}

fn find_macs(text: &str, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match mac_at(bytes, index) {
            Some(end) => {
                let span = Span::new(index, end, Category::Mac, &text[index..end]);
                if !is_placeholder_mac(&span.key) {
                    spans.push(span);
                }
                index = end;
            }
            None => index += 1,
        }
    }
}

/// If a UUID starts at `start`, returns where it ends. The groups may be
/// joined by `-` or by `\x2d`, which is how systemd escapes `-` in unit names.
fn uuid_at(bytes: &[u8], start: usize) -> Option<usize> {
    const ESCAPED_DASH: &[u8] = b"\\x2d";
    if start > 0
        && bytes[start - 1].is_ascii_alphanumeric()
        && !bytes[..start].ends_with(ESCAPED_DASH)
    {
        return None;
    }
    let mut position = start;
    for (index, digits) in [8_usize, 4, 4, 4, 12].into_iter().enumerate() {
        if index > 0 {
            if bytes.get(position) == Some(&b'-') {
                position += 1;
            } else if bytes[position.min(bytes.len())..].starts_with(ESCAPED_DASH) {
                position += ESCAPED_DASH.len();
            } else {
                return None;
            }
        }
        for _ in 0..digits {
            if !bytes
                .get(position)
                .is_some_and(|byte| byte.is_ascii_hexdigit())
            {
                return None;
            }
            position += 1;
        }
    }
    if bytes
        .get(position)
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(position)
}

/// If a dotted-quad IPv4 address starts at `start`, returns where it ends and
/// its octets.
///
/// Version numbers look like addresses. A candidate is rejected when it is
/// glued to a word, a `-`, a `_` or a `.` on the left (`v1.2.3.4`,
/// `pkg-1.2.3.4`), has a fifth dotted part (`1.2.3.4.5`, `1.2.3.4.fc45`), is
/// followed by `-` and a letter or digit (`1.2.3.4-5`), or has an octet with
/// a leading zero. The two ends of an address range (`1.2.3.4-1.2.3.9`) are
/// both candidates.
fn ipv4_at(bytes: &[u8], start: usize) -> Option<(usize, [u8; 4])> {
    if start > 0 {
        let previous = bytes[start - 1];
        let attached = previous.is_ascii_alphanumeric() || matches!(previous, b'.' | b'_' | b'-');
        // After "-", only the end of an address range ("1.2.3.4-1.2.3.9") counts.
        let range_end = previous == b'-' && start >= 2 && bytes[start - 2].is_ascii_digit();
        if attached && !range_end {
            return None;
        }
    }
    let mut octets = [0_u8; 4];
    let mut position = start;
    for (index, octet) in octets.iter_mut().enumerate() {
        if index > 0 {
            if bytes.get(position) != Some(&b'.') {
                return None;
            }
            position += 1;
        }
        let digits_start = position;
        let mut value: u32 = 0;
        while position - digits_start < 3
            && bytes
                .get(position)
                .is_some_and(|byte| byte.is_ascii_digit())
        {
            value = value * 10 + u32::from(bytes[position] - b'0');
            position += 1;
        }
        let digits = position - digits_start;
        if digits == 0 || (digits > 1 && bytes[digits_start] == b'0') {
            return None;
        }
        *octet = u8::try_from(value).ok()?;
    }
    let next = bytes.get(position).copied();
    let after_next = bytes.get(position + 1).copied();
    let glued = match next {
        Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_' => true,
        Some(b'.') => after_next.is_some_and(|byte| byte.is_ascii_alphanumeric()),
        // "1.2.3.4-5" is a version; "1.2.3.4-1.2.3.9" is a range of addresses.
        Some(b'-') => {
            after_next.is_some_and(|byte| byte.is_ascii_alphanumeric())
                && ipv4_at(bytes, position + 1).is_none()
        }
        _ => false,
    };
    if glued {
        return None;
    }
    Some((position, octets))
}

/// Whether an IPv4 address is globally routable, as opposed to loopback,
/// private, link-local, shared (CGNAT), documentation, benchmarking,
/// multicast, reserved or "this network".
fn ipv4_is_public(octets: [u8; 4]) -> bool {
    let [a, b, c, _] = octets;
    let special = a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113);
    !special
}

fn find_ipv4(text: &str, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match ipv4_at(bytes, index) {
            Some((end, octets)) => {
                if ipv4_is_public(octets) {
                    spans.push(Span::new(index, end, Category::Ipv4, &text[index..end]));
                }
                index = end;
            }
            None => index += 1,
        }
    }
}

fn is_ipv6_byte(byte: u8) -> bool {
    byte.is_ascii_hexdigit() || byte == b':' || byte == b'.'
}

#[cfg(test)]
mod tests;

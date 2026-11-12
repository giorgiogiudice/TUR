// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The receiver's own removal of personal data: the second pass, written
//! independently of `tur-diag`'s redactor so that a gap in one is not
//! automatically a gap in the other.
//!
//! It works on every text that goes into a summary. It first *learns* from
//! the whole bundle the values that can be recognised only by their label
//! (a user name in a home path, a host name after `hostname=`, a Wi-Fi name
//! after `SSID`, a serial number after `Serial Number:`), then *scrubs* a
//! text by applying the rules below in order and finally replacing every
//! learnt value wherever it stands as a whole word.
//!
//! | Rule | Replaced by |
//! |---|---|
//! | PEM private key blocks | `<private-key>` |
//! | SSH public keys with their comment | `<ssh-key>` |
//! | Credentials in a URL or share (`scheme://user:pass@`, `//user:pass@`) | `<credentials>` |
//! | Known token formats (GitHub, Anthropic, OpenAI, Google, AWS, Slack, GitLab, npm) and JSON web tokens | `<token>` |
//! | `Authorization:` lines; `Bearer`/`Basic` values | `<secret>` |
//! | Values after keys naming a password, passphrase, PSK, secret, token, API or access key, credential, session ID or cookie | `<secret>` |
//! | E-mail addresses (not systemd unit names such as `user@1000.service`) | `<email>` |
//! | `user@host` without a domain | `<user>@<host>` |
//! | Home paths: `/home/`, `/var/home/`, `/Users/`, `/run/media/`, `/media/` | the name becomes `<user>` |
//! | Windows profile paths: `C:\Users\`, `drive_c/users/` (except Public and Default) | the name becomes `<user>` |
//! | User names after `USER=`, `user=`, `login=`, `uid=N(`, `for user`, `Accepted ... for` | `<user>` |
//! | Host names after `hostname`, `_HOSTNAME`, `Static hostname`, `nodename`, in `uname` output, and names in `.local`, `.lan`, `.home`, `.internal`, `.localdomain`, `.home.arpa`, `.fritz.box` | `<host>` |
//! | Wi-Fi names after `SSID`/`ESSID`, and NetworkManager's `access point '...'`, `connection '...'` | `<wifi>` |
//! | Serial numbers after a serial label or `wwn`, if they contain a digit | `<serial>` |
//! | MAC addresses: six to eight hexadecimal pairs with `:` or `-`, dotted `xxxx.xxxx.xxxx`, and twelve digits after a MAC label | `<mac>` |
//! | IPv6 addresses (not `::` and `::1`) | `<ipv6>` |
//! | IPv4 addresses (not `127.0.0.0/8`, `0.0.0.0`, `255.255.255.255`, and not four-part numbers inside a longer dotted version) | `<ipv4>` |
//! | UUIDs, also in systemd's `\x2d` spelling | `<uuid>` |
//! | 32 hexadecimal digits standing alone (machine and boot IDs) | `<id>` |
//! | 13 to 19 digits that pass the Luhn check (card numbers) | `<card>` |
//! | Runs of 32 or more letters, digits and `+/_=-` mixing upper case, lower case and digits | `<token>` |
//! | Every learnt user, host, Wi-Fi and serial value, as a whole word, ignoring case | its tag |
//!
//! Values that are already pseudonyms of the client (`<user-1>`) are left
//! as they are. When a rule has to guess, it guesses towards removing.

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::sync::LazyLock;

use regex::{Captures, Regex};

/// What a learnt value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tag {
    /// A user name.
    User,
    /// A host name.
    Host,
    /// A Wi-Fi network name.
    Wifi,
    /// A serial number.
    Serial,
}

impl Tag {
    /// The text that replaces a value of this kind.
    pub const fn text(self) -> &'static str {
        match self {
            Tag::User => "<user>",
            Tag::Host => "<host>",
            Tag::Wifi => "<wifi>",
            Tag::Serial => "<serial>",
        }
    }

    const fn min_len(self) -> usize {
        match self {
            Tag::User | Tag::Host => 3,
            Tag::Wifi => 4,
            Tag::Serial => 6,
        }
    }
}

/// Names that are not personal and must stay readable.
const GENERIC: &[&str] = &[
    "admin",
    "all users",
    "android",
    "audio",
    "avahi",
    "bin",
    "card",
    "chrony",
    "colord",
    "credentials",
    "daemon",
    "dbus",
    "default",
    "default user",
    "dnsmasq",
    "email",
    "fedora",
    "flatpak",
    "gdm",
    "geoclue",
    "guest",
    "home",
    "host",
    "id",
    "invalid",
    "ipv4",
    "ipv6",
    "kinoite",
    "linux",
    "localhost",
    "localhost.localdomain",
    "lp",
    "mac",
    "mail",
    "network",
    "nobody",
    "none",
    "null",
    "pipewire",
    "polkitd",
    "private-key",
    "public",
    "root",
    "rpc",
    "rtkit",
    "sddm",
    "secret",
    "serial",
    "ssh-key",
    "sshd",
    "sssd",
    "system",
    "systemd",
    "token",
    "tss",
    "tur",
    "unknown",
    "user",
    "users",
    "uuid",
    "version",
    "wifi",
];

/// The prefixes of systemd template units, which look like `user@host`.
const UNIT_PREFIXES: &[&str] = &[
    "autovt",
    "container-getty",
    "dbus",
    "getty",
    "modprobe",
    "serial-getty",
    "systemd-fsck",
    "user",
    "user-runtime-dir",
];

/// The suffixes of systemd unit names.
const UNIT_SUFFIXES: &[&str] = &[
    ".service",
    ".socket",
    ".target",
    ".timer",
    ".mount",
    ".automount",
    ".path",
    ".slice",
    ".scope",
    ".device",
    ".swap",
];

/// How many learnt values go into one pattern.
const LITERAL_CHUNK: usize = 200;

fn regex(pattern: &str) -> Regex {
    // Every pattern is a constant of this file; the tests compile them all.
    Regex::new(pattern).unwrap_or_else(|error| panic!("pattern {pattern}: {error}"))
}

macro_rules! rule {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| regex($pattern));
    };
}

rule!(
    PEM,
    r"(?s)-----BEGIN[A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END[A-Z0-9 ]*PRIVATE KEY-----|\z)"
);
rule!(
    SSH_KEY,
    r"(?:ssh-(?:rsa|dss|ed25519)|ecdsa-sha2-nistp[0-9]+|sk-(?:ssh-ed25519|ecdsa-sha2-nistp256)@openssh\.com)\s+AAAA[0-9A-Za-z+/]+=*(?:[ \t]+[^\s]+@[^\s]+)?"
);
rule!(
    URL_CREDENTIALS,
    r"((?:[A-Za-z][A-Za-z0-9+.-]*:)?//)[^/\s:@<>]+(?::[^/\s@<>]*)?@"
);
rule!(
    KNOWN_TOKEN,
    r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,}|sk-ant-[A-Za-z0-9_-]{16,}|sk-(?:proj-)?[A-Za-z0-9_-]{20,}|AIza[0-9A-Za-z_-]{30,}|(?:AKIA|ASIA)[0-9A-Z]{16}|xox[abposr]-[A-Za-z0-9-]{10,}|glpat-[A-Za-z0-9_-]{20,}|npm_[A-Za-z0-9]{30,})"
);
rule!(
    JWT,
    r"\beyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}"
);
rule!(
    AUTHORIZATION,
    r"(?im)\b((?:proxy-)?authorization)(\s*[:=]\s*)[^\n]*"
);
rule!(BEARER, r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}");
rule!(
    LABELLED_SECRET,
    r#"(?i)([A-Za-z0-9_.-]*(?:password|passwd|passphrase|psk|secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|credentials?|session[_-]?id|cookie)[A-Za-z0-9_.-]*)(["']?\s*[:=]\s*)("[^"\n]*"|'[^'\n]*'|[^\s,;&"'<>]+)"#
);
rule!(
    EMAIL,
    r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,})"
);
rule!(
    USER_AT_HOST,
    r"\b([a-z_][a-z0-9_.-]{0,31})@([A-Za-z0-9][A-Za-z0-9-]{0,62})\b"
);
rule!(
    HOME_PATH,
    r#"(/(?:var/)?home/|/Users/|/run/media/|/media/)([^/\s"':<>]+)"#
);
rule!(
    WINDOWS_PROFILE,
    r#"(?i)([A-Z]:[\\/]+Users[\\/]+|drive_c[\\/]+users[\\/]+)([^\\/"':<>\n]+)"#
);
rule!(
    USER_ASSIGNMENT,
    r#"(?i)\b(user|username|login|logname|sudo_user|owner)(\s*=\s*["']?|["']\s*:\s*["']|:\s+["']?)([A-Za-z_][A-Za-z0-9._-]{0,31})"#
);
rule!(UID_NAME, r"(uid=[0-9]+\()([A-Za-z_][A-Za-z0-9._-]{0,31})\)");
rule!(
    USER_PHRASE,
    r"(?i)\b((?:for|of) (?:invalid )?user|(?:accepted|failed) [a-z-]+ for(?: invalid user)?)\s+'?([A-Za-z_][A-Za-z0-9._-]{0,31})"
);
rule!(
    HOST_ASSIGNMENT,
    r#"(?i)(\b_hostname|\bhostname|\bhost name|\bstatic hostname|\btransient hostname|\bnodename)(["']?\s*[:=]\s*["']?)([A-Za-z0-9][A-Za-z0-9.-]{0,252})"#
);
rule!(
    UNAME,
    r"\b(Linux)\s+([A-Za-z0-9][A-Za-z0-9.-]{0,62})\s+([0-9]+\.[0-9]+)"
);
rule!(
    LOCAL_HOST,
    r"(?i)\b[A-Za-z0-9][A-Za-z0-9-]*(?:\.[A-Za-z0-9-]+)*\.(?:local|lan|home|internal|localdomain|home\.arpa|fritz\.box)\b"
);
rule!(
    SSID,
    r#"(?i)\b(e?ssid)(\s*[:=]\s*)("[^"\n]{1,32}"|'[^'\n]{1,32}'|[^\s"',;<>]{1,32})"#
);
rule!(
    SSID_QUOTED,
    r#"(?i)\b(e?ssid)(\s+)("[^"\n]{1,32}"|'[^'\n]{1,32}')"#
);
rule!(
    NM_NAME,
    r"(?i)\b(access point|connection|network) '([^'\n]{1,64})'"
);
rule!(
    SERIAL,
    r#"(?i)\b(serial[ _-]?(?:number|no\.?)?|s/n|sn|iserial|serialnumber|id_serial(?:_short)?|product_serial|board_serial|chassis_serial|wwn)(["']?\s*[:=]\s*["']?|\s+)([A-Za-z0-9][A-Za-z0-9._-]{3,63})"#
);
rule!(MAC, r"\b[0-9A-Fa-f]{2}(?:[:-][0-9A-Fa-f]{2}){5,7}\b");
rule!(
    MAC_DOTTED,
    r"\b[0-9A-Fa-f]{4}\.[0-9A-Fa-f]{4}\.[0-9A-Fa-f]{4}\b"
);
rule!(
    MAC_BARE,
    r"(?i)\b(mac|hwaddr|ether|bssid|lladdr|mac address)(\s*[:=]?\s*)([0-9A-Fa-f]{12})\b"
);
rule!(
    IPV6,
    r"(?i)[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}(?::[0-9]{1,3}(?:\.[0-9]{1,3}){3})?(?:%[A-Za-z0-9_.-]+)?"
);
rule!(
    IPV4,
    r"\b([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})\b"
);
rule!(
    UUID,
    r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b"
);
rule!(
    UUID_ESCAPED,
    r"(?i)[0-9a-f]{8}\\x2d[0-9a-f]{4}\\x2d[0-9a-f]{4}\\x2d[0-9a-f]{4}\\x2d[0-9a-f]{12}"
);
rule!(HEX_ID, r"(?i)\b[0-9a-f]{32}\b");
rule!(CARD, r"\b[0-9](?:[ -]?[0-9]){12,18}\b");
rule!(HIGH_ENTROPY, r"[A-Za-z0-9+/_=-]{32,}");

/// Every rule, for the test that compiles them all.
#[cfg(test)]
fn all_rules() -> Vec<&'static Regex> {
    vec![
        &*PEM,
        &*SSH_KEY,
        &*URL_CREDENTIALS,
        &*KNOWN_TOKEN,
        &*JWT,
        &*AUTHORIZATION,
        &*BEARER,
        &*LABELLED_SECRET,
        &*EMAIL,
        &*USER_AT_HOST,
        &*HOME_PATH,
        &*WINDOWS_PROFILE,
        &*USER_ASSIGNMENT,
        &*UID_NAME,
        &*USER_PHRASE,
        &*HOST_ASSIGNMENT,
        &*UNAME,
        &*LOCAL_HOST,
        &*SSID,
        &*SSID_QUOTED,
        &*NM_NAME,
        &*SERIAL,
        &*MAC,
        &*MAC_DOTTED,
        &*MAC_BARE,
        &*IPV6,
        &*IPV4,
        &*UUID,
        &*UUID_ESCAPED,
        &*HEX_ID,
        &*CARD,
        &*HIGH_ENTROPY,
    ]
}

/// Whether `value` is a pseudonym or a tag, which stays.
fn is_placeholder(value: &str) -> bool {
    value.starts_with('<')
}

fn is_generic(value: &str) -> bool {
    GENERIC.contains(&value.to_ascii_lowercase().as_str())
}

fn is_unit_name(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    UNIT_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

fn char_before(text: &str, index: usize) -> Option<char> {
    text[..index].chars().next_back()
}

fn char_after(text: &str, index: usize) -> Option<char> {
    text[index..].chars().next()
}

fn is_word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn strip_quotes(value: &str) -> &str {
    value.trim_matches(|c| c == '"' || c == '\'')
}

fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            if index % 2 == 1 {
                let doubled = digit * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                *digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The second pass of redaction.
#[derive(Debug, Default)]
pub struct Scrubber {
    learnt: BTreeMap<String, Tag>,
    /// The learnt values as patterns, longest first, in chunks; empty when
    /// they have to be built again.
    literal: Vec<Regex>,
    replacements: u64,
}

impl Scrubber {
    /// A scrubber that has learnt nothing yet.
    pub fn new() -> Self {
        Scrubber::default()
    }

    /// How many replacements it has made.
    pub fn replacements(&self) -> u64 {
        self.replacements
    }

    /// How many values it has learnt.
    pub fn learnt(&self) -> usize {
        self.learnt.len()
    }

    fn remember(&mut self, tag: Tag, value: &str) {
        let value = strip_quotes(value.trim());
        if value.chars().count() < tag.min_len()
            || is_placeholder(value)
            || is_generic(value)
            || value.chars().all(|c| c.is_ascii_digit())
        {
            return;
        }
        if tag == Tag::Serial && !value.chars().any(|c| c.is_ascii_digit()) {
            return;
        }
        let key = value.to_lowercase();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.learnt.entry(key) {
            entry.insert(tag);
            self.literal.clear();
        }
    }

    /// Learns the values that can be recognised only by their label.
    pub fn learn(&mut self, text: &str) {
        let mut found: Vec<(Tag, String)> = Vec::new();
        let mut take = |regex: &Regex, group: usize, tag: Tag| {
            for caps in regex.captures_iter(text) {
                if let Some(value) = caps.get(group) {
                    found.push((tag, value.as_str().to_owned()));
                }
            }
        };
        take(&HOME_PATH, 2, Tag::User);
        take(&WINDOWS_PROFILE, 2, Tag::User);
        take(&USER_ASSIGNMENT, 3, Tag::User);
        take(&UID_NAME, 2, Tag::User);
        take(&USER_PHRASE, 2, Tag::User);
        take(&HOST_ASSIGNMENT, 3, Tag::Host);
        take(&UNAME, 2, Tag::Host);
        take(&SSID, 3, Tag::Wifi);
        take(&SSID_QUOTED, 3, Tag::Wifi);
        take(&NM_NAME, 2, Tag::Wifi);
        take(&SERIAL, 3, Tag::Serial);
        for caps in USER_AT_HOST.captures_iter(text) {
            let whole = caps.get(0).map_or(0..0, |m| m.range());
            if char_after(text, whole.end) == Some('.') {
                continue;
            }
            let user = &caps[1];
            if UNIT_PREFIXES.contains(&user) {
                continue;
            }
            found.push((Tag::User, user.to_owned()));
            found.push((Tag::Host, caps[2].to_owned()));
        }
        for (tag, value) in found {
            self.remember(tag, &value);
        }
    }

    /// Applies `regex`, replacing each match with what `replace` returns, or
    /// keeping it when it returns `None`.
    fn apply(
        &mut self,
        regex: &Regex,
        text: &str,
        mut replace: impl FnMut(&Captures<'_>, &str) -> Option<String>,
    ) -> String {
        let mut count = 0u64;
        let result = regex.replace_all(text, |caps: &Captures<'_>| match replace(caps, text) {
            Some(replacement) => {
                count += 1;
                replacement
            }
            None => caps[0].to_owned(),
        });
        let result = result.into_owned();
        self.replacements += count;
        result
    }

    /// The learnt values as patterns: at most [`LITERAL_CHUNK`] values in
    /// each, longest first, so that a longer value is replaced before a
    /// shorter one inside it.
    fn literal_regexes(&mut self) -> Vec<Regex> {
        if self.literal.is_empty() && !self.learnt.is_empty() {
            let mut values: Vec<&String> = self.learnt.keys().collect();
            values.sort_by_key(|value| std::cmp::Reverse(value.len()));
            for chunk in values.chunks(LITERAL_CHUNK) {
                let alternatives: Vec<String> =
                    chunk.iter().map(|value| regex::escape(value)).collect();
                let pattern = format!("(?i)(?:{})", alternatives.join("|"));
                // Escaped literals always compile; the size limit is raised
                // for long lists.
                let compiled = regex::RegexBuilder::new(&pattern)
                    .size_limit(256 * 1024 * 1024)
                    .build()
                    .unwrap_or_else(|error| panic!("learnt values do not compile: {error}"));
                self.literal.push(compiled);
            }
        }
        self.literal.clone()
    }

    /// Removes personal data from `text`.
    pub fn scrub(&mut self, text: &str) -> String {
        let mut out = self.apply(&PEM, text, |_, _| Some("<private-key>".to_owned()));
        out = self.apply(&SSH_KEY, &out, |_, _| Some("<ssh-key>".to_owned()));
        out = self.apply(&URL_CREDENTIALS, &out, |caps, _| {
            Some(format!("{}<credentials>@", &caps[1]))
        });
        out = self.apply(&KNOWN_TOKEN, &out, |_, _| Some("<token>".to_owned()));
        out = self.apply(&JWT, &out, |_, _| Some("<token>".to_owned()));
        out = self.apply(&AUTHORIZATION, &out, |caps, _| {
            Some(format!("{}{}<secret>", &caps[1], &caps[2]))
        });
        out = self.apply(&BEARER, &out, |caps, _| {
            Some(format!("{} <secret>", &caps[1]))
        });
        out = self.apply(&LABELLED_SECRET, &out, |caps, _| {
            if is_unit_name(&caps[1]) || is_placeholder(strip_quotes(&caps[3])) {
                return None;
            }
            let value = &caps[3];
            let quote = if value.starts_with('"') {
                "\""
            } else if value.starts_with('\'') {
                "'"
            } else {
                ""
            };
            Some(format!("{}{}{quote}<secret>{quote}", &caps[1], &caps[2]))
        });
        out = self.apply(&EMAIL, &out, |caps, _| {
            (!is_unit_name(&caps[0])).then(|| "<email>".to_owned())
        });
        out = self.apply(&USER_AT_HOST, &out, |caps, text| {
            let end = caps.get(0).map_or(0, |m| m.end());
            if char_after(text, end) == Some('.') || UNIT_PREFIXES.contains(&&caps[1]) {
                return None;
            }
            Some("<user>@<host>".to_owned())
        });
        out = self.apply(&HOME_PATH, &out, |caps, _| {
            (!is_placeholder(&caps[2])).then(|| format!("{}<user>", &caps[1]))
        });
        out = self.apply(&WINDOWS_PROFILE, &out, |caps, _| {
            let name = caps[2].trim();
            (!is_placeholder(name) && !is_generic(name)).then(|| format!("{}<user>", &caps[1]))
        });
        out = self.apply(&USER_ASSIGNMENT, &out, |caps, _| {
            (!is_generic(&caps[3])).then(|| format!("{}{}<user>", &caps[1], &caps[2]))
        });
        out = self.apply(&UID_NAME, &out, |caps, _| {
            (!is_generic(&caps[2])).then(|| format!("{}<user>)", &caps[1]))
        });
        out = self.apply(&USER_PHRASE, &out, |caps, _| {
            (!is_generic(&caps[2])).then(|| format!("{} <user>", &caps[1]))
        });
        out = self.apply(&HOST_ASSIGNMENT, &out, |caps, _| {
            (!is_generic(&caps[3])).then(|| format!("{}{}<host>", &caps[1], &caps[2]))
        });
        out = self.apply(&UNAME, &out, |caps, _| {
            (!is_generic(&caps[2])).then(|| format!("{} <host> {}", &caps[1], &caps[3]))
        });
        out = self.apply(&LOCAL_HOST, &out, |caps, _| {
            (!is_generic(&caps[0])).then(|| "<host>".to_owned())
        });
        for rule in [&*SSID, &*SSID_QUOTED] {
            out = self.apply(rule, &out, |caps, _| {
                (!is_placeholder(strip_quotes(&caps[3])))
                    .then(|| format!("{}{}<wifi>", &caps[1], &caps[2]))
            });
        }
        out = self.apply(&NM_NAME, &out, |caps, _| {
            (!is_placeholder(&caps[2])).then(|| format!("{} '<wifi>'", &caps[1]))
        });
        out = self.apply(&SERIAL, &out, |caps, _| {
            caps[3]
                .chars()
                .any(|c| c.is_ascii_digit())
                .then(|| format!("{}{}<serial>", &caps[1], &caps[2]))
        });
        out = self.apply(&MAC, &out, |caps, _| {
            let digits: String = caps[0].chars().filter(char::is_ascii_hexdigit).collect();
            let all_same =
                digits.chars().all(|c| c == '0') || digits.chars().all(|c| c == 'f' || c == 'F');
            (!all_same).then(|| "<mac>".to_owned())
        });
        out = self.apply(&MAC_DOTTED, &out, |_, _| Some("<mac>".to_owned()));
        out = self.apply(&MAC_BARE, &out, |caps, _| {
            Some(format!("{}{}<mac>", &caps[1], &caps[2]))
        });
        out = self.apply(&IPV6, &out, |caps, text| {
            let whole = caps.get(0)?;
            let before = char_before(text, whole.start());
            let after = char_after(text, whole.end());
            if is_word_char(before)
                || before == Some(':')
                || is_word_char(after)
                || after == Some(':')
            {
                return None;
            }
            let address = whole.as_str().split('%').next().unwrap_or("");
            let parsed: Ipv6Addr = address.parse().ok()?;
            if parsed.is_loopback() || parsed.is_unspecified() {
                return None;
            }
            Some("<ipv6>".to_owned())
        });
        out = self.apply(&IPV4, &out, |caps, text| {
            let whole = caps.get(0)?;
            let octets: Vec<u16> = (1..=4)
                .filter_map(|group| caps[group].parse().ok())
                .collect();
            if octets.len() != 4 || octets.iter().any(|octet| *octet > 255) {
                return None;
            }
            // Part of a longer dotted version such as 1.2.3.4.5.
            let rest = &text[whole.end()..];
            if rest.starts_with('.') && rest[1..].starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            if char_before(text, whole.start()) == Some('.') {
                return None;
            }
            let keep = octets[0] == 127
                || octets.iter().all(|octet| *octet == 0)
                || octets.iter().all(|octet| *octet == 255);
            (!keep).then(|| "<ipv4>".to_owned())
        });
        out = self.apply(&UUID, &out, |_, _| Some("<uuid>".to_owned()));
        out = self.apply(&UUID_ESCAPED, &out, |_, _| Some("<uuid>".to_owned()));
        out = self.apply(&HEX_ID, &out, |_, _| Some("<id>".to_owned()));
        out = self.apply(&CARD, &out, |caps, _| {
            let digits: Vec<u32> = caps[0].chars().filter_map(|c| c.to_digit(10)).collect();
            ((13..=19).contains(&digits.len()) && luhn(&digits)).then(|| "<card>".to_owned())
        });
        out = self.apply(&HIGH_ENTROPY, &out, |caps, _| {
            let value = &caps[0];
            if value.starts_with('/') {
                // A path, such as a D-Bus object path.
                return None;
            }
            let upper = value.chars().any(|c| c.is_ascii_uppercase());
            let lower = value.chars().any(|c| c.is_ascii_lowercase());
            let digit = value.chars().any(|c| c.is_ascii_digit());
            (upper && lower && digit).then(|| "<token>".to_owned())
        });
        self.replace_learnt(&out)
    }

    fn replace_learnt(&mut self, text: &str) -> String {
        let learnt = self.learnt.clone();
        let mut out = text.to_owned();
        for regex in self.literal_regexes() {
            out = self.apply(&regex, &out, |caps, text| {
                let whole = caps.get(0)?;
                if is_word_char(char_before(text, whole.start()))
                    || is_word_char(char_after(text, whole.end()))
                {
                    return None;
                }
                let tag = learnt.get(&whole.as_str().to_lowercase())?;
                Some(tag.text().to_owned())
            });
        }
        out
    }
}

#[cfg(test)]
mod tests;

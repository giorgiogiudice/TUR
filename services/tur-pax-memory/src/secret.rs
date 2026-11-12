// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The secret detector: a safety net at the point of writing.
//!
//! Addendum A section 16.1: "Secrets are never stored. Passwords, keys and
//! payment details are refused at the point of writing." Every text that is
//! about to be stored passes through [`detect`], and a hit refuses the write
//! with a `UOS-AI-MEMORY-SECRET-*` code.
//!
//! # What it recognises
//!
//! | Kind | Rule |
//! |---|---|
//! | Private key | `-----BEGIN ... PRIVATE KEY-----`, or a PuTTY key file header |
//! | Known token | A word starting with a known vendor prefix followed by enough token characters; a JSON Web Token; `Bearer` followed by a token |
//! | Payment card | 13 to 19 digits, alone or in groups, with a card-scheme prefix and a valid Luhn check digit |
//! | One-time code | A one-time-code word or phrase with a 4 to 8 digit number nearby; an `otpauth://` address |
//! | Password | A password word followed by `:`, `=`, "is" or "was" and a value; a name ending in a password word (`DB_PASSWORD=`); a PIN or card security code followed by digits; `--password value`; `scheme://user:password@host` |
//! | High entropy | A run of 32 or more letters, digits, `_`, `-`, `+` with lower case, upper case and digits and at least 4.5 bits of entropy per character |
//!
//! # What it does not do
//!
//! It is a heuristic over the text, not a guarantee, and it errs in both
//! directions:
//!
//! - **It misses secrets.** A password written without a telling word ("the
//!   door one is hunter2"), in another language, or split across entries is
//!   not seen. A key made only of hexadecimal digits looks exactly like a
//!   checksum or a commit hash and is not flagged unless a word such as
//!   `token:` precedes it. A random token shorter than 32 characters, or one
//!   in a single letter case, or one broken up by `/` is not flagged by the
//!   entropy rule; of random strings of letters and digits, the rule catches
//!   about two in three at 32 characters and nearly all from 40 (measured by
//!   simulation, not proved). Homoglyphs and deliberate obfuscation defeat
//!   it.
//! - **It refuses harmless text.** "The password is located in the manual"
//!   is refused. So is a note that puts a year next to "verification code".
//!   A refusal is the safe direction; the user can reword.
//!
//! The detector never returns, logs or stores the text it matched: only the
//! kind of secret.

use serde_json::Value;

use crate::codes::Code;

/// The kind of secret a text appears to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecretKind {
    /// A password, passphrase, PIN or card security code in an obvious form.
    Password,
    /// An API key or access token of a known form.
    KnownToken,
    /// A long random-looking string.
    HighEntropy,
    /// A private-key block.
    PrivateKey,
    /// A payment-card number.
    PaymentCard,
    /// A one-time code.
    OneTimeCode,
}

impl SecretKind {
    /// Every kind.
    pub const ALL: [SecretKind; 6] = [
        SecretKind::Password,
        SecretKind::KnownToken,
        SecretKind::HighEntropy,
        SecretKind::PrivateKey,
        SecretKind::PaymentCard,
        SecretKind::OneTimeCode,
    ];

    /// The error code a write is refused with.
    pub const fn code(self) -> Code {
        match self {
            SecretKind::Password => Code::MemorySecretPassword,
            SecretKind::KnownToken => Code::MemorySecretKnownToken,
            SecretKind::HighEntropy => Code::MemorySecretHighEntropy,
            SecretKind::PrivateKey => Code::MemorySecretPrivateKey,
            SecretKind::PaymentCard => Code::MemorySecretPaymentCard,
            SecretKind::OneTimeCode => Code::MemorySecretOneTimeCode,
        }
    }

    /// The kind in a few words, for a message to the user.
    pub const fn describe(self) -> &'static str {
        match self {
            SecretKind::Password => "a password, passphrase or PIN",
            SecretKind::KnownToken => "an API key or access token",
            SecretKind::HighEntropy => "a long random-looking string that may be a key",
            SecretKind::PrivateKey => "a private key",
            SecretKind::PaymentCard => "a payment-card number",
            SecretKind::OneTimeCode => "a one-time code",
        }
    }
}

/// The shortest run the entropy rule looks at.
pub const ENTROPY_MIN_LENGTH: usize = 32;

/// The entropy, in bits per character, at or above which a run is refused.
pub const ENTROPY_MIN_BITS: f64 = 4.5;

/// Look for a secret in a text. Returns the first kind found, or `None`.
pub fn detect(text: &str) -> Option<SecretKind> {
    // Invisible characters would otherwise split a keyword or a number.
    let cleaned: String = text.chars().filter(|c| !is_invisible(*c)).collect();
    if has_private_key(&cleaned) {
        Some(SecretKind::PrivateKey)
    } else if has_known_token(&cleaned) {
        Some(SecretKind::KnownToken)
    } else if has_payment_card(&cleaned) {
        Some(SecretKind::PaymentCard)
    } else if has_one_time_code(&cleaned) {
        Some(SecretKind::OneTimeCode)
    } else if has_password(&cleaned) {
        Some(SecretKind::Password)
    } else if has_high_entropy_run(&cleaned) {
        Some(SecretKind::HighEntropy)
    } else {
        None
    }
}

/// Look for a secret anywhere in a JSON value: in every string, in every
/// property name, in numbers, and in a property whose name says it holds a
/// secret and whose value is not a placeholder.
pub fn detect_in_json(value: &Value) -> Option<SecretKind> {
    match value {
        Value::Null | Value::Bool(_) => None,
        Value::Number(number) => detect(&number.to_string()),
        Value::String(text) => detect(text),
        Value::Array(items) => items.iter().find_map(detect_in_json),
        Value::Object(map) => {
            for (name, item) in map {
                if let Some(kind) = detect(name) {
                    return Some(kind);
                }
                if let Value::String(text) = item
                    && names_a_secret(name)
                    && !is_placeholder(text)
                {
                    return Some(SecretKind::Password);
                }
                if let Some(kind) = detect_in_json(item) {
                    return Some(kind);
                }
            }
            None
        }
    }
}

fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
    )
}

// ---------------------------------------------------------------- private keys

fn has_private_key(text: &str) -> bool {
    if text.contains("PuTTY-User-Key-File-") {
        return true;
    }
    const MARKER: &str = "-----BEGIN ";
    let mut rest = text;
    while let Some(at) = rest.find(MARKER) {
        let after = &rest[at + MARKER.len()..];
        let label: String = after.chars().take(48).collect();
        if label.contains("PRIVATE KEY") {
            return true;
        }
        rest = after;
    }
    false
}

// ---------------------------------------------------------------- known tokens

// A prefix and the least number of token characters that must follow it.
const TOKEN_PREFIXES: &[(&str, usize)] = &[
    ("sk-ant-", 20),
    ("sk-proj-", 20),
    ("sk-", 32),
    ("ghp_", 30),
    ("gho_", 30),
    ("ghu_", 30),
    ("ghs_", 30),
    ("ghr_", 30),
    ("github_pat_", 30),
    ("glpat-", 20),
    ("xoxb-", 20),
    ("xoxp-", 20),
    ("xoxa-", 20),
    ("xoxr-", 20),
    ("xoxs-", 20),
    ("xapp-", 20),
    ("AIza", 30),
    ("ya29.", 20),
    ("sk_live_", 16),
    ("rk_live_", 16),
    ("sk_test_", 16),
    ("npm_", 30),
    ("hf_", 30),
    ("dop_v1_", 30),
    ("shpat_", 20),
    ("pypi-", 30),
    ("SG.", 30),
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/' | '=' | '~')
}

// Words as a token would appear in running text: split at white space and at
// the punctuation that surrounds a value, with a sentence's full stop removed.
fn value_words(text: &str) -> Vec<&str> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | ':'
            )
    })
    .map(|word| word.trim_end_matches('.'))
    .filter(|word| !word.is_empty())
    .collect()
}

fn has_known_token(text: &str) -> bool {
    let words = value_words(text);
    for (index, word) in words.iter().copied().enumerate() {
        // `name=value`: look at the value.
        let candidate = word
            .rsplit('=')
            .find(|part| !part.is_empty())
            .unwrap_or(word);
        if has_token_prefix(candidate) || has_token_prefix(word) || is_aws_key_id(candidate) {
            return true;
        }
        if is_json_web_token(candidate) {
            return true;
        }
        if word.eq_ignore_ascii_case("bearer")
            && let Some(next) = words.get(index + 1)
            && next.len() >= 20
            && next.chars().all(is_token_char)
            && next.chars().any(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn has_token_prefix(word: &str) -> bool {
    TOKEN_PREFIXES.iter().any(|(prefix, least)| {
        word.strip_prefix(*prefix).is_some_and(|rest| {
            rest.len() >= *least
                && rest.chars().all(is_token_char)
                && rest.chars().any(|c| c.is_ascii_digit())
        })
    })
}

// An AWS access key identifier: AKIA or ASIA and sixteen upper-case letters
// and digits, at least one of them a digit.
fn is_aws_key_id(word: &str) -> bool {
    let rest = match word
        .strip_prefix("AKIA")
        .or_else(|| word.strip_prefix("ASIA"))
    {
        Some(rest) => rest,
        None => return false,
    };
    rest.len() == 16
        && rest
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && rest.chars().any(|c| c.is_ascii_digit())
}

fn is_json_web_token(word: &str) -> bool {
    if !word.starts_with("eyJ") || word.len() < 40 {
        return false;
    }
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() >= 3
        && parts[..2].iter().all(|part| {
            part.len() >= 8
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

// --------------------------------------------------------------- payment cards

fn has_payment_card(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        // A run of digits, allowing one space or hyphen between two digits.
        let start = index;
        let mut digits: Vec<u32> = Vec::new();
        while index < chars.len() {
            let c = chars[index];
            if let Some(digit) = c.to_digit(10) {
                digits.push(digit);
                index += 1;
            } else if (c == ' ' || c == '-')
                && chars.get(index + 1).is_some_and(char::is_ascii_digit)
            {
                index += 1;
            } else {
                break;
            }
        }
        // A number glued to letters, or a telephone number in international
        // form, is not a card number.
        let free_before = start == 0 || {
            let before = chars[start - 1];
            !(before.is_alphanumeric() || before == '+')
        };
        let free_after = chars
            .get(index)
            .is_none_or(|after| !after.is_alphanumeric());
        if free_before && free_after && is_card_number(&digits) {
            return true;
        }
    }
    false
}

/// Whether the digits have the length and prefix of a card scheme and a valid
/// Luhn check digit.
pub(crate) fn is_card_number(digits: &[u32]) -> bool {
    (13..=19).contains(&digits.len()) && has_card_prefix(digits) && luhn_valid(digits)
}

fn has_card_prefix(digits: &[u32]) -> bool {
    let length = digits.len();
    if length < 13 {
        return false;
    }
    let one = digits[0];
    let two = one * 10 + digits[1];
    let three = two * 10 + digits[2];
    let four = three * 10 + digits[3];
    let long = (16..=19).contains(&length);
    // Visa.
    (one == 4 && matches!(length, 13 | 16 | 19))
        // Mastercard.
        || (length == 16 && ((51..=55).contains(&two) || (2221..=2720).contains(&four)))
        // American Express.
        || (length == 15 && matches!(two, 34 | 37))
        // Discover.
        || (long && (four == 6011 || two == 65 || (644..=649).contains(&three)))
        // JCB.
        || (long && (3528..=3589).contains(&four))
        // Diners Club.
        || (length == 14 && ((300..=305).contains(&three) || matches!(two, 36 | 38)))
        // UnionPay.
        || (long && two == 62)
}

/// The Luhn check: from the right, every second digit is doubled, and the sum
/// of all digits is a multiple of ten.
pub(crate) fn luhn_valid(digits: &[u32]) -> bool {
    let mut sum = 0u32;
    for (position, digit) in digits.iter().rev().enumerate() {
        let mut value = *digit;
        if position % 2 == 1 {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
    }
    !digits.is_empty() && sum.is_multiple_of(10)
}

// -------------------------------------------------------------- one-time codes

const ONE_TIME_PHRASES: &[&[&str]] = &[
    &["otp"],
    &["totp"],
    &["2fa"],
    &["one-time"],
    &["one", "time", "code"],
    &["one", "time", "password"],
    &["one", "time", "passcode"],
    &["verification", "code"],
    &["authentication", "code"],
    &["authenticator", "code"],
    &["login", "code"],
    &["confirmation", "code"],
    &["sms", "code"],
];

// How many words either side of a one-time-code phrase a number counts.
const ONE_TIME_WINDOW: usize = 6;

fn has_one_time_code(text: &str) -> bool {
    if text.contains("otpauth://") {
        return true;
    }
    let words: Vec<String> = text.split_whitespace().map(plain_word).collect();
    let mut index = 0;
    while index < words.len() {
        if let Some(length) = phrase_at(&words, index, ONE_TIME_PHRASES) {
            let from = index.saturating_sub(ONE_TIME_WINDOW);
            let to = (index + length + ONE_TIME_WINDOW).min(words.len());
            let window = &words[from..to];
            let whole = window.iter().any(|word| is_code_number(word, 4, 8));
            let split = window
                .windows(2)
                .any(|pair| is_code_number(&pair[0], 3, 3) && is_code_number(&pair[1], 3, 3));
            if whole || split {
                return true;
            }
        }
        index += 1;
    }
    false
}

// A word in lower case without the punctuation around it.
fn plain_word(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

// The length in words of the longest phrase that starts at `index`.
fn phrase_at(words: &[String], index: usize, phrases: &[&[&str]]) -> Option<usize> {
    phrases
        .iter()
        .filter(|phrase| {
            phrase
                .iter()
                .enumerate()
                .all(|(offset, expected)| words.get(index + offset).is_some_and(|w| w == expected))
        })
        .map(|phrase| phrase.len())
        .max()
}

// Digits only, allowing one hyphen inside, with a length in the given range.
fn is_code_number(word: &str, least: usize, most: usize) -> bool {
    let digits: String = word.chars().filter(|c| *c != '-').collect();
    (least..=most).contains(&digits.len())
        && digits.chars().all(|c| c.is_ascii_digit())
        && word.chars().filter(|c| *c == '-').count() <= 1
}

// ------------------------------------------------------------------- passwords

// Words that name a secret given in words or characters.
const PASSWORD_PHRASES: &[&[&str]] = &[
    &["password"],
    &["passwd"],
    &["passphrase"],
    &["passcode"],
    &["pwd"],
    &["api", "key"],
    &["api_key"],
    &["api-key"],
    &["apikey"],
    &["secret", "key"],
    &["secret_key"],
    &["access", "token"],
    &["access_token"],
    &["auth", "token"],
    &["auth_token"],
    &["client", "secret"],
    &["client_secret"],
    &["recovery", "code"],
    &["recovery", "key"],
];

// Words that name a secret given in digits.
const DIGIT_PHRASES: &[&[&str]] = &[
    &["pin"],
    &["pin", "code"],
    &["cvv"],
    &["cvv2"],
    &["cvc"],
    &["security", "code"],
];

// Words that name a secret only when a credential-looking value follows a
// colon or an equals sign.
const WEAK_PHRASES: &[&[&str]] = &[&["token"], &["secret"]];

// After "password is", these words continue a sentence rather than give the
// password.
const SENTENCE_WORDS: &[&str] = &[
    "a",
    "about",
    "also",
    "always",
    "an",
    "at",
    "being",
    "by",
    "changed",
    "correct",
    "different",
    "empty",
    "encrypted",
    "expired",
    "for",
    "forgotten",
    "from",
    "going",
    "here",
    "hidden",
    "in",
    "incorrect",
    "it",
    "kept",
    "like",
    "located",
    "locked",
    "long",
    "lost",
    "managed",
    "my",
    "needed",
    "never",
    "no",
    "not",
    "now",
    "on",
    "only",
    "optional",
    "protected",
    "remembered",
    "required",
    "reset",
    "safe",
    "same",
    "saved",
    "secure",
    "set",
    "shared",
    "short",
    "something",
    "still",
    "stored",
    "strong",
    "that",
    "the",
    "there",
    "this",
    "to",
    "too",
    "unknown",
    "weak",
    "what",
    "with",
    "written",
    "wrong",
    "your",
];

// Values that stand in for a secret without being one.
const PLACEHOLDER_WORDS: &[&str] = &[
    "none",
    "null",
    "n/a",
    "unknown",
    "required",
    "optional",
    "hidden",
    "redacted",
    "empty",
    "yes",
    "no",
    "true",
    "false",
    "unchanged",
    "same",
];

/// Whether a value stands in for a secret without being one: empty, masked
/// (`****`), a template (`{{name}}`, `${NAME}`, `<name>`, `[name]`, `%s`,
/// `$NAME`) or a word such as "required".
pub(crate) fn is_placeholder(value: &str) -> bool {
    let value = value.trim().trim_matches(['"', '\'', '`']);
    if value.is_empty() {
        return true;
    }
    if value
        .chars()
        .all(|c| matches!(c, '*' | '•' | 'x' | 'X' | '.' | '-' | '_' | '?'))
    {
        return true;
    }
    if value.starts_with("{{")
        || value.starts_with("${")
        || value.starts_with('<')
        || value.starts_with('[')
        || value.starts_with('%')
        || value.starts_with('$')
    {
        return true;
    }
    let lower = value
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    PLACEHOLDER_WORDS.contains(&lower.as_str())
}

// Whether a JSON property name says the property holds a secret.
fn names_a_secret(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "password",
        "passwd",
        "passphrase",
        "passcode",
        "secret",
        "token",
        "api_key",
        "api-key",
        "apikey",
        "private_key",
        "cvv",
        "cvc",
    ]
    .iter()
    .any(|word| lower.contains(*word))
        || lower == "pin"
        || lower == "pwd"
}

// The text as words, with `:` and `=` as words of their own, so that
// `password=x`, `password: x` and `"password":"x"` all read the same.
fn password_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    for chunk in text.split_whitespace() {
        let mut current = String::new();
        for c in chunk.chars() {
            if c == ':' || c == '=' {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                words.push(c.to_string());
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
    }
    words
}

fn has_password(text: &str) -> bool {
    if has_url_credentials(text) {
        return true;
    }
    let raw = password_words(text);
    let plain: Vec<String> = raw.iter().map(|word| plain_word(word.as_str())).collect();
    let mut index = 0;
    while index < plain.len() {
        if let Some(length) = phrase_at(&plain, index, PASSWORD_PHRASES)
            && follows_with_secret(&raw, &plain, index, index + length)
        {
            return true;
        }
        if let Some(length) = phrase_at(&plain, index, DIGIT_PHRASES)
            && follows_with_digits(&plain, index + length)
        {
            return true;
        }
        if let Some(length) = phrase_at(&plain, index, WEAK_PHRASES)
            && follows_with_credential(&raw, index + length)
        {
            return true;
        }
        if assigns_to_secret_name(&raw, &plain, index) {
            return true;
        }
        index += 1;
    }
    false
}

// How a compound name that holds a secret ends, as in `DB_PASSWORD`.
const SECRET_NAME_ENDINGS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "_pwd",
    "_secret",
    "-secret",
    "_token",
    "-token",
    "api_key",
    "apikey",
    "api-key",
];

// A name such as `DB_PASSWORD` followed by `:` or `=` and a value.
fn assigns_to_secret_name(raw: &[String], plain: &[String], index: usize) -> bool {
    let named = SECRET_NAME_ENDINGS
        .iter()
        .any(|ending| plain[index].ends_with(*ending));
    let Some(separator) = raw.get(index + 1) else {
        return false;
    };
    let Some(value) = raw.get(index + 2) else {
        return false;
    };
    named && is_assignment(separator) && !is_placeholder(value) && !is_assignment(value)
}

fn is_assignment(word: &str) -> bool {
    word == ":" || word == "="
}

fn is_said(word: &str) -> bool {
    word == "is" || word == "was"
}

// After a password word at `keyword..after`: a separator and a value.
fn follows_with_secret(raw: &[String], plain: &[String], keyword: usize, after: usize) -> bool {
    // `--password value` on a command line.
    if raw[keyword].starts_with("--")
        && let Some(value) = raw.get(after)
        && !is_assignment(value)
        && !value.starts_with('-')
        && !is_placeholder(value)
    {
        return true;
    }
    // Allow a short "for the router" between the word and the separator.
    let mut at = after;
    if plain
        .get(at)
        .is_some_and(|word| matches!(word.as_str(), "for" | "to" | "on" | "of"))
    {
        let limit = (at + 4).min(raw.len());
        while at < limit && !is_assignment(&raw[at]) && !is_said(&plain[at]) {
            at += 1;
        }
    }
    let Some(separator) = raw.get(at) else {
        return false;
    };
    let Some(value) = raw.get(at + 1) else {
        return false;
    };
    if is_assignment(separator) {
        return !is_placeholder(value) && !is_assignment(value);
    }
    if is_said(&plain[at]) {
        let word = &plain[at + 1];
        return !word.is_empty()
            && !is_placeholder(value)
            && !SENTENCE_WORDS.contains(&word.as_str());
    }
    false
}

// After a PIN or security-code word: an optional separator, then 3 to 8 digits.
fn follows_with_digits(plain: &[String], after: usize) -> bool {
    let mut at = after;
    // The separators have no letters or digits, so their plain form is empty;
    // "is" and "was" are skipped as well.
    while plain
        .get(at)
        .is_some_and(|word| word.is_empty() || is_said(word))
        && at < after + 2
    {
        at += 1;
    }
    plain
        .get(at)
        .is_some_and(|word| is_code_number(word, 3, 8) && !word.contains('-'))
}

// After "token" or "secret": a colon or equals sign, then something that looks
// like a credential.
fn follows_with_credential(raw: &[String], after: usize) -> bool {
    let Some(separator) = raw.get(after) else {
        return false;
    };
    let Some(value) = raw.get(after + 1) else {
        return false;
    };
    if !is_assignment(separator) || is_placeholder(value) {
        return false;
    }
    let value = value.trim_matches(['"', '\'', '`', ',', ';']);
    let has_digit = value.chars().any(|c| c.is_ascii_digit());
    (value.len() >= 8 && has_digit) || value.len() >= 16
}

// `scheme://user:password@host`.
fn has_url_credentials(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let Some((_, rest)) = word.split_once("://") else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        authority
            .rsplit_once('@')
            .and_then(|(user_info, _)| user_info.split_once(':'))
            .is_some_and(|(_, password)| !is_placeholder(password))
    })
}

// ---------------------------------------------------------------- high entropy

fn has_high_entropy_run(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+')))
        .map(|run| run.trim_matches(['_', '-', '+']))
        .any(is_high_entropy)
}

fn is_high_entropy(run: &str) -> bool {
    run.len() >= ENTROPY_MIN_LENGTH
        && run.chars().any(|c| c.is_ascii_lowercase())
        && run.chars().any(|c| c.is_ascii_uppercase())
        && run.chars().any(|c| c.is_ascii_digit())
        && bits_per_character(run) >= ENTROPY_MIN_BITS
}

/// The Shannon entropy of the characters of `text`, in bits per character.
pub(crate) fn bits_per_character(text: &str) -> f64 {
    let mut counts: std::collections::BTreeMap<char, u32> = std::collections::BTreeMap::new();
    let mut total = 0u32;
    for c in text.chars() {
        *counts.entry(c).or_insert(0) += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let total = f64::from(total);
    counts
        .values()
        .map(|count| {
            let share = f64::from(*count) / total;
            -share * share.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Test secrets are assembled from pieces so that no complete token-shaped
    // string appears in the source, where a repository scanner would report
    // it. None of them is or was a real credential.
    fn join(parts: &[&str]) -> String {
        parts.concat()
    }

    fn digits_of(text: &str) -> Vec<u32> {
        text.chars().filter_map(|c| c.to_digit(10)).collect()
    }

    #[test]
    fn luhn_check() {
        for valid in [
            "4111111111111111",
            "5555555555554444",
            "378282246310005",
            "6011111111111117",
            "79927398713",
        ] {
            assert!(luhn_valid(&digits_of(valid)), "{valid}");
        }
        for invalid in ["4111111111111112", "5555555555554445", "79927398710"] {
            assert!(!luhn_valid(&digits_of(invalid)), "{invalid}");
        }
        assert!(!luhn_valid(&[]));
    }

    #[test]
    fn payment_cards_are_refused() {
        for text in [
            "my card is 4111111111111111",
            "card 4111 1111 1111 1111 expires next year",
            "pay with 5555-5555-5555-4444.",
            "amex 3782 822463 10005",
            "(6011111111111117)",
            "Visa: 4242424242424242",
        ] {
            assert_eq!(detect(text), Some(SecretKind::PaymentCard), "{text}");
        }
    }

    #[test]
    fn numbers_that_are_not_cards_are_kept() {
        for text in [
            // An order number: sixteen digits, no card prefix.
            "order number 9000123456789012 was delivered",
            // A card prefix but a failing check digit.
            "reference 4111111111111112",
            // Telephone numbers.
            "call me on +39 06 1234 5678",
            "the office is +44 20 7946 0958 and the mobile +4915112345678901",
            // A tracking number glued to letters.
            "parcel JD0146000032134455667788GB",
            // Too long to be a card: an account number.
            "account 4111 1111 1111 1111 2222 33",
            // Dates and small numbers.
            "meeting on 2026-11-12 at 10:30 in room 4111",
            "the build took 1234567890123 nanoseconds less than 9876543210987",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }

    #[test]
    fn private_keys_are_refused() {
        let pem = join(&["-----BEGIN RSA ", "PRIVATE KEY-----\nMIIEow\n-----END"]);
        let openssh = join(&["-----BEGIN OPENSSH ", "PRIVATE KEY-----"]);
        let pgp = join(&["-----BEGIN PGP ", "PRIVATE KEY BLOCK-----"]);
        let putty = join(&["PuTTY-User-Key-", "File-3: ssh-ed25519"]);
        for text in [pem, openssh, pgp, putty] {
            assert_eq!(detect(&text), Some(SecretKind::PrivateKey));
        }
        // A public key and a certificate are not secrets.
        assert_eq!(detect("-----BEGIN PUBLIC KEY-----"), None);
        assert_eq!(detect("-----BEGIN CERTIFICATE-----"), None);
        assert_eq!(detect("my private key is kept on a hardware token"), None);
    }

    #[test]
    fn known_tokens_are_refused() {
        let body = "q7Zr2LmX9vB4tKc8NpW3yHd6JfS1gAe5UoT0iRbM";
        let tokens = [
            join(&["sk-", "ant-", "api03-", body]),
            join(&["sk-", "proj-", body]),
            join(&["gh", "p_", body]),
            join(&["github", "_pat_", body]),
            join(&["gl", "pat-", body]),
            join(&["xo", "xb-", "1234567890-", body]),
            join(&["AI", "za", body]),
            join(&["sk_", "live_", body]),
            join(&["AK", "IA", "IOSFODNN7EXAMPLE"]),
            join(&[
                "ey",
                "JhbGciOiJIUzI1NiJ9",
                ".",
                "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
                ".",
                body,
            ]),
        ];
        for token in &tokens {
            let text = format!("use {token} for the build");
            assert_eq!(detect(&text), Some(SecretKind::KnownToken), "{token}");
            let assigned = format!("KEY={token}");
            assert!(detect(&assigned).is_some(), "{assigned}");
        }
        let bearer = format!("Authorization: Bearer {body}");
        assert_eq!(detect(&bearer), Some(SecretKind::KnownToken));
    }

    #[test]
    fn words_that_only_start_like_a_token_are_kept() {
        for text in [
            "install sk-learn and scikit-image",
            "the ghp_ prefix marks a GitHub token",
            "AKIA is how those identifiers start",
            "the bearer of this letter",
            "npm_config_registry is an environment variable",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }

    #[test]
    fn passwords_in_obvious_forms_are_refused() {
        for text in [
            "my password is hunter2",
            "The wifi password: correct horse battery staple",
            "password=hunter2",
            "Password : Tr0ub4dor&3",
            "the passphrase is open sesame",
            "login with user anna and pwd: s3cret!",
            "the password for the router is admin1234",
            "run mysql --password hunter2 -u root",
            "run mysql --password=hunter2 -u root",
            "api key: abcdef123456",
            "API_KEY=abcdef123456",
            "the recovery code is 8f3k-22md",
            "bank PIN is 4821",
            "PIN: 4821",
            "pin 482193",
            "cvv 123",
            "the security code is 9876",
            "token: abcd1234efgh",
            "client_secret = Zm9vYmFyYmF6",
            "mount smb://anna:hunter2@nas.local/share",
            "{\"password\":\"hunter2\"}",
            "DB_PASSWORD=hunter2",
            "export MYSQL_PWD=hunter2",
            "csrf_token: abc",
        ] {
            assert_eq!(detect(text), Some(SecretKind::Password), "{text}");
        }
    }

    #[test]
    fn talk_about_passwords_is_kept() {
        for text in [
            "always ask before showing my password",
            "I keep my passwords in KeePassXC",
            "the password manager is Bitwarden",
            "my password is stored in the keyring",
            "the password is not written anywhere",
            "password: required",
            "password: ********",
            "password = {{ask_user}}",
            "export PGPASSWORD=$DB_PASSWORD",
            "the password field is the second one on the page",
            "pin the terminal to the task bar",
            "the pin is on the back of the board",
            "token ring networks are obsolete",
            "the secret to good bread is time",
            "secret: I like rain",
            "open https://example.org/a/b?c=d#e in the browser",
            "ssh anna@nas.local works without a password",
            "git remote is https://git@example.org/team/repo.git",
            "error code 404 means the page is missing",
            "max_tokens=4096",
            "tokens: 5000 per request",
            "primary_key=id",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }

    #[test]
    fn one_time_codes_are_refused() {
        for text in [
            "your verification code is 482913",
            "482913 is your verification code",
            "OTP 123456",
            "the one-time code was 9912",
            "one time password: 55 12 99 and then 771204",
            "2FA code 123 456",
            "login code: 123-456",
            "otpauth://totp/Example:anna?secret=JBSWY3DPEHPK3PXP",
        ] {
            assert_eq!(detect(text), Some(SecretKind::OneTimeCode), "{text}");
        }
        for text in [
            "the verification code arrives by SMS",
            "error code 482913 in the log",
            "order number 482913",
            "I use an authenticator app for 2FA",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }

    #[test]
    fn long_random_strings_are_refused() {
        let random = join(&["q7Zr2LmX9vB4tKc8NpW3", "yHd6JfS1gAe5UoT0iRbM"]);
        assert!(bits_per_character(&random) > 5.0);
        let text = format!("the value {random} goes in the config file");
        assert_eq!(detect(&text), Some(SecretKind::HighEntropy));
    }

    #[test]
    fn long_ordinary_strings_are_kept() {
        for text in [
            // A git commit hash in a procedure, full and abbreviated.
            "check out commit 3f786850e387550fdab836ed7e6dc881de23001b and rebuild",
            "the fix is in 29a020e",
            // A SHA-256 checksum.
            "sha256 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            // A UUID.
            "volume 123e4567-e89b-12d3-a456-426614174000 is the backup disk",
            // Order numbers.
            "order ORD-2026-000123456789-IT was refunded",
            // Long identifiers and paths.
            "the unit is org.freedesktop.NetworkManager.Settings.Connection",
            "open /usr/share/applications/org.kde.dolphin.desktop to edit it",
            "package libreoffice-langpack-en-7.6.4.1-1.fc39.x86_64 is installed",
            "the edition is Windows10Enterprise2021LTSC",
            "see ThisIsAVeryLongCamelCaseIdentifierNameFrom2026Release",
            "https://example.org/Downloads/Reports2026/QuarterlySummaryFinalVersion2.pdf",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }

    #[test]
    fn invisible_characters_do_not_hide_a_secret() {
        assert_eq!(
            detect("pass\u{200b}word: hunter2"),
            Some(SecretKind::Password)
        );
        assert_eq!(
            detect("4111\u{200b}1111\u{feff}1111\u{00ad}1111"),
            Some(SecretKind::PaymentCard)
        );
    }

    #[test]
    fn json_values_are_searched_throughout() {
        assert_eq!(
            detect_in_json(&json!({ "to": "a@example.org", "body": "see you at 10" })),
            None
        );
        assert_eq!(
            detect_in_json(&json!({ "user": "anna", "password": "hunter2" })),
            Some(SecretKind::Password)
        );
        // A template in place of the secret is the point of a saved procedure.
        assert_eq!(
            detect_in_json(&json!({ "user": "anna", "password": "{{ask_user}}" })),
            None
        );
        assert_eq!(
            detect_in_json(&json!({ "steps": [{ "note": "card 4111 1111 1111 1111" }] })),
            Some(SecretKind::PaymentCard)
        );
        assert_eq!(
            detect_in_json(&json!({ "card": 4111111111111111u64 })),
            Some(SecretKind::PaymentCard)
        );
        assert_eq!(
            detect_in_json(&json!({ "my password is hunter2": true })),
            Some(SecretKind::Password)
        );
        assert_eq!(detect_in_json(&json!([1, 2.5, null, true, "plain"])), None);
    }

    #[test]
    fn every_kind_has_its_own_code() {
        let codes: std::collections::BTreeSet<&str> = SecretKind::ALL
            .iter()
            .map(|kind| kind.code().as_str())
            .collect();
        assert_eq!(codes.len(), SecretKind::ALL.len());
        for code in codes {
            assert!(code.starts_with("UOS-AI-MEMORY-SECRET-"), "{code}");
        }
    }
}

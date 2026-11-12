// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Cleaning of text that comes from sysfs, procfs and command output.
//!
//! Much of what the inventory reads is chosen by the hardware: a USB device
//! supplies its own manufacturer, product and serial strings. Everything read
//! is therefore treated as untrusted bytes. It is decoded leniently, stripped
//! of control and invisible formatting characters, and cut to a fixed length
//! before it enters the model, so that no later consumer (a terminal, a log, a
//! user interface, a language model) receives raw device-supplied bytes.

/// The most bytes read from one sysfs or procfs attribute. A sysfs attribute is
/// at most one page.
pub const MAX_ATTR_BYTES: usize = 4096;

/// The most characters kept from one attribute.
pub const MAX_TEXT_CHARS: usize = 256;

/// The most characters kept from a sysfs path shown in the inventory.
pub const MAX_PATH_CHARS: usize = 512;

/// The most characters kept from a `modalias` string.
pub const MAX_MODALIAS_CHARS: usize = 1024;

/// The most characters allowed in a driver, module or kernel device name.
pub const MAX_NAME_CHARS: usize = 64;

/// The character that marks a string cut short by [`clean_to`].
pub const ELLIPSIS: char = '\u{2026}';

/// Whether a character is invisible formatting that could disguise text:
/// zero-width characters, bidirectional overrides and isolates, the byte-order
/// mark and interlinear annotation marks.
fn is_invisible(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// Clean untrusted bytes into displayable text of at most [`MAX_TEXT_CHARS`]
/// characters. See [`clean_to`].
pub fn clean(raw: &[u8]) -> String {
    clean_to(raw, MAX_TEXT_CHARS)
}

/// Clean untrusted bytes into displayable text of at most `max_chars`
/// characters.
///
/// * Invalid UTF-8 becomes U+FFFD.
/// * Control characters and every kind of white space become one space; runs
///   are collapsed and the ends trimmed.
/// * Invisible formatting characters are removed.
/// * Text longer than `max_chars` is cut and ends in an ellipsis.
pub fn clean_to(raw: &[u8], max_chars: usize) -> String {
    let decoded = String::from_utf8_lossy(raw);
    let mut cleaned = String::new();
    let mut pending_space = false;
    for ch in decoded.chars() {
        if is_invisible(ch) {
            continue;
        }
        if ch.is_whitespace() || ch.is_control() {
            pending_space = !cleaned.is_empty();
            continue;
        }
        if pending_space {
            cleaned.push(' ');
            pending_space = false;
        }
        cleaned.push(ch);
    }
    if cleaned.chars().count() > max_chars {
        let keep = max_chars.saturating_sub(1);
        let cut = cleaned
            .char_indices()
            .nth(keep)
            .map_or(cleaned.len(), |(index, _)| index);
        cleaned.truncate(cut);
        while cleaned.ends_with(' ') {
            cleaned.pop();
        }
        if max_chars > 0 {
            cleaned.push(ELLIPSIS);
        }
    }
    cleaned
}

/// [`clean`], with empty text reported as `None`.
pub fn clean_opt(raw: &[u8]) -> Option<String> {
    let cleaned = clean(raw);
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

/// Normalise a hexadecimal identifier such as `0x8086` or `046D` to exactly
/// `digits` lower-case hexadecimal digits. Anything else is `None`.
pub fn hex_id(raw: &str, digits: usize) -> Option<String> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if body.is_empty() || body.len() > digits || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("{:0>digits$}", body.to_ascii_lowercase()))
}

/// Accept a driver, module or kernel device name: 1 to [`MAX_NAME_CHARS`]
/// ASCII letters, digits and `_ - . : + @`. Anything else is `None`.
pub fn kernel_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= MAX_NAME_CHARS
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'+' | b'@'));
    valid.then(|| trimmed.to_owned())
}

/// Turn arbitrary text into a token that is safe inside a device identifier:
/// ASCII letters, digits and `: . _ -` are kept, everything else becomes `_`,
/// and the result is cut to [`MAX_NAME_CHARS`] characters.
pub fn id_token(raw: &str) -> String {
    let token: String = raw
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if token.is_empty() { "_".to_owned() } else { token }
}

/// Parse a sysfs flag that is `0` or `1`.
pub fn flag(raw: &str) -> Option<bool> {
    match raw.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// A sort key that orders `input2` before `input10`: the name without its
/// trailing digits, then the digits as a number.
pub fn natural_key(name: &str) -> (String, u64) {
    let prefix = name.trim_end_matches(|ch: char| ch.is_ascii_digit());
    let number = name
        .get(prefix.len()..)
        .and_then(|digits| digits.parse::<u64>().ok())
        .unwrap_or(0);
    (prefix.to_owned(), number)
}

/// Whether text is a MAC address of six colon-separated octets that is not
/// all zeros.
pub fn is_mac_address(text: &str) -> bool {
    let octets: Vec<&str> = text.split(':').collect();
    octets.len() == 6
        && octets
            .iter()
            .all(|octet| octet.len() == 2 && octet.bytes().all(|b| b.is_ascii_hexdigit()))
        && octets.iter().any(|octet| *octet != "00")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_trims_and_collapses_white_space() {
        assert_eq!(clean(b"  QEMU   HARDDISK   \n"), "QEMU HARDDISK");
        assert_eq!(clean(b"\n"), "");
        assert_eq!(clean(b""), "");
    }

    #[test]
    fn clean_replaces_control_characters() {
        assert_eq!(clean(b"Evil\x1b[31mDevice\x07\x00"), "Evil [31mDevice");
        assert_eq!(clean(b"line one\r\nline two"), "line one line two");
        assert_eq!(clean("a\u{0085}b\u{009b}c".as_bytes()), "a b c");
    }

    #[test]
    fn clean_removes_invisible_formatting() {
        // A right-to-left override would otherwise let "exe.txt" read as "txt.exe".
        assert_eq!(clean("safe\u{202E}txt.exe".as_bytes()), "safetxt.exe");
        assert_eq!(clean("\u{FEFF}Key\u{200B}board".as_bytes()), "Keyboard");
    }

    #[test]
    fn clean_survives_invalid_utf8() {
        let cleaned = clean(b"Cam\xff\xfeera \xc3");
        assert_eq!(cleaned, "Cam\u{FFFD}\u{FFFD}era \u{FFFD}");
    }

    #[test]
    fn clean_bounds_length_and_marks_the_cut() {
        let long = b"A".repeat(100_000);
        let cleaned = clean(&long);
        assert_eq!(cleaned.chars().count(), MAX_TEXT_CHARS);
        assert!(cleaned.ends_with(ELLIPSIS));

        let exact = b"B".repeat(MAX_TEXT_CHARS);
        assert_eq!(clean(&exact).chars().count(), MAX_TEXT_CHARS);
        assert!(!clean(&exact).ends_with(ELLIPSIS));

        // Multi-byte characters are cut on a character boundary.
        let wide = "\u{00e9}".repeat(1000);
        let cleaned = clean_to(wide.as_bytes(), 10);
        assert_eq!(cleaned.chars().count(), 10);
        assert_eq!(clean_to(b"abc", 0), "");
    }

    #[test]
    fn clean_opt_reports_empty_as_none() {
        assert_eq!(clean_opt(b" \n"), None);
        assert_eq!(clean_opt(b"x\n"), Some("x".to_owned()));
    }

    #[test]
    fn hex_id_normalises() {
        assert_eq!(hex_id("0x8086\n", 4), Some("8086".to_owned()));
        assert_eq!(hex_id("046D", 4), Some("046d".to_owned()));
        assert_eq!(hex_id("0x30000", 6), Some("030000".to_owned()));
        assert_eq!(hex_id("9", 2), Some("09".to_owned()));
        assert_eq!(hex_id("", 4), None);
        assert_eq!(hex_id("0x", 4), None);
        assert_eq!(hex_id("12345", 4), None);
        assert_eq!(hex_id("80g6", 4), None);
        assert_eq!(hex_id("-1", 4), None);
    }

    #[test]
    fn kernel_name_accepts_only_plain_names() {
        assert_eq!(kernel_name("snd_hda_intel\n"), Some("snd_hda_intel".to_owned()));
        assert_eq!(kernel_name("usb-storage"), Some("usb-storage".to_owned()));
        assert_eq!(kernel_name("0000:00:14.3"), Some("0000:00:14.3".to_owned()));
        assert_eq!(kernel_name(""), None);
        assert_eq!(kernel_name("a b"), None);
        assert_eq!(kernel_name("../../etc"), None);
        assert_eq!(kernel_name(&"x".repeat(65)), None);
    }

    #[test]
    fn id_token_is_always_safe() {
        assert_eq!(id_token("enp0s31f6"), "enp0s31f6");
        assert_eq!(id_token("we ird/n\u{00e9}me"), "we_ird_n_me");
        assert_eq!(id_token(""), "_");
        assert_eq!(id_token(&"y".repeat(500)).len(), MAX_NAME_CHARS);
    }

    #[test]
    fn natural_key_orders_numbered_names() {
        let mut names = vec!["input10", "input2", "event1", "input1"];
        names.sort_by_key(|name| natural_key(name));
        assert_eq!(names, ["event1", "input1", "input2", "input10"]);
        assert_eq!(natural_key("mice"), ("mice".to_owned(), 0));
        assert_eq!(natural_key("x99999999999999999999999"), ("x".to_owned(), 0));
    }

    #[test]
    fn mac_addresses_are_recognised() {
        assert!(is_mac_address("00:e0:4c:68:00:01"));
        assert!(!is_mac_address("00:00:00:00:00:00"));
        assert!(!is_mac_address("00:e0:4c:68:00"));
        assert!(!is_mac_address("00:e0:4c:68:00:zz"));
        assert!(!is_mac_address(""));
    }

    #[test]
    fn flags_parse() {
        assert_eq!(flag("1\n"), Some(true));
        assert_eq!(flag("0"), Some(false));
        assert_eq!(flag("yes"), None);
    }
}

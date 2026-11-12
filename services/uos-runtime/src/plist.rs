// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! A minimal reader for XML property lists.
//!
//! It extracts the string values of the top-level dictionary and nothing
//! else. That is all the bundle inspection needs (`CFBundleExecutable` and a
//! few descriptive keys). Nested dictionaries, arrays, numbers, dates, data
//! and CDATA sections are skipped. It is a scanner, not an XML parser: it does
//! not recurse, so nesting depth cannot exhaust the stack.

use std::collections::BTreeMap;

use crate::limits::{MAX_PLIST_KEYS, MAX_PLIST_VALUE_LEN};

/// The string values of the top-level dictionary of an XML property list.
pub(crate) fn top_level_strings(text: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut rest = text;
    let mut depth: u32 = 0;
    let mut pending_key: Option<String> = None;

    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            let Some(end) = comment.find("-->") else {
                break;
            };
            rest = &comment[end + 3..];
            continue;
        }
        let Some(close) = rest.find('>') else {
            break;
        };
        let tag = &rest[1..close];
        rest = &rest[close + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        match name {
            "dict" | "array" => {
                if !self_closing {
                    depth = depth.saturating_add(1);
                }
                pending_key = None;
            }
            "/dict" | "/array" => {
                depth = depth.saturating_sub(1);
                pending_key = None;
            }
            "key" if !self_closing => {
                let Some(end) = rest.find("</key>") else {
                    break;
                };
                pending_key = (depth == 1).then(|| decode(&rest[..end]));
                rest = &rest[end + "</key>".len()..];
            }
            "string" if !self_closing => {
                let Some(end) = rest.find("</string>") else {
                    break;
                };
                if depth == 1
                    && let Some(key) = pending_key.take()
                    && found.len() < MAX_PLIST_KEYS
                    && end <= MAX_PLIST_VALUE_LEN
                {
                    found.entry(key).or_insert_with(|| decode(&rest[..end]));
                }
                pending_key = None;
                rest = &rest[end + "</string>".len()..];
            }
            _ => pending_key = None,
        }
    }
    found
}

/// Decodes the five predefined XML entities and trims surrounding space.
fn decode(raw: &str) -> String {
    raw.trim()
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- a comment with <key>CFBundleExecutable</key><string>wrong</string> inside -->
    <key>CFBundleExecutable</key>
    <string>Example</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.App</string>
    <key>CFBundleName</key>
    <string>Tom &amp; Jerry &lt;3</string>
    <key>LSRequiresNativeExecution</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleExecutable</key>
            <string>nested-must-not-win</string>
        </dict>
    </array>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>Empty</key>
    <string/>
    <key>CFBundleShortVersionString</key>
    <string> 1.2.3 </string>
</dict>
</plist>
"#;

    #[test]
    fn reads_top_level_strings_only() {
        let found = top_level_strings(SAMPLE);
        assert_eq!(
            found.get("CFBundleExecutable").map(String::as_str),
            Some("Example")
        );
        assert_eq!(
            found.get("CFBundleIdentifier").map(String::as_str),
            Some("com.example.App")
        );
        assert_eq!(
            found.get("CFBundleName").map(String::as_str),
            Some("Tom & Jerry <3")
        );
        assert_eq!(
            found.get("CFBundleShortVersionString").map(String::as_str),
            Some("1.2.3")
        );
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn first_value_of_a_repeated_key_wins() {
        let text = "<plist><dict><key>A</key><string>one</string><key>A</key><string>two</string></dict></plist>";
        assert_eq!(
            top_level_strings(text).get("A").map(String::as_str),
            Some("one")
        );
    }

    #[test]
    fn a_key_followed_by_another_type_is_not_paired_with_a_later_string() {
        let text = "<dict><key>A</key><integer>1</integer><string>stray</string></dict>";
        assert!(top_level_strings(text).is_empty());
    }

    #[test]
    fn malformed_input_never_panics() {
        for text in [
            "",
            "<",
            "<>",
            "<dict>",
            "<dict><key>",
            "<dict><key>A</key><string>",
            "<dict><key>A</key><string>x</string",
            "<!-- never closed",
            "</dict></dict></dict><dict><key>A</key><string>ok</string>",
            "<dict><key>é</key><string>ü</string></dict>",
            "<dict/><key>A</key><string>no</string>",
        ] {
            let _ = top_level_strings(text);
        }
        let deep = "<dict>".repeat(100_000);
        assert!(top_level_strings(&deep).is_empty());
        let sample_len = SAMPLE.len();
        for end in (0..=sample_len).filter(|&end| SAMPLE.is_char_boundary(end)) {
            let _ = top_level_strings(&SAMPLE[..end]);
        }
    }

    #[test]
    fn oversized_values_are_dropped() {
        let long = "x".repeat(MAX_PLIST_VALUE_LEN + 1);
        let text = format!("<dict><key>A</key><string>{long}</string></dict>");
        assert!(top_level_strings(&text).is_empty());
    }
}

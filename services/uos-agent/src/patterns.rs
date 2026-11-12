// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The conservative rule set for accessibility and screen actions (addendum
//! section 5.1).
//!
//! A click is only a click, so a UI action cannot be classified from itself.
//! The engine looks at the name of the control and the title of the window. If
//! either contains a word that begins with one of the stems in
//! [`CONSEQUENTIAL_PATTERNS`], the action is treated as class 2 and the user is
//! asked first.
//!
//! The table is a heuristic, not a security boundary. It is English only, and a
//! control with a misleading or foreign-language label is not recognised. A
//! false match costs one prompt.

/// What kind of consequential action a pattern suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternCategory {
    /// Sending or publishing something to other people.
    Send,
    /// Paying or moving money.
    Pay,
    /// Buying something.
    Purchase,
    /// Deleting something permanently.
    Delete,
    /// Installing or removing software.
    Install,
    /// Submitting a form or agreeing to something.
    Submit,
}

impl PatternCategory {
    /// The category as text, for the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            PatternCategory::Send => "send",
            PatternCategory::Pay => "pay",
            PatternCategory::Purchase => "purchase",
            PatternCategory::Delete => "delete",
            PatternCategory::Install => "install",
            PatternCategory::Submit => "submit",
        }
    }
}

/// One entry of the pattern table: a lower-case word stem and its category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsequentialPattern {
    /// What the stem suggests.
    pub category: PatternCategory,
    /// A lower-case stem. A word matches if it begins with the stem.
    pub stem: &'static str,
}

const fn pattern(category: PatternCategory, stem: &'static str) -> ConsequentialPattern {
    ConsequentialPattern { category, stem }
}

/// The pattern table. This is the only place the patterns are defined.
pub const CONSEQUENTIAL_PATTERNS: &[ConsequentialPattern] = &[
    pattern(PatternCategory::Send, "send"),
    pattern(PatternCategory::Send, "resend"),
    pattern(PatternCategory::Send, "post"),
    pattern(PatternCategory::Send, "publish"),
    pattern(PatternCategory::Send, "share"),
    pattern(PatternCategory::Send, "upload"),
    pattern(PatternCategory::Pay, "pay"),
    pattern(PatternCategory::Pay, "donat"),
    pattern(PatternCategory::Pay, "transfer"),
    pattern(PatternCategory::Pay, "withdraw"),
    pattern(PatternCategory::Purchase, "purchas"),
    pattern(PatternCategory::Purchase, "buy"),
    pattern(PatternCategory::Purchase, "order"),
    pattern(PatternCategory::Purchase, "checkout"),
    pattern(PatternCategory::Purchase, "subscrib"),
    pattern(PatternCategory::Delete, "delet"),
    pattern(PatternCategory::Delete, "remov"),
    pattern(PatternCategory::Delete, "eras"),
    pattern(PatternCategory::Delete, "wipe"),
    pattern(PatternCategory::Delete, "destroy"),
    pattern(PatternCategory::Delete, "discard"),
    pattern(PatternCategory::Delete, "purg"),
    pattern(PatternCategory::Delete, "shred"),
    pattern(PatternCategory::Delete, "empty"),
    pattern(PatternCategory::Install, "install"),
    pattern(PatternCategory::Install, "uninstall"),
    pattern(PatternCategory::Install, "reinstall"),
    pattern(PatternCategory::Submit, "submit"),
    pattern(PatternCategory::Submit, "confirm"),
    pattern(PatternCategory::Submit, "accept"),
    pattern(PatternCategory::Submit, "agree"),
    pattern(PatternCategory::Submit, "sign"),
];

/// The category of the first word in `text` that matches the pattern table.
///
/// The text is lower-cased and split into words at every character that is not
/// a letter or a digit.
///
/// ```
/// use uos_agent::patterns::{PatternCategory, match_consequential};
///
/// assert_eq!(match_consequential("Send"), Some(PatternCategory::Send));
/// assert_eq!(match_consequential("Empty Trash…"), Some(PatternCategory::Delete));
/// assert_eq!(match_consequential("Bold"), None);
/// ```
pub fn match_consequential(text: &str) -> Option<PatternCategory> {
    let lowered = text.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .find_map(|word| {
            CONSEQUENTIAL_PATTERNS
                .iter()
                .find(|entry| word.starts_with(entry.stem))
                .map(|entry| entry.category)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_required_category_has_a_pattern_that_matches() {
        let examples = [
            ("Send", PatternCategory::Send),
            ("Pay now", PatternCategory::Pay),
            ("Complete purchase", PatternCategory::Purchase),
            ("Delete permanently", PatternCategory::Delete),
            ("Install", PatternCategory::Install),
            ("Submit form", PatternCategory::Submit),
        ];
        for (text, category) in examples {
            assert_eq!(match_consequential(text), Some(category), "{text}");
        }
    }

    #[test]
    fn matching_ignores_case_punctuation_and_inflection() {
        for text in [
            "SEND",
            "send…",
            "&Send",
            "Re-send message",
            "Sending",
            "Payment",
            "Uninstalling",
            "Deleted items",
            "confirm_purchase",
            "Place order (3 items)",
            "Buy\u{a0}now",
        ] {
            assert!(match_consequential(text).is_some(), "{text}");
        }
    }

    #[test]
    fn ordinary_labels_do_not_match() {
        for text in [
            "",
            "Bold",
            "Open",
            "Save",
            "Cancel",
            "Close",
            "Next page",
            "Display settings",
            "Border",
            "Compose",
            "Document 1",
        ] {
            assert_eq!(match_consequential(text), None, "{text}");
        }
    }

    #[test]
    fn table_is_lower_case_and_free_of_duplicates() {
        let mut stems: Vec<&str> = CONSEQUENTIAL_PATTERNS
            .iter()
            .map(|entry| entry.stem)
            .collect();
        for stem in &stems {
            assert!(!stem.is_empty());
            assert!(
                stem.chars().all(|c| c.is_ascii_lowercase()),
                "{stem} must be lower-case ASCII letters"
            );
        }
        stems.sort_unstable();
        let before = stems.len();
        stems.dedup();
        assert_eq!(stems.len(), before);
    }
}

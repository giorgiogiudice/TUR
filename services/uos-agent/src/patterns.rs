// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The conservative rule set for accessibility and screen actions (addendum
//! section 5.1).
//!
//! A click is only a click, so a UI action cannot be classified from itself.
//! The engine looks at the name of the control and the title of the window:
//!
//! * If either contains a word that is in [`CONSEQUENTIAL_WORDS`], or that
//!   begins with one of the stems in [`CONSEQUENTIAL_PATTERNS`], the action is
//!   treated as class 2 and the user is asked first.
//! * If the name of the control cannot be read against the table at all
//!   ([`is_legible`]), the action is treated as class 2 as well: when in
//!   doubt, ask.
//!
//! The table is a heuristic, not a security boundary. It is English only. A
//! control with a misleading label, or a label in another language written in
//! plain ASCII letters, is not recognised. A false match costs one prompt.

/// What kind of consequential action a pattern suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternCategory {
    /// Sending or publishing something to other people.
    Send,
    /// Paying or moving money.
    Pay,
    /// Buying something.
    Purchase,
    /// Deleting, overwriting or resetting something.
    Delete,
    /// Installing, removing or updating software.
    Install,
    /// Submitting a form, agreeing to something, or granting something.
    Submit,
    /// Running a command or a program: a terminal, a console, a run dialog.
    Execute,
}

impl PatternCategory {
    /// Every category.
    pub const ALL: [PatternCategory; 7] = [
        PatternCategory::Send,
        PatternCategory::Pay,
        PatternCategory::Purchase,
        PatternCategory::Delete,
        PatternCategory::Install,
        PatternCategory::Submit,
        PatternCategory::Execute,
    ];

    /// The category as text, for the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            PatternCategory::Send => "send",
            PatternCategory::Pay => "pay",
            PatternCategory::Purchase => "purchase",
            PatternCategory::Delete => "delete",
            PatternCategory::Install => "install",
            PatternCategory::Submit => "submit",
            PatternCategory::Execute => "execute",
        }
    }
}

/// One entry of a pattern table: a lower-case stem or word and its category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsequentialPattern {
    /// What the entry suggests.
    pub category: PatternCategory,
    /// Lower-case ASCII letters. In [`CONSEQUENTIAL_PATTERNS`] a word matches
    /// if it begins with this; in [`CONSEQUENTIAL_WORDS`] a word matches if it
    /// is this.
    pub stem: &'static str,
}

const fn pattern(category: PatternCategory, stem: &'static str) -> ConsequentialPattern {
    ConsequentialPattern { category, stem }
}

/// The stems. A word matches if it begins with one of them.
///
/// Together with [`CONSEQUENTIAL_WORDS`] this is the only place the patterns
/// are defined.
pub const CONSEQUENTIAL_PATTERNS: &[ConsequentialPattern] = &[
    pattern(PatternCategory::Send, "send"),
    pattern(PatternCategory::Send, "resend"),
    pattern(PatternCategory::Send, "post"),
    pattern(PatternCategory::Send, "publish"),
    pattern(PatternCategory::Send, "share"),
    pattern(PatternCategory::Send, "upload"),
    pattern(PatternCategory::Send, "transmit"),
    pattern(PatternCategory::Send, "broadcast"),
    pattern(PatternCategory::Send, "invit"),
    pattern(PatternCategory::Pay, "pay"),
    pattern(PatternCategory::Pay, "donat"),
    pattern(PatternCategory::Pay, "transfer"),
    pattern(PatternCategory::Pay, "withdraw"),
    pattern(PatternCategory::Pay, "deposit"),
    pattern(PatternCategory::Pay, "refund"),
    pattern(PatternCategory::Purchase, "purchas"),
    pattern(PatternCategory::Purchase, "buy"),
    pattern(PatternCategory::Purchase, "order"),
    pattern(PatternCategory::Purchase, "checkout"),
    pattern(PatternCategory::Purchase, "subscri"),
    pattern(PatternCategory::Purchase, "unsubscri"),
    pattern(PatternCategory::Purchase, "reserv"),
    pattern(PatternCategory::Delete, "delet"),
    pattern(PatternCategory::Delete, "remov"),
    pattern(PatternCategory::Delete, "eras"),
    pattern(PatternCategory::Delete, "wipe"),
    pattern(PatternCategory::Delete, "destroy"),
    pattern(PatternCategory::Delete, "discard"),
    pattern(PatternCategory::Delete, "purg"),
    pattern(PatternCategory::Delete, "shred"),
    pattern(PatternCategory::Delete, "empty"),
    pattern(PatternCategory::Delete, "clear"),
    pattern(PatternCategory::Delete, "overwrit"),
    pattern(PatternCategory::Delete, "replac"),
    pattern(PatternCategory::Delete, "format"),
    pattern(PatternCategory::Delete, "partition"),
    pattern(PatternCategory::Delete, "reset"),
    pattern(PatternCategory::Delete, "restor"),
    pattern(PatternCategory::Delete, "revert"),
    pattern(PatternCategory::Delete, "rollback"),
    pattern(PatternCategory::Delete, "encrypt"),
    pattern(PatternCategory::Delete, "decrypt"),
    pattern(PatternCategory::Delete, "revok"),
    pattern(PatternCategory::Delete, "deactivat"),
    pattern(PatternCategory::Delete, "terminat"),
    pattern(PatternCategory::Delete, "unlink"),
    pattern(PatternCategory::Delete, "factory"),
    pattern(PatternCategory::Install, "install"),
    pattern(PatternCategory::Install, "uninstall"),
    pattern(PatternCategory::Install, "reinstall"),
    pattern(PatternCategory::Install, "upgrad"),
    pattern(PatternCategory::Install, "downgrad"),
    pattern(PatternCategory::Install, "updat"),
    pattern(PatternCategory::Install, "flash"),
    pattern(PatternCategory::Install, "firmware"),
    pattern(PatternCategory::Submit, "submit"),
    pattern(PatternCategory::Submit, "confirm"),
    pattern(PatternCategory::Submit, "accept"),
    pattern(PatternCategory::Submit, "agree"),
    pattern(PatternCategory::Submit, "sign"),
    pattern(PatternCategory::Submit, "login"),
    pattern(PatternCategory::Submit, "approv"),
    pattern(PatternCategory::Submit, "authori"),
    pattern(PatternCategory::Submit, "allow"),
    pattern(PatternCategory::Submit, "grant"),
    pattern(PatternCategory::Submit, "permit"),
    pattern(PatternCategory::Submit, "consent"),
    pattern(PatternCategory::Submit, "trust"),
    pattern(PatternCategory::Submit, "proceed"),
    pattern(PatternCategory::Submit, "continu"),
    pattern(PatternCategory::Submit, "finish"),
    pattern(PatternCategory::Submit, "complet"),
    pattern(PatternCategory::Submit, "verif"),
    pattern(PatternCategory::Submit, "register"),
    pattern(PatternCategory::Submit, "enrol"),
    pattern(PatternCategory::Submit, "vote"),
    pattern(PatternCategory::Execute, "terminal"),
    pattern(PatternCategory::Execute, "konsole"),
    pattern(PatternCategory::Execute, "console"),
    pattern(PatternCategory::Execute, "shell"),
    pattern(PatternCategory::Execute, "powershell"),
    pattern(PatternCategory::Execute, "command"),
    pattern(PatternCategory::Execute, "execut"),
    pattern(PatternCategory::Execute, "script"),
    pattern(PatternCategory::Execute, "macro"),
    pattern(PatternCategory::Execute, "sudo"),
    pattern(PatternCategory::Execute, "superuser"),
    pattern(PatternCategory::Execute, "administrat"),
];

/// Whole words. A word matches only if it is one of them.
///
/// These are too short to be stems (`ok` would match "Okular"), or they are
/// the generic affirmations of a dialog: what "OK" confirms cannot be read
/// from the button, so the user is asked.
pub const CONSEQUENTIAL_WORDS: &[ConsequentialPattern] = &[
    pattern(PatternCategory::Submit, "ok"),
    pattern(PatternCategory::Submit, "okay"),
    pattern(PatternCategory::Submit, "yes"),
    pattern(PatternCategory::Submit, "apply"),
    pattern(PatternCategory::Submit, "done"),
    pattern(PatternCategory::Pay, "tip"),
    pattern(PatternCategory::Pay, "bid"),
    pattern(PatternCategory::Purchase, "rent"),
    pattern(PatternCategory::Purchase, "book"),
    pattern(PatternCategory::Execute, "run"),
    pattern(PatternCategory::Execute, "exec"),
    pattern(PatternCategory::Execute, "cmd"),
    pattern(PatternCategory::Execute, "bash"),
    pattern(PatternCategory::Execute, "zsh"),
    pattern(PatternCategory::Execute, "root"),
];

/// The characters outside printable ASCII that an ordinary label may contain:
/// the no-break space, hyphens and dashes, typographic quotation marks and the
/// ellipsis.
pub const TYPOGRAPHIC_MARKS: &[char] = &[
    '\u{a0}', '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2018}',
    '\u{2019}', '\u{201c}', '\u{201d}', '\u{2026}',
];

/// The category of the first word in `text` that matches the pattern tables.
///
/// The text is lower-cased and split into words at every character that is not
/// a letter or a digit.
///
/// ```
/// use uos_agent::patterns::{PatternCategory, match_consequential};
///
/// assert_eq!(match_consequential("Send"), Some(PatternCategory::Send));
/// assert_eq!(match_consequential("Empty Trash…"), Some(PatternCategory::Delete));
/// assert_eq!(match_consequential("OK"), Some(PatternCategory::Submit));
/// assert_eq!(match_consequential("Okular"), None);
/// assert_eq!(match_consequential("Bold"), None);
/// ```
pub fn match_consequential(text: &str) -> Option<PatternCategory> {
    let lowered = text.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .find_map(category_of_word)
}

fn category_of_word(word: &str) -> Option<PatternCategory> {
    CONSEQUENTIAL_WORDS
        .iter()
        .find(|entry| word == entry.stem)
        .or_else(|| {
            CONSEQUENTIAL_PATTERNS
                .iter()
                .find(|entry| word.starts_with(entry.stem))
        })
        .map(|entry| entry.category)
}

/// Whether the name of a control can be read against the pattern tables.
///
/// The tables are ASCII words, so a name can be read only if it has at least
/// one ASCII letter or digit and consists of printable ASCII and the
/// [`TYPOGRAPHIC_MARKS`]. Everything else is illegible: an empty name, a name
/// that is only a symbol or an icon, a name in another script, and a name
/// with a look-alike letter, an accent, or a character that does not show
/// (a zero-width space, a soft hyphen, a direction override). The engine
/// treats acting on an illegible control as class 2.
///
/// ```
/// use uos_agent::patterns::is_legible;
///
/// assert!(is_legible("Bold"));
/// assert!(is_legible("Save As…"));
/// assert!(!is_legible(""));
/// assert!(!is_legible("\u{2192}")); // an arrow
/// assert!(!is_legible("\u{405}end")); // "Send" with a Cyrillic first letter
/// assert!(!is_legible("S\u{200b}end")); // a zero-width space inside
/// ```
pub fn is_legible(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_alphanumeric())
        && text
            .chars()
            .all(|c| matches!(c, ' '..='~') || TYPOGRAPHIC_MARKS.contains(&c))
}

#[cfg(any())]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_a_pattern_that_matches() {
        let examples = [
            ("Send", PatternCategory::Send),
            ("Pay now", PatternCategory::Pay),
            ("Complete purchase", PatternCategory::Submit),
            ("Purchase", PatternCategory::Purchase),
            ("Delete permanently", PatternCategory::Delete),
            ("Install", PatternCategory::Install),
            ("Submit form", PatternCategory::Submit),
            ("Open Terminal", PatternCategory::Execute),
        ];
        for (text, category) in examples {
            assert_eq!(match_consequential(text), Some(category), "{text}");
        }
        for category in PatternCategory::ALL {
            assert!(
                CONSEQUENTIAL_PATTERNS
                    .iter()
                    .chain(CONSEQUENTIAL_WORDS)
                    .any(|entry| entry.category == category),
                "{category:?}"
            );
        }
        let mut names: Vec<&str> = PatternCategory::ALL
            .iter()
            .map(|category| category.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PatternCategory::ALL.len());
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
    fn generic_affirmations_destructive_words_and_command_surfaces_match() {
        for text in [
            "OK",
            "Ok",
            "&OK",
            "Okay",
            "Yes",
            "Apply",
            "Done",
            "Continue",
            "Proceed",
            "Finish",
            "Allow",
            "Grant access",
            "Approve",
            "Authorize",
            "Authorise",
            "Trust this device",
            "Format",
            "Format disk…",
            "Create partition",
            "Overwrite",
            "Replace",
            "Reset to defaults",
            "Restore",
            "Encrypt",
            "Clear history",
            "Manage subscription",
            "Unsubscribe",
            "Update now",
            "Upgrade",
            "Run",
            "Run a Command",
            "Execute",
            "Terminal",
            "user@host: ~ — Konsole",
            "Command Prompt",
            "Windows PowerShell",
            "root@host",
            "Administrator: cmd",
        ] {
            assert!(match_consequential(text).is_some(), "{text}");
        }
    }

    #[test]
    fn a_whole_word_entry_is_not_a_stem() {
        for text in [
            "Okular",
            "Yesterday",
            "Application",
            "Applying",
            "Runtime",
            "Running",
            "Tips",
            "Bookmark",
            "Rental",
            "Rooted",
            "Bashful",
        ] {
            assert_eq!(match_consequential(text), None, "{text}");
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
            "Back",
            "Search",
            "Zoom in",
            "Help",
        ] {
            assert_eq!(match_consequential(text), None, "{text}");
        }
    }

    #[test]
    fn tables_are_lower_case_and_free_of_duplicates() {
        let mut stems: Vec<&str> = CONSEQUENTIAL_PATTERNS
            .iter()
            .chain(CONSEQUENTIAL_WORDS)
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

        // A whole-word entry that a stem already covers would be dead.
        for word in CONSEQUENTIAL_WORDS {
            assert!(
                !CONSEQUENTIAL_PATTERNS
                    .iter()
                    .any(|entry| word.stem.starts_with(entry.stem)),
                "{} is already covered by a stem",
                word.stem
            );
        }
    }

    #[test]
    fn a_name_is_legible_only_if_it_is_plain_text() {
        for text in [
            "Bold",
            "OK",
            "Save As…",
            "Buy\u{a0}now",
            "Don\u{2019}t save",
            "Zoom — 100%",
            "\u{201c}Quoted\u{201d}",
            "1",
            "A",
            "File (3)",
            "&Send",
        ] {
            assert!(is_legible(text), "{text:?}");
        }
        for text in [
            // Nothing to read.
            "",
            " ",
            "…",
            "?",
            "+",
            "---",
            "\u{2192}",
            "\u{2713}",
            "\u{1f5d1}",
            "\u{2699}\u{fe0f}",
            // Another script, or one letter of another script.
            "L\u{f6}schen",
            "Envoy\u{e9}",
            "\u{423}\u{434}\u{430}\u{43b}\u{438}\u{442}\u{44c}",
            "\u{5220}\u{9664}",
            "\u{405}end",
            "P\u{430}y",
            "\u{ff33}\u{ff45}\u{ff4e}\u{ff44}",
            // Characters that do not show, or that change what shows.
            "S\u{200b}end",
            "Se\u{ad}nd",
            "Se\u{301}nd",
            "Send\u{feff}",
            "\u{202e}dneS",
            "Send\u{e0020}",
            // Control characters.
            "Send\n",
            "Se\tnd",
            "Send\u{0}",
            "Send\u{7f}",
            "Send\u{85}",
            // An emoji next to text.
            "\u{1f50d} Search",
        ] {
            assert!(!is_legible(text), "{text:?}");
        }
    }
}

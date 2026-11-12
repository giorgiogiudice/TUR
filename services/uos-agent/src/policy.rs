// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The user-adjustable policy (addendum section 5.3) and its file format.
//!
//! The user can make the assistant stricter at any time, and can relax the
//! class 2 handling of accessibility and screen actions for a named
//! application. That is the only relaxation there is. Nothing in this file
//! format can relax class 3 or class 4. The file is owned by a separate system
//! component and written through system settings on the trusted path; the
//! assistant cannot write it (addendum section 17, item 5), and changing it is
//! a class 4 operation for the assistant.
//!
//! Two lists are built in and apply whatever the file says, an empty or
//! missing file included: [`DEFAULT_SENSITIVE_APPLICATIONS`] and
//! [`DEFAULT_EXCLUDED_APPLICATIONS`]. The user's lists add to them. Nothing in
//! the file format takes an entry away.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Problem};

/// The policy file schema version this build reads.
pub const POLICY_SCHEMA_VERSION: u64 = 1;

/// Applications that execute what is typed into them: terminal emulators, run
/// dialogs and launchers (addendum section 17, item 1).
///
/// Acting in one of them through the accessibility tree or the screen would be
/// a way to run any command with no sandbox and no approval. They are
/// therefore sensitive whatever the user policy says: every accessibility or
/// screen action in them is class 2. The assistant uses the command tier
/// instead, where the sandbox bounds what a command can do.
///
/// The entries are the application identifiers the compositor reports: the
/// desktop-file name on Wayland, the window class under XWayland. They are
/// compared like the user's own entries, ignoring ASCII case. The list cannot
/// be complete. An application that embeds a terminal, such as an editor with
/// a terminal panel, is not on it; the pattern table catches the usual names
/// of such panels, and the user can add the application.
pub const DEFAULT_SENSITIVE_APPLICATIONS: &[&str] = &[
    // KDE Plasma.
    "org.kde.konsole",
    "konsole",
    "org.kde.yakuake",
    "yakuake",
    "org.kde.krunner",
    "krunner",
    // GNOME.
    "org.gnome.Terminal",
    "gnome-terminal",
    "gnome-terminal-server",
    "org.gnome.Console",
    "kgx",
    "org.gnome.Ptyxis",
    "app.devsuite.Ptyxis",
    "ptyxis",
    // Other terminal emulators.
    "xterm",
    "uxterm",
    "kitty",
    "Alacritty",
    "foot",
    "footclient",
    "org.codeberg.dnkl.foot",
    "org.wezfurlong.wezterm",
    "wezterm",
    "com.mitchellh.ghostty",
    "ghostty",
    "com.gexperts.Tilix",
    "tilix",
    "terminator",
    "xfce4-terminal",
    "lxterminal",
    "qterminal",
    "mate-terminal",
    "deepin-terminal",
    "io.elementary.terminal",
    "com.raggesilver.BlackBox",
    "com.system76.CosmicTerm",
    "cool-retro-term",
    "urxvt",
    "rxvt",
    "st",
    "st-256color",
    "guake",
    "tilda",
    "terminology",
    "contour",
    "rio",
    "sakura",
    "termite",
    "roxterm",
    "mlterm",
    "zutty",
    "dev.warp.Warp",
    "tabby",
    "hyper",
    // Run dialogs and launchers.
    "rofi",
    "wofi",
    "dmenu",
    "dmenu_run",
    "bemenu",
    "bemenu-run",
    "fuzzel",
    "tofi",
    "io.ulauncher.Ulauncher",
    "ulauncher",
    "albert",
    "kupfer",
    "synapse",
    "gmrun",
    "xfce4-appfinder",
    "lxqt-runner",
    // Consoles of the Windows compatibility runtime.
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "wineconsole",
    "wineconsole.exe",
];

/// Applications whose windows hold secrets: password managers, the keyring,
/// authenticators, and the prompts that ask for a password (addendum
/// section 8).
///
/// They are excluded whatever the user policy says: the assistant neither
/// observes them nor acts in them. The prompts in the list are protected
/// surfaces, which the compositor already keeps from the assistant; listing
/// them here is a second layer. Private browser windows cannot be told from
/// ordinary ones by an application identifier and are the compositor's to
/// exclude.
pub const DEFAULT_EXCLUDED_APPLICATIONS: &[&str] = &[
    // Password managers and keyrings.
    "org.keepassxc.KeePassXC",
    "keepassxc",
    "org.keepass.KeePass",
    "keepass2",
    "org.kde.kwalletmanager",
    "org.kde.kwalletmanager5",
    "kwalletmanager",
    "kwalletmanager5",
    "org.kde.kwalletd5",
    "org.kde.kwalletd6",
    "kwalletd5",
    "kwalletd6",
    "org.kde.ksecretd",
    "ksecretd",
    "org.gnome.seahorse.Application",
    "seahorse",
    "org.gnome.World.Secrets",
    "org.gnome.PasswordSafe",
    "com.bitwarden.desktop",
    "bitwarden",
    "com.1password.1Password",
    "1password",
    "me.proton.Pass",
    "proton-pass",
    "com.enpass.Enpass",
    "enpass",
    "org.ijhack.QtPass",
    "qtpass",
    // One-time-password authenticators.
    "org.kde.keysmith",
    "keysmith",
    "com.belmoussaoui.Authenticator",
    "com.yubico.authenticator",
    "com.yubico.yubioath",
    // Prompts that ask for a password or a passphrase.
    "org.kde.polkit-kde-authentication-agent-1",
    "polkit-kde-authentication-agent-1",
    "polkit-gnome-authentication-agent-1",
    "org.kde.kscreenlocker_greet",
    "kscreenlocker_greet",
    "org.kde.ksshaskpass",
    "ksshaskpass",
    "ssh-askpass",
    "gcr-prompter",
    "org.gnome.keyring.SystemPrompter",
    "pinentry",
    "pinentry-qt",
    "pinentry-qt5",
    "pinentry-gnome3",
    "pinentry-gtk-2",
];

/// The global mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The rules of the addendum, unchanged.
    #[default]
    Normal,
    /// Ask before every action, of every class.
    AskEveryAction,
    /// Only class 0 actions are available.
    ObserveOnly,
}

impl Mode {
    /// Every mode.
    pub const ALL: [Mode; 3] = [Mode::Normal, Mode::AskEveryAction, Mode::ObserveOnly];

    /// The mode as written in the policy file.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::AskEveryAction => "ask_every_action",
            Mode::ObserveOnly => "observe_only",
        }
    }
}

/// How far a relaxation goes.
///
/// There is one level. Class 2 handling of accessibility and screen actions in
/// a named application is the only standing relaxation the design has
/// (addendum sections 5.3 and 17, item 7); there is no level for class 3 or
/// class 4, and the type has no value for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RelaxationLevel {
    /// Class 2: consequential accessibility and screen actions in the named
    /// application run without asking, in normal mode, while attended.
    Consequential,
}

impl RelaxationLevel {
    /// Every level.
    pub const ALL: [RelaxationLevel; 1] = [RelaxationLevel::Consequential];

    /// The class number written in the policy file.
    pub const fn class_number(self) -> u8 {
        match self {
            RelaxationLevel::Consequential => 2,
        }
    }
}

/// A relaxation for one named application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaxation {
    /// The application identifier. Matched exactly, including case.
    pub application: String,
    /// The highest class the relaxation covers.
    pub level: RelaxationLevel,
}

/// The user's policy.
///
/// The default is normal mode with no exclusions, sensitive applications or
/// relaxations of the user's own. The built-in lists apply all the same.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserPolicy {
    /// The global mode.
    pub mode: Mode,
    /// Applications the assistant must neither observe nor act in, in addition
    /// to [`DEFAULT_EXCLUDED_APPLICATIONS`].
    pub excluded_applications: Vec<String>,
    /// Applications in which every action is treated as class 2 at least, in
    /// addition to [`DEFAULT_SENSITIVE_APPLICATIONS`].
    pub sensitive_applications: Vec<String>,
    /// Per-application relaxations.
    pub relaxations: Vec<Relaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    schema_version: u64,
    mode: Mode,
    #[serde(default)]
    excluded_applications: Vec<String>,
    #[serde(default)]
    sensitive_applications: Vec<String>,
    #[serde(default)]
    relaxations: Vec<RawRelaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelaxation {
    application: String,
    class: i64,
}

impl UserPolicy {
    /// The policy to use when the policy file cannot be read or is invalid:
    /// observe-only.
    pub fn fail_closed() -> Self {
        UserPolicy {
            mode: Mode::ObserveOnly,
            ..UserPolicy::default()
        }
    }

    /// Load a policy from the text of a policy file.
    ///
    /// Returns every problem found, not only the first.
    pub fn from_json(text: &str) -> Result<Self, Vec<Problem>> {
        let raw: RawPolicy = serde_json::from_str(text)
            .map_err(|err| vec![Problem::new(Code::PolicyFileParse, err.to_string())])?;

        let mut problems = Vec::new();
        if raw.schema_version != POLICY_SCHEMA_VERSION {
            problems.push(Problem::new(
                Code::PolicyFileVersion,
                format!(
                    "schema_version is {}, this build reads {POLICY_SCHEMA_VERSION}",
                    raw.schema_version
                ),
            ));
        }

        let lists = [
            ("excluded_applications", &raw.excluded_applications),
            ("sensitive_applications", &raw.sensitive_applications),
        ];
        for (list, applications) in lists {
            for (index, application) in applications.iter().enumerate() {
                if !is_plain_application_id(application) {
                    problems.push(Problem::new(
                        Code::PolicyFileApplication,
                        format!("{list}[{index}]: {APPLICATION_RULE}"),
                    ));
                }
            }
        }

        let mut relaxed = BTreeSet::new();
        let mut relaxations = Vec::with_capacity(raw.relaxations.len());
        for (index, relaxation) in raw.relaxations.into_iter().enumerate() {
            if !is_plain_application_id(&relaxation.application) {
                problems.push(Problem::new(
                    Code::PolicyFileApplication,
                    format!("relaxations[{index}].application: {APPLICATION_RULE}"),
                ));
                continue;
            }
            // Two entries that differ only in the way exclusion and
            // sensitivity ignore are the same application written twice.
            if !relaxed.insert(canonical(&relaxation.application).to_ascii_lowercase()) {
                problems.push(Problem::new(
                    Code::PolicyFileRelaxation,
                    format!("relaxations[{index}]: the application already has a relaxation"),
                ));
                continue;
            }
            if relaxation.class != i64::from(RelaxationLevel::Consequential.class_number()) {
                problems.push(Problem::new(
                    Code::PolicyFileRelaxation,
                    format!(
                        "relaxations[{index}].class is {}; only class 2 can be relaxed",
                        relaxation.class
                    ),
                ));
                continue;
            }
            relaxations.push(Relaxation {
                application: relaxation.application,
                level: RelaxationLevel::Consequential,
            });
        }

        if problems.is_empty() {
            Ok(UserPolicy {
                mode: raw.mode,
                excluded_applications: raw.excluded_applications,
                sensitive_applications: raw.sensitive_applications,
                relaxations,
            })
        } else {
            Err(problems)
        }
    }

    /// Whether the application is excluded: by the user, or by
    /// [`DEFAULT_EXCLUDED_APPLICATIONS`].
    ///
    /// Compared without regard to ASCII case or surrounding whitespace, and an
    /// identifier also matches its own form with `.desktop` appended, so that
    /// the same application reported in another form is still excluded.
    pub fn is_excluded(&self, application: &str) -> bool {
        self.excluded_applications
            .iter()
            .any(|entry| same_application(entry, application))
            || DEFAULT_EXCLUDED_APPLICATIONS
                .iter()
                .any(|entry| same_application(entry, application))
    }

    /// Whether every accessibility and screen action in the application is
    /// treated as class 2 at least.
    ///
    /// True if the user marked the application sensitive, if it is one of the
    /// [`DEFAULT_SENSITIVE_APPLICATIONS`], or if its identifier is not a plain
    /// one ([`is_plain_application_id`]): such an identifier cannot be
    /// compared reliably with any list, so it is not known what the
    /// application is. Compared like [`UserPolicy::is_excluded`].
    pub fn is_sensitive(&self, application: &str) -> bool {
        !is_plain_application_id(application)
            || self
                .sensitive_applications
                .iter()
                .any(|entry| same_application(entry, application))
            || DEFAULT_SENSITIVE_APPLICATIONS
                .iter()
                .any(|entry| same_application(entry, application))
    }

    /// The relaxation for the application, if any.
    ///
    /// Compared exactly, so that a near-identical identifier is not relaxed.
    pub fn relaxation_for(&self, application: &str) -> Option<RelaxationLevel> {
        self.relaxations
            .iter()
            .filter(|relaxation| relaxation.application == application)
            .map(|relaxation| relaxation.level)
            .min()
    }
}

const APPLICATION_RULE: &str = "an application identifier must be 1 to 255 printable ASCII characters with no surrounding space";

const DESKTOP_SUFFIX: &str = ".desktop";

/// Whether an application identifier is a plain one: 1 to 255 printable ASCII
/// characters, with no space at either end.
///
/// The identifiers desktops use are of this kind. One that is not (a letter of
/// another script, a character that does not show, a control character) may
/// look the same as a listed identifier without being equal to it. The policy
/// file therefore accepts plain identifiers only, and the engine treats an
/// application whose reported identifier is not plain as sensitive.
///
/// ```
/// use uos_agent::policy::is_plain_application_id;
///
/// assert!(is_plain_application_id("org.kde.konsole"));
/// assert!(is_plain_application_id("VirtualBox Manager"));
/// assert!(!is_plain_application_id(""));
/// assert!(!is_plain_application_id(" org.kde.konsole"));
/// assert!(!is_plain_application_id("org.kde.konsole\u{200b}"));
/// ```
pub fn is_plain_application_id(application: &str) -> bool {
    !application.is_empty()
        && application.len() <= 255
        && application.bytes().all(|b| matches!(b, b' '..=b'~'))
        && !application.starts_with(' ')
        && !application.ends_with(' ')
}

/// `application` without surrounding whitespace and without a trailing
/// `.desktop`, which some toolkits append to the identifier they report. Used
/// to recognise one application written twice in the list of relaxations.
fn canonical(application: &str) -> &str {
    let trimmed = application.trim();
    let split = trimmed.len().saturating_sub(DESKTOP_SUFFIX.len());
    if split > 0
        && trimmed.is_char_boundary(split)
        && trimmed[split..].eq_ignore_ascii_case(DESKTOP_SUFFIX)
    {
        &trimmed[..split]
    } else {
        trimmed
    }
}

/// Whether two identifiers name the same application: equal without regard to
/// ASCII case and surrounding whitespace, or one is the other followed by
/// `.desktop`.
fn same_application(left: &str, right: &str) -> bool {
    let (left, right) = (left.trim(), right.trim());
    left.eq_ignore_ascii_case(right)
        || is_desktop_form(left, right)
        || is_desktop_form(right, left)
}

/// Whether `longer` is `shorter` followed by `.desktop`.
fn is_desktop_form(shorter: &str, longer: &str) -> bool {
    !shorter.is_empty()
        && longer.len() == shorter.len() + DESKTOP_SUFFIX.len()
        && longer.is_char_boundary(shorter.len())
        && longer[..shorter.len()].eq_ignore_ascii_case(shorter)
        && longer[shorter.len()..].eq_ignore_ascii_case(DESKTOP_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(result: Result<UserPolicy, Vec<Problem>>) -> Vec<Code> {
        result
            .expect_err("the policy must be rejected")
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn loads_a_complete_policy() {
        let policy = UserPolicy::from_json(
            r#"{
                "schema_version": 1,
                "mode": "ask_every_action",
                "excluded_applications": ["org.example.Vault"],
                "sensitive_applications": ["org.example.Bank"],
                "relaxations": [
                    { "application": "org.example.Mail", "class": 2 }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(policy.mode, Mode::AskEveryAction);
        assert!(policy.is_excluded("org.example.Vault"));
        assert!(policy.is_sensitive("org.example.Bank"));
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Consequential)
        );
        assert_eq!(policy.relaxation_for("org.example.Other"), None);
    }

    #[test]
    fn lists_are_optional_but_mode_and_version_are_not() {
        let policy = UserPolicy::from_json(r#"{ "schema_version": 1, "mode": "normal" }"#).unwrap();
        assert_eq!(policy, UserPolicy::default());

        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "schema_version": 1 }"#)),
            [Code::PolicyFileParse]
        );
        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "mode": "normal" }"#)),
            [Code::PolicyFileParse]
        );
        assert_eq!(codes(UserPolicy::from_json("{}")), [Code::PolicyFileParse]);
        assert_eq!(codes(UserPolicy::from_json("")), [Code::PolicyFileParse]);
    }

    #[test]
    fn every_mode_round_trips_through_its_file_name() {
        for mode in Mode::ALL {
            let text = format!(r#"{{ "schema_version": 1, "mode": "{}" }}"#, mode.as_str());
            assert_eq!(UserPolicy::from_json(&text).unwrap().mode, mode);
        }
    }

    #[test]
    fn rejects_unknown_fields_modes_and_versions() {
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal", "allow_everything": true }"#
            )),
            [Code::PolicyFileParse]
        );
        for mode in ["relaxed", "Normal", "NORMAL", " normal", "normal ", ""] {
            let text = format!(r#"{{ "schema_version": 1, "mode": "{mode}" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "{mode:?}"
            );
        }
        for version in ["0", "2", "18446744073709551615"] {
            let text = format!(r#"{{ "schema_version": {version}, "mode": "normal" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileVersion],
                "{version}"
            );
        }
        for version in ["-1", "1.0", "\"1\"", "null", "true", "[1]"] {
            let text = format!(r#"{{ "schema_version": {version}, "mode": "normal" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "{version}"
            );
        }
        assert_eq!(
            codes(UserPolicy::from_json("not json")),
            [Code::PolicyFileParse]
        );
    }

    #[test]
    fn a_repeated_field_is_an_error_and_never_the_last_or_the_first_value() {
        for text in [
            r#"{ "schema_version": 1, "mode": "observe_only", "mode": "normal" }"#,
            r#"{ "schema_version": 1, "mode": "normal", "mode": "observe_only" }"#,
            r#"{ "schema_version": 2, "schema_version": 1, "mode": "normal" }"#,
            r#"{ "schema_version": 1, "mode": "normal",
                 "excluded_applications": ["org.example.Vault"], "excluded_applications": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal",
                 "sensitive_applications": ["org.example.Bank"], "sensitive_applications": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [], "relaxations": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [
                 { "application": "org.example.Mail", "class": 3, "class": 2 } ] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [
                 { "application": "org.example.Bank", "application": "org.example.Mail", "class": 2 } ] }"#,
        ] {
            assert_eq!(
                codes(UserPolicy::from_json(text)),
                [Code::PolicyFileParse],
                "{text}"
            );
        }
    }

    #[test]
    fn only_class_2_can_be_relaxed() {
        for class in ["-1", "0", "1", "3", "4", "5", "9223372036854775807"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "org.example.App", "class": {class} }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileRelaxation],
                "class {class}"
            );
        }
        for class in ["2.0", "\"2\"", "null", "[2]", "9223372036854775808"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "org.example.App", "class": {class} }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "class {class}"
            );
        }
        assert_eq!(RelaxationLevel::ALL.len(), 1);
        assert_eq!(RelaxationLevel::Consequential.class_number(), 2);
    }

    #[test]
    fn rejects_duplicate_relaxations_and_bad_application_names() {
        for second in [
            "org.example.App",
            "org.example.app",
            "ORG.EXAMPLE.APP",
            "org.example.App.desktop",
        ] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal", "relaxations": [
                    {{ "application": "org.example.App", "class": 2 }},
                    {{ "application": "{second}", "class": 2 }}
                ] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileRelaxation],
                "{second}"
            );
        }
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal",
                     "excluded_applications": ["", " padded "],
                     "sensitive_applications": ["line\nbreak"],
                     "relaxations": [{ "application": "", "class": 2 }] }"#
            )),
            [
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication
            ]
        );
    }

    #[test]
    fn an_identifier_that_is_not_plain_is_rejected_in_every_list() {
        let long = "a".repeat(256);
        for application in [
            "org.example.Vault\\u200b",
            "\\ufefforg.example.Vault",
            "org.ex\\u0430mple.Vault",
            "org.example.V\\u00e4ult",
            "org.example.Vault\\u00a0",
            "org.example.Vault\\t",
            "org.example.Vault\\u007f",
            "org.example\\u202e.Vault",
            long.as_str(),
        ] {
            for list in ["excluded_applications", "sensitive_applications"] {
                let text = format!(
                    r#"{{ "schema_version": 1, "mode": "normal", "{list}": ["{application}"] }}"#
                );
                assert_eq!(
                    codes(UserPolicy::from_json(&text)),
                    [Code::PolicyFileApplication],
                    "{list}: {application}"
                );
            }
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "{application}", "class": 2 }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileApplication],
                "relaxations: {application}"
            );
        }
        // The longest identifier, and one with a space inside, are accepted.
        let longest = "a".repeat(255);
        for application in [longest.as_str(), "VirtualBox Manager", "a"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal", "excluded_applications": ["{application}"] }}"#
            );
            assert!(UserPolicy::from_json(&text).is_ok(), "{application}");
        }
    }

    #[test]
    fn exclusion_matches_loosely_and_relaxation_exactly() {
        let policy = UserPolicy {
            mode: Mode::Normal,
            excluded_applications: vec!["org.example.Vault".to_owned()],
            sensitive_applications: vec!["org.example.Bank".to_owned()],
            relaxations: vec![Relaxation {
                application: "org.example.Mail".to_owned(),
                level: RelaxationLevel::Consequential,
            }],
        };
        for application in [
            "org.example.Vault",
            "ORG.EXAMPLE.VAULT",
            " org.example.vault ",
            "\torg.example.Vault\n",
            "org.example.Vault.desktop",
            "org.example.vault.DESKTOP",
            " org.example.Vault.desktop ",
        ] {
            assert!(policy.is_excluded(application), "{application:?}");
        }
        for application in [
            "org.example.Vault2",
            "org.example.Vaul",
            "org.example",
            "xorg.example.Vault",
            "org.example.Vault.desktop.desktop",
            "org.example.Vault.desktopx",
            "org.example.Vault desktop",
            ".desktop",
            "",
        ] {
            assert!(!policy.is_excluded(application), "{application:?}");
        }
        assert!(policy.is_sensitive("org.example.bank"));
        assert!(policy.is_sensitive("org.example.Bank.desktop"));
        assert!(!policy.is_sensitive("org.example.Banks"));

        assert_eq!(policy.relaxation_for("org.example.mail"), None);
        assert_eq!(policy.relaxation_for(" org.example.Mail"), None);
        assert_eq!(policy.relaxation_for("org.example.Mail.desktop"), None);
    }

    #[test]
    fn an_application_that_cannot_be_identified_is_sensitive() {
        let policy = UserPolicy::default();
        for application in [
            "",
            " org.example.Editor",
            "org.example.Editor ",
            "org.example.Editor\u{200b}",
            "org.ex\u{430}mple.Editor",
            "org.example.Edit\u{f6}r",
            "org.example.Editor\n",
            "\u{feff}org.example.Editor",
        ] {
            assert!(policy.is_sensitive(application), "{application:?}");
        }
        let too_long = "a".repeat(256);
        assert!(policy.is_sensitive(&too_long));
        assert!(!policy.is_sensitive("org.example.Editor"));
        assert!(!policy.is_sensitive(&"a".repeat(255)));
    }

    #[test]
    fn the_built_in_lists_apply_to_every_policy() {
        for policy in [UserPolicy::default(), UserPolicy::fail_closed()] {
            for application in DEFAULT_SENSITIVE_APPLICATIONS {
                assert!(policy.is_sensitive(application), "{application}");
                assert!(
                    policy.is_sensitive(&application.to_ascii_uppercase()),
                    "{application}"
                );
                assert!(
                    policy.is_sensitive(&format!("{application}.desktop")),
                    "{application}"
                );
            }
            for application in DEFAULT_EXCLUDED_APPLICATIONS {
                assert!(policy.is_excluded(application), "{application}");
                assert!(
                    policy.is_excluded(&application.to_ascii_lowercase()),
                    "{application}"
                );
                // Also an entry that itself ends in ".desktop".
                assert!(
                    policy.is_excluded(&format!("{application}.desktop")),
                    "{application}"
                );
            }
            assert!(!policy.is_sensitive("org.kde.kate"));
            assert!(!policy.is_excluded("org.kde.kate"));
            // An entry is a whole identifier, not the beginning of one.
            assert!(!policy.is_sensitive("stellarium"));
            assert!(!policy.is_sensitive("org.kde.konsolepart"));
        }
    }

    #[test]
    fn the_built_in_lists_are_plain_and_free_of_duplicates() {
        for list in [DEFAULT_SENSITIVE_APPLICATIONS, DEFAULT_EXCLUDED_APPLICATIONS] {
            let mut seen = BTreeSet::new();
            for application in list {
                assert!(is_plain_application_id(application), "{application}");
                assert!(
                    seen.insert(application.to_ascii_lowercase()),
                    "{application} is listed twice"
                );
            }
        }
        // The two lists do not overlap: an excluded application is refused
        // before its sensitivity is looked at.
        for application in DEFAULT_SENSITIVE_APPLICATIONS {
            assert!(
                !DEFAULT_EXCLUDED_APPLICATIONS
                    .iter()
                    .any(|entry| same_application(entry, application)),
                "{application}"
            );
        }
        // The surfaces named in the design.
        for application in ["org.kde.konsole", "org.kde.krunner", "org.gnome.Terminal"] {
            assert!(DEFAULT_SENSITIVE_APPLICATIONS.contains(&application));
        }
    }

    #[test]
    fn a_policy_built_in_code_may_name_an_application_twice() {
        let policy = UserPolicy {
            relaxations: vec![
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Consequential,
                },
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Consequential,
                },
            ],
            ..UserPolicy::default()
        };
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Consequential)
        );
    }

    #[test]
    fn fail_closed_is_observe_only() {
        assert_eq!(UserPolicy::fail_closed().mode, Mode::ObserveOnly);
    }
}

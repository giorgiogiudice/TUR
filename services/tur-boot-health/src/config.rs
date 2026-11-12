// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The gate's configuration (design section 6.5).
//!
//! Configuration is part of the image, under `/usr/lib/tur/boot-health/`.
//! There is no `/etc` override: a local override would be merged into every
//! later deployment and could fail every future update.
//!
//! The files are TOML. This reader understands the subset they use: tables,
//! and keys whose values are strings, integers, booleans or arrays of strings
//! on one line. A file it cannot read is an error, never silently ignored.

use std::collections::BTreeMap;

/// The directory in the image that holds the configuration.
pub(crate) const CONFIG_DIR: &str = "/usr/lib/tur/boot-health";

/// A value in a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Text(String),
    Number(i64),
    Flag(bool),
    List(Vec<String>),
}

/// A parsed file: `table.key` (or `key` before any table) to value.
pub(crate) type Table = BTreeMap<String, Item>;

fn parse_string(text: &str) -> Result<(String, &str), String> {
    // `text` starts after the opening quote. Returns the string and the rest.
    let mut out = String::new();
    let mut chars = text.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => return Ok((out, &text[index + 1..])),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                _ => return Err("unknown escape in a string".to_owned()),
            },
            ch => out.push(ch),
        }
    }
    Err("unterminated string".to_owned())
}

fn rest_is_empty(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

fn parse_item(text: &str) -> Result<Item, String> {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('"') {
        let (value, rest) = parse_string(inner)?;
        if !rest_is_empty(rest) {
            return Err("unexpected text after a string".to_owned());
        }
        return Ok(Item::Text(value));
    }
    if let Some(mut inner) = text.strip_prefix('[') {
        let mut items = Vec::new();
        loop {
            inner = inner.trim_start();
            if let Some(rest) = inner.strip_prefix(']') {
                if !rest_is_empty(rest) {
                    return Err("unexpected text after an array".to_owned());
                }
                return Ok(Item::List(items));
            }
            let Some(after_quote) = inner.strip_prefix('"') else {
                return Err("an array may hold strings only".to_owned());
            };
            let (value, rest) = parse_string(after_quote)?;
            items.push(value);
            inner = rest.trim_start();
            if let Some(rest) = inner.strip_prefix(',') {
                inner = rest;
            } else if !inner.starts_with(']') {
                return Err("expected ',' or ']' in an array".to_owned());
            }
        }
    }
    let bare = match text.split_once('#') {
        Some((before, _)) => before.trim(),
        None => text,
    };
    match bare {
        "true" => Ok(Item::Flag(true)),
        "false" => Ok(Item::Flag(false)),
        _ => bare
            .parse::<i64>()
            .map(Item::Number)
            .map_err(|_| format!("value {bare:?} is not a string, integer, boolean or array")),
    }
}

/// Reads a configuration file.
pub(crate) fn parse_table(text: &str) -> Result<Table, String> {
    let mut table = Table::new();
    let mut prefix = String::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fail = |what: String| format!("line {}: {what}", number + 1);
        if let Some(rest) = line.strip_prefix('[') {
            let Some((name, after)) = rest.split_once(']') else {
                return Err(fail("unterminated table name".to_owned()));
            };
            if !rest_is_empty(after) || name.trim().is_empty() {
                return Err(fail("malformed table name".to_owned()));
            }
            prefix = format!("{}.", name.trim());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(fail("expected key = value".to_owned()));
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(fail(format!("malformed key {key:?}")));
        }
        let item = parse_item(value).map_err(fail)?;
        if table.insert(format!("{prefix}{key}"), item).is_some() {
            return Err(fail(format!("{prefix}{key} is set twice")));
        }
    }
    Ok(table)
}

/// The time limits of the required checks, in seconds (design section 6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) root: u64,
    pub(crate) paths: u64,
    pub(crate) default_target: u64,
    pub(crate) core_units: u64,
    pub(crate) tur_units: u64,
    pub(crate) display_manager: u64,
    pub(crate) seat_session: u64,
    pub(crate) stability: u64,
    /// How long the seat's session must stay unchanged.
    pub(crate) hold: u64,
}

/// `gate.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GateConfig {
    /// Attempts a new deployment gets before GRUB starts the previous one.
    pub(crate) attempts: i64,
    /// Kernel arguments added to every trial boot (design section 5.4).
    pub(crate) trial_args: String,
    pub(crate) limits: Limits,
}

impl Default for GateConfig {
    /// The values of the design, used when the image has no `gate.toml`.
    fn default() -> GateConfig {
        GateConfig {
            attempts: 3,
            trial_args: "panic=30 rd.shell=0 rd.emergency=reboot systemd.watchdog_sec=180"
                .to_owned(),
            limits: Limits {
                root: 20,
                paths: 5,
                default_target: 240,
                core_units: 60,
                tur_units: 60,
                display_manager: 60,
                seat_session: 90,
                stability: 20,
                hold: 15,
            },
        }
    }
}

impl GateConfig {
    /// Reads `gate.toml`. Keys that are absent keep the design's values; a
    /// key that is not known, or has the wrong kind of value, is an error.
    pub(crate) fn parse(text: &str) -> Result<GateConfig, String> {
        let table = parse_table(text)?;
        let mut config = GateConfig::default();
        for (key, item) in &table {
            let number = |low: i64, high: i64| -> Result<u64, String> {
                match item {
                    Item::Number(value) if (low..=high).contains(value) => Ok(*value as u64),
                    _ => Err(format!("{key} must be a number from {low} to {high}")),
                }
            };
            match key.as_str() {
                "schema_version" => {
                    if *item != Item::Number(1) {
                        return Err("schema_version must be 1".to_owned());
                    }
                }
                "attempts" => config.attempts = number(1, 9)? as i64,
                "trial_args" => match item {
                    Item::Text(value) if !value.contains('\n') && value.len() <= 512 => {
                        config.trial_args = value.clone();
                    }
                    _ => return Err("trial_args must be a one-line string".to_owned()),
                },
                "limits.root" => config.limits.root = number(1, 3600)?,
                "limits.paths" => config.limits.paths = number(1, 3600)?,
                "limits.default_target" => config.limits.default_target = number(1, 3600)?,
                "limits.core_units" => config.limits.core_units = number(1, 3600)?,
                "limits.tur_units" => config.limits.tur_units = number(1, 3600)?,
                "limits.display_manager" => config.limits.display_manager = number(1, 3600)?,
                "limits.seat_session" => config.limits.seat_session = number(1, 3600)?,
                "limits.stability" => config.limits.stability = number(1, 3600)?,
                "limits.hold" => config.limits.hold = number(0, 3600)?,
                other => return Err(format!("unknown key {other}")),
            }
        }
        if config.limits.hold >= config.limits.stability {
            return Err("limits.hold must be shorter than limits.stability".to_owned());
        }
        Ok(config)
    }
}

/// An additional check from `checks.d` (design section 6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtraCheck {
    /// The file name without `.toml`.
    pub(crate) name: String,
    /// The executable. Run without a shell.
    pub(crate) exec: String,
    pub(crate) args: Vec<String>,
    /// A required check that fails, fails the gate. Others only mark the
    /// boot degraded.
    pub(crate) required: bool,
    /// Seconds.
    pub(crate) timeout: u64,
}

impl ExtraCheck {
    /// Reads one file of `checks.d`.
    pub(crate) fn parse(name: &str, text: &str) -> Result<ExtraCheck, String> {
        let table = parse_table(text)?;
        let mut check = ExtraCheck {
            name: name.to_owned(),
            exec: String::new(),
            args: Vec::new(),
            required: false,
            timeout: 30,
        };
        for (key, item) in &table {
            match (key.as_str(), item) {
                ("exec", Item::Text(value)) => check.exec = value.clone(),
                ("args", Item::List(values)) => check.args = values.clone(),
                ("required", Item::Flag(value)) => check.required = *value,
                ("timeout", Item::Number(value)) if (1..=600).contains(value) => {
                    check.timeout = *value as u64;
                }
                (other, _) => return Err(format!("unknown or malformed key {other}")),
            }
        }
        if !check.exec.starts_with('/') {
            return Err("exec must be an absolute path".to_owned());
        }
        Ok(check)
    }
}

/// The unit a file of `required-units.d` names: its first line that is not
/// empty and not a comment.
pub(crate) fn parse_required_unit(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gives_the_design_values() {
        let config = GateConfig::parse("# nothing\n").unwrap();
        assert_eq!(config, GateConfig::default());
        assert_eq!(config.attempts, 3);
        assert_eq!(config.limits.default_target, 240);
        assert_eq!(config.limits.hold, 15);
        assert_eq!(
            config.trial_args,
            "panic=30 rd.shell=0 rd.emergency=reboot systemd.watchdog_sec=180"
        );
    }

    #[test]
    fn the_limits_of_the_design_add_up_to_less_than_the_unit_timeout() {
        let limits = GateConfig::default().limits;
        let total = limits.root
            + limits.paths
            + limits.default_target
            + limits.core_units
            + limits.tur_units
            + limits.display_manager
            + limits.seat_session
            + limits.stability;
        assert_eq!(total, 555);
        assert!(total < 600);
    }

    #[test]
    fn values_can_be_changed() {
        let config = GateConfig::parse(
            "schema_version = 1\nattempts = 2 # fewer\ntrial_args = \"panic=5\"\n\n[limits]\n\
             default_target = 40\nhold = 3\nstability = 8\n",
        )
        .unwrap();
        assert_eq!(config.attempts, 2);
        assert_eq!(config.trial_args, "panic=5");
        assert_eq!(config.limits.default_target, 40);
        assert_eq!(config.limits.hold, 3);
        assert_eq!(config.limits.seat_session, 90);
    }

    #[test]
    fn mistakes_are_errors_not_defaults() {
        for text in [
            "attempts = 0",
            "attempts = \"three\"",
            "unknown = 1",
            "[limits]\nroot = -1",
            "[limits]\nhold = 30",
            "schema_version = 2",
            "attempts 3",
            "attempts = 3\nattempts = 3",
            "trial_args = \"unterminated",
            "[limits",
        ] {
            assert!(GateConfig::parse(text).is_err(), "{text:?} was accepted");
        }
    }

    #[test]
    fn an_extra_check_is_read() {
        let check = ExtraCheck::parse(
            "storage",
            "exec = \"/usr/libexec/tur/check-storage\"\nargs = [\"--quick\", \"a b\"]\n\
             required = true\ntimeout = 20\n",
        )
        .unwrap();
        assert_eq!(check.name, "storage");
        assert_eq!(check.exec, "/usr/libexec/tur/check-storage");
        assert_eq!(check.args, vec!["--quick".to_owned(), "a b".to_owned()]);
        assert!(check.required);
        assert_eq!(check.timeout, 20);
    }

    #[test]
    fn an_extra_check_needs_an_absolute_executable() {
        assert!(ExtraCheck::parse("x", "exec = \"false\"").is_err());
        assert!(ExtraCheck::parse("x", "required = true").is_err());
        assert!(ExtraCheck::parse("x", "exec = \"/bin/false\"\nshell = true").is_err());
        assert!(ExtraCheck::parse("x", "exec = \"/bin/false\"\ntimeout = 0").is_err());
        let wanted = ExtraCheck::parse("x", "exec = \"/bin/false\"").unwrap();
        assert!(!wanted.required);
        assert_eq!(wanted.timeout, 30);
    }

    #[test]
    fn a_required_unit_file_names_one_unit() {
        assert_eq!(
            parse_required_unit("# the storage manager\n\ntur-storage.service\nignored\n"),
            Some("tur-storage.service".to_owned())
        );
        assert_eq!(parse_required_unit("# only a comment\n"), None);
    }

    #[test]
    fn arrays_and_strings_are_strict() {
        assert_eq!(
            parse_item("[]").unwrap(),
            Item::List(Vec::new())
        );
        assert_eq!(
            parse_item("[\"a\", \"b\",]").unwrap(),
            Item::List(vec!["a".to_owned(), "b".to_owned()])
        );
        assert!(parse_item("[1]").is_err());
        assert!(parse_item("[\"a\" \"b\"]").is_err());
        assert!(parse_item("\"a\" b").is_err());
        assert_eq!(parse_item("\"a\\\"b\" # c").unwrap(), Item::Text("a\"b".to_owned()));
    }
}

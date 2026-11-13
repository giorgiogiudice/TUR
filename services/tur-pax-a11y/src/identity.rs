// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Who an application is: every identifier the policy engine's lists might
//! know it by, and whose process it is.
//!
//! The accessibility bus does not say which desktop file an application
//! belongs to. What it does say, from its own record of each connection, is
//! the process and the user (`GetConnectionUnixProcessID`,
//! `GetConnectionUnixUser`). From the process the kernel gives the program
//! (`/proc/<pid>/exe`, `comm`, the first argument), a Flatpak sandbox its
//! application identifier (`/.flatpak-info`), and the desktop files on the
//! system say which of them start that program. All of these are offered to
//! the policy engine; one match against a list is enough (the stricter
//! reading). The application's own name for itself on the bus is added too:
//! an application can choose it, so it can only add a match, never remove
//! one.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Whose process owns a window, in the words of the bridge protocol.
pub fn owner_word(user: Option<u32>, own: Option<u32>) -> &'static str {
    match (user, own) {
        (Some(0), _) => "administrator",
        (Some(user), Some(own)) if user == own => "user",
        _ => "unknown",
    }
}

/// The directories desktop files are read from, in order.
pub const DESKTOP_DIRECTORIES: [&str; 4] = [
    "/usr/share/applications",
    "/usr/local/share/applications",
    "/var/lib/flatpak/exports/share/applications",
    "/etc/xdg/autostart",
];

/// The most desktop files read, so that a directory full of them does not
/// hold the bridge up.
pub const MAX_DESKTOP_FILES: usize = 4000;

/// What a desktop file says starts which program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopTable {
    /// Desktop file identifier, the program its `Exec` line starts, and its
    /// `StartupWMClass`.
    pub entries: Vec<(String, String, String)>,
}

impl DesktopTable {
    /// Read the desktop files of the system and of the user.
    pub fn load() -> Self {
        let mut table = DesktopTable::default();
        let mut directories: Vec<String> = DESKTOP_DIRECTORIES
            .iter()
            .map(|directory| (*directory).to_owned())
            .collect();
        if let Ok(home) = std::env::var("HOME") {
            directories.push(format!("{home}/.local/share/applications"));
            directories.push(format!("{home}/.local/share/flatpak/exports/share/applications"));
        }
        let mut read = 0;
        for directory in directories {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                if read >= MAX_DESKTOP_FILES {
                    return table;
                }
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("desktop") {
                    continue;
                }
                read += 1;
                let Some(identifier) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let (program, class) = parse_desktop(&text);
                table.entries.push((identifier, program, class));
            }
        }
        table
    }

    /// The desktop files that start one of `programs`, or whose window
    /// class is one of them.
    pub fn matching(&self, programs: &BTreeSet<String>) -> Vec<String> {
        let mut found: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, program, class)| {
                (!program.is_empty() && programs.contains(program))
                    || (!class.is_empty() && programs.contains(class))
            })
            .map(|(identifier, _, _)| identifier.clone())
            .collect();
        found.sort();
        found.dedup();
        found
    }
}

/// The program the `Exec` line of a desktop file's main group starts (its
/// file name), and its `StartupWMClass`.
pub fn parse_desktop(text: &str) -> (String, String) {
    let mut in_main = false;
    let mut program = String::new();
    let mut class = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some(exec) = line.strip_prefix("Exec=")
            && program.is_empty()
        {
            program = exec_program(exec);
        } else if let Some(value) = line.strip_prefix("StartupWMClass=")
            && class.is_empty()
        {
            class = value.trim().to_owned();
        }
    }
    (program, class)
}

/// The file name of the program an `Exec` value starts, past `env` and its
/// assignments, and past `flatpak run` to the application identifier.
pub fn exec_program(exec: &str) -> String {
    let words: Vec<&str> = exec
        .split_whitespace()
        .map(|word| word.trim_matches(|c| c == '"' || c == '\''))
        .collect();
    let mut index = 0;
    if words.first().map(|word| base_name(word)) == Some("env") {
        index = 1;
        while words.get(index).is_some_and(|word| word.contains('=') || word.starts_with('-')) {
            index += 1;
        }
    }
    let program = words.get(index).map(|word| base_name(word)).unwrap_or_default();
    if program == "flatpak" && words.get(index + 1) == Some(&"run") {
        // The application identifier is the first word that is not an
        // option.
        return words[index + 2..]
            .iter()
            .find(|word| !word.starts_with('-') && !word.starts_with('@'))
            .map(|word| (*word).to_owned())
            .unwrap_or_default();
    }
    program.to_owned()
}

/// The last component of a path.
pub fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What the kernel says about a process: its program in three forms and,
/// in a Flatpak sandbox, the application's identifier.
pub fn process_names(pid: u32) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let base = format!("/proc/{pid}");
    if let Ok(comm) = fs::read_to_string(format!("{base}/comm")) {
        names.insert(comm.trim().to_owned());
    }
    if let Ok(exe) = fs::read_link(format!("{base}/exe"))
        && let Some(name) = exe.file_name().and_then(|name| name.to_str())
    {
        // A program replaced while running is shown with " (deleted)".
        names.insert(name.trim_end_matches(" (deleted)").to_owned());
    }
    if let Ok(cmdline) = fs::read(format!("{base}/cmdline"))
        && let Some(first) = cmdline.split(|byte| *byte == 0).next()
        && let Ok(first) = std::str::from_utf8(first)
    {
        names.insert(base_name(first).to_owned());
    }
    if let Ok(info) = fs::read_to_string(Path::new(&base).join("root/.flatpak-info")) {
        names.extend(flatpak_application(&info));
    }
    names.retain(|name| !name.is_empty());
    names
}

/// The `name` of the `[Application]` group of a `.flatpak-info` file.
pub fn flatpak_application(text: &str) -> Option<String> {
    let mut in_application = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_application = line == "[Application]";
        } else if in_application && let Some(name) = line.strip_prefix("name=") {
            return Some(name.trim().to_owned());
        }
    }
    None
}

/// The identifiers of an application, the main one first.
///
/// The main one is, in this order: the Flatpak identifier or a desktop
/// file that starts the program, the application's name on the bus if it
/// is a plain identifier, the program's file name.
pub fn identities(
    bus_name_of_application: &str,
    process: &BTreeSet<String>,
    desktop: &[String],
) -> (String, Vec<String>) {
    let mut all: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !all.iter().any(|known| known == name) {
            all.push(name.to_owned());
        }
    };
    // Flatpak identifiers and desktop files read like reverse domain names.
    for name in desktop {
        push(name);
    }
    let plain = |name: &str| {
        !name.is_empty()
            && name.len() <= 255
            && name.chars().all(|c| matches!(c, ' '..='~'))
            && name.trim() == name
    };
    if plain(bus_name_of_application) {
        push(bus_name_of_application);
    }
    for name in process {
        push(name);
    }
    if !plain(bus_name_of_application) {
        push(bus_name_of_application);
    }
    match all.split_first() {
        Some((main, rest)) => (main.clone(), rest.to_vec()),
        None => ("unknown".to_owned(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_is_the_user_only_when_the_bus_says_the_same_user() {
        assert_eq!(owner_word(Some(1000), Some(1000)), "user");
        assert_eq!(owner_word(Some(0), Some(1000)), "administrator");
        assert_eq!(owner_word(Some(0), Some(0)), "administrator");
        assert_eq!(owner_word(Some(1001), Some(1000)), "unknown");
        assert_eq!(owner_word(None, Some(1000)), "unknown");
        assert_eq!(owner_word(Some(1000), None), "unknown");
    }

    #[test]
    fn the_program_of_a_desktop_file_is_found() {
        let text = "[Desktop Entry]\nName=Konsole\nExec=konsole\nStartupWMClass=konsole\n\n[Desktop Action NewWindow]\nExec=konsole --new-window\n";
        assert_eq!(
            parse_desktop(text),
            ("konsole".to_owned(), "konsole".to_owned())
        );
        assert_eq!(exec_program("/usr/bin/firefox %u"), "firefox");
        assert_eq!(exec_program("env GDK_BACKEND=x11 FOO=1 gedit %U"), "gedit");
        assert_eq!(
            exec_program("/usr/bin/flatpak run --branch=stable --command=keepassxc org.keepassxc.KeePassXC"),
            "org.keepassxc.KeePassXC"
        );
        assert_eq!(exec_program(""), "");
        // Only the main group counts.
        assert_eq!(
            parse_desktop("[Desktop Action X]\nExec=other\n"),
            (String::new(), String::new())
        );
    }

    #[test]
    fn a_desktop_file_matches_by_program_or_window_class() {
        let table = DesktopTable {
            entries: vec![
                ("org.kde.konsole".to_owned(), "konsole".to_owned(), String::new()),
                ("firefox".to_owned(), "firefox".to_owned(), "firefox".to_owned()),
                ("org.example.Wm".to_owned(), "launcher".to_owned(), "wmclass".to_owned()),
            ],
        };
        let programs: BTreeSet<String> = ["konsole", "wmclass"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            table.matching(&programs),
            ["org.example.Wm", "org.kde.konsole"]
        );
    }

    #[test]
    fn the_main_identity_is_the_desktop_file_and_nothing_is_lost() {
        let process: BTreeSet<String> = ["konsole"].iter().map(|s| (*s).to_owned()).collect();
        let (main, others) = identities("konsole", &process, &["org.kde.konsole".to_owned()]);
        assert_eq!(main, "org.kde.konsole");
        assert_eq!(others, ["konsole"]);

        // No desktop file: the name on the bus, then the program.
        let process: BTreeSet<String> =
            ["python3", "python3.14"].iter().map(|s| (*s).to_owned()).collect();
        let (main, others) = identities("tur-a11y-mail", &process, &[]);
        assert_eq!(main, "tur-a11y-mail");
        assert_eq!(others, ["python3", "python3.14"]);

        // A name on the bus that is not plain comes last, and is kept.
        let (main, others) = identities("Mail\u{200b}", &process, &[]);
        assert_eq!(main, "python3");
        assert_eq!(others, ["python3.14", "Mail\u{200b}"]);

        let (main, others) = identities("", &BTreeSet::new(), &[]);
        assert_eq!(main, "unknown");
        assert!(others.is_empty());
    }

    #[test]
    fn a_flatpak_identifier_is_read() {
        let info = "[Application]\nname=org.keepassxc.KeePassXC\nruntime=x\n\n[Instance]\nname=other\n";
        assert_eq!(
            flatpak_application(info).as_deref(),
            Some("org.keepassxc.KeePassXC")
        );
        assert_eq!(flatpak_application("[Instance]\nname=x\n"), None);
    }
}

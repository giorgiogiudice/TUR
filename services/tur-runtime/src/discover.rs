// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Finding out what an installer installed.
//!
//! An installer is not asked what it did. The prefix is looked at before and
//! after it runs, and three things are compared:
//!
//! * the programs (`.exe` files) on the prefix's `C:` drive;
//! * the shortcuts (`.lnk` files) there, which is how an installer puts an
//!   application into the Start menu or onto the desktop;
//! * the uninstall entries in the prefix's registry.
//!
//! [`choose_main`] then picks the program a launcher entry should start.
//! The rules are written down in the function; when they do not single out
//! one program the answer is "unclear", with the candidates, never a guess.
//!
//! Everything read here was written by the installer and is untrusted.
//! Symbolic links are neither followed nor listed, so nothing outside the
//! prefix is read; the walk is bounded in depth and in entries; names with
//! control characters are skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::shortcut::{is_drive_path, parse_shell_link};
use crate::winreg::{UninstallEntry, read_uninstall_entries};

/// Directory entries examined in one walk of a prefix.
const MAX_WALK_ENTRIES: usize = 200_000;
/// How deep below `drive_c` the walk goes.
const MAX_WALK_DEPTH: usize = 24;
/// Bytes read from one shortcut file.
const MAX_SHORTCUT_BYTES: u64 = 64 * 1024;
/// Shortcuts read after one installation.
const MAX_SHORTCUTS: usize = 256;

/// What a prefix held before an installer ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    files: BTreeSet<String>,
    uninstall: BTreeSet<String>,
}

/// Where a shortcut is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShortcutPlace {
    /// In a Start menu.
    StartMenu,
    /// On a desktop.
    Desktop,
    /// Anywhere else.
    Elsewhere,
}

/// A shortcut an installer created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundShortcut {
    /// The shortcut file, relative to the prefix's `C:` drive.
    pub file: String,
    /// The shortcut's name: its file name without `.lnk`.
    pub name: String,
    /// Where it is.
    pub place: ShortcutPlace,
    /// What it points to, as a Windows path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The arguments it passes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    /// The newly installed program it points to, relative to the `C:`
    /// drive, when it points to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
}

/// What an installer added to a prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Discovery {
    /// The new programs, relative to the prefix's `C:` drive, with `/`
    /// between components, sorted.
    pub executables: Vec<String>,
    /// The new shortcuts.
    pub shortcuts: Vec<FoundShortcut>,
    /// The new uninstall entries.
    pub uninstall_entries: Vec<UninstallEntry>,
    /// Whether the walk stopped at a limit, so that the lists may be
    /// incomplete.
    pub truncated: bool,
}

#[derive(Default)]
struct Walked {
    executables: Vec<String>,
    shortcuts: Vec<String>,
    truncated: bool,
}

/// Lists the programs and shortcuts under `drive_c`.
fn walk(drive_c: &Path) -> Walked {
    let mut walked = Walked::default();
    // The drive itself must be a real directory: a link in its place would
    // lead the walk out of the prefix.
    if !fs::symlink_metadata(drive_c).is_ok_and(|metadata| metadata.is_dir()) {
        return walked;
    }
    let mut pending: Vec<(PathBuf, String, usize)> =
        vec![(drive_c.to_path_buf(), String::new(), 0)];
    let mut seen: usize = 0;
    'walk: while let Some((directory, relative, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_WALK_ENTRIES {
                walked.truncated = true;
                break 'walk;
            }
            // The type of the entry itself: a link is reported as a link.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.chars().any(char::is_control) {
                continue;
            }
            let path = if relative.is_empty() {
                name.to_owned()
            } else {
                format!("{relative}/{name}")
            };
            if kind.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    pending.push((entry.path(), path, depth + 1));
                } else {
                    walked.truncated = true;
                }
            } else if kind.is_file() {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".exe") {
                    walked.executables.push(path);
                } else if lower.ends_with(".lnk") {
                    walked.shortcuts.push(path);
                }
            }
        }
    }
    walked.executables.sort();
    walked.shortcuts.sort();
    walked
}

/// Records what the prefix at `prefix` holds, to compare against later.
pub fn snapshot(prefix: &Path) -> Snapshot {
    let walked = walk(&prefix.join("drive_c"));
    Snapshot {
        files: walked
            .executables
            .into_iter()
            .chain(walked.shortcuts)
            .collect(),
        uninstall: read_uninstall_entries(prefix)
            .iter()
            .map(UninstallEntry::identity)
            .collect(),
    }
}

/// The Windows path of a file given relative to the `C:` drive.
pub fn windows_path(relative: &str) -> String {
    format!("C:\\{}", relative.replace('/', "\\"))
}

/// A Windows path in the form paths are compared in: backslashes, lower
/// case. Windows file names are not case-sensitive.
fn fold(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

fn place_of(relative: &str) -> ShortcutPlace {
    let lower = relative.to_ascii_lowercase();
    if lower.split('/').any(|part| part == "start menu") {
        ShortcutPlace::StartMenu
    } else if lower.split('/').any(|part| part == "desktop") {
        ShortcutPlace::Desktop
    } else {
        ShortcutPlace::Elsewhere
    }
}

fn read_shortcut(
    drive_c: &Path,
    file: String,
    programs: &BTreeMap<String, String>,
) -> FoundShortcut {
    let name = file
        .rsplit('/')
        .next()
        .and_then(|name| name.get(..name.len().saturating_sub(4)))
        .unwrap_or("")
        .to_owned();
    let mut bytes = Vec::new();
    let link = File::open(drive_c.join(&file))
        .and_then(|handle| handle.take(MAX_SHORTCUT_BYTES).read_to_end(&mut bytes))
        .ok()
        .and_then(|_| parse_shell_link(&bytes));
    let target = link.as_ref().and_then(|link| link.target.clone());
    let executable = target
        .as_deref()
        .and_then(|target| programs.get(&fold(target)))
        .cloned();
    FoundShortcut {
        place: place_of(&file),
        file,
        name,
        target,
        arguments: link.and_then(|link| link.arguments),
        executable,
    }
}

/// Compares the prefix at `prefix` with what it held `before`.
pub fn discover(prefix: &Path, before: &Snapshot) -> Discovery {
    let drive_c = prefix.join("drive_c");
    let walked = walk(&drive_c);
    let executables: Vec<String> = walked
        .executables
        .into_iter()
        .filter(|file| !before.files.contains(file))
        .collect();
    let programs: BTreeMap<String, String> = executables
        .iter()
        .map(|relative| (fold(&windows_path(relative)), relative.clone()))
        .collect();
    let shortcuts = walked
        .shortcuts
        .into_iter()
        .filter(|file| !before.files.contains(file))
        .take(MAX_SHORTCUTS)
        .map(|file| read_shortcut(&drive_c, file, &programs))
        .collect();
    let uninstall_entries = read_uninstall_entries(prefix)
        .into_iter()
        .filter(|entry| !before.uninstall.contains(&entry.identity()))
        .collect();
    Discovery {
        executables,
        shortcuts,
        uninstall_entries,
        truncated: walked.truncated,
    }
}

/// The program a launcher entry starts, and why it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainProgram {
    /// The program, relative to the prefix's `C:` drive.
    pub executable: String,
    /// The rule that chose it, in plain language.
    pub reason: String,
}

/// Why no main program was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceError {
    /// No new program was found at all.
    NothingInstalled,
    /// Several programs were found and no rule singles one out.
    Unclear {
        /// The programs it could be, as Windows paths.
        candidates: Vec<String>,
    },
    /// The program the caller named is not one of the new programs, or
    /// names more than one.
    NotFound {
        /// What the caller named.
        requested: String,
        /// The programs it could have been, as Windows paths.
        candidates: Vec<String>,
    },
}

/// Whether a new program can be an application's main program. Programs in
/// the Windows directory and in temporary directories, and uninstallers,
/// cannot.
fn is_candidate(relative: &str) -> bool {
    let lower = relative.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or("");
    !(lower.starts_with("windows/")
        || lower.split('/').any(|part| part == "temp" || part == "tmp")
        || name.starts_with("unins")
        || name.contains("uninstall")
        || name == "au_.exe")
}

/// The program a `DisplayIcon` value names: the value without quotes and
/// without the icon index that may follow a comma.
fn icon_program(value: &str) -> String {
    let value = value.trim().trim_matches('"');
    let path = match value.rsplit_once(',') {
        Some((path, index))
            if !index.is_empty()
                && index
                    .trim()
                    .trim_start_matches('-')
                    .chars()
                    .all(|character| character.is_ascii_digit()) =>
        {
            path
        }
        _ => value,
    };
    fold(path.trim().trim_matches('"'))
}

/// Whether `display_name` starts with `name` as a whole word, ignoring
/// case: "PuTTY release 0.85" starts with "PuTTY" and not with "PuTTYgen".
fn starts_with_word(display_name: &str, name: &str) -> bool {
    let display_name = display_name.to_lowercase();
    let name = name.to_lowercase();
    !name.is_empty()
        && display_name.strip_prefix(&name).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|next| !next.is_alphanumeric())
        })
}

/// The one element of `found`, when there is exactly one after duplicates
/// are removed.
fn only(mut found: Vec<&String>) -> Option<&String> {
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// Chooses the program a launcher entry should start.
///
/// The rules, in order; the first that singles out one program decides:
///
/// 1. The program the caller named (`requested`), by file name or by a path
///    ending. It must name exactly one new program.
/// 2. The one program that Start-menu and desktop shortcuts point to.
/// 3. Among several such programs: the one that is also the icon of an
///    uninstall entry; failing that, the one whose shortcut is named after
///    the application, as the uninstall entry names it.
/// 4. With no shortcut to a program: the icon of an uninstall entry.
/// 5. The only new program.
/// 6. The only new program nearest the top of the drive.
///
/// # Errors
///
/// [`ChoiceError`] when nothing was installed, when the named program is
/// not there, or when no rule singles out one program.
pub fn choose_main(
    discovery: &Discovery,
    requested: Option<&str>,
) -> Result<MainProgram, ChoiceError> {
    let candidates: Vec<&String> = discovery
        .executables
        .iter()
        .filter(|relative| is_candidate(relative))
        .collect();
    let listed = |programs: &[&String]| -> Vec<String> {
        programs
            .iter()
            .map(|relative| windows_path(relative))
            .collect()
    };
    let chosen = |executable: &String, reason: String| MainProgram {
        executable: executable.clone(),
        reason,
    };

    if let Some(requested) = requested {
        let mut wanted = fold(requested.trim());
        if is_drive_path(&wanted) {
            wanted = wanted.get(3..).unwrap_or("").to_owned();
        }
        let wanted = wanted.trim_matches('\\').replace('\\', "/");
        let named: Vec<&String> = candidates
            .iter()
            .copied()
            .filter(|relative| {
                let lower = relative.to_lowercase();
                !wanted.is_empty() && (lower == wanted || lower.ends_with(&format!("/{wanted}")))
            })
            .collect();
        return match only(named) {
            Some(executable) => Ok(chosen(executable, "it was named by the caller".to_owned())),
            None => Err(ChoiceError::NotFound {
                requested: requested.to_owned(),
                candidates: listed(&candidates),
            }),
        };
    }
    if candidates.is_empty() {
        return Err(ChoiceError::NothingInstalled);
    }

    // The programs the installer made shortcuts to, where a user would
    // look for the application.
    let linked: Vec<(&FoundShortcut, &String)> = discovery
        .shortcuts
        .iter()
        .filter(|shortcut| shortcut.place != ShortcutPlace::Elsewhere)
        .filter_map(|shortcut| {
            let executable = shortcut.executable.as_ref()?;
            candidates
                .iter()
                .copied()
                .find(|candidate| *candidate == executable)
                .map(|candidate| (shortcut, candidate))
        })
        .collect();
    let icons: Vec<String> = discovery
        .uninstall_entries
        .iter()
        .filter_map(|entry| entry.display_icon.as_deref())
        .map(icon_program)
        .collect();
    let is_icon = |relative: &String| icons.contains(&fold(&windows_path(relative)));

    let linked_programs: Vec<&String> = linked.iter().map(|(_, executable)| *executable).collect();
    if let Some(executable) = only(linked_programs.clone()) {
        // Several shortcuts can point to one program; the one without
        // arguments and with the shortest name is the application's own.
        let name = linked
            .iter()
            .map(|(shortcut, _)| *shortcut)
            .min_by_key(|shortcut| (shortcut.arguments.is_some(), shortcut.name.chars().count()))
            .map_or("", |shortcut| shortcut.name.as_str());
        return Ok(chosen(
            executable,
            format!("it is the only program a shortcut points to (\"{name}\")"),
        ));
    }
    if !linked.is_empty() {
        let with_icon: Vec<&String> = linked_programs
            .iter()
            .copied()
            .filter(|executable| is_icon(executable))
            .collect();
        if let Some(executable) = only(with_icon) {
            return Ok(chosen(
                executable,
                "it has a shortcut and is the icon of the application's uninstall entry".to_owned(),
            ));
        }
        let names: Vec<&str> = discovery
            .uninstall_entries
            .iter()
            .filter_map(|entry| entry.display_name.as_deref())
            .collect();
        let named_after: Vec<&String> = linked
            .iter()
            .filter(|(shortcut, _)| {
                names
                    .iter()
                    .any(|display_name| starts_with_word(display_name, &shortcut.name))
            })
            .map(|(_, executable)| *executable)
            .collect();
        if let Some(executable) = only(named_after) {
            return Ok(chosen(
                executable,
                "its shortcut is named after the application".to_owned(),
            ));
        }
        let mut unclear = linked_programs;
        unclear.sort();
        unclear.dedup();
        return Err(ChoiceError::Unclear {
            candidates: listed(&unclear),
        });
    }

    let with_icon: Vec<&String> = candidates
        .iter()
        .copied()
        .filter(|executable| is_icon(executable))
        .collect();
    if let Some(executable) = only(with_icon) {
        return Ok(chosen(
            executable,
            "it is the icon of the application's uninstall entry".to_owned(),
        ));
    }
    if let [executable] = candidates.as_slice() {
        return Ok(chosen(
            executable,
            "it is the only program that was installed".to_owned(),
        ));
    }
    let depth = |relative: &String| relative.matches('/').count();
    let top = candidates.iter().copied().map(depth).min().unwrap_or(0);
    let at_top: Vec<&String> = candidates
        .iter()
        .copied()
        .filter(|relative| depth(relative) == top)
        .collect();
    if let Some(executable) = only(at_top) {
        return Ok(chosen(
            executable,
            "it is the only program at the top of what was installed".to_owned(),
        ));
    }
    Err(ChoiceError::Unclear {
        candidates: listed(&candidates),
    })
}

/// The uninstall entry that describes the application whose main program is
/// `main` (relative to the `C:` drive): the one installed where the program
/// is, or the only one that has a name. System components are not
/// considered.
pub fn primary_entry<'a>(discovery: &'a Discovery, main: &str) -> Option<&'a UninstallEntry> {
    let visible: Vec<&UninstallEntry> = discovery
        .uninstall_entries
        .iter()
        .filter(|entry| !entry.system_component)
        .collect();
    let program = fold(&windows_path(main));
    let located = visible.iter().copied().find(|entry| {
        entry.install_location.as_deref().is_some_and(|location| {
            let location = fold(location.trim().trim_matches('"'));
            let location = location.trim_end_matches('\\');
            !location.is_empty() && program.starts_with(&format!("{location}\\"))
        })
    });
    let named = || {
        visible
            .iter()
            .copied()
            .find(|entry| entry.display_name.is_some())
    };
    located.or_else(named).or_else(|| visible.first().copied())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::shortcut::tests::LinkSpec;
    use crate::testdata::TempDir;
    use crate::winreg::Hive;

    const START_MENU: &str = "ProgramData/Microsoft/Windows/Start Menu/Programs";

    /// Puts a file into a scratch prefix, relative to its `C:` drive.
    pub(crate) fn put(prefix: &Path, relative: &str, bytes: &[u8]) {
        let path = prefix.join("drive_c").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Puts a shortcut to `target` into a scratch prefix.
    pub(crate) fn link(prefix: &Path, relative: &str, target: &str) {
        put(prefix, relative, &LinkSpec::to(target).build());
    }

    /// A scratch prefix as Wine leaves one: its own programs, no uninstall
    /// entries.
    pub(crate) fn fresh_prefix(label: &str) -> TempDir {
        let temp = TempDir::new(label);
        for program in [
            "windows/system32/notepad.exe",
            "windows/explorer.exe",
            "Program Files/Internet Explorer/iexplore.exe",
        ] {
            put(temp.path(), program, b"MZ");
        }
        fs::write(
            temp.path().join("system.reg"),
            "WINE REGISTRY Version 2\n\n#arch=win64\n",
        )
        .unwrap();
        temp
    }

    fn entry(
        key: &str,
        name: Option<&str>,
        icon: Option<&str>,
        location: Option<&str>,
    ) -> UninstallEntry {
        UninstallEntry {
            hive: Some(Hive::Machine),
            key: key.to_owned(),
            display_name: name.map(str::to_owned),
            display_icon: icon.map(str::to_owned),
            install_location: location.map(str::to_owned),
            ..UninstallEntry::default()
        }
    }

    fn shortcut(name: &str, place: ShortcutPlace, executable: Option<&str>) -> FoundShortcut {
        FoundShortcut {
            file: format!("{name}.lnk"),
            name: name.to_owned(),
            place,
            target: executable.map(windows_path),
            arguments: None,
            executable: executable.map(str::to_owned),
        }
    }

    fn found(
        executables: &[&str],
        shortcuts: Vec<FoundShortcut>,
        entries: Vec<UninstallEntry>,
    ) -> Discovery {
        Discovery {
            executables: executables.iter().map(|path| (*path).to_owned()).collect(),
            shortcuts,
            uninstall_entries: entries,
            truncated: false,
        }
    }

    #[test]
    fn only_what_the_installer_added_is_reported() {
        let temp = fresh_prefix("discover-diff");
        let prefix = temp.path();
        link(
            prefix,
            &format!("{START_MENU}/Accessories/Notepad.lnk"),
            "C:\\windows\\system32\\notepad.exe",
        );
        let before = snapshot(prefix);
        assert_eq!(discover(prefix, &before), Discovery::default());

        // What an installer leaves behind.
        put(prefix, "Program Files/7-Zip/7zFM.exe", b"MZ");
        put(prefix, "Program Files/7-Zip/7z.exe", b"MZ");
        put(prefix, "Program Files/7-Zip/Uninstall.exe", b"MZ");
        put(prefix, "Program Files/7-Zip/7z.dll", b"MZ");
        put(prefix, "windows/system32/helper.EXE", b"MZ");
        link(
            prefix,
            &format!("{START_MENU}/7-Zip/7-Zip File Manager.lnk"),
            "C:\\PROGRAM FILES\\7-zip\\7ZFM.EXE",
        );
        link(
            prefix,
            &format!("{START_MENU}/7-Zip/7-Zip Help.lnk"),
            "C:\\Program Files\\7-Zip\\7-zip.chm",
        );
        put(prefix, "users/Public/Desktop/broken.lnk", b"not a shortcut");
        fs::write(prefix.join("system.reg"), crate::winreg::tests::SYSTEM_REG).unwrap();

        let discovery = discover(prefix, &before);
        assert_eq!(
            discovery.executables,
            [
                "Program Files/7-Zip/7z.exe",
                "Program Files/7-Zip/7zFM.exe",
                "Program Files/7-Zip/Uninstall.exe",
                "windows/system32/helper.EXE",
            ]
        );
        assert!(!discovery.truncated);
        let shortcuts: Vec<(&str, ShortcutPlace, Option<&str>)> = discovery
            .shortcuts
            .iter()
            .map(|shortcut| {
                (
                    shortcut.name.as_str(),
                    shortcut.place,
                    shortcut.executable.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shortcuts,
            [
                (
                    "7-Zip File Manager",
                    ShortcutPlace::StartMenu,
                    // Found whatever the case of the path in the shortcut.
                    Some("Program Files/7-Zip/7zFM.exe")
                ),
                ("7-Zip Help", ShortcutPlace::StartMenu, None),
                ("broken", ShortcutPlace::Desktop, None),
            ]
        );
        assert_eq!(
            discovery.shortcuts[1].target.as_deref(),
            Some("C:\\Program Files\\7-Zip\\7-zip.chm")
        );
        assert_eq!(discovery.uninstall_entries.len(), 3);

        let main = choose_main(&discovery, None).unwrap();
        assert_eq!(main.executable, "Program Files/7-Zip/7zFM.exe");
        assert!(
            main.reason.contains("7-Zip File Manager"),
            "{}",
            main.reason
        );
        let primary = primary_entry(&discovery, &main.executable).unwrap();
        assert_eq!(primary.key, "7-Zip");
    }

    #[test]
    fn links_are_neither_followed_nor_listed() {
        let temp = fresh_prefix("discover-links");
        let prefix = temp.path();
        let outside = TempDir::new("discover-outside");
        fs::write(outside.path().join("secret.exe"), b"MZ").unwrap();
        let before = snapshot(prefix);
        // A directory link and a file link, both leading out of the prefix.
        std::os::unix::fs::symlink(outside.path(), prefix.join("drive_c/escape")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.exe"),
            prefix.join("drive_c/linked.exe"),
        )
        .unwrap();
        put(prefix, "app/app.exe", b"MZ");
        let discovery = discover(prefix, &before);
        assert_eq!(discovery.executables, ["app/app.exe"]);

        // A drive that is itself a link is not walked at all.
        let hostile = TempDir::new("discover-hostile");
        std::os::unix::fs::symlink(outside.path(), hostile.path().join("drive_c")).unwrap();
        assert_eq!(
            discover(hostile.path(), &Snapshot::default()),
            Discovery::default()
        );
        // Nor is a prefix that does not exist.
        assert_eq!(
            discover(&hostile.path().join("absent"), &Snapshot::default()),
            Discovery::default()
        );
    }

    #[test]
    fn the_one_program_with_a_shortcut_is_the_main_program() {
        let discovery = found(
            &[
                "Program Files/Notepad++/notepad++.exe",
                "Program Files/Notepad++/updater/GUP.exe",
                "Program Files/Notepad++/uninstall.exe",
            ],
            vec![
                // A second shortcut to the same program, with arguments.
                FoundShortcut {
                    arguments: Some("-safe".to_owned()),
                    ..shortcut(
                        "A safe mode",
                        ShortcutPlace::StartMenu,
                        Some("Program Files/Notepad++/notepad++.exe"),
                    )
                },
                shortcut(
                    "Notepad++ editor",
                    ShortcutPlace::StartMenu,
                    Some("Program Files/Notepad++/notepad++.exe"),
                ),
                shortcut(
                    "Notepad++",
                    ShortcutPlace::Desktop,
                    Some("Program Files/Notepad++/notepad++.exe"),
                ),
                // A shortcut to the uninstaller does not make it a candidate.
                shortcut(
                    "Uninstall",
                    ShortcutPlace::StartMenu,
                    Some("Program Files/Notepad++/uninstall.exe"),
                ),
                // A shortcut somewhere else is not where a user looks.
                shortcut(
                    "Updater",
                    ShortcutPlace::Elsewhere,
                    Some("Program Files/Notepad++/updater/GUP.exe"),
                ),
            ],
            Vec::new(),
        );
        let main = choose_main(&discovery, None).unwrap();
        assert_eq!(main.executable, "Program Files/Notepad++/notepad++.exe");
        // The reason names the application's own shortcut: the one without
        // arguments and with the shortest name.
        assert_eq!(
            main.reason,
            "it is the only program a shortcut points to (\"Notepad++\")"
        );
    }

    #[test]
    fn several_shortcuts_are_settled_by_the_uninstall_entry() {
        let programs = [
            "Program Files/PuTTY/pageant.exe",
            "Program Files/PuTTY/psftp.exe",
            "Program Files/PuTTY/putty.exe",
            "Program Files/PuTTY/puttygen.exe",
        ];
        let shortcuts = || {
            vec![
                shortcut("Pageant", ShortcutPlace::StartMenu, Some(programs[0])),
                shortcut("PSFTP", ShortcutPlace::StartMenu, Some(programs[1])),
                shortcut("PuTTY", ShortcutPlace::StartMenu, Some(programs[2])),
                shortcut("PuTTYgen", ShortcutPlace::StartMenu, Some(programs[3])),
                shortcut("PuTTY Manual", ShortcutPlace::StartMenu, None),
            ]
        };
        // By the name of the application.
        let by_name = found(
            &programs,
            shortcuts(),
            vec![entry(
                "{CODE}",
                Some("PuTTY release 0.85 (64-bit)"),
                None,
                None,
            )],
        );
        let main = choose_main(&by_name, None).unwrap();
        assert_eq!(main.executable, programs[2]);
        assert!(main.reason.contains("named after"));
        // By the icon of the uninstall entry, which wins over the name.
        let by_icon = found(
            &programs,
            shortcuts(),
            vec![entry(
                "App",
                Some("Pageant and friends"),
                Some("\"C:\\Program Files\\PuTTY\\PSFTP.EXE\",0"),
                None,
            )],
        );
        assert_eq!(choose_main(&by_icon, None).unwrap().executable, programs[1]);
        // With neither, the answer is "unclear", with the candidates.
        let unclear = found(&programs, shortcuts(), Vec::new());
        assert_eq!(
            choose_main(&unclear, None).unwrap_err(),
            ChoiceError::Unclear {
                candidates: programs.iter().map(|path| windows_path(path)).collect()
            }
        );
    }

    #[test]
    fn without_shortcuts_the_icon_then_the_only_or_topmost_program_is_taken() {
        let by_icon = found(
            &["Program Files/App/app.exe", "Program Files/App/tool.exe"],
            Vec::new(),
            vec![entry(
                "App",
                Some("App"),
                Some("C:\\Program Files\\App\\tool.exe,-101"),
                None,
            )],
        );
        assert_eq!(
            choose_main(&by_icon, None).unwrap().executable,
            "Program Files/App/tool.exe"
        );
        let single = found(
            &[
                "Program Files/App/app.exe",
                "Program Files/App/unins000.exe",
            ],
            Vec::new(),
            Vec::new(),
        );
        let main = choose_main(&single, None).unwrap();
        assert_eq!(main.executable, "Program Files/App/app.exe");
        assert!(main.reason.contains("only program"));
        // An unpacked archive: the program at the top, not its helper.
        let unpacked = found(
            &[
                "Program Files/npp/notepad++.exe",
                "Program Files/npp/updater/GUP.exe",
            ],
            Vec::new(),
            Vec::new(),
        );
        let main = choose_main(&unpacked, None).unwrap();
        assert_eq!(main.executable, "Program Files/npp/notepad++.exe");
        assert!(main.reason.contains("top"));
        let unclear = found(
            &["Program Files/App/one.exe", "Program Files/App/two.exe"],
            Vec::new(),
            Vec::new(),
        );
        assert!(matches!(
            choose_main(&unclear, None),
            Err(ChoiceError::Unclear { candidates }) if candidates.len() == 2
        ));
    }

    #[test]
    fn nothing_installed_is_said_plainly() {
        assert_eq!(
            choose_main(&Discovery::default(), None).unwrap_err(),
            ChoiceError::NothingInstalled
        );
        // Programs in the Windows directory, temporary files and
        // uninstallers are not applications.
        let nothing = found(
            &[
                "windows/system32/helper.exe",
                "users/ada/Temp/setup.exe",
                "users/ada/AppData/Local/Temp/~nsu.tmp/Au_.exe",
                "Program Files/App/unins000.exe",
                "Program Files/App/Uninstall App.exe",
            ],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            choose_main(&nothing, None).unwrap_err(),
            ChoiceError::NothingInstalled
        );
    }

    #[test]
    fn a_named_program_wins_and_must_name_exactly_one() {
        let discovery = found(
            &[
                "Program Files/PuTTY/putty.exe",
                "Program Files/PuTTY/puttygen.exe",
                "Program Files/Other/putty.exe",
                "Program Files/PuTTY/unins000.exe",
            ],
            vec![shortcut(
                "PuTTYgen",
                ShortcutPlace::StartMenu,
                Some("Program Files/PuTTY/puttygen.exe"),
            )],
            Vec::new(),
        );
        for request in [
            "PuTTY/putty.exe",
            "putty\\PUTTY.EXE",
            "C:\\Program Files\\PuTTY\\putty.exe",
            "Program Files/PuTTY/putty.exe",
        ] {
            let main = choose_main(&discovery, Some(request))
                .unwrap_or_else(|error| panic!("{request}: {error:?}"));
            assert_eq!(
                main.executable, "Program Files/PuTTY/putty.exe",
                "{request}"
            );
            assert!(main.reason.contains("named by the caller"));
        }
        // Two programs have this name; an uninstaller cannot be named; a
        // program that is not there cannot be named; nor can part of a name.
        for request in [
            "putty.exe",
            "unins000.exe",
            "absent.exe",
            "tty.exe",
            "",
            "\\",
        ] {
            let error = choose_main(&discovery, Some(request)).unwrap_err();
            assert!(
                matches!(&error, ChoiceError::NotFound { requested, candidates }
                    if requested == request && candidates.len() == 3),
                "{request}: {error:?}"
            );
        }
    }

    #[test]
    fn the_uninstall_entry_of_the_application_is_found() {
        let entries = vec![
            entry(
                "Helper",
                Some("Helper runtime"),
                None,
                Some("C:\\Program Files\\Helper"),
            ),
            entry(
                "App",
                Some("The App"),
                None,
                Some("\"C:\\Program Files\\App\\\""),
            ),
            UninstallEntry {
                system_component: true,
                ..entry("Hidden", Some("Hidden"), None, Some("C:\\Program Files"))
            },
        ];
        let discovery = found(&["Program Files/App/bin/app.exe"], Vec::new(), entries);
        // The one installed where the program is, not the first one.
        assert_eq!(
            primary_entry(&discovery, "Program Files/App/bin/app.exe")
                .unwrap()
                .key,
            "App"
        );
        // A program elsewhere: the first entry that has a name.
        assert_eq!(
            primary_entry(&discovery, "Tools/tool.exe").unwrap().key,
            "Helper"
        );
        // "Program Files\Apple" is not inside "Program Files\App".
        assert_eq!(
            primary_entry(&discovery, "Program Files/Apple/app.exe")
                .unwrap()
                .key,
            "Helper"
        );
        assert_eq!(primary_entry(&Discovery::default(), "x.exe"), None);
        let nameless = found(&[], Vec::new(), vec![entry("Key", None, None, None)]);
        assert_eq!(primary_entry(&nameless, "x.exe").unwrap().key, "Key");
    }

    #[test]
    fn helper_rules() {
        assert_eq!(
            windows_path("Program Files/App/app.exe"),
            "C:\\Program Files\\App\\app.exe"
        );
        assert_eq!(icon_program("\"C:\\App\\App.exe\",0"), "c:\\app\\app.exe");
        assert_eq!(icon_program("C:/App/App.exe,-12"), "c:\\app\\app.exe");
        assert_eq!(
            icon_program("C:\\Apps, Inc\\app.exe"),
            "c:\\apps, inc\\app.exe"
        );
        assert!(starts_with_word("PuTTY release 0.85", "PuTTY"));
        assert!(starts_with_word("putty", "PuTTY"));
        assert!(!starts_with_word("PuTTYgen", "PuTTY"));
        assert!(!starts_with_word("PuTTY", ""));
        assert_eq!(
            place_of("users/ada/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/A.lnk"),
            ShortcutPlace::StartMenu
        );
        assert_eq!(
            place_of("users/Public/Desktop/A.lnk"),
            ShortcutPlace::Desktop
        );
        assert_eq!(
            place_of("Program Files/App/A.lnk"),
            ShortcutPlace::Elsewhere
        );
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Guards the promise that this crate cannot change a disk.
//!
//! Two checks over the crate's own sources under `src/`:
//!
//! 1. No line of code names a partitioning, formatting, encryption, volume
//!    management or bootloader tool. Comments (which is where documentation
//!    lives) are exempt, and so are the fixtures, which are not under `src/`.
//! 2. Exactly one place starts a process, and the program it starts is the
//!    read-only `lsblk`.
//!
//! Together these mean there is no code path, reachable or not, that runs
//! such a tool. When destructive execution is added it must be in another
//! crate, behind the `CommandRunner` trait, and this test stays as it is.

use std::fs;
use std::path::{Path, PathBuf};

/// Programs that partition, format, encrypt, assemble volumes, discard
/// blocks, or install a bootloader or boot entry.
const FORBIDDEN_PROGRAMS: [&str; 44] = [
    "parted",
    "partprobe",
    "partx",
    "sfdisk",
    "sgdisk",
    "cgdisk",
    "fdisk",
    "gdisk",
    "wipefs",
    "blkdiscard",
    "shred",
    "mkfs",
    "mke2fs",
    "mkdosfs",
    "mkntfs",
    "mkswap",
    "mkudffs",
    "cryptsetup",
    "veritysetup",
    "integritysetup",
    "systemd-repart",
    "systemd-cryptenroll",
    "systemd-cryptsetup",
    "pvcreate",
    "vgcreate",
    "lvcreate",
    "pvremove",
    "vgremove",
    "lvremove",
    "mdadm",
    "zpool",
    "losetup",
    "dmsetup",
    "kpartx",
    "bootctl",
    "bootupctl",
    "bootc",
    "ostree",
    "grub-install",
    "grub2-install",
    "grub2-mkconfig",
    "efibootmgr",
    "dracut",
    "kernel-install",
];

/// Short program names that are also ordinary words, so they are matched
/// only as a whole string literal.
const FORBIDDEN_LITERALS: [&str; 5] = ["\"dd\"", "\"mount\"", "\"umount\"", "\"sh\"", "\"bash\""];

/// The one file allowed to start a process, and the only form it may take.
const PROBE_FILE: &str = "system_probe.rs";
const ALLOWED_SPAWN: &str = "Command::new(\"lsblk\")";

/// Ways of starting a process from Rust's standard library.
const SPAWN_MARKERS: [&str; 3] = ["Command::new(", "process::Command", "CommandExt"];

fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} cannot be listed: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("a directory entry reads").path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// The lines of `text` that are code: everything except lines that are
/// wholly a comment. Each is returned with its line number.
fn code_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .collect()
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// True when `line` contains `program` as a whole word, so that `mkfs.ext4`
/// and `"sgdisk"` match but an identifier that merely contains the letters
/// does not.
fn names_program(line: &str, program: &str) -> bool {
    line.match_indices(program).any(|(start, _)| {
        let before = line[..start].chars().next_back();
        let after = line[start + program.len()..].chars().next();
        !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
    })
}

fn sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&root, &mut files);
    files.sort();
    assert!(
        files.len() >= 10,
        "expected the crate's sources under {}",
        root.display()
    );
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} cannot be read: {error}", path.display()));
            (path, text)
        })
        .collect()
}

#[test]
fn no_code_names_a_tool_that_changes_a_disk() {
    let mut violations = Vec::new();
    for (path, text) in sources() {
        for (number, line) in code_lines(&text) {
            let lower = line.to_ascii_lowercase();
            let named = FORBIDDEN_PROGRAMS
                .iter()
                .filter(|program| names_program(&lower, program))
                .chain(
                    FORBIDDEN_LITERALS
                        .iter()
                        .filter(|literal| lower.contains(**literal)),
                );
            for program in named {
                violations.push(format!(
                    "{}:{number}: names {program}: {}",
                    path.display(),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "code in src/ names tools that change a disk:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_only_process_the_crate_starts_is_the_read_only_inventory_probe() {
    let mut spawns = Vec::new();
    for (path, text) in sources() {
        let in_probe = path.file_name().is_some_and(|name| name == PROBE_FILE);
        for (number, line) in code_lines(&text) {
            if !SPAWN_MARKERS.iter().any(|marker| line.contains(marker)) {
                continue;
            }
            assert!(
                in_probe,
                "{}:{number} starts a process outside {PROBE_FILE}: {}",
                path.display(),
                line.trim()
            );
            if line.contains("Command::new(") {
                spawns.push(line.trim().to_owned());
            }
        }
    }
    assert_eq!(
        spawns.len(),
        1,
        "{PROBE_FILE} must start exactly one process: {spawns:?}"
    );
    assert!(
        spawns[0].contains(ALLOWED_SPAWN),
        "the one process must be lsblk, found: {}",
        spawns[0]
    );
}

#[test]
fn the_scanner_sees_through_code_but_not_comments() {
    let sample = "// mkfs is only mentioned here\n    let program = \"mkfs.ext4\";\n/// sgdisk\n";
    let lines = code_lines(sample);
    assert_eq!(lines, [(2, "    let program = \"mkfs.ext4\";")]);
    assert!(names_program(lines[0].1, "mkfs"));
    assert!(names_program("run(\"sgdisk\", &arguments)", "sgdisk"));
    assert!(names_program("bootc install to-disk", "bootc"));
    assert!(!names_program("phaseid::firstbootconfiguration", "bootc"));
    assert!(!names_program("let departed = true;", "parted"));
    assert!(FORBIDDEN_PROGRAMS.contains(&"mkfs"));
}

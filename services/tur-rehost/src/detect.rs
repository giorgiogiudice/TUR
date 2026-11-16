// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Which portable runtime a package carries, at which version, and what
//! platform-specific code travels with it.
//!
//! [`detect`] walks a directory (a package opened without running it, or
//! a directory or `.app` bundle the person named) and returns
//! [`Finding`]s, the most significant first. It ports `detect()` of the
//! prototype (tests/experiments/rehost/rehostlib/detect.py,
//! docs/reports/exp-rehost-1.md, 2.2). Nothing is executed: versions are
//! read from version strings in programs, property lists, JSON files and
//! archive headers.
//!
//! Electron is read in full: version, payload, fuses, native Node modules
//! with their owning npm packages, and helper programs. The other families
//! (NW.js, .NET, .NET Framework, Java, Godot, LÖVE, Ren'Py, PyInstaller,
//! native toolkits) are recognised with their platform, their version
//! where it is cheap to read, and the prototype's verdict and reasons.
//! There is no inflate here, so a member of a zip archive (a jar's
//! manifest, a game's `conf.lua`) is read only when it is stored; where
//! that matters, a note says so.
//!
//! Hostile input: every read is bounded, offsets from files are checked,
//! no size from a file decides an allocation, strings that reach a finding
//! lose their control characters, and symbolic links are never followed
//! (docs/architecture/runtime-manager.md, "Hostile input").

use std::cell::{OnceCell, RefCell};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::asar::{self, Asar};
use crate::finding::{
    BinaryFormat, BinaryKind, ElectronPayload, Evidence, Family, Finding, Location, NativeModule,
    PayloadKind, Platform, Sidecar, Verdict,
};
use crate::{scan, sniff};

/// How many directory levels below the root the walk examines.
const MAX_DEPTH: usize = 9;
/// How many directory entries the walk examines in all.
const MAX_FILES: usize = 200_000;
/// How many files of one directory are examined for being programs.
const MAX_PROGRAM_CHECKS: usize = 4096;
/// The most native modules listed for one payload.
const MAX_NATIVES: usize = 2000;
/// The most sidecars (and .NET native libraries) listed for one finding.
const MAX_SIDECARS: usize = 400;
/// How many entries a walk through a payload directory examines.
const MAX_PAYLOAD_FILES: usize = 500_000;
/// How deep a walk through a payload directory goes.
const PAYLOAD_DEPTH: usize = 64;
/// How many inner archives (`node_modules.asar`) one payload opens.
const MAX_INNER_ARCHIVES: usize = 8;
/// The largest JSON file read.
const JSON_MAX: u64 = 4 << 20;
/// The largest text file read (a jpackage `.cfg`, a jar manifest).
const TEXT_MAX: u64 = 1 << 20;
/// The largest property list read.
const PLIST_MAX: u64 = 1 << 20;
/// How much of a program's end is searched for PyInstaller's cookie.
const PYI_TAIL: u64 = 8 << 20;
/// The most bytes of a .NET bundle manifest read.
const BUNDLE_MANIFEST_MAX: u64 = 16 << 20;
/// The most bytes of a Godot pack directory read.
const PACK_DIRECTORY_MAX: u64 = 16 << 20;
/// The most Godot pack entries listed.
const PACK_FILES_MAX: u32 = 200_000;
/// The longest string from a file kept in a finding, in characters.
const TEXT_CHARS: usize = 512;

/// Directories the walk never enters.
const SKIP_DIRS: [&str; 8] = [
    "node_modules",
    "locales",
    "__MACOSX",
    ".git",
    "swiftshader",
    "legal",
    "$PLUGINSDIR",
    "$TEMP",
];
/// File names that mark machine code (lower case).
const BINARY_EXT: [&str; 8] = [
    ".exe", ".dll", ".node", ".dylib", ".so", ".pyd", ".jnilib", ".sys",
];
/// File names that are libraries, never the program (lower case).
const LIBRARY_EXT: [&str; 5] = [".dll", ".so", ".dylib", ".node", ".pyd"];
/// Programs that sit beside NW.js's and are not the application.
const NW_HELPERS: [&str; 12] = [
    "notification_helper.exe",
    "nwjc.exe",
    "chromedriver.exe",
    "payload.exe",
    "nwjc",
    "chromedriver",
    "chrome_crashpad_handler",
    "crashpad_handler",
    "chrome-sandbox",
    "minidump_stackwalk",
    "nacl_helper",
    "nacl_helper_bootstrap",
];
/// The .NET framework that exists only on Windows.
const WINDOWS_DESKTOP: &str = "Microsoft.WindowsDesktop.App";
/// Assemblies of Windows-only user interfaces (lower case).
const WINDOWS_UI: [&str; 6] = [
    "presentationframework.dll",
    "system.windows.forms.dll",
    "microsoft.winui.dll",
    "microsoft.windowsappsdk.dll",
    "microsoft.maui.dll",
    "winrt.runtime.dll",
];
/// The subset of [`WINDOWS_UI`] the prototype looks for beside a
/// `runtimeconfig.json` (WPF and Windows Forms are named by the framework).
const WINDOWS_UI_BESIDE: [&str; 3] = [
    "microsoft.winui.dll",
    "microsoft.windowsappsdk.dll",
    "microsoft.maui.dll",
];
/// The signature of a .NET single-file bundle.
const BUNDLE_SIGNATURE: &[u8] = b"\x8b\x12\x02\xb9\x6a\x61\x20\x38\x72\x7b\x93\x02\x14\xd7\xa0\x32\
\x13\xf5\xb9\xe6\xef\xae\x33\x18\xee\x3b\x2d\xce\x24\xb3\x6a\xae";
/// The magic of PyInstaller's archive cookie.
const PYI_MAGIC: &[u8] = b"MEI\x0c\x0b\x0a\x0b\x0e";
/// Files and directories (lower case) that name a native toolkit.
const NATIVE_MARKERS: [(&str, &[&str]); 11] = [
    (
        "qt",
        &[
            "qt6core.dll",
            "qt5core.dll",
            "qtcore4.dll",
            "qt6widgets.dll",
            "qtcore.framework",
            "qt6gui.dll",
            "libqt5core.so.5",
            "libqt6core.so.6",
        ],
    ),
    (
        "flutter",
        &[
            "flutter_windows.dll",
            "fluttermacos.framework",
            "flutter_assets",
            "libflutter_linux_gtk.so",
            "app.framework",
        ],
    ),
    (
        "unity",
        &[
            "unityplayer.dll",
            "unityplayer.dylib",
            "unityplayer.so",
            "unitycrashhandler64.exe",
        ],
    ),
    (
        "unreal",
        &["ue4prereqsetup_x64.exe", "ueprereqsetup_x64.exe", "engine"],
    ),
    (
        "cef",
        &[
            "libcef.dll",
            "chromium embedded framework.framework",
            "libcef.so",
        ],
    ),
    (
        "webview2",
        &["webview2loader.dll", "microsoft.web.webview2.core.dll"],
    ),
    ("gamemaker", &["data.win", "game.ios", "game.unx"]),
    (
        "wxwidgets",
        &["wxbase32u_vc_custom.dll", "wxmsw32u_core_vc_custom.dll"],
    ),
    ("gtk", &["libgtk-3-0.dll", "libgtk-4-1.dll", "gtk-3-vs17.dll"]),
    ("sdl", &["sdl2.dll", "sdl3.dll"]),
    ("uwp-msix", &["appxmanifest.xml"]),
];

// ------------------------------------------------------------------ helpers

/// A string from a file, fit for a finding: control characters removed,
/// at most [`TEXT_CHARS`] characters, trimmed.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| clean(&name.to_string_lossy()))
        .unwrap_or_default()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

fn is_file(path: &Path) -> bool {
    sniff::regular_len(path).is_some()
}

/// `base` joined with a relative path, no directory on the way being a
/// symbolic link (see [`asar::resolve_below`]).
fn below(base: &Path, rel: &str) -> Option<PathBuf> {
    asar::resolve_below(base, rel)
}

fn read_text(path: &Path, limit: u64) -> Option<String> {
    let raw = sniff::read_at(path, 0, limit)?;
    let body = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw);
    Some(String::from_utf8_lossy(body).into_owned())
}

fn read_json(path: &Path) -> Option<Value> {
    asar::parse_json(&sniff::read_at(path, 0, JSON_MAX)?)
}

/// The names in a directory, sorted (at most 4096; names that are not
/// UTF-8 are left out).
fn list_names(directory: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .take(4096)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// A non-empty string field of a JSON object, cleaned.
fn json_text(value: &Value, key: &str) -> Option<String> {
    let text = clean(value.get(key)?.as_str()?);
    (!text.is_empty()).then_some(text)
}

/// The `version` of a JSON object: a string, or a number written out.
fn json_version(value: &Value) -> Option<String> {
    match value.get("version")? {
        Value::String(text) => {
            let text = clean(text);
            (!text.is_empty()).then_some(text)
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn base(family: Family, platform: Platform, app_dir: &Path) -> Finding {
    Finding {
        family,
        toolkit: None,
        platform,
        app_dir: app_dir.to_path_buf(),
        depth: 0,
        arch: Vec::new(),
        version: None,
        version_evidence: Vec::new(),
        verdict: Verdict::Rehostable,
        reasons: Vec::new(),
        notes: Vec::new(),
        electron: None,
        natives: Vec::new(),
        sidecars: Vec::new(),
    }
}

fn push_evidence(finding: &mut Finding, from: &str, value: &str) {
    let value = clean(value);
    if !value.is_empty() {
        finding.version_evidence.push(Evidence {
            from: clean(from),
            value,
        });
    }
}

/// The version is the first evidence, when there is any.
fn settle_version(finding: &mut Finding) {
    if finding.version.is_none() {
        finding.version = finding
            .version_evidence
            .first()
            .map(|evidence| evidence.value.clone());
    }
}

fn reason(finding: &mut Finding, code: &str) {
    finding.reasons.push(code.to_owned());
}

fn note(finding: &mut Finding, text: String) {
    finding.notes.push(text);
}

/// A rehostable verdict becomes rehostable with work; a refusal stays.
fn with_work(finding: &mut Finding) {
    if finding.verdict == Verdict::Rehostable {
        finding.verdict = Verdict::RehostableWithWork;
    }
}

fn version_unknown(finding: &mut Finding) {
    if finding.version.is_none() {
        reason(finding, "runtime-version-unknown");
        with_work(finding);
    }
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

fn expect_byte(bytes: &[u8], at: usize, wanted: u8) -> Option<usize> {
    (bytes.get(at) == Some(&wanted)).then_some(at + 1)
}

/// A run of one to nine ASCII digits at `at`: its end and its text.
fn digits_at(bytes: &[u8], at: usize) -> Option<(usize, String)> {
    let count = bytes
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take(9)
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if count == 0 {
        return None;
    }
    let end = at + count;
    Some((end, String::from_utf8_lossy(bytes.get(at..end)?).into_owned()))
}

/// `[0-9]+\.[0-9]+` at the start of a text.
fn starts_with_version(text: &str) -> bool {
    let bytes = text.as_bytes();
    digits_at(bytes, 0)
        .and_then(|(end, _)| expect_byte(bytes, end, b'.'))
        .and_then(|end| digits_at(bytes, end))
        .is_some()
}

/// The first three digit groups of a text, joined by dots.
fn dotted(text: &str) -> String {
    text.split(|character: char| !character.is_ascii_digit())
        .filter(|group| !group.is_empty())
        .take(3)
        .collect::<Vec<&str>>()
        .join(".")
}

// ------------------------------------------------------------ property list

/// The string values of the top-level dictionary of an XML property list
/// (at most 1 MiB is read). A binary property list gives nothing.
fn plist_strings(path: &Path) -> BTreeMap<String, String> {
    let Some(raw) = sniff::read_at(path, 0, PLIST_MAX) else {
        return BTreeMap::new();
    };
    if raw.starts_with(b"bplist") {
        return BTreeMap::new();
    }
    top_level_strings(&String::from_utf8_lossy(&raw))
}

/// A scanner, not an XML parser: it pairs each `<key>` of the top-level
/// dictionary with an immediately following `<string>`, and skips the
/// rest. It does not recurse. The first value of a repeated key wins;
/// at most 256 keys and values of at most 4096 bytes are kept.
fn top_level_strings(text: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut rest = text;
    let mut depth: u32 = 0;
    let mut key: Option<String> = None;
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
            "dict" | "array" if !self_closing => {
                depth = depth.saturating_add(1);
                key = None;
            }
            "/dict" | "/array" => {
                depth = depth.saturating_sub(1);
                key = None;
            }
            "key" if !self_closing => {
                let Some(end) = rest.find("</key>") else {
                    break;
                };
                key = (depth == 1).then(|| decode_entities(&rest[..end]));
                rest = &rest[end + "</key>".len()..];
            }
            "string" if !self_closing => {
                let Some(end) = rest.find("</string>") else {
                    break;
                };
                if depth == 1
                    && end <= 4096
                    && found.len() < 256
                    && let Some(pending) = key.take()
                {
                    found
                        .entry(pending)
                        .or_insert_with(|| decode_entities(&rest[..end]));
                }
                key = None;
                rest = &rest[end + "</string>".len()..];
            }
            _ => key = None,
        }
    }
    found
}

fn decode_entities(raw: &str) -> String {
    clean(
        &raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

// --------------------------------------------------------------------- walk

/// The entries of one directory: files (and links to files) in `names`,
/// directories in `dirs`. A link to a directory is in neither: it is never
/// followed.
struct Listing {
    names: Vec<String>,
    dirs: Vec<String>,
}

fn list_dir(path: &Path, budget: &mut usize) -> Option<Listing> {
    let entries = fs::read_dir(path).ok()?;
    let mut names = Vec::new();
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let linked_dir = kind.is_symlink() && fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
        if kind.is_dir() {
            dirs.push(name);
        } else if !linked_dir {
            names.push(name);
        }
    }
    names.sort();
    dirs.sort();
    Some(Listing { names, dirs })
}

/// A program found in a directory (a PE, ELF or Mach-O file larger than
/// 4096 bytes whose name is not a library's).
struct Exe {
    size: u64,
    name: String,
    kind: BinaryKind,
}

fn programs_in(directory: &Path, names: &[String]) -> Vec<Exe> {
    let mut programs = Vec::new();
    for name in names.iter().take(MAX_PROGRAM_CHECKS) {
        let lower = name.to_ascii_lowercase();
        if LIBRARY_EXT.iter().any(|ext| lower.ends_with(ext)) || lower.contains(".so.") {
            continue;
        }
        let path = directory.join(name);
        let Some(size) = sniff::regular_len(&path) else {
            continue;
        };
        if size <= 4096 {
            continue;
        }
        if let Some(kind) = sniff::binary_kind(&path) {
            programs.push(Exe {
                size,
                name: name.clone(),
                kind,
            });
        }
    }
    programs.sort_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| right.name.cmp(&left.name))
    });
    programs
}

/// One directory as the detectors see it.
struct Place<'a> {
    path: &'a Path,
    names: &'a [String],
    dirs: &'a [String],
    lower: BTreeSet<String>,
    depth: usize,
    exes: OnceCell<Vec<Exe>>,
}

impl Place<'_> {
    /// Whether the directory holds a file of this lower-case name.
    fn has(&self, lower_name: &str) -> bool {
        self.lower.contains(lower_name)
    }

    /// The real name of the file of this lower-case name.
    fn name_like(&self, lower_name: &str) -> Option<&String> {
        self.names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(lower_name))
    }

    /// The programs of the given formats, largest first.
    fn exes(&self, formats: &[BinaryFormat]) -> Vec<&Exe> {
        self.exes
            .get_or_init(|| programs_in(self.path, self.names))
            .iter()
            .filter(|exe| formats.contains(&exe.kind.format))
            .collect()
    }
}

/// A program seen by the walk, kept for the native verdict.
struct Program {
    depth: usize,
    path: PathBuf,
    kind: BinaryKind,
    size: u64,
}

struct Walk {
    programs: Vec<Program>,
    file_checks: usize,
}

/// The sort rank of a family: runtimes before what they may carry.
fn order(family: Family) -> u8 {
    match family {
        Family::Electron | Family::Nwjs => 0,
        Family::Renpy | Family::Godot | Family::Love => 1,
        Family::Dotnet | Family::Jvm => 2,
        Family::Pyinstaller => 3,
        Family::DotnetFramework => 4,
        Family::Native => 5,
    }
}

/// Whether a relative path names an ARM build: `arm64` or `aarch64` not
/// inside a longer word (the prototype's
/// `(^|[^a-z])(arm64|aarch64)([^a-z]|$)`, case-insensitive).
fn mentions_arm(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    ["arm64", "aarch64"].iter().any(|word| {
        let mut from = 0;
        while let Some(at) = sniff::find_bytes(bytes, word.as_bytes(), from) {
            let before = at.checked_sub(1).and_then(|index| bytes.get(index));
            let after = bytes.get(at + word.len());
            if !before.is_some_and(u8::is_ascii_lowercase) && !after.is_some_and(u8::is_ascii_lowercase) {
                return true;
            }
            from = at + 1;
        }
        false
    })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Every finding below `root`, the most significant first.
///
/// The walk goes at most nine levels deep, never enters `node_modules`,
/// `locales`, `__MACOSX`, `.git`, `swiftshader`, `legal`, `$PLUGINSDIR`,
/// `$TEMP` or a `*.lproj` directory, never follows a symbolic link, and
/// examines at most 200 000 entries. A directory that yields a finding is
/// claimed: nothing below it is examined (a .NET Framework finding claims
/// nothing). Findings are sorted by family (Electron and NW.js first,
/// then Ren'Py, Godot and LÖVE, then .NET and Java, then PyInstaller, then
/// .NET Framework), an ARM build after its x86-64 sibling, then by depth.
/// When nothing portable is found, the programs seen decide a native
/// finding (Qt, Flutter, Tauri, ...), unless a .NET Framework program was
/// found, which describes the package better.
pub fn detect(root: &Path) -> Vec<Finding> {
    let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let mut walk = Walk {
        programs: Vec::new(),
        file_checks: 0,
    };
    let mut findings: Vec<Finding> = Vec::new();
    let mut all_names: BTreeSet<String> = BTreeSet::new();
    let mut budget = MAX_FILES;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.clone(), 0)];
    while let Some((directory, depth)) = stack.pop() {
        if budget == 0 {
            break;
        }
        let Some(listing) = list_dir(&directory, &mut budget) else {
            continue;
        };
        let dirs: Vec<String> = listing
            .dirs
            .into_iter()
            .filter(|name| !SKIP_DIRS.contains(&name.as_str()) && !name.ends_with(".lproj"))
            .collect();
        all_names.extend(listing.names.iter().map(|name| name.to_ascii_lowercase()));
        all_names.extend(dirs.iter().map(|name| name.to_ascii_lowercase()));
        let place = Place {
            path: &directory,
            names: &listing.names,
            dirs: &dirs,
            lower: listing
                .names
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
            depth,
            exes: OnceCell::new(),
        };
        if let Some(mut finding) = examine(&place, &mut walk) {
            finding.depth = depth;
            let claims = finding.family != Family::DotnetFramework;
            findings.push(finding);
            if claims {
                continue;
            }
        }
        if depth < MAX_DEPTH {
            for name in dirs.iter().rev() {
                stack.push((directory.join(name), depth + 1));
            }
        }
    }
    findings.sort_by_key(|finding| {
        (
            order(finding.family),
            mentions_arm(&relative(&root, &finding.app_dir)),
            finding.depth,
        )
    });
    if findings.is_empty() {
        walk.programs
            .sort_by_key(|program| (program.depth, Reverse(program.size)));
        if let Some(native) = native_finding(&root, &all_names, &walk.programs) {
            findings.push(native);
        }
    }
    findings
}

/// The most significant finding: the first of [`detect`]'s list.
pub fn primary(findings: &[Finding]) -> Option<&Finding> {
    findings.first()
}

/// The detectors, in the prototype's order, for one directory.
fn examine(place: &Place<'_>, walk: &mut Walk) -> Option<Finding> {
    if file_name(place.path).ends_with(".app") && is_real_dir(&place.path.join("Contents")) {
        let found = electron_app(place.path)
            .or_else(|| nwjs_app(place.path))
            .or_else(|| love_app(place.path));
        if found.is_some() {
            return found;
        }
    }
    if place.dirs.iter().any(|name| name == "resources")
        && let Some(found) = electron_dir(place)
    {
        return Some(found);
    }
    let found = nwjs_dir(place)
        .or_else(|| renpy_dir(place))
        .or_else(|| godot_dir(place))
        .or_else(|| dotnet_dir(place))
        .or_else(|| jvm_dir(place));
    if found.is_some() {
        return found;
    }
    if place.depth > 4 {
        return None;
    }
    let formats = [BinaryFormat::Pe, BinaryFormat::Elf, BinaryFormat::MachO];
    for exe in place.exes(&formats).into_iter().take(6) {
        let path = place.path.join(&exe.name);
        walk.programs.push(Program {
            depth: place.depth,
            path: path.clone(),
            kind: exe.kind.clone(),
            size: exe.size,
        });
        if walk.file_checks > 40 {
            continue;
        }
        walk.file_checks += 1;
        let found = pyinstaller(&path, &exe.kind)
            .or_else(|| love_file(&path))
            .or_else(|| launch4j(&path))
            .or_else(|| dotnet_bundle(&path, &exe.kind));
        if found.is_some() {
            return found;
        }
    }
    for name in place.names {
        if name.to_ascii_lowercase().ends_with(".love")
            && let Some(found) = love_file(&place.path.join(name))
        {
            return Some(found);
        }
    }
    dotnet_framework(place)
}

/// Walks the regular files below `root` (never through a symbolic link),
/// names sorted, at most [`PAYLOAD_DEPTH`] levels and
/// [`MAX_PAYLOAD_FILES`] entries, skipping the directory `skip`. `visit`
/// gets the `/`-separated relative path, the path and the size, and
/// returns `false` to stop.
fn walk_files(root: &Path, skip: Option<&Path>, visit: &mut dyn FnMut(&str, &Path, u64) -> bool) {
    let mut budget = MAX_PAYLOAD_FILES;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((directory, prefix, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                subdirs.push(name);
            } else if kind.is_file() {
                files.push(name);
            }
        }
        files.sort();
        subdirs.sort();
        for name in files {
            let path = directory.join(&name);
            let Some(size) = sniff::regular_len(&path) else {
                continue;
            };
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if !visit(&rel, &path, size) {
                return;
            }
        }
        if budget == 0 || depth + 1 >= PAYLOAD_DEPTH {
            continue;
        }
        for name in subdirs.into_iter().rev() {
            let path = directory.join(&name);
            if skip.is_some_and(|skipped| skipped == path.as_path()) {
                continue;
            }
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            stack.push((path, rel, depth + 1));
        }
    }
}

/// Machine-code files below `directory` that are not Node modules: names
/// ending in a binary extension, holding `.so.`, or without a dot, of at
/// least 4096 bytes. Paths get `prefix`. At most [`MAX_SIDECARS`] in `out`.
fn collect_sidecars(directory: &Path, skip: Option<&Path>, prefix: &str, out: &mut Vec<Sidecar>) {
    if out.len() >= MAX_SIDECARS {
        return;
    }
    walk_files(directory, skip, &mut |rel, path, size| {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".node") || size < 4096 {
            return true;
        }
        let candidate = BINARY_EXT.iter().any(|ext| lower.ends_with(ext))
            || lower.contains(".so.")
            || !name.contains('.');
        if candidate && let Some(kind) = sniff::binary_kind(path) {
            out.push(Sidecar {
                path: clean(&format!("{prefix}{rel}")),
                kind,
                size,
            });
        }
        out.len() < MAX_SIDECARS
    });
}

// ----------------------------------------------------------------- payloads

/// Where the files of an Electron or NW.js payload are.
enum Source {
    /// An asar archive (with its unpacked directory).
    Archive(Asar),
    /// A plain directory.
    Directory(PathBuf),
}

/// The npm package that owns a file.
#[derive(Clone)]
struct Package {
    dir: String,
    name: String,
    version: Option<String>,
    os: Vec<String>,
}

/// The application files of an Electron or NW.js program. Paths are
/// relative and `/`-separated.
struct Payload {
    source: Source,
    /// Inner archives of a directory payload (`node_modules.asar`), opened
    /// on demand; `None` for one that could not be opened.
    inner: RefCell<BTreeMap<PathBuf, Option<Asar>>>,
}

impl Payload {
    fn new(source: Source) -> Payload {
        Payload {
            source,
            inner: RefCell::new(BTreeMap::new()),
        }
    }

    fn archive(path: &Path) -> Option<Payload> {
        Asar::open(path)
            .ok()
            .map(|asar| Payload::new(Source::Archive(asar)))
    }

    fn directory(path: &Path) -> Payload {
        Payload::new(Source::Directory(path.to_path_buf()))
    }

    fn kind(&self) -> PayloadKind {
        match self.source {
            Source::Archive(_) => PayloadKind::Asar,
            Source::Directory(_) => PayloadKind::Directory,
        }
    }

    fn root(&self) -> &Path {
        match &self.source {
            Source::Archive(asar) => asar.path.as_path(),
            Source::Directory(directory) => directory.as_path(),
        }
    }

    fn read(&self, rel: &str, limit: u64) -> Option<Vec<u8>> {
        match &self.source {
            Source::Archive(asar) => asar.read(rel, limit),
            Source::Directory(directory) => {
                if let Some(path) = below(directory, rel)
                    && let Some(bytes) = sniff::read_at(&path, 0, limit)
                {
                    return Some(bytes);
                }
                self.inner_read(directory, rel, limit)
            }
        }
    }

    /// A directory payload may keep its modules in an archive of their own
    /// (VS Code: `node_modules.asar` beside `node_modules.asar.unpacked/`):
    /// a path through `<head>.asar.unpacked/` is read through `<head>.asar`.
    fn inner_read(&self, directory: &Path, rel: &str, limit: u64) -> Option<Vec<u8>> {
        let (head, tail) = rel.split_once(".asar.unpacked/")?;
        let archive = below(directory, &format!("{head}.asar"))?;
        let mut cache = self.inner.borrow_mut();
        if !cache.contains_key(&archive) && cache.len() >= MAX_INNER_ARCHIVES {
            return None;
        }
        let opened = cache
            .entry(archive.clone())
            .or_insert_with(|| Asar::open(&archive).ok());
        opened.as_ref()?.read(tail, limit)
    }

    fn read_json(&self, rel: &str) -> Option<Value> {
        asar::parse_json(&self.read(rel, JSON_MAX)?)
    }

    /// The `package.json` of a directory of the payload, when it names a
    /// package.
    fn package_at(&self, dir: &str) -> Option<Package> {
        let json = self.read_json(&format!("{dir}/package.json"))?;
        let name = json_text(&json, "name")?;
        let os = json
            .get("os")
            .and_then(Value::as_array)
            .map(|systems| {
                systems
                    .iter()
                    .filter_map(Value::as_str)
                    .map(clean)
                    .take(32)
                    .collect()
            })
            .unwrap_or_default();
        Some(Package {
            dir: clean(dir),
            name,
            version: json_version(&json),
            os,
        })
    }

    /// The nearest `package.json` with a name above a file.
    fn package_of(&self, rel: &str, cache: &mut BTreeMap<String, Option<Package>>) -> Option<Package> {
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        while !parts.is_empty() {
            let dir = parts.join("/");
            let entry = cache
                .entry(dir.clone())
                .or_insert_with(|| self.package_at(&dir));
            if let Some(found) = entry {
                return Some(found.clone());
            }
            parts.pop();
        }
        None
    }

    /// Every `*.node` file of at least 512 bytes, with the machine code
    /// its first 4096 bytes hold and the package that owns it.
    fn natives(&self) -> Vec<NativeModule> {
        let mut candidates: Vec<(String, u64, Location)> = Vec::new();
        match &self.source {
            Source::Archive(asar) => {
                for entry in asar.walk() {
                    if candidates.len() >= MAX_NATIVES {
                        break;
                    }
                    if !entry.is_dir && !entry.is_link && entry.size >= 512 && entry.path.ends_with(".node") {
                        let location = if entry.unpacked {
                            Location::Unpacked
                        } else {
                            Location::Packed
                        };
                        candidates.push((entry.path, entry.size, location));
                    }
                }
            }
            Source::Directory(directory) => {
                walk_files(directory, None, &mut |rel, _path, size| {
                    if size >= 512 && rel.ends_with(".node") {
                        candidates.push((rel.to_owned(), size, Location::Directory));
                    }
                    candidates.len() < MAX_NATIVES
                });
            }
        }
        let mut packages: BTreeMap<String, Option<Package>> = BTreeMap::new();
        let mut natives = Vec::new();
        for (rel, size, location) in candidates {
            let Some(head) = self.read(&rel, sniff::HEAD_LEN) else {
                continue;
            };
            let Some(kind) = sniff::binary_kind_of_head(&head) else {
                continue;
            };
            let package = self.package_of(&rel, &mut packages);
            natives.push(NativeModule {
                path: clean(&rel),
                location,
                kind,
                size,
                package: package.as_ref().map(|found| found.name.clone()),
                version: package.as_ref().and_then(|found| found.version.clone()),
                package_dir: package.as_ref().map(|found| found.dir.clone()),
                os: package.map(|found| found.os).unwrap_or_default(),
            });
        }
        natives
    }
}

/// The natives, sidecars and reasons a web payload gives a finding.
fn apply_natives(finding: &mut Finding, natives: Vec<NativeModule>, sidecars: Vec<Sidecar>) {
    let foreign: Vec<&NativeModule> = natives
        .iter()
        .filter(|module| module.kind.format != BinaryFormat::Elf)
        .collect();
    if let Some(first) = foreign.first() {
        let count = foreign.len();
        let platform = first.kind.format.platform().as_str();
        reason(finding, "native-modules");
        finding.verdict = Verdict::RehostableWithWork;
        note(
            finding,
            format!("{count} native Node module(s) built for {platform} need a Linux build"),
        );
        let orphans = foreign.iter().filter(|module| module.package.is_none()).count();
        if orphans > 0 {
            note(
                finding,
                format!(
                    "{orphans} of them have no package.json above them: no public source can be found for them by name"
                ),
            );
        }
    }
    if sidecars
        .iter()
        .any(|sidecar| sidecar.kind.format != BinaryFormat::Elf)
    {
        reason(finding, "platform-helper-binaries");
    }
    finding.natives = natives;
    finding.sidecars = sidecars;
}

// ----------------------------------------------------------------- Electron

/// `resources/app.asar`, or `resources/app/` or `resources/default_app/`
/// with a `package.json`.
fn electron_payload(resources: &Path) -> Option<Payload> {
    let archive = resources.join("app.asar");
    if is_file(&archive) {
        return Payload::archive(&archive);
    }
    for name in ["app", "default_app"] {
        let directory = resources.join(name);
        if is_real_dir(&directory) && is_file(&directory.join("package.json")) {
            return Some(Payload::directory(&directory));
        }
    }
    None
}

/// What an Electron payload holds besides the version.
struct WebFacts {
    package: Value,
    natives: Vec<NativeModule>,
    modules_root: PathBuf,
    sidecars: Vec<Sidecar>,
    updater: Option<String>,
}

/// The payload's `package.json`, its native modules (from `app-x64.asar`
/// when a universal macOS build has one: the per-processor archive holds
/// them), the platform-specific files under `resources`, and the updater.
fn describe_web(payload: &Payload, resources: &Path) -> WebFacts {
    let package = payload
        .read_json("package.json")
        .filter(Value::is_object)
        .unwrap_or(Value::Null);
    let split = if payload.kind() == PayloadKind::Asar {
        let path = resources.join("app-x64.asar");
        if is_file(&path) {
            Payload::archive(&path)
        } else {
            None
        }
    } else {
        None
    };
    let modules = split.as_ref().unwrap_or(payload);
    let natives = modules.natives();
    let modules_root = modules.root().to_path_buf();
    let directory = match &payload.source {
        Source::Directory(directory) => Some(directory.as_path()),
        Source::Archive(_) => None,
    };
    let mut sidecars = Vec::new();
    collect_sidecars(resources, directory, "", &mut sidecars);
    if let Some(directory) = directory {
        let prefix = format!("{}/", file_name(directory));
        collect_sidecars(directory, None, &prefix, &mut sidecars);
    }
    let updater = fs::symlink_metadata(resources.join("app-update.yml"))
        .is_ok()
        .then(|| "electron-updater".to_owned());
    WebFacts {
        package,
        natives,
        modules_root,
        sidecars,
        updater,
    }
}

/// The version from the evidence, or the reason it is unknown; a vendor
/// build (a version with `+`) is said so.
fn settle_electron(finding: &mut Finding) {
    settle_version(finding);
    version_unknown(finding);
    if finding
        .version
        .as_deref()
        .is_some_and(|version| version.contains('+'))
    {
        reason(finding, "vendor-electron-build");
        note(
            finding,
            "a vendor build of Electron (for example castlabs' with Widevine): the stock build lacks what the vendor added"
                .to_owned(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn electron_payload_of(
    app_dir: &Path,
    resources: &Path,
    program: Option<String>,
    payload: &Payload,
    facts: &WebFacts,
    chromium: Option<String>,
    fuses: Option<BTreeMap<String, String>>,
    bundle_id: Option<String>,
) -> ElectronPayload {
    ElectronPayload {
        app_dir: app_dir.to_path_buf(),
        resources: resources.to_path_buf(),
        program,
        kind: payload.kind(),
        modules_root: facts.modules_root.clone(),
        name: json_text(&facts.package, "productName").or_else(|| json_text(&facts.package, "name")),
        package_name: json_text(&facts.package, "name"),
        app_version: json_version(&facts.package),
        main: json_text(&facts.package, "main"),
        chromium,
        fuses,
        bundle_id,
        updater: facts.updater.clone(),
    }
}

/// Windows and Linux layout: `<app>/resources/app.asar` (or `app/`)
/// beside the program.
fn electron_dir(place: &Place<'_>) -> Option<Finding> {
    let resources = place.path.join("resources");
    let payload = electron_payload(&resources)?;
    let exes = place.exes(&[BinaryFormat::Pe, BinaryFormat::Elf]);
    let main = *exes.first()?;
    if ["nw.dll", "nw_elf.dll", "nw.pak", "nw_100_percent.pak"]
        .iter()
        .any(|name| place.has(name))
    {
        return None;
    }
    let mut finding = base(Family::Electron, main.kind.format.platform(), place.path);
    finding.arch = main.kind.archs.clone();
    let binary = place.path.join(&main.name);
    let program = clean(&main.name);
    for value in scan::electron_versions(&binary, 3) {
        push_evidence(&mut finding, &format!("user-agent string in {program}"), &value);
    }
    if let Some(text) = read_text(&place.path.join("version"), 64) {
        let text = text.trim();
        if is_version_file(text) {
            push_evidence(&mut finding, "version file", text.trim_start_matches('v'));
        }
    }
    let chromium = scan::chrome_versions(&binary, 3).into_iter().next();
    let fuses = scan::read_fuses(&binary);
    settle_electron(&mut finding);
    let facts = describe_web(&payload, &resources);
    finding.electron = Some(electron_payload_of(
        place.path,
        &resources,
        Some(program),
        &payload,
        &facts,
        chromium,
        fuses,
        None,
    ));
    apply_natives(&mut finding, facts.natives, facts.sidecars);
    Some(finding)
}

/// `v?[0-9]+\.[0-9]+\.[0-9]+\S*`, the whole (trimmed) text.
fn is_version_file(text: &str) -> bool {
    let body = text.strip_prefix('v').unwrap_or(text);
    let bytes = body.as_bytes();
    let Some((end, _)) = digits_at(bytes, 0) else {
        return false;
    };
    let Some(end) = expect_byte(bytes, end, b'.').and_then(|at| digits_at(bytes, at)) else {
        return false;
    };
    let Some(end) = expect_byte(bytes, end.0, b'.').and_then(|at| digits_at(bytes, at)) else {
        return false;
    };
    // Python's \d+ is unbounded: the rest may continue with digits.
    !body
        .get(end.0..)
        .unwrap_or_default()
        .chars()
        .any(char::is_whitespace)
}

/// macOS layout: `X.app/Contents/Frameworks/Electron Framework.framework`,
/// the payload in `Contents/Resources`.
fn electron_app(bundle: &Path) -> Option<Finding> {
    let framework = below(bundle, "Contents/Frameworks/Electron Framework.framework")
        .filter(|path| is_real_dir(path))?;
    let resources = below(bundle, "Contents/Resources").filter(|path| is_real_dir(path))?;
    let payload = electron_payload(&resources)?;
    let mut finding = base(Family::Electron, Platform::Macos, bundle);
    let info = below(bundle, "Contents/Info.plist")
        .map(|path| plist_strings(&path))
        .unwrap_or_default();
    let binary = ["Versions/A/Electron Framework", "Electron Framework"]
        .iter()
        .filter_map(|candidate| below(&framework, candidate))
        .find(|path| is_file(path));
    if let Some(binary) = &binary {
        finding.arch = sniff::binary_kind(binary)
            .map(|kind| kind.archs)
            .unwrap_or_default();
        for value in scan::electron_versions(binary, 3) {
            push_evidence(&mut finding, "user-agent string in Electron Framework", &value);
        }
    }
    let chromium = binary
        .as_deref()
        .and_then(|path| scan::chrome_versions(path, 3).into_iter().next());
    let fuses = binary.as_deref().and_then(scan::read_fuses);
    for candidate in ["Versions/A/Resources/Info.plist", "Resources/Info.plist"] {
        let version = below(&framework, candidate)
            .and_then(|path| plist_strings(&path).remove("CFBundleVersion"));
        if let Some(version) = version {
            push_evidence(&mut finding, "Electron Framework Info.plist", &version);
            break;
        }
    }
    settle_electron(&mut finding);
    let facts = describe_web(&payload, &resources);
    finding.electron = Some(electron_payload_of(
        bundle,
        &resources,
        info.get("CFBundleExecutable").cloned(),
        &payload,
        &facts,
        chromium,
        fuses,
        info.get("CFBundleIdentifier").cloned(),
    ));
    apply_natives(&mut finding, facts.natives, facts.sidecars);
    Some(finding)
}

// -------------------------------------------------------------------- NW.js

fn nw_evidence(finding: &mut Finding, binary: &Path) {
    let name = file_name(binary);
    for value in scan::nw_versions(binary, 3) {
        push_evidence(finding, &format!("version string in {name}"), &value);
    }
}

/// NW.js binaries name Chromium's version, not their own: without a
/// version string the version stays unknown here (the prototype looked it
/// up in nwjs.io's index, which detection does not fetch).
fn nw_version_unknown(finding: &mut Finding) {
    settle_version(finding);
    if finding.version.is_none() {
        version_unknown(finding);
        note(
            finding,
            "the NW.js version is not written in its binaries: it has to be looked up from Chromium's version in nwjs.io's index"
                .to_owned(),
        );
    }
}

/// `package.nw` as a zip archive: its native modules are counted from the
/// central directory.
fn describe_package_nw(finding: &mut Finding, path: &Path) {
    let names = sniff::zip_tail(path).and_then(|tail| sniff::zip_names(path, &tail, 100_000));
    match names {
        Some(names) => {
            let modules = names.iter().filter(|name| name.ends_with(".node")).count();
            if modules > 0 {
                note(
                    finding,
                    format!("package.nw holds {modules} native Node module(s); they are listed once it is unpacked"),
                );
            }
        }
        None => note(
            finding,
            "package.nw could not be read as a zip archive".to_owned(),
        ),
    }
}

/// Windows and Linux layout: the payload beside `nw.dll` or `lib/libnw.so`
/// (in `package.nw`, or `package.json` beside the program).
fn nwjs_dir(place: &Place<'_>) -> Option<Finding> {
    let windows = place.name_like("nw.dll");
    let libnw = below(place.path, "lib/libnw.so").filter(|path| is_file(path));
    let library = match (windows, libnw) {
        (Some(name), _) => place.path.join(name),
        (None, Some(path)) => path,
        (None, None) => return None,
    };
    let platform = if windows.is_some() {
        Platform::Windows
    } else {
        Platform::Linux
    };
    let mut finding = base(Family::Nwjs, platform, place.path);
    let program = place
        .exes(&[BinaryFormat::Pe, BinaryFormat::Elf])
        .into_iter()
        .find(|exe| !NW_HELPERS.contains(&exe.name.to_ascii_lowercase().as_str()));
    nw_evidence(&mut finding, &library);
    if let Some(exe) = program {
        finding.arch = exe.kind.archs.clone();
        nw_evidence(&mut finding, &place.path.join(&exe.name));
    }
    let package_nw = place.path.join("package.nw");
    if is_real_dir(&package_nw) && is_file(&package_nw.join("package.json")) {
        apply_natives(&mut finding, Payload::directory(&package_nw).natives(), Vec::new());
    } else if is_file(&package_nw) {
        describe_package_nw(&mut finding, &package_nw);
    } else if is_file(&place.path.join("package.json")) {
        apply_natives(&mut finding, Payload::directory(place.path).natives(), Vec::new());
    } else {
        reason(&mut finding, "payload-not-found");
        with_work(&mut finding);
        note(
            &mut finding,
            "no package.nw or package.json beside the program: the payload may be appended to the program"
                .to_owned(),
        );
    }
    nw_version_unknown(&mut finding);
    Some(finding)
}

/// macOS layout: `nwjs Framework.framework`, the payload in
/// `Contents/Resources/app.nw`.
fn nwjs_app(bundle: &Path) -> Option<Finding> {
    let framework = below(bundle, "Contents/Frameworks/nwjs Framework.framework")
        .filter(|path| is_real_dir(path))?;
    let mut finding = base(Family::Nwjs, Platform::Macos, bundle);
    let binary = [
        "Versions/A/nwjs Framework",
        "nwjs Framework",
        "Versions/Current/nwjs Framework",
    ]
    .iter()
    .filter_map(|candidate| below(&framework, candidate))
    .find(|path| is_file(path));
    if let Some(binary) = &binary {
        finding.arch = sniff::binary_kind(binary)
            .map(|kind| kind.archs)
            .unwrap_or_default();
        nw_evidence(&mut finding, binary);
    }
    match below(bundle, "Contents/Resources/app.nw") {
        Some(app_nw) if is_real_dir(&app_nw) => {
            apply_natives(&mut finding, Payload::directory(&app_nw).natives(), Vec::new());
        }
        _ => reason(&mut finding, "payload-not-found"),
    }
    nw_version_unknown(&mut finding);
    Some(finding)
}

// ------------------------------------------------------------------- Ren'Py

/// `^version\s*=\s*u?['"]([0-9][0-9.]+)` on a line, else
/// `version_tuple\s*=\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)` anywhere.
fn renpy_version_in(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let rest = rest.strip_prefix('u').unwrap_or(rest);
        let Some(rest) = rest.strip_prefix(['\'', '"']) else {
            continue;
        };
        let version: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit() || *character == '.')
            .take(32)
            .collect();
        if version.len() >= 2 && version.starts_with(|character: char| character.is_ascii_digit()) {
            return Some(version);
        }
    }
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = sniff::find_bytes(bytes, b"version_tuple", from) {
        if let Some(version) = version_tuple_at(bytes, at + b"version_tuple".len()) {
            return Some(version);
        }
        from = at + 1;
    }
    None
}

fn version_tuple_at(bytes: &[u8], at: usize) -> Option<String> {
    let at = skip_spaces(bytes, at);
    let at = expect_byte(bytes, at, b'=')?;
    let at = skip_spaces(bytes, at);
    let mut at = expect_byte(bytes, at, b'(')?;
    let mut parts = Vec::new();
    for index in 0..3 {
        at = skip_spaces(bytes, at);
        if index > 0 {
            at = expect_byte(bytes, at, b',')?;
            at = skip_spaces(bytes, at);
        }
        let (end, part) = digits_at(bytes, at)?;
        parts.push(part);
        at = end;
    }
    Some(parts.join("."))
}

/// A Ren'Py game: `renpy/` and `game/`, the version from
/// `renpy/vc_version.py` or `renpy/__init__.py`.
fn renpy_dir(place: &Place<'_>) -> Option<Finding> {
    if !is_real_dir(&place.path.join("renpy")) || !is_real_dir(&place.path.join("game")) {
        return None;
    }
    let lib = place.path.join("lib");
    let platforms = if is_real_dir(&lib) {
        list_names(&lib)
    } else {
        Vec::new()
    };
    let windows = platforms.iter().any(|name| name.contains("windows"));
    let linux = platforms.iter().any(|name| name.contains("linux-x86_64"));
    let macos = platforms.iter().any(|name| {
        ["py3-mac", "py2-mac", "mac-", "darwin-"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    });
    let in_bundle = place
        .path
        .components()
        .any(|component| component.as_os_str() == "Contents");
    let platform = if in_bundle || (macos && !windows) {
        Platform::Macos
    } else if windows {
        Platform::Windows
    } else if linux {
        Platform::Linux
    } else {
        Platform::Unknown
    };
    let mut finding = base(Family::Renpy, platform, place.path);
    for name in ["vc_version.py", "__init__.py"] {
        let Some(text) = below(place.path, &format!("renpy/{name}"))
            .and_then(|path| read_text(&path, 200_000))
        else {
            continue;
        };
        if let Some(version) = renpy_version_in(&text) {
            push_evidence(&mut finding, &format!("renpy/{name}"), &version);
            break;
        }
    }
    settle_version(&mut finding);
    version_unknown(&mut finding);
    if linux {
        reason(&mut finding, "package-carries-linux-runtime");
        note(
            &mut finding,
            "this package already carries Ren'Py's Linux runtime: it only has to be started".to_owned(),
        );
    }
    Some(finding)
}

// -------------------------------------------------------------------- Godot

/// Where a Godot pack is: the whole file, a PE section named `pck`, or the
/// tail of a program.
struct Located {
    start: u64,
    how: &'static str,
}

fn pck_locate(path: &Path) -> Option<Located> {
    let size = sniff::regular_len(path)?;
    if size < 100 {
        return None;
    }
    let is_pack = |at: u64| sniff::read_at(path, at, 4).is_some_and(|magic| magic == b"GDPC");
    if is_pack(0) {
        return Some(Located {
            start: 0,
            how: "standalone",
        });
    }
    let details = sniff::pe_details(path);
    if let Some(details) = &details
        && let Some(section) = details
            .sections
            .iter()
            .find(|section| section.name == "pck" && section.size > 0 && is_pack(section.raw))
    {
        return Some(Located {
            start: section.raw,
            how: "pe-section",
        });
    }
    let mut ends = vec![size];
    if let Some(details) = &details {
        let signature = details.signature_offset;
        if signature > 0 && signature < size {
            ends.push(signature);
        }
    }
    for end in ends {
        if end < 16 {
            continue;
        }
        let Some(tail) = sniff::read_at(path, end - 12, 12) else {
            continue;
        };
        if tail.get(8..12) != Some(&b"GDPC"[..]) {
            continue;
        }
        let Some(length) = sniff::u64_le(&tail, 0) else {
            continue;
        };
        if let Some(start) = (end - 12).checked_sub(length)
            && is_pack(start)
        {
            return Some(Located {
                start,
                how: "appended",
            });
        }
    }
    None
}

/// The header of a Godot pack and the paths and flags of its files (when
/// its directory is not encrypted and its format is at most 3).
struct Pack {
    engine: String,
    encrypted: bool,
    files: Vec<(String, u32)>,
}

fn pck_read(path: &Path, start: u64) -> Option<Pack> {
    let head = sniff::read_at(path, start, 40)?;
    if !head.starts_with(b"GDPC") {
        return None;
    }
    let format = sniff::u32_le(&head, 4)?;
    let major = sniff::u32_le(&head, 8)?;
    let minor = sniff::u32_le(&head, 12)?;
    let patch = sniff::u32_le(&head, 16)?;
    let flags = if format >= 2 {
        sniff::u32_le(&head, 20)?
    } else {
        0
    };
    let encrypted = flags & 1 != 0;
    let mut pack = Pack {
        engine: format!("{major}.{minor}.{patch}"),
        encrypted,
        files: Vec::new(),
    };
    if encrypted || format > 3 {
        return Some(pack);
    }
    let directory = if format >= 3 {
        start.checked_add(sniff::u64_le(&head, 32)?)?
    } else if format == 2 {
        start + 32 + 64
    } else {
        start + 20 + 64
    };
    let Some(raw) = sniff::read_at(path, directory, PACK_DIRECTORY_MAX) else {
        return Some(pack);
    };
    let count = sniff::u32_le(&raw, 0).unwrap_or(0).min(PACK_FILES_MAX);
    let mut position = 4usize;
    for _ in 0..count {
        let Some(length) = sniff::u32_le(&raw, position).and_then(|length| usize::try_from(length).ok()) else {
            break;
        };
        if length > 4096 {
            break;
        }
        let name_start = position + 4;
        let name_end = name_start + length;
        let Some(name) = raw.get(name_start..name_end) else {
            break;
        };
        let name = String::from_utf8_lossy(name)
            .trim_end_matches('\0')
            .to_owned();
        // Offset and size (8 bytes each), then an MD5 (16 bytes).
        position = name_end + 32;
        let file_flags = if format >= 2 {
            let Some(file_flags) = sniff::u32_le(&raw, position) else {
                break;
            };
            position += 4;
            file_flags
        } else {
            0
        };
        if position > raw.len() {
            break;
        }
        pack.files.push((name, file_flags));
    }
    Some(pack)
}

/// The version, encryption and native extensions of a pack. Returns
/// whether the game is a C# one, or `None` when there is no pack.
fn describe_pck(finding: &mut Finding, carrier: &Path, located: &Located) -> Option<bool> {
    let pack = pck_read(carrier, located.start)?;
    let how = located.how;
    let carrier_name = file_name(carrier);
    push_evidence(
        finding,
        &format!("pack header ({how}) in {carrier_name}"),
        &pack.engine,
    );
    settle_version(finding);
    let encrypted_files = pack
        .files
        .iter()
        .filter(|(_, flags)| flags & 1 != 0)
        .count();
    let extensions = pack
        .files
        .iter()
        .filter(|(path, _)| {
            [".gdextension", ".gdnlib", ".gdns"]
                .iter()
                .any(|ext| path.ends_with(ext))
        })
        .count();
    let csharp = pack.files.iter().any(|(path, _)| {
        path.ends_with(".cs") || path.ends_with(".csproj") || path.contains("/.mono/")
    });
    if pack.encrypted || encrypted_files > 0 {
        reason(finding, "pack-encrypted");
        finding.verdict = Verdict::NotRehostable;
        note(
            finding,
            "the pack is encrypted with a key compiled into the publisher's engine build: the stock engine cannot read it"
                .to_owned(),
        );
    }
    if extensions > 0 {
        reason(finding, "godot-native-extension");
        with_work(finding);
        note(
            finding,
            format!("{extensions} native extension(s) (GDExtension or GDNative): each needs its Linux library"),
        );
    }
    Some(csharp)
}

/// A Godot game: a `.pck` beside the program, or a pack inside it.
fn godot_dir(place: &Place<'_>) -> Option<Finding> {
    let packs: Vec<&String> = place
        .names
        .iter()
        .filter(|name| {
            name.to_ascii_lowercase().ends_with(".pck")
                && sniff::read_at(&place.path.join(name), 0, 4).is_some_and(|magic| magic == b"GDPC")
        })
        .collect();
    let exes = place.exes(&[BinaryFormat::Pe, BinaryFormat::Elf, BinaryFormat::MachO]);
    let (mut finding, csharp) = if let Some(pack) = packs.first() {
        let platform = exes
            .first()
            .map_or(Platform::Any, |exe| exe.kind.format.platform());
        let mut finding = base(Family::Godot, platform, place.path);
        if let Some(exe) = exes.first() {
            finding.arch = exe.kind.archs.clone();
        }
        let located = Located {
            start: 0,
            how: "standalone",
        };
        let csharp = describe_pck(&mut finding, &place.path.join(pack), &located)?;
        (finding, csharp)
    } else {
        let mut found = None;
        for exe in exes.iter().take(4) {
            let carrier = place.path.join(&exe.name);
            if let Some(located) = pck_locate(&carrier) {
                let mut finding = base(Family::Godot, exe.kind.format.platform(), place.path);
                finding.arch = exe.kind.archs.clone();
                found = describe_pck(&mut finding, &carrier, &located).map(|csharp| (finding, csharp));
                break;
            }
        }
        found?
    };
    let mono = place
        .dirs
        .iter()
        .any(|name| name.to_ascii_lowercase().starts_with("data_"));
    if mono || csharp {
        reason(&mut finding, "godot-dotnet");
        with_work(&mut finding);
        note(
            &mut finding,
            "a C# (Godot .NET) game: needs the .NET build of the engine and the game's assemblies re-resolved for Linux"
                .to_owned(),
        );
    }
    for name in place.names {
        if finding.sidecars.len() >= 20 {
            break;
        }
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".dll") || lower.ends_with(".dylib") || lower.ends_with(".so")) {
            continue;
        }
        let path = place.path.join(name);
        if let Some(size) = sniff::regular_len(&path)
            && let Some(kind) = sniff::binary_kind(&path)
        {
            finding.sidecars.push(Sidecar {
                path: clean(name),
                kind,
                size,
            });
        }
    }
    Some(finding)
}

// --------------------------------------------------------------------- .NET

fn config_stem(name: &str) -> &str {
    name.strip_suffix(".runtimeconfig.json").unwrap_or(name)
}

/// A parsed `*.runtimeconfig.json`.
struct RuntimeConfig {
    name: String,
    options: Value,
    frameworks: Vec<String>,
}

/// `frameworks` (or `framework`) and `includedFrameworks` of
/// `runtimeOptions`.
fn framework_entries(options: &Value) -> Vec<&Value> {
    let listed = options
        .get("frameworks")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty());
    let mut entries: Vec<&Value> = match listed {
        Some(list) => list.iter().collect(),
        None => options.get("framework").into_iter().collect(),
    };
    if let Some(included) = options.get("includedFrameworks").and_then(Value::as_array) {
        entries.extend(included);
    }
    entries.retain(|entry| entry.is_object());
    entries
}

/// The version of `Microsoft.NETCore.App` that `runtimeOptions` names.
fn core_version(options: &Value) -> Option<String> {
    framework_entries(options)
        .into_iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some("Microsoft.NETCore.App"))
        .and_then(json_version)
}

fn runtime_config(directory: &Path, name: &str) -> Option<RuntimeConfig> {
    let config = read_json(&directory.join(name))?;
    let options = config
        .get("runtimeOptions")
        .filter(|options| options.is_object())
        .cloned()
        .unwrap_or(Value::Null);
    let frameworks = framework_entries(&options)
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(clean)
        .collect();
    Some(RuntimeConfig {
        name: name.to_owned(),
        options,
        frameworks,
    })
}

fn names_framework(config: &RuntimeConfig, framework: &str) -> bool {
    config.frameworks.iter().any(|name| name == framework)
}

/// The application's own configuration first, not a tool's that travels
/// with it: one with its program beside it, then the graphical one (an
/// Avalonia `deps.json`, then a Windows GUI subsystem), then the shortest
/// name.
fn config_rank(place: &Place<'_>, name: &str) -> (u8, u8, usize) {
    let stem = config_stem(name);
    let mut graphical = 2u8;
    let deps = place.path.join(format!("{stem}.deps.json"));
    if sniff::read_at(&deps, 0, JSON_MAX).is_some_and(|raw| sniff::find_bytes(&raw, b"Avalonia", 0).is_some()) {
        graphical = 0;
    }
    if graphical == 2
        && sniff::pe_info(&place.path.join(format!("{stem}.exe"))).is_some_and(|info| info.subsystem == 2)
    {
        graphical = 1;
    }
    let program = place.has(&format!("{stem}.exe").to_ascii_lowercase()) || place.names.iter().any(|other| other == stem);
    (u8::from(!program), graphical, name.len())
}

fn library_module(name: &str, kind: BinaryKind, size: u64) -> NativeModule {
    NativeModule {
        path: clean(name),
        location: Location::Directory,
        kind,
        size,
        package: None,
        version: None,
        package_dir: None,
        os: Vec::new(),
    }
}

/// Native libraries beside a .NET program: PE DLLs without a CLR header,
/// and ELF or Mach-O libraries.
fn dotnet_natives(place: &Place<'_>) -> Vec<NativeModule> {
    let mut natives = Vec::new();
    for name in place.names {
        if natives.len() >= MAX_SIDECARS {
            break;
        }
        let lower = name.to_ascii_lowercase();
        let dll = lower.ends_with(".dll");
        if !(dll || lower.ends_with(".so") || lower.ends_with(".dylib")) {
            continue;
        }
        let path = place.path.join(name);
        let Some(size) = sniff::regular_len(&path) else {
            continue;
        };
        let info = if dll { sniff::pe_info(&path) } else { None };
        let kind = match info {
            Some(info) if info.clr => continue,
            Some(info) => BinaryKind {
                format: BinaryFormat::Pe,
                archs: vec![info.machine],
            },
            None => match sniff::binary_kind(&path) {
                Some(kind) if kind.format != BinaryFormat::Pe => kind,
                _ => continue,
            },
        };
        natives.push(library_module(name, kind, size));
    }
    natives
}

/// A .NET program: a directory with a `*.runtimeconfig.json`.
fn dotnet_dir(place: &Place<'_>) -> Option<Finding> {
    let mut configs: Vec<&String> = place
        .names
        .iter()
        .filter(|name| name.ends_with(".runtimeconfig.json"))
        .collect();
    if configs.is_empty() {
        return None;
    }
    configs.sort_by_cached_key(|name| config_rank(place, name));
    let parsed: Vec<RuntimeConfig> = configs
        .iter()
        .take(8)
        .filter_map(|name| runtime_config(place.path, name))
        .collect();
    let first = parsed.first()?;
    let mut chosen = first;
    let mut skipped: Option<&RuntimeConfig> = None;
    if names_framework(first, WINDOWS_DESKTOP) {
        // A Windows-only front end (a tray program) beside a portable
        // program of the same application: the portable one is the payload.
        let prefix = format!("{}.", config_stem(&first.name));
        let mut others: Vec<&RuntimeConfig> = parsed.iter().skip(1).collect();
        others.sort_by_key(|config| {
            (
                !config.name.starts_with(&prefix),
                !names_framework(config, "Microsoft.AspNetCore.App"),
                config.name.len(),
            )
        });
        let portable = others.into_iter().find(|config| {
            !names_framework(config, WINDOWS_DESKTOP)
                && place.has(&format!("{}.exe", config_stem(&config.name)).to_ascii_lowercase())
        });
        if let Some(portable) = portable {
            skipped = Some(first);
            chosen = portable;
        }
    }
    let stem = config_stem(&chosen.name);
    let exes = place.exes(&[BinaryFormat::Pe, BinaryFormat::Elf, BinaryFormat::MachO]);
    let platform = match exes.first() {
        Some(exe) => exe.kind.format.platform(),
        None if place.has("coreclr.dll") || place.has("hostfxr.dll") => Platform::Windows,
        None => Platform::Unknown,
    };
    let mut finding = base(Family::Dotnet, platform, place.path);
    if let Some(exe) = exes.first() {
        finding.arch = exe.kind.archs.clone();
    }
    if let Some(version) = core_version(&chosen.options) {
        push_evidence(&mut finding, &chosen.name, &version);
    }
    settle_version(&mut finding);
    let avalonia = place.has("avalonia.dll") || place.has("avalonia.base.dll");
    let windows_ui: Vec<&str> = WINDOWS_UI_BESIDE
        .iter()
        .copied()
        .filter(|name| place.has(name))
        .collect();
    if let Some(front) = skipped {
        let front = clean(config_stem(&front.name));
        let portable = clean(stem);
        reason(&mut finding, "windows-front-end-skipped");
        note(
            &mut finding,
            format!(
                "{front} is a Windows-only front end (Windows desktop framework); {portable} beside it is portable and is what gets re-hosted"
            ),
        );
    }
    if names_framework(chosen, WINDOWS_DESKTOP) || (!windows_ui.is_empty() && !avalonia) {
        finding.verdict = Verdict::NotRehostable;
        reason(&mut finding, "dotnet-windows-desktop");
        let detail = if windows_ui.is_empty() {
            String::new()
        } else {
            format!(": {}", windows_ui.join(", "))
        };
        note(
            &mut finding,
            format!(
                "uses the Windows desktop framework (WPF or Windows Forms{detail}): there is no Linux build of it"
            ),
        );
    }
    finding.natives = dotnet_natives(place);
    if !place.has(&format!("{stem}.dll").to_ascii_lowercase()) {
        reason(&mut finding, "main-assembly-not-found");
        with_work(&mut finding);
    }
    Some(finding)
}

/// One file of a .NET single-file bundle.
struct BundleEntry {
    offset: u64,
    size: u64,
    compressed: u64,
    path: String,
}

/// A 7-bit-encoded length, then that many bytes of UTF-8.
fn bundle_string(buffer: &[u8], at: usize) -> Option<(String, usize)> {
    let mut length: usize = 0;
    let mut position = at;
    for shift in [0u32, 7, 14, 21, 28] {
        let byte = *buffer.get(position)?;
        position += 1;
        length |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            if length > 65_536 {
                return None;
            }
            let end = position.checked_add(length)?;
            let text = String::from_utf8_lossy(buffer.get(position..end)?).into_owned();
            return Some((text, end));
        }
    }
    None
}

/// The manifest of a .NET single-file bundle: the signature, the manifest
/// offset in the 8 bytes before it, then the header (version, count,
/// bundle id) and one record per file. `None` when there is none.
fn dotnet_bundle_read(path: &Path) -> Option<Vec<BundleEntry>> {
    let size = sniff::regular_len(path)?;
    let at = scan::find_first(path, BUNDLE_SIGNATURE)?;
    let pointer = sniff::read_at(path, at.checked_sub(8)?, 8)?;
    let manifest_offset = sniff::u64_le(&pointer, 0)?;
    // The offset is a signed 64-bit number: a negative one reads as huge.
    if manifest_offset == 0 || manifest_offset >= size {
        return None;
    }
    let manifest = sniff::read_at(path, manifest_offset, BUNDLE_MANIFEST_MAX)?;
    let major = sniff::u32_le(&manifest, 0)?;
    let count = sniff::u32_le(&manifest, 8)?;
    if major > 20 || count > 100_000 {
        return None;
    }
    let (_, mut position) = bundle_string(&manifest, 12)?;
    if major >= 2 {
        position = position.checked_add(40)?;
    }
    let mut entries = Vec::new();
    for _ in 0..count {
        let offset = sniff::u64_le(&manifest, position)?;
        let file_size = sniff::u64_le(&manifest, position + 8)?;
        position += 16;
        let compressed = if major >= 6 {
            let compressed = sniff::u64_le(&manifest, position)?;
            position += 8;
            compressed
        } else {
            0
        };
        // The file type (one byte).
        if position >= manifest.len() {
            return None;
        }
        position += 1;
        let (name, next) = bundle_string(&manifest, position)?;
        position = next;
        entries.push(BundleEntry {
            offset,
            size: file_size,
            compressed,
            path: name.replace('\\', "/"),
        });
    }
    Some(entries)
}

/// A .NET single-file bundle: the assemblies are inside the program.
fn dotnet_bundle(path: &Path, kind: &BinaryKind) -> Option<Finding> {
    let entries = dotnet_bundle_read(path)?;
    let mut finding = base(Family::Dotnet, kind.format.platform(), path.parent()?);
    finding.arch = kind.archs.clone();
    let lower: BTreeSet<String> = entries
        .iter()
        .map(|entry| entry.path.to_ascii_lowercase())
        .collect();
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.path.ends_with(".runtimeconfig.json"))
    {
        let config = clean(&entry.path);
        if entry.compressed != 0 {
            note(
                &mut finding,
                format!("{config} inside the bundle is compressed: the runtime version was not read"),
            );
        } else if let Some(raw) = sniff::read_at(path, entry.offset, entry.size.min(JSON_MAX))
            && let Some(json) = asar::parse_json(&raw)
            && let Some(version) = json.get("runtimeOptions").and_then(core_version)
        {
            push_evidence(&mut finding, &format!("{config} inside the bundle"), &version);
        }
    }
    settle_version(&mut finding);
    let avalonia = lower.contains("avalonia.dll") || lower.contains("avalonia.base.dll");
    let windows_ui = WINDOWS_UI.iter().any(|name| lower.contains(*name));
    if windows_ui && !avalonia {
        finding.verdict = Verdict::NotRehostable;
        reason(&mut finding, "dotnet-windows-desktop");
    } else {
        finding.verdict = Verdict::RehostableWithWork;
    }
    reason(&mut finding, "dotnet-single-file");
    note(
        &mut finding,
        "a single-file bundle: the assemblies are inside the program and are extracted without running it".to_owned(),
    );
    Some(finding)
}

/// A .NET Framework program: a PE program with a CLR header and no
/// `runtimeconfig.json`. Windows only.
fn dotnet_framework(place: &Place<'_>) -> Option<Finding> {
    for exe in place.exes(&[BinaryFormat::Pe]).into_iter().take(6) {
        let path = place.path.join(&exe.name);
        if !sniff::pe_info(&path).is_some_and(|info| info.clr) {
            continue;
        }
        let mut finding = base(Family::DotnetFramework, Platform::Windows, place.path);
        finding.arch = exe.kind.archs.clone();
        finding.verdict = Verdict::NotRehostable;
        reason(&mut finding, "dotnet-framework");
        if let Some(version) = scan::dotnet_framework_versions(&path, 1).into_iter().next() {
            let program = clean(&exe.name);
            push_evidence(&mut finding, &format!("target framework in {program}"), &version);
        }
        settle_version(&mut finding);
        note(
            &mut finding,
            "a .NET Framework program: Windows-only; Mono was not tested, so this is not re-hosted".to_owned(),
        );
        return Some(finding);
    }
    None
}

// --------------------------------------------------------------------- Java

/// What a jar says without inflating anything: its member names, and its
/// manifest and first class only when they are stored.
struct JarFacts {
    main_class: Option<String>,
    class_major: Option<u16>,
    windows: usize,
    linux: usize,
    swt_port: Option<&'static str>,
    has_manifest: bool,
    has_classes: bool,
}

fn manifest_main_class(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw)
        .replace("\r\n ", "")
        .replace("\n ", "");
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':')
            && key.trim().eq_ignore_ascii_case("main-class")
        {
            let value = clean(value);
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn class_major(head: &[u8]) -> Option<u16> {
    if head.starts_with(b"\xca\xfe\xba\xbe") {
        sniff::u16_be(head, 6)
    } else {
        None
    }
}

/// The minimum Java release a class-file major version needs (52 is
/// Java 8).
fn java_from_class(major: u16) -> Option<u16> {
    major.checked_sub(44).filter(|java| *java >= 1)
}

fn jar_facts(path: &Path) -> Option<JarFacts> {
    let tail = sniff::zip_tail(path)?;
    let names = sniff::zip_names(path, &tail, 20_000)?;
    let manifest = sniff::zip_read_stored(path, &tail, "META-INF/MANIFEST.MF", TEXT_MAX);
    let mut facts = JarFacts {
        main_class: manifest.as_deref().and_then(manifest_main_class),
        class_major: None,
        windows: 0,
        linux: 0,
        swt_port: None,
        has_manifest: false,
        has_classes: false,
    };
    let (mut swt, mut win32, mut gtk, mut cocoa) = (false, false, false, false);
    let mut first_class: Option<&String> = None;
    for name in &names {
        let lower = name.to_ascii_lowercase();
        let native = [".dll", ".so", ".dylib", ".jnilib"]
            .iter()
            .any(|ext| lower.ends_with(ext));
        if lower.ends_with(".dll") {
            facts.windows += 1;
        } else if lower.ends_with(".so") {
            facts.linux += 1;
        }
        if (native && lower.contains("swt-")) || name.starts_with("org/eclipse/swt/") {
            swt = true;
        }
        if name.starts_with("org/eclipse/swt/internal/") && name.contains("/win32/") {
            win32 = true;
        }
        if name.starts_with("org/eclipse/swt/internal/gtk") {
            gtk = true;
        }
        if name.starts_with("org/eclipse/swt/internal/cocoa") {
            cocoa = true;
        }
        if name == "META-INF/MANIFEST.MF" {
            facts.has_manifest = true;
        }
        if name.ends_with(".class") && !name.starts_with("META-INF/") && !name.contains("module-info") {
            facts.has_classes = true;
            if first_class.is_none() {
                first_class = Some(name);
            }
        }
    }
    if swt {
        facts.swt_port = Some(if win32 {
            "win32"
        } else if gtk {
            "gtk"
        } else if cocoa {
            "cocoa"
        } else {
            "unknown"
        });
    }
    let main_path = facts
        .main_class
        .as_ref()
        .map(|class| format!("{}.class", class.replace('.', "/")));
    for candidate in main_path.iter().chain(first_class) {
        if names.contains(candidate)
            && let Some(head) = sniff::zip_read_stored(path, &tail, candidate, 8)
        {
            facts.class_major = class_major(&head);
            break;
        }
    }
    Some(facts)
}

/// The version and modules of a Java runtime image's `release` file.
struct Release {
    version: Option<String>,
    modules: Vec<String>,
}

fn quoted_value(text: &str, key: &str) -> Option<String> {
    let start = text.find(key)? + key.len();
    let rest = text.get(start..)?;
    let value = rest.get(..rest.find('"')?)?;
    (!value.is_empty()).then(|| value.to_owned())
}

fn java_release(runtime: &Path) -> Option<Release> {
    if !is_real_dir(runtime) {
        return None;
    }
    let text = read_text(&runtime.join("release"), 8192)?;
    Some(Release {
        version: quoted_value(&text, "JAVA_VERSION=\"").map(|version| clean(&version)),
        modules: quoted_value(&text, "MODULES=\"")
            .map(|modules| modules.split_whitespace().map(clean).take(512).collect())
            .unwrap_or_default(),
    })
}

/// What [`describe_jars`] needs besides the finding.
struct JvmFacts {
    app: PathBuf,
    mainjar: Option<String>,
    mainmodule: bool,
    modules: Vec<String>,
}

/// The `app.mainclass`, `app.mainjar` and `app.mainmodule` of a jpackage
/// configuration.
struct Cfg {
    mainclass: Option<String>,
    mainjar: Option<String>,
    mainmodule: Option<String>,
}

fn parse_cfg(path: &Path) -> Cfg {
    let mut cfg = Cfg {
        mainclass: None,
        mainjar: None,
        mainmodule: None,
    };
    let text = read_text(path, TEXT_MAX).unwrap_or_default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = Some(clean(value));
        match key {
            "app.mainclass" => cfg.mainclass = value,
            "app.mainjar" => cfg.mainjar = value,
            "app.mainmodule" => cfg.mainmodule = value,
            _ => {}
        }
    }
    cfg
}

/// A jpackage image: `app/*.cfg` (or `Contents/app/`, `lib/app/`), the
/// runtime's `release` file for the version.
fn jpackage(place: &Place<'_>) -> Option<(Finding, JvmFacts)> {
    let app = ["app", "Contents/app", "lib/app"]
        .iter()
        .filter_map(|candidate| below(place.path, candidate))
        .find(|path| is_real_dir(path) && list_names(path).iter().any(|name| name.ends_with(".cfg")))?;
    let cfg_name = list_names(&app)
        .into_iter()
        .find(|name| name.ends_with(".cfg"))?;
    let cfg = parse_cfg(&app.join(&cfg_name));
    if cfg.mainclass.is_none() && cfg.mainjar.is_none() && cfg.mainmodule.is_none() {
        return None;
    }
    let exes = place.exes(&[BinaryFormat::Pe, BinaryFormat::Elf, BinaryFormat::MachO]);
    let platform = if app
        .components()
        .any(|component| component.as_os_str() == "Contents")
    {
        Platform::Macos
    } else {
        exes.first()
            .map_or(Platform::Unknown, |exe| exe.kind.format.platform())
    };
    let mut finding = base(Family::Jvm, platform, place.path);
    if let Some(exe) = exes.first() {
        finding.arch = exe.kind.archs.clone();
    }
    let mut modules = Vec::new();
    for runtime in ["runtime", "Contents/runtime/Contents/Home", "lib/runtime"] {
        if let Some(release) = below(place.path, runtime).and_then(|path| java_release(&path)) {
            if let Some(version) = &release.version {
                push_evidence(&mut finding, &format!("{runtime}/release"), version);
            }
            modules = release.modules;
            break;
        }
    }
    settle_version(&mut finding);
    let facts = JvmFacts {
        app,
        mainjar: cfg.mainjar,
        mainmodule: cfg.mainmodule.is_some(),
        modules,
    };
    Some((finding, facts))
}

/// Jars beside a launcher and a bundled Java runtime. A jar counts when
/// its stored manifest names a main class; when every manifest is
/// compressed, a bundled runtime's `release` file is what recognises the
/// package.
fn jars_beside(place: &Place<'_>) -> Option<(Finding, JvmFacts)> {
    let jars: Vec<&String> = place
        .names
        .iter()
        .filter(|name| name.to_ascii_lowercase().ends_with(".jar"))
        .collect();
    if jars.is_empty() {
        return None;
    }
    let runtime_beside = place
        .dirs
        .iter()
        .any(|name| ["jre", "runtime", "jdk"].contains(&name.to_ascii_lowercase().as_str()));
    if place.depth > 3 && !runtime_beside {
        return None;
    }
    let mut with_main: Vec<(u64, &String)> = Vec::new();
    for name in jars.iter().take(40) {
        let path = place.path.join(name);
        if jar_facts(&path).is_some_and(|facts| facts.main_class.is_some()) {
            with_main.push((sniff::regular_len(&path).unwrap_or(0), *name));
        }
    }
    let release = ["jre", "runtime", "jdk"].iter().find_map(|runtime| {
        java_release(&place.path.join(runtime)).map(|release| (*runtime, release))
    });
    if with_main.is_empty() && release.is_none() {
        return None;
    }
    let mainjar = with_main.iter().max().map(|(_, name)| (*name).clone()).or_else(|| {
        jars.iter()
            .map(|name| (sniff::regular_len(&place.path.join(name)).unwrap_or(0), *name))
            .max()
            .map(|(_, name)| name.clone())
    });
    let exes = place.exes(&[BinaryFormat::Pe, BinaryFormat::Elf, BinaryFormat::MachO]);
    let platform = exes
        .first()
        .map_or(Platform::Any, |exe| exe.kind.format.platform());
    let mut finding = base(Family::Jvm, platform, place.path);
    if let Some(exe) = exes.first() {
        finding.arch = exe.kind.archs.clone();
    }
    let mut modules = Vec::new();
    if let Some((runtime, release)) = release {
        if let Some(version) = &release.version {
            push_evidence(&mut finding, &format!("{runtime}/release"), version);
        }
        modules = release.modules;
    }
    settle_version(&mut finding);
    if with_main.is_empty() {
        note(
            &mut finding,
            "the jars' manifests are compressed, so the main class was not read: the bundled Java runtime identifies the package"
                .to_owned(),
        );
    }
    let facts = JvmFacts {
        app: place.path.to_path_buf(),
        mainjar: mainjar.map(|name| clean(&name)),
        mainmodule: false,
        modules,
    };
    Some((finding, facts))
}

/// `javafx[-.].*-(win|mac|linux)(-[a-z0-9]+)?\.jar`, the whole name.
fn is_javafx_platform_jar(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("javafx")
        .and_then(|rest| rest.strip_prefix(['-', '.']))
    else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".jar") else {
        return false;
    };
    let ends_with_platform = |text: &str| {
        ["-win", "-mac", "-linux"]
            .iter()
            .any(|platform| text.ends_with(platform))
    };
    if ends_with_platform(stem) {
        return true;
    }
    stem.rsplit_once('-').is_some_and(|(head, tail)| {
        !tail.is_empty()
            && tail
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && ends_with_platform(head)
    })
}

/// SWT ports, JavaFX platform jars, JNI libraries and the class-file
/// version of the jars below the application directory (at most 400).
fn describe_jars(finding: &mut Finding, facts: &JvmFacts) {
    let mut jars = 0usize;
    let mut windows = 0usize;
    let mut linux = 0usize;
    let mut swt_port: Option<&'static str> = None;
    let mut javafx_jars: Vec<String> = Vec::new();
    let mut major: Option<u16> = None;
    walk_files(&facts.app, None, &mut |rel, path, _size| {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        if !name.to_ascii_lowercase().ends_with(".jar") {
            return true;
        }
        jars += 1;
        if jars > 400 {
            return false;
        }
        if is_javafx_platform_jar(name) {
            javafx_jars.push(clean(name));
        }
        let Some(jar) = jar_facts(path) else {
            return true;
        };
        windows += jar.windows;
        linux += jar.linux;
        if jar.swt_port.is_some() && (swt_port.is_none() || swt_port == Some("unknown")) {
            swt_port = jar.swt_port;
        }
        if facts.mainjar.as_deref() == Some(name) || major.is_none() {
            major = jar.class_major.or(major);
        }
        true
    });
    if finding.version.is_none()
        && let Some(java) = major.and_then(java_from_class)
    {
        push_evidence(
            finding,
            "class-file version of the main jar (a minimum)",
            &java.to_string(),
        );
        settle_version(finding);
    }
    if let Some(port) = swt_port
        && port != "gtk"
    {
        reason(finding, "swt-platform-port");
        with_work(finding);
        note(
            finding,
            format!("uses SWT's {port} port: the GTK port of the same SWT version has to replace it"),
        );
    }
    if !javafx_jars.is_empty() && !javafx_jars.iter().any(|name| name.contains("-linux")) {
        reason(finding, "javafx-platform-jars");
        with_work(finding);
        note(
            finding,
            "carries JavaFX jars for another platform: the Linux jars of the same version replace them".to_owned(),
        );
    }
    if facts.modules.iter().any(|module| module.starts_with("javafx.")) {
        reason(finding, "javafx-in-runtime-image");
        with_work(finding);
        note(
            finding,
            "JavaFX is linked into the bundled runtime image: the Linux JavaFX modules of the same version are needed"
                .to_owned(),
        );
    }
    if facts.mainmodule && jars == 0 {
        reason(finding, "application-in-runtime-image");
        with_work(finding);
        note(
            finding,
            "the application's classes are inside the runtime image (jlink): they have to be extracted with jimage"
                .to_owned(),
        );
    }
    if windows > 0 && linux == 0 && finding.platform == Platform::Windows {
        reason(finding, "jni-libraries-windows-only");
        with_work(finding);
    }
}

/// A Java program: a jpackage image, or jars beside a launcher.
fn jvm_dir(place: &Place<'_>) -> Option<Finding> {
    let (mut finding, facts) = jpackage(place).or_else(|| jars_beside(place))?;
    describe_jars(&mut finding, &facts);
    Some(finding)
}

/// A program with a jar appended (launch4j, JSmooth, or a jar made
/// executable).
fn launch4j(path: &Path) -> Option<Finding> {
    let facts = jar_facts(path)?;
    if facts.main_class.is_none()
        && facts.class_major.is_none()
        && !(facts.has_manifest && facts.has_classes)
    {
        return None;
    }
    let mut finding = base(Family::Jvm, Platform::Windows, path.parent()?);
    if let Some(kind) = sniff::binary_kind(path) {
        finding.arch = kind.archs;
    }
    let wide: Vec<u8> = "Launch4j"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let wrapper = if scan::contains(path, b"launch4j") || scan::contains(path, &wide) {
        "launch4j"
    } else {
        "a jar appended to the program"
    };
    note(&mut finding, format!("a Java program carried by {wrapper}"));
    if let Some(java) = facts.class_major.and_then(java_from_class) {
        push_evidence(
            &mut finding,
            "class-file version of the main class (a minimum)",
            &java.to_string(),
        );
        settle_version(&mut finding);
    }
    if facts.main_class.is_none() {
        note(
            &mut finding,
            "the jar's manifest is compressed: its main class was not read".to_owned(),
        );
    }
    if let Some(port) = facts.swt_port
        && port != "gtk"
    {
        reason(&mut finding, "swt-platform-port");
        with_work(&mut finding);
        note(
            &mut finding,
            format!("uses SWT's {port} port: the GTK port of the same SWT version has to replace it"),
        );
    }
    Some(finding)
}

// --------------------------------------------------------------------- LÖVE

/// `\.version\s*=\s*['"]([0-9.]+)['"]` in a `conf.lua`.
fn love_conf_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = sniff::find_bytes(bytes, b".version", from) {
        from = at + 1;
        if let Some(version) = quoted_version_after(bytes, at + b".version".len()) {
            return Some(version);
        }
    }
    None
}

fn quoted_version_after(bytes: &[u8], at: usize) -> Option<String> {
    let at = skip_spaces(bytes, at);
    let at = expect_byte(bytes, at, b'=')?;
    let at = skip_spaces(bytes, at);
    if !matches!(bytes.get(at), Some(b'\'' | b'"')) {
        return None;
    }
    let start = at + 1;
    let length = bytes
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take(33)
        .take_while(|byte| byte.is_ascii_digit() || **byte == b'.')
        .count();
    if length == 0 || length > 32 {
        return None;
    }
    let end = start + length;
    if !matches!(bytes.get(end), Some(b'\'' | b'"')) {
        return None;
    }
    Some(String::from_utf8_lossy(bytes.get(start..end)?).into_owned())
}

/// A fused LÖVE game (`love.exe` with the game's zip appended) or a
/// `.love`: a zip archive holding `main.lua`.
fn love_file(path: &Path) -> Option<Finding> {
    let name = file_name(path);
    let is_love = name.to_ascii_lowercase().ends_with(".love");
    let tail = sniff::zip_tail(path)?;
    let names = sniff::zip_names(path, &tail, 100_000)?;
    if !names.iter().any(|member| member == "main.lua") {
        return None;
    }
    let conf = sniff::zip_read_stored(path, &tail, "conf.lua", TEXT_MAX)
        .and_then(|raw| love_conf_version(&String::from_utf8_lossy(&raw)));
    let kind = sniff::binary_kind(path);
    let directory = path.parent()?;
    let platform = kind
        .as_ref()
        .map_or(Platform::Any, |kind| kind.format.platform());
    let mut finding = base(Family::Love, platform, directory);
    if let Some(kind) = &kind {
        finding.arch = kind.archs.clone();
    }
    for candidate in ["love.dll", name.as_str()] {
        let binary = directory.join(candidate);
        if sniff::binary_kind(&binary).is_none() {
            continue;
        }
        let strings = scan::pe_version_strings(&binary);
        if let Some(value) = strings
            .get("ProductVersion")
            .or_else(|| strings.get("FileVersion"))
            && starts_with_version(value)
        {
            push_evidence(
                &mut finding,
                &format!("version resource of {candidate}"),
                &dotted(value),
            );
        }
        for value in scan::love_versions(&binary, 2) {
            push_evidence(&mut finding, &format!("version string in {candidate}"), &value);
        }
    }
    if let Some(conf) = &conf {
        push_evidence(
            &mut finding,
            "t.version in the game's conf.lua (what the game was written for)",
            conf,
        );
    }
    if conf.is_none() && names.iter().any(|member| member == "conf.lua") {
        note(
            &mut finding,
            "conf.lua is compressed in the archive: the LÖVE version the game was written for was not read".to_owned(),
        );
    }
    if !is_love && tail.start > 0 {
        note(
            &mut finding,
            "the game is fused to the program: its archive is appended to love.exe".to_owned(),
        );
    }
    settle_version(&mut finding);
    version_unknown(&mut finding);
    Some(finding)
}

/// A LÖVE game in a macOS bundle: `love.framework` and a `.love` in
/// `Contents/Resources`.
fn love_app(bundle: &Path) -> Option<Finding> {
    let framework = ["Contents/Frameworks/love.framework", "Contents/Frameworks/Lua.framework"]
        .iter()
        .filter_map(|candidate| below(bundle, candidate))
        .find(|path| is_real_dir(path))?;
    let resources = below(bundle, "Contents/Resources").filter(|path| is_real_dir(path))?;
    let game = list_names(&resources)
        .into_iter()
        .find(|name| name.to_ascii_lowercase().ends_with(".love"))?;
    let mut finding = love_file(&resources.join(game))?;
    finding.platform = Platform::Macos;
    finding.app_dir = bundle.to_path_buf();
    for candidate in ["Versions/A/Resources/Info.plist", "Resources/Info.plist"] {
        let Some(mut info) = below(&framework, candidate).map(|path| plist_strings(&path)) else {
            continue;
        };
        let version = info
            .remove("CFBundleShortVersionString")
            .or_else(|| info.remove("CFBundleVersion"));
        if let Some(version) = version {
            let value = clean(&version);
            finding.version_evidence.insert(
                0,
                Evidence {
                    from: "love.framework Info.plist".to_owned(),
                    value: value.clone(),
                },
            );
            finding.version = Some(value);
            if let Some(index) = finding
                .reasons
                .iter()
                .position(|code| code == "runtime-version-unknown")
            {
                finding.reasons.remove(index);
                if finding.reasons.is_empty() {
                    finding.verdict = Verdict::Rehostable;
                }
            }
            break;
        }
    }
    Some(finding)
}

// -------------------------------------------------------------- PyInstaller

/// What PyInstaller's cookie says.
struct PyArchive {
    python: String,
    pylib: String,
}

/// The cookie of a PyInstaller archive: the last occurrence of its magic
/// in the last 8 MiB of the program (the archive is appended; only a
/// signature may follow it), then the archive length, the table of
/// contents, the Python version and the Python library's name.
fn pyinstaller_read(path: &Path) -> Option<PyArchive> {
    let size = sniff::regular_len(path)?;
    let window = size.min(PYI_TAIL);
    let base_offset = size - window;
    let tail = sniff::read_at(path, base_offset, window)?;
    let at = sniff::rfind_bytes(&tail, PYI_MAGIC)?;
    let cookie = tail.get(at..at.checked_add(88)?)?;
    let length = u64::from(sniff::u32_be(cookie, 8)?);
    let python = sniff::u32_be(cookie, 20)?;
    let cookie_end = base_offset.checked_add(u64::try_from(at).ok()?)?.checked_add(88)?;
    // Where the archive starts (`cookie_end - length`) must lie inside the
    // file.
    if cookie_end < length {
        return None;
    }
    let pylib: String = cookie
        .get(24..88)?
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| char::from(byte))
        .collect();
    let (major, minor) = (python / 100, python % 100);
    Some(PyArchive {
        python: format!("{major}.{minor}"),
        pylib: clean(&pylib),
    })
}

/// A frozen Python program (PyInstaller): its bytecode is portable within
/// one Python minor version.
fn pyinstaller(path: &Path, kind: &BinaryKind) -> Option<Finding> {
    let archive = pyinstaller_read(path)?;
    let mut finding = base(Family::Pyinstaller, kind.format.platform(), path.parent()?);
    finding.arch = kind.archs.clone();
    let pylib = &archive.pylib;
    push_evidence(&mut finding, &format!("CArchive cookie ({pylib})"), &archive.python);
    settle_version(&mut finding);
    finding.verdict = Verdict::RehostableWithWork;
    reason(&mut finding, "python-frozen");
    note(
        &mut finding,
        "frozen Python: the bytecode is portable within one Python minor version; every extension module needs its Linux build"
            .to_owned(),
    );
    Some(finding)
}

// ------------------------------------------------------------------- native

fn native_words(toolkit: &str) -> String {
    let words = match toolkit {
        "qt" => "a native Qt program",
        "flutter" => "a Flutter program (compiled ahead of time for one platform)",
        "tauri" => "a Tauri program (a native Rust host around the system web view)",
        "unity" => "a Unity game (the player and IL2CPP or Mono code are built for one platform)",
        "unreal" => "an Unreal Engine game",
        "cef" => "a native program that embeds Chromium (CEF): the host is native code",
        "webview2" => "a native program around Microsoft's WebView2: the host is native code",
        "gamemaker" => "a GameMaker game",
        "unknown" => "a native program with no portable runtime found",
        other => return format!("a native program ({other}): not re-hostable"),
    };
    format!("{words}: not re-hostable")
}

/// No portable runtime: the toolkits the package's file names mark, and
/// the first program seen (the shallowest, then the largest). Tauri is
/// recognised by `tauri` and `wry` in that program.
fn native_finding(root: &Path, all_names: &BTreeSet<String>, programs: &[Program]) -> Option<Finding> {
    let mut toolkits: Vec<&str> = Vec::new();
    for (toolkit, markers) in NATIVE_MARKERS {
        let mut hits: Vec<&str> = markers
            .iter()
            .copied()
            .filter(|marker| all_names.contains(*marker))
            .collect();
        if toolkit == "unreal" {
            hits.retain(|marker| *marker != "engine");
            if hits.is_empty()
                && all_names.contains("engine")
                && all_names.iter().any(|name| name.ends_with("-win64-shipping.exe"))
            {
                hits.push("engine");
            }
        }
        if toolkit == "flutter" && hits == ["app.framework"] {
            hits.clear();
        }
        if !hits.is_empty() {
            toolkits.push(toolkit);
        }
    }
    let program = programs.first();
    if let Some(program) = program
        && (toolkits.contains(&"webview2") || toolkits.is_empty())
        && scan::contains(&program.path, b"tauri")
        && scan::contains(&program.path, b"wry")
    {
        toolkits.insert(0, "tauri");
    }
    if toolkits.is_empty() && program.is_none() {
        return None;
    }
    let toolkit = toolkits.first().copied().unwrap_or("unknown");
    let platform = program.map_or(Platform::Unknown, |program| program.kind.format.platform());
    let mut finding = base(Family::Native, platform, root);
    finding.toolkit = Some(toolkit.to_owned());
    if let Some(program) = program {
        finding.arch = program.kind.archs.clone();
    }
    finding.verdict = Verdict::NotRehostable;
    reason(&mut finding, "native-program");
    note(&mut finding, native_words(toolkit));
    Some(finding)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sniff::tests::{Scratch, macho_image, pe_image, stored_zip};

    const PE_X64: u16 = 0x8664;

    fn user_agent(version: &str) -> Vec<u8> {
        format!("\0Chrome/138.0.7204.100 Electron/{version}\0").into_bytes()
    }

    fn make_dir(path: &Path) {
        fs::create_dir_all(path).expect("directory");
    }

    /// A PE32 DLL with a CLR header: a managed assembly.
    fn managed_image() -> Vec<u8> {
        let mut image = vec![0u8; 0x40];
        image[0] = b'M';
        image[1] = b'Z';
        image[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        image.extend_from_slice(b"PE\0\0");
        image.extend_from_slice(&0x14cu16.to_le_bytes());
        image.extend_from_slice(&[0u8; 14]);
        image.extend_from_slice(&224u16.to_le_bytes());
        image.extend_from_slice(&0x2102u16.to_le_bytes());
        let mut optional = vec![0u8; 224];
        optional[..2].copy_from_slice(&0x10bu16.to_le_bytes());
        optional[68..70].copy_from_slice(&3u16.to_le_bytes());
        optional[208..212].copy_from_slice(&0x2008u32.to_le_bytes());
        optional[212..216].copy_from_slice(&0x48u32.to_le_bytes());
        image.extend_from_slice(&optional);
        image.resize(5000, 0);
        image
    }

    fn texts(values: &[String]) -> Vec<&str> {
        values.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_windows_electron_package_is_read_in_full() {
        let scratch = Scratch::new("detect-electron-windows");
        let mut extra = user_agent("43.7.7");
        extra.extend_from_slice(scan::FUSE_SENTINEL);
        extra.extend_from_slice(b"\x01\x0810101000");
        scratch.write("Foo/Foo.exe", &pe_image(PE_X64, &extra, 8192));
        let resources = scratch.path.join("Foo/resources");
        make_dir(&resources);
        let archive = resources.join("app.asar");
        let files: [(&str, &[u8]); 2] = [
            (
                "package.json",
                br#"{"name":"foo","productName":"Foo","version":"1.2.3","main":"main.js"}"#,
            ),
            (
                "node_modules/bar/package.json",
                br#"{"name":"bar","version":"2.0.0","os":["win32"]}"#,
            ),
        ];
        Asar::create(&archive, &files).expect("create");
        let mut asar = Asar::open(&archive).expect("open");
        asar.put_unpacked(
            "node_modules/bar/build/Release/bar.node",
            &pe_image(PE_X64, &[], 1024),
            false,
        )
        .expect("put");
        asar.save().expect("save");
        scratch.write("Foo/resources/bin/helper.exe", &pe_image(PE_X64, &[], 5000));

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = primary(&findings).expect("finding");
        assert_eq!(finding.family, Family::Electron);
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.app_dir, scratch.path.join("Foo"));
        assert_eq!(finding.depth, 1);
        assert_eq!(texts(&finding.arch), ["x86_64"]);
        assert_eq!(finding.version.as_deref(), Some("43.7.7"));
        assert_eq!(finding.version_evidence[0].from, "user-agent string in Foo.exe");
        assert_eq!(finding.verdict, Verdict::RehostableWithWork);
        assert_eq!(
            texts(&finding.reasons),
            ["native-modules", "platform-helper-binaries"]
        );

        let payload = finding.electron.as_ref().expect("payload");
        assert_eq!(payload.kind, PayloadKind::Asar);
        assert_eq!(payload.program.as_deref(), Some("Foo.exe"));
        assert_eq!(payload.name.as_deref(), Some("Foo"));
        assert_eq!(payload.package_name.as_deref(), Some("foo"));
        assert_eq!(payload.app_version.as_deref(), Some("1.2.3"));
        assert_eq!(payload.main.as_deref(), Some("main.js"));
        assert_eq!(payload.chromium.as_deref(), Some("138.0.7204.100"));
        assert_eq!(payload.modules_root, archive);
        assert_eq!(payload.resources, resources);
        assert_eq!(payload.updater, None);
        let fuses = payload.fuses.as_ref().expect("fuses");
        assert_eq!(fuses.len(), 8);
        assert_eq!(fuses.get("RunAsNode").map(String::as_str), Some("1"));
        assert_eq!(fuses.get("EnableCookieEncryption").map(String::as_str), Some("0"));
        assert_eq!(fuses.get("OnlyLoadAppFromAsar").map(String::as_str), Some("0"));

        assert_eq!(finding.natives.len(), 1);
        let module = &finding.natives[0];
        assert_eq!(module.path, "node_modules/bar/build/Release/bar.node");
        assert_eq!(module.location, Location::Unpacked);
        assert_eq!(module.kind.format, BinaryFormat::Pe);
        assert_eq!(module.size, 1024);
        assert_eq!(module.identity().as_deref(), Some("bar@2.0.0"));
        assert_eq!(module.package_dir.as_deref(), Some("node_modules/bar"));
        assert_eq!(texts(&module.os), ["win32"]);
        assert!(module.declared_not_for_linux());
        assert_eq!(finding.foreign_natives().count(), 1);

        assert_eq!(finding.sidecars.len(), 1);
        assert_eq!(finding.sidecars[0].path, "bin/helper.exe");
        assert!(finding.summary().starts_with("electron 43.7.7, built for windows"));
    }

    fn plist(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut text = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n",
        );
        for (key, value) in pairs {
            text.push_str(&format!("  <key>{key}</key>\n  <string>{value}</string>\n"));
        }
        text.push_str("</dict>\n</plist>\n");
        text.into_bytes()
    }

    const FRAMEWORK: &str = "Foo.app/Contents/Frameworks/Electron Framework.framework";

    #[test]
    fn a_macos_electron_bundle_takes_its_version_from_the_framework_plist() {
        let scratch = Scratch::new("detect-electron-macos");
        scratch.write(
            "Foo.app/Contents/Info.plist",
            &plist(&[
                ("CFBundleExecutable", "Foo"),
                ("CFBundleIdentifier", "com.example.foo"),
            ]),
        );
        scratch.write(
            &format!("{FRAMEWORK}/Versions/A/Resources/Info.plist"),
            &plist(&[("CFBundleVersion", "42.1.0")]),
        );
        scratch.write(
            &format!("{FRAMEWORK}/Versions/A/Electron Framework"),
            &macho_image(0x0100_000c, 8192),
        );
        let resources = scratch.path.join("Foo.app/Contents/Resources");
        make_dir(&resources);
        let files: [(&str, &[u8]); 1] = [("package.json", br#"{"name":"foo","version":"2.0.0"}"#)];
        Asar::create(&resources.join("app.asar"), &files).expect("create");
        scratch.write("Foo.app/Contents/Resources/app-update.yml", b"provider: github\n");

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Electron);
        assert_eq!(finding.platform, Platform::Macos);
        assert_eq!(finding.app_dir, scratch.path.join("Foo.app"));
        assert_eq!(finding.version.as_deref(), Some("42.1.0"));
        assert_eq!(finding.version_evidence.len(), 1);
        assert_eq!(finding.version_evidence[0].from, "Electron Framework Info.plist");
        assert_eq!(texts(&finding.arch), ["arm64"]);
        assert_eq!(finding.verdict, Verdict::Rehostable);
        assert!(finding.reasons.is_empty());
        let payload = finding.electron.as_ref().expect("payload");
        assert_eq!(payload.program.as_deref(), Some("Foo"));
        assert_eq!(payload.bundle_id.as_deref(), Some("com.example.foo"));
        assert_eq!(payload.updater.as_deref(), Some("electron-updater"));
        assert_eq!(payload.app_version.as_deref(), Some("2.0.0"));
        assert_eq!(payload.name.as_deref(), Some("foo"));
        assert_eq!(payload.fuses, None);
        assert_eq!(payload.chromium, None);
    }

    #[test]
    fn a_universal_macos_build_keeps_its_modules_in_the_x64_archive() {
        let scratch = Scratch::new("detect-electron-universal");
        scratch.write(&format!("{FRAMEWORK}/Versions/A/Resources/README"), b"no plist");
        let resources = scratch.path.join("Foo.app/Contents/Resources");
        make_dir(&resources);
        let small: [(&str, &[u8]); 1] = [("package.json", br#"{"name":"foo"}"#)];
        Asar::create(&resources.join("app.asar"), &small).expect("create");
        let module = macho_image(0x0100_0007, 1024);
        let split: [(&str, &[u8]); 3] = [
            ("package.json", br#"{"name":"foo"}"#),
            (
                "node_modules/baz/package.json",
                br#"{"name":"baz","version":"1.4.0","os":["darwin"]}"#,
            ),
            ("node_modules/baz/build/Release/baz.node", &module),
        ];
        Asar::create(&resources.join("app-x64.asar"), &split).expect("create");

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.version, None);
        assert_eq!(
            texts(&finding.reasons),
            ["runtime-version-unknown", "native-modules"]
        );
        let payload = finding.electron.as_ref().expect("payload");
        assert_eq!(payload.modules_root, resources.join("app-x64.asar"));
        assert_eq!(finding.natives.len(), 1);
        let native = &finding.natives[0];
        assert_eq!(native.location, Location::Packed);
        assert_eq!(native.kind.format, BinaryFormat::MachO);
        assert_eq!(texts(&native.kind.archs), ["x86_64"]);
        assert_eq!(native.identity().as_deref(), Some("baz@1.4.0"));
    }

    #[test]
    fn a_directory_payload_reads_packages_through_its_inner_archive() {
        let scratch = Scratch::new("detect-electron-inner");
        scratch.write("Code/Code.exe", &pe_image(PE_X64, &user_agent("37.2.0"), 8192));
        scratch.write(
            "Code/resources/app/package.json",
            br#"{"name":"code-oss-dev","version":"1.103.0","main":"./out/main.js"}"#,
        );
        let app = scratch.path.join("Code/resources/app");
        let inner: [(&str, &[u8]); 1] = [(
            "qux/package.json",
            br#"{"name":"qux","version":"3.0.0"}"#,
        )];
        Asar::create(&app.join("node_modules.asar"), &inner).expect("create");
        scratch.write(
            "Code/resources/app/node_modules.asar.unpacked/qux/build/Release/qux.node",
            &pe_image(PE_X64, &[], 2048),
        );

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.version.as_deref(), Some("37.2.0"));
        let payload = finding.electron.as_ref().expect("payload");
        assert_eq!(payload.kind, PayloadKind::Directory);
        assert_eq!(payload.modules_root, app);
        assert_eq!(payload.main.as_deref(), Some("./out/main.js"));
        assert_eq!(finding.natives.len(), 1);
        let native = &finding.natives[0];
        assert_eq!(
            native.path,
            "node_modules.asar.unpacked/qux/build/Release/qux.node"
        );
        assert_eq!(native.location, Location::Directory);
        assert_eq!(native.identity().as_deref(), Some("qux@3.0.0"));
        assert_eq!(
            native.package_dir.as_deref(),
            Some("node_modules.asar.unpacked/qux")
        );
        assert!(finding.sidecars.is_empty());
    }

    #[test]
    fn a_vendor_electron_build_is_named() {
        let scratch = Scratch::new("detect-electron-vendor");
        let mut program = user_agent("28.3.3+wvcus");
        program.extend_from_slice(b"Chrome/120.0.6099.291 \0");
        scratch.write("App/App.exe", &pe_image(PE_X64, &program, 8192));
        scratch.write("App/resources/app/package.json", br#"{"name":"app"}"#);
        scratch.write("App/version", b"v28.3.3\n");

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.version.as_deref(), Some("28.3.3+wvcus"));
        assert_eq!(finding.version_evidence.len(), 2);
        assert_eq!(finding.version_evidence[1].from, "version file");
        assert_eq!(finding.version_evidence[1].value, "28.3.3");
        assert_eq!(texts(&finding.reasons), ["vendor-electron-build"]);
        assert_eq!(finding.verdict, Verdict::Rehostable);
    }

    #[test]
    fn a_qt_program_is_native_and_refused() {
        let scratch = Scratch::new("detect-qt");
        scratch.write("KeePassXC.exe", &pe_image(PE_X64, &[], 8192));
        scratch.write("Qt6Core.dll", &pe_image(PE_X64, &[], 5000));

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Native);
        assert_eq!(finding.toolkit.as_deref(), Some("qt"));
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.verdict, Verdict::NotRehostable);
        assert_eq!(texts(&finding.reasons), ["native-program"]);
    }

    #[test]
    fn an_empty_directory_has_no_findings() {
        let scratch = Scratch::new("detect-empty");
        scratch.write("readme.txt", b"hello");
        assert!(detect(&scratch.path).is_empty());
        assert_eq!(primary(&[]), None);
    }

    fn pack_header(format: u32, version: [u32; 3]) -> Vec<u8> {
        let mut pack = b"GDPC".to_vec();
        for value in [format, version[0], version[1], version[2]] {
            pack.extend_from_slice(&value.to_le_bytes());
        }
        pack
    }

    #[test]
    fn a_godot_pack_beside_the_program_gives_the_engine_version() {
        let scratch = Scratch::new("detect-godot");
        scratch.write("game.exe", &pe_image(PE_X64, &[], 8192));
        let mut pack = pack_header(1, [3, 5, 2]);
        pack.extend_from_slice(&[0u8; 64]);
        let files = ["res://project.binary", "res://addons/sound/sound.gdns"];
        pack.extend_from_slice(&u32::try_from(files.len()).expect("count").to_le_bytes());
        for file in files {
            pack.extend_from_slice(&u32::try_from(file.len()).expect("length").to_le_bytes());
            pack.extend_from_slice(file.as_bytes());
            pack.extend_from_slice(&[0u8; 32]);
        }
        scratch.write("game.pck", &pack);

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Godot);
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.version.as_deref(), Some("3.5.2"));
        assert_eq!(
            finding.version_evidence[0].from,
            "pack header (standalone) in game.pck"
        );
        assert_eq!(texts(&finding.reasons), ["godot-native-extension"]);
        assert_eq!(finding.verdict, Verdict::RehostableWithWork);
    }

    #[test]
    fn an_encrypted_godot_pack_appended_to_the_program_is_refused() {
        let scratch = Scratch::new("detect-godot-appended");
        let mut pack = pack_header(2, [4, 2, 1]);
        pack.extend_from_slice(&1u32.to_le_bytes());
        pack.extend_from_slice(&0u64.to_le_bytes());
        pack.extend_from_slice(&[0u8; 64]);
        pack.extend_from_slice(&0u32.to_le_bytes());
        let mut program = pe_image(PE_X64, &[], 8192);
        program.extend_from_slice(&pack);
        program.extend_from_slice(&u64::try_from(pack.len()).expect("length").to_le_bytes());
        program.extend_from_slice(b"GDPC");
        scratch.write("game.exe", &program);

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Godot);
        assert_eq!(finding.version.as_deref(), Some("4.2.1"));
        assert_eq!(
            finding.version_evidence[0].from,
            "pack header (appended) in game.exe"
        );
        assert_eq!(texts(&finding.reasons), ["pack-encrypted"]);
        assert_eq!(finding.verdict, Verdict::NotRehostable);
    }

    #[test]
    fn a_fused_love_game_is_found_with_its_conf_version() {
        let scratch = Scratch::new("detect-love");
        let mut program = pe_image(PE_X64, &[], 5000);
        let members: [(&str, &[u8]); 2] = [
            ("main.lua", b"function love.draw() end\n"),
            ("conf.lua", b"function love.conf(t)\n  t.version = \"11.5\"\nend\n"),
        ];
        program.extend_from_slice(&stored_zip(&members));
        scratch.write("game.exe", &program);

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Love);
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.version.as_deref(), Some("11.5"));
        assert_eq!(
            finding.version_evidence[0].from,
            "t.version in the game's conf.lua (what the game was written for)"
        );
        assert_eq!(finding.verdict, Verdict::Rehostable);
        assert!(finding.notes.iter().any(|text| text.contains("fused")));
    }

    #[test]
    fn a_pyinstaller_cookie_gives_the_python_version() {
        let scratch = Scratch::new("detect-pyinstaller");
        let mut archive = vec![b'z'; 100];
        let name = b"pyimod01\0\0";
        let entry_size = u32::try_from(18 + name.len()).expect("size");
        let mut toc = Vec::new();
        toc.extend_from_slice(&entry_size.to_be_bytes());
        toc.extend_from_slice(&0u32.to_be_bytes());
        toc.extend_from_slice(&100u32.to_be_bytes());
        toc.extend_from_slice(&100u32.to_be_bytes());
        toc.extend_from_slice(&[0, b'z']);
        toc.extend_from_slice(name);
        archive.extend_from_slice(&toc);
        let length = u32::try_from(archive.len() + 88).expect("length");
        archive.extend_from_slice(PYI_MAGIC);
        archive.extend_from_slice(&length.to_be_bytes());
        archive.extend_from_slice(&100u32.to_be_bytes());
        archive.extend_from_slice(&u32::try_from(toc.len()).expect("toc").to_be_bytes());
        archive.extend_from_slice(&312u32.to_be_bytes());
        let mut pylib = b"python312.dll".to_vec();
        pylib.resize(64, 0);
        archive.extend_from_slice(&pylib);
        let mut program = pe_image(PE_X64, &[], 6000);
        program.extend_from_slice(&archive);
        scratch.write("yt-dlp.exe", &program);

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Pyinstaller);
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.version.as_deref(), Some("3.12"));
        assert_eq!(
            finding.version_evidence[0].from,
            "CArchive cookie (python312.dll)"
        );
        assert_eq!(texts(&finding.reasons), ["python-frozen"]);
        assert_eq!(finding.verdict, Verdict::RehostableWithWork);
    }

    #[test]
    fn a_dotnet_directory_names_its_runtime_and_an_arm_build_comes_last() {
        let scratch = Scratch::new("detect-dotnet");
        let config = br#"{"runtimeOptions":{"tfm":"net8.0","framework":{"name":"Microsoft.NETCore.App","version":"8.0.1"}}}"#;
        for arch in ["linux-arm64", "win-x64"] {
            scratch.write(&format!("{arch}/App.runtimeconfig.json"), config);
            scratch.write(&format!("{arch}/App.exe"), &pe_image(PE_X64, &[], 8192));
            scratch.write(&format!("{arch}/App.dll"), &managed_image());
            scratch.write(&format!("{arch}/native.dll"), &pe_image(PE_X64, &[], 5000));
        }

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert_eq!(findings[0].app_dir, scratch.path.join("win-x64"));
        assert_eq!(findings[1].app_dir, scratch.path.join("linux-arm64"));
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Dotnet);
        assert_eq!(finding.version.as_deref(), Some("8.0.1"));
        assert_eq!(finding.version_evidence[0].from, "App.runtimeconfig.json");
        assert_eq!(finding.verdict, Verdict::Rehostable);
        assert!(finding.reasons.is_empty());
        let natives: Vec<&str> = finding.natives.iter().map(|module| module.path.as_str()).collect();
        assert_eq!(natives, ["native.dll"]);
    }

    #[test]
    fn a_renpy_game_reads_its_version_and_linux_runtime() {
        let scratch = Scratch::new("detect-renpy");
        scratch.write("Game/game/script.rpy", b"label start:\n");
        scratch.write("Game/renpy/vc_version.py", b"branch = 'fix'\nversion = '8.3.7.25031702'\n");
        scratch.write("Game/lib/py3-windows-x86_64/python.exe", b"x");
        scratch.write("Game/lib/py3-linux-x86_64/python", b"x");

        let findings = detect(&scratch.path);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.family, Family::Renpy);
        assert_eq!(finding.platform, Platform::Windows);
        assert_eq!(finding.version.as_deref(), Some("8.3.7.25031702"));
        assert_eq!(texts(&finding.reasons), ["package-carries-linux-runtime"]);
    }

    #[test]
    fn small_readers_follow_the_prototype_patterns() {
        assert_eq!(
            renpy_version_in("x = 1\nversion = u'7.4.11.2266'\n").as_deref(),
            Some("7.4.11.2266")
        );
        assert_eq!(
            renpy_version_in("version_tuple = (8, 1, 3, vc_version)\n").as_deref(),
            Some("8.1.3")
        );
        assert_eq!(renpy_version_in("version = '1'\n"), None);
        assert_eq!(
            love_conf_version("t.version = '0.10.2'").as_deref(),
            Some("0.10.2")
        );
        assert_eq!(love_conf_version("t.version = nil"), None);
        assert!(is_version_file("v43.7.7"));
        assert!(is_version_file("1.2.3-beta.1"));
        assert!(!is_version_file("1.2"));
        assert!(!is_version_file("1.2.3 extra"));
        assert!(starts_with_version("11.5.0.0"));
        assert_eq!(dotted("11, 5, 0, 0"), "11.5.0");
        assert!(mentions_arm("linux-arm64/app"));
        assert!(mentions_arm("ARM64"));
        assert!(mentions_arm("x/aarch64"));
        assert!(!mentions_arm("farm64"));
        assert!(!mentions_arm("linux-x64"));
        assert!(is_javafx_platform_jar("javafx-base-17-win.jar"));
        assert!(is_javafx_platform_jar("javafx-graphics-21-linux-aarch64.jar"));
        assert!(!is_javafx_platform_jar("javafx-base-17.jar"));
        assert_eq!(
            manifest_main_class(b"Manifest-Version: 1.0\r\nMain-Class: org.example.Ma\r\n in\r\n").as_deref(),
            Some("org.example.Main")
        );
        assert_eq!(clean("a\u{1b}[31mb\n"), "a[31mb");
    }

    #[test]
    fn the_plist_reader_keeps_top_level_strings_only() {
        let text = "<?xml version=\"1.0\"?><!DOCTYPE plist><plist version=\"1.0\"><dict>\
            <key>CFBundleVersion</key><string>42.1.0</string>\
            <!-- <key>Hidden</key><string>no</string> -->\
            <key>Nested</key><dict><key>Inner</key><string>no</string></dict>\
            <key>Flag</key><true/>\
            <key>After</key><integer>3</integer><string>orphan</string>\
            <key>Name</key><string>A &amp; B</string>\
            <key>CFBundleVersion</key><string>second</string>\
            </dict></plist>";
        let strings = top_level_strings(text);
        assert_eq!(strings.get("CFBundleVersion").map(String::as_str), Some("42.1.0"));
        assert_eq!(strings.get("Name").map(String::as_str), Some("A & B"));
        assert_eq!(strings.len(), 2, "{strings:?}");
    }
}

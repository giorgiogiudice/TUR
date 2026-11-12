// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! macOS application bundle inspection.
//!
//! A bundle is a directory with `Contents/Info.plist` and a main program
//! under `Contents/MacOS`. The program is located and then inspected like any
//! other file. Nothing outside the bundle directory is ever read: an
//! executable name that is not a plain file name is ignored, and symbolic
//! links are not followed.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::InspectError;
use crate::inspect::{Inspection, inspect_file};
use crate::limits::{MAX_BUNDLE_DIR_ENTRIES, MAX_PLIST_BYTES};
use crate::plist;

/// What became of the bundle's `Contents/Info.plist`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlistStatus {
    /// An XML property list; its top-level string values were read.
    Parsed,
    /// A binary property list. It is not parsed.
    BinaryNotParsed,
    /// The file is neither an XML nor a binary property list, or is larger
    /// than the inspection limit. It is not parsed.
    NotRecognised,
    /// There is no `Contents/Info.plist`, or it is not a regular file.
    Missing,
}

impl PlistStatus {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            PlistStatus::Parsed => "XML, parsed",
            PlistStatus::BinaryNotParsed => "binary, not parsed",
            PlistStatus::NotRecognised => "not recognised, not parsed",
            PlistStatus::Missing => "missing",
        }
    }
}

/// How the bundle's main program was located.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutableSource {
    /// Named by `CFBundleExecutable` in `Info.plist`.
    InfoPlist,
    /// `Contents/MacOS/<bundle name>`, the convention when the property list
    /// does not name a program.
    BundleName,
    /// The only regular file in `Contents/MacOS`.
    OnlyFile,
}

/// What was read from a macOS application bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppBundleInfo {
    /// What became of `Contents/Info.plist`.
    pub plist: PlistStatus,
    /// `CFBundleIdentifier`, when the property list was parsed and has it.
    pub identifier: Option<String>,
    /// `CFBundleDisplayName` or `CFBundleName`, likewise.
    pub name: Option<String>,
    /// `CFBundleShortVersionString`, likewise.
    pub version: Option<String>,
    /// The file name of the main program under `Contents/MacOS`, if located.
    pub executable_name: Option<String>,
    /// How the main program was located.
    pub executable_source: Option<ExecutableSource>,
    /// The inspection of the main program.
    pub executable: Option<Box<Inspection>>,
}

/// Whether `directory` has the shape of an application bundle.
pub(crate) fn is_bundle(directory: &Path) -> bool {
    is_regular_file(&directory.join("Contents").join("Info.plist"))
        || is_directory(&directory.join("Contents").join("MacOS"))
}

/// Inspects the bundle at `directory`.
pub(crate) fn inspect_bundle(
    directory: &Path,
    notes: &mut Vec<String>,
) -> Result<AppBundleInfo, InspectError> {
    let contents = directory.join("Contents");
    let (status, values) = read_plist(&contents.join("Info.plist"))?;
    let value = |key: &str| values.get(key).filter(|text| is_printable(text)).cloned();

    let programs = contents.join("MacOS");
    let mut located: Option<(String, ExecutableSource)> = None;
    if let Some(declared) = values.get("CFBundleExecutable") {
        if !is_plain_file_name(declared) {
            notes.push(
                "Info.plist names a main program that is not a plain file name; it was ignored."
                    .to_owned(),
            );
        } else if is_regular_file(&programs.join(declared)) {
            located = Some((declared.clone(), ExecutableSource::InfoPlist));
        } else {
            notes.push(
                "Info.plist names a main program that is not a regular file in Contents/MacOS."
                    .to_owned(),
            );
        }
    }
    if located.is_none()
        && let Some(stem) = directory.file_stem().and_then(|stem| stem.to_str())
        && is_plain_file_name(stem)
        && is_regular_file(&programs.join(stem))
    {
        located = Some((stem.to_owned(), ExecutableSource::BundleName));
    }
    if located.is_none()
        && let Some(only) = only_regular_file(&programs)
    {
        located = Some((only, ExecutableSource::OnlyFile));
    }

    let (executable_name, executable_source, executable) = match located {
        Some((name, source)) => {
            let inspection = inspect_file(&programs.join(&name))?;
            (Some(name), Some(source), Some(Box::new(inspection)))
        }
        None => (None, None, None),
    };

    Ok(AppBundleInfo {
        plist: status,
        identifier: value("CFBundleIdentifier"),
        name: value("CFBundleDisplayName").or_else(|| value("CFBundleName")),
        version: value("CFBundleShortVersionString"),
        executable_name,
        executable_source,
        executable,
    })
}

type PlistValues = std::collections::BTreeMap<String, String>;

fn read_plist(path: &Path) -> Result<(PlistStatus, PlistValues), InspectError> {
    if !is_regular_file(path) {
        return Ok((PlistStatus::Missing, PlistValues::new()));
    }
    let file = File::open(path).map_err(|error| InspectError::io(path, &error))?;
    let mut bytes = Vec::new();
    file.take(MAX_PLIST_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| InspectError::io(path, &error))?;
    let too_large = u64::try_from(bytes.len()).is_ok_and(|len| len > MAX_PLIST_BYTES);
    if too_large {
        return Ok((PlistStatus::NotRecognised, PlistValues::new()));
    }
    if bytes.starts_with(b"bplist") {
        return Ok((PlistStatus::BinaryNotParsed, PlistValues::new()));
    }
    match std::str::from_utf8(&bytes) {
        Ok(text) if text.contains("<plist") => {
            Ok((PlistStatus::Parsed, plist::top_level_strings(text)))
        }
        _ => Ok((PlistStatus::NotRecognised, PlistValues::new())),
    }
}

/// The name of the only regular file in `directory`, if there is exactly one.
fn only_regular_file(directory: &Path) -> Option<String> {
    let mut found: Option<String> = None;
    for entry in fs::read_dir(directory).ok()?.take(MAX_BUNDLE_DIR_ENTRIES) {
        let entry = entry.ok()?;
        let path: PathBuf = entry.path();
        if !is_regular_file(&path) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(entry.file_name().to_str()?.to_owned());
    }
    found.filter(|name| is_plain_file_name(name))
}

/// Whether `path` is a regular file, without following a symbolic link.
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Whether `path` is a directory, without following a symbolic link.
fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

/// Whether `name` is a single path component that stays inside its directory.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && is_printable(name)
}

fn is_printable(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::Arch;
    use crate::inspect::{Detected, inspect};
    use crate::macho::MachOFileType;
    use crate::testdata::{CPU_X86_64, TempDir, macho_thin};

    fn plist_xml(executable: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
             <key>CFBundleExecutable</key><string>{executable}</string>\n\
             <key>CFBundleIdentifier</key><string>com.example.Demo</string>\n\
             <key>CFBundleName</key><string>Demo</string>\n\
             <key>CFBundleShortVersionString</key><string>2.5</string>\n\
             </dict>\n</plist>\n"
        )
    }

    fn make_bundle(temp: &TempDir, name: &str, plist: Option<&[u8]>, programs: &[&str]) -> PathBuf {
        let bundle = temp.path().join(name);
        let macos = bundle.join("Contents").join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        if let Some(plist) = plist {
            fs::write(bundle.join("Contents").join("Info.plist"), plist).unwrap();
        }
        for program in programs {
            fs::write(macos.join(program), macho_thin(CPU_X86_64, 2, true, false)).unwrap();
        }
        bundle
    }

    fn bundle_info(path: &Path) -> AppBundleInfo {
        match inspect(path).unwrap().detected {
            Detected::MacAppBundle(info) => info,
            other => panic!("expected an application bundle, got {other:?}"),
        }
    }

    fn executable_arch(info: &AppBundleInfo) -> Option<Arch> {
        match &info.executable.as_ref()?.detected {
            Detected::MachO(macho) => macho.slices.first().map(|slice| slice.arch),
            _ => None,
        }
    }

    #[test]
    fn xml_plist_names_the_main_program() {
        let temp = TempDir::new("bundle-xml");
        let plist = plist_xml("DemoMain");
        let path = make_bundle(
            &temp,
            "Demo.app",
            Some(plist.as_bytes()),
            &["DemoMain", "helper"],
        );
        let info = bundle_info(&path);
        assert_eq!(info.plist, PlistStatus::Parsed);
        assert_eq!(info.identifier.as_deref(), Some("com.example.Demo"));
        assert_eq!(info.name.as_deref(), Some("Demo"));
        assert_eq!(info.version.as_deref(), Some("2.5"));
        assert_eq!(info.executable_name.as_deref(), Some("DemoMain"));
        assert_eq!(info.executable_source, Some(ExecutableSource::InfoPlist));
        assert_eq!(executable_arch(&info), Some(Arch::X86_64));
        let Some(Detected::MachO(macho)) = info.executable.map(|inner| inner.detected) else {
            panic!("the main program should be Mach-O");
        };
        assert_eq!(macho.slices[0].file_type, MachOFileType::Executable);
    }

    #[test]
    fn binary_plist_is_reported_as_not_parsed() {
        let temp = TempDir::new("bundle-binary");
        let path = make_bundle(
            &temp,
            "Tool.app",
            Some(&b"bplist00\xd1\x01\x02_\x10\x12CFBundleExecutable"[..]),
            &["Tool"],
        );
        let info = bundle_info(&path);
        assert_eq!(info.plist, PlistStatus::BinaryNotParsed);
        assert_eq!(info.identifier, None);
        assert_eq!(info.name, None);
        // Located by convention, and the report says so.
        assert_eq!(info.executable_name.as_deref(), Some("Tool"));
        assert_eq!(info.executable_source, Some(ExecutableSource::BundleName));
        assert_eq!(executable_arch(&info), Some(Arch::X86_64));
    }

    #[test]
    fn missing_plist_falls_back_to_the_only_program() {
        let temp = TempDir::new("bundle-noplist");
        let path = make_bundle(&temp, "Other.app", None, &["launcher"]);
        let info = bundle_info(&path);
        assert_eq!(info.plist, PlistStatus::Missing);
        assert_eq!(info.executable_name.as_deref(), Some("launcher"));
        assert_eq!(info.executable_source, Some(ExecutableSource::OnlyFile));
    }

    #[test]
    fn ambiguous_or_absent_program_is_not_guessed() {
        let temp = TempDir::new("bundle-ambiguous");
        let path = make_bundle(&temp, "Many.app", None, &["one", "two"]);
        let info = bundle_info(&path);
        assert_eq!(info.executable_name, None);
        assert_eq!(info.executable, None);

        let path = make_bundle(&temp, "Empty.app", None, &[]);
        let info = bundle_info(&path);
        assert_eq!(info.executable_name, None);
        assert_eq!(info.executable_source, None);
    }

    #[test]
    fn program_name_that_escapes_the_bundle_is_ignored() {
        let temp = TempDir::new("bundle-escape");
        fs::write(
            temp.path().join("outside"),
            macho_thin(CPU_X86_64, 2, true, false),
        )
        .unwrap();
        let plist = plist_xml("../../../outside");
        let path = make_bundle(&temp, "Evil.app", Some(plist.as_bytes()), &[]);
        let inspection = inspect(&path).unwrap();
        let Detected::MacAppBundle(info) = &inspection.detected else {
            panic!("expected an application bundle");
        };
        assert_eq!(info.plist, PlistStatus::Parsed);
        assert_eq!(info.executable_name, None);
        assert_eq!(info.executable, None);
        assert!(
            inspection
                .notes
                .iter()
                .any(|note| note.contains("not a plain file name"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_as_main_program_is_not_followed() {
        let temp = TempDir::new("bundle-symlink");
        let target = temp.path().join("outside");
        fs::write(&target, macho_thin(CPU_X86_64, 2, true, false)).unwrap();
        let plist = plist_xml("Linked");
        let path = make_bundle(&temp, "Linked.app", Some(plist.as_bytes()), &[]);
        std::os::unix::fs::symlink(&target, path.join("Contents").join("MacOS").join("Linked"))
            .unwrap();
        let info = bundle_info(&path);
        assert_eq!(info.executable_name, None);
        assert_eq!(info.executable, None);
    }

    #[test]
    fn main_program_that_is_a_script_is_reported_as_one() {
        let temp = TempDir::new("bundle-script");
        let plist = plist_xml("run");
        let path = make_bundle(&temp, "Script.app", Some(plist.as_bytes()), &[]);
        fs::write(
            path.join("Contents").join("MacOS").join("run"),
            b"#!/bin/sh\nexec true\n",
        )
        .unwrap();
        let info = bundle_info(&path);
        let detected = info.executable.map(|inner| inner.detected);
        assert!(matches!(detected, Some(Detected::Script(_))));
    }

    #[test]
    fn truncated_main_program_is_a_typed_error() {
        let temp = TempDir::new("bundle-truncated");
        let plist = plist_xml("Cut");
        let path = make_bundle(&temp, "Cut.app", Some(plist.as_bytes()), &[]);
        let program = macho_thin(CPU_X86_64, 2, true, false);
        fs::write(
            path.join("Contents").join("MacOS").join("Cut"),
            &program[..12],
        )
        .unwrap();
        assert!(matches!(
            inspect(&path),
            Err(InspectError::Truncated { .. })
        ));
    }

    #[test]
    fn directory_that_is_not_a_bundle_is_unknown() {
        let temp = TempDir::new("bundle-plain-dir");
        let inspection = inspect(temp.path()).unwrap();
        assert_eq!(inspection.detected, Detected::Unknown);
        assert!(!inspection.notes.is_empty());
    }

    #[test]
    fn file_name_rules() {
        assert!(is_plain_file_name("Example"));
        assert!(is_plain_file_name("My App 2"));
        for bad in ["", ".", "..", "a/b", "..\\x", "a\0b", "a\nb", "/abs"] {
            assert!(!is_plain_file_name(bad), "{bad:?} should be rejected");
        }
        assert!(!is_plain_file_name(&"x".repeat(256)));
    }
}

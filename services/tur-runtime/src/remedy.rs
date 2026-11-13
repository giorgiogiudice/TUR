// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The remedy catalogue of the compatibility autopilot
//! (`docs/architecture/compat-autopilot.md`).
//!
//! A remedy is a declarative, reversible change: values in the prefix's
//! registry, variables in the environment of a launch, a case-insensitive
//! drive C:, or the `ntsync` device left out of the sandbox. The catalogue
//! is data, `services/tur-runtime/remedies/catalogue.json`, in the format
//! `schemas/remedy-catalogue.schema.json`; it is compiled into the program
//! and checked by [`Catalogue::validate`] and by this module's tests.
//!
//! No remedy downloads, installs or runs anything: the four kinds cannot
//! express it. A failure that needs a Microsoft component is listed among
//! [`Catalogue::microsoft_components`] and is answered with the Windows
//! guest, never with a remedy.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::failure::{ComponentFamily, FailureClass, Hint};
use crate::metadata::RuntimeKind;
use crate::runtime_cmd::check_extra_environment;

/// The version of the catalogue format.
pub const REMEDY_SCHEMA_VERSION: u32 = 1;

/// The catalogue this program ships.
pub const CATALOGUE_TEXT: &str = include_str!("../remedies/catalogue.json");

/// The most remedies a catalogue may hold.
const MAX_REMEDIES: usize = 256;

/// What a remedy changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemedyKind {
    /// Values in the prefix's registry, written with Wine's own `reg import`.
    Registry,
    /// Variables in the environment of the application's launches.
    Environment,
    /// Drive C: rebuilt with the case-folding attribute.
    DriveCCaseFolding,
    /// `/dev/ntsync` left out of the application's sandbox.
    HideNtsync,
}

impl RemedyKind {
    /// The kind's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RemedyKind::Registry => "registry",
            RemedyKind::Environment => "environment",
            RemedyKind::DriveCCaseFolding => "drive-c-case-folding",
            RemedyKind::HideNtsync => "hide-ntsync",
        }
    }
}

/// What a remedy needs from the machine or the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Requirement {
    /// An X11 display.
    X11Display,
    /// Wine implements the DLL named in the evidence.
    WineBuiltinDll,
    /// The DLL named in the evidence lies beside the program.
    DllBesideProgram,
    /// The file system can fold case (ext4 with `casefold`).
    CaseFoldingFileSystem,
    /// `/dev/ntsync` opens and Wine uses it.
    NtsyncDevice,
    /// The font file named by `font_file` is on the machine.
    FontFile,
}

/// The parameter a remedy takes from the evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Parameter {
    /// The DLL the evidence names.
    Dll,
}

/// The type of a registry value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistryType {
    /// A string.
    String,
    /// A 32-bit number, written in decimal in the catalogue.
    Dword,
}

/// One registry value a remedy sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryValue {
    /// The key, starting with `HKEY_CURRENT_USER\`. May hold `{exe}`.
    pub key: String,
    /// The value's name. May be `{dll}`.
    pub name: String,
    /// The value's type.
    #[serde(rename = "type")]
    pub value_type: RegistryType,
    /// The value's data.
    pub value: String,
}

/// One environment variable a remedy sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentValue {
    /// The variable's name.
    pub name: String,
    /// Its value.
    pub value: String,
}

/// Where a remedy comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The kind of source.
    pub kind: SourceKind,
    /// The document that was read.
    pub reference: String,
    /// What it says, in TUR's words.
    pub detail: String,
    /// The day it was read, `YYYY-MM-DD`.
    pub checked: String,
}

/// The kinds of source a remedy may cite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Wine's source code.
    WineSource,
    /// Wine's release notes.
    WineReleaseNotes,
    /// Proton's README.
    ProtonReadme,
    /// A measurement in TUR's own CI.
    TurMeasurement,
}

/// One remedy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remedy {
    /// The identifier.
    pub id: String,
    /// A short title.
    pub title: String,
    /// What the remedy changes.
    pub description: String,
    /// The kind of change.
    pub kind: RemedyKind,
    /// Remedies of one group are alternatives and are never combined.
    pub group: String,
    /// The runtimes it applies to.
    pub runtimes: Vec<RuntimeKind>,
    /// The failure classes it is tried for.
    pub addresses: Vec<FailureClass>,
    /// The hints that bring it forward.
    pub hints: Vec<Hint>,
    /// Whether it is tried only when one of its hints is present.
    #[serde(default)]
    pub needs_hint: bool,
    /// The parameter it takes from the evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<Parameter>,
    /// The order among remedies of equal standing; lower first.
    pub order: u32,
    /// The registry values it sets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry: Vec<RegistryValue>,
    /// The environment variables it sets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<EnvironmentValue>,
    /// What it needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<Requirement>,
    /// The font file `font-file` looks for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_file: Option<String>,
    /// Whether the autopilot tries it by itself.
    pub automatic: bool,
    /// How it is undone.
    pub reverting: String,
    /// Where it comes from.
    pub sources: Vec<Source>,
}

/// A Microsoft redistributable, by the names of its DLLs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrosoftComponent {
    /// The identifier.
    pub id: String,
    /// The name.
    pub name: String,
    /// The DLL name prefixes; see [`ComponentFamily::matches`].
    pub dll_prefixes: Vec<String>,
    /// What else is known.
    pub note: String,
}

/// The remedy catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// The licence and the description of the file.
    #[serde(rename = "$comment", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// [`REMEDY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The day the sources were read.
    pub checked: String,
    /// The remedies.
    pub remedies: Vec<Remedy>,
    /// The Microsoft components TUR cannot supply.
    pub microsoft_components: Vec<MicrosoftComponent>,
}

/// One remedy as it is tried: its identifier and the values of its
/// parameters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemedyInstance {
    /// The remedy's identifier.
    pub id: String,
    /// The parameters: `exe`, and `dll` for remedies that take one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    /// The identifier with its DLL, as a catalogue record names it:
    /// `dll-builtin:d3dx9_43`.
    pub label: String,
}

/// A remedy instance with everything it does spelled out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The instance.
    pub instance: RemedyInstance,
    /// The kind.
    pub kind: RemedyKind,
    /// The group.
    pub group: String,
    /// The registry values, with their parameters filled in.
    pub registry: Vec<RegistryValue>,
    /// The environment variables.
    pub environment: Vec<(String, String)>,
}

/// Whether `text` is a catalogue identifier: lower-case words of letters
/// and digits joined by hyphens.
pub fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Whether `text` can be the `exe` parameter: a plain file name.
pub fn is_program_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b' '))
        && !text.starts_with('.')
}

/// Whether `text` can be the `dll` parameter: a DLL's base name in lower
/// case, without `.dll`.
pub fn is_dll_parameter(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
        })
        && !text.starts_with('.')
        && !text.ends_with(".dll")
}

fn is_plain_text(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// Words no remedy may contain: a remedy that installs something is not a
/// remedy here.
const FORBIDDEN_WORDS: [&str; 8] = [
    "winetricks",
    "vcredist",
    "dotnetfx",
    "corefonts",
    "download",
    "install ",
    "http://",
    "msiexec",
];

impl Catalogue {
    /// Parses a catalogue.
    ///
    /// # Errors
    ///
    /// The serde message when the text is not a catalogue's shape.
    pub fn parse(text: &str) -> Result<Catalogue, String> {
        serde_json::from_str(text).map_err(|error| error.to_string())
    }

    /// The catalogue this program ships, parsed and validated.
    ///
    /// # Panics
    ///
    /// Never for a released build: the shipped catalogue is checked by this
    /// module's tests.
    pub fn shipped() -> Catalogue {
        let catalogue = Catalogue::parse(CATALOGUE_TEXT).expect("the shipped catalogue parses");
        debug_assert!(catalogue.validate().is_empty());
        catalogue
    }

    /// The remedy with this identifier.
    pub fn remedy(&self, id: &str) -> Option<&Remedy> {
        self.remedies.iter().find(|remedy| remedy.id == id)
    }

    /// The Microsoft components as families of DLL names.
    pub fn families(&self) -> Vec<ComponentFamily> {
        self.microsoft_components
            .iter()
            .map(|component| ComponentFamily {
                id: component.id.clone(),
                name: component.name.clone(),
                prefixes: component.dll_prefixes.clone(),
            })
            .collect()
    }

    /// Every rule the schema cannot express, as problems. Empty when the
    /// catalogue is valid.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema_version != REMEDY_SCHEMA_VERSION {
            problems.push(format!("schema_version must be {REMEDY_SCHEMA_VERSION}"));
        }
        if !is_date(&self.checked) {
            problems.push("checked is not a date".to_owned());
        }
        if self.remedies.is_empty() || self.remedies.len() > MAX_REMEDIES {
            problems.push(format!("there must be 1 to {MAX_REMEDIES} remedies"));
        }
        let mut ids = BTreeSet::new();
        for remedy in &self.remedies {
            for problem in remedy.problems() {
                problems.push(format!("{}: {problem}", remedy.id));
            }
            if !ids.insert(remedy.id.as_str()) {
                problems.push(format!("{}: the identifier is used twice", remedy.id));
            }
        }
        let mut component_ids = BTreeSet::new();
        for component in &self.microsoft_components {
            if !is_identifier(&component.id) || !component_ids.insert(component.id.as_str()) {
                problems.push(format!(
                    "microsoft component {}: the identifier is not valid or is used twice",
                    component.id
                ));
            }
            if component.dll_prefixes.is_empty()
                || !component
                    .dll_prefixes
                    .iter()
                    .all(|prefix| is_dll_parameter(prefix))
            {
                problems.push(format!(
                    "microsoft component {}: dll_prefixes must be lower-case DLL names",
                    component.id
                ));
            }
            if !is_plain_text(&component.name, 200) || !is_plain_text(&component.note, 1000) {
                problems.push(format!(
                    "microsoft component {}: text too long or holds a control character",
                    component.id
                ));
            }
        }
        problems
    }

    /// The remedy instance for `id` with the parameters of this program and
    /// failure.
    ///
    /// # Errors
    ///
    /// A message when there is no such remedy or a parameter it needs is
    /// missing or not acceptable.
    pub fn resolve(&self, instance: &RemedyInstance) -> Result<Resolved, String> {
        let remedy = self
            .remedy(&instance.id)
            .ok_or_else(|| format!("there is no remedy {}", instance.id))?;
        let exe = instance.parameters.get("exe");
        if let Some(exe) = exe
            && !is_program_name(exe)
        {
            return Err(format!("{exe:?} is not a program's file name"));
        }
        let dll = instance.parameters.get("dll");
        if remedy.parameter == Some(Parameter::Dll) {
            match dll {
                Some(dll) if is_dll_parameter(dll) => {}
                _ => return Err(format!("{} needs the name of a DLL", remedy.id)),
            }
        }
        let fill = |text: &str| -> Result<String, String> {
            let mut out = text.to_owned();
            if out.contains("{exe}") {
                let exe = exe.ok_or_else(|| format!("{} needs the program's name", remedy.id))?;
                out = out.replace("{exe}", exe);
            }
            if out.contains("{dll}") {
                let dll = dll.ok_or_else(|| format!("{} needs the name of a DLL", remedy.id))?;
                out = out.replace("{dll}", dll);
            }
            Ok(out)
        };
        let mut registry = Vec::new();
        for value in &remedy.registry {
            registry.push(RegistryValue {
                key: fill(&value.key)?,
                name: fill(&value.name)?,
                value_type: value.value_type,
                value: fill(&value.value)?,
            });
        }
        let environment = remedy
            .environment
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect();
        Ok(Resolved {
            instance: instance.clone(),
            kind: remedy.kind,
            group: remedy.group.clone(),
            registry,
            environment,
        })
    }
}

impl Remedy {
    /// What is wrong with this remedy, beyond its shape.
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_identifier(&self.id) {
            problems.push("the identifier is not lower-case words joined by hyphens".to_owned());
        }
        if !is_identifier(&self.group) {
            problems.push("the group is not an identifier".to_owned());
        }
        for (field, text, max) in [
            ("title", &self.title, 200),
            ("description", &self.description, 2000),
            ("reverting", &self.reverting, 1000),
        ] {
            if text.is_empty() || !is_plain_text(text, max) {
                problems.push(format!(
                    "{field} is empty, too long or holds a control character"
                ));
            }
        }
        if self.runtimes.is_empty() {
            problems.push("it names no runtime".to_owned());
        }
        if self.automatic && self.addresses.is_empty() {
            problems.push("an automatic remedy must address a failure class".to_owned());
        }
        if self.needs_hint && self.hints.is_empty() {
            problems.push("needs_hint without a hint".to_owned());
        }
        if self.addresses.iter().any(|class| class.is_unsuppliable()) {
            problems.push(
                "no remedy addresses a missing Microsoft component or .NET runtime: TUR cannot \
                 supply them"
                    .to_owned(),
            );
        }
        if self.sources.is_empty() {
            problems.push("it cites no source".to_owned());
        }
        for source in &self.sources {
            if !is_date(&source.checked)
                || source.detail.is_empty()
                || !is_plain_text(&source.detail, 2000)
                || !is_plain_text(&source.reference, 500)
            {
                problems.push(format!("the source {} is not complete", source.reference));
            }
            if !(source.reference.starts_with("https://") || source.reference.starts_with("docs/"))
            {
                problems.push(format!(
                    "the source {} is neither an https address nor a document of this repository",
                    source.reference
                ));
            }
        }
        let shape_ok = match self.kind {
            RemedyKind::Registry => !self.registry.is_empty() && self.environment.is_empty(),
            RemedyKind::Environment => self.registry.is_empty() && !self.environment.is_empty(),
            RemedyKind::DriveCCaseFolding | RemedyKind::HideNtsync => {
                self.registry.is_empty() && self.environment.is_empty()
            }
        };
        if !shape_ok {
            problems.push(format!(
                "a {} remedy has the wrong kind of changes",
                self.kind.as_str()
            ));
        }
        let takes_dll = self.parameter == Some(Parameter::Dll);
        for value in &self.registry {
            if !value.key.starts_with("HKEY_CURRENT_USER\\") {
                problems.push(format!(
                    "{}: only the user's own registry (HKEY_CURRENT_USER) is changed",
                    value.key
                ));
            }
            for text in [&value.key, &value.name, &value.value] {
                if !is_plain_text(text, 512)
                    || text.contains('"')
                    || text.contains('[')
                    || text.contains(']')
                {
                    problems.push(format!("{text:?} cannot be written into a .reg file"));
                }
                let rest = text.replace("{exe}", "").replace("{dll}", "");
                if rest.contains('{') || rest.contains('}') {
                    problems.push(format!("{text:?} holds an unknown placeholder"));
                }
                if text.contains("{dll}") && !takes_dll {
                    problems.push(format!("{text:?} uses {{dll}} and the remedy takes no DLL"));
                }
            }
            if value.value_type == RegistryType::Dword && value.value.parse::<u32>().is_err() {
                problems.push(format!("{}: a dword must be a decimal number", value.name));
            }
        }
        let pairs: Vec<(String, String)> = self
            .environment
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect();
        if let Err(error) = check_extra_environment(&pairs) {
            problems.push(error.to_string());
        }
        if self.requires.contains(&Requirement::WineBuiltinDll) && !takes_dll {
            problems.push("wine-builtin-dll needs the dll parameter".to_owned());
        }
        if self.requires.contains(&Requirement::FontFile) != self.font_file.is_some() {
            problems.push("font-file and font_file go together".to_owned());
        }
        let everything = serde_json::to_string(self)
            .unwrap_or_default()
            .to_lowercase();
        for word in FORBIDDEN_WORDS {
            if everything.contains(word) {
                problems.push(format!(
                    "it mentions {word:?}: a remedy never fetches or installs anything"
                ));
            }
        }
        problems
    }
}

/// Escapes a string for a `.reg` file: backslash and double quote.
fn reg_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The `.reg` file (REGEDIT4) that sets the registry values of `remedies`,
/// and `extra` before them.
pub fn reg_file(extra: &[RegistryValue], remedies: &[&Resolved]) -> String {
    let mut text = String::from("REGEDIT4\n\n");
    let values = extra
        .iter()
        .chain(remedies.iter().flat_map(|remedy| remedy.registry.iter()));
    for value in values {
        let _ = writeln!(text, "[{}]", value.key);
        let data = match value.value_type {
            RegistryType::String => format!("\"{}\"", reg_string(&value.value)),
            RegistryType::Dword => {
                format!("dword:{:08x}", value.value.parse::<u32>().unwrap_or(0))
            }
        };
        let _ = writeln!(text, "\"{}\"={data}\n", reg_string(&value.name));
    }
    text
}

/// The `.reg` text that deletes what `remedies` set: how a registry remedy
/// is undone when no backup is used.
pub fn revert_reg_file(remedies: &[&Resolved]) -> String {
    let mut text = String::from("REGEDIT4\n\n");
    for value in remedies.iter().flat_map(|remedy| remedy.registry.iter()) {
        let _ = writeln!(text, "[{}]", value.key);
        let _ = writeln!(text, "\"{}\"=-\n", reg_string(&value.name));
    }
    text
}

/// The registry values every trial copy gets before anything is tried:
/// Wine's crash dialog is switched off, so that a crash ends the program
/// instead of waiting for someone to close a dialog. Never applied to the
/// real prefix.
pub fn trial_settings() -> Vec<RegistryValue> {
    vec![RegistryValue {
        key: "HKEY_CURRENT_USER\\Software\\Wine\\WineDbg".to_owned(),
        name: "ShowCrashDialog".to_owned(),
        value_type: RegistryType::Dword,
        value: "0".to_owned(),
    }]
}

/// The instance of `remedy` for a program and, for a remedy that takes one,
/// a DLL.
pub fn instance(remedy: &Remedy, exe: &str, dll: Option<&str>) -> RemedyInstance {
    let mut parameters = BTreeMap::new();
    let uses_exe = remedy
        .registry
        .iter()
        .any(|value| value.key.contains("{exe}") || value.name.contains("{exe}"));
    if uses_exe {
        parameters.insert("exe".to_owned(), exe.to_owned());
    }
    let mut label = remedy.id.clone();
    if remedy.parameter == Some(Parameter::Dll)
        && let Some(dll) = dll
    {
        let base = crate::failure::dll_base(dll);
        label = format!("{}:{base}", remedy.id);
        parameters.insert("dll".to_owned(), base);
    }
    RemedyInstance {
        id: remedy.id.clone(),
        parameters,
        label,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const SCHEMA: &str = include_str!("../../../schemas/remedy-catalogue.schema.json");

    fn keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .unwrap_or_else(|| panic!("not an object: {value}"))
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn the_shipped_catalogue_is_valid() {
        let catalogue = Catalogue::parse(CATALOGUE_TEXT).unwrap();
        assert_eq!(catalogue.validate(), Vec::<String>::new());
        assert!(catalogue.remedies.len() >= 20);
        // Every remedy cites a source that was read, and every automatic
        // remedy addresses something.
        for remedy in &catalogue.remedies {
            assert!(!remedy.sources.is_empty(), "{}", remedy.id);
        }
        // The kinds of change the task asks for are all there.
        for id in [
            "windows-version-win11",
            "dll-builtin",
            "virtual-desktop-1024x768",
            "dpi-120",
            "renderer-gl",
            "renderer-vulkan",
            "audio-off",
            "font-arial-liberation-sans",
            "locale-ja",
            "drive-c-case-insensitive",
            "ntsync-off",
            "proton-no-esync",
            "proton-no-fsync",
        ] {
            assert!(catalogue.remedy(id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_schema_and_the_rust_types_name_the_same_fields() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        let written = serde_json::to_value(Catalogue::shipped()).unwrap();
        assert_eq!(keys(&written), keys(&schema["properties"]));
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            REMEDY_SCHEMA_VERSION
        );
        // A remedy with every optional field set.
        let full = Catalogue::shipped()
            .remedies
            .iter()
            .find(|remedy| remedy.id == "font-arial-liberation-sans")
            .cloned()
            .unwrap();
        let mut full = serde_json::to_value(full).unwrap();
        full["parameter"] = Value::from("dll");
        full["environment"] = serde_json::json!([{ "name": "A", "value": "b" }]);
        full["needs_hint"] = Value::Bool(true);
        assert_eq!(keys(&full), keys(&schema["$defs"]["remedy"]["properties"]));
        let component =
            serde_json::to_value(&Catalogue::shipped().microsoft_components[0]).unwrap();
        assert_eq!(
            keys(&component),
            keys(&schema["$defs"]["microsoft_component"]["properties"])
        );
        // The enumerations agree.
        let names = |value: &Value| -> BTreeSet<String> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect()
        };
        let classes: BTreeSet<String> = FailureClass::ALL
            .iter()
            .map(|class| class.as_str().to_owned())
            .collect();
        assert_eq!(names(&schema["$defs"]["failure_class"]["enum"]), classes);
        let kinds: BTreeSet<String> = [
            RemedyKind::Registry,
            RemedyKind::Environment,
            RemedyKind::DriveCCaseFolding,
            RemedyKind::HideNtsync,
        ]
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();
        assert_eq!(
            names(&schema["$defs"]["remedy"]["properties"]["kind"]["enum"]),
            kinds
        );
        for object in [
            &schema,
            &schema["$defs"]["remedy"],
            &schema["$defs"]["registry_value"],
            &schema["$defs"]["environment_value"],
            &schema["$defs"]["source"],
            &schema["$defs"]["microsoft_component"],
        ] {
            assert_eq!(object["additionalProperties"], Value::Bool(false));
        }
    }

    #[test]
    fn a_remedy_that_would_install_or_reach_outside_the_user_is_refused() {
        let catalogue = Catalogue::shipped();
        let base = catalogue.remedy("windows-version-win11").unwrap().clone();
        let check = |remedy: Remedy| {
            let mut changed = catalogue.clone();
            changed.remedies = vec![remedy];
            changed.validate()
        };
        let mut machine = base.clone();
        machine.registry[0].key = "HKEY_LOCAL_MACHINE\\Software\\Wine".to_owned();
        assert!(
            check(machine)
                .iter()
                .any(|p| p.contains("HKEY_CURRENT_USER"))
        );
        let mut fetch = base.clone();
        fetch.description = "Runs winetricks vcrun2019".to_owned();
        assert!(check(fetch).iter().any(|p| p.contains("winetricks")));
        let mut unsuppliable = base.clone();
        unsuppliable.addresses = vec![FailureClass::MicrosoftComponentMissing];
        assert!(check(unsuppliable).iter().any(|p| p.contains("Microsoft")));
        let mut loader = base.clone();
        loader.kind = RemedyKind::Environment;
        loader.registry.clear();
        loader.environment = vec![EnvironmentValue {
            name: "LD_PRELOAD".to_owned(),
            value: "/x.so".to_owned(),
        }];
        assert!(!check(loader).is_empty());
        let mut reserved = base.clone();
        reserved.kind = RemedyKind::Environment;
        reserved.registry.clear();
        reserved.environment = vec![EnvironmentValue {
            name: "WINEDLLOVERRIDES".to_owned(),
            value: "x=n".to_owned(),
        }];
        assert!(!check(reserved).is_empty());
        let mut injected = base.clone();
        injected.registry[0].value = "win11\"\n\"Other\"=\"x".to_owned();
        assert!(!check(injected).is_empty());
        let mut sourceless = base;
        sourceless.sources.clear();
        assert!(!check(sourceless).is_empty());
    }

    #[test]
    fn instances_fill_in_the_program_and_the_dll() {
        let catalogue = Catalogue::shipped();
        let remedy = catalogue.remedy("dll-builtin").unwrap();
        let instance = instance(remedy, "game.exe", Some("D3DX9_43.dll"));
        assert_eq!(instance.label, "dll-builtin:d3dx9_43");
        assert_eq!(instance.parameters["dll"], "d3dx9_43");
        assert_eq!(instance.parameters["exe"], "game.exe");
        let resolved = catalogue.resolve(&instance).unwrap();
        assert_eq!(
            resolved.registry[0].key,
            "HKEY_CURRENT_USER\\Software\\Wine\\AppDefaults\\game.exe\\DllOverrides"
        );
        assert_eq!(resolved.registry[0].name, "d3dx9_43");
        assert_eq!(resolved.registry[0].value, "builtin");

        let text = reg_file(&trial_settings(), &[&resolved]);
        assert!(text.starts_with("REGEDIT4\n\n"));
        assert!(text.contains(
            "[HKEY_CURRENT_USER\\Software\\Wine\\AppDefaults\\game.exe\\DllOverrides]\n\"d3dx9_43\"=\"builtin\"\n"
        ));
        assert!(text.contains("\"ShowCrashDialog\"=dword:00000000"));
        let revert = revert_reg_file(&[&resolved]);
        assert!(revert.contains("\"d3dx9_43\"=-"));

        // A DLL remedy without a DLL, and a hostile parameter, are refused.
        let mut bare = instance.clone();
        bare.parameters.remove("dll");
        assert!(catalogue.resolve(&bare).is_err());
        let mut hostile = instance;
        hostile
            .parameters
            .insert("exe".to_owned(), "a]\n[HKEY_LOCAL_MACHINE".to_owned());
        assert!(catalogue.resolve(&hostile).is_err());

        let dpi = catalogue
            .resolve(&super::instance(
                catalogue.remedy("dpi-120").unwrap(),
                "a.exe",
                None,
            ))
            .unwrap();
        assert!(reg_file(&[], &[&dpi]).contains("\"LogPixels\"=dword:00000078"));
        let locale = catalogue
            .resolve(&super::instance(
                catalogue.remedy("locale-ja").unwrap(),
                "a.exe",
                None,
            ))
            .unwrap();
        assert_eq!(
            locale.environment,
            vec![("LC_ALL".to_owned(), "ja_JP.UTF-8".to_owned())]
        );
        assert!(locale.instance.parameters.is_empty());
    }
}

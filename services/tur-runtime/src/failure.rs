// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What went wrong when a Windows program was started: stable failure
//! classes, read from evidence (the compatibility autopilot,
//! `docs/architecture/compat-autopilot.md`).
//!
//! The evidence is what a launch leaves behind: the exit status, whether a
//! window of the program appeared, whether the run had to be stopped, and
//! everything the program and Wine printed. Wine's own messages are matched
//! by the text its loader and debugger print (Wine 9.0,
//! `dlls/ntdll/loader.c`, `dlls/ntdll/signal_x86_64.c`,
//! `dlls/kernelbase/debug.c`, `programs/winedbg/info.c`):
//!
//! | Wine prints | Class |
//! |---|---|
//! | `Library X (which is needed by ...) not found` | `dll-missing`, or `microsoft-component-missing` when X is a Microsoft redistributable this Wine does not implement |
//! | `mscoree.dll not found, IL-only binary ... cannot be loaded` | `dotnet-runtime-missing` |
//! | `Loading library X (which is needed by ...) failed` | `dll-load-failed` |
//! | `"X" failed to initialize, aborting` | `dll-init-failed` |
//! | `unimplemented function X.Y` | `unimplemented-function` |
//! | `Unhandled exception`, `Unhandled page fault`, `starting debugger` | `crash` |
//! | `wine: could not load kernel32.dll`, `wine: cannot find`, ... | `prefix-unusable` |
//!
//! Without such a line the class comes from what was observed: no window
//! where one was expected, a non-zero exit status, a run that never ended,
//! or a functional check that did not hold.
//!
//! Classification reads; it never runs anything.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The most evidence lines a diagnosis keeps.
const MAX_EVIDENCE_LINES: usize = 12;
/// The longest evidence line kept, in characters.
const MAX_LINE_CHARS: usize = 300;

/// A stable class of launch failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureClass {
    /// Wine could not start in the prefix at all.
    PrefixUnusable,
    /// A DLL the program imports was not found or is disabled.
    DllMissing,
    /// A DLL the program imports belongs to a Microsoft redistributable
    /// this Wine does not implement. TUR cannot supply it.
    MicrosoftComponentMissing,
    /// The program is a .NET program and no .NET runtime is there.
    DotnetRuntimeMissing,
    /// A DLL was found and could not be loaded.
    DllLoadFailed,
    /// A DLL was loaded and its initialisation failed.
    DllInitFailed,
    /// The program called a function Wine does not implement.
    UnimplementedFunction,
    /// The program ended with an unhandled exception.
    Crash,
    /// The program exited with a non-zero status and no other evidence.
    ExitedWithError,
    /// A window was expected and none of the program's appeared.
    NoWindow,
    /// The program neither finished nor showed what was expected in time.
    Hung,
    /// The program ran and its functional check did not hold.
    CheckFailed,
}

impl FailureClass {
    /// Every class, in code order.
    pub const ALL: [FailureClass; 12] = [
        FailureClass::PrefixUnusable,
        FailureClass::DllMissing,
        FailureClass::MicrosoftComponentMissing,
        FailureClass::DotnetRuntimeMissing,
        FailureClass::DllLoadFailed,
        FailureClass::DllInitFailed,
        FailureClass::UnimplementedFunction,
        FailureClass::Crash,
        FailureClass::ExitedWithError,
        FailureClass::NoWindow,
        FailureClass::Hung,
        FailureClass::CheckFailed,
    ];

    /// The class's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            FailureClass::PrefixUnusable => "prefix-unusable",
            FailureClass::DllMissing => "dll-missing",
            FailureClass::MicrosoftComponentMissing => "microsoft-component-missing",
            FailureClass::DotnetRuntimeMissing => "dotnet-runtime-missing",
            FailureClass::DllLoadFailed => "dll-load-failed",
            FailureClass::DllInitFailed => "dll-init-failed",
            FailureClass::UnimplementedFunction => "unimplemented-function",
            FailureClass::Crash => "crash",
            FailureClass::ExitedWithError => "exited-with-error",
            FailureClass::NoWindow => "no-window",
            FailureClass::Hung => "hung",
            FailureClass::CheckFailed => "check-failed",
        }
    }

    /// The class from its name.
    pub fn from_name(name: &str) -> Option<FailureClass> {
        FailureClass::ALL
            .into_iter()
            .find(|class| class.as_str() == name)
    }

    /// The stable code of the class, `TUR-WINDOWS-COMPAT-0NN`.
    pub const fn code(self) -> &'static str {
        match self {
            FailureClass::PrefixUnusable => "TUR-WINDOWS-COMPAT-001",
            FailureClass::DllMissing => "TUR-WINDOWS-COMPAT-002",
            FailureClass::MicrosoftComponentMissing => "TUR-WINDOWS-COMPAT-003",
            FailureClass::DotnetRuntimeMissing => "TUR-WINDOWS-COMPAT-004",
            FailureClass::DllLoadFailed => "TUR-WINDOWS-COMPAT-005",
            FailureClass::DllInitFailed => "TUR-WINDOWS-COMPAT-006",
            FailureClass::UnimplementedFunction => "TUR-WINDOWS-COMPAT-007",
            FailureClass::Crash => "TUR-WINDOWS-COMPAT-008",
            FailureClass::ExitedWithError => "TUR-WINDOWS-COMPAT-009",
            FailureClass::NoWindow => "TUR-WINDOWS-COMPAT-010",
            FailureClass::Hung => "TUR-WINDOWS-COMPAT-011",
            FailureClass::CheckFailed => "TUR-WINDOWS-COMPAT-012",
        }
    }

    /// What the class means, in plain language.
    pub const fn describe(self) -> &'static str {
        match self {
            FailureClass::PrefixUnusable => "Wine could not start in the prefix",
            FailureClass::DllMissing => "a DLL the program needs was not found or is disabled",
            FailureClass::MicrosoftComponentMissing => {
                "the program needs a Microsoft component that TUR cannot supply"
            }
            FailureClass::DotnetRuntimeMissing => {
                "the program is a .NET program and no .NET runtime is installed"
            }
            FailureClass::DllLoadFailed => "a DLL the program needs could not be loaded",
            FailureClass::DllInitFailed => "a DLL the program needs failed to initialise",
            FailureClass::UnimplementedFunction => {
                "the program called a function Wine does not implement"
            }
            FailureClass::Crash => "the program crashed with an unhandled exception",
            FailureClass::ExitedWithError => "the program exited with an error status",
            FailureClass::NoWindow => "no window of the program appeared",
            FailureClass::Hung => "the program did not finish in the time it was given",
            FailureClass::CheckFailed => "the program ran and its check did not hold",
        }
    }

    /// Whether the class can only be answered by something TUR does not
    /// supply: the route is then the Windows guest, not a remedy.
    pub const fn is_unsuppliable(self) -> bool {
        matches!(
            self,
            FailureClass::MicrosoftComponentMissing | FailureClass::DotnetRuntimeMissing
        )
    }
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A sign in the evidence or in the program that makes some remedies more
/// likely than others. Hints order remedies; they never decide a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Hint {
    /// The program speaks of the Windows version.
    WindowsVersion,
    /// The program speaks of the screen size or display mode.
    DisplaySize,
    /// The program speaks of DPI or scaling.
    Dpi,
    /// The program's resources are in Japanese.
    LocaleJa,
    /// The program's resources are in Simplified Chinese.
    LocaleZhCn,
    /// The program's resources are in Traditional Chinese.
    LocaleZhTw,
    /// The program's resources are in Korean.
    LocaleKo,
    /// Drive C: holds names that differ only in case.
    CaseCollision,
    /// The program speaks of sound.
    Audio,
    /// The program or Wine speaks of Direct3D, OpenGL or Vulkan.
    Direct3d,
    /// The program speaks of fonts.
    Fonts,
    /// The DLL named in the evidence lies beside the program.
    DllBesideProgram,
}

impl Hint {
    /// The hint's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Hint::WindowsVersion => "windows-version",
            Hint::DisplaySize => "display-size",
            Hint::Dpi => "dpi",
            Hint::LocaleJa => "locale-ja",
            Hint::LocaleZhCn => "locale-zh-cn",
            Hint::LocaleZhTw => "locale-zh-tw",
            Hint::LocaleKo => "locale-ko",
            Hint::CaseCollision => "case-collision",
            Hint::Audio => "audio",
            Hint::Direct3d => "direct3d",
            Hint::Fonts => "fonts",
            Hint::DllBesideProgram => "dll-beside-program",
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ended {
    /// The program exited by itself.
    Exited,
    /// The program was stopped after it had shown what was expected.
    Stopped,
    /// The program was stopped because its time was up.
    TimedOut,
}

/// What one run of a program left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// The exit status, when the program exited by itself.
    pub exit_status: Option<i32>,
    /// How the run ended.
    pub ended: Ended,
    /// Whether a window was expected.
    pub window_required: bool,
    /// Whether a window of the program appeared.
    pub window_seen: bool,
    /// What the program and Wine printed, bounded.
    pub output: String,
    /// The parts of the functional check that did not hold.
    pub check_problems: Vec<String>,
}

/// What is known about the program without running it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticFacts {
    /// The program's file name, such as `app.exe`.
    pub exe_name: String,
    /// The language identifiers of the program's resources.
    pub resource_languages: Vec<u16>,
    /// The DLLs beside the program, in lower case.
    pub dlls_beside_program: Vec<String>,
    /// Examples of names on drive C: that differ only in case.
    pub case_collisions: Vec<String>,
}

/// One Microsoft redistributable family, by the names of its DLLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFamily {
    /// The family's identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The DLL name prefixes. A DLL belongs to the family when its name,
    /// without `.dll`, is a prefix followed by nothing or by a digit.
    pub prefixes: Vec<String>,
}

impl ComponentFamily {
    /// Whether `dll` (lower case, with or without `.dll`) is one of the
    /// family's DLLs.
    pub fn matches(&self, dll: &str) -> bool {
        let base = dll_base(dll);
        self.prefixes.iter().any(|prefix| {
            base.strip_prefix(prefix.as_str()).is_some_and(|rest| {
                rest.is_empty() || rest.as_bytes().first().is_some_and(u8::is_ascii_digit)
            })
        })
    }
}

/// What the evidence says went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnosis {
    /// The failure class.
    pub class: FailureClass,
    /// Its stable code.
    pub code: &'static str,
    /// One sentence.
    pub summary: String,
    /// The DLL the evidence names, in lower case, with `.dll`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dll: Option<String>,
    /// The function the evidence names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The Microsoft component the DLL belongs to, for
    /// `microsoft-component-missing`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// Hints for ordering remedies.
    pub hints: Vec<Hint>,
    /// The lines of evidence the class was read from.
    pub evidence: Vec<String>,
}

/// A DLL name without its directory and `.dll`, in lower case.
pub fn dll_base(name: &str) -> String {
    let name = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    if let Some(base) = lower.strip_suffix(".dll") {
        return base.to_owned();
    }
    lower
}

/// A DLL name as the evidence names it, in lower case with `.dll`.
fn dll_name(name: &str) -> String {
    format!("{}.dll", dll_base(name.trim().trim_matches('"')))
}

/// Whether a line is one of Wine's own messages rather than the program's.
fn is_wine_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("wine:") || trimmed.starts_with("Unhandled exception") {
        return true;
    }
    // Debug channels: `err:module:...` or `0024:err:module:...`.
    let rest = match trimmed.split_once(':') {
        Some((head, rest)) if head.len() <= 8 && head.bytes().all(|b| b.is_ascii_hexdigit()) => {
            rest
        }
        _ => trimmed,
    };
    ["err:", "fixme:", "warn:", "trace:"]
        .iter()
        .any(|channel| rest.starts_with(channel))
}

fn bounded(line: &str) -> String {
    line.trim()
        .chars()
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .take(MAX_LINE_CHARS)
        .collect()
}

/// A failure read from one line of Wine's messages.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LineFailure {
    class: FailureClass,
    dll: Option<String>,
    function: Option<String>,
}

/// The failure one line names, if it names one.
fn line_failure(line: &str) -> Option<LineFailure> {
    let plain = |class| {
        Some(LineFailure {
            class,
            dll: None,
            function: None,
        })
    };
    if line.contains("IL-only binary") {
        return plain(FailureClass::DotnetRuntimeMissing);
    }
    if let Some(at) = line.find("Loading library ")
        && line.contains("(which is needed by")
    {
        let rest = line.get(at + "Loading library ".len()..)?;
        let dll = rest.split(" (which").next()?;
        return Some(LineFailure {
            class: FailureClass::DllLoadFailed,
            dll: Some(dll_name(dll)),
            function: None,
        });
    }
    if let Some(at) = line.find("Library ")
        && line.contains("(which is needed by")
        && line.trim_end().ends_with("not found")
    {
        let rest = line.get(at + "Library ".len()..)?;
        let dll = rest.split(" (which").next()?;
        return Some(LineFailure {
            class: FailureClass::DllMissing,
            dll: Some(dll_name(dll)),
            function: None,
        });
    }
    if let Some(at) = line.find("failed to initialize, aborting") {
        let before = line.get(..at)?.trim_end();
        let quoted = before.strip_suffix('"')?;
        let start = quoted.rfind('"')?;
        let dll = quoted.get(start + 1..)?;
        return Some(LineFailure {
            class: FailureClass::DllInitFailed,
            dll: Some(dll_name(dll)),
            function: None,
        });
    }
    if let Some(at) = line.find("unimplemented function ") {
        let rest = line.get(at + "unimplemented function ".len()..)?;
        let token = rest
            .split([' ', ','])
            .next()
            .unwrap_or_default()
            .trim_end_matches(',');
        let (dll, function) = token.rsplit_once('.')?;
        return Some(LineFailure {
            class: FailureClass::UnimplementedFunction,
            dll: Some(dll_name(dll)),
            function: Some(function.to_owned()),
        });
    }
    if line.contains("Unhandled exception")
        || line.contains("wine: Unhandled")
        || line.contains("starting debugger")
    {
        return plain(FailureClass::Crash);
    }
    let unusable = [
        "wine: could not load kernel32.dll",
        "wine: failed to initialize",
        "wine: could not open working directory",
        "wine: cannot find",
        "is not owned by you",
        "wine: failed to create main module",
        "wine: could not exec",
    ];
    if unusable.iter().any(|text| line.contains(text)) {
        return plain(FailureClass::PrefixUnusable);
    }
    None
}

/// Whether a line is one of Wine's messages that name a failure: a DLL
/// missing or failing, an unimplemented function, a crash, a prefix Wine
/// cannot start in.
pub fn reports_failure(line: &str) -> bool {
    line_failure(line).is_some()
}

/// Whether a line of Wine's says that a process was ended: a crash, an
/// unimplemented function, a DLL that failed to initialise, imports that
/// could not be resolved, a prefix Wine cannot start in. A DLL reported
/// missing on its own is not: Wine says so for optional imports of its own
/// DLLs too, and the line that ends the process follows.
pub fn reports_fatal(line: &str) -> bool {
    if line.contains("Importing dlls for") || line.contains("Initializing dlls for") {
        return true;
    }
    line_failure(line).is_some_and(|found| {
        !matches!(
            found.class,
            FailureClass::DllMissing | FailureClass::DllLoadFailed
        )
    })
}

/// Which line failures win when several are present: the earliest cause.
fn precedence(class: FailureClass) -> u8 {
    match class {
        FailureClass::PrefixUnusable => 0,
        FailureClass::DotnetRuntimeMissing => 1,
        FailureClass::MicrosoftComponentMissing | FailureClass::DllMissing => 2,
        FailureClass::DllLoadFailed => 3,
        FailureClass::DllInitFailed => 4,
        FailureClass::UnimplementedFunction => 5,
        FailureClass::Crash => 6,
        _ => 7,
    }
}

/// Keywords in the program's own output that suggest a remedy, matched in
/// lower case.
const OUTPUT_HINTS: [(Hint, &[&str]); 6] = [
    (
        Hint::WindowsVersion,
        &[
            "windows version",
            "requires windows",
            "operating system",
            "os version",
            "version of windows",
            "windows 11",
            "windows 10",
            "windows 7",
            "windows xp",
            "service pack",
        ],
    ),
    (
        Hint::DisplaySize,
        &[
            "resolution",
            "screen size",
            "screen of",
            "display mode",
            "desktop size",
        ],
    ),
    (Hint::Dpi, &["dpi", "scaling", "logical pixels"]),
    (
        Hint::Audio,
        &["audio", "sound device", "dsound", "winmm", "mmdevapi"],
    ),
    (
        Hint::Direct3d,
        &["direct3d", "d3d9", "d3d11", "opengl", "vulkan", "dxgi"],
    ),
    (Hint::Fonts, &["font"]),
];

/// The hints the output, the program's resources and drive C: give.
pub fn hints(output: &str, facts: &StaticFacts, dll: Option<&str>) -> Vec<Hint> {
    let mut found = Vec::new();
    // What Wine's debugger prints after a crash (registers, a backtrace, the
    // modules, "System information: ... Version: Windows 10") is not the
    // program's own text.
    let program_text: String = output
        .lines()
        .take_while(|line| {
            !line.contains("starting debugger") && !line.contains("Unhandled exception")
        })
        .filter(|line| !is_wine_line(line))
        .collect::<Vec<&str>>()
        .join("\n")
        .to_lowercase();
    for (hint, words) in OUTPUT_HINTS {
        if words.iter().any(|word| program_text.contains(word)) {
            found.push(hint);
        }
    }
    // Wine's own Direct3D and OpenGL channels.
    if !found.contains(&Hint::Direct3d)
        && output.lines().any(|line| {
            is_wine_line(line)
                && (line.contains("err:d3d")
                    || line.contains("err:wgl")
                    || line.contains("err:vulkan"))
        })
    {
        found.push(Hint::Direct3d);
    }
    for language in &facts.resource_languages {
        let hint = match *language {
            0x0411 => Some(Hint::LocaleJa),
            0x0804 | 0x1004 => Some(Hint::LocaleZhCn),
            0x0404 | 0x0c04 | 0x1404 => Some(Hint::LocaleZhTw),
            0x0412 => Some(Hint::LocaleKo),
            _ => None,
        };
        if let Some(hint) = hint
            && !found.contains(&hint)
        {
            found.push(hint);
        }
    }
    if !facts.case_collisions.is_empty() {
        found.push(Hint::CaseCollision);
    }
    if let Some(dll) = dll
        && facts
            .dlls_beside_program
            .iter()
            .any(|beside| dll_base(beside) == dll_base(dll))
    {
        found.push(Hint::DllBesideProgram);
    }
    found.sort();
    found.dedup();
    found
}

/// Reads the failure from the evidence of a run that did not pass.
///
/// `has_builtin` says whether this Wine implements a DLL; `families` are the
/// Microsoft redistributables a missing DLL may belong to.
pub fn diagnose(
    evidence: &Evidence,
    facts: &StaticFacts,
    has_builtin: &dyn Fn(&str) -> bool,
    families: &[ComponentFamily],
) -> Diagnosis {
    let mut best: Option<(LineFailure, String)> = None;
    let mut lines = Vec::new();
    for line in evidence.output.lines() {
        if let Some(found) = line_failure(line) {
            if lines.len() < MAX_EVIDENCE_LINES {
                lines.push(bounded(line));
            }
            let better = best
                .as_ref()
                .is_none_or(|(current, _)| precedence(found.class) < precedence(current.class));
            if better {
                best = Some((found, bounded(line)));
            }
        }
    }

    let (mut class, dll, function, mut component) = match best {
        Some((found, _)) => (found.class, found.dll, found.function, None),
        None => {
            let class = if evidence.window_required
                && !evidence.window_seen
                && evidence.ended != Ended::Stopped
            {
                FailureClass::NoWindow
            } else if evidence.ended == Ended::TimedOut {
                FailureClass::Hung
            } else if evidence.exit_status.is_some_and(|status| status != 0) {
                FailureClass::ExitedWithError
            } else {
                FailureClass::CheckFailed
            };
            (class, None, None, None)
        }
    };

    // A missing DLL that belongs to a Microsoft redistributable this Wine
    // does not implement cannot be helped by any remedy.
    if class == FailureClass::DllMissing
        && let Some(name) = &dll
        && !has_builtin(name)
        && let Some(family) = families.iter().find(|family| family.matches(name))
    {
        class = if family.id == "dotnet-framework" {
            FailureClass::DotnetRuntimeMissing
        } else {
            FailureClass::MicrosoftComponentMissing
        };
        component = Some(family.name.clone());
    }

    if lines.is_empty() {
        if let Some(status) = evidence.exit_status {
            lines.push(format!("exit status {status}"));
        }
        for problem in evidence.check_problems.iter().take(MAX_EVIDENCE_LINES) {
            lines.push(bounded(problem));
        }
        // The program's own last words, which often say why.
        let own: Vec<&str> = evidence
            .output
            .lines()
            .filter(|line| !line.trim().is_empty() && !is_wine_line(line))
            .collect();
        for line in own.iter().rev().take(3).rev() {
            if lines.len() < MAX_EVIDENCE_LINES {
                lines.push(bounded(line));
            }
        }
    }

    let summary = match (&dll, &function, &component) {
        (Some(dll), _, Some(component)) => format!(
            "{}: {dll} belongs to {component}, which this Wine does not implement",
            class.describe()
        ),
        (Some(dll), Some(function), None) => {
            format!("{}: {dll} {function}", class.describe())
        }
        (Some(dll), None, None) => format!("{}: {dll}", class.describe()),
        _ => class.describe().to_owned(),
    };
    Diagnosis {
        class,
        code: class.code(),
        summary,
        hints: hints(&evidence.output, facts, dll.as_deref()),
        dll,
        function,
        component,
        evidence: lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(output: &str) -> Evidence {
        Evidence {
            exit_status: Some(1),
            ended: Ended::Exited,
            window_required: false,
            window_seen: false,
            output: output.to_owned(),
            check_problems: Vec::new(),
        }
    }

    fn families() -> Vec<ComponentFamily> {
        vec![
            ComponentFamily {
                id: "visual-cpp-mfc".to_owned(),
                name: "Microsoft Foundation Classes".to_owned(),
                prefixes: vec!["mfc".to_owned()],
            },
            ComponentFamily {
                id: "dotnet-framework".to_owned(),
                name: ".NET Framework".to_owned(),
                prefixes: vec!["mscoree".to_owned()],
            },
        ]
    }

    fn builtin(dll: &str) -> bool {
        ["comctl32", "d3dx9_43", "kernel32"].contains(&dll_base(dll).as_str())
    }

    fn diagnosis(output: &str) -> Diagnosis {
        diagnose(
            &evidence(output),
            &StaticFacts::default(),
            &builtin,
            &families(),
        )
    }

    #[test]
    fn every_class_has_a_unique_valid_code() {
        let mut codes = std::collections::BTreeSet::new();
        for class in FailureClass::ALL {
            let parsed: tur_core::ErrorCode = class.code().parse().unwrap();
            assert_eq!(parsed.as_str(), class.code());
            assert!(codes.insert(class.code()));
            assert_eq!(FailureClass::from_name(class.as_str()), Some(class));
            let written = serde_json::to_value(class).unwrap();
            assert_eq!(written, class.as_str());
        }
        let numbers: Vec<String> = FailureClass::ALL
            .iter()
            .map(|class| class.code().to_owned())
            .collect();
        let expected: Vec<String> = (1..=12)
            .map(|number| format!("TUR-WINDOWS-COMPAT-{number:03}"))
            .collect();
        assert_eq!(numbers, expected);
    }

    #[test]
    fn wine_loader_messages_are_classified() {
        let missing = diagnosis(
            "0024:err:module:import_dll Library tur_own.dll (which is needed by L\"C:\\\\App\\\\app.exe\") not found\n\
             0024:err:module:LdrInitializeThunk Importing dlls for L\"C:\\\\App\\\\app.exe\" failed, status c0000135\n",
        );
        assert_eq!(missing.class, FailureClass::DllMissing);
        assert_eq!(missing.dll.as_deref(), Some("tur_own.dll"));
        assert_eq!(missing.code, "TUR-WINDOWS-COMPAT-002");
        assert_eq!(missing.evidence.len(), 1);

        let disabled = diagnosis(
            "err:module:import_dll Library COMCTL32.dll (which is needed by L\"C:\\\\x.exe\") not found",
        );
        assert_eq!(disabled.class, FailureClass::DllMissing);
        assert_eq!(disabled.dll.as_deref(), Some("comctl32.dll"));

        let init = diagnosis(
            "0024:err:module:LdrInitializeThunk \"d3dx9_43.dll\" failed to initialize, aborting\n\
             0024:err:module:LdrInitializeThunk Initializing dlls for L\"C:\\\\a.exe\" failed, status c0000142",
        );
        assert_eq!(init.class, FailureClass::DllInitFailed);
        assert_eq!(init.dll.as_deref(), Some("d3dx9_43.dll"));

        let load = diagnosis(
            "err:module:import_dll Loading library foo.dll (which is needed by L\"C:\\\\a.exe\") failed (error c000007b).",
        );
        assert_eq!(load.class, FailureClass::DllLoadFailed);
        assert_eq!(load.dll.as_deref(), Some("foo.dll"));

        let stub = diagnosis(
            "wine: Call from 00006FFFFFC3E7AB to unimplemented function api-ms-win-core-foo.dll.Bar, aborting",
        );
        assert_eq!(stub.class, FailureClass::UnimplementedFunction);
        assert_eq!(stub.dll.as_deref(), Some("api-ms-win-core-foo.dll"));
        assert_eq!(stub.function.as_deref(), Some("Bar"));

        let crash = diagnosis(
            "wine: Unhandled page fault on write access to 0000000000000000 at address 0000000140001234 (thread 0024), starting debugger...\n\
             Unhandled exception: page fault on write access to 0x0000000000000000 in 64-bit code (0x0000000140001234).",
        );
        assert_eq!(crash.class, FailureClass::Crash);

        let dotnet = diagnosis(
            "err:module:load_dll mscoree.dll not found, IL-only binary L\"a.exe\" cannot be loaded",
        );
        assert_eq!(dotnet.class, FailureClass::DotnetRuntimeMissing);

        let unusable = diagnosis("wine: could not load kernel32.dll, status c0000135");
        assert_eq!(unusable.class, FailureClass::PrefixUnusable);
    }

    #[test]
    fn the_earliest_cause_wins() {
        let both = diagnosis(
            "wine: Unhandled page fault on read access, starting debugger...\n\
             err:module:import_dll Library a.dll (which is needed by L\"x\") not found",
        );
        assert_eq!(both.class, FailureClass::DllMissing);
        assert_eq!(both.evidence.len(), 2);
    }

    #[test]
    fn a_microsoft_redistributable_this_wine_lacks_is_named_and_not_remedied() {
        let mfc = diagnosis(
            "err:module:import_dll Library mfc140u.dll (which is needed by L\"C:\\\\a.exe\") not found",
        );
        assert_eq!(mfc.class, FailureClass::MicrosoftComponentMissing);
        assert!(mfc.class.is_unsuppliable());
        assert_eq!(
            mfc.component.as_deref(),
            Some("Microsoft Foundation Classes")
        );
        // A name that only starts like a family is not one of its DLLs.
        let own = diagnosis(
            "err:module:import_dll Library mfcustom.dll (which is needed by L\"C:\\\\a.exe\") not found",
        );
        assert_eq!(own.class, FailureClass::DllMissing);
        // A DLL of a family that this Wine implements is an ordinary
        // missing DLL: the remedy is to use Wine's.
        let family = ComponentFamily {
            id: "directx-redistributable".to_owned(),
            name: "DirectX".to_owned(),
            prefixes: vec!["d3dx".to_owned()],
        };
        assert!(family.matches("d3dx9_43.dll"));
        assert!(family.matches("D3DX9_43"));
        let implemented = diagnose(
            &evidence(
                "err:module:import_dll Library d3dx9_43.dll (which is needed by L\"x\") not found",
            ),
            &StaticFacts::default(),
            &builtin,
            &[family],
        );
        assert_eq!(implemented.class, FailureClass::DllMissing);
        let dotnet = diagnosis(
            "err:module:import_dll Library mscoree.dll (which is needed by L\"x\") not found",
        );
        assert_eq!(dotnet.class, FailureClass::DotnetRuntimeMissing);
    }

    #[test]
    fn without_a_wine_message_the_class_comes_from_what_was_observed() {
        let facts = StaticFacts::default();
        let none = |_: &str| false;
        let mut seen = evidence("This program requires Windows 11 or later.\n");
        assert_eq!(
            diagnose(&seen, &facts, &none, &[]).class,
            FailureClass::ExitedWithError
        );
        seen.exit_status = Some(0);
        seen.check_problems = vec!["the output does not contain \"PASS\"".to_owned()];
        let checked = diagnose(&seen, &facts, &none, &[]);
        assert_eq!(checked.class, FailureClass::CheckFailed);
        assert!(checked.evidence.iter().any(|line| line.contains("PASS")));
        seen.window_required = true;
        assert_eq!(
            diagnose(&seen, &facts, &none, &[]).class,
            FailureClass::NoWindow
        );
        seen.window_required = false;
        seen.exit_status = None;
        seen.ended = Ended::TimedOut;
        assert_eq!(
            diagnose(&seen, &facts, &none, &[]).class,
            FailureClass::Hung
        );
    }

    #[test]
    fn hints_come_from_the_program_not_from_wine() {
        let facts = StaticFacts {
            exe_name: "a.exe".to_owned(),
            resource_languages: vec![0x0411, 0x0409],
            dlls_beside_program: vec!["d3dx9_43.dll".to_owned()],
            case_collisions: vec!["Plugins and plugins".to_owned()],
        };
        let output = "0024:err:winediag:ALSA sound device not found\n\
                      This program requires Windows 11 or later.\n\
                      Display resolution 1280x800 is not supported\n";
        let found = hints(output, &facts, Some("D3DX9_43.dll"));
        assert!(found.contains(&Hint::WindowsVersion));
        assert!(found.contains(&Hint::DisplaySize));
        assert!(found.contains(&Hint::LocaleJa));
        assert!(found.contains(&Hint::CaseCollision));
        assert!(found.contains(&Hint::DllBesideProgram));
        // "sound device" is only in a line of Wine's.
        assert!(!found.contains(&Hint::Audio));
        assert!(is_wine_line("0024:fixme:ntdll:Foo"));
        assert!(is_wine_line("err:module:import_dll x"));
        assert!(!is_wine_line("error: the screen is too large"));
    }
}

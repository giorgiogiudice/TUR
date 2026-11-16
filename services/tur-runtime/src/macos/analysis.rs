// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Whether Darling can be expected to start a macOS program, from the
//! program's own bytes: its architectures, the platform it was built for,
//! whether it is encrypted, and which libraries it links (Swift, SwiftUI,
//! Apple's private frameworks).
//!
//! The answer is a list of findings, each with a stable reason, a severity
//! and, for refusals and warnings, a code of the `TUR-RUNTIME-MACOS` topic.
//! It never says that a program works: only a recorded run says that. It
//! says when a program certainly cannot start, and what stands against it.
//!
//! The facts behind the refusals were read in Darling's own repositories on
//! 16 November 2026 (docs/reports/exp-macfw-0.md, section 2;
//! docs/reports/macos-darling-1.md):
//!
//! * Darling runs x86-64 (and i386) Mach-O programs on an x86-64 PC and
//!   arm64 programs only on an arm64 Linux machine; nothing translates
//!   arm64 code to x86-64.
//! * SwiftUI is absent from Darling (darlinghq/darling#2059).
//! * Darling takes the Swift runtime unmodified from Apple's swift.org
//!   package. TUR never ships Apple binaries, so TUR's build of Darling
//!   leaves it out ([`SWIFT_RUNTIME_DIRECTORY`]), and a program that links
//!   the system's Swift runtime cannot load.

use std::fs::{self, File};
use std::path::Path;

use serde::Serialize;

use super::loadcmd::{LoadCommands, read_load_commands};
use crate::arch::Arch;
use crate::bundle::AppBundleInfo;
use crate::codes::Code;
use crate::error::InspectError;
use crate::inspect::{Detected, inspect};
use crate::macho::{MachOFileType, MachOInfo, MachOSlice};
use crate::reader::Reader;

/// Where macOS keeps the Swift runtime, and where Darling's guest system
/// would have it. TUR's build of Darling has nothing there.
pub const SWIFT_RUNTIME_DIRECTORY: &str = "/usr/lib/swift/";

/// The most framework names that are listed.
const MAX_LISTED_FRAMEWORKS: usize = 128;

/// How much a finding weighs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The program cannot run in Darling. Nothing overrides this.
    Refusal,
    /// The program is not expected to run; it is refused unless the person
    /// allows it (`--allow-swift`).
    RefusedUnlessAllowed,
    /// Something that may stop the program or part of it.
    Warning,
    /// Worth knowing.
    Note,
}

/// One thing found about a program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// A stable identifier of the finding, such as `apple-silicon-only`.
    pub reason: &'static str,
    /// How much it weighs.
    pub severity: Severity,
    /// The code reported when the finding refuses the program or warns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Code>,
    /// The finding in plain language.
    pub message: String,
}

impl Finding {
    fn new(reason: &'static str, severity: Severity, code: Option<Code>, message: String) -> Self {
        Finding {
            reason,
            severity,
            code,
            message,
        }
    }

    /// Whether the finding stops the program from being installed.
    pub fn refuses(&self) -> bool {
        matches!(
            self.severity,
            Severity::Refusal | Severity::RefusedUnlessAllowed
        )
    }
}

/// The overall answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Nothing known stands against running it in Darling. Not a promise
    /// that it works.
    Runnable,
    /// It can be tried; the warnings say what may stop it.
    RunnableWithWarnings,
    /// It is refused; the first refusing finding says why.
    Refused,
}

/// What a program's analysis is made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// The processor of this computer.
    pub host: Arch,
    /// The person allows a Swift application that embeds its own Swift
    /// runtime to be tried.
    pub allow_swift: bool,
}

/// What was found about one Mach-O program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgramAnalysis {
    /// The architectures of the file's images, in file order.
    pub architectures: Vec<Arch>,
    /// Whether the file is universal.
    pub universal: bool,
    /// The image Darling would run on this computer, when there is one.
    pub slice: Option<Arch>,
    /// The platform that image was built for.
    pub platform: Option<String>,
    /// The oldest macOS it says it runs on.
    pub minimum_macos: Option<String>,
    /// The SDK it was built with.
    pub sdk: Option<String>,
    /// Whether it contains or links Swift code.
    pub swift: bool,
    /// Whether it links SwiftUI.
    pub swiftui: bool,
    /// Apple's public frameworks it links, by name, sorted.
    pub frameworks: Vec<String>,
    /// Apple's private frameworks it links, by name, sorted.
    pub private_frameworks: Vec<String>,
    /// The load commands of the chosen image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_commands: Option<LoadCommands>,
    /// What was found.
    pub findings: Vec<Finding>,
    /// The overall answer.
    pub verdict: Verdict,
}

impl ProgramAnalysis {
    /// The first finding that refuses the program.
    pub fn refusal(&self) -> Option<&Finding> {
        self.findings.iter().find(|finding| finding.refuses())
    }
}

/// The verdict a list of findings comes to.
pub fn verdict_of(findings: &[Finding]) -> Verdict {
    if findings.iter().any(Finding::refuses) {
        Verdict::Refused
    } else if findings
        .iter()
        .any(|finding| finding.severity == Severity::Warning)
    {
        Verdict::RunnableWithWarnings
    } else {
        Verdict::Runnable
    }
}

/// Analyses the Mach-O program at `path`.
///
/// # Errors
///
/// [`InspectError`] when the file cannot be read, or when it is a Mach-O
/// file that is truncated, malformed or over a limit. A file that is not a
/// Mach-O program at all is an analysis with a refusal, not an error.
pub fn analyse_program(
    path: &Path,
    options: &AnalysisOptions,
) -> Result<ProgramAnalysis, InspectError> {
    let inspection = inspect(path)?;
    let Detected::MachO(info) = inspection.detected else {
        return Ok(not_mach_o());
    };
    let file = File::open(path).map_err(|error| InspectError::io(path, &error))?;
    let size = file
        .metadata()
        .map_err(|error| InspectError::io(path, &error))?
        .len();
    let mut reader = Reader::from_file(file, size);
    analyse(&info, |slice| read_load_commands(&mut reader, slice), options)
}

/// Analyses a Mach-O program held in memory.
///
/// # Errors
///
/// As [`analyse_program`].
pub fn analyse_bytes(
    bytes: &[u8],
    options: &AnalysisOptions,
) -> Result<ProgramAnalysis, InspectError> {
    let inspection = crate::inspect::inspect_bytes(bytes)?;
    let Detected::MachO(info) = inspection.detected else {
        return Ok(not_mach_o());
    };
    let mut reader = Reader::from_bytes(bytes);
    analyse(&info, |slice| read_load_commands(&mut reader, slice), options)
}

fn not_mach_o() -> ProgramAnalysis {
    let findings = vec![Finding::new(
        "not-mach-o",
        Severity::Refusal,
        Some(Code::MacosNotMacSoftware),
        "The file is not a macOS program (Mach-O).".to_owned(),
    )];
    ProgramAnalysis {
        architectures: Vec::new(),
        universal: false,
        slice: None,
        platform: None,
        minimum_macos: None,
        sdk: None,
        swift: false,
        swiftui: false,
        frameworks: Vec::new(),
        private_frameworks: Vec::new(),
        load_commands: None,
        verdict: verdict_of(&findings),
        findings,
    }
}

fn arch_list(archs: &[Arch]) -> String {
    let names: Vec<&str> = archs.iter().map(|arch| arch.as_str()).collect();
    names.join(", ")
}

/// The analysis, given the file's images and a way to read an image's load
/// commands.
fn analyse(
    info: &MachOInfo,
    mut read: impl FnMut(&MachOSlice) -> Result<LoadCommands, InspectError>,
    options: &AnalysisOptions,
) -> Result<ProgramAnalysis, InspectError> {
    let architectures = info.architectures();
    let mut findings = Vec::new();
    let mut analysis = ProgramAnalysis {
        architectures: architectures.clone(),
        universal: info.universal,
        slice: None,
        platform: None,
        minimum_macos: None,
        sdk: None,
        swift: false,
        swiftui: false,
        frameworks: Vec::new(),
        private_frameworks: Vec::new(),
        load_commands: None,
        findings: Vec::new(),
        verdict: Verdict::Refused,
    };

    if info.has_kext() {
        findings.push(Finding::new(
            "kernel-extension",
            Severity::Refusal,
            Some(Code::DarwinKext),
            "It is a macOS kernel extension. TUR never loads macOS kernel code.".to_owned(),
        ));
        analysis.verdict = verdict_of(&findings);
        analysis.findings = findings;
        return Ok(analysis);
    }

    let chosen = info.slices.iter().find(|slice| {
        slice.arch == options.host && slice.file_type == MachOFileType::Executable
    });
    let Some(slice) = chosen else {
        findings.push(no_slice_finding(info, options.host));
        analysis.verdict = verdict_of(&findings);
        analysis.findings = findings;
        return Ok(analysis);
    };
    analysis.slice = Some(slice.arch);

    let commands = read(slice)?;
    analysis.platform = commands.platform.clone();
    analysis.minimum_macos = commands.minimum_os.clone();
    analysis.sdk = commands.sdk.clone();

    match commands.platform.as_deref() {
        None | Some("macos") => {}
        Some("mac-catalyst") => findings.push(Finding::new(
            "mac-catalyst",
            Severity::RefusedUnlessAllowed,
            Some(Code::MacosForeignPlatform),
            "It is an iPad application built for the Mac (Mac Catalyst). It needs Apple's iPad \
             frameworks for the Mac, which Darling is not known to provide."
                .to_owned(),
        )),
        Some(other) => findings.push(Finding::new(
            "not-macos",
            Severity::Refusal,
            Some(Code::MacosForeignPlatform),
            format!("It is built for {other}, not for macOS."),
        )),
    }

    if commands.encrypted {
        findings.push(Finding::new(
            "encrypted",
            Severity::Refusal,
            Some(Code::MacosEncrypted),
            "Its code is encrypted, as Mac App Store copies are, and only macOS can decrypt it."
                .to_owned(),
        ));
    }

    let mut frameworks = Vec::new();
    let mut private = Vec::new();
    let mut system_swift = false;
    let mut embedded_swift = false;
    let mut swiftui = None;
    for library in &commands.libraries {
        let path = library.path.as_str();
        if let Some(name) = framework_name(path, "/System/Library/Frameworks/") {
            if name == "SwiftUI" || name == "SwiftUICore" {
                let required = swiftui.unwrap_or(false) || library.kind.required();
                swiftui = Some(required);
            }
            if frameworks.len() < MAX_LISTED_FRAMEWORKS && !frameworks.contains(&name) {
                frameworks.push(name);
            }
        } else if let Some(name) = framework_name(path, "/System/Library/PrivateFrameworks/") {
            if private.len() < MAX_LISTED_FRAMEWORKS && !private.contains(&name) {
                private.push(name);
            }
        } else if is_swift_library(path) {
            if path.starts_with(SWIFT_RUNTIME_DIRECTORY) {
                system_swift |= library.kind.required();
            } else {
                embedded_swift = true;
            }
        }
    }
    frameworks.sort();
    private.sort();
    let swift_code = !commands.swift_sections.is_empty();
    analysis.swift = system_swift || embedded_swift || swift_code;
    analysis.swiftui = swiftui.is_some();

    match swiftui {
        Some(true) => findings.push(Finding::new(
            "swiftui",
            Severity::Refusal,
            Some(Code::MacosSwiftUi),
            "It is built with SwiftUI, which Darling does not implement (darlinghq/darling#2059): \
             it cannot start."
                .to_owned(),
        )),
        Some(false) => findings.push(Finding::new(
            "swiftui-optional",
            Severity::Warning,
            Some(Code::MacosSwiftUi),
            "It links SwiftUI weakly. Darling has no SwiftUI, so any part of it written with \
             SwiftUI will fail."
                .to_owned(),
        )),
        None => {}
    }
    if system_swift {
        findings.push(Finding::new(
            "swift-system-runtime",
            Severity::Refusal,
            Some(Code::MacosSwift),
            "It is a Swift application that uses macOS's own Swift runtime. TUR's build of \
             Darling does not include that runtime (Darling takes it unmodified from Apple's \
             swift.org package, and TUR never ships Apple binaries), and Swift applications from \
             current Xcode do not run in Darling yet (darlinghq/darling#2059)."
                .to_owned(),
        ));
    } else if embedded_swift || swift_code {
        let (severity, ending) = if options.allow_swift {
            (Severity::Warning, "It is tried because you allowed it.")
        } else {
            (
                Severity::RefusedUnlessAllowed,
                "It is refused unless you allow it (--allow-swift).",
            )
        };
        findings.push(Finding::new(
            "swift",
            severity,
            Some(Code::MacosSwift),
            format!(
                "It contains Swift code with its own copy of the Swift runtime, from before macOS \
                 had one. Swift applications are not known to run in Darling. {ending}"
            ),
        ));
    }
    if !private.is_empty() {
        findings.push(Finding::new(
            "private-frameworks",
            Severity::Warning,
            None,
            format!(
                "It uses Apple's private frameworks ({}), which Darling is unlikely to provide in \
                 full.",
                private.join(", ")
            ),
        ));
    }
    if !commands.entry_point {
        findings.push(Finding::new(
            "no-entry-point",
            Severity::Warning,
            None,
            "Its load commands name no entry point.".to_owned(),
        ));
    }
    if let Some(minimum) = &commands.minimum_os {
        findings.push(Finding::new(
            "minimum-macos",
            Severity::Note,
            None,
            format!(
                "It says it needs macOS {minimum} or later. Darling reports one fixed macOS \
                 version, set when it was built."
            ),
        ));
    }
    if architectures.len() > 1 {
        findings.push(Finding::new(
            "universal",
            Severity::Note,
            None,
            format!(
                "It holds programs for {}; the {} one is used.",
                arch_list(&architectures),
                slice.arch.as_str()
            ),
        ));
    }

    analysis.frameworks = frameworks;
    analysis.private_frameworks = private;
    analysis.load_commands = Some(commands);
    analysis.verdict = verdict_of(&findings);
    analysis.findings = findings;
    Ok(analysis)
}

/// The finding for a file with no program for this computer's processor.
fn no_slice_finding(info: &MachOInfo, host: Arch) -> Finding {
    let executables: Vec<Arch> = info
        .slices
        .iter()
        .filter(|slice| slice.file_type == MachOFileType::Executable)
        .map(|slice| slice.arch)
        .collect();
    if executables.is_empty() {
        let kinds: Vec<&str> = info
            .slices
            .iter()
            .map(|slice| slice.file_type.label())
            .collect();
        return Finding::new(
            "not-a-program",
            Severity::Refusal,
            Some(Code::MacosNoRunnableSlice),
            format!(
                "It is not a program: the file holds {}.",
                if kinds.is_empty() {
                    "nothing".to_owned()
                } else {
                    kinds.join(", ")
                }
            ),
        );
    }
    if host == Arch::X86_64 && executables.iter().all(|arch| *arch == Arch::Aarch64) {
        return Finding::new(
            "apple-silicon-only",
            Severity::Refusal,
            Some(Code::MacosArm64Only),
            "It is built only for Apple silicon (arm64). Darling runs only programs built for the \
             computer's own processor, this computer is x86-64, and nothing translates arm64 code \
             for it. A version of the application for Intel Macs, or a universal one, would run."
                .to_owned(),
        );
    }
    let mut message = format!(
        "It is built for {}, and Darling on this computer runs {} programs only.",
        arch_list(&executables),
        host.as_str()
    );
    if executables.contains(&Arch::I386) {
        message.push_str(
            " 32-bit Intel programs are not supported: TUR's Darling is built without 32-bit \
             support, and macOS itself stopped running them in 2019.",
        );
    }
    Finding::new(
        "no-program-for-this-processor",
        Severity::Refusal,
        Some(Code::MacosNoRunnableSlice),
        message,
    )
}

/// `Name` from `<prefix>Name.framework/...`.
fn framework_name(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let (directory, _) = rest.split_once('/')?;
    let name = directory.strip_suffix(".framework")?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// Whether a library path names a Swift runtime or overlay library
/// (`libswiftCore.dylib`, `libswiftAppKit.dylib` and the like).
fn is_swift_library(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("libswift") && name.ends_with(".dylib"))
}

/// What was found about an application bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppAnalysis {
    /// The bundle's directory name, such as `Example.app`.
    pub bundle: String,
    /// `CFBundleIdentifier`.
    pub identifier: Option<String>,
    /// The application's name.
    pub name: Option<String>,
    /// Its version.
    pub version: Option<String>,
    /// The main program, relative to the bundle: `Contents/MacOS/<name>`.
    pub executable: Option<String>,
    /// The main program's analysis.
    pub program: Option<ProgramAnalysis>,
    /// Findings about the bundle itself, then the program's.
    pub findings: Vec<Finding>,
    /// The overall answer.
    pub verdict: Verdict,
}

impl AppAnalysis {
    /// The first finding that refuses the application.
    pub fn refusal(&self) -> Option<&Finding> {
        self.findings.iter().find(|finding| finding.refuses())
    }
}

/// Analyses the application bundle at `directory`.
///
/// # Errors
///
/// [`InspectError`] when the bundle or its main program cannot be read or
/// is malformed.
pub fn analyse_bundle(
    directory: &Path,
    options: &AnalysisOptions,
) -> Result<AppAnalysis, InspectError> {
    let inspection = inspect(directory)?;
    let bundle = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_owned();
    let Detected::MacAppBundle(info) = inspection.detected else {
        let findings = vec![Finding::new(
            "not-an-application-bundle",
            Severity::Refusal,
            Some(Code::MacosNotMacSoftware),
            "The directory is not a macOS application bundle.".to_owned(),
        )];
        return Ok(AppAnalysis {
            bundle,
            identifier: None,
            name: None,
            version: None,
            executable: None,
            program: None,
            verdict: verdict_of(&findings),
            findings,
        });
    };
    analyse_bundle_info(directory, bundle, &info, options)
}

fn analyse_bundle_info(
    directory: &Path,
    bundle: String,
    info: &AppBundleInfo,
    options: &AnalysisOptions,
) -> Result<AppAnalysis, InspectError> {
    let mut findings = Vec::new();
    let (executable, program) = match &info.executable_name {
        Some(name) => {
            let path = directory.join("Contents").join("MacOS").join(name);
            let analysis = analyse_program(&path, options)?;
            (Some(format!("Contents/MacOS/{name}")), Some(analysis))
        }
        None => {
            findings.push(Finding::new(
                "no-main-program",
                Severity::Refusal,
                Some(Code::MacosNoRunnableSlice),
                "The bundle has no main program that could be found.".to_owned(),
            ));
            (None, None)
        }
    };
    if fs::symlink_metadata(directory.join("Contents").join("Frameworks")).is_ok() {
        findings.push(Finding::new(
            "embedded-frameworks-not-examined",
            Severity::Note,
            None,
            "The frameworks inside the bundle were not examined; one built for another \
             processor would stop the application when it is loaded."
                .to_owned(),
        ));
    }
    if let Some(program) = &program {
        findings.extend(program.findings.iter().cloned());
    }
    Ok(AppAnalysis {
        bundle,
        identifier: info.identifier.clone(),
        name: info.name.clone(),
        version: info.version.clone(),
        executable,
        program,
        verdict: verdict_of(&findings),
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::super::loadcmd::LinkKind;
    use super::super::loadcmd::tests::{Command, image, universal};
    use super::*;
    use crate::testdata::TempDir;

    const X86_64: u32 = 0x0100_0007;
    const ARM64: u32 = 0x0100_000c;
    const I386: u32 = 7;
    const APPKIT: &str = "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit";
    const FOUNDATION: &str =
        "/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation";
    const SWIFTUI: &str = "/System/Library/Frameworks/SwiftUI.framework/Versions/A/SwiftUI";
    const SWIFT_CORE: &str = "/usr/lib/swift/libswiftCore.dylib";

    fn options() -> AnalysisOptions {
        AnalysisOptions {
            host: Arch::X86_64,
            allow_swift: false,
        }
    }

    fn appkit_program(cpu: u32) -> Vec<u8> {
        image(
            cpu,
            2,
            &[
                Command::Segment64(&["__text"]),
                Command::Build(1, 0x000b_0000, 0x001a_0000),
                Command::Main,
                Command::Dylib(LinkKind::Load, APPKIT),
                Command::Dylib(LinkKind::Load, FOUNDATION),
                Command::Dylib(LinkKind::Load, "/usr/lib/libobjc.A.dylib"),
            ],
        )
    }

    fn reasons(analysis: &ProgramAnalysis) -> Vec<&'static str> {
        analysis
            .findings
            .iter()
            .map(|finding| finding.reason)
            .collect()
    }

    #[test]
    fn an_objective_c_appkit_program_for_intel_is_runnable() {
        let analysis = analyse_bytes(&appkit_program(X86_64), &options()).unwrap();
        assert_eq!(analysis.verdict, Verdict::Runnable);
        assert_eq!(analysis.slice, Some(Arch::X86_64));
        assert_eq!(analysis.frameworks, ["AppKit", "Foundation"]);
        assert!(!analysis.swift);
        assert!(!analysis.swiftui);
        assert_eq!(analysis.minimum_macos.as_deref(), Some("11.0"));
        assert_eq!(analysis.sdk.as_deref(), Some("26.0"));
        assert!(analysis.refusal().is_none());
    }

    #[test]
    fn a_universal_program_runs_its_intel_slice() {
        let bytes = universal(&[
            (ARM64, appkit_program(ARM64)),
            (X86_64, appkit_program(X86_64)),
        ]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.architectures, [Arch::Aarch64, Arch::X86_64]);
        assert!(analysis.universal);
        assert_eq!(analysis.slice, Some(Arch::X86_64));
        assert_eq!(analysis.verdict, Verdict::Runnable);
        assert!(reasons(&analysis).contains(&"universal"));
    }

    #[test]
    fn an_apple_silicon_only_program_is_refused_with_a_plain_reason() {
        let analysis = analyse_bytes(&appkit_program(ARM64), &options()).unwrap();
        assert_eq!(analysis.verdict, Verdict::Refused);
        let refusal = analysis.refusal().unwrap();
        assert_eq!(refusal.reason, "apple-silicon-only");
        assert_eq!(refusal.code, Some(Code::MacosArm64Only));
        assert!(refusal.message.contains("Apple silicon"));
        assert!(refusal.message.contains("Intel"));

        // The same file on an arm64 machine has a program for it.
        let arm = AnalysisOptions {
            host: Arch::Aarch64,
            allow_swift: false,
        };
        let analysis = analyse_bytes(&appkit_program(ARM64), &arm).unwrap();
        assert_eq!(analysis.slice, Some(Arch::Aarch64));
    }

    #[test]
    fn thirty_two_bit_and_non_programs_are_refused() {
        let bytes = image(I386, 2, &[Command::Main]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        let refusal = analysis.refusal().unwrap();
        assert_eq!(refusal.code, Some(Code::MacosNoRunnableSlice));
        assert!(refusal.message.contains("32-bit"));

        let bytes = image(X86_64, 6, &[]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().reason, "not-a-program");

        let bytes = image(X86_64, 0xb, &[]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().code, Some(Code::DarwinKext));

        let analysis = analyse_bytes(b"#!/bin/sh\necho hi\n", &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().code, Some(Code::MacosNotMacSoftware));
    }

    #[test]
    fn swiftui_is_refused_and_weak_swiftui_is_a_warning() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Main,
                Command::Dylib(LinkKind::Load, SWIFTUI),
                Command::Dylib(LinkKind::Load, SWIFT_CORE),
            ],
        );
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert!(analysis.swiftui);
        assert!(analysis.swift);
        assert_eq!(analysis.refusal().unwrap().code, Some(Code::MacosSwiftUi));
        // Allowing Swift does not make SwiftUI appear.
        let allowed = AnalysisOptions {
            allow_swift: true,
            ..options()
        };
        let analysis = analyse_bytes(&bytes, &allowed).unwrap();
        assert_eq!(analysis.verdict, Verdict::Refused);

        let bytes = image(
            X86_64,
            2,
            &[
                Command::Main,
                Command::Dylib(LinkKind::Load, APPKIT),
                Command::Dylib(LinkKind::Weak, SWIFTUI),
            ],
        );
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.verdict, Verdict::RunnableWithWarnings);
        assert!(reasons(&analysis).contains(&"swiftui-optional"));
    }

    #[test]
    fn swift_with_the_system_runtime_is_refused_whatever_is_allowed() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Segment64(&["__text", "__swift5_types"]),
                Command::Main,
                Command::Dylib(LinkKind::Load, APPKIT),
                Command::Dylib(LinkKind::Load, SWIFT_CORE),
                Command::Dylib(LinkKind::Load, "/usr/lib/swift/libswiftAppKit.dylib"),
            ],
        );
        for allow_swift in [false, true] {
            let options = AnalysisOptions {
                host: Arch::X86_64,
                allow_swift,
            };
            let analysis = analyse_bytes(&bytes, &options).unwrap();
            let refusal = analysis.refusal().unwrap();
            assert_eq!(refusal.reason, "swift-system-runtime");
            assert_eq!(refusal.severity, Severity::Refusal);
            assert_eq!(refusal.code, Some(Code::MacosSwift));
        }
    }

    #[test]
    fn swift_with_an_embedded_runtime_is_refused_unless_allowed() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Main,
                Command::Dylib(LinkKind::Load, APPKIT),
                Command::Dylib(LinkKind::Load, "@rpath/libswiftCore.dylib"),
            ],
        );
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        let refusal = analysis.refusal().unwrap();
        assert_eq!(refusal.severity, Severity::RefusedUnlessAllowed);
        assert_eq!(analysis.verdict, Verdict::Refused);

        let allowed = AnalysisOptions {
            allow_swift: true,
            ..options()
        };
        let analysis = analyse_bytes(&bytes, &allowed).unwrap();
        assert_eq!(analysis.verdict, Verdict::RunnableWithWarnings);
        assert!(analysis.refusal().is_none());
    }

    #[test]
    fn encrypted_catalyst_ios_and_private_frameworks() {
        let bytes = image(X86_64, 2, &[Command::Main, Command::Encryption(1)]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().code, Some(Code::MacosEncrypted));

        let bytes = image(X86_64, 2, &[Command::Main, Command::Build(6, 0x000e_0000, 0)]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().reason, "mac-catalyst");

        let bytes = image(X86_64, 2, &[Command::Main, Command::Build(2, 0x000e_0000, 0)]);
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().reason, "not-macos");

        let bytes = image(
            X86_64,
            2,
            &[
                Command::Main,
                Command::Dylib(
                    LinkKind::Load,
                    "/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/SkyLight",
                ),
            ],
        );
        let analysis = analyse_bytes(&bytes, &options()).unwrap();
        assert_eq!(analysis.private_frameworks, ["SkyLight"]);
        assert_eq!(analysis.verdict, Verdict::RunnableWithWarnings);
    }

    #[test]
    fn a_bundle_is_analysed_through_its_main_program() {
        let scratch = TempDir::new("mac-bundle");
        let app = scratch.path().join("Example.app");
        std::fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        std::fs::write(
            app.join("Contents").join("Info.plist"),
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
             <key>CFBundleExecutable</key><string>Example</string>\
             <key>CFBundleIdentifier</key><string>org.example.app</string>\
             <key>CFBundleName</key><string>Example</string>\
             <key>CFBundleShortVersionString</key><string>1.2</string>\
             </dict></plist>",
        )
        .unwrap();
        std::fs::write(
            app.join("Contents").join("MacOS").join("Example"),
            appkit_program(ARM64),
        )
        .unwrap();
        let analysis = analyse_bundle(&app, &options()).unwrap();
        assert_eq!(analysis.bundle, "Example.app");
        assert_eq!(analysis.identifier.as_deref(), Some("org.example.app"));
        assert_eq!(analysis.version.as_deref(), Some("1.2"));
        assert_eq!(analysis.executable.as_deref(), Some("Contents/MacOS/Example"));
        assert_eq!(analysis.verdict, Verdict::Refused);
        assert_eq!(
            analysis.refusal().unwrap().code,
            Some(Code::MacosArm64Only)
        );

        // A directory that is not a bundle.
        let analysis = analyse_bundle(scratch.path(), &options()).unwrap();
        assert_eq!(analysis.refusal().unwrap().code, Some(Code::MacosNotMacSoftware));
    }

    #[test]
    fn framework_and_swift_names() {
        assert_eq!(
            framework_name(APPKIT, "/System/Library/Frameworks/").as_deref(),
            Some("AppKit")
        );
        assert_eq!(framework_name("/usr/lib/libz.dylib", "/System/Library/Frameworks/"), None);
        assert!(is_swift_library(SWIFT_CORE));
        assert!(is_swift_library("@rpath/libswiftAppKit.dylib"));
        assert!(!is_swift_library("/usr/lib/libobjc.A.dylib"));
    }
}

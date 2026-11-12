// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Runtime recommendation (specification §7.4).
//!
//! [`recommend`] is a pure function of an [`Inspection`] and the
//! [`HostFacts`]. It recommends; it never launches anything. The
//! specification forbids running a file because of its type (§7.2), and the
//! decision to run always stays with the user.
//!
//! The fallback order is the specification's: native Linux, Wine, Darling,
//! then the offer of a full Windows virtual machine for software that needs a
//! Windows kernel, and otherwise a plain statement that the file is not
//! supported and why. The Android runtime is the route for Android packages.
//!
//! A recommendation is a statement about the route, never a promise that a
//! particular application works.

use serde::Serialize;

use crate::android::{ARM_ABIS, AndroidInfo, AndroidPackageKind, NativeCode};
use crate::arch::Arch;
use crate::bundle::AppBundleInfo;
use crate::codes::Code;
use crate::elf::{ElfInfo, ElfType};
use crate::host::{Component, HostFacts};
use crate::inspect::{Detected, Inspection};
use crate::macho::{MachOFileType, MachOInfo};
use crate::ole::{OleInfo, OleKind};
use crate::pe::{MzInfo, MzNewHeader, PeImageFormat, PeInfo, PeSubsystem};
use crate::script::ScriptInfo;

/// The route recommended for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    /// Run directly as a Linux process.
    NativeLinux,
    /// Run under the Wine compatibility runtime.
    Wine,
    /// Run under the Darling compatibility runtime.
    Darling,
    /// Install into the Android runtime.
    Android,
    /// Wine cannot host this; offer a full Windows virtual machine.
    WindowsVmOffer,
    /// No safe route exists.
    Unsupported,
}

impl Runtime {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Runtime::NativeLinux => "native Linux",
            Runtime::Wine => "Wine",
            Runtime::Darling => "Darling",
            Runtime::Android => "Android runtime",
            Runtime::WindowsVmOffer => "full Windows virtual machine (offer)",
            Runtime::Unsupported => "unsupported",
        }
    }
}

/// A machine-readable reason behind a recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// An ELF program for the host's own architecture.
    ElfMatchesHost,
    /// An ELF program for a different architecture.
    ElfForeignArchitecture,
    /// A 32-bit x86 ELF program on an x86-64 host: the processor can run it,
    /// but the 32-bit system libraries are not provided.
    ElfNeedsMultilib,
    /// An ELF program for an operating system other than Linux.
    ElfForeignOs,
    /// An ELF file that is not a program: a library, object file or core dump.
    ElfNotAProgram,
    /// The ELF program is packaged as an AppImage.
    AppImage,
    /// A script that names its interpreter.
    ScriptInterpreter,
    /// A graphical or console Windows program for a runnable architecture.
    PeWindowsApplication,
    /// The Windows program is a .NET assembly.
    PeDotnet,
    /// A native-subsystem image such as a kernel driver: it requires a
    /// Windows kernel.
    PeRequiresWindowsKernel,
    /// A Windows program for an architecture the host cannot run.
    PeForeignArchitecture,
    /// A UEFI application, driver or ROM image.
    PeEfiImage,
    /// A Windows DLL, which is not a program.
    PeLibrary,
    /// A PE image for a subsystem no runtime provides.
    PeUnsupportedSubsystem,
    /// An MS-DOS, 16-bit Windows or OS/2 program.
    LegacyExecutable,
    /// A Windows Installer package.
    MsiInstaller,
    /// An OLE compound file that is not an application.
    OleNotAnApplication,
    /// A Mach-O file with a program for an architecture Darling runs.
    MachORunnableSlice,
    /// A Mach-O program built only for Apple Silicon.
    MachOAppleSiliconOnly,
    /// A Mach-O program with no code for an architecture Darling runs.
    MachONoRunnableSlice,
    /// A macOS kernel extension, which is never loaded.
    MachOKernelExtension,
    /// A Mach-O file that is not a program: a library, plug-in or object file.
    MachONotAProgram,
    /// Darling support is experimental.
    DarlingExperimental,
    /// Darling does not run on the host's architecture.
    DarlingHostUnsupported,
    /// The input is a macOS application bundle.
    MacAppBundle,
    /// The bundle's main program could not be located.
    BundleExecutableNotFound,
    /// The bundle's main program is not a Mach-O file.
    BundleExecutableNotMachO,
    /// An Android package with no native code.
    AndroidNoNativeCode,
    /// An Android package with native code for the host's architecture.
    AndroidNativeCodeMatchesHost,
    /// An Android package whose native code is ARM-only on an x86-64 host: it
    /// needs ARM translation.
    AndroidNeedsArmTranslation,
    /// An Android package whose native code is for an architecture that can
    /// be neither run nor translated.
    AndroidForeignNativeCode,
    /// An archive of Android packages; the packages inside were not opened.
    AndroidNestedPackagesNotInspected,
    /// An Android App Bundle, which is a publishing format and not installable.
    AndroidAppBundleNotInstallable,
    /// A plain archive, which is not an application.
    ArchiveNotAnApplication,
    /// The format was not recognised.
    UnrecognisedFormat,
    /// The recommended runtime, or a component it needs, is not installed.
    RuntimeNotInstalled,
}

/// What to do with an inspected file, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// The recommended route.
    pub runtime: Runtime,
    /// Whether the route is experimental.
    pub experimental: bool,
    /// Machine-readable reasons, most significant first.
    pub reasons: Vec<Reason>,
    /// Components the route needs that are not installed on the host.
    pub missing: Vec<Component>,
    /// A stable error code when the outcome is not plain success: the file is
    /// unsupported, something is missing, or only a virtual machine will do.
    pub code: Option<Code>,
    /// A plain-language explanation for the user.
    pub explanation: String,
}

impl Recommendation {
    fn new(runtime: Runtime, reason: Reason, explanation: impl Into<String>) -> Self {
        Recommendation {
            runtime,
            experimental: false,
            reasons: vec![reason],
            missing: Vec::new(),
            code: None,
            explanation: explanation.into(),
        }
    }

    fn unsupported(reason: Reason, code: Code, explanation: impl Into<String>) -> Self {
        Recommendation::new(Runtime::Unsupported, reason, explanation).with_code(code)
    }

    fn with_code(mut self, code: Code) -> Self {
        self.code = Some(code);
        self
    }

    fn because(mut self, reason: Reason) -> Self {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
        self
    }

    /// Records that the route needs `component`; if the host lacks it, the
    /// recommendation says so and carries `code`.
    fn requires(mut self, host: &HostFacts, component: Component, code: Code) -> Self {
        if host.has(component) {
            return self;
        }
        self.missing.push(component);
        self.code = Some(code);
        self.explanation.push(' ');
        self.explanation.push_str(component.label());
        self.explanation
            .push_str(" is not installed on this system.");
        self.because(Reason::RuntimeNotInstalled)
    }

    /// Whether the file has a route and nothing the route needs is missing.
    pub fn is_ready(&self) -> bool {
        self.runtime != Runtime::Unsupported && self.missing.is_empty()
    }
}

/// Recommends a route for an inspected file on the given host.
///
/// This never launches anything, and never will: see the module
/// documentation.
pub fn recommend(inspection: &Inspection, host: &HostFacts) -> Recommendation {
    match &inspection.detected {
        Detected::Elf(elf) => recommend_elf(elf, host),
        Detected::Pe(pe) => recommend_pe(pe, host),
        Detected::Mz(mz) => recommend_mz(mz),
        Detected::MachO(macho) => recommend_macho(macho, host),
        Detected::MacAppBundle(bundle) => recommend_bundle(bundle, host),
        Detected::Android(android) => recommend_android(android, host),
        Detected::Ole(ole) => recommend_ole(ole, host),
        Detected::Script(script) => recommend_script(script),
        Detected::Zip(_) => Recommendation::unsupported(
            Reason::ArchiveNotAnApplication,
            Code::NoRoute,
            "This is a zip archive that is not an Android package. It is not an application; \
             extract it and inspect what is inside.",
        ),
        Detected::Unknown => Recommendation::unsupported(
            Reason::UnrecognisedFormat,
            Code::NoRoute,
            "The format of this file was not recognised, so no runtime can be recommended. \
             That does not mean the file is invalid, only that this tool does not know it.",
        ),
    }
}

fn recommend_elf(elf: &ElfInfo, host: &HostFacts) -> Recommendation {
    let not_a_program = match elf.file_type {
        ElfType::Executable | ElfType::PieExecutable => None,
        ElfType::SharedObject => Some("a Linux shared library"),
        ElfType::Relocatable => Some("an object file for the linker"),
        ElfType::Core => Some("a core dump"),
        ElfType::Other => Some("an ELF file of a kind that is not a program"),
    };
    if let Some(what) = not_a_program {
        return Recommendation::unsupported(
            Reason::ElfNotAProgram,
            Code::NoRoute,
            format!("This is {what}, not a program that can be started."),
        );
    }
    if !elf.os_abi.is_linux_compatible() {
        return Recommendation::unsupported(
            Reason::ElfForeignOs,
            Code::NoRoute,
            format!(
                "This program is built for {}, not for Linux. TUR does not run it.",
                elf.os_abi.label()
            ),
        );
    }
    if elf.machine == Arch::Other {
        return Recommendation::unsupported(
            Reason::ElfForeignArchitecture,
            Code::NoRoute,
            format!(
                "This Linux program is built for a processor this tool does not name (ELF \
                 machine number {}). It cannot run on this {} computer.",
                elf.machine_raw, host.arch
            ),
        );
    }
    if elf.machine == host.arch {
        let mut recommendation = Recommendation::new(
            Runtime::NativeLinux,
            Reason::ElfMatchesHost,
            format!(
                "This is a native Linux program for {}, the same processor architecture as \
                 this computer. It runs directly, with no compatibility layer.",
                elf.machine
            ),
        );
        if elf.app_image.is_some() {
            recommendation = recommendation.because(Reason::AppImage);
            recommendation
                .explanation
                .push_str(" It is packaged as an AppImage.");
        }
        return recommendation;
    }
    if elf.machine == Arch::I386 && host.arch == Arch::X86_64 {
        return Recommendation::unsupported(
            Reason::ElfNeedsMultilib,
            Code::NoRoute,
            "This is a 32-bit x86 Linux program. This computer's processor can run it, but \
             32-bit programs need 32-bit system libraries, which TUR does not provide yet.",
        );
    }
    Recommendation::unsupported(
        Reason::ElfForeignArchitecture,
        Code::NoRoute,
        format!(
            "This Linux program is built for {} processors, but this computer is {}. TUR does \
             not yet run Linux programs built for a different processor architecture.",
            elf.machine, host.arch
        ),
    )
}

fn recommend_pe(pe: &PeInfo, host: &HostFacts) -> Recommendation {
    if pe.subsystem.is_efi() {
        return Recommendation::unsupported(
            Reason::PeEfiImage,
            Code::WindowsUnsupported,
            "This is a UEFI firmware program. It runs before an operating system starts, not \
             as a desktop application, so no TUR runtime applies to it.",
        );
    }
    if pe.requires_windows_kernel {
        return Recommendation::new(
            Runtime::WindowsVmOffer,
            Reason::PeRequiresWindowsKernel,
            "This is a Windows kernel-mode component, such as a device driver. It requires a \
             Windows kernel: Wine does not provide one, and TUR never loads Windows drivers \
             into the Linux kernel. Software that depends on it can only be used inside a full \
             Windows virtual machine.",
        )
        .with_code(Code::WindowsKernelRequired)
        .requires(host, Component::WindowsVm, Code::WindowsVmMissing);
    }
    if pe.is_dll {
        return Recommendation::unsupported(
            Reason::PeLibrary,
            Code::WindowsUnsupported,
            "This is a Windows library (DLL), not a program that can be started on its own.",
        );
    }
    let kind = match pe.subsystem {
        PeSubsystem::WindowsGui => "graphical",
        PeSubsystem::WindowsConsole => "console",
        other => {
            return Recommendation::unsupported(
                Reason::PeUnsupportedSubsystem,
                Code::WindowsUnsupported,
                format!(
                    "This Windows file is built for an environment that no TUR runtime \
                     provides ({}).",
                    other.label()
                ),
            );
        }
    };
    let runs_on_host = pe.machine != Arch::Other
        && (pe.machine == host.arch || (pe.machine == Arch::I386 && host.arch == Arch::X86_64));
    if !runs_on_host {
        let built_for = if pe.machine == Arch::Other {
            format!(
                "a processor this tool does not name (machine number {:#06x})",
                pe.machine_raw
            )
        } else {
            format!("{} processors", pe.machine)
        };
        return Recommendation::unsupported(
            Reason::PeForeignArchitecture,
            Code::WindowsUnsupported,
            format!(
                "This Windows program is built for {built_for}, but this computer is {}. TUR \
                 does not translate Windows programs between processor architectures.",
                host.arch
            ),
        );
    }
    let width = match pe.image_format {
        PeImageFormat::Pe32 => "32-bit ",
        PeImageFormat::Pe32Plus => "64-bit ",
        _ => "",
    };
    let mut recommendation = Recommendation::new(
        Runtime::Wine,
        Reason::PeWindowsApplication,
        format!(
            "This is a {width}{kind} Windows program for {}. The Wine compatibility runtime \
             is the route for it. Whether this particular program works under Wine has not \
             been tested.",
            pe.machine
        ),
    );
    if pe.is_dotnet {
        recommendation = recommendation.because(Reason::PeDotnet);
        recommendation
            .explanation
            .push_str(" It is a .NET program, so it also needs a .NET runtime in its Wine prefix.");
    }
    recommendation.requires(host, Component::Wine, Code::WineMissing)
}

fn recommend_mz(mz: &MzInfo) -> Recommendation {
    let explanation = match mz.new_header {
        MzNewHeader::OutsideFile => {
            "This file starts like an MS-DOS or Windows program, but its header points outside \
             the file. It is either an MS-DOS program or an incomplete Windows program; neither \
             can be run."
        }
        MzNewHeader::Ne | MzNewHeader::Le | MzNewHeader::Lx => {
            "This is a 16-bit Windows, OS/2 or DOS-extender program. No TUR runtime is set up \
             for these."
        }
        MzNewHeader::None | MzNewHeader::Unrecognised => {
            "This is an MS-DOS program, or a file that only starts like one. No TUR runtime is \
             set up for MS-DOS programs."
        }
    };
    Recommendation::unsupported(
        Reason::LegacyExecutable,
        Code::WindowsUnsupported,
        explanation,
    )
}

fn recommend_macho(info: &MachOInfo, host: &HostFacts) -> Recommendation {
    if info.has_kext() {
        return Recommendation::unsupported(
            Reason::MachOKernelExtension,
            Code::DarwinKext,
            "This is a macOS kernel extension. macOS kernel extensions cannot be loaded: TUR \
             runs on the Linux kernel and never loads another system's kernel drivers.",
        );
    }
    let programs: Vec<Arch> = info
        .slices
        .iter()
        .filter(|slice| slice.file_type == MachOFileType::Executable)
        .map(|slice| slice.arch)
        .collect();
    if programs.is_empty() {
        return Recommendation::unsupported(
            Reason::MachONotAProgram,
            Code::DarwinUnsupported,
            "This Mach-O file is a library, plug-in or other component, not a program that \
             can be started on its own.",
        );
    }
    if programs.contains(&Arch::X86_64) {
        if host.arch != Arch::X86_64 {
            return Recommendation::unsupported(
                Reason::DarlingHostUnsupported,
                Code::DarwinUnsupported,
                format!(
                    "This is a macOS program with x86-64 code. Darling, the macOS \
                     compatibility runtime, only runs on x86-64 computers, and this computer \
                     is {}.",
                    host.arch
                ),
            );
        }
        let shape = if info.universal {
            "universal macOS program that includes x86-64 code"
        } else {
            "macOS program for x86-64"
        };
        let mut recommendation = Recommendation::new(
            Runtime::Darling,
            Reason::MachORunnableSlice,
            format!(
                "This is a {shape}. The Darling compatibility runtime is the route for it. \
                 Darling support is experimental: command-line tools are the first target, \
                 and most graphical applications are not expected to work."
            ),
        )
        .because(Reason::DarlingExperimental);
        recommendation.experimental = true;
        return recommendation.requires(host, Component::Darling, Code::DarlingMissing);
    }
    if programs.iter().all(|arch| *arch == Arch::Aarch64) {
        return Recommendation::unsupported(
            Reason::MachOAppleSiliconOnly,
            Code::DarwinUnsupported,
            "This macOS program is built only for Apple Silicon (ARM64). Darling does not run \
             ARM64 programs, and translating them for an x86-64 processor has been deferred \
             (specification section 23, question 7).",
        );
    }
    let built_for = programs
        .iter()
        .map(|arch| arch.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Recommendation::unsupported(
        Reason::MachONoRunnableSlice,
        Code::DarwinUnsupported,
        format!(
            "This macOS program has no x86-64 code (it is built for: {built_for}). Darling's \
             initial scope is x86-64 programs only."
        ),
    )
}

fn recommend_bundle(bundle: &AppBundleInfo, host: &HostFacts) -> Recommendation {
    let Some(executable) = &bundle.executable else {
        return Recommendation::unsupported(
            Reason::BundleExecutableNotFound,
            Code::DarwinUnsupported,
            "This is a macOS application bundle, but its main program could not be located \
             under Contents/MacOS, so it cannot be assessed.",
        );
    };
    match &executable.detected {
        Detected::MachO(info) => {
            let mut recommendation = recommend_macho(info, host);
            recommendation.reasons.insert(0, Reason::MacAppBundle);
            recommendation.explanation.insert_str(
                0,
                "This is a macOS application bundle. About its main program: ",
            );
            recommendation
        }
        _ => Recommendation::unsupported(
            Reason::BundleExecutableNotMachO,
            Code::DarwinUnsupported,
            "This is a macOS application bundle, but its main program is not a Mach-O program, \
             so Darling cannot start it.",
        ),
    }
}

fn recommend_android(info: &AndroidInfo, host: &HostFacts) -> Recommendation {
    match info.kind {
        AndroidPackageKind::Apk => recommend_apk(info, host),
        AndroidPackageKind::Aab => Recommendation::unsupported(
            Reason::AndroidAppBundleNotInstallable,
            Code::AndroidUnsupported,
            "This is an Android App Bundle, the format developers upload to an app store, \
             which then builds installable packages from it. It cannot be installed directly, \
             and TUR does not convert it.",
        ),
        AndroidPackageKind::Xapk | AndroidPackageKind::Apks | AndroidPackageKind::ApkArchive => {
            Recommendation::new(
                Runtime::Android,
                Reason::AndroidNestedPackagesNotInspected,
                "This is an archive of Android packages. The Android runtime is the route for \
                 it. The packages inside were not opened, so it is not known whether they hold \
                 native code for this computer's processor.",
            )
            .requires(host, Component::Android, Code::AndroidRuntimeMissing)
        }
    }
}

fn recommend_apk(info: &AndroidInfo, host: &HostFacts) -> Recommendation {
    if info.native_code != NativeCode::Present {
        return Recommendation::new(
            Runtime::Android,
            Reason::AndroidNoNativeCode,
            "This is an Android application with no native code, so it does not depend on the \
             processor architecture. The Android runtime is the route for it.",
        )
        .requires(host, Component::Android, Code::AndroidRuntimeMissing);
    }
    let abis = info.native_abis.join(", ");
    let host_abis: &[&str] = match host.arch {
        Arch::X86_64 => &["x86_64", "x86"],
        Arch::I386 => &["x86"],
        Arch::Aarch64 => &["arm64-v8a", "armeabi-v7a", "armeabi"],
        Arch::Arm => &["armeabi-v7a", "armeabi"],
        _ => &[],
    };
    let matches_host = info
        .native_abis
        .iter()
        .any(|abi| host_abis.contains(&abi.as_str()));
    if matches_host {
        return Recommendation::new(
            Runtime::Android,
            Reason::AndroidNativeCodeMatchesHost,
            format!(
                "This is an Android application with native code for this computer's \
                 processor (it contains: {abis}). The Android runtime is the route for it."
            ),
        )
        .requires(host, Component::Android, Code::AndroidRuntimeMissing);
    }
    let has_arm = info
        .native_abis
        .iter()
        .any(|abi| ARM_ABIS.contains(&abi.as_str()));
    if has_arm && host.arch == Arch::X86_64 {
        return Recommendation::new(
            Runtime::Android,
            Reason::AndroidNeedsArmTranslation,
            format!(
                "This Android application only contains native code for ARM processors \
                 ({abis}). On this x86-64 computer the Android runtime needs ARM translation \
                 to run it. Translated code is slower, and not every application works with it."
            ),
        )
        .requires(
            host,
            Component::ArmTranslation,
            Code::AndroidTranslationMissing,
        )
        .requires(host, Component::Android, Code::AndroidRuntimeMissing);
    }
    Recommendation::unsupported(
        Reason::AndroidForeignNativeCode,
        Code::AndroidUnsupported,
        format!(
            "This Android application only contains native code for other processors \
             ({abis}); none of it matches this {} computer.",
            host.arch
        ),
    )
}

fn recommend_ole(ole: &OleInfo, host: &HostFacts) -> Recommendation {
    match ole.kind {
        OleKind::MsiInstaller => Recommendation::new(
            Runtime::Wine,
            Reason::MsiInstaller,
            "This is a Windows Installer package (MSI). The Wine compatibility runtime is the \
             route for it; Wine installs such packages with its own installer service. The \
             processor architecture of the software inside was not read, and whether it \
             installs or works under Wine has not been tested.",
        )
        .requires(host, Component::Wine, Code::WineMissing),
        OleKind::MsiPatch | OleKind::MsiTransform => Recommendation::unsupported(
            Reason::OleNotAnApplication,
            Code::WindowsUnsupported,
            "This is a Windows Installer patch or transform. It modifies an installer package \
             and cannot be used on its own.",
        ),
        OleKind::Unidentified => Recommendation::unsupported(
            Reason::OleNotAnApplication,
            Code::NoRoute,
            "This is an OLE compound file, the container used by older Office documents among \
             others. It was not identified as an application.",
        ),
    }
}

fn recommend_script(script: &ScriptInfo) -> Recommendation {
    Recommendation::new(
        Runtime::NativeLinux,
        Reason::ScriptInterpreter,
        format!(
            "This is a script that names {} as its interpreter. It runs natively on Linux, \
             provided that interpreter is installed.",
            script.interpreter
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{ExecutableSource, PlistStatus};
    use crate::inspect::inspect_bytes;
    use crate::ole::CLSID_MSI;
    use crate::testdata::{
        CPU_ARM64, CPU_I386, CPU_PPC, CPU_X86_64, ElfSpec, PeSpec, macho_fat, macho_thin, mz_with,
        ole, zip,
    };

    /// An x86-64 host with every runtime component installed.
    fn full_host() -> HostFacts {
        HostFacts::new(Arch::X86_64, &Component::ALL)
    }

    /// An x86-64 host with only these components installed.
    fn host_with(components: &[Component]) -> HostFacts {
        HostFacts::new(Arch::X86_64, components)
    }

    fn for_bytes(bytes: &[u8], host: &HostFacts) -> Recommendation {
        recommend(&inspect_bytes(bytes).unwrap(), host)
    }

    fn assert_unsupported(recommendation: &Recommendation, reason: Reason, code: Code) {
        assert_eq!(recommendation.runtime, Runtime::Unsupported);
        assert_eq!(recommendation.reasons, [reason]);
        assert_eq!(recommendation.code, Some(code));
        assert!(recommendation.missing.is_empty());
        assert!(!recommendation.is_ready());
        assert!(!recommendation.explanation.is_empty());
    }

    #[test]
    fn same_architecture_elf_is_native() {
        let recommendation = for_bytes(&ElfSpec::linux_x86_64().build(), &host_with(&[]));
        assert_eq!(recommendation.runtime, Runtime::NativeLinux);
        assert_eq!(recommendation.reasons, [Reason::ElfMatchesHost]);
        assert_eq!(recommendation.code, None);
        assert!(!recommendation.experimental);
        assert!(recommendation.is_ready());

        let arm = ElfSpec {
            machine: 183,
            ..ElfSpec::linux_x86_64()
        };
        let arm_host = HostFacts::new(Arch::Aarch64, &[]);
        assert_eq!(
            for_bytes(&arm.build(), &arm_host).runtime,
            Runtime::NativeLinux
        );
    }

    #[test]
    fn foreign_architecture_elf_is_unsupported_and_says_why() {
        let arm = ElfSpec {
            machine: 183,
            ..ElfSpec::linux_x86_64()
        };
        let recommendation = for_bytes(&arm.build(), &full_host());
        assert_unsupported(
            &recommendation,
            Reason::ElfForeignArchitecture,
            Code::NoRoute,
        );
        assert!(recommendation.explanation.contains("aarch64"));
        assert!(recommendation.explanation.contains("x86_64"));

        let unnamed = ElfSpec {
            machine: 0x1234,
            ..ElfSpec::linux_x86_64()
        };
        assert_unsupported(
            &for_bytes(&unnamed.build(), &full_host()),
            Reason::ElfForeignArchitecture,
            Code::NoRoute,
        );
    }

    #[test]
    fn thirty_two_bit_x86_elf_names_the_missing_libraries() {
        let i386 = ElfSpec {
            wide: false,
            machine: 3,
            interpreter: Some("/lib/ld-linux.so.2"),
            ..ElfSpec::linux_x86_64()
        };
        let recommendation = for_bytes(&i386.build(), &full_host());
        assert_unsupported(&recommendation, Reason::ElfNeedsMultilib, Code::NoRoute);
        assert!(recommendation.explanation.contains("32-bit"));
    }

    #[test]
    fn elf_files_that_are_not_linux_programs() {
        let library = ElfSpec {
            interpreter: None,
            dynamic_flags: Some(0),
            ..ElfSpec::linux_x86_64()
        };
        let core = ElfSpec {
            file_type: 4,
            interpreter: None,
            dynamic_flags: None,
            ..ElfSpec::linux_x86_64()
        };
        for spec in [library, core] {
            assert_unsupported(
                &for_bytes(&spec.build(), &full_host()),
                Reason::ElfNotAProgram,
                Code::NoRoute,
            );
        }
        let freebsd = ElfSpec {
            os_abi: 9,
            ..ElfSpec::linux_x86_64()
        };
        let recommendation = for_bytes(&freebsd.build(), &full_host());
        assert_unsupported(&recommendation, Reason::ElfForeignOs, Code::NoRoute);
        assert!(recommendation.explanation.contains("FreeBSD"));
    }

    #[test]
    fn app_image_is_native_and_flagged() {
        let spec = ElfSpec {
            app_image: Some(2),
            ..ElfSpec::linux_x86_64()
        };
        let recommendation = for_bytes(&spec.build(), &full_host());
        assert_eq!(recommendation.runtime, Runtime::NativeLinux);
        assert_eq!(
            recommendation.reasons,
            [Reason::ElfMatchesHost, Reason::AppImage]
        );
    }

    #[test]
    fn windows_gui_and_console_programs_go_to_wine() {
        let gui = for_bytes(&PeSpec::windows_gui_x86_64().build(), &full_host());
        assert_eq!(gui.runtime, Runtime::Wine);
        assert_eq!(gui.reasons, [Reason::PeWindowsApplication]);
        assert_eq!(gui.code, None);
        assert!(!gui.experimental);
        assert!(gui.is_ready());
        assert!(gui.explanation.contains("64-bit graphical"));

        let console = PeSpec {
            machine: 0x014c,
            plus: false,
            subsystem: 3,
            ..PeSpec::windows_gui_x86_64()
        };
        let console = for_bytes(&console.build(), &full_host());
        assert_eq!(console.runtime, Runtime::Wine);
        assert!(console.explanation.contains("32-bit console"));
        assert!(console.is_ready());
    }

    #[test]
    fn wine_recommendation_never_claims_the_program_works() {
        let gui = for_bytes(&PeSpec::windows_gui_x86_64().build(), &full_host());
        assert!(gui.explanation.contains("has not been tested"));
    }

    #[test]
    fn dotnet_program_goes_to_wine_with_a_reason() {
        let dotnet = PeSpec {
            clr: true,
            ..PeSpec::windows_gui_x86_64()
        };
        let recommendation = for_bytes(&dotnet.build(), &full_host());
        assert_eq!(recommendation.runtime, Runtime::Wine);
        assert_eq!(
            recommendation.reasons,
            [Reason::PeWindowsApplication, Reason::PeDotnet]
        );
    }

    #[test]
    fn kernel_driver_gets_a_windows_vm_offer() {
        let driver = PeSpec {
            subsystem: 1,
            ..PeSpec::windows_gui_x86_64()
        };
        let recommendation = for_bytes(&driver.build(), &full_host());
        assert_eq!(recommendation.runtime, Runtime::WindowsVmOffer);
        assert_eq!(recommendation.reasons, [Reason::PeRequiresWindowsKernel]);
        assert_eq!(recommendation.code, Some(Code::WindowsKernelRequired));
        assert!(
            recommendation
                .explanation
                .contains("requires a Windows kernel")
        );
        assert!(recommendation.is_ready());

        // Wine being installed does not help; the VM backend is what counts.
        let without_vm = for_bytes(&driver.build(), &host_with(&[Component::Wine]));
        assert_eq!(without_vm.runtime, Runtime::WindowsVmOffer);
        assert_eq!(without_vm.missing, [Component::WindowsVm]);
        assert_eq!(without_vm.code, Some(Code::WindowsVmMissing));
        assert!(!without_vm.is_ready());

        // A kernel-mode DLL is still a kernel-mode component.
        let kernel_dll = PeSpec {
            subsystem: 1,
            characteristics: 0x2022,
            ..PeSpec::windows_gui_x86_64()
        };
        assert_eq!(
            for_bytes(&kernel_dll.build(), &full_host()).runtime,
            Runtime::WindowsVmOffer
        );
    }

    #[test]
    fn arm64_windows_program_on_x86_64_is_unsupported() {
        let arm64 = PeSpec {
            machine: 0xaa64,
            ..PeSpec::windows_gui_x86_64()
        };
        let recommendation = for_bytes(&arm64.build(), &full_host());
        assert_unsupported(
            &recommendation,
            Reason::PeForeignArchitecture,
            Code::WindowsUnsupported,
        );
        assert!(recommendation.explanation.contains("aarch64"));

        let unnamed = PeSpec {
            machine: 0x9999,
            ..PeSpec::windows_gui_x86_64()
        };
        let recommendation = for_bytes(&unnamed.build(), &full_host());
        assert_unsupported(
            &recommendation,
            Reason::PeForeignArchitecture,
            Code::WindowsUnsupported,
        );
        assert!(recommendation.explanation.contains("0x9999"));
    }

    #[test]
    fn efi_application_is_unsupported() {
        for subsystem in [10, 11, 12, 13] {
            let efi = PeSpec {
                subsystem,
                ..PeSpec::windows_gui_x86_64()
            };
            assert_unsupported(
                &for_bytes(&efi.build(), &full_host()),
                Reason::PeEfiImage,
                Code::WindowsUnsupported,
            );
        }
    }

    #[test]
    fn dll_and_other_subsystems_are_unsupported() {
        let dll = PeSpec {
            characteristics: 0x2022,
            ..PeSpec::windows_gui_x86_64()
        };
        assert_unsupported(
            &for_bytes(&dll.build(), &full_host()),
            Reason::PeLibrary,
            Code::WindowsUnsupported,
        );
        for subsystem in [0, 7, 9, 14, 16, 99] {
            let other = PeSpec {
                subsystem,
                ..PeSpec::windows_gui_x86_64()
            };
            assert_unsupported(
                &for_bytes(&other.build(), &full_host()),
                Reason::PeUnsupportedSubsystem,
                Code::WindowsUnsupported,
            );
        }
        assert_unsupported(
            &for_bytes(&mz_with(b"NE\x05\x00"), &full_host()),
            Reason::LegacyExecutable,
            Code::WindowsUnsupported,
        );
    }

    #[test]
    fn missing_wine_is_reported_with_what_is_missing() {
        let recommendation = for_bytes(
            &PeSpec::windows_gui_x86_64().build(),
            &host_with(&[Component::Darling, Component::Android]),
        );
        assert_eq!(recommendation.runtime, Runtime::Wine);
        assert_eq!(recommendation.missing, [Component::Wine]);
        assert_eq!(recommendation.code, Some(Code::WineMissing));
        assert_eq!(
            recommendation.reasons,
            [Reason::PeWindowsApplication, Reason::RuntimeNotInstalled]
        );
        assert!(
            recommendation
                .explanation
                .ends_with("Wine is not installed on this system.")
        );
        assert!(!recommendation.is_ready());
    }

    #[test]
    fn x86_64_mach_o_goes_to_darling_as_experimental() {
        let thin = for_bytes(&macho_thin(CPU_X86_64, 2, true, false), &full_host());
        assert_eq!(thin.runtime, Runtime::Darling);
        assert!(thin.experimental);
        assert_eq!(
            thin.reasons,
            [Reason::MachORunnableSlice, Reason::DarlingExperimental]
        );
        assert_eq!(thin.code, None);
        assert!(thin.is_ready());
        assert!(thin.explanation.contains("experimental"));

        let universal = macho_fat(&[(CPU_ARM64, 2), (CPU_X86_64, 2)], false);
        let universal = for_bytes(&universal, &full_host());
        assert_eq!(universal.runtime, Runtime::Darling);
        assert!(universal.experimental);
        assert!(universal.explanation.contains("universal"));
    }

    #[test]
    fn missing_darling_is_reported() {
        let recommendation = for_bytes(
            &macho_thin(CPU_X86_64, 2, true, false),
            &host_with(&[Component::Wine]),
        );
        assert_eq!(recommendation.runtime, Runtime::Darling);
        assert!(recommendation.experimental);
        assert_eq!(recommendation.missing, [Component::Darling]);
        assert_eq!(recommendation.code, Some(Code::DarlingMissing));
        assert!(
            recommendation
                .explanation
                .contains("Darling is not installed")
        );
    }

    #[test]
    fn apple_silicon_only_mach_o_is_unsupported() {
        let thin = for_bytes(&macho_thin(CPU_ARM64, 2, true, false), &full_host());
        assert_unsupported(
            &thin,
            Reason::MachOAppleSiliconOnly,
            Code::DarwinUnsupported,
        );
        assert!(thin.explanation.contains("question 7"));
        assert!(!thin.experimental);

        let universal = macho_fat(&[(CPU_ARM64, 2)], false);
        assert_unsupported(
            &for_bytes(&universal, &full_host()),
            Reason::MachOAppleSiliconOnly,
            Code::DarwinUnsupported,
        );
    }

    #[test]
    fn kernel_extension_is_never_loaded() {
        let recommendation = for_bytes(&macho_thin(CPU_X86_64, 0xb, true, false), &full_host());
        assert_unsupported(
            &recommendation,
            Reason::MachOKernelExtension,
            Code::DarwinKext,
        );
        assert!(
            recommendation
                .explanation
                .contains("macOS kernel extensions cannot be loaded")
        );

        // A kext slice anywhere in a universal file decides the matter.
        let universal = macho_fat(&[(CPU_X86_64, 2), (CPU_ARM64, 0xb)], false);
        assert_unsupported(
            &for_bytes(&universal, &full_host()),
            Reason::MachOKernelExtension,
            Code::DarwinKext,
        );
    }

    #[test]
    fn mach_o_libraries_and_other_architectures_are_unsupported() {
        assert_unsupported(
            &for_bytes(&macho_thin(CPU_X86_64, 6, true, false), &full_host()),
            Reason::MachONotAProgram,
            Code::DarwinUnsupported,
        );
        let old = for_bytes(
            &macho_fat(&[(CPU_PPC, 2), (CPU_I386, 2)], false),
            &full_host(),
        );
        assert_unsupported(&old, Reason::MachONoRunnableSlice, Code::DarwinUnsupported);
        assert!(old.explanation.contains("ppc, i386"));

        let arm_host = HostFacts::new(Arch::Aarch64, &Component::ALL);
        assert_unsupported(
            &for_bytes(&macho_thin(CPU_X86_64, 2, true, false), &arm_host),
            Reason::DarlingHostUnsupported,
            Code::DarwinUnsupported,
        );
    }

    fn bundle_with(executable: Option<&[u8]>) -> Inspection {
        Inspection {
            file_size: 0,
            detected: Detected::MacAppBundle(AppBundleInfo {
                plist: PlistStatus::Parsed,
                identifier: Some("com.example.Demo".to_owned()),
                name: Some("Demo".to_owned()),
                version: None,
                executable_name: executable.map(|_| "Demo".to_owned()),
                executable_source: executable.map(|_| ExecutableSource::InfoPlist),
                executable: executable.map(|bytes| Box::new(inspect_bytes(bytes).unwrap())),
            }),
            notes: Vec::new(),
        }
    }

    #[test]
    fn app_bundle_follows_its_main_program() {
        let program = macho_thin(CPU_X86_64, 2, true, false);
        let recommendation = recommend(&bundle_with(Some(program.as_slice())), &full_host());
        assert_eq!(recommendation.runtime, Runtime::Darling);
        assert!(recommendation.experimental);
        assert_eq!(
            recommendation.reasons,
            [
                Reason::MacAppBundle,
                Reason::MachORunnableSlice,
                Reason::DarlingExperimental
            ]
        );
        assert!(
            recommendation
                .explanation
                .starts_with("This is a macOS application bundle.")
        );

        let arm = macho_thin(CPU_ARM64, 2, true, false);
        let recommendation = recommend(&bundle_with(Some(arm.as_slice())), &full_host());
        assert_eq!(recommendation.runtime, Runtime::Unsupported);
        assert_eq!(
            recommendation.reasons,
            [Reason::MacAppBundle, Reason::MachOAppleSiliconOnly]
        );

        assert_unsupported(
            &recommend(&bundle_with(None), &full_host()),
            Reason::BundleExecutableNotFound,
            Code::DarwinUnsupported,
        );
        assert_unsupported(
            &recommend(&bundle_with(Some(&b"#!/bin/sh\n"[..])), &full_host()),
            Reason::BundleExecutableNotMachO,
            Code::DarwinUnsupported,
        );
    }

    #[test]
    fn apk_with_host_code_or_no_native_code_goes_to_the_android_runtime() {
        let with_x86 = zip(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/x86_64/libapp.so",
            "lib/arm64-v8a/libapp.so",
        ]);
        let recommendation = for_bytes(&with_x86, &full_host());
        assert_eq!(recommendation.runtime, Runtime::Android);
        assert_eq!(
            recommendation.reasons,
            [Reason::AndroidNativeCodeMatchesHost]
        );
        assert_eq!(recommendation.code, None);
        assert!(recommendation.is_ready());
        // ARM translation is not needed, so its absence does not matter.
        let recommendation = for_bytes(&with_x86, &host_with(&[Component::Android]));
        assert!(recommendation.is_ready());

        let no_native = zip(&["AndroidManifest.xml", "classes.dex"]);
        let recommendation = for_bytes(&no_native, &host_with(&[Component::Android]));
        assert_eq!(recommendation.runtime, Runtime::Android);
        assert_eq!(recommendation.reasons, [Reason::AndroidNoNativeCode]);
        assert!(recommendation.is_ready());
    }

    #[test]
    fn arm_only_apk_on_x86_64_needs_arm_translation() {
        let arm_only = zip(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/arm64-v8a/libapp.so",
            "lib/armeabi-v7a/libapp.so",
        ]);
        let recommendation = for_bytes(&arm_only, &full_host());
        assert_eq!(recommendation.runtime, Runtime::Android);
        assert_eq!(recommendation.reasons, [Reason::AndroidNeedsArmTranslation]);
        assert!(recommendation.explanation.contains("needs ARM translation"));
        assert!(recommendation.explanation.contains("arm64-v8a"));
        assert_eq!(recommendation.code, None);
        assert!(recommendation.is_ready());

        let without_translation = for_bytes(&arm_only, &host_with(&[Component::Android]));
        assert_eq!(without_translation.runtime, Runtime::Android);
        assert_eq!(without_translation.missing, [Component::ArmTranslation]);
        assert_eq!(
            without_translation.code,
            Some(Code::AndroidTranslationMissing)
        );
        assert_eq!(
            without_translation.reasons,
            [
                Reason::AndroidNeedsArmTranslation,
                Reason::RuntimeNotInstalled
            ]
        );
        assert!(!without_translation.is_ready());

        let nothing = for_bytes(&arm_only, &host_with(&[]));
        assert_eq!(
            nothing.missing,
            [Component::ArmTranslation, Component::Android]
        );
        assert_eq!(nothing.code, Some(Code::AndroidRuntimeMissing));

        // On an ARM host the same package needs no translation.
        let arm_host = HostFacts::new(Arch::Aarch64, &[Component::Android]);
        let native = for_bytes(&arm_only, &arm_host);
        assert_eq!(native.reasons, [Reason::AndroidNativeCodeMatchesHost]);
        assert!(native.is_ready());
    }

    #[test]
    fn other_android_packages() {
        let mips = zip(&["AndroidManifest.xml", "classes.dex", "lib/mips/libapp.so"]);
        assert_unsupported(
            &for_bytes(&mips, &full_host()),
            Reason::AndroidForeignNativeCode,
            Code::AndroidUnsupported,
        );

        let bundle = zip(&["base/manifest/AndroidManifest.xml", "base/dex/classes.dex"]);
        assert_unsupported(
            &for_bytes(&bundle, &full_host()),
            Reason::AndroidAppBundleNotInstallable,
            Code::AndroidUnsupported,
        );

        let xapk = zip(&["manifest.json", "base.apk"]);
        let recommendation = for_bytes(&xapk, &host_with(&[]));
        assert_eq!(recommendation.runtime, Runtime::Android);
        assert_eq!(
            recommendation.reasons,
            [
                Reason::AndroidNestedPackagesNotInspected,
                Reason::RuntimeNotInstalled
            ]
        );
        assert_eq!(recommendation.missing, [Component::Android]);
        assert_eq!(recommendation.code, Some(Code::AndroidRuntimeMissing));
    }

    #[test]
    fn installers_scripts_archives_and_unknown_files() {
        let msi = for_bytes(&ole(&CLSID_MSI, 9), &full_host());
        assert_eq!(msi.runtime, Runtime::Wine);
        assert_eq!(msi.reasons, [Reason::MsiInstaller]);
        assert!(msi.is_ready());
        let msi = for_bytes(&ole(&CLSID_MSI, 9), &host_with(&[]));
        assert_eq!(msi.missing, [Component::Wine]);

        assert_unsupported(
            &for_bytes(&ole(&[0u8; 16], 9), &full_host()),
            Reason::OleNotAnApplication,
            Code::NoRoute,
        );

        let script = for_bytes(b"#!/bin/sh\necho hi\n", &host_with(&[]));
        assert_eq!(script.runtime, Runtime::NativeLinux);
        assert_eq!(script.reasons, [Reason::ScriptInterpreter]);
        assert!(script.explanation.contains("/bin/sh"));

        assert_unsupported(
            &for_bytes(&zip(&["readme.txt"]), &full_host()),
            Reason::ArchiveNotAnApplication,
            Code::NoRoute,
        );
        assert_unsupported(
            &for_bytes(b"plain text", &full_host()),
            Reason::UnrecognisedFormat,
            Code::NoRoute,
        );
    }

    #[test]
    fn serialises_with_machine_readable_names() {
        let driver = PeSpec {
            subsystem: 1,
            ..PeSpec::windows_gui_x86_64()
        };
        let recommendation = for_bytes(&driver.build(), &host_with(&[]));
        let json = serde_json::to_value(recommendation).unwrap();
        assert_eq!(json["runtime"], "windows-vm-offer");
        assert_eq!(json["experimental"], false);
        assert_eq!(json["reasons"][0], "pe-requires-windows-kernel");
        assert_eq!(json["reasons"][1], "runtime-not-installed");
        assert_eq!(json["missing"][0], "windows-vm");
        assert_eq!(json["code"], "TUR-WINDOWS-RUNTIME-003");

        let thin = for_bytes(&macho_thin(CPU_X86_64, 2, true, false), &full_host());
        let json = serde_json::to_value(thin).unwrap();
        assert_eq!(json["runtime"], "darling");
        assert_eq!(json["experimental"], true);
        assert_eq!(json["reasons"][0], "mach-o-runnable-slice");
        assert_eq!(json["code"], serde_json::Value::Null);

        let native = for_bytes(&ElfSpec::linux_x86_64().build(), &full_host());
        let json = serde_json::to_value(native).unwrap();
        assert_eq!(json["runtime"], "native-linux");
        assert_eq!(json["reasons"][0], "elf-matches-host");
    }
}

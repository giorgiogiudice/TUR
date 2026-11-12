// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Plain-text rendering of an inspection and its recommendation.
//!
//! Every string taken from the inspected file that reaches this report has
//! already been checked to hold no control characters, so a hostile file
//! cannot inject terminal escape sequences through it.

use serde::Serialize;

use crate::android::NativeCode;
use crate::inspect::{Detected, Inspection};
use crate::macho::MachOInfo;
use crate::recommend::Recommendation;

/// The machine-readable name a value serialises to, for example
/// `pe-requires-windows-kernel` for a reason.
pub fn wire_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

/// One line that says what the input is.
pub fn summary(detected: &Detected) -> String {
    match detected {
        Detected::Elf(elf) => format!(
            "ELF {} {} for {}, {}, {}",
            elf.class.label(),
            elf.endianness.label(),
            elf.machine,
            elf.file_type.label(),
            elf.linkage.label()
        ),
        Detected::Pe(pe) => {
            let mut text = format!(
                "Windows PE ({}) for {}, {}",
                pe.image_format.label(),
                pe.machine,
                pe.subsystem.label()
            );
            if pe.is_dll {
                text.push_str(", DLL");
            }
            if pe.is_dotnet {
                text.push_str(", .NET");
            }
            text
        }
        Detected::Mz(mz) => format!("MZ executable: {}", mz.new_header.label()),
        Detected::MachO(macho) => macho_summary(macho),
        Detected::MacAppBundle(bundle) => match &bundle.name {
            Some(name) => format!("macOS application bundle \"{name}\""),
            None => "macOS application bundle".to_owned(),
        },
        Detected::Android(android) => android.kind.label().to_owned(),
        Detected::Ole(ole) => ole.kind.label().to_owned(),
        Detected::Script(script) => format!("script for {}", script.interpreter),
        Detected::Zip(zip) => format!("zip archive with {} entries", zip.entries),
        Detected::Unknown => "not recognised".to_owned(),
    }
}

fn macho_summary(macho: &MachOInfo) -> String {
    let slices: Vec<String> = macho
        .slices
        .iter()
        .map(|slice| format!("{} {}", slice.arch, slice.file_type.label()))
        .collect();
    let listed = slices.join(", ");
    if macho.universal {
        format!("Mach-O universal file: {listed}")
    } else {
        let bits = macho.slices.first().map_or(0, |slice| slice.bits);
        format!("Mach-O {bits}-bit {listed}")
    }
}

/// The inspection and recommendation as aligned `Label: value` lines.
pub fn render_text(path: &str, inspection: &Inspection, recommendation: &Recommendation) -> String {
    let mut lines: Vec<(&str, String)> = vec![
        ("Path", path.to_owned()),
        ("Detected", summary(&inspection.detected)),
    ];
    details(&inspection.detected, &mut lines);
    for note in &inspection.notes {
        lines.push(("Note", note.clone()));
    }

    let mut route = recommendation.runtime.label().to_owned();
    if recommendation.experimental {
        route.push_str(" (experimental)");
    }
    lines.push(("Recommended", route));
    let reasons: Vec<String> = recommendation.reasons.iter().map(wire_name).collect();
    lines.push(("Reasons", reasons.join(", ")));
    if !recommendation.missing.is_empty() {
        let missing: Vec<&str> = recommendation
            .missing
            .iter()
            .map(|component| component.as_str())
            .collect();
        lines.push(("Not installed", missing.join(", ")));
    }
    if let Some(code) = recommendation.code {
        let title = code.title();
        lines.push(("Code", format!("{code} ({title})")));
    }
    lines.push(("Explanation", recommendation.explanation.clone()));

    let longest = lines
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    let width = longest + 1;
    let mut text = String::new();
    for (label, value) in &lines {
        let key = format!("{label}:");
        text.push_str(&format!("{key:<width$} {value}\n"));
    }
    text
}

/// Lines that add to the one-line summary.
fn details(detected: &Detected, lines: &mut Vec<(&str, String)>) {
    match detected {
        Detected::Elf(elf) => {
            if let Some(interpreter) = &elf.interpreter {
                lines.push(("Interpreter", interpreter.clone()));
            }
            lines.push(("OS ABI", elf.os_abi.label().to_owned()));
            if let Some(kind) = elf.app_image {
                lines.push(("Packaging", format!("AppImage type {kind}")));
            }
        }
        Detected::Pe(pe) if pe.requires_windows_kernel => {
            lines.push(("Requires", "a Windows kernel".to_owned()));
        }
        Detected::MacAppBundle(bundle) => {
            lines.push(("Info.plist", bundle.plist.label().to_owned()));
            if let Some(identifier) = &bundle.identifier {
                lines.push(("Identifier", identifier.clone()));
            }
            if let Some(version) = &bundle.version {
                lines.push(("Version", version.clone()));
            }
            let program = match (&bundle.executable_name, &bundle.executable) {
                (Some(name), Some(program)) => {
                    let what = summary(&program.detected);
                    format!("Contents/MacOS/{name}: {what}")
                }
                _ => "not located".to_owned(),
            };
            lines.push(("Main program", program));
        }
        Detected::Android(android) => {
            let native = match android.native_code {
                NativeCode::Absent => "none".to_owned(),
                NativeCode::Present => android.native_abis.join(", "),
                NativeCode::NotInspected => "not inspected".to_owned(),
            };
            lines.push(("Native code", native));
            if android.nested_package_count > 0 {
                lines.push((
                    "Packages",
                    format!(
                        "{} inside: {}",
                        android.nested_package_count,
                        android.nested_packages.join(", ")
                    ),
                ));
            }
        }
        Detected::Ole(ole) => {
            if let Some(clsid) = &ole.root_clsid {
                lines.push(("Root class", clsid.clone()));
            }
        }
        Detected::Script(script) => {
            if let Some(argument) = &script.argument {
                lines.push(("Argument", argument.clone()));
            }
        }
        Detected::Pe(_)
        | Detected::Mz(_)
        | Detected::MachO(_)
        | Detected::Zip(_)
        | Detected::Unknown => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::Arch;
    use crate::host::{Component, HostFacts};
    use crate::inspect::inspect_bytes;
    use crate::recommend::{Reason, Runtime, recommend};
    use crate::testdata::{
        CPU_ARM64, CPU_X86_64, ElfSpec, PeSpec, macho_fat, macho_thin, samples, zip,
    };

    fn render(bytes: &[u8], components: &[Component]) -> String {
        let inspection = inspect_bytes(bytes).unwrap();
        let host = HostFacts::new(Arch::X86_64, components);
        let recommendation = recommend(&inspection, &host);
        render_text("/tmp/sample", &inspection, &recommendation)
    }

    /// The value on the first line that starts with `label:`.
    fn value_of<'a>(text: &'a str, label: &str) -> Option<&'a str> {
        let prefix = format!("{label}:");
        text.lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .map(str::trim)
    }

    #[test]
    fn wire_names_are_the_json_names() {
        assert_eq!(
            wire_name(&Reason::PeRequiresWindowsKernel),
            "pe-requires-windows-kernel"
        );
        assert_eq!(wire_name(&Runtime::WindowsVmOffer), "windows-vm-offer");
        assert_eq!(wire_name(&Component::ArmTranslation), "arm-translation");
        assert_eq!(wire_name(&42), "unknown");
    }

    #[test]
    fn native_program_report() {
        let text = render(&ElfSpec::linux_x86_64().build(), &[]);
        assert_eq!(value_of(&text, "Path"), Some("/tmp/sample"));
        assert_eq!(
            value_of(&text, "Detected"),
            Some(
                "ELF 64-bit little-endian for x86_64, position-independent executable, \
                 dynamically linked"
            )
        );
        assert_eq!(
            value_of(&text, "Interpreter"),
            Some("/lib64/ld-linux-x86-64.so.2")
        );
        assert_eq!(value_of(&text, "OS ABI"), Some("System V"));
        assert_eq!(value_of(&text, "Recommended"), Some("native Linux"));
        assert_eq!(value_of(&text, "Reasons"), Some("elf-matches-host"));
        assert_eq!(value_of(&text, "Code"), None);
        assert_eq!(value_of(&text, "Not installed"), None);
        assert!(text.ends_with("no compatibility layer.\n"));
    }

    #[test]
    fn values_are_aligned_in_one_column() {
        let text = render(&PeSpec::windows_gui_x86_64().build(), &[]);
        // "Not installed:" is the longest label: 14 characters, then one space.
        for line in text.lines() {
            let (label, value) = line.split_at(15);
            assert!(label.trim_end().ends_with(':'), "{line:?}");
            assert!(!value.starts_with(' '), "{line:?}");
        }
    }

    #[test]
    fn missing_runtime_and_code_are_shown() {
        let text = render(&PeSpec::windows_gui_x86_64().build(), &[]);
        assert_eq!(
            value_of(&text, "Detected"),
            Some("Windows PE (PE32+) for x86_64, graphical")
        );
        assert_eq!(value_of(&text, "Recommended"), Some("Wine"));
        assert_eq!(value_of(&text, "Not installed"), Some("wine"));
        assert_eq!(
            value_of(&text, "Code"),
            Some("TUR-WINDOWS-RUNTIME-001 (Wine is not installed)")
        );
        assert_eq!(
            value_of(&text, "Reasons"),
            Some("pe-windows-application, runtime-not-installed")
        );
    }

    #[test]
    fn driver_report_says_what_it_requires() {
        let driver = PeSpec {
            subsystem: 1,
            ..PeSpec::windows_gui_x86_64()
        };
        let text = render(&driver.build(), &Component::ALL);
        assert!(text.contains("native subsystem (kernel mode)"));
        assert_eq!(value_of(&text, "Requires"), Some("a Windows kernel"));
        assert_eq!(
            value_of(&text, "Recommended"),
            Some("full Windows virtual machine (offer)")
        );
        assert!(text.contains("TUR-WINDOWS-RUNTIME-002"));
    }

    #[test]
    fn mach_o_and_android_reports() {
        let text = render(&macho_thin(CPU_X86_64, 2, true, false), &Component::ALL);
        assert_eq!(
            value_of(&text, "Detected"),
            Some("Mach-O 64-bit x86_64 executable")
        );
        assert_eq!(
            value_of(&text, "Recommended"),
            Some("Darling (experimental)")
        );

        let universal = macho_fat(&[(CPU_X86_64, 2), (CPU_ARM64, 2)], false);
        let text = render(&universal, &Component::ALL);
        assert_eq!(
            value_of(&text, "Detected"),
            Some("Mach-O universal file: x86_64 executable, aarch64 executable")
        );

        let apk = zip(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/arm64-v8a/libapp.so",
        ]);
        let text = render(&apk, &Component::ALL);
        assert_eq!(
            value_of(&text, "Detected"),
            Some("Android application package (APK)")
        );
        assert_eq!(value_of(&text, "Native code"), Some("arm64-v8a"));
        assert_eq!(
            value_of(&text, "Reasons"),
            Some("android-needs-arm-translation")
        );

        let xapk = zip(&["manifest.json", "base.apk", "config.en.apk"]);
        let text = render(&xapk, &Component::ALL);
        assert_eq!(
            value_of(&text, "Packages"),
            Some("2 inside: base.apk, config.en.apk")
        );
        assert_eq!(value_of(&text, "Native code"), Some("not inspected"));
    }

    #[test]
    fn every_sample_renders_without_control_characters() {
        for (name, bytes) in samples() {
            let text = render(&bytes, &[]);
            assert!(value_of(&text, "Recommended").is_some(), "{name}");
            assert!(value_of(&text, "Explanation").is_some(), "{name}");
            assert!(
                !text.chars().any(|c| c.is_control() && c != '\n'),
                "{name}: {text:?}"
            );
        }
    }
}

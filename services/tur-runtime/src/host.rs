// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Facts about the machine a recommendation is made for.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::arch::Arch;

/// An optional runtime component that a recommendation can depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Component {
    /// The Wine compatibility runtime.
    Wine,
    /// The Darling compatibility runtime.
    Darling,
    /// The Android runtime.
    Android,
    /// Translation of ARM native code inside the Android runtime.
    ArmTranslation,
    /// The backend for a full Windows virtual machine.
    WindowsVm,
}

impl Component {
    /// Every component, in a fixed order.
    pub const ALL: [Component; 5] = [
        Component::Wine,
        Component::Darling,
        Component::Android,
        Component::ArmTranslation,
        Component::WindowsVm,
    ];

    /// The component's machine-readable name, for example `arm-translation`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Component::Wine => "wine",
            Component::Darling => "darling",
            Component::Android => "android",
            Component::ArmTranslation => "arm-translation",
            Component::WindowsVm => "windows-vm",
        }
    }

    /// Parses a machine-readable component name.
    pub fn from_name(name: &str) -> Option<Component> {
        Component::ALL
            .into_iter()
            .find(|component| component.as_str() == name)
    }

    /// The component's name as the subject of a sentence.
    pub const fn label(self) -> &'static str {
        match self {
            Component::Wine => "Wine",
            Component::Darling => "Darling",
            Component::Android => "The Android runtime",
            Component::ArmTranslation => "ARM translation for the Android runtime",
            Component::WindowsVm => "The Windows virtual machine backend",
        }
    }
}

/// The host architecture and the runtime components that are installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostFacts {
    /// The host's processor architecture.
    pub arch: Arch,
    /// The components that are installed.
    pub available: Vec<Component>,
}

impl HostFacts {
    /// A host of the given architecture with exactly these components.
    pub fn new(arch: Arch, available: &[Component]) -> Self {
        let mut available = available.to_vec();
        available.sort_unstable();
        available.dedup();
        HostFacts { arch, available }
    }

    /// Whether `component` is installed.
    pub fn has(&self, component: Component) -> bool {
        self.available.contains(&component)
    }

    /// Detects the running host.
    ///
    /// The architecture is the one this program was built for. Components are
    /// detected by looking for their launcher in the directories of `PATH`;
    /// nothing is executed. See [`HostFacts::probe`] for the exact rules.
    pub fn detect() -> Self {
        let arch = Arch::from_name(std::env::consts::ARCH).unwrap_or(Arch::Other);
        let directories: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        let kvm = Path::new("/dev/kvm").exists();
        HostFacts::probe(arch, &directories, kvm)
    }

    /// Builds host facts from a list of directories to search and whether KVM
    /// is present.
    ///
    /// * Wine: a file named `wine` or `wine64`.
    /// * Darling: a file named `darling`.
    /// * Android runtime: a file named `waydroid`. This is provisional: the
    ///   Android runtime's design is not settled.
    /// * Windows VM backend: a file named `qemu-system-x86_64`, and KVM.
    /// * ARM translation: never detected; there is no reliable file to look for.
    pub fn probe(arch: Arch, directories: &[PathBuf], kvm: bool) -> Self {
        let mut available = Vec::new();
        if any_file(directories, &["wine", "wine64"]) {
            available.push(Component::Wine);
        }
        if any_file(directories, &["darling"]) {
            available.push(Component::Darling);
        }
        if any_file(directories, &["waydroid"]) {
            available.push(Component::Android);
        }
        if kvm && any_file(directories, &["qemu-system-x86_64"]) {
            available.push(Component::WindowsVm);
        }
        HostFacts::new(arch, &available)
    }
}

/// Whether any of `directories` holds a file with one of `names`.
fn any_file(directories: &[PathBuf], names: &[&str]) -> bool {
    directories
        .iter()
        .any(|directory| names.iter().any(|name| directory.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::TempDir;

    #[test]
    fn component_names_round_trip() {
        for component in Component::ALL {
            assert_eq!(Component::from_name(component.as_str()), Some(component));
            let json = serde_json::to_value(component).unwrap();
            assert_eq!(json, component.as_str());
        }
        assert_eq!(Component::from_name("proton"), None);
    }

    #[test]
    fn new_sorts_and_removes_duplicates() {
        let host = HostFacts::new(
            Arch::X86_64,
            &[Component::WindowsVm, Component::Wine, Component::Wine],
        );
        assert_eq!(host.available, [Component::Wine, Component::WindowsVm]);
        assert!(host.has(Component::Wine));
        assert!(!host.has(Component::Darling));
    }

    #[test]
    fn probe_finds_launchers_in_the_given_directories() {
        let temp = TempDir::new("probe");
        let empty = temp.path().join("empty");
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("wine64"), b"").unwrap();
        std::fs::write(tools.join("qemu-system-x86_64"), b"").unwrap();
        // A directory with a launcher's name is not a launcher.
        std::fs::create_dir_all(tools.join("darling")).unwrap();

        let directories = [empty, tools];
        let host = HostFacts::probe(Arch::X86_64, &directories, true);
        assert_eq!(host.available, [Component::Wine, Component::WindowsVm]);

        let without_kvm = HostFacts::probe(Arch::X86_64, &directories, false);
        assert_eq!(without_kvm.available, [Component::Wine]);

        let nothing = HostFacts::probe(Arch::Aarch64, &[], true);
        assert_eq!(nothing.arch, Arch::Aarch64);
        assert!(nothing.available.is_empty());
    }

    #[test]
    fn detect_does_not_panic() {
        let host = HostFacts::detect();
        assert!(host.available.len() <= Component::ALL.len());
    }
}

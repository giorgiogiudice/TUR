// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The two things the router is told besides the catalogue entry: facts
//! about the machine, and the user's preferences.
//!
//! Both are plain data, read from JSON. Nothing here detects anything: the
//! caller states the facts. Detection belongs to the runtime manager.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::model::Route;

/// The processor architecture of the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Arch {
    /// 64-bit x86.
    #[serde(rename = "x86_64")]
    X86_64,
    /// 64-bit ARM.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// Any other architecture.
    #[serde(rename = "other")]
    Other,
}

impl Arch {
    /// Every architecture the router distinguishes.
    pub const ALL: [Arch; 3] = [Arch::X86_64, Arch::Aarch64, Arch::Other];

    /// The architecture's name, for example `x86_64`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Other => "other",
        }
    }
}

/// How capable the host's graphics processor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpuClass {
    /// No graphics processor that can accelerate applications.
    None,
    /// Graphics integrated in the processor.
    Integrated,
    /// A discrete graphics card.
    Discrete,
    /// Not known.
    #[default]
    Unknown,
}

impl GpuClass {
    /// Every class.
    pub const ALL: [GpuClass; 4] = [
        GpuClass::None,
        GpuClass::Integrated,
        GpuClass::Discrete,
        GpuClass::Unknown,
    ];
}

/// Facts about the machine a routing decision is made for.
///
/// A field left out of the JSON takes its default: an x86-64 machine with
/// Flatpak and nothing optional installed or enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HostFacts {
    /// The processor architecture.
    pub arch: Arch,
    /// Flatpak is installed.
    pub flatpak: bool,
    /// A tool that runs containers of other distributions is installed.
    pub distro_containers: bool,
    /// Wine is installed.
    pub wine: bool,
    /// Proton is available through `umu-launcher`.
    pub proton: bool,
    /// The Android runtime is enabled.
    pub android_runtime: bool,
    /// The user has supplied an ARM translation layer for the Android
    /// runtime. TUR never ships one.
    pub arm_translation: bool,
    /// The user has opted into Google Play (ADR-0011).
    pub google_play: bool,
    /// A Windows guest is set up.
    pub windows_guest: bool,
    /// A Windows licence for the guest is present.
    pub windows_licence: bool,
    /// The user has enabled Darling.
    pub darling_enabled: bool,
    /// The class of the graphics processor.
    pub gpu: GpuClass,
    /// Installed memory in mebibytes, when known.
    pub memory_mib: Option<u64>,
}

impl Default for HostFacts {
    fn default() -> Self {
        HostFacts {
            arch: Arch::X86_64,
            flatpak: true,
            distro_containers: false,
            wine: false,
            proton: false,
            android_runtime: false,
            arm_translation: false,
            google_play: false,
            windows_guest: false,
            windows_licence: false,
            darling_enabled: false,
            gpu: GpuClass::Unknown,
            memory_mib: None,
        }
    }
}

/// The user's routing preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Preferences {
    /// Never offer a virtual machine.
    pub never_use_virtual_machine: bool,
    /// Among editions on the same rung of the ladder, list open-source ones
    /// first.
    pub prefer_open_source: bool,
    /// Routes the user has pinned, by application ID.
    pub pinned_routes: BTreeMap<String, Route>,
}

impl Preferences {
    /// The route pinned for the application `id`, if any.
    pub fn pinned_route(&self, id: &str) -> Option<Route> {
        self.pinned_routes.get(id).copied()
    }
}

/// Parses a JSON object into `T`. Anything that is not an object is
/// refused: left to itself, a derived reader would also accept a list of
/// values in field order.
fn parse_object<T: DeserializeOwned>(json: &str) -> Result<T, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("expected a JSON object".to_owned());
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Parses host facts from a JSON object. Unknown fields are refused.
pub fn parse_host_facts(json: &str) -> Result<HostFacts, String> {
    parse_object(json)
}

/// Parses preferences from a JSON object. Unknown fields are refused, and
/// so is a pin to `no-route`, which is a result and not a route.
pub fn parse_preferences(json: &str) -> Result<Preferences, String> {
    let preferences: Preferences = parse_object(json)?;
    for (id, route) in &preferences.pinned_routes {
        if *route == Route::NoRoute {
            return Err(format!(
                "pinned_routes: {id} is pinned to no-route, which is not a route"
            ));
        }
    }
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_host_is_a_plain_x86_64_machine_with_flatpak_only() {
        let host = HostFacts::default();
        assert_eq!(host.arch, Arch::X86_64);
        assert!(host.flatpak);
        assert!(!host.wine && !host.proton && !host.android_runtime && !host.windows_guest);
        assert!(!host.windows_licence && !host.darling_enabled && !host.distro_containers);
        assert_eq!(host.gpu, GpuClass::Unknown);
        assert_eq!(host.memory_mib, None);
        assert_eq!(parse_host_facts("{}").unwrap(), host);
    }

    #[test]
    fn parses_host_facts_and_fills_in_what_is_left_out() {
        let host = parse_host_facts(
            r#"{"arch": "aarch64", "wine": true, "gpu": "discrete", "memory_mib": 32768}"#,
        )
        .unwrap();
        assert_eq!(host.arch, Arch::Aarch64);
        assert!(host.wine && host.flatpak && !host.proton);
        assert_eq!(host.gpu, GpuClass::Discrete);
        assert_eq!(host.memory_mib, Some(32768));
    }

    #[test]
    fn host_facts_round_trip_through_json() {
        let host = HostFacts {
            arch: Arch::Other,
            proton: true,
            gpu: GpuClass::Integrated,
            memory_mib: Some(8192),
            ..HostFacts::default()
        };
        let json = serde_json::to_string(&host).unwrap();
        assert_eq!(parse_host_facts(&json).unwrap(), host);
        for arch in Arch::ALL {
            let json = serde_json::to_value(arch).unwrap();
            assert_eq!(json, arch.as_str());
        }
    }

    #[test]
    fn rejects_unknown_fields_and_wrong_values() {
        assert!(parse_host_facts(r#"{"wine": true, "bottles": true}"#).is_err());
        assert!(parse_host_facts(r#"{"arch": "sparc"}"#).is_err());
        assert!(parse_host_facts(r#"{"gpu": "fast"}"#).is_err());
        assert!(parse_host_facts(r#"{"wine": "yes"}"#).is_err());
        assert!(parse_host_facts("[]").is_err());
        assert!(parse_host_facts("[\"x86_64\", true]").is_err());
        assert!(parse_host_facts("null").is_err());
        assert!(parse_host_facts("{").is_err());
        assert!(parse_preferences("[]").is_err());
        assert!(parse_preferences("[true, false, {}]").is_err());
        assert!(parse_preferences(r#"{"never_use_vm": true}"#).is_err());
        assert!(parse_preferences(r#"{"pinned_routes": {"org.example.app": "vm"}}"#).is_err());
        assert!(
            parse_preferences(r#"{"pinned_routes": {"org.example.app": "no-route"}}"#).is_err()
        );
    }

    #[test]
    fn parses_preferences() {
        assert_eq!(parse_preferences("{}").unwrap(), Preferences::default());
        let preferences = parse_preferences(
            r#"{"never_use_virtual_machine": true, "prefer_open_source": true,
                "pinned_routes": {"org.example.app": "windows-guest"}}"#,
        )
        .unwrap();
        assert!(preferences.never_use_virtual_machine && preferences.prefer_open_source);
        assert_eq!(
            preferences.pinned_route("org.example.app"),
            Some(Route::WindowsGuest)
        );
        assert_eq!(preferences.pinned_route("org.example.other"), None);
    }
}

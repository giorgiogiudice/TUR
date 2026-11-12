// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Everything a decision depends on besides the deployments: power, the
//! network, free space, boot health, a repair session and the user's
//! settings.
//!
//! These are plain values handed to the policy. Nothing in this slice
//! measures them on a live machine: the command line takes them as options,
//! and the update service will gather them (UPower, NetworkManager, the
//! filesystem, `tur-boot-health`) in a later slice. A value that is not known
//! is `unknown`, and the policy says what it does with each unknown.

use serde::Serialize;

use crate::health::BootHealth;

/// One gibibyte.
pub const GIB: u64 = 1024 * 1024 * 1024;

/// What the computer runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Power {
    /// Not known. Treated as mains: a desktop computer reports no battery.
    #[default]
    Unknown,
    /// Mains power.
    Mains,
    /// Battery, with the charge in percent if it is known.
    Battery {
        /// The charge, 0 to 100.
        percent: Option<u8>,
    },
}

impl Serialize for Power {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (source, percent) = match self {
            Power::Unknown => ("unknown", None),
            Power::Mains => ("mains", None),
            Power::Battery { percent } => ("battery", *percent),
        };
        let mut fields = serializer.serialize_struct("Power", 2)?;
        fields.serialize_field("source", source)?;
        fields.serialize_field("battery_percent", &percent)?;
        fields.end()
    }
}

string_enum! {
    /// Whether the connection is charged by volume.
    pub enum Metered {
        /// Not known. Treated as not metered, as NetworkManager's own
        /// "unknown" is by desktop software.
        Unknown => "unknown",
        /// Not metered.
        No => "no",
        /// Metered.
        Yes => "yes",
    }
}

/// The network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Network {
    /// Not known. Treated as online: the fetch itself reports a failure.
    #[default]
    Unknown,
    /// No connection.
    Offline,
    /// Connected.
    Online {
        /// Whether the connection is metered.
        metered: Metered,
    },
}

impl Network {
    /// Whether the connection is known to be metered.
    pub fn is_metered(self) -> bool {
        matches!(
            self,
            Network::Online {
                metered: Metered::Yes
            }
        )
    }
}

impl Serialize for Network {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (state, metered) = match self {
            Network::Unknown => ("unknown", Metered::Unknown),
            Network::Offline => ("offline", Metered::Unknown),
            Network::Online { metered } => ("online", *metered),
        };
        let mut fields = serializer.serialize_struct("Network", 2)?;
        fields.serialize_field("state", state)?;
        fields.serialize_field("metered", &metered)?;
        fields.end()
    }
}

/// The two switches of Settings ▸ Updates (desktop design, section 11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Settings {
    /// "Download updates automatically". On by default.
    pub automatic_download: bool,
    /// "Download on metered connections". Off by default.
    pub download_on_metered: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            automatic_download: true,
            download_on_metered: false,
        }
    }
}

/// The numbers the policy compares against. They are judgements, written
/// down in one place so that they can be changed in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Limits {
    /// On battery, an automatic download starts only at or above this charge.
    pub automatic_battery_percent: u8,
    /// On battery, a download a person asked for starts only at or above
    /// this charge.
    pub requested_battery_percent: u8,
    /// The size assumed for an update whose size is not known.
    pub assumed_update_bytes: u64,
    /// Free space that must remain after the update is stored.
    pub reserve_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            automatic_battery_percent: 50,
            requested_battery_percent: 20,
            assumed_update_bytes: 8 * GIB,
            reserve_bytes: 2 * GIB,
        }
    }
}

/// Everything a decision depends on besides the deployments.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Conditions {
    /// What the computer runs on.
    pub power: Power,
    /// The network.
    pub network: Network,
    /// Free space on the system disk, in bytes, if known.
    pub free_bytes: Option<u64>,
    /// The size of the available update on disk, in bytes, if known.
    pub update_bytes: Option<u64>,
    /// What boot health recorded. `None` when it is not known.
    pub boot_health: Option<BootHealth>,
    /// A repair session is open.
    pub repair_session: bool,
    /// The user's settings.
    pub settings: Settings,
    /// The numbers the policy compares against.
    pub limits: Limits,
}

impl Conditions {
    /// The space an update needs: its size, or the assumed size, plus the
    /// reserve.
    pub fn required_bytes(&self) -> u64 {
        self.update_bytes
            .unwrap_or(self.limits.assumed_update_bytes)
            .saturating_add(self.limits.reserve_bytes)
    }

    /// Conditions under which nothing holds an update back: mains power, an
    /// unmetered connection, ample space, healthy boot health. For tests and
    /// examples.
    pub fn favourable() -> Conditions {
        Conditions {
            power: Power::Mains,
            network: Network::Online {
                metered: Metered::No,
            },
            free_bytes: Some(200 * GIB),
            update_bytes: None,
            boot_health: Some(BootHealth::healthy()),
            repair_session: false,
            settings: Settings::default(),
            limits: Limits::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_ones_the_desktop_shows() {
        let settings = Settings::default();
        assert!(settings.automatic_download);
        assert!(!settings.download_on_metered);
        let conditions = Conditions::default();
        assert_eq!(conditions.power, Power::Unknown);
        assert_eq!(conditions.network, Network::Unknown);
        assert!(conditions.boot_health.is_none());
        assert!(!conditions.repair_session);
    }

    #[test]
    fn the_space_an_update_needs() {
        let mut conditions = Conditions::default();
        assert_eq!(conditions.required_bytes(), 10 * GIB);
        conditions.update_bytes = Some(3 * GIB);
        assert_eq!(conditions.required_bytes(), 5 * GIB);
        conditions.update_bytes = Some(u64::MAX);
        assert_eq!(conditions.required_bytes(), u64::MAX);
    }

    #[test]
    fn only_a_connection_known_to_be_metered_is_metered() {
        assert!(
            Network::Online {
                metered: Metered::Yes
            }
            .is_metered()
        );
        for network in [
            Network::Unknown,
            Network::Offline,
            Network::Online {
                metered: Metered::No,
            },
            Network::Online {
                metered: Metered::Unknown,
            },
        ] {
            assert!(!network.is_metered(), "{network:?}");
        }
    }

    #[test]
    fn the_report_form() {
        let value = serde_json::to_value(Conditions::favourable()).unwrap();
        assert_eq!(value["power"]["source"], "mains");
        assert!(value["power"]["battery_percent"].is_null());
        assert_eq!(value["network"]["state"], "online");
        assert_eq!(value["network"]["metered"], "no");
        assert_eq!(value["settings"]["automatic_download"], true);
        assert_eq!(value["limits"]["automatic_battery_percent"], 50);
        assert_eq!(value["boot_health"]["counting_usable"], true);
        let battery = serde_json::to_value(Power::Battery { percent: Some(41) }).unwrap();
        assert_eq!(battery["source"], "battery");
        assert_eq!(battery["battery_percent"], 41);
        let offline = serde_json::to_value(Network::Offline).unwrap();
        assert_eq!(offline["state"], "offline");
        assert_eq!(offline["metered"], "unknown");
    }
}

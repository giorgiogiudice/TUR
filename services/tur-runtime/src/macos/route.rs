// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The order in which the routes for a macOS package are tried, and the
//! refusal when none applies.
//!
//! 1. **A native Linux edition** of the application, when the catalogue
//!    knows one: it always wins (ADR-0007).
//! 2. **Re-hosting**: when the package's payload is portable (an Electron,
//!    Java, Python or .NET application whose platform pieces have Linux
//!    builds), it runs on the native runtime (docs/reports/exp-rehost-1.md).
//!    Whether a payload is portable is the re-hosting component's verdict;
//!    this function takes it as an input and does not compute it.
//! 3. **Darling**, only when the catalogue records the application as
//!    measured to work in it (a record rated `good` or `usable`), and the
//!    analysis of the program refuses nothing.
//! 4. Otherwise **no route**, with every reason.
//!
//! The same order is the catalogue router's for a macOS edition
//! (docs/architecture/catalogue-and-routing.md): Darling last, and offered
//! only when measured. This function is the runtime manager's half, for a
//! file the person already has.

use serde::Serialize;

use crate::codes::Code;

/// A route for a macOS package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MacRoute {
    /// The application's native Linux edition.
    LinuxEdition,
    /// The package's portable payload on the native runtime.
    Rehost,
    /// The package in Darling.
    Darling,
}

impl MacRoute {
    /// The route's name.
    pub const fn as_str(self) -> &'static str {
        match self {
            MacRoute::LinuxEdition => "linux-edition",
            MacRoute::Rehost => "rehost",
            MacRoute::Darling => "darling",
        }
    }
}

/// The re-hosting component's verdict on the package's payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RehostVerdict {
    /// The payload runs on the native runtime.
    Portable,
    /// It does not: native Mac code that has no Linux build.
    NotPortable,
    /// Not known: the package was not examined for re-hosting.
    Unknown,
}

impl RehostVerdict {
    /// The verdict for a name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "portable" => Some(RehostVerdict::Portable),
            "not-portable" => Some(RehostVerdict::NotPortable),
            "unknown" => Some(RehostVerdict::Unknown),
            _ => None,
        }
    }
}

/// What the decision is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteFacts {
    /// Whether the catalogue knows a native Linux edition.
    pub linux_edition: bool,
    /// The re-hosting component's verdict.
    pub rehost: RehostVerdict,
    /// Whether the catalogue records the application as measured to work in
    /// Darling.
    pub darling_measured: bool,
    /// The refusal of the program's analysis, when it refuses: its code and
    /// message.
    pub darling_refusal: Option<(Code, String)>,
}

/// One route as the decision saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Considered {
    /// The route.
    pub route: MacRoute,
    /// Whether it can be taken.
    pub usable: bool,
    /// Why, or why not, in plain language.
    pub reason: String,
}

/// The decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteDecision {
    /// The route to take, when there is one.
    pub chosen: Option<MacRoute>,
    /// Every route in the order it was considered.
    pub considered: Vec<Considered>,
    /// The code when no route is taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Code>,
    /// The decision in plain language.
    pub message: String,
}

/// Decides the route for a macOS package.
pub fn decide(facts: &RouteFacts) -> RouteDecision {
    let linux = Considered {
        route: MacRoute::LinuxEdition,
        usable: facts.linux_edition,
        reason: if facts.linux_edition {
            "The application has a native Linux edition; it is used instead of the Mac package."
                .to_owned()
        } else {
            "No native Linux edition is known.".to_owned()
        },
    };
    let rehost = Considered {
        route: MacRoute::Rehost,
        usable: facts.rehost == RehostVerdict::Portable,
        reason: match facts.rehost {
            RehostVerdict::Portable => {
                "The package's payload is portable and runs on the native runtime.".to_owned()
            }
            RehostVerdict::NotPortable => {
                "The package's payload is native Mac code, which re-hosting cannot run."
                    .to_owned()
            }
            RehostVerdict::Unknown => {
                "The package was not examined for re-hosting.".to_owned()
            }
        },
    };
    let darling_usable = facts.darling_measured && facts.darling_refusal.is_none();
    let darling_reason = match (&facts.darling_refusal, facts.darling_measured) {
        (Some((_, message)), _) => format!("Darling cannot run it: {message}"),
        (None, false) => "Darling is offered only for applications measured to work in it, and \
                          this one has not been."
            .to_owned(),
        (None, true) => {
            "It is measured to work in Darling, TUR's experimental macOS layer.".to_owned()
        }
    };
    let darling = Considered {
        route: MacRoute::Darling,
        usable: darling_usable,
        reason: darling_reason,
    };
    let considered = vec![linux, rehost, darling];

    let chosen = considered
        .iter()
        .find(|route| route.usable)
        .map(|route| route.route);
    let (refusal, message) = match chosen {
        Some(route) => {
            let reason = considered
                .iter()
                .find(|considered| considered.route == route)
                .map(|considered| considered.reason.clone())
                .unwrap_or_default();
            (None, reason)
        }
        None => {
            let reasons: Vec<&str> = considered
                .iter()
                .map(|considered| considered.reason.as_str())
                .collect();
            (
                Some(Code::MacosNoRoute),
                format!("No route runs this application. {}", reasons.join(" ")),
            )
        }
    };
    RouteDecision {
        chosen,
        considered,
        refusal,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> RouteFacts {
        RouteFacts {
            linux_edition: false,
            rehost: RehostVerdict::Unknown,
            darling_measured: false,
            darling_refusal: None,
        }
    }

    #[test]
    fn the_order_is_linux_then_rehost_then_measured_darling() {
        // Everything available: the Linux edition wins.
        let all = RouteFacts {
            linux_edition: true,
            rehost: RehostVerdict::Portable,
            darling_measured: true,
            ..facts()
        };
        assert_eq!(decide(&all).chosen, Some(MacRoute::LinuxEdition));

        // Without it, re-hosting before Darling.
        let no_linux = RouteFacts {
            linux_edition: false,
            ..all.clone()
        };
        assert_eq!(decide(&no_linux).chosen, Some(MacRoute::Rehost));

        // A payload that is not portable: Darling, because it is measured.
        let darling = RouteFacts {
            rehost: RehostVerdict::NotPortable,
            ..no_linux
        };
        let decision = decide(&darling);
        assert_eq!(decision.chosen, Some(MacRoute::Darling));
        assert!(decision.refusal.is_none());
        let order: Vec<MacRoute> = decision.considered.iter().map(|c| c.route).collect();
        assert_eq!(
            order,
            [MacRoute::LinuxEdition, MacRoute::Rehost, MacRoute::Darling]
        );
    }

    #[test]
    fn darling_is_never_chosen_unmeasured_or_refused() {
        let unmeasured = decide(&facts());
        assert_eq!(unmeasured.chosen, None);
        assert_eq!(unmeasured.refusal, Some(Code::MacosNoRoute));
        assert!(unmeasured.message.contains("measured"));

        let refused = RouteFacts {
            darling_measured: true,
            darling_refusal: Some((
                Code::MacosArm64Only,
                "It is built only for Apple silicon (arm64).".to_owned(),
            )),
            ..facts()
        };
        let decision = decide(&refused);
        assert_eq!(decision.chosen, None);
        assert!(decision.message.contains("Apple silicon"));
    }

    #[test]
    fn verdicts_have_names() {
        assert_eq!(
            RehostVerdict::from_name("portable"),
            Some(RehostVerdict::Portable)
        );
        assert_eq!(RehostVerdict::from_name("yes"), None);
        assert_eq!(MacRoute::Darling.as_str(), "darling");
    }
}

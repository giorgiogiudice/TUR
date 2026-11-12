// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The USB hand-off state machine, as types only.
//!
//! Specification §10.4 gives the states a USB device passes through on its way
//! to a guest and back:
//!
//! ```text
//! discovered -> identified -> driver_bound -> permission_assigned -> available
//!
//! available -> handoff_requested -> host_driver_unbound -> guest_attached
//!           -> guest_owned -> guest_detached -> host_rebind_attempted -> available
//! ```
//!
//! This module defines those states, the events that move between them and
//! [`transition`], a pure function that says whether a move is legal. **Nothing
//! here touches hardware.** No code in this crate unbinds a driver, attaches a
//! device to a guest or changes who owns a device; a later change that does
//! must drive it through this function and log every transition.
//!
//! Three things are added to the two chains of the specification, each needed
//! to honour its text:
//!
//! * [`HandoffState::Unavailable`], with a reason. "If guest detach fails, do
//!   not silently attempt conflicting ownership. Show the device as
//!   unavailable and provide a recovery action." There is no way out of
//!   `Unavailable` after a failed detach except confirmation that the guest
//!   has released the device, or removal of the device.
//! * Failure and cancellation edges, so that every step that can fail has a
//!   defined result.
//! * [`HandoffState::Removed`]: a device can be unplugged in any state.

use std::fmt;

use serde::Serialize;

/// Why a device is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The guest did not release the device. The guest may still be using it.
    GuestDetachFailed,
    /// The guest released the device, but the host driver could not be bound
    /// again.
    HostRebindFailed,
}

/// The state of one USB device with respect to hand-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum HandoffState {
    /// The kernel has announced the device.
    Discovered,
    /// Its descriptors have been read.
    Identified,
    /// A host driver is bound.
    DriverBound,
    /// Access permissions have been set.
    PermissionAssigned,
    /// The host owns the device and it is ready for use.
    Available,
    /// A hand-off to a guest has been requested and authorised.
    HandoffRequested,
    /// The host driver has been unbound. Nobody owns the device.
    HostDriverUnbound,
    /// The device has been attached to the guest.
    GuestAttached,
    /// The guest has claimed the device.
    GuestOwned,
    /// The guest has released the device. Nobody owns the device.
    GuestDetached,
    /// The host driver is being bound again.
    HostRebindAttempted,
    /// Ownership is uncertain or the device could not be returned.
    Unavailable(UnavailableReason),
    /// The device has been unplugged. Terminal: a device plugged in again
    /// starts at `Discovered` as a new device.
    Removed,
}

/// Who may be driving the device in a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// A host driver, or the host with no driver bound yet.
    Host,
    /// The guest.
    Guest,
    /// Neither: the device is between owners.
    Nobody,
    /// It cannot be said. The device must not be given to anyone.
    Uncertain,
}

/// Something that happened to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffEvent {
    /// The device's descriptors were read.
    Identified,
    /// A host driver bound.
    DriverBound,
    /// Access permissions were set.
    PermissionAssigned,
    /// The device became ready for use.
    MadeAvailable,
    /// A hand-off was requested and authorised.
    HandoffRequested,
    /// The request was withdrawn or refused before anything was changed.
    HandoffCancelled,
    /// The host driver was unbound.
    HostDriverUnbound,
    /// The host driver could not be unbound; it still owns the device.
    HostUnbindFailed,
    /// The device was attached to the guest.
    GuestAttached,
    /// The device could not be attached to the guest.
    GuestAttachFailed,
    /// The guest claimed the device.
    GuestClaimed,
    /// The guest released the device.
    GuestDetached,
    /// The guest did not release the device.
    GuestDetachFailed,
    /// It was confirmed that the guest no longer holds the device (the guest
    /// was shut down, for example). The recovery action after a failed detach.
    GuestReleaseConfirmed,
    /// Binding the host driver again was started.
    HostRebindStarted,
    /// The host driver bound again.
    HostRebindSucceeded,
    /// The host driver could not be bound again.
    HostRebindFailed,
    /// Binding the host driver again is being retried. The recovery action
    /// after a failed rebind.
    HostRebindRetried,
    /// The device was unplugged.
    DeviceRemoved,
}

impl HandoffState {
    /// Every state.
    pub const ALL: [HandoffState; 14] = [
        HandoffState::Discovered,
        HandoffState::Identified,
        HandoffState::DriverBound,
        HandoffState::PermissionAssigned,
        HandoffState::Available,
        HandoffState::HandoffRequested,
        HandoffState::HostDriverUnbound,
        HandoffState::GuestAttached,
        HandoffState::GuestOwned,
        HandoffState::GuestDetached,
        HandoffState::HostRebindAttempted,
        HandoffState::Unavailable(UnavailableReason::GuestDetachFailed),
        HandoffState::Unavailable(UnavailableReason::HostRebindFailed),
        HandoffState::Removed,
    ];

    /// The state's name as the specification writes it.
    pub const fn name(self) -> &'static str {
        match self {
            HandoffState::Discovered => "discovered",
            HandoffState::Identified => "identified",
            HandoffState::DriverBound => "driver_bound",
            HandoffState::PermissionAssigned => "permission_assigned",
            HandoffState::Available => "available",
            HandoffState::HandoffRequested => "handoff_requested",
            HandoffState::HostDriverUnbound => "host_driver_unbound",
            HandoffState::GuestAttached => "guest_attached",
            HandoffState::GuestOwned => "guest_owned",
            HandoffState::GuestDetached => "guest_detached",
            HandoffState::HostRebindAttempted => "host_rebind_attempted",
            HandoffState::Unavailable(_) => "unavailable",
            HandoffState::Removed => "removed",
        }
    }

    /// Who may be driving the device in this state.
    pub const fn owner(self) -> Owner {
        match self {
            HandoffState::Discovered
            | HandoffState::Identified
            | HandoffState::DriverBound
            | HandoffState::PermissionAssigned
            | HandoffState::Available
            | HandoffState::HandoffRequested => Owner::Host,
            HandoffState::GuestAttached | HandoffState::GuestOwned => Owner::Guest,
            HandoffState::HostDriverUnbound
            | HandoffState::GuestDetached
            | HandoffState::HostRebindAttempted
            | HandoffState::Unavailable(UnavailableReason::HostRebindFailed)
            | HandoffState::Removed => Owner::Nobody,
            HandoffState::Unavailable(UnavailableReason::GuestDetachFailed) => Owner::Uncertain,
        }
    }

    /// Whether the device can be used by the host or offered for hand-off.
    pub const fn is_available(self) -> bool {
        matches!(self, HandoffState::Available)
    }
}

impl fmt::Display for HandoffState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl HandoffEvent {
    /// Every event.
    pub const ALL: [HandoffEvent; 19] = [
        HandoffEvent::Identified,
        HandoffEvent::DriverBound,
        HandoffEvent::PermissionAssigned,
        HandoffEvent::MadeAvailable,
        HandoffEvent::HandoffRequested,
        HandoffEvent::HandoffCancelled,
        HandoffEvent::HostDriverUnbound,
        HandoffEvent::HostUnbindFailed,
        HandoffEvent::GuestAttached,
        HandoffEvent::GuestAttachFailed,
        HandoffEvent::GuestClaimed,
        HandoffEvent::GuestDetached,
        HandoffEvent::GuestDetachFailed,
        HandoffEvent::GuestReleaseConfirmed,
        HandoffEvent::HostRebindStarted,
        HandoffEvent::HostRebindSucceeded,
        HandoffEvent::HostRebindFailed,
        HandoffEvent::HostRebindRetried,
        HandoffEvent::DeviceRemoved,
    ];
}

/// An event that is not allowed in a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IllegalTransition {
    /// The state the device was in.
    pub from: HandoffState,
    /// The event that is not allowed there.
    pub event: HandoffEvent,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "event {:?} is not allowed in state {}",
            self.event, self.from
        )
    }
}

impl std::error::Error for IllegalTransition {}

/// The state that follows `state` when `event` happens, or an error if the
/// event is not allowed there. The state is unchanged on error.
///
/// This is the whole of the state machine. It performs nothing.
pub fn transition(
    state: HandoffState,
    event: HandoffEvent,
) -> Result<HandoffState, IllegalTransition> {
    use HandoffEvent as E;
    use HandoffState as S;
    use UnavailableReason as R;

    let next = match (state, event) {
        // A removed device stays removed.
        (S::Removed, _) => None,
        // A device can be unplugged in any other state.
        (_, E::DeviceRemoved) => Some(S::Removed),

        // Arrival (§10.4, first chain).
        (S::Discovered, E::Identified) => Some(S::Identified),
        (S::Identified, E::DriverBound) => Some(S::DriverBound),
        (S::DriverBound, E::PermissionAssigned) => Some(S::PermissionAssigned),
        (S::PermissionAssigned, E::MadeAvailable) => Some(S::Available),

        // Hand-off to the guest (§10.4, second chain).
        (S::Available, E::HandoffRequested) => Some(S::HandoffRequested),
        (S::HandoffRequested, E::HostDriverUnbound) => Some(S::HostDriverUnbound),
        (S::HostDriverUnbound, E::GuestAttached) => Some(S::GuestAttached),
        (S::GuestAttached, E::GuestClaimed) => Some(S::GuestOwned),

        // And back.
        (S::GuestAttached | S::GuestOwned, E::GuestDetached) => Some(S::GuestDetached),
        (S::GuestDetached, E::HostRebindStarted) => Some(S::HostRebindAttempted),
        (S::HostRebindAttempted, E::HostRebindSucceeded) => Some(S::Available),

        // Before anything was changed, the host still owns the device.
        (S::HandoffRequested, E::HandoffCancelled | E::HostUnbindFailed) => Some(S::Available),
        // The guest never got the device: give it back to the host.
        (S::HostDriverUnbound, E::GuestAttachFailed) => Some(S::HostRebindAttempted),

        // The guest may still hold the device: do not touch it.
        (S::GuestAttached | S::GuestOwned, E::GuestDetachFailed) => {
            Some(S::Unavailable(R::GuestDetachFailed))
        }
        (S::Unavailable(R::GuestDetachFailed), E::GuestReleaseConfirmed) => Some(S::GuestDetached),

        // Nobody holds the device, and the host could not take it back.
        (S::HostRebindAttempted, E::HostRebindFailed) => Some(S::Unavailable(R::HostRebindFailed)),
        (S::Unavailable(R::HostRebindFailed), E::HostRebindRetried) => Some(S::HostRebindAttempted),

        _ => None,
    };
    next.ok_or(IllegalTransition { from: state, event })
}

#[cfg(test)]
mod tests {
    use super::*;
    use HandoffEvent as E;
    use HandoffState as S;
    use UnavailableReason as R;

    /// Every legal transition other than removal. Anything not listed here,
    /// and not a removal, must be refused.
    const LEGAL: [(HandoffState, HandoffEvent, HandoffState); 20] = [
        (S::Discovered, E::Identified, S::Identified),
        (S::Identified, E::DriverBound, S::DriverBound),
        (S::DriverBound, E::PermissionAssigned, S::PermissionAssigned),
        (S::PermissionAssigned, E::MadeAvailable, S::Available),
        (S::Available, E::HandoffRequested, S::HandoffRequested),
        (S::HandoffRequested, E::HandoffCancelled, S::Available),
        (S::HandoffRequested, E::HostUnbindFailed, S::Available),
        (
            S::HandoffRequested,
            E::HostDriverUnbound,
            S::HostDriverUnbound,
        ),
        (S::HostDriverUnbound, E::GuestAttached, S::GuestAttached),
        (
            S::HostDriverUnbound,
            E::GuestAttachFailed,
            S::HostRebindAttempted,
        ),
        (S::GuestAttached, E::GuestClaimed, S::GuestOwned),
        (S::GuestAttached, E::GuestDetached, S::GuestDetached),
        (
            S::GuestAttached,
            E::GuestDetachFailed,
            S::Unavailable(R::GuestDetachFailed),
        ),
        (S::GuestOwned, E::GuestDetached, S::GuestDetached),
        (
            S::GuestOwned,
            E::GuestDetachFailed,
            S::Unavailable(R::GuestDetachFailed),
        ),
        (
            S::GuestDetached,
            E::HostRebindStarted,
            S::HostRebindAttempted,
        ),
        (S::HostRebindAttempted, E::HostRebindSucceeded, S::Available),
        (
            S::HostRebindAttempted,
            E::HostRebindFailed,
            S::Unavailable(R::HostRebindFailed),
        ),
        (
            S::Unavailable(R::GuestDetachFailed),
            E::GuestReleaseConfirmed,
            S::GuestDetached,
        ),
        (
            S::Unavailable(R::HostRebindFailed),
            E::HostRebindRetried,
            S::HostRebindAttempted,
        ),
    ];

    fn run(
        start: HandoffState,
        events: &[HandoffEvent],
    ) -> Result<HandoffState, IllegalTransition> {
        events
            .iter()
            .try_fold(start, |state, event| transition(state, *event))
    }

    #[test]
    fn the_specification_chains_are_legal() {
        let arrived = run(
            S::Discovered,
            &[
                E::Identified,
                E::DriverBound,
                E::PermissionAssigned,
                E::MadeAvailable,
            ],
        );
        assert_eq!(arrived, Ok(S::Available));

        let returned = run(
            S::Available,
            &[
                E::HandoffRequested,
                E::HostDriverUnbound,
                E::GuestAttached,
                E::GuestClaimed,
                E::GuestDetached,
                E::HostRebindStarted,
                E::HostRebindSucceeded,
            ],
        );
        assert_eq!(returned, Ok(S::Available));
    }

    #[test]
    fn every_pair_of_state_and_event_is_decided_by_the_table() {
        let mut legal = 0;
        let mut illegal = 0;
        for state in HandoffState::ALL {
            for event in HandoffEvent::ALL {
                let expected = if state == S::Removed {
                    None
                } else if event == E::DeviceRemoved {
                    Some(S::Removed)
                } else {
                    LEGAL
                        .iter()
                        .find(|(from, on, _)| *from == state && *on == event)
                        .map(|(_, _, to)| *to)
                };
                match expected {
                    Some(to) => {
                        assert_eq!(transition(state, event), Ok(to), "{state:?} + {event:?}");
                        legal += 1;
                    }
                    None => {
                        assert_eq!(
                            transition(state, event),
                            Err(IllegalTransition { from: state, event }),
                            "{state:?} + {event:?}"
                        );
                        illegal += 1;
                    }
                }
            }
        }
        // 20 listed transitions and removal from each of the 13 other states.
        assert_eq!(legal, 20 + 13);
        assert_eq!(legal + illegal, 14 * 19);
    }

    #[test]
    fn steps_cannot_be_skipped() {
        // Straight to the guest without unbinding the host driver.
        assert!(transition(S::Available, E::GuestAttached).is_err());
        assert!(transition(S::HandoffRequested, E::GuestAttached).is_err());
        // A hand-off of a device that is not yet available.
        assert!(transition(S::DriverBound, E::HandoffRequested).is_err());
        // Declaring the device available without rebinding it.
        assert!(transition(S::GuestDetached, E::HostRebindSucceeded).is_err());
        assert!(transition(S::GuestOwned, E::MadeAvailable).is_err());
        // A second hand-off while one is in progress.
        assert!(transition(S::GuestOwned, E::HandoffRequested).is_err());
        // Cancelling after the host driver is gone is not a cancellation.
        assert!(transition(S::HostDriverUnbound, E::HandoffCancelled).is_err());
    }

    #[test]
    fn a_failed_guest_detach_never_leads_to_a_host_rebind() {
        let stuck = transition(S::GuestOwned, E::GuestDetachFailed).unwrap();
        assert_eq!(stuck, S::Unavailable(R::GuestDetachFailed));
        assert_eq!(stuck.owner(), Owner::Uncertain);
        assert!(!stuck.is_available());

        // Only confirmation of release, or removal, leaves this state.
        for event in HandoffEvent::ALL {
            let result = transition(stuck, event);
            match event {
                E::GuestReleaseConfirmed => assert_eq!(result, Ok(S::GuestDetached)),
                E::DeviceRemoved => assert_eq!(result, Ok(S::Removed)),
                _ => assert!(result.is_err(), "{event:?}"),
            }
        }
        // In particular the host driver is not rebound while the guest may
        // still hold the device.
        assert!(transition(stuck, E::HostRebindStarted).is_err());
        assert!(transition(stuck, E::HostRebindRetried).is_err());
        assert!(transition(stuck, E::HostRebindSucceeded).is_err());

        // The recovery path, once release is confirmed.
        let recovered = run(
            stuck,
            &[
                E::GuestReleaseConfirmed,
                E::HostRebindStarted,
                E::HostRebindSucceeded,
            ],
        );
        assert_eq!(recovered, Ok(S::Available));
    }

    #[test]
    fn a_failed_rebind_can_be_retried() {
        let failed = run(
            S::GuestDetached,
            &[E::HostRebindStarted, E::HostRebindFailed],
        )
        .unwrap();
        assert_eq!(failed, S::Unavailable(R::HostRebindFailed));
        assert_eq!(failed.owner(), Owner::Nobody);
        assert_eq!(
            run(failed, &[E::HostRebindRetried, E::HostRebindSucceeded]),
            Ok(S::Available)
        );
        // It is not a guest problem, so guest confirmation does not apply.
        assert!(transition(failed, E::GuestReleaseConfirmed).is_err());
    }

    #[test]
    fn removal_is_possible_everywhere_and_final() {
        for state in HandoffState::ALL {
            if state == S::Removed {
                continue;
            }
            assert_eq!(
                transition(state, E::DeviceRemoved),
                Ok(S::Removed),
                "{state:?}"
            );
        }
        for event in HandoffEvent::ALL {
            assert_eq!(
                transition(S::Removed, event),
                Err(IllegalTransition {
                    from: S::Removed,
                    event
                })
            );
        }
    }

    #[test]
    fn host_and_guest_never_own_the_device_together() {
        // No single transition moves ownership directly between host and
        // guest: there is always a state in which nobody owns the device.
        for state in HandoffState::ALL {
            for event in HandoffEvent::ALL {
                let Ok(next) = transition(state, event) else {
                    continue;
                };
                let pair = (state.owner(), next.owner());
                assert_ne!(pair, (Owner::Host, Owner::Guest), "{state:?} + {event:?}");
                assert_ne!(pair, (Owner::Guest, Owner::Host), "{state:?} + {event:?}");
                assert_ne!(
                    pair,
                    (Owner::Uncertain, Owner::Host),
                    "{state:?} + {event:?}"
                );
            }
        }
    }

    #[test]
    fn states_are_named_as_in_the_specification() {
        let names: Vec<&str> = HandoffState::ALL.iter().map(|state| state.name()).collect();
        assert_eq!(
            names,
            [
                "discovered",
                "identified",
                "driver_bound",
                "permission_assigned",
                "available",
                "handoff_requested",
                "host_driver_unbound",
                "guest_attached",
                "guest_owned",
                "guest_detached",
                "host_rebind_attempted",
                "unavailable",
                "unavailable",
                "removed",
            ]
        );
        assert_eq!(S::GuestOwned.to_string(), "guest_owned");
        assert_eq!(
            serde_json::to_value(S::GuestOwned).unwrap(),
            serde_json::json!({ "state": "guest_owned" })
        );
        assert_eq!(
            serde_json::to_value(S::Unavailable(R::GuestDetachFailed)).unwrap(),
            serde_json::json!({ "state": "unavailable", "reason": "guest_detach_failed" })
        );
        let error = transition(S::Available, E::GuestAttached).unwrap_err();
        assert_eq!(
            error.to_string(),
            "event GuestAttached is not allowed in state available"
        );
    }
}

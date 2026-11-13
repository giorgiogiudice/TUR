// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x's indicator and pointer: `org.tur.PaxOverlay1`
//! (`docs/architecture/tur-pax-overlay.md`, section 3).
//!
//! The bridge holds a lease on the overlay for as long as it works, and
//! reads or acts only while the overlay says it is shown (the lease rule of
//! the AI control layer design, section 19, item 3). Every window it reads
//! is framed and every element it acts on is highlighted, with P@x's pointer
//! travelling to it first; the bridge acts only once the pointer has
//! arrived, or a short wait has passed.
//!
//! The lease is also the stop control the bridge obeys by itself: when the
//! lease ends (the indicator was switched off with `Hide`), when the overlay
//! leaves the bus, or when it sends `StopRequested`, the bridge stops. What
//! it is doing is abandoned at its next step and every later request is
//! answered "stopped".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tur_pax::bus::{BUS_NAME, BUS_PATH, BusError, Connection, Kind, Message, Value};

/// The overlay's name on the session bus.
pub const OVERLAY: &str = "org.tur.PaxOverlay1";

/// The overlay's object path.
pub const OVERLAY_PATH: &str = "/org/tur/PaxOverlay1";

/// How long the bridge waits for the overlay to say it is shown.
pub const SHOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the bridge waits for P@x's pointer to arrive at an element.
pub const ARRIVAL_TIMEOUT: Duration = Duration::from_millis(2500);

/// How long the pointer rests on an element before the bridge acts on it,
/// so that the person sees where P@x is about to act.
pub const DWELL: Duration = Duration::from_millis(600);

/// How long a highlight stays after the action, in seconds.
pub const HIGHLIGHT_SECONDS: f64 = 4.0;

const CALL: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct Shared {
    lease: String,
    owner: String,
    arrivals: u64,
}

/// The overlay as the bridge uses it.
#[derive(Debug)]
pub struct Overlay {
    bus: Connection,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    stopped: Arc<AtomicBool>,
}

fn locked(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    match shared.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Overlay {
    /// Watch the session bus for the overlay's signals. `stopped` is set
    /// when the bridge must stop.
    pub fn new(bus: Connection, incoming: Receiver<Message>, stopped: Arc<AtomicBool>) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let _ = bus.add_match(&format!(
            "type='signal',sender='{BUS_NAME}',interface='{BUS_NAME}',member='NameOwnerChanged',arg0='{OVERLAY}'"
        ));
        let watched = Arc::clone(&shared);
        let stop = Arc::clone(&stopped);
        let _ = thread::Builder::new()
            .name("tur-pax-a11y-overlay".to_owned())
            .spawn(move || {
                for message in incoming {
                    watch(&message, &watched, &stop);
                }
                // The session bus is gone: so is the indicator.
                stop.store(true, Ordering::SeqCst);
                watched.1.notify_all();
            });
        Overlay {
            bus,
            shared,
            stopped,
        }
    }

    fn call(&self, member: &str, body: Vec<Value>) -> Result<Message, BusError> {
        self.bus.call(
            Message::method_call(OVERLAY, OVERLAY_PATH, OVERLAY, member, body),
            CALL,
        )
    }

    fn lease(&self) -> String {
        locked(&self.shared.0).lease.clone()
    }

    /// Hold a lease and see the overlay shown, or say why not.
    ///
    /// # Errors
    ///
    /// The overlay is not running, refused a lease, or did not show itself
    /// in time.
    pub fn ensure(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err("P@x was stopped".to_owned());
        }
        if self.lease().is_empty() {
            let owner = self
                .bus
                .call(
                    Message::method_call(
                        BUS_NAME,
                        BUS_PATH,
                        BUS_NAME,
                        "GetNameOwner",
                        vec![Value::string(OVERLAY)],
                    ),
                    CALL,
                )
                .ok()
                .and_then(|reply| reply.first_string().map(str::to_owned))
                .ok_or_else(|| {
                    "P@x's indicator (tur-pax-overlay) is not running, and the bridge works only while it is shown".to_owned()
                })?;
            let reply = self
                .call(
                    "Acquire",
                    vec![Value::string("P@x is reading and operating windows")],
                )
                .map_err(|error| format!("P@x's indicator gave no lease: {error}"))?;
            let lease = reply
                .first_string()
                .ok_or_else(|| "P@x's indicator gave no lease".to_owned())?
                .to_owned();
            let mut shared = locked(&self.shared.0);
            shared.lease = lease;
            shared.owner = owner;
        }
        let lease = self.lease();
        let _ = self.call(
            "SetActivity",
            vec![
                Value::string(&lease),
                Value::string("observing"),
                Value::variant_map(vec![("scope", Value::string("screen"))]),
            ],
        );
        let started = Instant::now();
        loop {
            if self.shown() {
                return Ok(());
            }
            if self.stopped.load(Ordering::SeqCst) {
                return Err("P@x was stopped".to_owned());
            }
            if started.elapsed() >= SHOWN_TIMEOUT {
                return Err("P@x's indicator is not shown on the screen".to_owned());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Whether the overlay says it is shown.
    pub fn shown(&self) -> bool {
        self.bus
            .call(
                Message::method_call(
                    OVERLAY,
                    OVERLAY_PATH,
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    vec![Value::string(OVERLAY), Value::string("Shown")],
                ),
                CALL,
            )
            .ok()
            .and_then(|reply| reply.body.first().and_then(|value| value.unwrapped().as_bool()))
            .unwrap_or(false)
    }

    /// Say what P@x is doing, framing a window of process `pid` with the
    /// title `title`, or the screen.
    pub fn activity(&self, state: &str, window: Option<(u32, &str)>) {
        let detail = match window {
            Some((pid, title)) => Value::variant_map(vec![
                ("scope", Value::string("window")),
                ("pid", Value::U32(pid)),
                ("title", Value::string(title)),
            ]),
            None => Value::variant_map(vec![("scope", Value::string("screen"))]),
        };
        let _ = self.call(
            "SetActivity",
            vec![Value::string(self.lease()), Value::string(state), detail],
        );
    }

    /// The number of times P@x's pointer has arrived so far.
    pub fn arrivals(&self) -> u64 {
        locked(&self.shared.0).arrivals
    }

    /// Highlight a rectangle in a window, relative to the window's content,
    /// and send P@x's pointer to it. A window not found by its title is
    /// looked for by its process alone.
    ///
    /// # Errors
    ///
    /// The overlay could not place the highlight.
    pub fn highlight(&self, pid: u32, title: &str, rect: [i32; 4], caption: &str) -> Result<u32, String> {
        // One highlight at a time: what was pointed at before goes.
        let _ = self.call("Clear", vec![Value::string(self.lease()), Value::U32(0)]);
        let width = f64::from(rect[2].max(1));
        let height = f64::from(rect[3].max(1));
        let place = |title: &str| {
            self.call(
                "HighlightInWindow",
                vec![
                    Value::string(self.lease()),
                    Value::U32(pid),
                    Value::string(""),
                    Value::string(title),
                    Value::Struct(vec![
                        Value::Double(f64::from(rect[0])),
                        Value::Double(f64::from(rect[1])),
                        Value::Double(width),
                        Value::Double(height),
                    ]),
                    Value::variant_map(vec![
                        ("point", Value::Bool(true)),
                        ("caption", Value::string(caption)),
                        ("duration", Value::Double(HIGHLIGHT_SECONDS)),
                    ]),
                ],
            )
        };
        let reply = match place(title) {
            Ok(reply) => reply,
            Err(_) if !title.is_empty() => place("").map_err(|error| error.to_string())?,
            Err(error) => return Err(error.to_string()),
        };
        Ok(reply.body.first().and_then(Value::as_u32).unwrap_or(0))
    }

    /// Wait until P@x's pointer has arrived since `before` arrivals had been
    /// counted, the bridge is stopped, or the wait is over. True if it
    /// arrived.
    pub fn wait_for_arrival(&self, before: u64, timeout: Duration) -> bool {
        let (mutex, arrived) = &*self.shared;
        let deadline = Instant::now() + timeout;
        let mut shared = locked(mutex);
        while shared.arrivals <= before && !self.stopped.load(Ordering::SeqCst) {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            shared = match arrived.wait_timeout(shared, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        shared.arrivals > before
    }

    /// Show a click where the pointer is.
    pub fn pulse(&self) {
        let _ = self.call(
            "PulsePointer",
            vec![Value::string(self.lease()), Value::string("click")],
        );
    }

    /// Say that P@x is idle, keeping the lease.
    pub fn idle(&self) {
        let _ = self.call(
            "SetActivity",
            vec![
                Value::string(self.lease()),
                Value::string("idle"),
                Value::variant_map(Vec::new()),
            ],
        );
    }

    /// Give the lease back: everything drawn for the bridge goes.
    pub fn release(&self) {
        let lease = self.lease();
        if !lease.is_empty() {
            let _ = self.call("Release", vec![Value::string(lease)]);
        }
    }
}

/// What one message from the session bus means for the bridge.
fn watch(message: &Message, shared: &Arc<(Mutex<Shared>, Condvar)>, stop: &AtomicBool) {
    if message.kind != Kind::Signal {
        return;
    }
    let member = message.member.as_deref().unwrap_or("");
    let interface = message.interface.as_deref().unwrap_or("");
    let first = message.first_string().unwrap_or("");
    let (mutex, arrived) = &**shared;
    let mut state = locked(mutex);
    if interface == BUS_NAME && member == "NameOwnerChanged" {
        // The overlay left the bus, or another took its name.
        let old = message.body.get(1).and_then(Value::as_str).unwrap_or("");
        if first == OVERLAY && !state.owner.is_empty() && old == state.owner {
            stop.store(true, Ordering::SeqCst);
            arrived.notify_all();
        }
        return;
    }
    if interface != OVERLAY {
        return;
    }
    // Only the overlay that gave the lease is listened to.
    if state.owner.is_empty() || message.sender.as_deref() != Some(state.owner.as_str()) {
        return;
    }
    match member {
        "PointerArrived" if first == state.lease => {
            state.arrivals += 1;
            arrived.notify_all();
        }
        "LeaseEnded" if first == state.lease => {
            stop.store(true, Ordering::SeqCst);
            arrived.notify_all();
        }
        "StopRequested" => {
            stop.store(true, Ordering::SeqCst);
            arrived.notify_all();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(sender: &str, interface: &str, member: &str, body: Vec<Value>) -> Message {
        let mut message = Message::signal(OVERLAY_PATH, interface, member, body);
        message.sender = Some(sender.to_owned());
        message
    }

    fn shared(lease: &str, owner: &str) -> Arc<(Mutex<Shared>, Condvar)> {
        Arc::new((
            Mutex::new(Shared {
                lease: lease.to_owned(),
                owner: owner.to_owned(),
                arrivals: 0,
            }),
            Condvar::new(),
        ))
    }

    #[test]
    fn only_the_overlay_that_gave_the_lease_can_stop_the_bridge_or_move_it_on() {
        let state = shared("L1", ":1.7");
        let stop = AtomicBool::new(false);
        // Another connection's signals do nothing.
        watch(
            &signal(":1.9", OVERLAY, "LeaseEnded", vec![Value::string("L1"), Value::string("hidden")]),
            &state,
            &stop,
        );
        watch(
            &signal(":1.9", OVERLAY, "PointerArrived", vec![Value::string("L1")]),
            &state,
            &stop,
        );
        assert!(!stop.load(Ordering::SeqCst));
        assert_eq!(locked(&state.0).arrivals, 0);
        // Another lease's arrival does nothing.
        watch(
            &signal(":1.7", OVERLAY, "PointerArrived", vec![Value::string("L2")]),
            &state,
            &stop,
        );
        assert_eq!(locked(&state.0).arrivals, 0);
        watch(
            &signal(":1.7", OVERLAY, "PointerArrived", vec![Value::string("L1")]),
            &state,
            &stop,
        );
        assert_eq!(locked(&state.0).arrivals, 1);
        assert!(!stop.load(Ordering::SeqCst));
        watch(
            &signal(":1.7", OVERLAY, "LeaseEnded", vec![Value::string("L1"), Value::string("hidden")]),
            &state,
            &stop,
        );
        assert!(stop.load(Ordering::SeqCst));
    }

    #[test]
    fn the_overlay_leaving_the_bus_or_asking_to_stop_stops_the_bridge() {
        let state = shared("L1", ":1.7");
        let stop = AtomicBool::new(false);
        watch(
            &signal(
                BUS_NAME,
                BUS_NAME,
                "NameOwnerChanged",
                vec![Value::string(OVERLAY), Value::string(":1.7"), Value::string("")],
            ),
            &state,
            &stop,
        );
        assert!(stop.load(Ordering::SeqCst));

        let stop = AtomicBool::new(false);
        watch(&signal(":1.7", OVERLAY, "StopRequested", Vec::new()), &state, &stop);
        assert!(stop.load(Ordering::SeqCst));

        // Before any lease nothing is listened to.
        let state = shared("", "");
        let stop = AtomicBool::new(false);
        watch(&signal(":1.7", OVERLAY, "StopRequested", Vec::new()), &state, &stop);
        assert!(!stop.load(Ordering::SeqCst));
    }
}

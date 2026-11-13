// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `org.tur.Agent1` on the session bus.
//!
//! This module is the bus side of the session service: it owns the name,
//! checks who is calling, reads the arguments of a call, calls one function
//! of [`crate::service::Core`] and writes its answer back. It turns the
//! service's events into signals. It decides nothing else.
//!
//! The interface is described in `interfaces/org.tur.Agent1.xml`, which is
//! what `Introspect` returns. The tables below are what the service
//! dispatches on; a test holds them and the file together.
//!
//! # Who is served
//!
//! A call is answered only if the bus reports that the connection it came
//! from runs under the same Unix user as the service. A call from another
//! user, or from a connection the bus cannot say anything about, is refused
//! with `TUR-AI-SERVICE-005`, and so is every call when the service cannot
//! read its own user.
//!
//! What that rule is worth, said plainly: on today's desktop every program
//! the person runs is that user (addendum A section 19, item 1). Any of them
//! can call every method here: start a task, switch P@x on, set a key, set
//! a limit, create a schedule. The rule keeps out other accounts and
//! nothing more. What keeps the model out is elsewhere: the commands and
//! tool programs P@x runs have no session bus in their sandbox, no tool
//! calls this interface, and whatever reaches the interface runs a task
//! through the one loop that performs classes 0 and 1 only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::bus::{BusError, Connection, Kind, Message, Value};
use crate::faults::{Fault, TaskCode};
use crate::service::{Core, Event, Events, INTERFACE_VERSION};

/// The well-known name of the service.
pub const NAME: &str = "org.tur.Agent1";

/// The object path of the service.
pub const PATH: &str = "/org/tur/Agent1";

/// The interface of the service.
pub const INTERFACE: &str = "org.tur.Agent1";

/// The introspection data, as committed.
pub const INTROSPECTION: &str = include_str!("../interfaces/org.tur.Agent1.xml");

/// The methods of the interface: name, input signature, output signature.
pub const METHODS: &[(&str, &str, &str)] = &[
    ("StartTask", "ss", "s"),
    ("CancelTask", "s", ""),
    ("Pause", "", ""),
    ("Resume", "", ""),
    ("GetTask", "s", "s"),
    ("ListActivity", "u", "s"),
    ("GetStatus", "", "s"),
    ("GetConsentText", "s", "ss"),
    ("Enable", "s", ""),
    ("Disable", "", ""),
    ("SetProvider", "ss", ""),
    ("SetLimits", "s", ""),
    ("ListModels", "", "s"),
    ("ListProviderModels", "sb", "s"),
    ("GetGenerationSettings", "ss", "s"),
    ("SetGenerationSettings", "sss", ""),
    ("ListLocalModels", "", "s"),
    ("UseLocalModel", "ss", "s"),
    ("CancelLocalModel", "", ""),
    ("SetKey", "ss", ""),
    ("RemoveKey", "s", "b"),
    ("HasKey", "s", "b"),
    ("ListProposals", "", "s"),
    ("ConfirmProposal", "s", "s"),
    ("DismissProposal", "s", ""),
    ("CreateSchedule", "s", "s"),
    ("RemoveSchedule", "s", ""),
    ("ListSchedules", "", "s"),
    ("GetAway", "", "s"),
    ("MarkAwaySeen", "as", ""),
    ("Wake", "", ""),
    ("ListMemory", "", "s"),
    ("SetMemoryEnabled", "b", ""),
    ("ConfirmMemory", "s", ""),
    ("ForgetMemory", "s", ""),
];

/// The signals of the interface: name and signature.
pub const SIGNALS: &[(&str, &str)] = &[
    ("TaskStarted", "ss"),
    ("TextAdded", "sss"),
    ("ActionProposed", "ss"),
    ("ActionCompleted", "ss"),
    ("TaskFinished", "ss"),
    ("ScheduleProposed", "ss"),
    ("SchedulesChanged", ""),
    ("MemoryChanged", ""),
    ("StateChanged", "s"),
    ("LocalModelProgress", "ss"),
];

/// The properties of the interface: name and type. All read-only.
pub const PROPERTIES: &[(&str, &str)] =
    &[("State", "s"), ("Enabled", "b"), ("InterfaceVersion", "u")];

/// The most calls that are worked on at one time. Calls beyond it are
/// answered at once with `TUR-AI-SERVICE-004`.
pub const MAX_CALLS_IN_FLIGHT: usize = 16;

const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";
const PEER: &str = "org.freedesktop.DBus.Peer";
const INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
const UNKNOWN_OBJECT: &str = "org.freedesktop.DBus.Error.UnknownObject";
const UNKNOWN_PROPERTY: &str = "org.freedesktop.DBus.Error.UnknownProperty";
const PROPERTY_READ_ONLY: &str = "org.freedesktop.DBus.Error.PropertyReadOnly";

/// The screen locker of the desktop, which says when the session is locked.
const SCREENSAVER_NAME: &str = "org.freedesktop.ScreenSaver";
const SCREENSAVER_PATH: &str = "/org/freedesktop/ScreenSaver";

/// The error name for a fault: `org.tur.Agent1.Error.` and the code with
/// its hyphens as underscores.
pub fn error_name(fault: &Fault) -> String {
    format!(
        "{INTERFACE}.Error.{}",
        fault.code.as_str().replace('-', "_")
    )
}

/// Whether a caller is served: only if this process knows its own user
/// and the bus reported the same user for the caller.
pub fn admits(own_user: Option<u32>, caller_user: Option<u32>) -> bool {
    matches!((own_user, caller_user), (Some(own), Some(caller)) if own == caller)
}

/// The service's events as signals.
pub struct BusEvents {
    bus: Connection,
    core: Mutex<Option<std::sync::Weak<Core>>>,
}

impl BusEvents {
    /// Signals sent over `bus`.
    pub fn new(bus: Connection) -> Self {
        BusEvents {
            bus,
            core: Mutex::new(None),
        }
    }

    /// The service whose state the property signals report.
    pub fn attach(&self, core: &Arc<Core>) {
        *self
            .core
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::downgrade(core));
    }

    fn signal(&self, member: &str, body: Vec<Value>) {
        let _ = self
            .bus
            .send(&Message::signal(PATH, INTERFACE, member, body));
    }
}

impl Events for BusEvents {
    fn emit(&self, event: &Event) {
        let text = |value: &serde_json::Value| Value::string(value.to_string());
        match event {
            Event::TaskStarted { task, info } => {
                self.signal("TaskStarted", vec![Value::string(task), text(info)]);
            }
            Event::TextAdded { task, kind, text } => self.signal(
                "TextAdded",
                vec![
                    Value::string(task),
                    Value::string(*kind),
                    Value::string(text),
                ],
            ),
            Event::ActionProposed { task, step } => {
                self.signal("ActionProposed", vec![Value::string(task), text(step)]);
            }
            Event::ActionCompleted { task, step } => {
                self.signal("ActionCompleted", vec![Value::string(task), text(step)]);
            }
            Event::TaskFinished { task, result } => {
                self.signal("TaskFinished", vec![Value::string(task), text(result)]);
            }
            Event::ScheduleProposed { proposal, details } => self.signal(
                "ScheduleProposed",
                vec![Value::string(proposal), text(details)],
            ),
            Event::SchedulesChanged => self.signal("SchedulesChanged", Vec::new()),
            Event::LocalModelProgress { model, event } => {
                self.signal(
                    "LocalModelProgress",
                    vec![Value::string(model), text(event)],
                );
            }
            Event::MemoryChanged => self.signal("MemoryChanged", Vec::new()),
            Event::StateChanged => {
                let core = self
                    .core
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade);
                let Some(core) = core else {
                    return;
                };
                let state = core.state_name();
                self.signal("StateChanged", vec![Value::string(state)]);
                let _ = self.bus.send(&Message::signal(
                    PATH,
                    PROPERTIES_INTERFACE,
                    "PropertiesChanged",
                    vec![
                        Value::string(INTERFACE),
                        Value::variant_map(vec![
                            ("State", Value::string(state)),
                            ("Enabled", Value::Bool(core.is_enabled())),
                        ]),
                        Value::strings(Vec::<String>::new()),
                    ],
                ));
            }
        }
    }
}

type Reply = Result<Vec<Value>, (String, String)>;

fn refused(fault: &Fault) -> (String, String) {
    (error_name(fault), fault.to_string())
}

fn property(core: &Core, name: &str) -> Option<Value> {
    match name {
        "State" => Some(Value::string(core.state_name())),
        "Enabled" => Some(Value::Bool(core.is_enabled())),
        "InterfaceVersion" => Some(Value::U32(INTERFACE_VERSION)),
        _ => None,
    }
}

fn machine_id() -> String {
    std::fs::read_to_string("/etc/machine-id")
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// Answer one call. The caller has been admitted.
pub fn dispatch(core: &Arc<Core>, call: &Message) -> Reply {
    let interface = call.interface.as_deref().unwrap_or(INTERFACE);
    let member = call.member.as_deref().unwrap_or_default();
    let signature = call.signature();
    let unknown = || {
        Err((
            UNKNOWN_METHOD.to_owned(),
            format!("{interface} has no method {member} taking ({signature})"),
        ))
    };
    if call.path.as_deref() != Some(PATH) {
        return Err((
            UNKNOWN_OBJECT.to_owned(),
            "there is no such object".to_owned(),
        ));
    }
    match interface {
        INTROSPECTABLE if member == "Introspect" && signature.is_empty() => {
            return Ok(vec![Value::string(INTROSPECTION)]);
        }
        PEER if member == "Ping" && signature.is_empty() => return Ok(Vec::new()),
        PEER if member == "GetMachineId" && signature.is_empty() => {
            return Ok(vec![Value::string(machine_id())]);
        }
        PROPERTIES_INTERFACE => {
            let text = |index: usize| call.body.get(index).and_then(Value::as_str).unwrap_or("");
            return match (member, signature.as_str()) {
                ("Get", "ss") if text(0) == INTERFACE => property(core, text(1))
                    .map(|value| vec![Value::variant(value)])
                    .ok_or_else(|| (UNKNOWN_PROPERTY.to_owned(), "no such property".to_owned())),
                ("GetAll", "s") if text(0) == INTERFACE => {
                    let entries = PROPERTIES
                        .iter()
                        .filter_map(|(name, _)| property(core, name).map(|value| (*name, value)))
                        .collect();
                    Ok(vec![Value::variant_map(entries)])
                }
                ("Get" | "GetAll", _) => Err((
                    UNKNOWN_PROPERTY.to_owned(),
                    "no such interface or property".to_owned(),
                )),
                ("Set", "ssv") => Err((
                    PROPERTY_READ_ONLY.to_owned(),
                    "the properties of this service are read-only".to_owned(),
                )),
                _ => unknown(),
            };
        }
        INTERFACE => {}
        _ => return unknown(),
    }

    let Some((_, input, _)) = METHODS.iter().find(|(name, _, _)| *name == member) else {
        return unknown();
    };
    if signature != *input {
        return Err((
            INVALID_ARGS.to_owned(),
            format!("{member} takes ({input}), not ({signature})"),
        ));
    }
    let text = |index: usize| call.body.get(index).and_then(Value::as_str).unwrap_or("");
    let json = |value: serde_json::Value| vec![Value::string(value.to_string())];
    let done = |result: Result<(), Fault>| result.map(|()| Vec::new()).map_err(|f| refused(&f));
    match member {
        "StartTask" => core
            .start_task(text(0), text(1))
            .map(|task| vec![Value::string(task)])
            .map_err(|f| refused(&f)),
        "CancelTask" => done(core.cancel_task(text(0))),
        "Pause" => done(core.pause()),
        "Resume" => done(core.resume()),
        "GetTask" => core.task(text(0)).map(json).map_err(|f| refused(&f)),
        "ListActivity" => {
            let limit = call
                .body
                .first()
                .and_then(Value::as_u32)
                .unwrap_or(20)
                .clamp(1, 500);
            core.activity(limit).map(json).map_err(|f| refused(&f))
        }
        "GetStatus" => Ok(json(core.status())),
        "GetConsentText" => core
            .consent(text(0))
            .map(|(version, text)| vec![Value::string(version), Value::string(text)])
            .map_err(|f| refused(&f)),
        "Enable" => done(core.enable(text(0))),
        "Disable" => done(core.disable()),
        "SetProvider" => done(core.set_provider(text(0), text(1))),
        "SetLimits" => done(core.set_limits(text(0))),
        "ListModels" => Ok(json(core.models())),
        "ListProviderModels" => core
            .provider_models(
                text(0),
                call.body.get(1).and_then(Value::as_bool).unwrap_or(false),
            )
            .map(json)
            .map_err(|f| refused(&f)),
        "GetGenerationSettings" => core
            .generation_settings(text(0), text(1))
            .map(json)
            .map_err(|f| refused(&f)),
        "SetGenerationSettings" => done(core.set_generation_settings(text(0), text(1), text(2))),
        "ListLocalModels" => core.local_models().map(json).map_err(|f| refused(&f)),
        "UseLocalModel" => core
            .use_local_model(text(0), text(1))
            .map(|job| vec![Value::string(job)])
            .map_err(|f| refused(&f)),
        "CancelLocalModel" => done(core.cancel_local_model()),
        "SetKey" => done(core.set_key(text(0), text(1))),
        "RemoveKey" => core
            .remove_key(text(0))
            .map(|removed| vec![Value::Bool(removed)])
            .map_err(|f| refused(&f)),
        "HasKey" => core
            .has_key(text(0))
            .map(|set| vec![Value::Bool(set)])
            .map_err(|f| refused(&f)),
        "ListProposals" => Ok(json(core.proposals())),
        "ConfirmProposal" => core
            .confirm_proposal(text(0))
            .map(|schedule| vec![Value::string(schedule)])
            .map_err(|f| refused(&f)),
        "DismissProposal" => done(core.dismiss_proposal(text(0))),
        "CreateSchedule" => core
            .create_schedule(text(0))
            .map(|schedule| vec![Value::string(schedule)])
            .map_err(|f| refused(&f)),
        "RemoveSchedule" => done(core.remove_schedule(text(0))),
        "ListSchedules" => core.schedules().map(json).map_err(|f| refused(&f)),
        "GetAway" => core.away().map(json).map_err(|f| refused(&f)),
        "MarkAwaySeen" => {
            let runs: Vec<String> = call
                .body
                .first()
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .filter_map(|run| run.as_str().map(str::to_owned))
                .collect();
            done(core.mark_away_seen(&runs))
        }
        "Wake" => {
            core.poke();
            Ok(Vec::new())
        }
        "ListMemory" => core.memory_list().map(json).map_err(|f| refused(&f)),
        "SetMemoryEnabled" => done(
            core.set_memory_enabled(call.body.first().and_then(Value::as_bool).unwrap_or(false)),
        ),
        "ConfirmMemory" => done(core.confirm_memory(text(0))),
        "ForgetMemory" => done(core.forget_memory(text(0))),
        _ => unknown(),
    }
}

/// What the service knows of its callers: the user the bus reported for
/// each unique name. A unique name is never given to another connection
/// for the life of the bus, so the answer can be kept.
struct Callers {
    bus: Connection,
    own: Option<u32>,
    known: Mutex<HashMap<String, Option<u32>>>,
}

impl Callers {
    fn admits(&self, sender: Option<&str>) -> bool {
        let Some(sender) = sender else {
            return false;
        };
        let cached = self
            .known
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(sender)
            .copied();
        let user = match cached {
            Some(user) => user,
            None => {
                let user = self.bus.unix_user_of(sender).ok();
                self.known
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(sender.to_owned(), user);
                user
            }
        };
        admits(self.own, user)
    }
}

/// Ask the screen locker whether the session is locked. `None` if there is
/// no screen locker on the bus.
fn screen_locked(bus: &Connection) -> Option<bool> {
    bus.call(
        Message::method_call(
            SCREENSAVER_NAME,
            SCREENSAVER_PATH,
            SCREENSAVER_NAME,
            "GetActive",
            Vec::new(),
        ),
        std::time::Duration::from_secs(5),
    )
    .ok()
    .and_then(|reply| reply.body.first().and_then(Value::as_bool))
}

/// Serve `org.tur.Agent1` on `bus` until the connection ends.
///
/// # Errors
///
/// The name is owned by another connection, or the bus refused it.
pub fn serve(
    core: &Arc<Core>,
    bus: &Connection,
    incoming: &Receiver<Message>,
) -> Result<(), BusError> {
    // What the screen locker says arrives as a signal from now on.
    let _ = bus.add_match(&format!(
        "type='signal',interface='{SCREENSAVER_NAME}',member='ActiveChanged'"
    ));
    core.set_session_locked(screen_locked(bus));
    if !bus.request_name(NAME)? {
        return Err(BusError::Unavailable(format!(
            "another program already owns {NAME}"
        )));
    }
    let callers = Arc::new(Callers {
        bus: bus.clone(),
        own: crate::bus::own_user(),
        known: Mutex::new(HashMap::new()),
    });
    let in_flight = Arc::new(AtomicUsize::new(0));
    // What came due while the service was not running is looked at now.
    core.poke();

    while let Ok(message) = incoming.recv() {
        match message.kind {
            Kind::Signal => {
                let locker = message.interface.as_deref() == Some(SCREENSAVER_NAME)
                    && message.member.as_deref() == Some("ActiveChanged");
                if locker {
                    core.set_session_locked(message.body.first().and_then(Value::as_bool));
                }
            }
            Kind::MethodCall => {
                if in_flight.load(Ordering::SeqCst) >= MAX_CALLS_IN_FLIGHT {
                    let busy = Fault::new(TaskCode::ServiceBusy, "too many calls at once");
                    if message.wants_reply() {
                        let _ =
                            bus.send(&message.error_reply(&error_name(&busy), &busy.to_string()));
                    }
                    continue;
                }
                in_flight.fetch_add(1, Ordering::SeqCst);
                let core = Arc::clone(core);
                let bus = bus.clone();
                let callers = Arc::clone(&callers);
                let counter = Arc::clone(&in_flight);
                let spawned = thread::Builder::new()
                    .name("tur-pax-call".to_owned())
                    .spawn(move || {
                        let answer = if callers.admits(message.sender.as_deref()) {
                            dispatch(&core, &message)
                        } else {
                            let refusal = Fault::new(
                                TaskCode::ServiceCallerRefused,
                                TaskCode::ServiceCallerRefused.user_message(),
                            );
                            Err(refused(&refusal))
                        };
                        if message.wants_reply() {
                            let reply = match answer {
                                Ok(body) => message.reply(body),
                                Err((name, text)) => message.error_reply(&name, &text),
                            };
                            let _ = bus.send(&reply);
                        }
                        counter.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                }
            }
            Kind::MethodReturn | Kind::Error => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_caller_of_the_same_user_is_served() {
        assert!(admits(Some(1000), Some(1000)));
        assert!(!admits(Some(1000), Some(1001)));
        assert!(!admits(Some(1000), Some(0)));
        assert!(!admits(Some(0), Some(1000)));
        assert!(!admits(Some(1000), None));
        assert!(!admits(None, Some(1000)));
        assert!(!admits(None, None));
    }

    #[test]
    fn an_error_name_is_the_code_of_the_fault() {
        let fault = Fault::new(TaskCode::ServiceDisabled, "off");
        assert_eq!(
            error_name(&fault),
            "org.tur.Agent1.Error.TUR_AI_SERVICE_001"
        );
        assert!(crate::bus::is_interface_name(&error_name(&fault)));
        for entry in crate::faults::TASK_REGISTRY {
            let name = error_name(&Fault::new(entry.code, "x"));
            assert!(crate::bus::is_interface_name(&name), "{name}");
        }
    }

    #[test]
    fn the_tables_are_valid_d_bus() {
        for (name, input, output) in METHODS {
            assert!(crate::bus::is_member_name(name), "{name}");
            assert!(crate::bus::split_signature(input).is_ok(), "{name}");
            assert!(crate::bus::split_signature(output).is_ok(), "{name}");
        }
        for (name, signature) in SIGNALS {
            assert!(crate::bus::is_member_name(name), "{name}");
            assert!(crate::bus::split_signature(signature).is_ok(), "{name}");
        }
        let mut names: Vec<&str> = METHODS.iter().map(|(name, _, _)| *name).collect();
        names.extend(SIGNALS.iter().map(|(name, _)| *name));
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a name is used twice");
    }
}

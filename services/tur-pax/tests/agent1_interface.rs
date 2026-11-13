// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `org.tur.Agent1` as committed, as dispatched, and what reaches it.
//!
//! * The committed introspection file and the tables the service dispatches
//!   on say the same thing: every method with its arguments, every signal,
//!   every property. The `pax` workflow also compares what the running
//!   service returns from `Introspect` with the file.
//! * Every way into the session service ends in one task loop that is built
//!   with nothing that could lift it above class 1. This is checked in the
//!   sources, because it is a property of what code exists.

use std::path::Path;

use tur_pax::dbus::{INTERFACE, INTROSPECTION, METHODS, PROPERTIES, SIGNALS};

/// One member of an interface in the introspection data.
#[derive(Debug, PartialEq, Eq)]
struct Member {
    kind: String,
    name: String,
    inputs: String,
    outputs: String,
}

/// The value of an attribute of one tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_owned())
}

/// The members of one interface of the introspection data, in order.
fn members(xml: &str, interface: &str) -> Vec<Member> {
    let opening = format!("<interface name=\"{interface}\">");
    let start = xml.find(&opening).expect("the interface is described") + opening.len();
    let end = xml[start..]
        .find("</interface>")
        .expect("the interface ends")
        + start;
    let body = &xml[start..end];
    let mut found: Vec<Member> = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('<') {
        let close = rest[open..].find('>').expect("a tag ends") + open;
        let tag = &rest[open..=close];
        rest = &rest[close + 1..];
        let kind = tag
            .trim_start_matches('<')
            .split([' ', '>', '/'])
            .next()
            .unwrap_or_default();
        match kind {
            "method" | "signal" | "property" => found.push(Member {
                kind: kind.to_owned(),
                name: attribute(tag, "name").expect("a member has a name"),
                inputs: String::new(),
                outputs: attribute(tag, "type").unwrap_or_default(),
            }),
            "arg" => {
                let member = found.last_mut().expect("an argument belongs to a member");
                let kind = attribute(tag, "type").expect("an argument has a type");
                match attribute(tag, "direction").as_deref() {
                    Some("out") => member.outputs.push_str(&kind),
                    // A signal's arguments have no direction.
                    Some("in") | None if member.kind == "method" => member.inputs.push_str(&kind),
                    _ => member.outputs.push_str(&kind),
                }
            }
            _ => {}
        }
    }
    found
}

#[test]
fn the_served_interface_is_the_committed_file() {
    let file = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("interfaces")
            .join("org.tur.Agent1.xml"),
    )
    .expect("the committed file");
    // What Introspect returns is the file, byte for byte.
    assert_eq!(INTROSPECTION, file);
    assert!(file.contains("SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0"));

    let described = members(&file, INTERFACE);
    let methods: Vec<(String, String, String)> = described
        .iter()
        .filter(|member| member.kind == "method")
        .map(|member| {
            (
                member.name.clone(),
                member.inputs.clone(),
                member.outputs.clone(),
            )
        })
        .collect();
    let dispatched: Vec<(String, String, String)> = METHODS
        .iter()
        .map(|(name, inputs, outputs)| {
            (
                (*name).to_owned(),
                (*inputs).to_owned(),
                (*outputs).to_owned(),
            )
        })
        .collect();
    assert_eq!(methods, dispatched);

    let signals: Vec<(String, String)> = described
        .iter()
        .filter(|member| member.kind == "signal")
        .map(|member| (member.name.clone(), member.outputs.clone()))
        .collect();
    let emitted: Vec<(String, String)> = SIGNALS
        .iter()
        .map(|(name, signature)| ((*name).to_owned(), (*signature).to_owned()))
        .collect();
    assert_eq!(signals, emitted);

    let properties: Vec<(String, String)> = described
        .iter()
        .filter(|member| member.kind == "property")
        .map(|member| (member.name.clone(), member.outputs.clone()))
        .collect();
    let served: Vec<(String, String)> = PROPERTIES
        .iter()
        .map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned()))
        .collect();
    assert_eq!(properties, served);
    // Every property is read-only.
    assert_eq!(
        file.matches("access=\"read\"").count(),
        PROPERTIES.len(),
        "a property is writable"
    );
    assert!(!file.contains("access=\"readwrite\""));
    assert!(!file.contains("access=\"write\""));

    // The standard interfaces the service answers are described too.
    for (interface, method) in [
        ("org.freedesktop.DBus.Introspectable", "Introspect"),
        ("org.freedesktop.DBus.Properties", "GetAll"),
        ("org.freedesktop.DBus.Peer", "Ping"),
    ] {
        assert!(
            members(&file, interface)
                .iter()
                .any(|member| member.name == method),
            "{interface}.{method}"
        );
    }
}

#[test]
fn no_method_gives_back_a_key_and_none_asks_for_an_approval() {
    // The key goes in, in SetKey; no method has an output that could carry
    // it: what comes back about a key is a boolean.
    for (name, _, outputs) in METHODS {
        if name.contains("Key") {
            assert!(
                outputs.is_empty() || *outputs == "b",
                "{name} gives back ({outputs})"
            );
        }
    }
    // Nothing on the interface approves, confirms an action, opens a repair
    // session or raises authority: the design's ConfirmationRequested and
    // EndRepairSession are left out until a trusted session exists.
    let names: Vec<&str> = METHODS.iter().map(|(name, _, _)| *name).collect();
    for forbidden in [
        "Approve",
        "Confirm",
        "Authenticate",
        "Repair",
        "Trusted",
        "Standing",
        "Policy",
        "Allow",
    ] {
        for name in &names {
            let allowed_confirm = *name == "ConfirmProposal" || *name == "ConfirmMemory";
            assert!(
                allowed_confirm || !name.contains(forbidden),
                "{name} contains {forbidden}"
            );
        }
    }
    for (name, _) in SIGNALS {
        assert!(!name.contains("Confirmation"), "{name}");
    }
}

fn source(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name))
        .expect("a source file")
}

/// The shipped part of a source file: what comes before its unit tests.
fn shipped(name: &str) -> String {
    let text = source(name);
    text.split("#[cfg(test)]")
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn every_way_into_the_service_ends_in_one_task_loop_that_grants_nothing() {
    let modules = [
        "service.rs",
        "dbus.rs",
        "daemon.rs",
        "scheduler.rs",
        "settings.rs",
        "spending.rs",
        "keyring.rs",
        "notify.rs",
        "wake.rs",
        "consent.rs",
        "bus.rs",
        "local.rs",
        "tuning.rs",
    ];
    for name in modules {
        let text = shipped(name);
        // Nothing sets the trusted path, a repair session or a standing
        // approval, and nothing reads approvals from a policy store.
        for forbidden in [
            "trusted_session_proven",
            "RepairSession",
            // A standing approval built or granted; the reason
            // `NoStandingApproval` of a deferred action is no such thing.
            "StandingApproval {",
            "StandingApproval::",
            "HeldApprovals",
            "PolicyStore",
            "standing_approvals:",
            "Context {",
            "decide(",
        ] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden:?}");
        }
        // Only the service builds a task loop, and it builds exactly one.
        let loops = text.matches("Orchestrator {").count();
        if name == "service.rs" {
            assert_eq!(loops, 1, "{name} builds {loops} task loops");
        } else {
            assert_eq!(loops, 0, "{name} builds a task loop");
        }
    }

    // The service says who is there in two ways only: a person asked, or a
    // schedule came due with nobody there.
    let service = shipped("service.rs");
    let situations: Vec<&str> = service
        .match_indices("Situation")
        .map(|(at, _)| &service[at..(at + 80).min(service.len())])
        .filter(|text| text.starts_with("Situation::") || text.starts_with("Situation {"))
        .collect();
    assert!(!situations.is_empty());
    for situation in situations {
        assert!(
            situation.starts_with("Situation::attended()")
                || situation.starts_with(
                    "Situation {\n            presence: crate::context::Presence::Unattended"
                ),
            "{situation}"
        );
    }
    // The task loop the service builds is told the situation of the job,
    // the user's policy as read, and nothing else about authority.
    let built = service
        .split("Orchestrator {")
        .nth(1)
        .and_then(|rest| rest.split("};").next())
        .expect("the loop");
    for field in [
        "provider:",
        "config:",
        "registry:",
        "policy:",
        "situation: job.situation",
        "limits: TaskLimits::default()",
        "rules:",
        "grants:",
        "executor:",
        "trail:",
        "clock:",
        "control:",
        "observer:",
        "budget:",
        "assistant:",
    ] {
        assert!(built.contains(field), "the loop has no {field}");
    }

    // The schedule runner runs tasks only through the service.
    let scheduler = shipped("scheduler.rs");
    assert!(!scheduler.contains("Orchestrator"));
    assert!(!scheduler.contains("Executor"));

    // The bus layer dispatches to the service and to nothing that runs.
    let dbus = shipped("dbus.rs");
    for forbidden in [
        "Orchestrator",
        "Executor",
        "SandboxPlan",
        "Command::new",
        "keyring::",
    ] {
        assert!(!dbus.contains(forbidden), "dbus.rs contains {forbidden:?}");
    }
    // The key goes from the bus to the password store and nowhere else.
    assert_eq!(dbus.matches("set_key(").count(), 1);
}

#[test]
fn the_units_start_one_fixed_command() {
    let unit = |name: &str| {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("units")
                .join(name),
        )
        .expect("a unit file")
    };
    let service = unit("tur-pax.service");
    assert!(service.contains("Type=dbus"));
    assert!(service.contains("BusName=org.tur.Agent1"));
    assert!(service.contains("ExecStart=/usr/bin/tur-pax serve\n"));
    let activation = unit("org.tur.Agent1.service");
    assert!(activation.contains("Name=org.tur.Agent1"));
    assert!(activation.contains("SystemdService=tur-pax.service"));
    let catchup = unit("tur-pax-catchup.service");
    assert!(catchup.contains("ExecStart=/usr/bin/tur-pax schedule wake\n"));
    let timer = unit("tur-pax-catchup.timer");
    for line in [
        "OnStartupSec=",
        "OnClockChange=true",
        "OnTimezoneChange=true",
    ] {
        assert!(timer.contains(line), "{line}");
    }
    // No timer of P@x runs on a period: nothing wakes it to look for work.
    for text in [&service, &catchup, &timer] {
        assert!(!text.contains("OnUnitActiveSec"));
        assert!(!text.contains("OnCalendar"));
    }
    for text in [&service, &activation, &catchup, &timer] {
        assert!(text.contains("SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0"));
    }
}

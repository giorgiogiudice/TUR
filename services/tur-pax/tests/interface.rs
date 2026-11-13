// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The interface tools, through the real task loop: the registry, the policy
//! engine, the audit trail and the output filter are the shipped ones; the
//! accessibility bridge is a stand-in that answers from a table, or, for the
//! stop control, a real child process that never answers.
//!
//! What these tests hold (pax-accessibility.md, section 8):
//!
//! * the class of an interface action comes from what the bridge read on the
//!   bus, never from the model's arguments;
//! * nothing above class 1 is performed, for every interface tool, in every
//!   situation and policy;
//! * excluded applications are neither read nor listed; windows of
//!   administrator processes neither;
//! * a call whose target cannot be resolved never becomes an action;
//! * every decision is in the trail before anything is done;
//! * the stop control abandons a call the bridge is working on, at once,
//!   and kills the bridge.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use tur_pax::interface::{
    Answer, BridgeInterface, Failure, INTERFACE_MANIFEST, Interface, InterfaceCall, InterfaceTool,
    Target,
};
use tur_pax::orchestrator::{Control, ReportItem, Signal, Situation, TaskOutcome};
use tur_pax::trail::EventKind;
use tur_pax::{Presence, ToolManifest, UserPolicy};

use common::{FakeServer, Harness, call, calls, frame, says, tool_results};

/// A stand-in bridge: targets by name, and a log of what was performed.
struct Stand {
    targets: HashMap<String, Target>,
    performed: Rc<RefCell<Vec<String>>>,
    answer: Option<Answer<Value>>,
}

impl Interface for Stand {
    fn resolve(&mut self, call: &InterfaceCall, _control: &dyn Control) -> Answer<Target> {
        let key = call
            .arguments
            .get("name")
            .or_else(|| call.arguments.get("window"))
            .and_then(Value::as_str)
            .unwrap_or("desktop")
            .to_owned();
        match self.targets.get(&key) {
            Some(target) => Answer::Done(target.clone()),
            None => Answer::NotDone(Failure::NotFound, format!("nothing is called {key}")),
        }
    }

    fn perform(
        &mut self,
        call: &InterfaceCall,
        target: &Target,
        _control: &dyn Control,
    ) -> Answer<Value> {
        self.performed
            .borrow_mut()
            .push(format!("{} {}", call.tool.name(), target.name));
        match &self.answer {
            Some(answer) => answer.clone(),
            None => Answer::Done(json!({ "done": true, "element": target.element })),
        }
    }
}

fn element(application: &str, name: &str, role: u32, editable: bool) -> Target {
    Target {
        window: "w1".to_owned(),
        element: "e7".to_owned(),
        application: application.to_owned(),
        also_known_as: Vec::new(),
        window_title: "Notes".to_owned(),
        owner: "user".to_owned(),
        name: name.to_owned(),
        description: String::new(),
        role,
        role_name: String::new(),
        default_control: false,
        editable,
    }
}

/// The targets the stand-in knows: a window, ordinary and consequential
/// controls, fields, an excluded application and an administrator window.
fn targets() -> HashMap<String, Target> {
    let mut targets = HashMap::new();
    let mut add = |key: &str, target: Target| {
        targets.insert(key.to_owned(), target);
    };
    add("desktop", element("org.a11y.atspi.Registry", "", 0, false));
    add("Notes", element("org.example.Editor", "", 0, false));
    add("Bold", element("org.example.Editor", "Bold", 43, false));
    add("Send", element("org.example.Editor", "Send", 43, false));
    add("Delete", element("org.example.Editor", "Delete", 43, false));
    add("Body", element("org.example.Editor", "", 61, true));
    add("Password", element("org.example.Editor", "Password", 40, true));
    add("Folders", element("org.example.Editor", "Folders", 91, false));
    add("General", element("org.example.Editor", "General", 37, false));
    let mut vault = element("python3", "", 0, false);
    vault.also_known_as = vec!["org.keepassxc.KeePassXC".to_owned()];
    vault.window_title = "Passwords".to_owned();
    add("Passwords", vault);
    let mut disks = element("org.example.Disks", "Bold", 43, false);
    disks.owner = "administrator".to_owned();
    add("Disks", disks);
    let mut ownerless = element("org.example.Editor", "Italic", 43, false);
    ownerless.owner = "unknown".to_owned();
    add("Italic", ownerless);
    targets
}

fn harness() -> (Harness, Rc<RefCell<Vec<String>>>) {
    let mut harness = Harness::new();
    harness.registry = harness.registry.clone().with_interface_tools(true);
    let performed = Rc::new(RefCell::new(Vec::new()));
    harness.interface = Box::new(Stand {
        targets: targets(),
        performed: Rc::clone(&performed),
        answer: None,
    });
    (harness, performed)
}

fn proposals(items: &[ReportItem]) -> Vec<&tur_pax::ProposalReport> {
    items
        .iter()
        .filter_map(|item| match item {
            ReportItem::Proposal(proposal) => Some(proposal),
            _ => None,
        })
        .collect()
}

#[test]
fn the_interface_tools_are_offered_only_when_asked_for() {
    let harness = Harness::new();
    let names: Vec<String> = harness
        .registry
        .offered()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(!names.iter().any(|name| name.starts_with("ui_")));
    let (with, _) = harness_with_names();
    for tool in InterfaceTool::ALL {
        assert!(with.contains(&tool.name().to_owned()), "{}", tool.name());
    }
    // A component cannot take one of their names.
    let manifest = r#"{"schema_version":1,"service":"org.example.Clash1","tools":[{"name":"ui_press","description":"A clash.","class":0,"method":"Press","input_schema":{"type":"object","additionalProperties":false}}]}"#;
    assert!(tur_pax::ToolRegistry::load(&[manifest.to_owned()], &[], true).is_err());
}

fn harness_with_names() -> (Vec<String>, Rc<RefCell<Vec<String>>>) {
    let (harness, performed) = harness();
    (
        harness
            .registry
            .offered()
            .into_iter()
            .map(|tool| tool.name)
            .collect(),
        performed,
    )
}

#[test]
fn an_ordinary_press_is_decided_from_the_bus_recorded_and_performed() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![call(
                "toolu_1",
                "ui_press",
                r#"{"window": "Notes", "name": "Bold"}"#,
            )],
        ),
        says("The text is bold now."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Make it bold.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_ACTION"), ["accessibility:act"]);
    assert_eq!(harness.decisions("TUR_AI_TIER"), ["2"]);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["1"]);
    assert_eq!(harness.decisions("TUR_AI_DECISION"), ["allowed"]);
    assert_eq!(*performed.borrow(), ["ui_press Bold"]);
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_ACTION"),
        ["accessibility:act"]
    );
    // The decision is written before the action, the outcome after it.
    let order: Vec<String> = harness
        .trail
        .entries()
        .iter()
        .map(|entry| entry.field("TUR_AI_EVENT").unwrap_or_default())
        .collect();
    assert_eq!(
        order,
        ["task_started", "decision", "executed", "task_finished"]
    );
    // The record names the operation and the role, never the control.
    let text = harness.trail_text();
    assert!(text.contains("operation press, role button"), "{text}");
    assert!(!text.contains("Bold"), "{text}");
    // The result goes back as untrusted data.
    let results = tool_results(&server.bodies()[1]);
    assert_eq!(frame(&results[0])["status"], "ok");
    assert_eq!(frame(&results[0])["output"]["element"], "e7");
}

#[test]
fn send_and_delete_are_refused_and_nothing_happens() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call(
                    "toolu_1",
                    "ui_press",
                    r#"{"window": "Notes", "name": "Send"}"#,
                ),
                call(
                    "toolu_2",
                    "ui_press",
                    r#"{"window": "Notes", "name": "Delete"}"#,
                ),
                // The model says the button is a harmless label. The role is
                // the bus's, not the model's.
                call(
                    "toolu_3",
                    "ui_press",
                    r#"{"window": "Notes", "name": "Send", "role": "label"}"#,
                ),
            ],
        ),
        says("I cannot do that."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Send it, then delete the draft.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["2", "2", "2"]);
    assert_eq!(
        harness.decisions("TUR_AI_DECISION"),
        ["refused", "refused", "refused"]
    );
    assert_eq!(
        harness.decisions("TUR_ERROR_CODE"),
        ["TUR-AI-POLICY-006", "TUR-AI-POLICY-006", "TUR-AI-POLICY-006"]
    );
    assert!(harness.trail_text().contains("pattern send"));
    assert!(harness.trail_text().contains("pattern delete"));
    assert!(performed.borrow().is_empty());
    assert!(harness.trail.events(EventKind::Executed).is_empty());
    for block in tool_results(&server.bodies()[1]) {
        let frame = frame(&block);
        assert_eq!(frame["status"], "refused");
        assert_eq!(frame["reason"], "trusted_path_unavailable");
    }
    for proposal in proposals(&report.items) {
        assert!(!proposal.executed);
        assert!(proposal.message.is_some());
    }
}

#[test]
fn typing_is_class_1_into_a_field_and_refused_with_a_line_break_or_into_a_secret() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call(
                    "toolu_1",
                    "ui_type",
                    r#"{"window": "Notes", "name": "Body", "text": "hello"}"#,
                ),
                call(
                    "toolu_2",
                    "ui_type",
                    r#"{"window": "Notes", "name": "Body", "text": "hello\n"}"#,
                ),
                call(
                    "toolu_3",
                    "ui_type",
                    r#"{"window": "Notes", "name": "Password", "text": "hunter2"}"#,
                ),
                call(
                    "toolu_4",
                    "ui_type",
                    r#"{"window": "Notes", "name": "Bold", "text": "x"}"#,
                ),
            ],
        ),
        says("Typed."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Type hello.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["1", "2", "2", "2"]);
    assert_eq!(
        harness.decisions("TUR_AI_DECISION"),
        ["allowed", "refused", "refused", "refused"]
    );
    assert_eq!(*performed.borrow(), ["ui_type "]);
    // The typed text is never in the trail; its length is.
    let text = harness.trail_text();
    assert!(!text.contains("hunter2"));
    assert!(text.contains("5 characters"), "{text}");
}

#[test]
fn moving_the_keyboard_expanding_and_selecting_are_class_1() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "ui_focus", r#"{"window": "Notes", "name": "Body"}"#),
                call(
                    "toolu_2",
                    "ui_expand",
                    r#"{"window": "Notes", "name": "Folders"}"#,
                ),
                call(
                    "toolu_3",
                    "ui_collapse",
                    r#"{"window": "Notes", "name": "Folders"}"#,
                ),
                call(
                    "toolu_4",
                    "ui_select",
                    r#"{"window": "Notes", "name": "General"}"#,
                ),
                call(
                    "toolu_5",
                    "ui_scroll",
                    r#"{"window": "Notes", "name": "Bold"}"#,
                ),
            ],
        ),
        says("Done."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Go to the General tab.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["1", "1", "1", "1", "1"]);
    assert_eq!(performed.borrow().len(), 5);
}

#[test]
fn excluded_and_administrator_windows_are_not_read_and_unknown_owners_not_acted_in() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "ui_read_window", r#"{"window": "Passwords"}"#),
                call(
                    "toolu_2",
                    "ui_press",
                    r#"{"window": "Disks", "name": "Disks"}"#,
                ),
                call(
                    "toolu_3",
                    "ui_press",
                    r#"{"window": "Notes", "name": "Italic"}"#,
                ),
                call("toolu_4", "ui_read_window", r#"{"window": "Notes"}"#),
            ],
        ),
        says("Done."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Look around.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(
        harness.decisions("TUR_AI_DECISION"),
        ["refused", "refused", "refused", "allowed"]
    );
    assert_eq!(
        harness.decisions("TUR_ERROR_CODE"),
        [
            "TUR-AI-POLICY-003",
            "TUR-AI-POLICY-010",
            "TUR-AI-POLICY-006",
            ""
        ]
    );
    assert_eq!(*performed.borrow(), ["ui_read_window "]);
}

#[test]
fn the_list_of_windows_leaves_out_what_the_policy_excludes() {
    let server = FakeServer::start(vec![
        calls("", vec![call("toolu_1", "ui_list_windows", "{}")]),
        says("Two windows."),
    ]);
    let (mut harness, _) = harness();
    harness.interface = Box::new(Stand {
        targets: targets(),
        performed: Rc::new(RefCell::new(Vec::new())),
        answer: Some(Answer::Done(json!({ "windows": [
            { "window": "w1", "application": "org.example.Editor", "owner": "user", "title": "Notes" },
            { "window": "w2", "application": "python3", "also_known_as": ["org.keepassxc.KeePassXC"], "owner": "user", "title": "Bank PINs" },
            { "window": "w3", "application": "org.example.Disks", "owner": "administrator", "title": "Disks" }
        ]}))),
    });
    let report = harness.run(&server, "What is open?");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    let results = tool_results(&server.bodies()[1]);
    let output = &frame(&results[0])["output"];
    assert_eq!(output["windows"].as_array().map(Vec::len), Some(1));
    assert_eq!(output["windows_left_out"], 2);
    assert!(!output.to_string().contains("Bank PINs"));
}

#[test]
fn a_call_whose_target_is_not_found_never_becomes_an_action() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![call(
                "toolu_1",
                "ui_press",
                r#"{"window": "Notes", "name": "Nowhere"}"#,
            )],
        ),
        says("It is not there."),
    ]);
    let (mut harness, performed) = harness();
    let report = harness.run(&server, "Press Nowhere.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert!(harness.decisions("TUR_AI_ACTION").is_empty());
    assert_eq!(
        harness.events(EventKind::ProposalRejected, "TUR_ERROR_CODE"),
        ["TUR-AI-INTERFACE-002"]
    );
    assert!(performed.borrow().is_empty());
}

#[test]
fn an_unattended_task_does_not_use_the_accessibility_tree() {
    let server = FakeServer::start(vec![
        calls("", vec![call("toolu_1", "ui_read_window", r#"{"window": "Notes"}"#)]),
        says("Skipped."),
    ]);
    let (mut harness, performed) = harness();
    harness.situation = Situation::unattended();
    harness.run(&server, "Read my notes.");
    assert!(harness.decisions("TUR_AI_ACTION").is_empty());
    assert_eq!(
        harness.events(EventKind::ProposalRejected, "TUR_AI_REASON"),
        ["unattended_interface"]
    );
    assert!(performed.borrow().is_empty());
}

#[test]
fn a_stop_while_the_bridge_works_ends_the_task() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "ui_press", r#"{"window": "Notes", "name": "Bold"}"#),
                call("toolu_2", "ui_press", r#"{"window": "Notes", "name": "Bold"}"#),
            ],
        ),
        says("Never sent."),
    ]);
    let (mut harness, performed) = harness();
    harness.interface = Box::new(Stand {
        targets: targets(),
        performed: Rc::clone(&performed),
        answer: Some(Answer::NotDone(
            Failure::Stopped,
            "P@x's indicator was switched off".to_owned(),
        )),
    });
    let report = harness.run(&server, "Make it bold twice.");
    assert_eq!(report.outcome, TaskOutcome::Cancelled);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-INTERFACE-004"));
    // The first press was tried and the second never decided.
    assert_eq!(performed.borrow().len(), 1);
    assert_eq!(harness.decisions("TUR_AI_ACTION").len(), 1);
    assert_eq!(server.bodies().len(), 1);
}

/// Every interface tool, with arguments the schema allows, aimed at every
/// kind of target.
fn every_interface_call() -> Vec<(String, String)> {
    let manifest = ToolManifest::from_json(INTERFACE_MANIFEST).expect("a manifest");
    let mut calls = Vec::new();
    for tool in manifest.tools() {
        for name in [
            "Notes",
            "Bold",
            "Send",
            "Delete",
            "Body",
            "Password",
            "Passwords",
            "Disks",
            "Italic",
        ] {
            let mut arguments = Map::new();
            arguments.insert("window".to_owned(), json!("Notes"));
            if !InterfaceTool::from_name(tool.name())
                .expect("a tool")
                .observes()
            {
                arguments.insert("name".to_owned(), json!(name));
            } else if tool.name() != "ui_list_windows" && tool.name() != "ui_read_focused" {
                arguments.insert("window".to_owned(), json!(name));
            } else {
                arguments.clear();
            }
            if tool.name() == "ui_type" {
                arguments.insert("text".to_owned(), json!("hello\nworld"));
            }
            calls.push((tool.name().to_owned(), Value::Object(arguments).to_string()));
        }
    }
    calls
}

#[test]
fn nothing_above_class_one_is_performed_through_the_interface_in_any_situation() {
    let policies = [
        r#"{"schema_version":1,"mode":"normal"}"#,
        r#"{"schema_version":1,"mode":"ask_every_action"}"#,
        r#"{"schema_version":1,"mode":"observe_only"}"#,
        // A relaxation is the one standing loosening there is; without a
        // trusted session it relaxes nothing.
        r#"{"schema_version":1,"mode":"normal","relaxations":[{"application":"org.example.Editor","class":2}]}"#,
    ];
    let mut runs = 0;
    for policy in policies {
        for presence in Presence::ALL {
            for session_locked in [false, true] {
                let context = format!("{policy} {presence:?} locked={session_locked}");
                let (mut harness, performed) = harness();
                harness.policy = UserPolicy::from_json(policy).expect("a valid policy");
                harness.situation = Situation {
                    presence,
                    session_locked,
                };
                harness.limits.max_calls_per_response = 200;
                let proposed: Vec<common::Call> = every_interface_call()
                    .iter()
                    .enumerate()
                    .map(|(index, (name, arguments))| call(&format!("toolu_{index}"), name, arguments))
                    .collect();
                let count = proposed.len();
                let server = FakeServer::start(vec![calls("", proposed), says("Finished.")]);
                let report = harness.run(&server, "Do everything you can.");
                assert_eq!(report.outcome, TaskOutcome::Completed, "{context}");
                runs += 1;

                let classes = harness.decisions("TUR_AI_CLASS");
                let decisions = harness.decisions("TUR_AI_DECISION");
                for (class, decision) in classes.iter().zip(&decisions) {
                    if class.as_str() >= "2" {
                        assert_ne!(decision, "allowed", "{context}: class {class}");
                    }
                }
                // Everything performed was an ordinary target, and nothing
                // was performed with nobody there or the session locked.
                for done in performed.borrow().iter() {
                    let tool = done.split(' ').next().and_then(InterfaceTool::from_name);
                    if tool.is_some_and(|tool| !tool.observes()) {
                        assert!(
                            !done.contains("Send") && !done.contains("Delete"),
                            "{context}: {done}"
                        );
                        assert!(!done.starts_with("ui_type"), "{context}: {done}");
                    }
                }
                if presence == Presence::Unattended || session_locked {
                    assert!(performed.borrow().is_empty(), "{context}");
                }
                for proposal in proposals(&report.items) {
                    if proposal.class.is_some_and(|class| class >= 2) {
                        assert!(!proposal.executed, "{context}: {}", proposal.tool);
                    }
                }
                assert!(classes.len() <= count);
            }
        }
    }
    assert_eq!(runs, 16);
}

/// A control that says "go on" a number of times and then "cancel".
struct CancelAfter(RefCell<u32>);

impl Control for CancelAfter {
    fn signal(&self) -> Signal {
        let mut left = self.0.borrow_mut();
        if *left == 0 {
            Signal::Cancel
        } else {
            *left -= 1;
            Signal::Proceed
        }
    }
}

#[test]
fn the_stop_control_abandons_a_call_in_flight_and_kills_the_bridge() {
    // A bridge that never answers.
    let directory = std::env::temp_dir().join(format!("tur-pax-stop-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a directory");
    let pid_file = directory.join("pid");
    let program = directory.join("bridge");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 60\n",
            pid_file.display()
        ),
    )
    .expect("the stand-in");
    let mut permissions = std::fs::metadata(&program)
        .expect("metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&program, permissions).expect("executable");

    let mut bridge = BridgeInterface::new(&program);
    let call = InterfaceCall {
        tool: InterfaceTool::ListWindows,
        arguments: Map::new(),
    };
    // About half a second of polling, then the stop.
    let control = CancelAfter(RefCell::new(50));
    let started = Instant::now();
    let answer = bridge.resolve(&call, &control);
    let took = started.elapsed();
    assert!(
        matches!(answer, Answer::NotDone(Failure::Cancelled, _)),
        "{answer:?}"
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert!(!bridge.is_running());
    // The process is gone: killed and reaped.
    let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
    let pid = pid.trim();
    if !pid.is_empty() {
        assert!(!Path::new(&format!("/proc/{pid}")).exists(), "{pid} lives");
    }
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_bridge_that_does_not_answer_in_time_is_closed() {
    let mut bridge =
        BridgeInterface::new(Path::new("/bin/cat")).with_timeout(Duration::from_millis(300));
    let call = InterfaceCall {
        tool: InterfaceTool::ListWindows,
        arguments: Map::new(),
    };
    // cat echoes the request: it answers, but not with an answer.
    let answer = bridge.resolve(&call, &tur_pax::orchestrator::NoControl);
    assert!(
        matches!(answer, Answer::NotDone(Failure::Unavailable, _)),
        "{answer:?}"
    );
    assert!(!bridge.is_running());
}

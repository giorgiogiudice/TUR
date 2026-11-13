// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-pax-a11y serve`: the bridge's side of the protocol with the task
//! loop (`tur_pax::interface`). One request per line on standard input, one
//! answer per line on standard output, in order.
//!
//! **resolve** reads the call's target from the bus and describes it.
//! **perform** carries the call out, and only after:
//!
//! 1. the target is read again and compared with what the engine decided
//!    on: a target that changed is not acted on (`changed`);
//! 2. the bridge decides once more by itself, with the policy engine, the
//!    built-in lists and the strictest context (no trusted session): what
//!    the engine would not allow at class 0 or 1 is not done (`refused`),
//!    whatever the task loop sent;
//! 3. P@x's indicator is shown and holds a lease (`unavailable` if not);
//! 4. P@x's pointer has travelled to the element and rested on it.
//!
//! When the stop control is used (the lease ends, the overlay leaves or
//! asks to stop), the request under way ends at its next step and every
//! later one is answered `stopped`.

use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use tur_pax::bus::Connection;
use tur_pax::interface::{
    BridgeError, BridgeReply, BridgeRequest, DESKTOP_APPLICATION, InterfaceCall, InterfaceTool,
    Target,
};
use tur_pax::{Action, ActionClass, Context, Timestamp, UiOperation, UserPolicy, decide};

use crate::act;
use crate::atspi::{Client, Node, role_name, state};
use crate::overlay::{ARRIVAL_TIMEOUT, DWELL, Overlay};
use crate::tree::{Budget, DEFAULT_NODES, Desktop, Facts, Miss, Window};

/// An answer that is not a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// One of the protocol's codes.
    pub code: &'static str,
    /// A sentence.
    pub message: String,
}

impl Refusal {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Refusal {
            code,
            message: message.into(),
        }
    }
}

impl From<Miss> for Refusal {
    fn from(miss: Miss) -> Self {
        Refusal::new(miss.code, miss.message)
    }
}

/// The bridge, once connected.
#[derive(Debug)]
pub struct Bridge {
    desktop: Desktop,
    overlay: Overlay,
    stopped: Arc<AtomicBool>,
}

fn argument<'a>(arguments: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// Write a line for the log of the bridge, on standard error. It names no
/// element, no window title and no text: only the operation, identifiers
/// and positions.
pub fn log(line: &str) {
    let _ = writeln!(io::stderr(), "TUR-A11Y {line}");
}

/// The facts the engine decided on, as the protocol carries them.
fn target_of(window: &Window, element: Option<(&str, &Facts)>) -> Target {
    let (id, facts) = match element {
        Some((id, facts)) => (id.to_owned(), facts.clone()),
        None => (String::new(), Facts::default()),
    };
    Target {
        window: window.id.clone(),
        element: id,
        application: window.application.application.clone(),
        also_known_as: window.application.also_known_as.clone(),
        window_title: window.title.clone(),
        owner: window.application.owner.to_owned(),
        name: facts.name.clone(),
        description: facts.description.clone(),
        role: facts.role,
        role_name: if element.is_some() {
            role_name(facts.role).to_owned()
        } else {
            String::new()
        },
        default_control: facts.states.has(state::IS_DEFAULT),
        editable: facts.states.has(state::EDITABLE),
    }
}

/// Whether two descriptions of a target are of the same thing in the same
/// state, as far as the decision is concerned.
pub fn same_target(decided: &Target, now: &Target) -> bool {
    decided == now
}

/// The bridge's own decision: what the engine would decide with the
/// built-in lists, nobody's relaxation, and no trusted session. Only an
/// allowed action of class 0 or 1 passes.
pub fn second_decision(call: &InterfaceCall, target: &Target) -> Result<(), Refusal> {
    let action = Action::Accessibility(target.action(call));
    let context = Context {
        target_window_owner: target.owner(),
        ..Context::attended()
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let decision = decide(&action, &context, &UserPolicy::default(), Timestamp(now));
    if decision.is_allow() && decision.class() <= ActionClass::Reversible {
        Ok(())
    } else {
        Err(Refusal::new(
            "refused",
            format!(
                "the bridge's own decision: class {}, {}",
                decision.class().as_u8(),
                decision.reason().as_str()
            ),
        ))
    }
}

impl Bridge {
    /// Connect to the session bus, P@x's indicator and the accessibility
    /// bus.
    ///
    /// # Errors
    ///
    /// A bus cannot be reached.
    pub fn connect() -> Result<Self, String> {
        let address = crate::atspi::session_address()
            .ok_or_else(|| "no session bus is known to this process".to_owned())?;
        let (session, incoming) = Connection::open(&address)
            .map_err(|error| format!("the session bus cannot be reached: {error}"))?;
        let stopped = Arc::new(AtomicBool::new(false));
        let client = Client::connect(Some(&session))
            .map_err(|error| format!("the accessibility bus cannot be reached: {error}"))?;
        let overlay = Overlay::new(session, incoming, Arc::clone(&stopped));
        Ok(Bridge {
            desktop: Desktop::new(client, Arc::clone(&stopped)),
            overlay,
            stopped,
        })
    }

    fn check_stopped(&self) -> Result<(), Refusal> {
        if self.stopped.load(Ordering::SeqCst) {
            Err(Refusal::new(
                "stopped",
                "P@x was stopped: its indicator was switched off or its stop control used",
            ))
        } else {
            Ok(())
        }
    }

    /// Answer one request.
    pub fn handle(&mut self, request: &BridgeRequest) -> Result<Value, Refusal> {
        self.check_stopped()?;
        let tool = InterfaceTool::from_name(&request.tool)
            .ok_or_else(|| Refusal::new("failed", "no such interface tool"))?;
        let call = InterfaceCall {
            tool,
            arguments: request.arguments.clone(),
        };
        match request.op.as_str() {
            "resolve" => {
                let target = self.resolve(&call)?;
                serde_json::to_value(target)
                    .map_err(|error| Refusal::new("failed", error.to_string()))
            }
            "perform" => {
                let target = request
                    .target
                    .as_ref()
                    .ok_or_else(|| Refusal::new("failed", "perform names no target"))?;
                self.perform(&call, target)
            }
            _ => Err(Refusal::new("failed", "no such request")),
        }
    }

    fn window(&mut self, call: &InterfaceCall) -> Result<Window, Refusal> {
        let query = argument(&call.arguments, "window")
            .ok_or_else(|| Refusal::new("not_found", "the call names no window"))?;
        Ok(self.desktop.find_window(query)?)
    }

    fn desktop_target(&self) -> Target {
        Target {
            application: DESKTOP_APPLICATION.to_owned(),
            owner: self.desktop.registry_owner().to_owned(),
            ..Target::default()
        }
    }

    /// What a call would act on.
    ///
    /// # Errors
    ///
    /// No window or element matches, or several do.
    pub fn resolve(&mut self, call: &InterfaceCall) -> Result<Target, Refusal> {
        match call.tool {
            InterfaceTool::ListWindows => Ok(self.desktop_target()),
            InterfaceTool::ReadWindow | InterfaceTool::Find => {
                let window = self.window(call)?;
                Ok(target_of(&window, None))
            }
            InterfaceTool::ReadFocused => {
                let (window, node, facts) = self
                    .desktop
                    .focused()
                    .ok_or_else(|| Refusal::new("not_found", "no element has the keyboard"))?;
                let id = self.desktop.element_id(&node, &window.id);
                let mut target = target_of(&window, Some((&id, &facts)));
                // Reading looks at the window; the element is named only so
                // that the same one is read.
                target.role_name = String::new();
                Ok(target)
            }
            _ => {
                let window = self.window(call)?;
                let (node, facts) = self.desktop.resolve_element(
                    &window,
                    argument(&call.arguments, "element"),
                    argument(&call.arguments, "name"),
                    argument(&call.arguments, "role"),
                    call.tool == InterfaceTool::Type,
                )?;
                let id = self.desktop.element_id(&node, &window.id);
                Ok(target_of(&window, Some((&id, &facts))))
            }
        }
    }

    /// The window and the element a target names, read again now.
    fn reread(&mut self, target: &Target) -> Result<(Window, Option<(Node, Facts)>), Refusal> {
        let window = self.desktop.find_window(&target.window)?;
        if window.id != target.window {
            return Err(Refusal::new(
                "changed",
                "the window is not the one decided on",
            ));
        }
        if target.element.is_empty() {
            return Ok((window, None));
        }
        let (node, in_window) = self
            .desktop
            .element(&target.element)
            .ok_or_else(|| Refusal::new("changed", "the element is not known any more"))?;
        if in_window != window.id {
            return Err(Refusal::new("changed", "the element is in another window"));
        }
        let facts = self.desktop.facts(&node).map_err(|error| {
            Refusal::new("changed", format!("the element cannot be read: {error}"))
        })?;
        if facts.states.has(state::DEFUNCT) {
            return Err(Refusal::new("changed", "the element no longer exists"));
        }
        Ok((window, Some((node, facts))))
    }

    /// Show P@x's pointer travelling to a rectangle of a window and resting
    /// there.
    fn point(&self, window: &Window, rect: [i32; 4], caption: &str, dwell: bool) {
        let Some(pid) = window.application.pid else {
            log(&format!(
                "arrived=false highlight=no-process window={}",
                window.id
            ));
            return;
        };
        let before = self.overlay.arrivals();
        let (arrived, placed) = match self.overlay.highlight(pid, &window.title, rect, caption) {
            Ok(_) => (
                self.overlay.wait_for_arrival(before, ARRIVAL_TIMEOUT),
                "ok".to_owned(),
            ),
            // The error's name only: its text may quote the window's title.
            Err(error) => (
                false,
                format!("failed:{}", error.split(':').next().unwrap_or_default()),
            ),
        };
        log(&format!(
            "arrived={arrived} highlight={placed} window={} rect={},{},{},{} pid={pid}",
            window.id, rect[0], rect[1], rect[2], rect[3]
        ));
        if dwell {
            thread::sleep(DWELL);
        }
    }

    fn window_rect(&self, window: &Window) -> [i32; 4] {
        match self.desktop.client().extents(&window.node) {
            Ok(rect) if rect[2] > 0 && rect[3] > 0 => [0, 0, rect[2], rect[3]],
            _ => [0, 0, 1, 1],
        }
    }

    /// Carry out a call on the target the engine decided on.
    ///
    /// # Errors
    ///
    /// The target changed, the bridge's own decision refuses, the indicator
    /// is not shown, the application did not do it, or P@x was stopped.
    pub fn perform(&mut self, call: &InterfaceCall, decided: &Target) -> Result<Value, Refusal> {
        // 2. The bridge's own decision, on what the task loop sent; and
        //    again below, on what is read now.
        second_decision(call, decided)?;
        // 3. The indicator.
        self.overlay.ensure().map_err(|message| {
            if self.stopped.load(Ordering::SeqCst) {
                Refusal::new("stopped", message)
            } else {
                Refusal::new("unavailable", message)
            }
        })?;
        let result = self.carry_out(call, decided);
        self.overlay.idle();
        // A stop that came while the call was under way wins.
        self.check_stopped()?;
        result
    }

    fn carry_out(&mut self, call: &InterfaceCall, decided: &Target) -> Result<Value, Refusal> {
        if call.tool == InterfaceTool::ListWindows {
            if *decided != self.desktop_target() {
                return Err(Refusal::new(
                    "changed",
                    "the registry is not the one decided on",
                ));
            }
            self.overlay.activity("observing", None);
            return self.list();
        }
        // 1. The same target, now.
        let (window, element) = self.reread(decided)?;
        let now = match &element {
            Some((_, facts)) => {
                let mut now = target_of(&window, Some((&decided.element, facts)));
                if call.tool == InterfaceTool::ReadFocused {
                    now.role_name = String::new();
                }
                now
            }
            None => target_of(&window, None),
        };
        if !same_target(decided, &now) {
            return Err(Refusal::new(
                "changed",
                "the window or the element changed since the decision",
            ));
        }
        second_decision(call, &now)?;
        let pid = window.application.pid.unwrap_or(0);
        let stopped = self.desktop.stop_flag();
        match call.tool {
            InterfaceTool::ReadWindow => {
                self.overlay
                    .activity("observing", Some((pid, &window.title)));
                self.point(
                    &window,
                    self.window_rect(&window),
                    "P@x is reading this window",
                    true,
                );
                let nodes = call
                    .arguments
                    .get("max_nodes")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(DEFAULT_NODES);
                let mut budget = Budget::new(nodes, stopped);
                let read = self.desktop.read(&window, &mut budget);
                if budget.stopped() {
                    return Err(Refusal::new("stopped", "P@x was stopped while reading"));
                }
                Ok(read)
            }
            InterfaceTool::Find => {
                self.overlay
                    .activity("observing", Some((pid, &window.title)));
                self.point(
                    &window,
                    self.window_rect(&window),
                    "P@x is reading this window",
                    true,
                );
                let mut budget = Budget::new(DEFAULT_NODES, stopped);
                let found = self.desktop.find(
                    &window,
                    argument(&call.arguments, "name"),
                    argument(&call.arguments, "role"),
                    &mut budget,
                );
                if budget.stopped() {
                    return Err(Refusal::new("stopped", "P@x was stopped while reading"));
                }
                Ok(found)
            }
            InterfaceTool::ReadFocused => {
                let Some((node, facts)) = element else {
                    return Err(Refusal::new("not_found", "no element has the keyboard"));
                };
                self.overlay
                    .activity("observing", Some((pid, &window.title)));
                if let Ok(rect) = self.desktop.client().extents(&node) {
                    self.point(&window, rect, "P@x is reading this", true);
                }
                let described = self.desktop.describe(&node, &window.id, &facts);
                Ok(json!({
                    "note": crate::tree::CONTENT_NOTE,
                    "window": window.id,
                    "title": crate::tree::cut(&window.title, crate::tree::MAX_LABEL),
                    "application": window.application.application,
                    "focused": described,
                }))
            }
            _ => {
                let Some((node, facts)) = element else {
                    return Err(Refusal::new("changed", "the element is not known any more"));
                };
                let operation = call.tool.operation();
                self.overlay.activity("acting", Some((pid, &window.title)));
                let rect = self.desktop.client().extents(&node).unwrap_or([0, 0, 1, 1]);
                // 4. The pointer travels to the element and rests there.
                self.point(&window, rect, &format!("P@x: {}", operation.as_str()), true);
                self.check_stopped()?;
                log(&format!(
                    "act op={} element={} window={}",
                    operation.as_str(),
                    decided.element,
                    window.id
                ));
                let done = act::perform(
                    self.desktop.client(),
                    &node,
                    &facts,
                    operation,
                    call.text(),
                    &stopped,
                )
                .map_err(|not_done| Refusal::new(not_done.code, not_done.message))?;
                self.overlay.pulse();
                let after = self.desktop.facts(&node).unwrap_or(facts);
                let mut answer = Map::new();
                answer.insert("done".to_owned(), json!(true));
                answer.insert("operation".to_owned(), json!(operation.as_str()));
                answer.insert("element".to_owned(), json!(decided.element));
                answer.insert("what".to_owned(), json!(done));
                answer.insert("states_after".to_owned(), json!(after.states.reported()));
                if operation == UiOperation::Type {
                    // Whether the text arrived, read back from the field.
                    let now = self.desktop.client().text(&node, 4000).unwrap_or_default();
                    // Read back as the person would see it, after the
                    // application has taken it.
                    answer.insert(
                        "text_now_contains_typed_text".to_owned(),
                        json!(now.contains(call.text())),
                    );
                }
                log(&format!(
                    "done op={} element={} window={}",
                    operation.as_str(),
                    decided.element,
                    window.id
                ));
                Ok(Value::Object(answer))
            }
        }
    }

    fn list(&mut self) -> Result<Value, Refusal> {
        let windows = self.desktop.windows().map_err(|error| {
            Refusal::new(
                "unavailable",
                format!("the registry did not answer: {error}"),
            )
        })?;
        let policy = UserPolicy::default();
        let mut listed = Vec::new();
        let mut left_out = 0;
        for window in windows {
            let application = &window.application;
            let mut identities = vec![application.application.as_str()];
            identities.extend(application.also_known_as.iter().map(String::as_str));
            // The built-in lists apply here already; the user's lists are
            // applied by the task loop.
            if application.owner == "administrator"
                || identities
                    .iter()
                    .any(|identity| policy.is_excluded(identity))
            {
                left_out += 1;
                continue;
            }
            listed.push(json!({
                "window": window.id,
                "title": crate::tree::cut(&window.title, crate::tree::MAX_LABEL),
                "application": application.application,
                "also_known_as": application.also_known_as,
                "toolkit": application.toolkit,
                "owner": application.owner,
                "active": window.states.has(state::ACTIVE),
            }));
        }
        Ok(json!({ "windows": listed, "windows_left_out": left_out }))
    }

    /// Give the indicator back.
    pub fn close(&self) {
        self.overlay.release();
    }
}

/// Serve requests from `input` until it ends.
///
/// # Errors
///
/// An answer cannot be written.
pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut bridge: Option<Bridge> = None;
    let mut connect_error: Option<String> = None;
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<BridgeRequest>(&line) {
            Err(error) => BridgeReply {
                id: 0,
                ok: false,
                result: Value::Null,
                error: Some(BridgeError {
                    code: "failed".to_owned(),
                    message: format!("not a request: {error}"),
                }),
            },
            Ok(request) => {
                if bridge.is_none() && connect_error.is_none() {
                    match Bridge::connect() {
                        Ok(connected) => bridge = Some(connected),
                        Err(error) => connect_error = Some(error),
                    }
                }
                let answer = match (&mut bridge, &connect_error) {
                    (Some(bridge), _) => bridge.handle(&request),
                    (None, Some(error)) => Err(Refusal::new("unavailable", error.clone())),
                    (None, None) => Err(Refusal::new("unavailable", "not connected")),
                };
                match answer {
                    Ok(result) => BridgeReply {
                        id: request.id,
                        ok: true,
                        result,
                        error: None,
                    },
                    Err(refusal) => BridgeReply {
                        id: request.id,
                        ok: false,
                        result: Value::Null,
                        error: Some(BridgeError {
                            code: refusal.code.to_owned(),
                            message: refusal.message,
                        }),
                    },
                }
            }
        };
        let text = serde_json::to_string(&reply).map_err(io::Error::other)?;
        writeln!(output, "{text}")?;
        output.flush()?;
    }
    if let Some(bridge) = &bridge {
        bridge.close();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn target(name: &str, role: u32) -> Target {
        Target {
            window: "w1".to_owned(),
            element: "e2".to_owned(),
            application: "org.example.Mail".to_owned(),
            owner: "user".to_owned(),
            window_title: "Compose".to_owned(),
            name: name.to_owned(),
            role,
            ..Target::default()
        }
    }

    #[test]
    fn the_bridge_refuses_by_itself_what_the_engine_would_not_allow() {
        let press = InterfaceCall {
            tool: InterfaceTool::Press,
            arguments: object(json!({ "window": "w1", "element": "e2" })),
        };
        assert!(second_decision(&press, &target("Bold", 43)).is_ok());
        for name in [
            "Send", "Delete", "Buy now", "Pay", "Install", "Submit", "Confirm", "Sign", "Transfer",
            "Remove", "OK", "",
        ] {
            let refused = second_decision(&press, &target(name, 43)).expect_err(name);
            assert_eq!(refused.code, "refused", "{name}");
        }
        // A window nobody vouches for, or an administrator's.
        for owner in ["unknown", "administrator"] {
            let found = Target {
                owner: owner.to_owned(),
                ..target("Bold", 43)
            };
            assert!(second_decision(&press, &found).is_err(), "{owner}");
        }
        // A password manager, under any of its names.
        let vault = Target {
            application: "python3".to_owned(),
            also_known_as: vec!["org.keepassxc.KeePassXC".to_owned()],
            ..target("Bold", 43)
        };
        let read = InterfaceCall {
            tool: InterfaceTool::ReadWindow,
            arguments: object(json!({ "window": "w1" })),
        };
        assert!(second_decision(&read, &vault).is_err());
        // Typing a line break.
        let typed = InterfaceCall {
            tool: InterfaceTool::Type,
            arguments: object(json!({ "window": "w1", "text": "hi\n" })),
        };
        let field = Target {
            editable: true,
            ..target("", 61)
        };
        assert!(second_decision(&typed, &field).is_err());
        let typed = InterfaceCall {
            tool: InterfaceTool::Type,
            arguments: object(json!({ "window": "w1", "text": "hi" })),
        };
        assert!(second_decision(&typed, &field).is_ok());
    }

    #[test]
    fn a_target_that_changed_in_any_way_is_not_the_same() {
        let decided = target("Bold", 43);
        assert!(same_target(&decided, &decided.clone()));
        for changed in [
            Target {
                name: "Send".to_owned(),
                ..decided.clone()
            },
            Target {
                role: 62,
                ..decided.clone()
            },
            Target {
                description: "Sends".to_owned(),
                ..decided.clone()
            },
            Target {
                default_control: true,
                ..decided.clone()
            },
            Target {
                window_title: "Pay".to_owned(),
                ..decided.clone()
            },
            Target {
                owner: "unknown".to_owned(),
                ..decided.clone()
            },
            Target {
                also_known_as: vec!["x".to_owned()],
                ..decided.clone()
            },
        ] {
            assert!(!same_target(&decided, &changed), "{changed:?}");
        }
    }

    #[test]
    fn a_request_that_is_not_one_is_answered_and_the_bridge_goes_on() {
        let input = b"not json\n\n".as_slice();
        let mut output = Vec::new();
        serve(input, &mut output).expect("served");
        let text = String::from_utf8(output).expect("text");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1);
        let reply: BridgeReply = serde_json::from_str(lines[0]).expect("a reply");
        assert!(!reply.ok);
        assert_eq!(
            reply.error.map(|error| error.code),
            Some("failed".to_owned())
        );
    }
}

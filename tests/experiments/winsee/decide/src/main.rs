// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `winsee-decide`: the accessibility bridge's second decision, as a
//! filter, for the winsee experiment (`docs/reports/exp-winsee-1.md`).
//!
//! One request a line on standard input:
//! `{"tool": "ui_press", "arguments": {...}, "target": {...}}`, where
//! `target` is a `tur_pax::interface::Target`. One answer a line on
//! standard output: `{"allowed": true}` or
//! `{"allowed": false, "code": "refused", "message": "..."}`.
//!
//! The decision is `tur_pax_a11y::serve::second_decision`, unchanged: the
//! policy engine with the built-in lists, nobody's relaxation and no
//! trusted session. The experiment's adapter asks it before it lets the
//! bridge inside the Wine prefix act, as `tur-pax-a11y` asks it of itself.

use std::io::{self, BufRead, Write};

use serde_json::{Map, Value, json};
use tur_pax::interface::{InterfaceCall, InterfaceTool, Target};
use tur_pax_a11y::serve::second_decision;

fn decide(line: &str) -> Value {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return json!({"allowed": false, "code": "failed", "message": format!("not a request: {error}")});
        }
    };
    let Some(tool) = request
        .get("tool")
        .and_then(Value::as_str)
        .and_then(InterfaceTool::from_name)
    else {
        return json!({"allowed": false, "code": "failed", "message": "no such interface tool"});
    };
    let arguments: Map<String, Value> = request
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let target = match request.get("target") {
        Some(value) => match serde_json::from_value::<Target>(value.clone()) {
            Ok(target) => target,
            Err(error) => {
                return json!({"allowed": false, "code": "failed", "message": format!("not a target: {error}")});
            }
        },
        None => {
            return json!({"allowed": false, "code": "failed", "message": "no target"});
        }
    };
    let call = InterfaceCall { tool, arguments };
    match second_decision(&call, &target) {
        Ok(()) => json!({"allowed": true}),
        Err(refusal) => {
            json!({"allowed": false, "code": refusal.code, "message": refusal.message})
        }
    }
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = decide(&line);
        if writeln!(stdout, "{answer}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

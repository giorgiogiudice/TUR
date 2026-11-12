// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Progress and results as events, for a program that shows them.
//!
//! With `--events`, an operation writes one JSON object per line on
//! standard output (schema `schemas/app-install-event.schema.json`), and
//! nothing else: first the plan, then steps, activity and progress, and last
//! exactly one `result` or `error`. Without it, the same events are written
//! as lines of text for a person.
//!
//! A `progress` event carries a total only when the total is known (a
//! pinned download); otherwise only what has been done so far. The
//! application centre shows a bar with numbers only when there is a total
//! (desktop-experience.md, section 8.5).

use std::io::Write;

use serde::Serialize;

use crate::codes::Failure;

/// The version of the event format.
pub const EVENT_VERSION: u32 = 1;

/// One event.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// What will be done, before anything is.
    Plan {
        /// The plan document.
        plan: serde_json::Value,
    },
    /// A step starts.
    StepStarted {
        /// The step's identifier, one of the plan's steps.
        step: String,
        /// What it does.
        text: String,
    },
    /// A step finished.
    StepFinished {
        /// The step's identifier.
        step: String,
        /// Whether it succeeded.
        ok: bool,
    },
    /// Something is happening within a step: a line a program printed.
    Activity {
        /// The step's identifier.
        step: String,
        /// What is happening.
        text: String,
    },
    /// How far a step with a size has come.
    Progress {
        /// The step's identifier.
        step: String,
        /// Bytes done.
        done: u64,
        /// Bytes in all, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// The operation finished.
    Result {
        /// `install`, `update`, `remove`, `launch` or `sources`.
        operation: String,
        /// What it did: a document of the operation.
        result: serde_json::Value,
    },
    /// The operation stopped.
    Error {
        /// The failure: code, title, message.
        error: Failure,
    },
}

/// Where events go.
pub struct Emitter {
    json: bool,
    out: Box<dyn Write>,
    /// Every event written, for the caller to look at.
    pub written: Vec<Event>,
}

impl Emitter {
    /// Events on standard output: JSON lines when `json`, text otherwise.
    pub fn stdout(json: bool) -> Self {
        Emitter {
            json,
            out: Box::new(std::io::stdout()),
            written: Vec::new(),
        }
    }

    /// Events into a buffer, for tests.
    pub fn into_buffer(json: bool, out: Box<dyn Write>) -> Self {
        Emitter {
            json,
            out,
            written: Vec::new(),
        }
    }

    /// Whether events are written as JSON.
    pub fn is_json(&self) -> bool {
        self.json
    }

    /// Writes an event.
    pub fn emit(&mut self, event: Event) {
        let line = if self.json {
            let mut value = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "schema_version".to_owned(),
                    serde_json::Value::from(EVENT_VERSION),
                );
            }
            Some(value.to_string())
        } else {
            text(&event)
        };
        if let Some(line) = line {
            let _ = writeln!(self.out, "{line}");
            let _ = self.out.flush();
        }
        self.written.push(event);
    }

    /// Writes text for a person; nothing when events are JSON.
    pub fn text(&mut self, text: &str) {
        if !self.json {
            let _ = writeln!(self.out, "{}", text.trim_end());
            let _ = self.out.flush();
        }
    }

    /// Writes a step's start.
    pub fn started(&mut self, step: &str, text: impl Into<String>) {
        self.emit(Event::StepStarted {
            step: step.to_owned(),
            text: text.into(),
        });
    }

    /// Writes a step's end.
    pub fn finished(&mut self, step: &str, ok: bool) {
        self.emit(Event::StepFinished {
            step: step.to_owned(),
            ok,
        });
    }

    /// Writes an activity line.
    pub fn activity(&mut self, step: &str, text: impl Into<String>) {
        self.emit(Event::Activity {
            step: step.to_owned(),
            text: text.into(),
        });
    }
}

/// An event as a line for a person, or nothing for events a person need
/// not see.
fn text(event: &Event) -> Option<String> {
    match event {
        Event::Plan { .. } => None,
        Event::StepStarted { text, .. } => Some(format!("· {text}")),
        Event::StepFinished { step, ok } => (!ok).then(|| format!("  step {step} failed")),
        Event::Activity { text, .. } => Some(format!("  {text}")),
        Event::Progress { done, total, .. } => match total {
            Some(total) if *total > 0 => Some(format!(
                "  {} of {} ({}%)",
                human_size(*done),
                human_size(*total),
                done.saturating_mul(100) / total
            )),
            _ => Some(format!("  {}", human_size(*done))),
        },
        Event::Result { operation, .. } => Some(format!("{operation}: done")),
        Event::Error { error } => Some(error.to_string()),
    }
}

/// A size for a person, in decimal units, as Flatpak writes them.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::codes::{Code, Failure};

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_events_are_one_object_per_line_with_their_kind() {
        let buffer = Shared::default();
        let mut emitter = Emitter::into_buffer(true, Box::new(buffer.clone()));
        emitter.started("download", "Downloading");
        emitter.emit(Event::Progress {
            step: "download".to_owned(),
            done: 10,
            total: Some(20),
        });
        emitter.emit(Event::Progress {
            step: "install".to_owned(),
            done: 10,
            total: None,
        });
        emitter.emit(Event::Error {
            error: Failure::new(Code::NotVerified, "no"),
        });
        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["event"], "step-started");
        assert_eq!(lines[0]["schema_version"], 1);
        assert_eq!(lines[1]["total"], 20);
        assert!(lines[2].get("total").is_none());
        assert_eq!(lines[3]["event"], "error");
        assert_eq!(lines[3]["error"]["code"], "TUR-DESKTOP-APPS-006");
        assert_eq!(emitter.written.len(), 4);
    }

    #[test]
    fn sizes_are_written_as_flatpak_writes_them() {
        assert_eq!(human_size(999), "999 bytes");
        assert_eq!(human_size(7_048_536), "7.0 MB");
        assert_eq!(human_size(282_700_000), "282.7 MB");
        assert_eq!(human_size(1_500_000_000), "1.5 GB");
    }
}

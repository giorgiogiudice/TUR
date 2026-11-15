// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What a command reports: a result with its facts, or a failure with its
//! code, and how each is printed.

use std::io::Write;

use serde_json::{Map, Value, json};

use crate::codes::Code;

/// Exit status: done.
pub(crate) const EXIT_OK: u8 = 0;
/// Exit status: tried and failed.
pub(crate) const EXIT_FAILED: u8 = 1;
/// Exit status: refused before anything was changed.
pub(crate) const EXIT_REFUSED: u8 = 3;

/// A command that did what it was asked.
#[derive(Debug, Clone)]
pub(crate) struct Done {
    /// One or more sentences for the person.
    pub(crate) message: String,
    /// The facts, as JSON fields.
    pub(crate) facts: Map<String, Value>,
}

/// A command that did not.
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    /// The code.
    pub(crate) code: Code,
    /// What happened and what can be done.
    pub(crate) message: String,
    /// Nothing was changed: the request was refused before acting.
    pub(crate) refused: bool,
    /// The facts known when it failed.
    pub(crate) facts: Map<String, Value>,
}

impl Failure {
    /// A failure after something was attempted.
    pub(crate) fn failed(code: Code, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
            refused: false,
            facts: Map::new(),
        }
    }

    /// A refusal: nothing was changed.
    pub(crate) fn refused(code: Code, message: impl Into<String>) -> Failure {
        Failure {
            refused: true,
            ..Failure::failed(code, message)
        }
    }

    /// The same failure with one more fact.
    pub(crate) fn with(mut self, key: &str, value: Value) -> Failure {
        self.facts.insert(key.to_owned(), value);
        self
    }
}

impl Done {
    /// A result with a message and no facts yet.
    pub(crate) fn new(message: impl Into<String>) -> Done {
        Done {
            message: message.into(),
            facts: Map::new(),
        }
    }

    /// The same result with one more fact.
    pub(crate) fn with(mut self, key: &str, value: Value) -> Done {
        self.facts.insert(key.to_owned(), value);
        self
    }
}

/// The JSON document for an outcome.
pub(crate) fn document(command: &str, outcome: &Result<Done, Failure>) -> Value {
    let mut out = Map::new();
    out.insert("schema_version".to_owned(), json!(1));
    out.insert("command".to_owned(), json!(command));
    match outcome {
        Ok(done) => {
            out.insert("ok".to_owned(), json!(true));
            out.insert("message".to_owned(), json!(done.message));
            for (key, value) in &done.facts {
                out.insert(key.clone(), value.clone());
            }
        }
        Err(failure) => {
            out.insert("ok".to_owned(), json!(false));
            out.insert("code".to_owned(), json!(failure.code));
            out.insert("title".to_owned(), json!(failure.code.title()));
            out.insert("message".to_owned(), json!(failure.message));
            out.insert("refused".to_owned(), json!(failure.refused));
            for (key, value) in &failure.facts {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(out)
}

/// Prints an outcome and returns the exit status.
pub(crate) fn print(
    command: &str,
    outcome: &Result<Done, Failure>,
    json_output: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let status = match outcome {
        Ok(_) => EXIT_OK,
        Err(failure) if failure.refused => EXIT_REFUSED,
        Err(_) => EXIT_FAILED,
    };
    let written = if json_output {
        let text = serde_json::to_string_pretty(&document(command, outcome)).unwrap_or_default();
        writeln!(stdout, "{text}")
    } else {
        match outcome {
            Ok(done) => writeln!(stdout, "{}", done.message),
            Err(failure) => writeln!(
                stderr,
                "{}: {}\n{}",
                failure.code.as_str(),
                failure.code.title(),
                failure.message
            ),
        }
    };
    if written.is_err() { EXIT_FAILED } else { status }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_prints_its_code_and_exits_3() {
        let outcome: Result<Done, Failure> =
            Err(Failure::refused(Code::ArmOnly, "ARM only.").with("package", json!("x")));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(print("install", &outcome, true, &mut out, &mut err), 3);
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["code"], "TUR-ANDROID-ABI-001");
        assert_eq!(value["ok"], false);
        assert_eq!(value["refused"], true);
        assert_eq!(value["package"], "x");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(print("install", &outcome, false, &mut out, &mut err), 3);
        assert!(String::from_utf8(err).unwrap().starts_with("TUR-ANDROID-ABI-001: "));
    }

    #[test]
    fn a_result_prints_its_facts_and_exits_0() {
        let outcome: Result<Done, Failure> = Ok(Done::new("Done.").with("seconds", json!(1.5)));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(print("enable", &outcome, true, &mut out, &mut err), 0);
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["seconds"], 1.5);
        assert_eq!(value["command"], "enable");
    }

    #[test]
    fn a_failure_exits_1() {
        let outcome: Result<Done, Failure> = Err(Failure::failed(Code::StepFailed, "x"));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(print("disable", &outcome, false, &mut out, &mut err), 1);
    }
}

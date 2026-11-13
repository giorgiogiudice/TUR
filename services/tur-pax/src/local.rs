// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Local AI for P@x: what `tur-ai-local` offers this computer, and the one
//! step that makes a model the person chose ready to use.
//!
//! P@x does not download, check or start a model itself. It asks
//! `tur-ai-local` (`services/tur-ai-local`, ADR-0021), which knows the
//! catalogue, the hardware, the publishers' files and their digests, and
//! the engine:
//!
//! * [`offers`] runs `tur-ai-local models --json` and `recommend --json` and
//!   puts together, for each model of the catalogue, its download size,
//!   whether it is downloaded, its licence in plain words, and how it suits
//!   this computer (runs well, slow, does not fit, with the reasons);
//! * [`start_ensure`] starts `tur-ai-local ensure <model> --json`, whose
//!   standard output is one event per line: what is downloaded, how far it
//!   is, that every file was checked, that the engine is ready.
//!
//! Nothing here runs unless a person asks: the service calls these only
//! for the methods `ListLocalModels` and `UseLocalModel` of
//! `org.tur.Agent1`.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

use crate::faults::{Fault, TaskCode};

/// The program of local AI on an installed system.
pub const AI_LOCAL: &str = "/usr/bin/tur-ai-local";

/// The context a local model is started with when the person chose none:
/// `tur-ai-local`'s own default.
pub const DEFAULT_CONTEXT: u32 = 8192;

/// The most bytes read from a report of `tur-ai-local`.
const REPORT_MAX_BYTES: u64 = 8 * 1024 * 1024;

fn unavailable(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceLocalUnavailable, detail)
}

/// The code a failure of `tur-ai-local` printed first on its standard
/// error, as a fault; or local AI's own unavailability.
pub fn fault_of(stderr: &str) -> Fault {
    let first = stderr.lines().next().unwrap_or("").trim();
    match first.parse::<tur_core::ErrorCode>() {
        Ok(code) => Fault {
            code,
            detail: stderr
                .lines()
                .skip(1)
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        },
        Err(_) => unavailable(stderr.lines().take(4).collect::<Vec<_>>().join(" ")),
    }
}

/// Run `tur-ai-local` with these arguments and read the JSON document it
/// prints.
///
/// # Errors
///
/// The code `tur-ai-local` reported, or `TUR-AI-SERVICE-010`.
pub fn report(program: &Path, arguments: &[&str]) -> Result<Value, Fault> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| unavailable(format!("{}: {error}", program.display())))?;
    if !output.status.success() {
        return Err(fault_of(&String::from_utf8_lossy(&output.stderr)));
    }
    if output.stdout.len() as u64 > REPORT_MAX_BYTES {
        return Err(unavailable("the report is too large"));
    }
    crate::strict::value_from_str(&String::from_utf8_lossy(&output.stdout))
        .map_err(|_| unavailable("the report is not one JSON document"))
}

/// What local AI offers this computer: every model of its catalogue that
/// converses, with what a person needs to choose one.
///
/// # Errors
///
/// As [`report`].
pub fn offers(program: &Path) -> Result<Value, Fault> {
    let models = report(program, &["models", "--json"])?;
    let recommendation = report(program, &["recommend", "--json"])?;
    let recommendation = &recommendation["recommendation"];
    let listed: Vec<Value> = models["models"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|model| {
            model["capabilities"]
                .as_array()
                .is_some_and(|capabilities| capabilities.iter().any(|c| c == "chat"))
        })
        .map(|model| {
            json!({
                "id": model["id"],
                "name": model["name"],
                "publisher": model["publisher"],
                "repository": model["repository"],
                "parameters": model["parameters"],
                "quantisation": model["quantisation"],
                "capabilities": model["capabilities"],
                "thinking": model["thinking"],
                "download_bytes": model["download_bytes"],
                "memory_bytes": model["memory_bytes"],
                "installed": model["installed"],
                "install_problem": model["install_problem"],
                "licence": model["licence"],
                "good_for": model["good_for"],
                "fit": model["fit"],
                "tier": model["tier"],
            })
        })
        .collect();
    Ok(json!({
        "recommended": recommendation["model"],
        "message": recommendation["message"],
        "reasons": recommendation["reasons"],
        "vision_model": recommendation["vision_model"],
        "models": listed,
    }))
}

/// One model of [`offers`], by its id.
pub fn offer<'a>(offers: &'a Value, model: &str) -> Option<&'a Value> {
    offers["models"]
        .as_array()?
        .iter()
        .find(|offer| offer["id"] == model)
}

/// How `ensure` is asked to make a model ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureRequest {
    /// The model's catalogue id.
    pub model: String,
    /// The person accepted the restrictions of its licence.
    pub accept_licence: bool,
    /// The context to start it with, in tokens.
    pub context: u32,
    /// Let the chat template decide whether it thinks, so that P@x can ask
    /// for thinking on or off with each request (a model that can think);
    /// otherwise it answers directly.
    pub reasoning_auto: bool,
}

impl EnsureRequest {
    /// The arguments of `tur-ai-local`.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "ensure".to_owned(),
            self.model.clone(),
            "--json".to_owned(),
            "--context".to_owned(),
            self.context.to_string(),
            "--reasoning".to_owned(),
            if self.reasoning_auto { "auto" } else { "off" }.to_owned(),
        ];
        if self.accept_licence {
            arguments.push("--accept-licence".to_owned());
        }
        arguments
    }
}

/// Start `tur-ai-local ensure`. Its standard output is one event per line;
/// its standard error is read when it ends.
///
/// # Errors
///
/// `TUR-AI-SERVICE-010` when the program cannot be started.
pub fn start_ensure(program: &Path, request: &EnsureRequest) -> Result<Child, Fault> {
    Command::new(program)
        .args(request.arguments())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| unavailable(format!("{}: {error}", program.display())))
}

/// Ask a running `ensure` to stop: `tur-ai-local cancel`.
///
/// # Errors
///
/// As [`report`].
pub fn cancel(program: &Path) -> Result<(), Fault> {
    report(program, &["cancel", "--json"]).map(|_| ())
}

/// What a child wrote on its standard error, up to a limit.
pub fn error_text(child: &mut Child) -> String {
    let mut text = String::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(64 * 1024).read_to_string(&mut text);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_local_ai_printed_is_the_fault() {
        let fault = fault_of(
            "TUR-AI-LOCAL-013\nThis model's licence restricts how it may be used.\n\nAction:\nRead it.\n",
        );
        assert_eq!(fault.code.as_str(), "TUR-AI-LOCAL-013");
        assert!(fault.detail.contains("licence restricts"));
        let unknown = fault_of("something else\n");
        assert_eq!(unknown.code.as_str(), "TUR-AI-SERVICE-010");
    }

    #[test]
    fn ensure_is_asked_for_exactly_what_the_person_chose() {
        let request = EnsureRequest {
            model: "ministral-3-3b".to_owned(),
            accept_licence: true,
            context: 16384,
            reasoning_auto: false,
        };
        assert_eq!(
            request.arguments().join(" "),
            "ensure ministral-3-3b --json --context 16384 --reasoning off --accept-licence"
        );
    }
}

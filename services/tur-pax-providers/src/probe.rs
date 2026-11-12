// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A cheap live check that a model calls tools.
//!
//! A provider entry declares what its models can do (ADR-0004, amended
//! 12 November 2026). Declared is not shown: a model served through the
//! OpenAI-compatible family may or may not call tools, depending on the
//! model and on how the server was started. [`probe_tool_calling`] sends
//! one small request that offers one harmless tool and asks for it to be
//! called, and reports what the model did. It is billed by the provider like
//! any request, and is made only when someone asks for it: when a person adds
//! a model, or from `tur-pax-providers check --probe-tools`.
//!
//! The tool does nothing. Its call is never performed; it is only looked at.

use serde_json::json;

use crate::model::{FinishReason, Message, Request, ToolDefinition, Turn, Usage};
use crate::provider::Provider;

/// The name of the tool the probe offers.
pub const PROBE_TOOL: &str = "tur_probe";

/// The most output tokens the probe asks for. A model that reasons before it
/// answers needs room to reach the call; at the prices of 2026-11-12 this is
/// a fraction of a cent with any listed model.
pub const PROBE_OUTPUT_TOKENS: u32 = 512;

/// What the probe found.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolProbe {
    /// The model called the tool, with arguments that read as one JSON
    /// object.
    Verified {
        /// What the request used.
        usage: Usage,
    },
    /// The model answered without calling the tool.
    NotCalled {
        /// What the request used.
        usage: Usage,
    },
    /// The response could not show it: cut at the output limit, refused, or
    /// failed.
    Inconclusive {
        /// Why the response ended.
        reason: FinishReason,
        /// What the request used.
        usage: Usage,
    },
}

impl ToolProbe {
    /// A word for a log or a settings page: `verified`, `not_called` or
    /// `inconclusive`.
    pub fn name(&self) -> &'static str {
        match self {
            ToolProbe::Verified { .. } => "verified",
            ToolProbe::NotCalled { .. } => "not_called",
            ToolProbe::Inconclusive { .. } => "inconclusive",
        }
    }
}

/// The request the probe sends.
pub fn probe_request(model: &str) -> Request {
    let tool = ToolDefinition::new(
        PROBE_TOOL,
        "Confirms that tool calling works. It does nothing else. Call it with word set to ok.",
        json!({
            "type": "object",
            "properties": { "word": { "type": "string" } },
            "required": ["word"],
            "additionalProperties": false
        }),
    );
    Request {
        model: model.to_owned(),
        system: None,
        messages: vec![Message::user_text(
            "This is a test of tool calling. Call the tool tur_probe with word set to ok. Do not \
             write anything else.",
        )],
        tools: tool.into_iter().collect(),
        max_output_tokens: PROBE_OUTPUT_TOKENS,
    }
}

/// Send the probe and say what the model did.
pub fn probe_tool_calling(provider: &dyn Provider, model: &str) -> ToolProbe {
    let turn = Turn::collect(provider.stream(&probe_request(model)));
    judge(turn)
}

fn judge(turn: Turn) -> ToolProbe {
    let usage = turn.usage;
    let called = turn.tool_calls().iter().any(|call| call.name == PROBE_TOOL);
    match turn.finish {
        FinishReason::ToolUse | FinishReason::EndTurn if called => ToolProbe::Verified { usage },
        FinishReason::ToolUse | FinishReason::EndTurn => ToolProbe::NotCalled { usage },
        reason => ToolProbe::Inconclusive { reason, usage },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value};

    use super::*;
    use crate::error::{ErrorKind, ProviderError};
    use crate::model::{Part, ToolCall};

    fn turn(parts: Vec<Part>, finish: FinishReason) -> Turn {
        Turn {
            message: Message::assistant(parts),
            usage: Usage::default(),
            finish,
        }
    }

    fn call(name: &str) -> Part {
        let mut arguments = Map::new();
        arguments.insert("word".to_owned(), Value::String("ok".to_owned()));
        Part::ToolCall(ToolCall {
            id: "c".to_owned(),
            name: name.to_owned(),
            arguments,
        })
    }

    #[test]
    fn the_probe_offers_one_harmless_tool_and_a_small_limit() {
        let request = probe_request("example-model");
        assert_eq!(request.model, "example-model");
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name(), PROBE_TOOL);
        assert_eq!(request.max_output_tokens, PROBE_OUTPUT_TOKENS);
        assert_eq!(request.messages.len(), 1);
    }

    #[test]
    fn what_the_model_did_decides_the_verdict() {
        let verdict = judge(turn(vec![call(PROBE_TOOL)], FinishReason::ToolUse));
        assert_eq!(verdict.name(), "verified");
        let verdict = judge(turn(vec![Part::text("ok")], FinishReason::EndTurn));
        assert_eq!(verdict.name(), "not_called");
        let verdict = judge(turn(vec![call("something_else")], FinishReason::ToolUse));
        assert_eq!(verdict.name(), "not_called");
        for reason in [
            FinishReason::OutputLimit,
            FinishReason::Refusal(ProviderError::new(ErrorKind::Refusal)),
            FinishReason::Error(ProviderError::new(ErrorKind::MalformedResponse)),
        ] {
            let verdict = judge(turn(vec![call(PROBE_TOOL)], reason));
            assert_eq!(verdict.name(), "inconclusive");
        }
    }
}

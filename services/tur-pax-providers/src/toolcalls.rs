// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Assembles streamed tool calls.
//!
//! Every provider streams a tool call the same way in outline: a start that
//! names the tool, fragments of the arguments as JSON text, and an end. They
//! differ in how a call is identified while it streams (a content block index,
//! an output index, a step index). An adapter passes that number as the `key`;
//! this module turns the pieces into neutral events and numbers the calls of a
//! response from 0.
//!
//! A call whose arguments do not parse as a JSON object is never delivered as
//! complete. Whether that is an error depends on why the response ended, which
//! [`ToolCalls::settle`] decides.

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;

use crate::error::{ErrorKind, ProviderError};
use crate::model::{FinishReason, StreamEvent, ToolCall};

struct OpenCall {
    index: usize,
    id: String,
    name: String,
    arguments: String,
}

/// The tool calls of one response.
#[derive(Default)]
pub(crate) struct ToolCalls {
    open: BTreeMap<u64, OpenCall>,
    started: usize,
    completed: usize,
    broken: bool,
}

impl ToolCalls {
    /// A call started. `key` identifies it until it ends.
    pub(crate) fn start(&mut self, key: u64, id: &str, name: &str, out: &mut VecDeque<StreamEvent>) {
        let index = self.started;
        self.started += 1;
        let id = if id.is_empty() {
            format!("call-{index}")
        } else {
            id.to_owned()
        };
        out.push_back(StreamEvent::ToolCallStart {
            index,
            id: id.clone(),
            name: name.to_owned(),
        });
        self.open.insert(
            key,
            OpenCall {
                index,
                id,
                name: name.to_owned(),
                arguments: String::new(),
            },
        );
    }

    /// Whether a call with this key is open.
    pub(crate) fn is_open(&self, key: u64) -> bool {
        self.open.contains_key(&key)
    }

    /// More argument text arrived for an open call.
    pub(crate) fn fragment(&mut self, key: u64, fragment: &str, out: &mut VecDeque<StreamEvent>) {
        if fragment.is_empty() {
            return;
        }
        if let Some(call) = self.open.get_mut(&key) {
            call.arguments.push_str(fragment);
            out.push_back(StreamEvent::ToolCallArgumentsDelta {
                index: call.index,
                fragment: fragment.to_owned(),
            });
        }
    }

    /// The provider sent the complete argument text of an open call. It
    /// replaces what the fragments added up to.
    pub(crate) fn set_arguments(&mut self, key: u64, arguments: &str) {
        if let Some(call) = self.open.get_mut(&key) {
            call.arguments = arguments.to_owned();
        }
    }

    /// An open call ended.
    pub(crate) fn end(&mut self, key: u64, out: &mut VecDeque<StreamEvent>) {
        let Some(call) = self.open.remove(&key) else {
            return;
        };
        let text = if call.arguments.trim().is_empty() {
            "{}"
        } else {
            call.arguments.as_str()
        };
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(arguments)) => {
                self.completed += 1;
                out.push_back(StreamEvent::ToolCallEnd {
                    index: call.index,
                    call: ToolCall {
                        id: call.id,
                        name: call.name,
                        arguments,
                    },
                });
            }
            _ => self.broken = true,
        }
    }

    /// How many calls were delivered complete.
    pub(crate) fn completed(&self) -> usize {
        self.completed
    }

    /// The reason to report, given the reason the provider gave.
    ///
    /// A response that claims a normal end while a call is unfinished or its
    /// arguments are not a JSON object is malformed. When the response was cut
    /// short, refused or failed, the incomplete call is expected and the
    /// provider's reason stands.
    pub(crate) fn settle(&self, reason: FinishReason) -> FinishReason {
        let incomplete = self.broken || !self.open.is_empty();
        match reason {
            FinishReason::EndTurn | FinishReason::ToolUse if incomplete => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse).with_detail(
                    "a tool call was left unfinished or its arguments were not a JSON object",
                ),
            ),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_call_is_assembled_from_fragments() {
        let mut calls = ToolCalls::default();
        let mut out = VecDeque::new();
        calls.start(7, "call-a", "storage_unmount", &mut out);
        assert!(calls.is_open(7));
        calls.fragment(7, "", &mut out);
        calls.fragment(7, "{\"volume_id\":", &mut out);
        calls.fragment(7, " \"vol-1\"}", &mut out);
        calls.fragment(8, "ignored: no such call", &mut out);
        calls.end(7, &mut out);
        calls.end(7, &mut out);
        assert!(!calls.is_open(7));
        assert_eq!(calls.completed(), 1);

        let events: Vec<StreamEvent> = out.into_iter().collect();
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            StreamEvent::ToolCallStart {
                index: 0,
                id: "call-a".to_owned(),
                name: "storage_unmount".to_owned()
            }
        );
        match &events[3] {
            StreamEvent::ToolCallEnd { index, call } => {
                assert_eq!(*index, 0);
                assert_eq!(call.id, "call-a");
                assert_eq!(Value::Object(call.arguments.clone()), json!({ "volume_id": "vol-1" }));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(calls.settle(FinishReason::ToolUse), FinishReason::ToolUse);
    }

    #[test]
    fn calls_are_numbered_in_order_and_empty_arguments_are_an_empty_object() {
        let mut calls = ToolCalls::default();
        let mut out = VecDeque::new();
        calls.start(3, "", "first", &mut out);
        calls.start(5, "id-2", "second", &mut out);
        calls.set_arguments(5, "{\"a\":1}");
        calls.end(5, &mut out);
        calls.end(3, &mut out);
        let ended: Vec<(usize, String, Value)> = out
            .into_iter()
            .filter_map(|event| match event {
                StreamEvent::ToolCallEnd { index, call } => {
                    Some((index, call.id, Value::Object(call.arguments)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            ended,
            vec![
                (1, "id-2".to_owned(), json!({ "a": 1 })),
                (0, "call-0".to_owned(), json!({})),
            ]
        );
        assert_eq!(calls.completed(), 2);
    }

    #[test]
    fn a_broken_or_unfinished_call_is_malformed_only_on_a_normal_end() {
        for arguments in ["{\"volume_id\": \"vo", "[1, 2]", "\"text\"", "not json"] {
            let mut calls = ToolCalls::default();
            let mut out = VecDeque::new();
            calls.start(0, "c", "tool", &mut out);
            calls.fragment(0, arguments, &mut out);
            calls.end(0, &mut out);
            assert_eq!(calls.completed(), 0, "{arguments}");
            assert!(
                !out.iter()
                    .any(|event| matches!(event, StreamEvent::ToolCallEnd { .. })),
                "{arguments}"
            );
            for normal in [FinishReason::EndTurn, FinishReason::ToolUse] {
                let settled = calls.settle(normal);
                let error = settled.error().expect("an error");
                assert_eq!(error.kind(), ErrorKind::MalformedResponse);
            }
            assert_eq!(
                calls.settle(FinishReason::OutputLimit),
                FinishReason::OutputLimit
            );
        }

        let mut calls = ToolCalls::default();
        let mut out = VecDeque::new();
        calls.start(0, "c", "tool", &mut out);
        assert!(calls.settle(FinishReason::ToolUse).error().is_some());
        let refusal = FinishReason::Refusal(ProviderError::new(ErrorKind::Refusal));
        assert_eq!(calls.settle(refusal.clone()), refusal);
    }
}

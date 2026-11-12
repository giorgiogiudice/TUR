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
//! **The arguments of a call have one reading, or the call is not delivered.**
//! The argument text is parsed with [`crate::strict`]: it must be one JSON
//! object in which no object, at any depth, repeats a key. Where a provider
//! sends both fragments and the complete text, the two must say the same.
//! The text that was parsed is delivered with the call, so that whoever acts
//! on the call can read it again.
//!
//! A call that fails any of this is never delivered as complete. Whether that
//! is an error depends on why the response ended, which [`ToolCalls::settle`]
//! decides.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value};

use crate::error::{ErrorKind, ProviderError};
use crate::model::{FinishReason, StreamEvent, ToolCall};
use crate::strict;

struct OpenCall {
    index: usize,
    id: String,
    name: String,
    /// The fragments the provider streamed, added up.
    streamed: String,
    /// The complete argument text, where the provider sent it in one piece.
    whole: Option<String>,
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
    pub(crate) fn start(
        &mut self,
        key: u64,
        id: &str,
        name: &str,
        out: &mut VecDeque<StreamEvent>,
    ) {
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
                streamed: String::new(),
                whole: None,
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
            call.streamed.push_str(fragment);
            out.push_back(StreamEvent::ToolCallArgumentsDelta {
                index: call.index,
                fragment: fragment.to_owned(),
            });
        }
    }

    /// The provider sent the complete argument text of an open call in one
    /// piece. It is the text the call is read from. Fragments, if there were
    /// any, must add up to the same arguments.
    pub(crate) fn set_arguments(&mut self, key: u64, arguments: &str) {
        if let Some(call) = self.open.get_mut(&key) {
            call.whole = Some(arguments.to_owned());
        }
    }

    /// An open call ended.
    pub(crate) fn end(&mut self, key: u64, out: &mut VecDeque<StreamEvent>) {
        let Some(call) = self.open.remove(&key) else {
            return;
        };
        match read_arguments(call.whole.as_deref(), &call.streamed) {
            Some((arguments, arguments_text)) => {
                self.completed += 1;
                out.push_back(StreamEvent::ToolCallEnd {
                    index: call.index,
                    call: ToolCall {
                        id: call.id,
                        name: call.name,
                        arguments,
                    },
                    arguments_text,
                });
            }
            None => self.broken = true,
        }
    }

    /// How many calls were delivered complete.
    pub(crate) fn completed(&self) -> usize {
        self.completed
    }

    /// The reason to report, given the reason the provider gave.
    ///
    /// A response that claims a normal end while a call is unfinished, or its
    /// arguments are not one JSON object with one reading, is malformed. When
    /// the response was cut short, refused or failed, the incomplete call is
    /// expected and the provider's reason stands.
    pub(crate) fn settle(&self, reason: FinishReason) -> FinishReason {
        let incomplete = self.broken || !self.open.is_empty();
        match reason {
            FinishReason::EndTurn | FinishReason::ToolUse if incomplete => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse).with_detail(
                    "a tool call was left unfinished, or its arguments were not one JSON object \
                     with one reading",
                ),
            ),
            other => other,
        }
    }
}

/// The arguments of a call and the text they were read from, or `None` if the
/// text has no single reading.
///
/// `whole` is the complete text where the provider sent one; `streamed` is
/// what the fragments add up to. No text at all means no arguments.
fn read_arguments(whole: Option<&str>, streamed: &str) -> Option<(Map<String, Value>, String)> {
    let streamed = streamed.trim();
    let text = match whole.map(str::trim) {
        Some(whole) if !whole.is_empty() => whole,
        _ => streamed,
    };
    let text = if text.is_empty() { "{}" } else { text };
    let arguments = strict::object_from_str(text).ok()?;
    // A provider that streamed one thing and then stated another has shown
    // two readings of one call.
    if whole.is_some() && !streamed.is_empty() {
        let fragments = strict::object_from_str(streamed).ok()?;
        if fragments != arguments {
            return None;
        }
    }
    Some((arguments, text.to_owned()))
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
            StreamEvent::ToolCallEnd {
                index,
                call,
                arguments_text,
            } => {
                assert_eq!(*index, 0);
                assert_eq!(call.id, "call-a");
                assert_eq!(
                    Value::Object(call.arguments.clone()),
                    json!({ "volume_id": "vol-1" })
                );
                // The text the arguments were read from comes with the call.
                assert_eq!(arguments_text, "{\"volume_id\": \"vol-1\"}");
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
        let ended: Vec<(usize, String, Value, String)> = out
            .into_iter()
            .filter_map(|event| match event {
                StreamEvent::ToolCallEnd {
                    index,
                    call,
                    arguments_text,
                } => Some((
                    index,
                    call.id,
                    Value::Object(call.arguments),
                    arguments_text,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            ended,
            vec![
                (
                    1,
                    "id-2".to_owned(),
                    json!({ "a": 1 }),
                    "{\"a\":1}".to_owned()
                ),
                (0, "call-0".to_owned(), json!({}), "{}".to_owned()),
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

    /// Deliver one call and say whether it came out complete.
    fn delivered(fragments: &[&str], whole: Option<&str>) -> bool {
        let mut calls = ToolCalls::default();
        let mut out = VecDeque::new();
        calls.start(0, "c", "tool", &mut out);
        for fragment in fragments {
            calls.fragment(0, fragment, &mut out);
        }
        if let Some(whole) = whole {
            calls.set_arguments(0, whole);
        }
        calls.end(0, &mut out);
        let complete = out
            .iter()
            .any(|event| matches!(event, StreamEvent::ToolCallEnd { .. }));
        assert_eq!(complete, calls.completed() == 1);
        // A call that was not delivered makes a normal end malformed.
        assert_eq!(
            calls.settle(FinishReason::ToolUse).error().is_none(),
            complete
        );
        complete
    }

    #[test]
    fn a_repeated_key_at_any_depth_is_never_delivered() {
        for text in [
            r#"{"to": "a", "to": "b"}"#,
            r#"{"to": "a", "to": "a"}"#,
            r#"{"to": "a", "cc": {"x": 1, "x": 2}}"#,
            r#"{"list": [1, {"deep": [{"k": null, "k": null}]}]}"#,
            // The same key, once written with an escape.
            r#"{"to": "a", "to": "b"}"#,
        ] {
            // In fragments, cut anywhere.
            let middle = text.len() / 2;
            assert!(
                !delivered(&[&text[..middle], &text[middle..]], None),
                "{text}"
            );
            // In one piece.
            assert!(!delivered(&[], Some(text)), "{text}");
            // The fragments are fine and the complete text is not, and the
            // other way round.
            assert!(!delivered(&[r#"{"to": "b"}"#], Some(text)), "{text}");
            assert!(!delivered(&[text], Some(r#"{"to": "b"}"#)), "{text}");
        }
        // The same texts without the repetition are delivered.
        assert!(delivered(&[r#"{"to": "a", "#, r#""cc": {"x": 1}}"#], None));
        assert!(delivered(&[], Some(r#"{"to": "a", "cc": {"x": 1}}"#)));
    }

    #[test]
    fn fragments_and_the_complete_text_must_say_the_same() {
        // The same arguments, written differently: one reading.
        assert!(delivered(
            &["{\"a\":", "1,\"b\":[2]}"],
            Some("{ \"b\": [2], \"a\": 1 }")
        ));
        // Another value, another key, another kind of number.
        assert!(!delivered(&["{\"a\":1}"], Some("{\"a\":2}")));
        assert!(!delivered(&["{\"a\":1}"], Some("{\"a\":1,\"b\":2}")));
        assert!(!delivered(&["{\"a\":1}"], Some("{\"a\":1.0}")));
        // Fragments that are not JSON at all, next to a complete text.
        assert!(!delivered(&["{\"a\":"], Some("{\"a\":1}")));
        // A complete text that is empty stands for nothing: the fragments
        // are read.
        assert!(delivered(&["{\"a\":1}"], Some("  ")));
        assert!(delivered(&[], Some("")));
        assert!(delivered(&[], None));
    }

    #[test]
    fn the_text_delivered_with_a_call_reads_as_its_arguments() {
        for (fragments, whole) in [
            (vec!["{\"a\":", " [1, 2]}"], None),
            (vec![], Some(" {\"a\": [1, 2]} ")),
            (vec!["{\"a\":[1,2]}"], Some("{\"a\": [1, 2]}")),
            (vec![], None),
        ] {
            let mut calls = ToolCalls::default();
            let mut out = VecDeque::new();
            calls.start(0, "c", "tool", &mut out);
            for fragment in &fragments {
                calls.fragment(0, fragment, &mut out);
            }
            if let Some(whole) = whole {
                calls.set_arguments(0, whole);
            }
            calls.end(0, &mut out);
            let Some(StreamEvent::ToolCallEnd {
                call,
                arguments_text,
                ..
            }) = out.pop_back()
            else {
                panic!("the call was not delivered: {fragments:?} {whole:?}");
            };
            assert_eq!(
                strict::object_from_str(&arguments_text).ok(),
                Some(call.arguments)
            );
        }
    }
}

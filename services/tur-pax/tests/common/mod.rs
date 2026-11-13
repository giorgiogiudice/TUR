// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Shared by the orchestrator's integration tests: a scripted provider, a
//! recording executor, a clock that is moved by hand, and a harness that
//! puts a task loop together.
//!
//! The scripted provider is a small in-process HTTP server that replays
//! responses in the streaming format of one of the three providers ([`Wire`]),
//! the same approach as the conformance suite of `tur-pax-providers`. The
//! loop under test talks to it through the real adapter for that provider,
//! over a real local connection. A script says what the model "does": the
//! text it writes, the tool calls it makes with the exact text of their
//! arguments, whether those arguments are streamed in fragments or arrive in
//! one piece, and how the response ends. The formats are written by hand
//! from the same documentation as the conformance scripts; they are not
//! recordings. No test talks to a real provider and none needs an API key.

#![allow(dead_code)]
#![allow(clippy::new_without_default)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use tur_pax::audit::{AuditRecord, AuditWriteError};
use tur_pax::exec::{ExecOutput, ExecStatus, Executor};
use tur_pax::orchestrator::{
    Control, NoAssistant, NoBudget, NoControl, NoObserver, Orchestrator, Signal, Situation,
    TaskClock, TaskLimits, TaskReport, TaskRequest,
};
use tur_pax::registry::ToolRegistry;
use tur_pax::sandbox::{Grant, GrantRules, SandboxPlan};
use tur_pax::trail::{EventKind, MemoryTrail, Trail, TrailEvent};
use tur_pax::{Timestamp, UserPolicy};
use tur_pax_memory::RecalledNotes;
use tur_pax_providers::{ApiKey, Message, Provider, ProviderConfig, SpendMeter, connect};

/// The key every test uses. It is not a real key.
pub const TEST_KEY: &str = "sk-test-KEY-7f3a9c2e5b1d4860";

/// The manifest of the test service: one tool of each class 0 to 3.
pub const TEST_MANIFEST: &str = include_str!("../data/test-service.manifest.json");

/// Adapters for every tool of the test service.
pub const TEST_ADAPTERS: &str = include_str!("../data/test-service.adapters.json");

// ------------------------------------------------------------- the script

/// One tool call of a scripted response.
#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    pub name: String,
    /// The arguments exactly as the model "typed" them: JSON text, which may
    /// repeat a key or be otherwise odd.
    pub arguments: String,
}

/// A tool call with the given identifier, tool name and argument text.
pub fn call(id: &str, name: &str, arguments: &str) -> Call {
    Call {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: arguments.to_owned(),
    }
}

/// One scripted response of the model.
#[derive(Debug, Clone)]
pub struct Turn {
    pub text: String,
    pub calls: Vec<Call>,
    /// The stop reason of the Messages API, or `error` for an error event in
    /// the middle of the stream.
    pub stop: &'static str,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Whether the arguments of each call arrive in one piece, with no
    /// fragment, instead of being streamed.
    pub whole: bool,
}

/// The model writes an answer and ends its turn.
pub fn says(text: &str) -> Turn {
    Turn {
        text: text.to_owned(),
        calls: Vec::new(),
        stop: "end_turn",
        input_tokens: 40,
        output_tokens: 30,
        whole: false,
    }
}

/// The model writes some text and makes tool calls.
pub fn calls(text: &str, calls: Vec<Call>) -> Turn {
    Turn {
        text: text.to_owned(),
        calls,
        stop: "tool_use",
        input_tokens: 40,
        output_tokens: 30,
        whole: false,
    }
}

impl Turn {
    /// The same response, ending another way.
    pub fn stopping(mut self, stop: &'static str) -> Turn {
        self.stop = stop;
        self
    }

    /// The same response, with the arguments of every call delivered in one
    /// piece: no fragment is streamed.
    pub fn whole(mut self) -> Turn {
        self.whole = true;
        self
    }
}

/// The wire format the scripted provider speaks, which decides the adapter
/// the loop talks to it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// The Anthropic Messages API.
    Anthropic,
    /// The OpenAI Responses API.
    OpenAi,
    /// The Google Interactions API.
    Google,
    /// The OpenAI-compatible Chat Completions API (DeepSeek and others).
    OpenAiChat,
}

impl Wire {
    /// Every format an adapter exists for.
    pub const ALL: [Wire; 4] = [
        Wire::Anthropic,
        Wire::OpenAi,
        Wire::Google,
        Wire::OpenAiChat,
    ];

    /// The provider name of a configuration file.
    pub fn provider(self) -> &'static str {
        match self {
            Wire::Anthropic => "anthropic",
            Wire::OpenAi => "openai",
            Wire::Google => "google",
            Wire::OpenAiChat => "openai-chat",
        }
    }

    /// A scripted response as an event stream in this format.
    pub fn render(self, turn: &Turn) -> String {
        match self {
            Wire::Anthropic => sse(turn),
            Wire::OpenAi => sse_openai(turn),
            Wire::Google => sse_google(turn),
            Wire::OpenAiChat => sse_openai_chat(turn),
        }
    }
}

/// One chunk of a Chat Completions stream.
fn chunk(choices: &Value, usage: &Value) -> String {
    let data = json!({ "id": "chatcmpl-scripted", "object": "chat.completion.chunk",
                       "created": 1_794_477_600, "model": "test-model",
                       "choices": choices, "usage": usage });
    format!("data: {data}\n\n")
}

/// The response as a stream of the OpenAI-compatible Chat Completions API,
/// written by hand from OpenAI's reference and DeepSeek's documentation.
///
/// A streamed call starts with its identifier and name and its argument text
/// arrives in two more deltas. A call delivered whole arrives in one delta,
/// as xAI documents. The usage comes in a last chunk with no choices.
pub fn sse_openai_chat(turn: &Turn) -> String {
    let delta = |delta: Value| json!([{ "index": 0, "delta": delta, "finish_reason": null }]);
    let mut stream = chunk(
        &delta(json!({ "role": "assistant", "content": "" })),
        &Value::Null,
    );
    if !turn.text.is_empty() {
        stream.push_str(&chunk(
            &delta(json!({ "content": turn.text })),
            &Value::Null,
        ));
    }
    for (index, call) in turn.calls.iter().enumerate() {
        if turn.whole {
            stream.push_str(&chunk(
                &delta(
                    json!({ "tool_calls": [{ "index": index, "id": call.id, "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments } }] }),
                ),
                &Value::Null,
            ));
            continue;
        }
        stream.push_str(&chunk(
            &delta(
                json!({ "tool_calls": [{ "index": index, "id": call.id, "type": "function",
                "function": { "name": call.name, "arguments": "" } }] }),
            ),
            &Value::Null,
        ));
        let (first, second) = halves(&call.arguments);
        for fragment in [first, second] {
            stream.push_str(&chunk(
                &delta(json!({ "tool_calls": [{ "index": index,
                    "function": { "arguments": fragment } }] })),
                &Value::Null,
            ));
        }
    }
    let finish = match turn.stop {
        "error" => {
            stream.push_str(&format!(
                "data: {}\n\n",
                json!({ "error": { "message": "The model is temporarily overloaded.",
                                   "type": "server_error", "code": "server_is_overloaded" } })
            ));
            return stream;
        }
        "max_tokens" => "length",
        "refusal" => {
            stream.push_str(&chunk(
                &delta(json!({ "refusal": "The scripted model declined." })),
                &Value::Null,
            ));
            "stop"
        }
        _ if !turn.calls.is_empty() => "tool_calls",
        _ => "stop",
    };
    stream.push_str(&chunk(
        &json!([{ "index": 0, "delta": {}, "finish_reason": finish }]),
        &Value::Null,
    ));
    stream.push_str(&chunk(
        &json!([]),
        &json!({ "prompt_tokens": turn.input_tokens, "completion_tokens": turn.output_tokens,
                 "total_tokens": turn.input_tokens + turn.output_tokens }),
    ));
    stream.push_str("data: [DONE]\n\n");
    stream
}

fn event(name: &str, data: &Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

/// An event whose data is written by hand, so that it can hold what a JSON
/// value cannot: an object that repeats a key.
fn raw_event(name: &str, data: &str) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

/// A text cut in two at a character.
fn halves(text: &str) -> (&str, &str) {
    let middle = text
        .char_indices()
        .map(|(at, _)| at)
        .nth(text.chars().count() / 2)
        .unwrap_or(0);
    text.split_at(middle)
}

/// The response as an event stream of the Messages API.
pub fn sse(turn: &Turn) -> String {
    let mut stream = event(
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": "msg_scripted", "type": "message", "role": "assistant", "content": [],
                "model": "test-model", "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": turn.input_tokens, "output_tokens": 1 }
            }
        }),
    );
    let mut index = 0;
    if !turn.text.is_empty() {
        stream.push_str(&event(
            "content_block_start",
            &json!({ "type": "content_block_start", "index": index,
                     "content_block": { "type": "text", "text": "" } }),
        ));
        stream.push_str(&event(
            "content_block_delta",
            &json!({ "type": "content_block_delta", "index": index,
                     "delta": { "type": "text_delta", "text": turn.text } }),
        ));
        stream.push_str(&event(
            "content_block_stop",
            &json!({ "type": "content_block_stop", "index": index }),
        ));
        index += 1;
    }
    for call in &turn.calls {
        if turn.whole {
            // The block starts with its whole input, written as the script
            // has it, and has no delta.
            stream.push_str(&raw_event(
                "content_block_start",
                &format!(
                    r#"{{"type":"content_block_start","index":{index},"content_block":{{"type":"tool_use","id":{id},"name":{name},"input":{input}}}}}"#,
                    id = json!(call.id),
                    name = json!(call.name),
                    input = call.arguments,
                ),
            ));
            stream.push_str(&event(
                "content_block_stop",
                &json!({ "type": "content_block_stop", "index": index }),
            ));
            index += 1;
            continue;
        }
        stream.push_str(&event(
            "content_block_start",
            &json!({ "type": "content_block_start", "index": index,
                     "content_block": { "type": "tool_use", "id": call.id, "name": call.name,
                                        "input": {} } }),
        ));
        // The argument text arrives in two fragments, cut at a character.
        let middle = call
            .arguments
            .char_indices()
            .map(|(at, _)| at)
            .nth(call.arguments.chars().count() / 2)
            .unwrap_or(0);
        let (first, second) = call.arguments.split_at(middle);
        for fragment in [first, second] {
            stream.push_str(&event(
                "content_block_delta",
                &json!({ "type": "content_block_delta", "index": index,
                         "delta": { "type": "input_json_delta", "partial_json": fragment } }),
            ));
        }
        stream.push_str(&event(
            "content_block_stop",
            &json!({ "type": "content_block_stop", "index": index }),
        ));
        index += 1;
    }
    if turn.stop == "error" {
        stream.push_str(&event(
            "error",
            &json!({ "type": "error",
                     "error": { "type": "overloaded_error", "message": "Overloaded" } }),
        ));
        return stream;
    }
    let mut delta = json!({ "stop_reason": turn.stop, "stop_sequence": null });
    if turn.stop == "refusal" {
        delta["stop_details"] = json!({ "type": "refusal", "category": "scripted",
                                        "explanation": "The scripted model declined." });
    }
    stream.push_str(&event(
        "message_delta",
        &json!({ "type": "message_delta", "delta": delta,
                 "usage": { "output_tokens": turn.output_tokens } }),
    ));
    stream.push_str(&event("message_stop", &json!({ "type": "message_stop" })));
    stream
}

/// The response as an event stream of the OpenAI Responses API.
///
/// A streamed call is announced, its argument text arrives in two deltas, and
/// the finished item repeats it. A call delivered whole is only the finished
/// item.
pub fn sse_openai(turn: &Turn) -> String {
    let response = |status: &str| {
        json!({ "id": "resp_scripted", "object": "response", "status": status,
                "model": "test-model", "output": [] })
    };
    let mut stream = event(
        "response.created",
        &json!({ "type": "response.created", "response": response("in_progress") }),
    );
    let mut index = 0;
    if !turn.text.is_empty() {
        stream.push_str(&event(
            "response.output_item.added",
            &json!({ "type": "response.output_item.added", "output_index": index,
                     "item": { "id": "msg_scripted", "type": "message", "status": "in_progress",
                               "role": "assistant", "content": [] } }),
        ));
        stream.push_str(&event(
            "response.output_text.delta",
            &json!({ "type": "response.output_text.delta", "item_id": "msg_scripted",
                     "output_index": index, "content_index": 0, "delta": turn.text }),
        ));
        stream.push_str(&event(
            "response.output_item.done",
            &json!({ "type": "response.output_item.done", "output_index": index,
                     "item": { "id": "msg_scripted", "type": "message", "status": "completed",
                               "role": "assistant",
                               "content": [{ "type": "output_text", "text": turn.text,
                                             "annotations": [] }] } }),
        ));
        index += 1;
    }
    for call in &turn.calls {
        let item_id = format!("fc_{index}");
        if !turn.whole {
            stream.push_str(&event(
                "response.output_item.added",
                &json!({ "type": "response.output_item.added", "output_index": index,
                         "item": { "type": "function_call", "id": item_id, "call_id": call.id,
                                   "name": call.name, "arguments": "" } }),
            ));
            let (first, second) = halves(&call.arguments);
            for fragment in [first, second] {
                stream.push_str(&event(
                    "response.function_call_arguments.delta",
                    &json!({ "type": "response.function_call_arguments.delta",
                             "item_id": item_id, "output_index": index, "delta": fragment }),
                ));
            }
            stream.push_str(&event(
                "response.function_call_arguments.done",
                &json!({ "type": "response.function_call_arguments.done", "item_id": item_id,
                         "output_index": index, "arguments": call.arguments }),
            ));
        }
        stream.push_str(&event(
            "response.output_item.done",
            &json!({ "type": "response.output_item.done", "output_index": index,
                     "item": { "type": "function_call", "id": item_id, "call_id": call.id,
                               "name": call.name, "arguments": call.arguments,
                               "status": "completed" } }),
        ));
        index += 1;
    }
    let usage = json!({
        "input_tokens": turn.input_tokens,
        "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens": turn.output_tokens,
        "output_tokens_details": { "reasoning_tokens": 0 },
        "total_tokens": turn.input_tokens + turn.output_tokens
    });
    match turn.stop {
        "error" => {
            stream.push_str(&event(
                "error",
                &json!({ "type": "error", "code": "server_is_overloaded",
                         "message": "The model is temporarily overloaded.", "param": null }),
            ));
            return stream;
        }
        "max_tokens" => {
            let mut ended = response("incomplete");
            ended["incomplete_details"] = json!({ "reason": "max_output_tokens" });
            ended["usage"] = usage;
            stream.push_str(&event(
                "response.incomplete",
                &json!({ "type": "response.incomplete", "response": ended }),
            ));
            return stream;
        }
        "refusal" => stream.push_str(&event(
            "response.refusal.done",
            &json!({ "type": "response.refusal.done", "item_id": "msg_refusal",
                     "output_index": index, "content_index": 0,
                     "refusal": "The scripted model declined." }),
        )),
        _ => {}
    }
    let mut ended = response("completed");
    ended["usage"] = usage;
    stream.push_str(&event(
        "response.completed",
        &json!({ "type": "response.completed", "response": ended }),
    ));
    stream
}

/// The response as an event stream of the Google Interactions API.
///
/// A streamed call starts with empty arguments and its argument text arrives
/// in two deltas. A call delivered whole starts with its arguments complete,
/// as a JSON object written as the script has it, and has no delta.
pub fn sse_google(turn: &Turn) -> String {
    let mut stream = event(
        "interaction.created",
        &json!({ "interaction": { "id": "v1_scripted", "status": "in_progress",
                                  "object": "interaction", "model": "test-model" },
                 "event_type": "interaction.created" }),
    );
    let stop = |index: u64| {
        event(
            "step.stop",
            &json!({ "index": index, "event_type": "step.stop" }),
        )
    };
    let mut index = 0u64;
    if !turn.text.is_empty() {
        stream.push_str(&event(
            "step.start",
            &json!({ "index": index, "step": { "type": "model_output" },
                     "event_type": "step.start" }),
        ));
        stream.push_str(&event(
            "step.delta",
            &json!({ "index": index, "delta": { "text": turn.text, "type": "text" },
                     "event_type": "step.delta" }),
        ));
        stream.push_str(&stop(index));
        index += 1;
    }
    for call in &turn.calls {
        if turn.whole {
            stream.push_str(&raw_event(
                "step.start",
                &format!(
                    r#"{{"index":{index},"step":{{"id":{id},"type":"function_call","name":{name},"arguments":{arguments}}},"event_type":"step.start"}}"#,
                    id = json!(call.id),
                    name = json!(call.name),
                    arguments = call.arguments,
                ),
            ));
        } else {
            stream.push_str(&event(
                "step.start",
                &json!({ "index": index,
                         "step": { "id": call.id, "type": "function_call", "name": call.name,
                                   "arguments": {} },
                         "event_type": "step.start" }),
            ));
            let (first, second) = halves(&call.arguments);
            for fragment in [first, second] {
                stream.push_str(&event(
                    "step.delta",
                    &json!({ "index": index,
                             "delta": { "arguments": fragment, "type": "arguments_delta" },
                             "event_type": "step.delta" }),
                ));
            }
        }
        stream.push_str(&stop(index));
        index += 1;
    }
    let failed = match turn.stop {
        "error" => Some((
            "service_unavailable",
            "The service is temporarily overloaded.",
        )),
        "refusal" => Some(("safety", "Safety violations blocked the request.")),
        _ => None,
    };
    if let Some((code, message)) = failed {
        stream.push_str(&event(
            "error",
            &json!({ "error": { "message": message, "code": code }, "event_type": "error" }),
        ));
        return stream;
    }
    let status = match turn.stop {
        "max_tokens" => "incomplete",
        "tool_use" => "requires_action",
        _ => "completed",
    };
    stream.push_str(&event(
        "interaction.completed",
        &json!({ "interaction": { "id": "v1_scripted", "status": status,
                                  "usage": { "total_tokens": turn.input_tokens + turn.output_tokens,
                                             "total_input_tokens": turn.input_tokens,
                                             "total_cached_tokens": 0,
                                             "total_output_tokens": turn.output_tokens,
                                             "total_tool_use_tokens": 0,
                                             "total_thought_tokens": 0 },
                                  "object": "interaction", "model": "test-model" },
                 "event_type": "interaction.completed" }),
    ));
    stream.push_str("event: done\ndata: [DONE]\n\n");
    stream
}

// ------------------------------------------------------------- the server

/// One request the server received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub path: String,
    pub body: String,
    /// The value of the `Authorization` header, if the request had one.
    pub authorization: Option<String>,
}

impl Recorded {
    /// The body as JSON.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the request body is JSON")
    }
}

/// The scripted provider.
pub struct FakeServer {
    wire: Wire,
    address: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FakeServer {
    /// A provider that answers one request with each response, in order, and
    /// then answers nothing more.
    pub fn start(turns: Vec<Turn>) -> FakeServer {
        FakeServer::serve(Wire::Anthropic, turns, false)
    }

    /// As [`FakeServer::start`], speaking the given wire format. The loop
    /// then reaches it through that provider's adapter.
    pub fn start_on(wire: Wire, turns: Vec<Turn>) -> FakeServer {
        FakeServer::serve(wire, turns, false)
    }

    /// A provider that goes on giving its last response for ever: a model
    /// that never stops.
    pub fn repeating(turns: Vec<Turn>) -> FakeServer {
        FakeServer::serve(Wire::Anthropic, turns, true)
    }

    fn serve(wire: Wire, turns: Vec<Turn>, repeat_last: bool) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let address = listener.local_addr().expect("the local address");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let bodies: Vec<String> = turns.iter().map(|turn| wire.render(turn)).collect();
        let worker = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut next = 0usize;
                while !stop.load(Ordering::SeqCst) {
                    let body = match bodies.get(next) {
                        Some(body) => body,
                        None if repeat_last => match bodies.last() {
                            Some(body) => body,
                            None => break,
                        },
                        None => break,
                    };
                    match listener.accept() {
                        Ok((stream, _)) => {
                            answer(stream, body, &requests);
                            next += 1;
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        FakeServer {
            wire,
            address,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    /// The wire format this server speaks.
    pub fn wire(&self) -> Wire {
        self.wire
    }

    /// The base URL to configure a provider with.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The requests received so far.
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("the request log").clone()
    }

    /// The bodies of the requests received so far, as JSON.
    pub fn bodies(&self) -> Vec<Value> {
        self.requests().iter().map(Recorded::json).collect()
    }

    /// A provider configuration that points at this server.
    pub fn config(&self) -> ProviderConfig {
        self.config_with(|_| {})
    }

    /// As [`FakeServer::config`], with a change applied to the JSON first.
    pub fn config_with(&self, change: impl FnOnce(&mut Value)) -> ProviderConfig {
        let mut value = json!({
            "schema_version": 1,
            "provider": self.wire.provider(),
            "base_url": self.base_url(),
            "model": "test-model",
            "max_output_tokens": 256,
            "timeouts": { "connect_ms": 3000, "response_ms": 3000, "idle_ms": 3000 },
            "retry": { "max_attempts": 1, "initial_delay_ms": 5, "max_delay_ms": 50 }
        });
        change(&mut value);
        ProviderConfig::from_json(&value.to_string()).expect("a valid test configuration")
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn answer(mut stream: TcpStream, body: &str, requests: &Mutex<Vec<Recorded>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Some(recorded) = read_request(&mut stream) else {
        return;
    };
    requests.lock().expect("the request log").push(recorded);
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";
    let _ = stream.write_all(head.as_bytes());
    for part in body.as_bytes().chunks(512) {
        let _ = stream.write_all(format!("{:x}\r\n", part.len()).as_bytes());
        let _ = stream.write_all(part);
        let _ = stream.write_all(b"\r\n");
    }
    let _ = stream.write_all(b"0\r\n\r\n");
    let _ = stream.flush();
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut words = lines.next()?.split(' ');
    let _method = words.next()?;
    let path = words.next()?.to_owned();
    let headers: Vec<(&str, &str)> = lines.filter_map(|line| line.split_once(':')).collect();
    let header = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.trim().eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.trim().to_owned())
    };
    let length = header("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let authorization = header("authorization");
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(count) => body.extend_from_slice(&chunk[..count]),
        }
    }
    body.truncate(length);
    Some(Recorded {
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
        authorization,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The adapter for a configuration, with the test key.
pub fn provider(config: &ProviderConfig) -> Box<dyn Provider> {
    let key = ApiKey::new(TEST_KEY.to_owned()).expect("a usable test key");
    connect(config, key).expect("an adapter")
}

// ------------------------------------------------- reading what was sent

/// Every `tool_result` block of a request body, in order.
pub fn tool_results(body: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    for message in body["messages"].as_array().into_iter().flatten() {
        for block in message["content"].as_array().into_iter().flatten() {
            if block["type"] == "tool_result" {
                blocks.push(block.clone());
            }
        }
    }
    blocks
}

/// The frame P@x put around the result in a `tool_result` block.
pub fn frame(block: &Value) -> Value {
    let text = block["content"][0]["text"]
        .as_str()
        .expect("a tool result holds one text");
    let value: Value = serde_json::from_str(text).expect("the result text is JSON");
    value["tur_pax_tool_result"].clone()
}

/// The text blocks of the first user message of a request body.
pub fn first_user_texts(body: &Value) -> Vec<String> {
    body["messages"][0]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str().map(str::to_owned))
        .collect()
}

/// The frames P@x put around the tool results of a request body, in order,
/// wherever the wire format puts a tool result: a `tool_result` block
/// (Anthropic), a `function_call_output` item (OpenAI) or a `function_result`
/// step (Google).
pub fn result_frames(wire: Wire, body: &Value) -> Vec<Value> {
    let texts: Vec<String> = match wire {
        Wire::Anthropic => tool_results(body)
            .iter()
            .filter_map(|block| block["content"][0]["text"].as_str().map(str::to_owned))
            .collect(),
        Wire::OpenAi => body["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "function_call_output")
            .filter_map(|item| item["output"].as_str().map(str::to_owned))
            .collect(),
        Wire::Google => body["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|step| step["type"] == "function_result")
            .filter_map(|step| step["result"][0]["text"].as_str().map(str::to_owned))
            .collect(),
        Wire::OpenAiChat => body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| message["role"] == "tool")
            .filter_map(|message| message["content"].as_str().map(str::to_owned))
            .collect(),
    };
    texts
        .iter()
        .map(|text| {
            let value: Value = serde_json::from_str(text).expect("the result text is JSON");
            value["tur_pax_tool_result"].clone()
        })
        .collect()
}

/// The system text of a request body, where the wire format puts it.
pub fn system_of(wire: Wire, body: &Value) -> Value {
    match wire {
        Wire::Anthropic => body["system"].clone(),
        Wire::OpenAi => body["instructions"].clone(),
        Wire::Google => body["system_instruction"].clone(),
        Wire::OpenAiChat => {
            let first = &body["messages"][0];
            if first["role"] == "system" {
                first["content"].clone()
            } else {
                Value::Null
            }
        }
    }
}

/// The names of the tools a request body offers. Chat Completions nests
/// the name under `function`.
pub fn offered_tools(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            tool["name"]
                .as_str()
                .or_else(|| tool["function"]["name"].as_str())
                .map(str::to_owned)
        })
        .collect()
}

// ----------------------------------------------------------- the executor

/// An executor that records the plans it is given and runs nothing.
pub struct FakeExecutor {
    /// Every plan it was asked to run, in order.
    pub plans: Vec<SandboxPlan>,
    /// What to answer, one per run; when empty, the default.
    pub replies: VecDeque<ExecOutput>,
    /// The answer when no reply is queued.
    pub default: ExecOutput,
}

/// A successful run that printed `stdout`.
pub fn printed(stdout: &str) -> ExecOutput {
    ExecOutput {
        status: ExecStatus::Exited(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

impl FakeExecutor {
    pub fn new() -> Self {
        FakeExecutor {
            plans: Vec::new(),
            replies: VecDeque::new(),
            default: printed("{\"ok\":true}"),
        }
    }

    /// The commands it was asked to run, as argument vectors.
    pub fn commands(&self) -> Vec<Vec<String>> {
        self.plans.iter().map(|plan| plan.argv.clone()).collect()
    }
}

impl Executor for FakeExecutor {
    fn run(&mut self, plan: &SandboxPlan) -> ExecOutput {
        self.plans.push(plan.clone());
        self.replies
            .pop_front()
            .unwrap_or_else(|| self.default.clone())
    }
}

// -------------------------------------------------------------- the clock

/// A clock that moves only when it is read or slept on.
pub struct ManualClock {
    now: Cell<Duration>,
    /// How far each reading of the clock moves it.
    pub per_reading: Duration,
    /// How often the loop slept.
    pub sleeps: Cell<u32>,
}

impl ManualClock {
    pub fn new(per_reading: Duration) -> Self {
        ManualClock {
            now: Cell::new(Duration::ZERO),
            per_reading,
            sleeps: Cell::new(0),
        }
    }
}

impl TaskClock for ManualClock {
    fn elapsed(&self) -> Duration {
        let now = self.now.get() + self.per_reading;
        self.now.set(now);
        now
    }

    fn now(&self) -> Timestamp {
        Timestamp(1_800_000_000)
    }

    fn sleep(&self, duration: Duration) {
        self.sleeps.set(self.sleeps.get() + 1);
        self.now.set(self.now.get() + duration);
    }
}

// ------------------------------------------------------------ the control

/// A control that gives a fixed sequence of signals and then says to go on.
pub struct ScriptedControl {
    signals: RefCell<VecDeque<Signal>>,
    /// What it says when the sequence is used up.
    pub then: Signal,
}

impl ScriptedControl {
    pub fn new(signals: Vec<Signal>, then: Signal) -> Self {
        ScriptedControl {
            signals: RefCell::new(signals.into()),
            then,
        }
    }
}

impl Control for ScriptedControl {
    fn signal(&self) -> Signal {
        self.signals.borrow_mut().pop_front().unwrap_or(self.then)
    }
}

// -------------------------------------------------------------- the trail

/// A trail that takes a number of entries and then fails.
pub struct FailingTrail<'a> {
    pub inner: &'a mut MemoryTrail,
    pub left: usize,
}

impl FailingTrail<'_> {
    fn take(&mut self) -> Result<(), AuditWriteError> {
        if self.left == 0 {
            return Err(AuditWriteError("the test trail is full".to_owned()));
        }
        self.left -= 1;
        Ok(())
    }
}

impl Trail for FailingTrail<'_> {
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.take()?;
        self.inner.decision(record)
    }

    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError> {
        self.take()?;
        self.inner.event(event)
    }
}

// ------------------------------------------------------------ the harness

/// Everything a task loop needs, with test doubles for the machine.
pub struct Harness {
    pub registry: ToolRegistry,
    pub policy: UserPolicy,
    pub situation: Situation,
    pub limits: TaskLimits,
    pub rules: GrantRules,
    pub grants: Vec<Grant>,
    pub executor: FakeExecutor,
    pub trail: MemoryTrail,
    pub clock: ManualClock,
    pub control: Box<dyn Control>,
    pub recalled: RecalledNotes,
    /// The conversation, kept between tasks.
    pub history: Vec<Message>,
    /// If set, the trail fails after this many entries.
    pub trail_capacity: Option<usize>,
    tasks: u32,
}

impl Harness {
    /// A harness with the test service's four tools and the command tool,
    /// the default policy, an attended session and generous limits.
    pub fn new() -> Self {
        Harness::with_tools(&[TEST_MANIFEST], &[TEST_ADAPTERS])
    }

    /// A harness with the given manifests and adapters, and the command tool.
    pub fn with_tools(manifests: &[&str], adapters: &[&str]) -> Self {
        let manifests: Vec<String> = manifests.iter().map(|text| (*text).to_owned()).collect();
        let adapters: Vec<String> = adapters.iter().map(|text| (*text).to_owned()).collect();
        Harness {
            registry: ToolRegistry::load(&manifests, &adapters, true).expect("a valid tool set"),
            policy: UserPolicy::default(),
            situation: Situation::attended(),
            limits: TaskLimits::default(),
            rules: GrantRules::for_home("/home/ada"),
            grants: Vec::new(),
            executor: FakeExecutor::new(),
            trail: MemoryTrail::new(),
            clock: ManualClock::new(Duration::from_millis(1)),
            control: Box::new(NoControl),
            recalled: RecalledNotes::none(),
            history: Vec::new(),
            trail_capacity: None,
            tasks: 0,
        }
    }

    /// Run one task against a scripted provider.
    pub fn run(&mut self, server: &FakeServer, request: &str) -> TaskReport {
        self.run_configured(&server.config(), request)
    }

    /// Run one task with the given provider configuration.
    pub fn run_configured(&mut self, config: &ProviderConfig, request: &str) -> TaskReport {
        self.tasks += 1;
        let task_id = format!("task-{}", self.tasks);
        let provider = provider(config);
        let mut meter = SpendMeter::for_config(config);
        let mut observer = NoObserver;
        let mut budget = NoBudget;
        let mut assistant = NoAssistant;
        let mut failing;
        let trail: &mut dyn Trail = match self.trail_capacity {
            Some(left) => {
                failing = FailingTrail {
                    inner: &mut self.trail,
                    left,
                };
                &mut failing
            }
            None => &mut self.trail,
        };
        let mut orchestrator = Orchestrator {
            provider: &*provider,
            config,
            registry: &self.registry,
            policy: &self.policy,
            situation: self.situation,
            limits: self.limits,
            rules: &self.rules,
            grants: &self.grants,
            executor: &mut self.executor,
            trail,
            clock: &self.clock,
            control: &*self.control,
            observer: &mut observer,
            budget: &mut budget,
            assistant: &mut assistant,
        };
        let task = TaskRequest {
            task_id: &task_id,
            provider_id: None,
            request,
            recalled: &self.recalled,
        };
        orchestrator.run(&task, &mut self.history, &mut meter)
    }

    /// The values of one journal field over the decisions, in order.
    pub fn decisions(&self, field: &str) -> Vec<String> {
        self.trail
            .decisions()
            .iter()
            .map(|record| {
                record
                    .journal_fields()
                    .into_iter()
                    .find(|(name, _)| *name == field)
                    .map(|(_, value)| value)
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The values of one field over the events of a kind, in order.
    pub fn events(&self, kind: EventKind, field: &str) -> Vec<String> {
        self.trail
            .events(kind)
            .iter()
            .map(|event| event.field(field).unwrap_or("").to_owned())
            .collect()
    }

    /// The whole trail as text, to look for something that must not be in it.
    pub fn trail_text(&self) -> String {
        self.trail
            .entries()
            .iter()
            .map(|entry| entry.to_json().to_string())
            .collect::<Vec<String>>()
            .join("\n")
    }
}

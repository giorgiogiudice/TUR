// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Shared by the integration tests: a small in-process HTTP server that
//! replays scripted responses, the scripts for each provider, and helpers.
//!
//! The server uses only the standard library: one listener, one thread, one
//! connection at a time. It records every request it receives and answers each
//! with the next scripted reply. No test talks to a real provider.

#![allow(dead_code)]

pub mod anthropic;
pub mod google;
pub mod openai;

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use tur_pax_providers::{
    ApiKey, Message, Provider, ProviderConfig, ProviderId, Request, StreamEvent, connect,
};

/// The key every test uses. No test output may contain it.
pub const TEST_KEY: &str = "sk-test-KEY-7f3a9c2e5b1d4860";

/// The system text of the test requests.
pub const SYSTEM: &str = "You are P@x.";

/// One scripted reply.
pub enum Reply {
    /// A complete JSON response with a status and extra headers.
    Status {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    /// A 200 event stream, sent with chunked transfer encoding in pieces of
    /// `piece` bytes. With `cut`, the connection is closed without the final
    /// chunk, as when it drops.
    Stream {
        body: Vec<u8>,
        piece: usize,
        cut: bool,
    },
    /// Read the request, stay silent for this long, then close.
    Silence(Duration),
}

impl Reply {
    /// A complete event stream, split into small pieces.
    pub fn stream(body: &str) -> Reply {
        Reply::Stream {
            body: body.as_bytes().to_vec(),
            piece: 7,
            cut: false,
        }
    }

    /// An event stream that is cut off after `body`.
    pub fn cut(body: &str) -> Reply {
        Reply::Stream {
            body: body.as_bytes().to_vec(),
            piece: 7,
            cut: true,
        }
    }

    /// A JSON response with a status.
    pub fn status(status: u16, body: &str) -> Reply {
        Reply::Status {
            status,
            headers: Vec::new(),
            body: body.to_owned(),
        }
    }

    /// A JSON response with a status and a `retry-after` header.
    pub fn status_retry_after(status: u16, body: &str, seconds: &str) -> Reply {
        Reply::Status {
            status,
            headers: vec![("retry-after", seconds.to_owned())],
            body: body.to_owned(),
        }
    }
}

/// One request the server received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    /// The headers, with lower-case names.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    /// The value of a header.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the request body is JSON")
    }
}

/// The scripted server.
pub struct FakeServer {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FakeServer {
    /// Start a server that answers one request with each reply, in order.
    pub fn start(replies: Vec<Reply>) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let address = listener.local_addr().expect("the local address");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut pending = replies.into_iter();
                let mut current = pending.next();
                while !stop.load(Ordering::SeqCst) {
                    let Some(reply) = current.as_ref() else {
                        break;
                    };
                    match listener.accept() {
                        Ok((stream, _)) => {
                            serve(stream, reply, &requests);
                            current = pending.next();
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        FakeServer {
            address,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    /// The base URL to configure a provider with.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The requests received so far.
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("the request log").clone()
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

fn serve(mut stream: TcpStream, reply: &Reply, requests: &Mutex<Vec<Recorded>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Some(recorded) = read_request(&mut stream) else {
        return;
    };
    requests.lock().expect("the request log").push(recorded);
    match reply {
        Reply::Status {
            status,
            headers,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} Scripted\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                body.len()
            );
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
        Reply::Stream { body, piece, cut } => {
            let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";
            let _ = stream.write_all(head.as_bytes());
            for part in body.chunks((*piece).max(1)) {
                let _ = stream.write_all(format!("{:x}\r\n", part.len()).as_bytes());
                let _ = stream.write_all(part);
                let _ = stream.write_all(b"\r\n");
                let _ = stream.flush();
            }
            if !*cut {
                let _ = stream.write_all(b"0\r\n\r\n");
            }
        }
        Reply::Silence(time) => thread::sleep(*time),
    }
    let _ = stream.flush();
    // Dropping the stream closes the connection.
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
    let method = words.next()?.to_owned();
    let path = words.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(count) => body.extend_from_slice(&chunk[..count]),
        }
    }
    body.truncate(length);
    Some(Recorded {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// An error response of a provider.
pub struct ErrorReply {
    pub status: u16,
    pub body: &'static str,
}

/// What the follow-up request of the tool round trip must contain.
pub struct FollowUp {
    /// The system text.
    pub system: &'static str,
    /// The identifier of the tool call being answered.
    pub call_id: String,
    /// The untrusted tool output.
    pub untrusted: &'static str,
}

/// The scripted responses of one provider, and the checks of what it must be
/// sent. Everything else in the conformance suite is the same for all three.
pub struct Wire {
    pub provider: ProviderId,
    /// A plain answer: "Hello, wörld!", 25 input and 15 output tokens.
    pub plain: &'static str,
    /// Provider-specific reasoning content, the text "I will unmount it.",
    /// then a call of `storage_unmount` with `{"volume_id": "vol-1"}` whose
    /// arguments arrive in several fragments. 40 input and 30 output tokens.
    pub tool_call: &'static str,
    /// A call of `storage_list_volumes` with no arguments, then a call of
    /// `storage_unmount` with `{"volume_id": "vol-2"}`.
    pub two_tool_calls: &'static str,
    /// A refusal or a safety block.
    pub refusal: &'static str,
    /// The text "Hello, " and then the start of an event, to be cut off.
    pub cut_short: &'static str,
    /// The text "Hello, " and then the provider's error event for overload.
    pub overloaded_mid_stream: &'static str,
    /// The text "Hello, " and then the end at the output limit.
    pub output_limit: &'static str,
    pub rate_limited: ErrorReply,
    pub auth_failed: ErrorReply,
    pub rejected: ErrorReply,
    pub overloaded: ErrorReply,
    /// The reasoning content of `tool_call`, as it must be sent back.
    pub opaque: fn() -> Value,
    /// Checks the request of [`hello`]: where it goes, how the key is sent,
    /// and the shape of the body.
    pub check_request: fn(&Recorded),
    /// Checks the body of the request that follows the tool call.
    pub check_follow_up: fn(&Value, &FollowUp),
}

/// A configuration that points at the server, with short timeouts and delays.
pub fn config(provider: ProviderId, server: &FakeServer) -> ProviderConfig {
    config_with(provider, server, |_| {})
}

/// As [`config`], with a change applied to the JSON first.
pub fn config_with(
    provider: ProviderId,
    server: &FakeServer,
    change: impl FnOnce(&mut Value),
) -> ProviderConfig {
    let mut value = json!({
        "schema_version": 1,
        "provider": provider.as_str(),
        "base_url": server.base_url(),
        "model": "test-model",
        "max_output_tokens": 256,
        "timeouts": { "connect_ms": 3000, "response_ms": 3000, "idle_ms": 3000 },
        "retry": { "max_attempts": 3, "initial_delay_ms": 5, "max_delay_ms": 50 }
    });
    change(&mut value);
    ProviderConfig::from_json(&value.to_string()).expect("a valid test configuration")
}

/// The adapter for a configuration, with the test key.
pub fn provider(config: &ProviderConfig) -> Box<dyn Provider> {
    let key = ApiKey::new(TEST_KEY.to_owned()).expect("a usable test key");
    connect(config, key).expect("an adapter")
}

/// A request that says "Hello", with the test system text.
pub fn hello(config: &ProviderConfig) -> Request {
    config.request(
        Some(SYSTEM.to_owned()),
        vec![Message::user_text("Hello")],
        Vec::new(),
    )
}

/// Send a request and collect every event.
pub fn run(config: &ProviderConfig, request: &Request) -> Vec<StreamEvent> {
    provider(config).stream(request).collect()
}

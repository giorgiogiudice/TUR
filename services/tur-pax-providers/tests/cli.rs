// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command-line tool, run as the built binary.
//!
//! `check` validates a configuration and prints the adapter's capabilities
//! without touching the network. `--ping` sends one request, here to the fake
//! server. The key is given to the child process in an environment variable
//! and must never come back in its output.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::json;
use tur_pax_providers::ProviderId;

use common::{FakeServer, Reply, TEST_KEY};

const EXIT_OK: i32 = 0;
const EXIT_INVALID: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_PING_FAILED: i32 = 3;
const EXIT_PROBE_NOT_VERIFIED: i32 = 4;

/// The variable the tests pass the key in.
const KEY_VARIABLE: &str = "TUR_PAX_PROVIDERS_CLI_TEST_KEY";

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn assert_no_key(&self) {
        assert!(!self.stdout.contains(TEST_KEY), "{}", self.stdout);
        assert!(!self.stderr.contains(TEST_KEY), "{}", self.stderr);
    }
}

/// Run the binary. The key is in its environment only if `key` is given.
fn tool(arguments: &[&str], key: Option<&str>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tur-pax-providers"));
    command
        .args(arguments)
        .env_remove("TUR_PAX_API_KEY")
        .env_remove(KEY_VARIABLE);
    if let Some(key) = key {
        command.env(KEY_VARIABLE, key);
    }
    let output = command.output().expect("the binary runs");
    Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Write a configuration that points at the server and return its path.
fn config_file(name: &str, provider: ProviderId, server: &FakeServer) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{provider}.json"));
    let config = json!({
        "schema_version": 1,
        "provider": provider.as_str(),
        "base_url": common::base_url(provider, server),
        "model": "test-model",
        "max_output_tokens": 256,
        "timeouts": { "connect_ms": 3000, "response_ms": 3000, "idle_ms": 3000 },
        "retry": { "max_attempts": 1 }
    });
    fs::write(&path, config.to_string()).expect("the configuration file is written");
    path.to_string_lossy().into_owned()
}

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn check_validates_and_prints_capabilities_without_a_network_call() {
    for wire in common::all_wires() {
        // The server has an answer ready, and must never be asked.
        let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
        let path = config_file("check", wire.provider, &server);
        let run = tool(&["check", "--config", &path], Some(TEST_KEY));

        assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
        assert!(run.stderr.is_empty(), "{}", run.stderr);
        let control = if wire.provider.is_company_interface() {
            "native computer control: offered by the provider as"
        } else {
            "native computer control: not offered by the provider"
        };
        for expected in [
            "config: valid",
            "model: test-model",
            "max output tokens: 256",
            "capabilities:",
            "streaming: yes",
            "tool calling: yes",
            "image input: yes",
            control,
            "model discovery: no",
            "ping: not requested; no network call was made",
        ] {
            assert!(run.stdout.contains(expected), "{expected}\n{}", run.stdout);
        }
        assert!(run.stdout.contains(&format!("provider: {}", wire.provider)));
        assert!(run.stdout.contains(&server.base_url()));
        run.assert_no_key();
        assert!(server.requests().is_empty(), "check made a network call");
    }
}

#[test]
fn ping_sends_one_request_and_reports_the_result() {
    for wire in common::all_wires() {
        let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
        let path = config_file("ping", wire.provider, &server);
        let run = tool(
            &[
                "check",
                "--config",
                &path,
                "--ping",
                "--key-env",
                KEY_VARIABLE,
            ],
            Some(TEST_KEY),
        );

        assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
        assert!(
            run.stdout
                .contains("ping: ok (finish: end_turn; input tokens: 25; output tokens: 15)"),
            "{}",
            run.stdout
        );
        run.assert_no_key();

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        // The key reached the server, in a header.
        assert!(
            requests[0]
                .headers
                .iter()
                .any(|(_, value)| value.contains(TEST_KEY))
        );
        // A ping asks for little.
        let body = requests[0].json();
        let limit = body["max_tokens"]
            .as_u64()
            .or(body["max_output_tokens"].as_u64())
            .or(body["generation_config"]["max_output_tokens"].as_u64());
        assert_eq!(limit, Some(64));
    }
}

#[test]
fn a_failed_ping_reports_a_stable_code_and_never_the_key() {
    for wire in common::all_wires() {
        // The server refuses the key, and repeats it in its message.
        let body = json!({ "error": { "message": format!("bad key {TEST_KEY}") } }).to_string();
        let server = FakeServer::start(vec![Reply::status(401, &body)]);
        let path = config_file("refused", wire.provider, &server);
        let run = tool(
            &[
                "check",
                "--config",
                &path,
                "--ping",
                "--key-env",
                KEY_VARIABLE,
            ],
            Some(TEST_KEY),
        );

        assert_eq!(run.status, EXIT_PING_FAILED);
        assert!(run.stdout.contains("config: valid"));
        assert!(run.stdout.contains("ping: failed"));
        assert!(
            run.stderr.starts_with("TUR-AI-PROVIDER-001"),
            "{}",
            run.stderr
        );
        assert!(run.stderr.contains("[redacted]"), "{}", run.stderr);
        run.assert_no_key();
    }
}

#[test]
fn ping_without_a_key_fails_before_any_network_call() {
    let server = FakeServer::start(vec![Reply::stream(common::anthropic::wire().plain)]);
    let path = config_file("no-key", ProviderId::Anthropic, &server);

    let run = tool(
        &[
            "check",
            "--config",
            &path,
            "--ping",
            "--key-env",
            KEY_VARIABLE,
        ],
        None,
    );
    assert_eq!(run.status, EXIT_PING_FAILED);
    assert!(
        run.stderr.starts_with("TUR-AI-PROVIDER-012"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains(KEY_VARIABLE));

    // Without --key-env the documented default variable is read.
    let run = tool(&["check", "--config", &path, "--ping"], None);
    assert_eq!(run.status, EXIT_PING_FAILED);
    assert!(run.stderr.contains("TUR_PAX_API_KEY"), "{}", run.stderr);

    assert!(server.requests().is_empty());
}

#[test]
fn the_tool_probe_reports_whether_the_model_called_the_tool() {
    let chat = common::openai_chat::wire();
    // A model that calls the probe's tool.
    let server = FakeServer::start(vec![Reply::stream(common::openai_chat::PROBE_CALLED)]);
    let path = config_file("probe", chat.provider, &server);
    let run = tool(
        &[
            "check",
            "--config",
            &path,
            "--probe-tools",
            "--key-env",
            KEY_VARIABLE,
        ],
        Some(TEST_KEY),
    );
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(
        run.stdout
            .contains("tool probe: verified (the model called the tool)"),
        "{}",
        run.stdout
    );
    run.assert_no_key();
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body = requests[0].json();
    assert_eq!(body["tools"][0]["function"]["name"], "tur_probe");
    assert_eq!(body["max_tokens"], 512);

    // A model that answers in words, in every family.
    for wire in common::all_wires() {
        let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
        let path = config_file("probe-plain", wire.provider, &server);
        let run = tool(
            &[
                "check",
                "--config",
                &path,
                "--probe-tools",
                "--key-env",
                KEY_VARIABLE,
            ],
            Some(TEST_KEY),
        );
        assert_eq!(run.status, EXIT_PROBE_NOT_VERIFIED, "{}", run.stderr);
        assert!(
            run.stdout.contains("tool probe: not verified"),
            "{}",
            run.stdout
        );
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn a_server_that_takes_no_key_is_sent_none() {
    let chat = common::openai_chat::wire();
    let server = FakeServer::start(vec![Reply::stream(chat.plain)]);
    let path = config_file("no-key-local", chat.provider, &server);
    let run = tool(&["check", "--config", &path, "--ping", "--no-key"], None);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stdout.contains("ping: ok"), "{}", run.stdout);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].header("authorization"), None);

    // The companies' interfaces always need a key: nothing is sent.
    let server = FakeServer::start(vec![Reply::stream(common::anthropic::wire().plain)]);
    let path = config_file("no-key-company", ProviderId::Anthropic, &server);
    let run = tool(&["check", "--config", &path, "--ping", "--no-key"], None);
    assert_eq!(run.status, EXIT_PING_FAILED);
    assert!(
        run.stderr.starts_with("TUR-AI-PROVIDER-011"),
        "{}",
        run.stderr
    );
    assert!(server.requests().is_empty());
}

#[test]
fn the_example_configurations_are_checked() {
    for valid in [
        "minimal-anthropic.json",
        "full-openai.json",
        "token-limit-google.json",
        "full-openai-chat.json",
    ] {
        let run = tool(&["check", "--config", &data(valid)], None);
        assert_eq!(run.status, EXIT_OK, "{valid}: {}", run.stderr);
        assert!(run.stdout.starts_with("config: valid\n"), "{valid}");
    }
    for invalid in [
        "invalid-no-model.json",
        "invalid-plain-http.json",
        "no-such-file.json",
    ] {
        let run = tool(&["check", "--config", &data(invalid)], None);
        assert_eq!(run.status, EXIT_INVALID, "{invalid}");
        assert!(run.stdout.is_empty(), "{invalid}");
        assert!(
            run.stderr.starts_with("TUR-AI-PROVIDER-011"),
            "{invalid}: {}",
            run.stderr
        );
    }
}

#[test]
fn a_command_line_that_is_not_understood_is_a_usage_error() {
    let lists: [&[&str]; 4] = [&[], &["check"], &["frobnicate"], &["check", "--ping"]];
    for list in lists {
        let run = tool(list, None);
        assert_eq!(run.status, EXIT_USAGE, "{list:?}");
        assert!(run.stderr.contains("Usage:"), "{list:?}");
    }
    let run = tool(&["--help"], None);
    assert_eq!(run.status, EXIT_OK);
    assert!(run.stdout.contains("--ping"));
    let run = tool(&["--version"], None);
    assert_eq!(run.status, EXIT_OK);
    assert!(run.stdout.starts_with("tur-pax-providers "));
}

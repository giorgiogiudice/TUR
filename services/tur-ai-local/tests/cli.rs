// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command line, run as a program: every JSON document against
//! `schemas/ai-local-report.schema.json`, the refusals with their codes, and
//! the whole life of a model with a stand-in engine: import, verify, refuse a
//! changed file, serve on the loopback interface, status, stop, stop when
//! idle, remove.
//!
//! The stand-in engine (`tests/fixtures/fake-llama-server`, a Python
//! script) answers what the supervisor uses: `/health`, `/metrics` and a
//! trivial chat completion. It proves the supervisor, not llama-server; the
//! local-ai workflow runs the real engine with real models.

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;

fn report_schema() -> Value {
    schema("ai-local-report.schema.json")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("an address")
        .port()
}

#[test]
fn the_read_only_commands_print_documents_of_the_schema() {
    let schema = report_schema();
    let catalogue = seed_catalogue();
    let catalogue = catalogue.to_str().expect("UTF-8");
    for host in ["small-vm", "desktop-amd"] {
        let root = fixture_host(host);
        let root = root.to_str().expect("UTF-8");
        let vulkaninfo = fixture_host(host).join("vulkaninfo-summary.txt");
        let vulkaninfo_text = vulkaninfo.to_string_lossy().into_owned();
        let mut places = vec![
            "--catalogue",
            catalogue,
            "--root",
            root,
            "--engine",
            "/nonexistent/llama-server",
        ];
        if vulkaninfo.exists() {
            places.extend(["--vulkaninfo", vulkaninfo_text.as_str()]);
        }
        let commands: [&[&str]; 8] = [
            &["hardware", "--json"],
            &["models", "--json", "--catalogue-only"],
            &["recommend", "--json"],
            &["recommend", "--json", "--context", "4096"],
            &["check", "qwen3-0.6b", "--json"],
            &["check", "qwen3-4b", "--json"],
            &["status", "--json"],
            &["codes", "--json"],
        ];
        for command in commands {
            let mut arguments = command.to_vec();
            arguments.extend(places.iter().copied());
            let output = run(&arguments);
            let document = stdout_json(&output);
            assert_valid(&schema, &document, &format!("{host} {command:?}"));
        }
    }
}

#[test]
fn the_hardware_of_recorded_machines() {
    let output = run(&[
        "hardware",
        "--json",
        "--root",
        fixture_host("small-vm").to_str().expect("UTF-8"),
    ]);
    let report = stdout_json(&output);
    let hardware = &report["hardware"];
    // Recorded from the development server on 2026-11-12.
    assert_eq!(hardware["memory"]["total_bytes"], 4_009_860_u64 * 1024);
    assert_eq!(hardware["cpu"]["model"], "AMD EPYC 7601 32-Core Processor");
    assert_eq!(hardware["cpu"]["logical_cpus"], 2);
    assert_eq!(hardware["cpu"]["fast_matrix_instructions"], true);
    assert_eq!(hardware["gpus"][0]["card"], "card0");
    assert_eq!(hardware["gpus"][0]["driver"], "bochs-drm");
    assert_eq!(hardware["gpus"][0]["vendor"], "other");
    assert_eq!(hardware["gpus"].as_array().map(Vec::len), Some(1));
    assert_eq!(hardware["vulkan"]["status"], "not-checked");
    assert_eq!(hardware["usable_open_backend"], Value::Null);

    let amd = fixture_host("desktop-amd");
    let output = run(&[
        "hardware",
        "--json",
        "--root",
        amd.to_str().expect("UTF-8"),
        "--vulkaninfo",
        amd.join("vulkaninfo-summary.txt").to_str().expect("UTF-8"),
    ]);
    let report = stdout_json(&output);
    let hardware = &report["hardware"];
    assert_eq!(hardware["cpu"]["logical_cpus"], 16);
    assert_eq!(hardware["cpu"]["physical_cores"], 8);
    assert_eq!(hardware["gpus"][0]["vendor"], "amd");
    assert_eq!(hardware["gpus"][0]["driver"], "amdgpu");
    assert_eq!(hardware["gpus"][0]["vram_bytes"], 8_573_157_376_u64);
    assert_eq!(hardware["vulkan"]["devices"][0]["usable"], true);
    assert_eq!(hardware["vulkan"]["devices"][1]["device_type"], "cpu");
    assert_eq!(hardware["usable_open_backend"], true);
}

#[test]
fn the_recommendation_follows_memory_and_processor() {
    let catalogue = seed_catalogue();
    let recommend = |host: &str| -> Value {
        let output = run(&[
            "recommend",
            "--json",
            "--catalogue",
            catalogue.to_str().expect("UTF-8"),
            "--root",
            fixture_host(host).to_str().expect("UTF-8"),
        ]);
        stdout_json(&output)["recommendation"].clone()
    };
    // 4 GB, under 1 GB free: nothing fits now, and the answer says so.
    let small = recommend("small-vm");
    assert_eq!(small["model"], Value::Null);
    assert_eq!(small["code"], "TUR-AI-LOCAL-007");
    let fits: Vec<(&str, &str)> = small["fits"]
        .as_array()
        .expect("fits")
        .iter()
        .map(|fit| (fit["model"].as_str().unwrap_or(""), fit["verdict"].as_str().unwrap_or("")))
        .collect();
    assert!(fits.contains(&("qwen3-0.6b", "not-now")), "{fits:?}");
    assert!(fits.contains(&("qwen3-4b", "too-large")), "{fits:?}");
    // 32 GB, 16 processors with AVX2: the largest model that can call tools
    // within the speed budget, and a model for images.
    let big = recommend("desktop-amd");
    assert_eq!(big["code"], "TUR-AI-LOCAL-004");
    assert_eq!(big["model"], "qwen3-4b");
    assert_eq!(big["vision_model"], "qwen3-vl-2b-instruct");
    assert_eq!(big["embeddings_model"], "qwen3-embedding-0.6b");
}

#[test]
fn unknown_models_and_bad_addresses_are_refused_with_codes() {
    let catalogue = seed_catalogue();
    let catalogue = catalogue.to_str().expect("UTF-8");
    let output = run(&["check", "no-such-model", "--catalogue", catalogue]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-003"));

    // Listening where other devices can connect is refused before anything
    // else is looked at.
    for host in ["0.0.0.0", "::", "192.168.1.20"] {
        let output = run(&["serve", "qwen3-0.6b", "--host", host, "--catalogue", catalogue]);
        assert_eq!(output.status.code(), Some(1), "{host}");
        assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-016"), "{host}: {}", stderr_text(&output));
    }
    let output = run(&["serve", "qwen3-0.6b", "--host", "not-an-address", "--catalogue", catalogue]);
    assert_eq!(output.status.code(), Some(2));

    // Not downloaded.
    let dir = scratch("refusals");
    let models = dir.join("models");
    let output = run(&[
        "serve",
        "qwen3-0.6b",
        "--catalogue",
        catalogue,
        "--models-dir",
        models.to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-012"));

    // A broken catalogue.
    let broken = dir.join("broken");
    fs::create_dir_all(&broken).expect("directory");
    fs::write(broken.join("x.toml"), "schema_version = 1\n").expect("entry");
    let output = run(&["models", "--catalogue", broken.to_str().expect("UTF-8")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-002"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn import_verify_refuse_a_changed_file_and_remove() {
    let schema = report_schema();
    let dir = scratch("lifecycle");
    let catalogue = tiny_catalogue(&dir);
    let models = dir.join("models");
    let places = [
        "--catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("desktop-amd").to_str().expect("UTF-8"),
    ];
    let with = |command: &[&str]| -> Vec<String> {
        command
            .iter()
            .chain(places.iter())
            .map(|text| (*text).to_owned())
            .collect()
    };
    let run_with = |command: &[&str]| {
        let arguments = with(command);
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        run(&arguments)
    };

    // A changed byte: refused, nothing kept.
    let mut changed = TINY.to_vec();
    changed[10] ^= 0x20;
    let changed_path = dir.join("changed.gguf");
    fs::write(&changed_path, &changed).expect("file");
    let output = run_with(&["pull", "tiny-test", "--from-file", changed_path.to_str().expect("UTF-8")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-010"), "{}", stderr_text(&output));
    assert!(!models.join("tiny-test/tiny.gguf").exists());
    assert!(!models.join("tiny-test/tiny.gguf.part").exists());

    // A short file: refused before it is copied.
    let short_path = dir.join("short.gguf");
    fs::write(&short_path, &TINY[..TINY.len() - 3]).expect("file");
    let output = run_with(&["pull", "tiny-test", "--from-file", short_path.to_str().expect("UTF-8")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-009"), "{}", stderr_text(&output));

    // The right file: imported and recorded.
    let good_path = dir.join("good.gguf");
    fs::write(&good_path, TINY).expect("file");
    let output = run_with(&["pull", "tiny-test", "--json", "--from-file", good_path.to_str().expect("UTF-8")]);
    let report = stdout_json(&output);
    assert_valid(&schema, &report, "pull");
    assert_eq!(report["code"], "TUR-AI-LOCAL-023");
    assert_eq!(report["files"][0]["action"], "imported");
    assert!(good_path.exists(), "the source is copied, not moved");

    let output = run_with(&["pull", "tiny-test", "--json"]);
    let report = stdout_json(&output);
    assert_eq!(report["files"][0]["action"], "already-present");

    let output = run_with(&["verify", "tiny-test", "--json"]);
    let report = stdout_json(&output);
    assert_valid(&schema, &report, "verify");
    assert_eq!(report["files"][0]["action"], "verified");

    let output = run_with(&["models", "--json"]);
    let report = stdout_json(&output);
    assert_valid(&schema, &report, "models");
    assert_eq!(report["models"][0]["installed"], "installed");

    // Changed on disk after it was checked: verify and serve refuse it.
    fs::write(models.join("tiny-test/tiny.gguf"), &changed).expect("file");
    let output = run_with(&["verify", "tiny-test"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-025"), "{}", stderr_text(&output));
    fs::write(models.join("tiny-test/tiny.gguf"), &changed[..5]).expect("file");
    let output = run_with(&["serve", "tiny-test", "--context", "512"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-025"), "{}", stderr_text(&output));

    let output = run_with(&["remove", "tiny-test", "--json"]);
    let report = stdout_json(&output);
    assert_valid(&schema, &report, "remove");
    assert_eq!(report["code"], "TUR-AI-LOCAL-024");
    assert!(!models.join("tiny-test").exists());
    let output = run_with(&["remove", "tiny-test"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-012"));
    let _ = fs::remove_dir_all(dir);
}

/// Start `serve` with the stand-in engine; returns the child and the ready
/// event.
fn start_serving(
    dir: &Path,
    catalogue: &Path,
    models: &Path,
    port: u16,
    idle: &str,
) -> (std::process::Child, Value) {
    let runtime = dir.join("run");
    let mut child = Command::new(PROGRAM)
        .args([
            "serve",
            "tiny-test",
            "--json",
            "--context",
            "512",
            "--port",
            &port.to_string(),
            "--idle-stop",
            idle,
            "--start-timeout",
            "30",
            "--engine",
            fake_engine().to_str().expect("UTF-8"),
            "--catalogue",
            catalogue.to_str().expect("UTF-8"),
            "--models-dir",
            models.to_str().expect("UTF-8"),
            "--runtime-dir",
            runtime.to_str().expect("UTF-8"),
            "--root",
            fixture_host("desktop-amd").to_str().expect("UTF-8"),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("serve starts");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("the ready line");
    if line.trim().is_empty() {
        let mut errors = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut errors);
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("serve printed no ready event: {errors}");
    }
    // Keep reading the rest in the child's own pipe after the test reads
    // the final line.
    child.stdout = Some(reader.into_inner());
    let event: Value = serde_json::from_str(line.trim()).expect("the ready event is JSON");
    (child, event)
}

fn chat(port: u16, key: &str) -> String {
    let body = r#"{"model":"tiny-test","messages":[{"role":"user","content":"hello"}]}"#;
    let request = format!(
        "POST /v1/chat/completions HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the engine listens");
    stream.write_all(request.as_bytes()).expect("request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response");
    response
}

fn install_tiny(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let catalogue = tiny_catalogue(dir);
    let models = dir.join("models");
    let source = dir.join("tiny.gguf");
    fs::write(&source, TINY).expect("file");
    let output = run(&[
        "pull",
        "tiny-test",
        "--from-file",
        source.to_str().expect("UTF-8"),
        "--catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("desktop-amd").to_str().expect("UTF-8"),
    ]);
    assert!(output.status.success(), "{}", stderr_text(&output));
    (catalogue, models)
}

#[test]
fn serve_status_and_stop_with_a_stand_in_engine() {
    let schema = report_schema();
    let dir = scratch("serve");
    let (catalogue, models) = install_tiny(&dir);
    let runtime = dir.join("run");
    let port = free_port();
    let (mut child, ready) = start_serving(&dir, &catalogue, &models, port, "600");
    assert_valid(&schema, &ready, "ready event");
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["code"], "TUR-AI-LOCAL-026");
    let state = &ready["state"];
    assert_eq!(state["address"], "127.0.0.1");
    assert_eq!(state["port"], port);
    assert_eq!(state["base_url"], format!("http://127.0.0.1:{port}"));
    assert_eq!(state["network_allowed"], false);

    // The key file is the person's only.
    let key_file = runtime.join("tur-ai-local/api-key");
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&key_file).expect("key file").permissions().mode() & 0o777
    };
    assert_eq!(mode, 0o600);
    let key = fs::read_to_string(&key_file).expect("key").trim().to_owned();
    assert_eq!(key.len(), 64);
    // The key is never printed.
    assert!(!ready.to_string().contains(&key));

    // The engine was told to listen on the loopback address only, to stay
    // offline and to ask for the key.
    let arguments: Vec<String> = serde_json::from_str(
        &fs::read_to_string(runtime.join("tur-ai-local/fake-engine-args.json")).expect("arguments"),
    )
    .expect("JSON");
    let joined = arguments.join(" ");
    assert!(joined.contains(&format!("--host 127.0.0.1 --port {port}")), "{joined}");
    for expected in ["--offline", "--no-webui", "--metrics", "--api-key-file", "--parallel 1", "--reasoning off"] {
        assert!(joined.contains(expected), "{expected}: {joined}");
    }

    let status_args = [
        "status",
        "--json",
        "--runtime-dir",
        runtime.to_str().expect("UTF-8"),
    ];
    let status = stdout_json(&run(&status_args));
    assert_valid(&schema, &status, "status while serving");
    assert_eq!(status["running"], true);
    assert_eq!(status["code"], "TUR-AI-LOCAL-026");
    assert_eq!(status["health"], "ok");
    assert!(status["memory"]["rss_bytes"].as_u64().is_some_and(|bytes| bytes > 0));
    assert!(!status.to_string().contains(&key));

    // A second serve is refused.
    let output = run(&[
        "serve",
        "tiny-test",
        "--context",
        "512",
        "--port",
        &free_port().to_string(),
        "--engine",
        fake_engine().to_str().expect("UTF-8"),
        "--catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--runtime-dir",
        runtime.to_str().expect("UTF-8"),
        "--root",
        fixture_host("desktop-amd").to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-018"), "{}", stderr_text(&output));

    // Removing the model being served is refused.
    let output = run(&[
        "remove",
        "tiny-test",
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--runtime-dir",
        runtime.to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-018"));

    // Requests work with the key, not without it.
    assert!(chat(port, &key).starts_with("HTTP/1.0 200"));
    assert!(chat(port, "wrong").starts_with("HTTP/1.0 401"));

    let stop = stdout_json(&run(&["stop", "--json", "--runtime-dir", runtime.to_str().expect("UTF-8")]));
    assert_valid(&schema, &stop, "stop");
    assert_eq!(stop["code"], "TUR-AI-LOCAL-021");
    assert_eq!(stop["model"], "tiny-test");
    let exit = child.wait().expect("serve ends");
    assert!(exit.success());
    let mut rest = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut rest)
        .expect("the stopped line");
    let stopped: Value = serde_json::from_str(rest.trim()).expect("the stopped event is JSON");
    assert_valid(&schema, &stopped, "stopped event");
    assert_eq!(stopped["code"], "TUR-AI-LOCAL-021");
    // Nothing is left: no state, no key, nothing listening.
    assert!(!runtime.join("tur-ai-local/state.json").exists());
    assert!(!key_file.exists());
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    let status = stdout_json(&run(&status_args));
    assert_eq!(status["running"], false);
    assert_eq!(status["code"], "TUR-AI-LOCAL-019");
    let stop = stdout_json(&run(&["stop", "--json", "--runtime-dir", runtime.to_str().expect("UTF-8")]));
    assert_eq!(stop["code"], "TUR-AI-LOCAL-019");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_model_that_is_not_used_is_stopped() {
    let dir = scratch("idle");
    let (catalogue, models) = install_tiny(&dir);
    let port = free_port();
    let started = Instant::now();
    let (mut child, ready) = start_serving(&dir, &catalogue, &models, port, "5");
    assert_eq!(ready["event"], "ready");
    let key = fs::read_to_string(dir.join("run/tur-ai-local/api-key"))
        .expect("key")
        .trim()
        .to_owned();
    // A request keeps it alive.
    std::thread::sleep(Duration::from_secs(3));
    assert!(chat(port, &key).starts_with("HTTP/1.0 200"));
    let exit = child.wait().expect("serve ends by itself");
    let elapsed = started.elapsed();
    assert!(exit.success());
    let mut rest = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut rest)
        .expect("the stopped line");
    let stopped: Value = serde_json::from_str(rest.trim()).expect("the stopped event is JSON");
    assert_eq!(stopped["code"], "TUR-AI-LOCAL-020");
    // At least the idle time after the request at about three seconds.
    assert!(elapsed >= Duration::from_secs(8), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(40), "{elapsed:?}");
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    let _ = fs::remove_dir_all(dir);
}

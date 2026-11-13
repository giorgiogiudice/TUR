// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `ensure`: a model downloaded, resumed, checked and put in place in one
//! step, then served.
//!
//! The download is tested against a small HTTP server on this machine that
//! serves the tiny test model with a known digest: a whole download, one
//! that breaks half-way and goes on with a range request, a server that
//! answers a range request with the whole file, a file with the wrong
//! digest, a stop the person asked for, and a disk without room. The
//! library is told to accept plain http to the loopback address for these
//! tests only; no command offers that, and every command downloads over
//! https from the catalogue's URL (the local-ai workflow downloads a real
//! model that way).
//!
//! The command is then run as a program with the stand-in engine: a model
//! already present is served, its events follow the report schema, a
//! licence with restrictions is refused without the person's acceptance,
//! and `cancel` leaves a stop request.

mod common;

use std::cell::Cell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use common::*;
use serde_json::Value;
use tur_ai_local::AiLocalCode;
use tur_ai_local::catalogue::{Catalogue, Model};
use tur_ai_local::download::{Scheme, agent_for, part_path};
use tur_ai_local::ensure::{EnsureEvent, FetchOptions, fetch_model};
use tur_ai_local::store::{InstallState, Store};

/// How the server answers one request.
#[derive(Clone)]
enum Answer {
    /// The whole file, or the part a range request asks for.
    Honour,
    /// The first `n` bytes, announced as the whole file, then the
    /// connection closes.
    Cut(usize),
    /// The whole file with status 200, whatever was asked.
    Whole,
    /// Other bytes of the same length.
    Other,
    /// The whole file in pieces of 8 bytes, 50 ms apart.
    Slowly,
}

struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    fn start(answers: Vec<Answer>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = listener.local_addr().expect("address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let worker = thread::spawn(move || {
            for answer in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                answer_one(stream, &answer, &log);
            }
        });
        Server {
            port,
            requests,
            worker: Some(worker),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/tiny.gguf", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("log").clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Unblock an accept that waits for a request that never comes.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn answer_one(mut stream: TcpStream, answer: &Answer, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    if head.is_empty() {
        return;
    }
    log.lock().expect("log").push(head.to_ascii_lowercase());
    let range_start = head
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("range: bytes=")
                .map(|rest| rest.trim().trim_end_matches('-').to_owned())
        })
        .and_then(|start| start.parse::<usize>().ok());
    let size = TINY.len();
    let send = |stream: &mut TcpStream, status: &str, extra: &str, body: &[u8], length: usize| {
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-length: {length}\r\ncontent-type: application/octet-stream\r\n{extra}connection: close\r\n\r\n"
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
        let _ = stream.flush();
    };
    match answer {
        Answer::Honour => match range_start {
            Some(start) => send(
                &mut stream,
                "206 Partial Content",
                &format!("content-range: bytes {start}-{}/{size}\r\n", size - 1),
                &TINY[start..],
                size - start,
            ),
            None => send(&mut stream, "200 OK", "", TINY, size),
        },
        Answer::Cut(n) => send(&mut stream, "200 OK", "", &TINY[..*n], size),
        Answer::Whole => send(&mut stream, "200 OK", "", TINY, size),
        Answer::Other => {
            let mut other = TINY.to_vec();
            other[7] ^= 0x01;
            send(&mut stream, "200 OK", "", &other, size);
        }
        Answer::Slowly => {
            let head =
                format!("HTTP/1.1 200 OK\r\ncontent-length: {size}\r\nconnection: close\r\n\r\n");
            let _ = stream.write_all(head.as_bytes());
            for piece in TINY.chunks(8) {
                if stream.write_all(piece).is_err() {
                    return;
                }
                let _ = stream.flush();
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// The tiny model, pointed at the test server.
fn model_at(dir: &Path, url: &str) -> Model {
    let catalogue = Catalogue::load(&tiny_catalogue(dir)).expect("the tiny catalogue");
    let mut model = catalogue.get("tiny-test").expect("tiny-test").clone();
    model.file.url = url.to_owned();
    model
}

struct Run {
    events: Vec<EnsureEvent>,
    result: Result<(), tur_ai_local::ensure::EnsureError>,
}

fn fetch(store: &Store, model: &Model, free: Option<u64>, cancel_after_progress: bool) -> Run {
    let agent = agent_for(Scheme::AllowLoopbackHttp);
    let seen_progress = Cell::new(false);
    let free_space = move |_: &Path| free;
    let cancelled = || cancel_after_progress && seen_progress.get();
    let options = FetchOptions {
        agent: &agent,
        scheme: Scheme::AllowLoopbackHttp,
        free_space: &free_space,
        cancelled: &cancelled,
    };
    let mut events = Vec::new();
    let result = fetch_model(store, model, &options, &mut |event: EnsureEvent| {
        if event.event == "progress" {
            seen_progress.set(true);
        }
        events.push(event);
    });
    Run { events, result }
}

fn names(run: &Run) -> Vec<&'static str> {
    run.events.iter().map(|event| event.event).collect()
}

#[test]
fn a_whole_download_is_checked_put_in_place_and_not_fetched_again() {
    let dir = scratch("ensure-whole");
    let server = Server::start(vec![Answer::Honour]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    let run = fetch(&store, &model, Some(u64::MAX), false);
    run.result.as_ref().expect("downloaded");
    let events = names(&run);
    assert_eq!(events.first(), Some(&"plan"));
    assert!(events.contains(&"progress"), "{events:?}");
    assert_eq!(&events[events.len() - 2..], &["verified", "installed"]);
    let plan = &run.events[0];
    assert_eq!(plan.bytes_to_download, Some(TINY.len() as u64));
    assert_eq!(plan.bytes_done, Some(0));
    assert_eq!(plan.free_bytes, Some(u64::MAX));
    assert_eq!(
        fs::read(dir.join("models/tiny-test/tiny.gguf")).expect("file"),
        TINY
    );
    assert_eq!(store.state(&model).0, InstallState::Installed);
    assert!(!part_path(&dir.join("models/tiny-test/tiny.gguf")).exists());
    // Already there: nothing is asked of the server.
    let again = fetch(&store, &model, Some(u64::MAX), false);
    again.result.as_ref().expect("already present");
    assert_eq!(names(&again), vec!["already-present"]);
    assert_eq!(again.events[0].code, Some(AiLocalCode::Installed));
    assert_eq!(server.requests().len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_broken_download_goes_on_from_where_it_stopped() {
    let dir = scratch("ensure-resume");
    let server = Server::start(vec![Answer::Cut(20), Answer::Honour]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    let destination = dir.join("models/tiny-test/tiny.gguf");
    let first = fetch(&store, &model, Some(u64::MAX), false);
    let error = first.result.expect_err("the connection broke");
    assert_eq!(error.code, AiLocalCode::DownloadFailed);
    // What arrived is kept for the next attempt, and nothing is usable.
    assert_eq!(
        fs::metadata(part_path(&destination)).expect("part").len(),
        20
    );
    assert!(!destination.exists());
    assert_eq!(store.state(&model).0, InstallState::Absent);

    let second = fetch(&store, &model, Some(u64::MAX), false);
    second.result.as_ref().expect("resumed");
    assert_eq!(second.events[0].bytes_done, Some(20));
    assert_eq!(
        second.events[0].bytes_to_download,
        Some(TINY.len() as u64 - 20)
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].contains("range:"), "{}", requests[0]);
    assert!(requests[1].contains("range: bytes=20-"), "{}", requests[1]);
    assert_eq!(fs::read(&destination).expect("file"), TINY);
    assert_eq!(store.state(&model).0, InstallState::Installed);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_server_that_sends_the_whole_file_again_is_read_from_the_start() {
    let dir = scratch("ensure-whole-again");
    let server = Server::start(vec![Answer::Cut(30), Answer::Whole]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    assert!(fetch(&store, &model, Some(u64::MAX), false).result.is_err());
    fetch(&store, &model, Some(u64::MAX), false)
        .result
        .expect("read from the start");
    assert_eq!(
        fs::read(dir.join("models/tiny-test/tiny.gguf")).expect("file"),
        TINY
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_file_with_another_digest_is_refused_and_nothing_is_kept() {
    let dir = scratch("ensure-digest");
    let server = Server::start(vec![Answer::Other]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    let destination = dir.join("models/tiny-test/tiny.gguf");
    let run = fetch(&store, &model, Some(u64::MAX), false);
    let error = run.result.expect_err("refused");
    assert_eq!(error.code, AiLocalCode::DigestMismatch);
    assert!(!destination.exists());
    assert!(!part_path(&destination).exists());
    assert_ne!(store.state(&model).0, InstallState::Installed);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_stop_keeps_what_arrived_and_the_next_attempt_finishes() {
    let dir = scratch("ensure-cancel");
    let server = Server::start(vec![Answer::Slowly, Answer::Honour]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    let destination = dir.join("models/tiny-test/tiny.gguf");
    let run = fetch(&store, &model, Some(u64::MAX), true);
    let error = run.result.expect_err("stopped");
    assert_eq!(error.code, AiLocalCode::Cancelled);
    assert!(!destination.exists());
    assert!(part_path(&destination).exists());
    assert!(fs::metadata(part_path(&destination)).expect("part").len() > 0);
    fetch(&store, &model, Some(u64::MAX), false)
        .result
        .expect("finished");
    assert_eq!(fs::read(&destination).expect("file"), TINY);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn without_room_on_the_disk_nothing_is_downloaded() {
    let dir = scratch("ensure-disk");
    let server = Server::start(vec![Answer::Honour]);
    let model = model_at(&dir, &server.url());
    let store = Store {
        root: dir.join("models"),
    };
    let run = fetch(&store, &model, Some(1_000_000), false);
    let error = run.result.as_ref().expect_err("no room");
    assert_eq!(error.code, AiLocalCode::NoDiskSpace);
    assert!(error.detail.contains("kept free"), "{}", error.detail);
    assert_eq!(names(&run), vec!["plan"]);
    assert!(server.requests().is_empty());
    assert!(!dir.join("models/tiny-test/tiny.gguf").exists());
    // A plain http address is refused by the commands' scheme.
    assert!(!Scheme::HttpsOnly.allows(&server.url()));
    let _ = fs::remove_dir_all(dir);
}

// ------------------------------------------------------------ the command

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

fn lines(output: &std::process::Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
        .collect()
}

#[test]
fn the_command_serves_a_model_already_present_and_its_events_follow_the_schema() {
    let schema = report_schema();
    let dir = scratch("ensure-command");
    let catalogue = tiny_catalogue(&dir);
    let models = dir.join("models");
    let runtime = dir.join("run");
    let source = dir.join("tiny.gguf");
    fs::write(&source, TINY).expect("file");
    let host = fixture_host("desktop-amd");
    let places = |command: &[&str]| -> Vec<String> {
        let mut arguments: Vec<String> = command.iter().map(|text| (*text).to_owned()).collect();
        for (flag, value) in [
            ("--catalogue", catalogue.to_str().expect("UTF-8")),
            ("--models-dir", models.to_str().expect("UTF-8")),
            ("--runtime-dir", runtime.to_str().expect("UTF-8")),
            ("--root", host.to_str().expect("UTF-8")),
            ("--engine", fake_engine().to_str().expect("UTF-8")),
        ] {
            arguments.push(flag.to_owned());
            arguments.push(value.to_owned());
        }
        arguments
    };
    let run_with = |command: &[&str]| {
        let arguments = places(command);
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        run(&arguments)
    };
    let output = run_with(&[
        "pull",
        "tiny-test",
        "--from-file",
        source.to_str().expect("UTF-8"),
    ]);
    assert!(output.status.success(), "{}", stderr_text(&output));

    let port = free_port().to_string();
    let output = run_with(&[
        "ensure",
        "tiny-test",
        "--json",
        "--context",
        "512",
        "--port",
        &port,
        "--start-timeout",
        "30",
        "--idle-stop",
        "600",
    ]);
    assert!(output.status.success(), "{}", stderr_text(&output));
    let events = lines(&output);
    for event in &events {
        assert_valid(&schema, event, "ensure event");
    }
    let names: Vec<&str> = events
        .iter()
        .filter_map(|event| event["event"].as_str())
        .collect();
    assert_eq!(names, vec!["already-present", "starting", "ready"]);
    let ready = events.last().expect("ready");
    assert_eq!(ready["code"], "TUR-AI-LOCAL-026");
    assert_eq!(ready["state"]["model"], "tiny-test");
    assert_eq!(
        ready["state"]["base_url"],
        format!("http://127.0.0.1:{port}")
    );

    // Asked again: the model is running as asked, nothing is started.
    let output = run_with(&[
        "ensure",
        "tiny-test",
        "--json",
        "--context",
        "512",
        "--port",
        &port,
    ]);
    assert!(output.status.success(), "{}", stderr_text(&output));
    let names: Vec<String> = lines(&output)
        .iter()
        .filter_map(|event| event["event"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names, vec!["already-present", "ready"]);

    let output = run_with(&["stop", "--json"]);
    assert!(output.status.success(), "{}", stderr_text(&output));

    // cancel leaves a stop request in the runtime folder.
    let output = run_with(&["cancel", "--json"]);
    let report = stdout_json(&output);
    assert_valid(&schema, &report, "cancel");
    assert_eq!(report["code"], "TUR-AI-LOCAL-029");
    assert!(runtime.join("tur-ai-local/ensure-cancel").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_command_refuses_before_downloading_what_cannot_be_used() {
    let schema = report_schema();
    let dir = scratch("ensure-refusals");
    let catalogue = tiny_catalogue(&dir);
    // The same entry with a use restriction.
    let entry = catalogue.join("tiny-test.toml");
    let text = fs::read_to_string(&entry).expect("entry");
    fs::write(
        &entry,
        text.replace(
            "restrictions = \"none\"",
            "restrictions = \"Not for testing nuclear reactors.\"",
        ),
    )
    .expect("entry");
    let models = dir.join("models");
    let runtime = dir.join("run");
    let host = fixture_host("desktop-amd");
    let arguments = [
        "ensure",
        "tiny-test",
        "--json",
        "--no-serve",
        "--catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--runtime-dir",
        runtime.to_str().expect("UTF-8"),
        "--root",
        host.to_str().expect("UTF-8"),
    ];
    let output = run(&arguments);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_text(&output).starts_with("TUR-AI-LOCAL-013"),
        "{}",
        stderr_text(&output)
    );
    let events = lines(&output);
    assert_eq!(events.len(), 1);
    assert_valid(&schema, &events[0], "failed event");
    assert_eq!(events[0]["event"], "failed");
    assert_eq!(events[0]["code"], "TUR-AI-LOCAL-013");
    assert!(!models.join("tiny-test").exists());

    // A model the computer could never hold is not downloaded: the
    // recorded 4 GB virtual machine and the model of the catalogue that
    // needs most.
    let output = run(&[
        "ensure",
        "qwen3-30b-a3b",
        "--json",
        "--no-serve",
        "--catalogue",
        seed_catalogue().to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("small-vm").to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_text(&output).starts_with("TUR-AI-LOCAL-005"),
        "{}",
        stderr_text(&output)
    );
    assert!(!models.join("qwen3-30b-a3b").exists());
    let _ = fs::remove_dir_all(dir);
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The program as a person runs it, on a store of the test's own (`--root`).
//! The system's store is written as root through pkexec, which CI does not
//! run; `src/main.rs` holds the rules of who writes where.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

fn program(arguments: &[&str], stdin: Option<&str>) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tur-pax-policy"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program starts");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(text.as_bytes())
            .expect("stdin written");
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().expect("the program ends");
    Run {
        status: output.status.code().expect("an exit status"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tur-pax-policy-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("permissions");
    dir
}

fn own_uid() -> u32 {
    fs::metadata("/proc/self").expect("/proc/self").uid()
}

fn show(root: &Path) -> Value {
    let run = program(&["show", "--json", "--root", root.to_str().expect("text")], None);
    assert_eq!(run.status, 0, "{}", run.stderr);
    serde_json::from_str(&run.stdout).expect("JSON")
}

const POLICY: &str = r#"{"schema_version": 1, "mode": "observe_only", "excluded_applications": ["org.example.Bank"]}"#;

fn approvals(task: &str) -> String {
    format!(
        r#"{{"schema_version": 1, "schedules": [{{"schedule": "s-00000001", "task": "{task}", "approvals": [{{"service": "org.tur.Mail1", "tool": "mail_send", "arguments": {{"to": "a@example.org"}}}}]}}]}}"#
    )
}

#[test]
fn an_empty_store_has_no_policy_and_no_approvals() {
    let root = scratch("empty");
    let shown = show(&root);
    assert_eq!(shown["policy"], Value::Null);
    assert_eq!(shown["approvals"]["schedules"], serde_json::json!([]));
    let text = program(&["show", "--root", root.to_str().expect("text")], None);
    assert!(text.stdout.contains("policy: none"), "{}", text.stdout);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_policy_is_stored_closed_to_others_and_read_back() {
    let root = scratch("policy");
    let root_text = root.to_str().expect("text");
    let run = program(&["set-policy", "--root", root_text], Some(POLICY));
    assert_eq!(run.status, 0, "{}", run.stderr);
    let shown = show(&root);
    assert_eq!(shown["policy"]["mode"], "observe_only");
    assert_eq!(shown["policy"]["excluded_applications"][0], "org.example.Bank");

    let dir = root.join(own_uid().to_string());
    let file = dir.join("policy.json");
    assert_eq!(fs::metadata(&dir).expect("dir").mode() & 0o777, 0o750);
    assert_eq!(fs::metadata(&file).expect("file").mode() & 0o777, 0o640);
    // Nothing is left beside it.
    let names: Vec<String> = fs::read_dir(&dir)
        .expect("dir")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["policy.json"]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_document_that_is_not_accepted_changes_nothing() {
    let root = scratch("refused");
    let root_text = root.to_str().expect("text");
    assert_eq!(
        program(&["set-policy", "--root", root_text], Some(POLICY)).status,
        0
    );
    let before = show(&root);
    for bad in [
        r#"{"schema_version": 1, "mode": "normal", "mode": "observe_only"}"#,
        r#"{"schema_version": 1, "mode": "lenient"}"#,
        r#"{"schema_version": 1, "mode": "normal", "trusted_session_proven": true}"#,
        "not JSON",
        "",
    ] {
        let run = program(&["set-policy", "--root", root_text], Some(bad));
        assert_eq!(run.status, 1, "{bad}: {}", run.stderr);
        assert_eq!(show(&root), before, "{bad}");
    }
    for bad in [
        approvals("Send the weekly report").replace(
            r#"{"to": "a@example.org"}"#,
            r#"{"to": "a@example.org", "to": "b@example.org"}"#,
        ),
        approvals(""),
    ] {
        let run = program(&["set-approvals", "--root", root_text], Some(&bad));
        assert_eq!(run.status, 1, "{bad}: {}", run.stderr);
        assert_eq!(show(&root), before, "{bad}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn standing_approvals_apply_to_their_schedule_and_its_task_and_can_be_taken_away() {
    let root = scratch("approvals");
    let root_text = root.to_str().expect("text");
    let run = program(
        &["set-approvals", "--root", root_text],
        Some(&approvals("Send the weekly report")),
    );
    assert_eq!(run.status, 0, "{}", run.stderr);
    let lookup = |schedule: &str, task: &str| -> Value {
        let run = program(
            &[
                "approvals-for",
                "--schedule",
                schedule,
                "--task",
                task,
                "--root",
                root_text,
            ],
            None,
        );
        assert_eq!(run.status, 0, "{}", run.stderr);
        serde_json::from_str(&run.stdout).expect("JSON")
    };
    let found = lookup("s-00000001", "Send the weekly report");
    assert_eq!(found[0]["tool"], "mail_send");
    assert_eq!(found[0]["arguments"]["to"], "a@example.org");
    for (schedule, task) in [
        ("s-00000001", "Send the weekly report to everyone"),
        ("s-00000002", "Send the weekly report"),
    ] {
        assert_eq!(lookup(schedule, task), serde_json::json!([]), "{schedule} {task}");
    }
    let run = program(
        &["clear-approvals", "--schedule", "s-00000001", "--root", root_text],
        None,
    );
    assert_eq!(run.status, 0, "{}", run.stderr);
    assert_eq!(
        lookup("s-00000001", "Send the weekly report"),
        serde_json::json!([])
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_link_in_place_of_the_person_s_directory_is_not_written_through() {
    let root = scratch("link");
    let elsewhere = scratch("link-elsewhere");
    std::os::unix::fs::symlink(&elsewhere, root.join(own_uid().to_string())).expect("a link");
    let run = program(
        &["set-policy", "--root", root.to_str().expect("text")],
        Some(POLICY),
    );
    assert_eq!(run.status, 3, "{}", run.stderr);
    assert!(!elsewhere.join("policy.json").exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&elsewhere);
}

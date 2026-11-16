// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command line, on small synthetic packages built here: an Electron
//! application as a Windows build lays it out (a program, `resources/`
//! with `app.asar`), with and without native modules, and a Qt program as
//! the negative control. No proprietary or downloaded file is used, and
//! nothing is fetched: `detect` and `plan` only read, and the packages are
//! directories, so no archive tool runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tur_rehost::asar::Asar;

const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-rehost");

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tur-rehost-cli-{}-{}-{label}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Windows x86-64 program of `size` bytes whose bytes include `text`.
fn pe_program(size: usize, text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; size.max(8192)];
    bytes[0] = b'M';
    bytes[1] = b'Z';
    bytes[0x3c] = 0x40;
    bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
    bytes[0x44] = 0x64;
    bytes[0x45] = 0x86;
    bytes[4096..4096 + text.len()].copy_from_slice(text);
    bytes
}

/// A shared object for Windows: a PE header is all a detector reads.
fn pe_module() -> Vec<u8> {
    pe_program(8192, b"module")
}

/// An Electron application as its Windows package lays it out.
fn electron_windows(root: &Path, electron: &str, modules: &[(&str, &str, &str)]) -> PathBuf {
    let app = root.join("Example");
    std::fs::create_dir_all(app.join("resources")).unwrap();
    let user_agent = format!("Chrome/138.0.7204.100 Electron/{electron} Safari/537.36");
    std::fs::write(app.join("Example.exe"), pe_program(16384, user_agent.as_bytes())).unwrap();
    let mut files: Vec<(String, Vec<u8>)> = vec![(
        "package.json".to_owned(),
        br#"{"name":"example","productName":"Example","version":"1.2.3","main":"main.js"}"#.to_vec(),
    )];
    files.push(("main.js".to_owned(), b"require('electron')".to_vec()));
    for (name, version, _) in modules {
        files.push((
            format!("node_modules/{name}/package.json"),
            format!(r#"{{"name":"{name}","version":"{version}"}}"#).into_bytes(),
        ));
    }
    let borrowed: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    let archive = app.join("resources").join("app.asar");
    Asar::create(&archive, &borrowed).unwrap();
    let mut asar = Asar::open(&archive).unwrap();
    let module = pe_module();
    for (name, _, file) in modules {
        asar.put_unpacked(&format!("node_modules/{name}/build/Release/{file}"), &module, true)
            .unwrap();
    }
    asar.save().unwrap();
    app
}

fn run(arguments: &[&str], data: &Path) -> (i32, Value) {
    let output = Command::new(PROGRAM)
        .arg("--data-dir")
        .arg(data)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", data)
        .output()
        .expect("the built program runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let document = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "not JSON ({error}): {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().expect("an exit status"), document)
}

#[test]
fn detect_reads_the_electron_version_and_the_foreign_native_modules() {
    let scratch = Scratch::new("detect");
    let app = electron_windows(&scratch.0, "43.7.7", &[("native-reg", "1.1.1", "reg.node")]);
    let (status, document) = run(&["detect", "--json", app.to_str().unwrap()], &scratch.0);
    assert_eq!(status, 0, "{document}");
    let finding = &document["findings"][0];
    assert_eq!(finding["family"], "electron");
    assert_eq!(finding["platform"], "windows");
    assert_eq!(finding["version"], "43.7.7");
    assert_eq!(finding["verdict"], "rehostable-with-work");
    let natives = finding["natives"].as_array().unwrap();
    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0]["package"], "native-reg");
    assert_eq!(natives[0]["version"], "1.1.1");
}

#[test]
fn a_plan_names_the_pinned_runtime_and_fetches_nothing() {
    let scratch = Scratch::new("plan");
    let app = electron_windows(&scratch.0, "43.7.7", &[]);
    let (status, document) = run(
        &["plan", "--json", "--app-id", "com.example.app", app.to_str().unwrap()],
        &scratch.0,
    );
    assert_eq!(status, 0, "{document}");
    assert_eq!(document["ready"], true);
    assert_eq!(document["runtime"]["version"], "43.7.7");
    assert_eq!(document["runtime"]["codecs"], "upstream");
    let downloads = document["downloads"].as_array().unwrap();
    assert_eq!(downloads.len(), 1);
    assert_eq!(
        downloads[0]["sha256"],
        "4d0a48398c444258dbcf2f5f83b49ca5bc53583130f354e0c299dad5b22b5271"
    );
    assert!(document["runtime"]["notice"].as_str().unwrap().contains("codecs"));
    // A plan writes nothing outside its scratch space: no application, no
    // download.
    assert!(!scratch.0.join("data/apps").exists());
    assert!(!scratch.0.join("cache/downloads").exists());

    let (status, document) = run(
        &["plan", "--json", "--codecs", "open-only", app.to_str().unwrap()],
        &scratch.0,
    );
    assert_eq!(status, 0, "{document}");
    assert_eq!(document["downloads"].as_array().unwrap().len(), 2);
}

#[test]
fn a_module_that_must_be_built_needs_permission_first() {
    let scratch = Scratch::new("build");
    let app = electron_windows(&scratch.0, "37.6.1", &[("native-reg", "1.1.1", "reg.node")]);
    let (status, document) = run(&["plan", "--json", app.to_str().unwrap()], &scratch.0);
    assert_eq!(status, 4, "{document}");
    assert_eq!(document["ready"], false);
    assert_eq!(document["blockers"][0]["code"], "TUR-REHOST-NATIVE-002");

    let (status, document) = run(
        &["plan", "--json", "--allow-builds", app.to_str().unwrap()],
        &scratch.0,
    );
    assert_eq!(status, 0, "{document}");
    assert_eq!(document["needs_build_environment"], true);
    let whats: Vec<&str> = document["downloads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|download| download["what"].as_str().unwrap())
        .collect();
    assert!(whats.iter().any(|what| what.starts_with("native-reg@1.1.1")), "{whats:?}");
    assert!(whats.iter().any(|what| what.contains("headers")), "{whats:?}");
}

#[test]
fn a_runtime_tur_has_not_pinned_is_refused_not_fetched() {
    let scratch = Scratch::new("unpinned");
    let app = electron_windows(&scratch.0, "1.2.3", &[]);
    let (status, document) = run(&["plan", "--json", app.to_str().unwrap()], &scratch.0);
    assert_eq!(status, 3, "{document}");
    assert_eq!(document["blockers"][0]["code"], "TUR-REHOST-INDEX-002");
    assert!(document["downloads"].as_array().unwrap().is_empty());
}

#[test]
fn a_module_with_no_identity_is_refused_unless_accepted() {
    let scratch = Scratch::new("anonymous");
    let app = electron_windows(&scratch.0, "44.9.1", &[]);
    // A module bundled without its package.json: a dead end.
    let mut asar = Asar::open(&app.join("resources/app.asar")).unwrap();
    asar.put_unpacked("native_modules/registry.node", &pe_module(), true).unwrap();
    asar.save().unwrap();
    let (status, document) = run(&["plan", "--json", app.to_str().unwrap()], &scratch.0);
    assert_eq!(status, 3, "{document}");
    assert_eq!(document["blockers"][0]["code"], "TUR-REHOST-NATIVE-001");
    let (status, document) = run(
        &["plan", "--json", "--allow-unresolved", "file:registry.node", app.to_str().unwrap()],
        &scratch.0,
    );
    assert_eq!(status, 0, "{document}");
    assert_eq!(document["modules"][0]["accepted"], true);
}

#[test]
fn a_native_program_is_refused_with_its_reason() {
    let scratch = Scratch::new("qt");
    let app = scratch.0.join("QtThing");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("QtThing.exe"), pe_program(16384, b"qt")).unwrap();
    std::fs::write(app.join("Qt6Core.dll"), pe_module()).unwrap();
    let (status, document) = run(&["plan", "--json", app.to_str().unwrap()], &scratch.0);
    assert_eq!(status, 3, "{document}");
    assert_eq!(document["blockers"][0]["code"], "TUR-REHOST-DETECT-004");
}

#[test]
fn nothing_is_listed_and_removing_an_unknown_application_is_refused() {
    let scratch = Scratch::new("list");
    let (status, document) = run(&["list", "--json"], &scratch.0);
    assert_eq!(status, 0);
    assert_eq!(document["applications"].as_array().unwrap().len(), 0);
    let (status, document) = run(&["uninstall", "--json", "com.example.app"], &scratch.0);
    assert_eq!(status, 6);
    assert_eq!(document["error"]["code"], "TUR-REHOST-APP-004");
    let (status, document) = run(&["show", "--json", "Not An Id"], &scratch.0);
    assert_eq!(status, 1);
    assert_eq!(document["error"]["code"], "TUR-REHOST-APP-001");
}

#[test]
fn the_index_command_prints_the_pinned_index() {
    let scratch = Scratch::new("index");
    let (status, document) = run(&["index", "--json"], &scratch.0);
    assert_eq!(status, 0);
    assert_eq!(document["schema_version"], 1);
    assert!(document["electron"]["43.7.7"]["linux_x64"]["url"].is_string());
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The re-hosted route, through `tur-rehost`'s command line, used as it is
//! (`services/tur-rehost/src/main.rs`; docs/reports/exp-rehost-1.md).
//!
//! `tur-rehost` runs the Windows or macOS package of an application on the
//! Linux build of its own portable runtime (Electron): `plan` says what it
//! would download and whether the package can be re-hosted, `install`
//! downloads the runtime, lays the package's files under it and stages a
//! launcher entry, `uninstall` deletes all of it, the application's private
//! home with it. This module never calls removal without the caller having
//! first decided about that home (see `ops::remove`).

use std::path::Path;

use serde::Deserialize;

/// What the person allowed for one re-host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RehostOptions {
    /// Native modules may be built from source on this computer
    /// (`--allow-builds`).
    pub allow_builds: bool,
    /// Native modules the application may run without
    /// (`--allow-unresolved`), as `name@version` or `file:<name>`.
    pub allow_unresolved: Vec<String>,
}

impl RehostOptions {
    /// The options as `tur-rehost` takes them.
    fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        if self.allow_builds {
            arguments.push("--allow-builds".to_owned());
        }
        for module in &self.allow_unresolved {
            arguments.push("--allow-unresolved".to_owned());
            arguments.push(module.clone());
        }
        arguments
    }
}

/// `tur-rehost plan <package> --app-id <id> --json [options]`.
pub fn plan_arguments(package: &Path, app_id: &str, options: &RehostOptions) -> Vec<String> {
    let mut arguments = vec![
        "plan".to_owned(),
        package.display().to_string(),
        "--app-id".to_owned(),
        app_id.to_owned(),
        "--json".to_owned(),
    ];
    arguments.extend(options.arguments());
    arguments
}

/// `tur-rehost install <package> --app-id <id> --name <name> --json
/// [options]`.
pub fn install_arguments(
    package: &Path,
    app_id: &str,
    name: &str,
    options: &RehostOptions,
) -> Vec<String> {
    let mut arguments = vec![
        "install".to_owned(),
        package.display().to_string(),
        "--app-id".to_owned(),
        app_id.to_owned(),
        "--name".to_owned(),
        name.to_owned(),
        "--json".to_owned(),
    ];
    arguments.extend(options.arguments());
    arguments
}

/// `tur-rehost uninstall <id> --json`.
pub fn uninstall_arguments(app_id: &str) -> Vec<String> {
    vec!["uninstall".to_owned(), app_id.to_owned(), "--json".to_owned()]
}

/// `tur-rehost launch <id> --accessibility`: with the runtime's
/// accessibility tree on, so that P@x and screen readers see its windows as
/// they see a Linux application's.
pub fn launch_arguments(app_id: &str) -> Vec<String> {
    vec!["launch".to_owned(), app_id.to_owned(), "--accessibility".to_owned()]
}

/// `tur-rehost list --json`.
pub fn list_arguments() -> Vec<String> {
    vec!["list".to_owned(), "--json".to_owned()]
}

/// One thing `tur-rehost plan` would stop at, as it reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Blocker {
    /// Its stable code, `TUR-REHOST-*`.
    #[serde(default)]
    pub code: String,
    /// Its title.
    #[serde(default)]
    pub title: String,
    /// What stands in the way, in plain language.
    #[serde(default)]
    pub message: String,
}

/// One file `tur-rehost` would download, from TUR's pinned index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PlannedDownload {
    /// What it is.
    #[serde(default)]
    pub what: String,
    /// Where from.
    #[serde(default)]
    pub url: String,
    /// Its size in bytes.
    #[serde(default)]
    pub size: u64,
    /// The SHA-256 it is checked against.
    #[serde(default)]
    pub sha256: String,
    /// Its licence, when known.
    #[serde(default)]
    pub licence: Option<String>,
}

/// The runtime `tur-rehost` would lay the package under.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PlannedRuntime {
    /// `electron`.
    #[serde(default)]
    pub family: String,
    /// The exact version the package was built with.
    #[serde(default)]
    pub version: String,
    /// `upstream` or `open-only`.
    #[serde(default)]
    pub codecs: String,
    /// What the person is told about the runtime and its codecs.
    #[serde(default)]
    pub notice: String,
}

/// What `tur-rehost plan --json` says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PlanReport {
    /// Whether the package can be re-hosted as asked.
    #[serde(default)]
    pub ready: bool,
    /// What stands in the way: empty when ready.
    #[serde(default)]
    pub blockers: Vec<Blocker>,
    /// Everything that would be downloaded.
    #[serde(default)]
    pub downloads: Vec<PlannedDownload>,
    /// Their total size in bytes.
    #[serde(default)]
    pub download_bytes: u64,
    /// The runtime, when the package can be re-hosted.
    #[serde(default)]
    pub runtime: Option<PlannedRuntime>,
    /// Whether native modules would be built on this computer.
    #[serde(default)]
    pub needs_build_environment: bool,
}

/// Reads what `tur-rehost plan --json` printed, or `None` when it is not a
/// plan (an error, or nothing).
pub fn parse_plan(text: &str) -> Option<PlanReport> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    if value.get("error").is_some() || value.get("ready").is_none() {
        return None;
    }
    serde_json::from_value(value).ok()
}

/// Why `tur-rehost` refused or failed, from what it printed with `--json`:
/// its error as `code: message`, or the blockers of a plan that is not
/// ready.
pub fn parse_failure(text: &str) -> Option<String> {
    if let Some(error) = crate::runtime::parse_error(text) {
        return Some(error);
    }
    let plan = parse_plan(text)?;
    if plan.blockers.is_empty() {
        return None;
    }
    Some(
        plan.blockers
            .iter()
            .map(|blocker| format!("{}: {}", blocker.code, blocker.message))
            .collect::<Vec<String>>()
            .join("; "),
    )
}

/// What `tur-rehost install --json` reported about the application it set
/// up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// The application's version, from its package.
    pub app_version: Option<String>,
    /// The package's platform: `windows` or `macos`.
    pub platform: Option<String>,
    /// The runtime, as `electron 44.10.2`.
    pub runtime: Option<String>,
    /// The staged launcher entry.
    pub desktop_entry: Option<String>,
}

/// Reads the report `install --json` prints: `{plan, record, directory}`.
pub fn parse_install_report(text: &str) -> Installed {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return Installed::default();
    };
    let record = &value["record"];
    let runtime = match (
        record["runtime"]["family"].as_str(),
        record["runtime"]["version"].as_str(),
    ) {
        (Some(family), Some(version)) => Some(format!("{family} {version}")),
        _ => None,
    };
    Installed {
        app_version: record["app_version"].as_str().map(str::to_owned),
        platform: record["package"]["platform"].as_str().map(str::to_owned),
        runtime,
        desktop_entry: record["desktop_entry"].as_str().map(str::to_owned),
    }
}

/// One application `tur-rehost list --json` lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The identifier.
    pub app_id: String,
    /// The name.
    pub name: String,
    /// The application's version, when known.
    pub app_version: Option<String>,
    /// The runtime, as `electron 44.10.2`.
    pub runtime: String,
    /// The package's platform.
    pub platform: String,
}

/// Reads `list --json`.
pub fn parse_list(text: &str) -> Vec<Listed> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return Vec::new();
    };
    value["applications"]
        .as_array()
        .map(|applications| {
            applications
                .iter()
                .filter_map(|record| {
                    Some(Listed {
                        app_id: record["app_id"].as_str()?.to_owned(),
                        name: record["name"].as_str().unwrap_or_default().to_owned(),
                        app_version: record["app_version"].as_str().map(str::to_owned),
                        runtime: record["runtime"].as_str().unwrap_or_default().to_owned(),
                        platform: record["platform"].as_str().unwrap_or_default().to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The runtime family's name for people: `Electron` for `electron`.
pub fn runtime_name(family: &str) -> String {
    match family {
        "electron" => "Electron".to_owned(),
        "nwjs" => "NW.js".to_owned(),
        "dotnet" => ".NET".to_owned(),
        "jvm" => "Java".to_owned(),
        "godot" => "Godot".to_owned(),
        "love" => "LÖVE".to_owned(),
        "renpy" => "Ren'Py".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_lines_are_tur_rehost_s() {
        let options = RehostOptions {
            allow_builds: true,
            allow_unresolved: vec!["a@1".to_owned(), "file:b.node".to_owned()],
        };
        assert_eq!(
            plan_arguments(Path::new("/f/x.zip"), "a.b", &RehostOptions::default()),
            ["plan", "/f/x.zip", "--app-id", "a.b", "--json"]
        );
        assert_eq!(
            install_arguments(Path::new("/f/x.zip"), "a.b", "B", &options),
            [
                "install",
                "/f/x.zip",
                "--app-id",
                "a.b",
                "--name",
                "B",
                "--json",
                "--allow-builds",
                "--allow-unresolved",
                "a@1",
                "--allow-unresolved",
                "file:b.node"
            ]
        );
        assert_eq!(uninstall_arguments("a.b"), ["uninstall", "a.b", "--json"]);
        assert_eq!(launch_arguments("a.b"), ["launch", "a.b", "--accessibility"]);
        assert_eq!(list_arguments(), ["list", "--json"]);
    }

    #[test]
    fn reports_are_read() {
        let plan = parse_plan(
            r#"{"package":"x.zip","app_id":"a.b","ready":false,"blockers":[{"code":"TUR-REHOST-APP-004","title":"t","message":"builds are not allowed"}],"downloads":[{"what":"Electron 44.10.2","url":"https://e/x.zip","size":10,"sha256":"00","licence":"MIT"}],"download_bytes":10,"runtime":{"family":"electron","version":"44.10.2","abi":140,"codecs":"upstream","notice":"n"},"modules":[],"needs_build_environment":true}"#,
        )
        .unwrap();
        assert!(!plan.ready);
        assert_eq!(plan.downloads[0].size, 10);
        assert_eq!(plan.runtime.as_ref().unwrap().version, "44.10.2");
        assert!(plan.needs_build_environment);
        assert_eq!(
            parse_failure(r#"{"ready":false,"blockers":[{"code":"C","title":"t","message":"m"}]}"#)
                .as_deref(),
            Some("C: m")
        );
        assert_eq!(
            parse_failure(r#"{"error":{"code":"C","title":"t","message":"m"}}"#).as_deref(),
            Some("C: m")
        );
        assert!(parse_plan(r#"{"error":{"code":"C","title":"t","message":"m"}}"#).is_none());
        assert!(parse_plan("garbage").is_none());

        let installed = parse_install_report(
            r#"{"plan":{},"record":{"app_id":"a.b","name":"B","app_version":"1.2","runtime":{"family":"electron","version":"44.10.2"},"package":{"platform":"windows"},"desktop_entry":"/d/tur-rehost-a.b.desktop"},"directory":"/d"}"#,
        );
        assert_eq!(installed.app_version.as_deref(), Some("1.2"));
        assert_eq!(installed.runtime.as_deref(), Some("electron 44.10.2"));
        assert_eq!(installed.platform.as_deref(), Some("windows"));
        assert_eq!(parse_install_report(""), Installed::default());

        let list = parse_list(
            r#"{"applications":[{"app_id":"a.b","name":"B","app_version":null,"runtime":"electron 44.10.2","platform":"windows","installed_at":"x"}]}"#,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].runtime, "electron 44.10.2");
        assert!(parse_list("").is_empty());
        assert_eq!(runtime_name("electron"), "Electron");
    }
}

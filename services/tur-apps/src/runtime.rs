// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The Windows route, through `tur-runtime`'s command line, used as it is
//! (docs/architecture/runtime-manager.md, "Installing a Windows
//! application" and "Command line").
//!
//! `tur-runtime app install` creates a prefix of the application's own,
//! runs the installer unattended inside its sandbox and records what was
//! installed; `app uninstall` removes all of it, and deletes the
//! application's private home with it. This module never calls removal
//! without the caller having first decided about that home (see
//! `ops::remove`).

use std::path::Path;

/// `tur-runtime app install <file> --app-id <id> --name <name> --json`.
pub fn install_arguments(file: &Path, app_id: &str, name: &str, portable: bool) -> Vec<String> {
    let mut arguments = vec![
        "app".to_owned(),
        "install".to_owned(),
        file.display().to_string(),
        "--app-id".to_owned(),
        app_id.to_owned(),
        "--name".to_owned(),
        name.to_owned(),
        "--json".to_owned(),
    ];
    if portable {
        arguments.push("--portable".to_owned());
    }
    arguments
}

/// `tur-runtime app uninstall <id> --json`; with `remove_backups`, the
/// prefix's backups go too.
pub fn uninstall_arguments(app_id: &str, remove_backups: bool) -> Vec<String> {
    let mut arguments = vec![
        "app".to_owned(),
        "uninstall".to_owned(),
        app_id.to_owned(),
        "--json".to_owned(),
    ];
    if remove_backups {
        arguments.push("--remove-backups".to_owned());
    }
    arguments
}

/// `tur-runtime app launch <id>`.
pub fn launch_arguments(app_id: &str) -> Vec<String> {
    vec!["app".to_owned(), "launch".to_owned(), app_id.to_owned()]
}

/// `tur-runtime app list --json`.
pub fn list_arguments() -> Vec<String> {
    vec!["app".to_owned(), "list".to_owned(), "--json".to_owned()]
}

/// What `app install --json` reported about the application it installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// The version the installer registered, when it did.
    pub version: Option<String>,
    /// The program the launcher entry starts, as a Windows path.
    pub main_executable: Option<String>,
}

/// Reads the report `app install --json` prints after a real run.
pub fn parse_install_report(text: &str) -> Installed {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return Installed::default();
    };
    let record = &value["record"];
    Installed {
        version: record["version"].as_str().map(str::to_owned),
        main_executable: record["main_executable_windows"]
            .as_str()
            .map(str::to_owned),
    }
}

/// The error `tur-runtime` printed with `--json`, as `code: message`.
pub fn parse_error(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let error = value.get("error")?;
    Some(format!(
        "{}: {}",
        error["code"].as_str().unwrap_or("?"),
        error["message"].as_str().unwrap_or("")
    ))
}

/// One application `app list --json` lists: its identifier, name and
/// version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The identifier.
    pub app_id: String,
    /// The name.
    pub name: String,
    /// The version, when known.
    pub version: Option<String>,
}

/// Reads `app list --json`.
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
                        version: record["version"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_lines_are_tur_runtime_s() {
        assert_eq!(
            install_arguments(Path::new("/c/npp.exe"), "org.example.app", "Example", false),
            [
                "app",
                "install",
                "/c/npp.exe",
                "--app-id",
                "org.example.app",
                "--name",
                "Example",
                "--json"
            ]
        );
        assert!(install_arguments(Path::new("/c/p.exe"), "a.b", "B", true)
            .contains(&"--portable".to_owned()));
        assert_eq!(
            uninstall_arguments("a.b", false),
            ["app", "uninstall", "a.b", "--json"]
        );
        assert!(uninstall_arguments("a.b", true).contains(&"--remove-backups".to_owned()));
        assert_eq!(launch_arguments("a.b"), ["app", "launch", "a.b"]);
    }

    #[test]
    fn reports_are_read() {
        let report = r#"{"dry_run":false,"record":{"app_id":"a.b","name":"Notepad++","version":"8.9.9.1","main_executable_windows":"C:\\Program Files\\Notepad++\\notepad++.exe"}}"#;
        let installed = parse_install_report(report);
        assert_eq!(installed.version.as_deref(), Some("8.9.9.1"));
        assert!(installed.main_executable.unwrap().ends_with("notepad++.exe"));
        assert_eq!(parse_install_report("garbage"), Installed::default());
        assert_eq!(
            parse_error(r#"{"error":{"code":"TUR-WINDOWS-APP-003","title":"t","message":"m"}}"#)
                .as_deref(),
            Some("TUR-WINDOWS-APP-003: m")
        );
        let list = parse_list(r#"{"applications":[{"app_id":"a.b","name":"B","version":null}],"problems":[]}"#);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].version, None);
        assert!(parse_list("").is_empty());
    }
}

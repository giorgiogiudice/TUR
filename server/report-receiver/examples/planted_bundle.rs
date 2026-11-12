// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Writes the test material of the `reports` workflow:
//!
//! ```text
//! planted_bundle OUT_DIR
//!   OUT_DIR/planted.tar.zst       a report bundle with the planted corpus,
//!                                 not redacted by a client
//!   OUT_DIR/planted-values.json   the planted values, to search for
//!   OUT_DIR/unsigned.tar.zst      a bundle without report.json
//!   OUT_DIR/technical.tar.zst     a report made with the technical profile
//!   OUT_DIR/duplicate-a.tar.zst   two reports with the same signature
//!   OUT_DIR/duplicate-b.tar.zst
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use tur_diag::Profile;
use tur_report_receiver::testing;

fn report(title: &str, code: &str, component: &str) -> String {
    serde_json::json!({
        "schema_version": 1,
        "kind": "request",
        "title": title,
        "description": "Written by the reports workflow.",
        "error_code": code,
        "component": component,
        "affected_version": null,
        "prepared_by": "person",
        "boot_health": null
    })
    .to_string()
}

fn main() -> ExitCode {
    let Some(directory) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: planted_bundle OUT_DIR");
        return ExitCode::FAILURE;
    };
    let values: Vec<serde_json::Value> = testing::PLANTED
        .iter()
        .map(|planted| serde_json::json!({ "category": planted.category, "value": planted.value }))
        .collect();
    let duplicate = report("Planted duplicate", "TUR-STORAGE-PLANT-001", "storage");
    let technical = report("Technical", "TUR-STORAGE-PLANT-002", "storage");
    let files = [
        ("planted.tar.zst", testing::planted_bundle()),
        (
            "planted-values.json",
            serde_json::to_vec_pretty(&values).unwrap_or_default(),
        ),
        (
            "unsigned.tar.zst",
            testing::bundle(
                None,
                &[("system/kernel.txt", "Linux x 6.17\n")],
                Profile::Default,
            ),
        ),
        (
            "technical.tar.zst",
            testing::bundle(Some(&technical), &[], Profile::Technical),
        ),
        (
            "duplicate-a.tar.zst",
            testing::bundle(
                Some(&duplicate),
                &[("system/kernel.txt", "a\n")],
                Profile::Default,
            ),
        ),
        (
            "duplicate-b.tar.zst",
            testing::bundle(
                Some(&duplicate),
                &[("system/kernel.txt", "b\n")],
                Profile::Default,
            ),
        ),
    ];
    for (name, data) in files {
        let path = directory.join(name);
        if let Err(error) = std::fs::write(&path, data) {
            eprintln!("{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

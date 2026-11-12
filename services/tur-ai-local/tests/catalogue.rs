// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The seed catalogue: every entry loads, follows
//! `schemas/local-model.schema.json`, names its publisher's repository
//! pinned to a revision, states its licence in plain words, and claims a
//! measured result only with a report that exists.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::*;
use serde_json::Value;
use tur_ai_local::catalogue::{Capability, Catalogue};

fn seed() -> Catalogue {
    Catalogue::load(&seed_catalogue()).unwrap_or_else(|problems| {
        panic!("the seed catalogue is not valid:\n{}", problems.join("\n"))
    })
}

#[test]
fn the_seed_catalogue_loads_and_follows_the_schema() {
    let catalogue = seed();
    let schema = schema("local-model.schema.json");
    let mut files: Vec<_> = fs::read_dir(seed_catalogue())
        .expect("the catalogue directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    assert_eq!(files.len(), catalogue.models.len());
    for path in files {
        let text = fs::read_to_string(&path).expect("readable");
        assert!(
            text.starts_with("# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"),
            "{}",
            path.display()
        );
        let document: Value = toml::from_str(&text).expect("TOML");
        assert_valid(&schema, &document, &path.display().to_string());
    }
}

#[test]
fn three_to_five_small_permissive_models_with_tools_and_vision() {
    let catalogue = seed();
    let count = catalogue.models.len();
    assert!((3..=5).contains(&count), "{count} models");
    assert!(
        catalogue
            .models
            .iter()
            .any(|model| model.can(Capability::Tools))
    );
    assert!(
        catalogue
            .models
            .iter()
            .any(|model| model.can(Capability::Vision))
    );
    for model in &catalogue.models {
        // Small enough for a processor: no file of the first list is larger
        // than 3 GB.
        assert!(model.download_bytes() <= 3_000_000_000, "{}", model.id);
        // Every licence of the first list is permissive and restricts no
        // use; a restricted licence would be listed with its restrictions.
        assert!(
            ["Apache-2.0", "MIT"].contains(&model.licence.id.as_str()),
            "{}: {}",
            model.id,
            model.licence.id
        );
        assert_eq!(model.licence.restrictions, "none", "{}", model.id);
        assert!(!model.licence.restricts_use());
    }
}

#[test]
fn every_file_comes_from_its_publisher_pinned_to_a_revision() {
    let catalogue = seed();
    let mut names = BTreeSet::new();
    for model in &catalogue.models {
        assert!(
            model.repository.starts_with("https://huggingface.co/"),
            "{}",
            model.id
        );
        for file in model.files() {
            let expected = format!(
                "{}/resolve/{}/{}",
                model.repository, model.revision, file.name
            );
            assert_eq!(file.url, expected, "{}", model.id);
            assert!(names.insert(file.name.clone()), "{} named twice", file.name);
        }
        if model.licence.url.contains("/blob/") {
            assert!(
                model.licence.url.contains(&model.revision),
                "{}: a licence file is linked at the pinned revision",
                model.id
            );
        }
    }
}

#[test]
fn a_measured_result_has_its_report() {
    let catalogue = seed();
    for model in &catalogue.models {
        for tested in &model.tested {
            let report = repository().join(&tested.report);
            assert!(
                report.is_file(),
                "{}: {} does not exist",
                model.id,
                tested.report
            );
            let text = fs::read_to_string(&report).expect("readable");
            assert!(
                text.contains(&model.id),
                "{}: the report {} does not name the model",
                model.id,
                tested.report
            );
            assert!(
                tested
                    .run
                    .starts_with("https://github.com/giorgiogiudice/TUR/actions/runs/"),
                "{}",
                model.id
            );
        }
    }
}

#[test]
fn the_schema_refuses_what_the_loader_refuses() {
    let schema = schema("local-model.schema.json");
    let text = fs::read_to_string(seed_catalogue().join("qwen3-0.6b.toml")).expect("entry");
    let document: Value = toml::from_str(&text).expect("TOML");
    assert!(schema_errors(&schema, &document).is_empty());
    let mut unknown = document.clone();
    unknown["secret_flag"] = Value::Bool(true);
    assert!(!schema_errors(&schema, &unknown).is_empty());
    let mut unpinned = document.clone();
    unpinned["revision"] = Value::String("main".to_owned());
    assert!(!schema_errors(&schema, &unpinned).is_empty());
    let mut plain_http = document.clone();
    plain_http["file"]["url"] = Value::String("http://example.org/x.gguf".to_owned());
    assert!(!schema_errors(&schema, &plain_http).is_empty());
    let mut bad_digest = document;
    bad_digest["file"]["sha256"] = Value::String("ABC".to_owned());
    assert!(!schema_errors(&schema, &bad_digest).is_empty());
}

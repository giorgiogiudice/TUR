// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The list of models of each family: the request each sends, how the key
//! goes, every page read, and what is made of a list that is not one.
//!
//! The answers are written by hand from each provider's documentation of
//! its list (pax-providers.md, section 15.4); no test talks to a provider.

mod common;

use tur_pax_providers::{
    ApiKey, ErrorKind, ListedModel, ProviderId, list_models,
};

use common::{FakeServer, Reply, TEST_KEY, config};

fn key() -> ApiKey {
    ApiKey::new(TEST_KEY.to_owned()).expect("a usable test key")
}

fn names(models: &[ListedModel]) -> Vec<&str> {
    models.iter().map(|listed| listed.model.as_str()).collect()
}

#[test]
fn anthropic_reads_every_page_with_its_key_and_version() {
    let server = FakeServer::start(vec![
        Reply::status(
            200,
            r#"{"data": [{"type": "model", "id": "model-one", "display_name": "Model One"}],
                "has_more": true, "first_id": "model-one", "last_id": "model-one"}"#,
        ),
        Reply::status(
            200,
            r#"{"data": [{"type": "model", "id": "model-two", "display_name": "Model Two"},
                         {"type": "model", "id": "model-one"}],
                "has_more": false, "first_id": "model-two", "last_id": "model-one"}"#,
        ),
    ]);
    let config = config(ProviderId::Anthropic, &server);
    let models = list_models(&config, Some(&key())).expect("a list");
    assert_eq!(names(&models), vec!["model-one", "model-two"]);
    assert_eq!(models[0].label.as_deref(), Some("Model One"));
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/models?limit=1000");
    assert_eq!(requests[1].path, "/v1/models?limit=1000&after_id=model-one");
    assert_eq!(requests[0].header("x-api-key"), Some(TEST_KEY));
    assert_eq!(requests[0].header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(requests[0].header("authorization"), None);
}

#[test]
fn openai_sends_the_key_as_a_bearer_and_reads_data() {
    let server = FakeServer::start(vec![Reply::status(
        200,
        r#"{"object": "list", "data": [{"id": "model-a", "object": "model", "owned_by": "x"},
                                      {"id": "model-b", "object": "model"}]}"#,
    )]);
    let config = config(ProviderId::OpenAi, &server);
    let models = list_models(&config, Some(&key())).expect("a list");
    assert_eq!(names(&models), vec!["model-a", "model-b"]);
    let request = &server.requests()[0];
    assert_eq!(request.path, "/v1/models");
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TEST_KEY}").as_str())
    );
}

#[test]
fn google_reads_every_page_and_takes_the_prefix_off() {
    let server = FakeServer::start(vec![
        Reply::status(
            200,
            r#"{"models": [{"name": "models/model-x", "displayName": "Model X"}],
                "nextPageToken": "page two/="}"#,
        ),
        Reply::status(200, r#"{"models": [{"name": "models/model-y"}]}"#),
    ]);
    let config = config(ProviderId::Google, &server);
    let models = list_models(&config, Some(&key())).expect("a list");
    assert_eq!(names(&models), vec!["model-x", "model-y"]);
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1beta/models?pageSize=1000");
    assert_eq!(
        requests[1].path,
        "/v1beta/models?pageSize=1000&pageToken=page%20two%2F%3D"
    );
    assert_eq!(requests[0].header("x-goog-api-key"), Some(TEST_KEY));
}

#[test]
fn a_compatible_service_is_asked_at_its_base_url_with_or_without_a_key() {
    let server = FakeServer::start(vec![
        Reply::status(200, r#"{"object": "list", "data": [{"id": "local-model"}]}"#),
        // Mistral's documentation shows a bare list in its example.
        Reply::status(200, r#"[{"id": "bare-one"}, {"id": "bare-two"}]"#),
    ]);
    let config = config(ProviderId::OpenAiChat, &server);
    let models = list_models(&config, None).expect("a list without a key");
    assert_eq!(names(&models), vec!["local-model"]);
    let models = list_models(&config, Some(&key())).expect("a bare list");
    assert_eq!(names(&models), vec!["bare-one", "bare-two"]);
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/models");
    assert_eq!(requests[0].header("authorization"), None);
    assert_eq!(
        requests[1].header("authorization"),
        Some(format!("Bearer {TEST_KEY}").as_str())
    );
}

#[test]
fn refusals_and_answers_that_are_not_lists_are_typed_errors_without_the_key() {
    // A company's family needs a key, and nothing is sent without one.
    let server = FakeServer::start(Vec::new());
    let config = config(ProviderId::OpenAi, &server);
    let error = list_models(&config, None).expect_err("no key");
    assert_eq!(error.kind(), ErrorKind::KeyUnavailable);
    assert!(server.requests().is_empty());

    let server = FakeServer::start(vec![Reply::status(
        401,
        &format!(r#"{{"error": {{"message": "bad key {TEST_KEY}", "type": "invalid_request_error"}}}}"#),
    )]);
    let config = config(ProviderId::OpenAi, &server);
    let error = list_models(&config, Some(&key())).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::AuthenticationFailed);
    assert!(!format!("{error} {error:?}").contains(TEST_KEY));

    let server = FakeServer::start(vec![Reply::status(200, r#"{"something": "else"}"#)]);
    let config = config(ProviderId::OpenAi, &server);
    let error = list_models(&config, Some(&key())).expect_err("not a list");
    assert_eq!(error.kind(), ErrorKind::MalformedResponse);

    let server = FakeServer::start(vec![Reply::status(
        200,
        r#"{"data": [{"id": "x"}, {"id": "x"}]}"#,
    )]);
    let config = config(ProviderId::OpenAi, &server);
    // A name listed twice is kept once.
    let models = list_models(&config, Some(&key())).expect("a list");
    assert_eq!(names(&models), vec!["x"]);
}

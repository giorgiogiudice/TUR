// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Speech on this computer (`src/speech.rs`, `tur-ai-local speech`): the
//! speech catalogue and its rules, the tiers on recorded machines, the
//! recommendation, the WAVE check, and recognition and synthesis with
//! stand-in engines that record how they were called. Every JSON report is
//! held to `schemas/ai-local-speech.schema.json`.
//!
//! The stand-ins prove the command lines and the reports, not whisper.cpp or
//! Piper: the `pax-voice` workflow runs the real engines.

mod common;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tur_ai_local::hardware::{self, Hardware, Source};
use tur_ai_local::speech::{
    ENGINE_LAUNCHER, OFFERED_LANGUAGES, SpeechCatalogue, SpeechEngine, SpeechEntry, SpeechKind,
    SpeechStore, SpeechTier, install_engine, parse_speech_entry, recommend, speech_tier,
};
use tur_ai_local::store::InstallState;

use common::*;

fn shipped() -> SpeechCatalogue {
    SpeechCatalogue::load(&speech_catalogue()).unwrap_or_else(|problems| {
        panic!(
            "the speech catalogue is not valid:\n{}",
            problems.join("\n")
        )
    })
}

fn host(name: &str) -> Hardware {
    let root = fixture_host(name);
    let summary = root.join("vulkaninfo-summary.txt");
    hardware::read(&Source {
        root: root.clone(),
        vulkaninfo: summary.exists().then_some(summary),
    })
    .expect("the recorded machine")
}

fn report_schema() -> Value {
    schema("ai-local-speech.schema.json")
}

#[test]
fn the_speech_catalogue_loads_and_follows_its_schema() {
    let catalogue = shipped();
    let schema = schema("speech-model.schema.json");
    let mut files: Vec<PathBuf> = fs::read_dir(speech_catalogue())
        .expect("the speech catalogue")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .collect();
    files.sort();
    assert_eq!(files.len(), catalogue.entries.len());
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
fn every_file_is_pinned_and_every_voice_is_free_to_use() {
    let catalogue = shipped();
    for entry in &catalogue.entries {
        assert_eq!(entry.licence.restrictions, "none", "{}", entry.id);
        if entry.kind == SpeechKind::Engine {
            // An engine comes from the Python Package Index, pinned by its
            // digest, with its source archive pinned for the GPL's offer.
            for file in &entry.files {
                assert!(
                    file.url.starts_with("https://files.pythonhosted.org/"),
                    "{}",
                    file.url
                );
                assert_eq!(file.sha256.len(), 64, "{}", file.name);
            }
            let source = entry.source_url.as_deref().expect("a source archive");
            assert!(source.starts_with("https://files.pythonhosted.org/"), "{source}");
            assert!(source.ends_with(".tar.gz"), "{source}");
            assert_eq!(entry.source_sha256.as_deref().map(str::len), Some(64));
            continue;
        }
        assert!(entry.source_url.is_none(), "{}", entry.id);
        assert_eq!(entry.revision.len(), 40, "{}", entry.id);
        for file in &entry.files {
            assert!(file.url.starts_with("https://huggingface.co/"), "{}", file.url);
            assert!(
                file.url.contains(&format!("/resolve/{}/", entry.revision)),
                "{} is not pinned",
                file.url
            );
            assert_eq!(file.sha256.len(), 64, "{}", file.name);
            assert!(file.size_bytes > 0, "{}", file.name);
        }
        // Nothing offered for P@x's voice restricts use: a voice is
        // offered only when its whole training chain is permissive.
        assert_eq!(entry.licence.restrictions, "none", "{}", entry.id);
    }
    // Research-only or unclear voices are not offered.
    for left_out in ["lessac", "libritts", "amy", "riccardo"] {
        assert!(
            catalogue
                .entries
                .iter()
                .all(|entry| entry.files.iter().all(|file| !file.name.contains(left_out))),
            "{left_out} is offered"
        );
    }
}

#[test]
fn english_and_italian_and_every_voiced_language_have_a_default_voice() {
    let catalogue = shipped();
    for language in ["en", "it"] {
        let voice = catalogue
            .default_voice(language)
            .unwrap_or_else(|| panic!("no default voice for {language}"));
        assert_eq!(voice.kind, SpeechKind::Synthesis);
    }
    for language in OFFERED_LANGUAGES {
        assert!(
            catalogue.default_voice(language).is_some(),
            "no default voice for {language}"
        );
    }
    for voice in catalogue.of_kind(SpeechKind::Synthesis) {
        for language in &voice.languages {
            assert!(
                catalogue.default_voice(language).is_some(),
                "{} speaks {language}, which has no default voice",
                voice.id
            );
        }
    }
    assert_eq!(
        catalogue.default_voice("en").map(|voice| voice.id.as_str()),
        Some("voice-en-us-ljspeech-medium")
    );
    assert_eq!(
        catalogue.default_voice("it").map(|voice| voice.id.as_str()),
        Some("voice-it-paola-medium")
    );
    // Every recognition model recognises Italian, and finds the language
    // itself when asked.
    for model in catalogue.of_kind(SpeechKind::Recognition) {
        assert!(model.offers_language("it"), "{}", model.id);
        assert!(model.offers_language("auto"), "{}", model.id);
    }
}

#[test]
fn tiers_on_the_recorded_machines() {
    let catalogue = shipped();
    let entry = |id: &str| -> SpeechEntry {
        catalogue
            .get(id)
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };

    // The 2-processor, 4 GB virtual machine: only the smallest model
    // answers within 2.5 s; the free tier's base model is slow there.
    let small = host("small-vm");
    let tiny = speech_tier(&entry("whisper-tiny-q5-1"), &small);
    assert_eq!(tiny.tier, SpeechTier::RunsWell, "{:?}", tiny.reasons);
    assert_eq!(tiny.expected_ms, Some(2480));
    let base = speech_tier(&entry("whisper-base-q5-1"), &small);
    assert_eq!(base.tier, SpeechTier::Slow);
    assert_eq!(base.expected_ms, Some(5920));
    let chosen = recommend(&catalogue, &small, Some("it"));
    let (recognition, judgement) = chosen.recognition.expect("a model");
    assert_eq!(recognition.id, "whisper-tiny-q5-1");
    assert_eq!(judgement.tier, SpeechTier::RunsWell);
    assert_eq!(
        chosen
            .voices
            .iter()
            .map(|(language, voice)| (language.as_str(), voice.id.as_str()))
            .collect::<Vec<_>>(),
        [("it", "voice-it-paola-medium")]
    );
    assert_eq!(
        chosen.vad.map(|vad| vad.id.as_str()),
        Some("silero-vad-v6.2.0")
    );

    // The 32 GB, 16-processor desktop with a graphics card TUR can use:
    // the best model runs well.
    let desktop = host("desktop-amd");
    let turbo = speech_tier(&entry("whisper-large-v3-turbo-q5-0"), &desktop);
    assert_eq!(turbo.tier, SpeechTier::RunsWell, "{:?}", turbo.reasons);
    assert_eq!(turbo.expected_ms, Some(1731));
    let chosen = recommend(&catalogue, &desktop, None);
    assert_eq!(
        chosen.recognition.map(|(model, _)| model.id.as_str()),
        Some("whisper-large-v3-turbo-q5-0")
    );
    assert_eq!(chosen.voices.len(), OFFERED_LANGUAGES.len());

    // Every voice runs well on both.
    for voice in catalogue.of_kind(SpeechKind::Synthesis) {
        assert_eq!(speech_tier(voice, &small).tier, SpeechTier::RunsWell);
        assert_eq!(speech_tier(voice, &desktop).tier, SpeechTier::RunsWell);
    }
}

#[test]
fn reports_of_the_catalogue_follow_the_schema() {
    let schema = report_schema();
    let catalogue = speech_catalogue();
    let catalogue = catalogue.to_str().expect("UTF-8");
    for name in ["small-vm", "desktop-amd"] {
        let root = fixture_host(name);
        let root = root.to_str().expect("UTF-8");
        for command in [
            vec!["speech", "models", "--json"],
            vec!["speech", "models", "--json", "--language", "it"],
            vec!["speech", "recommend", "--json"],
            vec!["speech", "recommend", "--json", "--language", "en"],
        ] {
            let mut arguments = command.clone();
            arguments.extend(["--speech-catalogue", catalogue, "--root", root]);
            let document = stdout_json(&run(&arguments));
            assert_valid(&schema, &document, &format!("{name} {command:?}"));
        }
    }
    let root = fixture_host("small-vm");
    let document = stdout_json(&run(&[
        "speech",
        "recommend",
        "--json",
        "--language",
        "it",
        "--speech-catalogue",
        catalogue,
        "--root",
        root.to_str().expect("UTF-8"),
    ]));
    assert_eq!(document["free"], true);
    assert_eq!(document["recognition"]["id"], "whisper-tiny-q5-1");
    assert_eq!(document["voices"][0]["id"], "voice-it-paola-medium");
    // Nothing is downloaded yet, so everything is to be downloaded: the
    // model, the detector, the voice and Piper, which the voice needs.
    let expected = 32_152_673_u64 + 885_098 + 63_221_984 + 5_457 + 34_131_442;
    assert_eq!(document["download_bytes_needed"], expected);
    assert_eq!(document["engines"][0]["id"], "piper-tts-1-8-0");
    assert_eq!(document["engines"][0]["installed"], false);
    assert_eq!(document["engines"][0]["licence"]["id"], "GPL-3.0-or-later");
    // Tiny is weak in Italian: the report says so, and names base, the
    // smallest model that is not.
    assert_eq!(document["recognition"]["weak_in"], serde_json::json!(["it"]));
    let warning = document["recognition"]["warning"]
        .as_str()
        .expect("a warning");
    assert!(warning.contains("Italian poorly"), "{warning}");
    assert!(warning.contains("Whisper base"), "{warning}");
    assert!(warning.contains("slow on this computer"), "{warning}");
    // Every other model this computer can hold, the best in Italian first:
    // Parakeet, offered and never recommended, with its credit.
    let alternatives = document["alternatives"].as_array().expect("alternatives");
    assert_eq!(alternatives[0]["id"], "parakeet-tdt-0-6b-v3-q4-k");
    assert_eq!(alternatives[0]["engine"], "parakeet");
    assert!(
        alternatives[0]["credit"]
            .as_str()
            .is_some_and(|credit| credit.contains("CC BY 4.0") && credit.contains("NVIDIA"))
    );
    let ids: Vec<&str> = alternatives
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert!(ids.contains(&"whisper-base-q5-1"), "{ids:?}");
    assert!(!ids.contains(&"whisper-tiny-q5-1"), "{ids:?}");
    // In English the same model is not said to be weak.
    let document = stdout_json(&run(&[
        "speech",
        "recommend",
        "--json",
        "--language",
        "en",
        "--speech-catalogue",
        catalogue,
        "--root",
        root.to_str().expect("UTF-8"),
    ]));
    assert_eq!(document["recognition"]["warning"], Value::Null);
}

#[test]
fn parakeet_is_offered_with_its_credit_and_never_recommended() {
    let catalogue = shipped();
    let parakeet = catalogue
        .get("parakeet-tdt-0-6b-v3-q4-k")
        .expect("Parakeet is offered");
    assert_eq!(parakeet.kind, SpeechKind::Recognition);
    assert_eq!(parakeet.engine, SpeechEngine::Parakeet);
    assert_eq!(parakeet.licence.id, "CC-BY-4.0");
    assert!(parakeet.needs_credit());
    assert!(parakeet.credit.contains("https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3"));
    assert_eq!(parakeet.languages.len(), 25);
    for language in ["it", "en", "de", "es", "fr"] {
        assert!(parakeet.offers_language(language), "{language}");
    }
    assert!(parakeet.weak_in().is_empty());
    for name in ["small-vm", "desktop-amd"] {
        let hardware = host(name);
        for language in [None, Some("it"), Some("en")] {
            let chosen = recommend(&catalogue, &hardware, language);
            assert_ne!(
                chosen.recognition.map(|(model, _)| model.id.as_str()),
                Some("parakeet-tdt-0-6b-v3-q4-k"),
                "{name} {language:?}"
            );
        }
    }
    let tiny = catalogue.get("whisper-tiny-q5-1").expect("tiny");
    assert_eq!(tiny.weak_in(), ["it"]);
    let base = catalogue.get("whisper-base-q5-1").expect("base");
    assert!(base.weak_in().is_empty());
}

const ENGINE_SOURCE: &str = "source_url = \"https://files.pythonhosted.org/packages/00/aa/x-1.0.tar.gz\"\nsource_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n";

fn engine_entry(wheel: &[u8], source: &str) -> String {
    format!(
        "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\nschema_version = 1\nid = \"tiny-engine\"\nkind = \"engine\"\nname = \"tiny-engine\"\npublisher = \"TUR tests\"\nengine = \"piper\"\nrepository = \"https://pypi.org/project/x/\"\nrevision = \"1.0\"\nparameters = \"0\"\nlanguages = [\"mul\"]\nmemory_bytes = 1000000\nspeed_basis = \"A test entry.\"\nnote = \"Tests.\"\ncredit = \"none\"\nchecked = \"2026-11-16\"\n{source}[[files]]\nname = \"x-1.0-py3-none-any.whl\"\nurl = \"https://files.pythonhosted.org/packages/00/bb/x-1.0-py3-none-any.whl\"\nsize_bytes = {}\nsha256 = \"{}\"\n{LICENCE}",
        wheel.len(),
        digest(wheel)
    )
}

#[test]
fn an_engine_is_pinned_by_its_digest_and_names_its_source_archive() {
    let wheel = b"PK a stand-in for a wheel, for tests only";
    let entry = parse_speech_entry("tiny-engine.toml", &engine_entry(wheel, ENGINE_SOURCE))
        .unwrap_or_else(|problems| panic!("{}", problems.join("\n")));
    assert_eq!(entry.kind, SpeechKind::Engine);
    // Without its source archive, an engine is refused.
    let problems = parse_speech_entry("tiny-engine.toml", &engine_entry(wheel, ""))
        .expect_err("refused");
    assert!(problems.iter().any(|problem| problem.contains("source")), "{problems:?}");
    // From anywhere but the Python Package Index, it is refused.
    let elsewhere = engine_entry(wheel, ENGINE_SOURCE).replace(
        "https://files.pythonhosted.org/packages/00/bb/",
        "https://example.org/",
    );
    assert!(parse_speech_entry("tiny-engine.toml", &elsewhere).is_err());
    // A model with a source archive is refused: only an engine has one.
    let model = format!(
        "{}{ENGINE_SOURCE}{}{LICENCE}",
        header(
            "tiny-whisper",
            "recognition",
            "whisper.cpp",
            "[\"mul\"]",
            "encode_ms_8_threads = 10\n"
        ),
        file_table("ggml-tiny-test.bin", MODEL_BYTES)
    );
    assert!(parse_speech_entry("tiny-whisper.toml", &model).is_err());
    // The shipped Piper engine is the one every recommended voice needs.
    let catalogue = shipped();
    let piper = catalogue.voice_engine().expect("Piper is in the catalogue");
    assert_eq!(piper.id, "piper-tts-1-8-0");
    assert_eq!(piper.files[0].size_bytes, 34_131_442);
    assert_eq!(
        piper.files[0].sha256,
        "25b4d3f31ff70c8fa7151908e00aaa5650cbdf16bca8fcf21299f3941b89a7d3"
    );
    assert_eq!(
        piper.source_sha256.as_deref(),
        Some("830588aded347df579c91a32703e0fc2a3685d84f1e3533b14f2de69135d4904")
    );
    assert_eq!(piper.licence.id, "GPL-3.0-or-later");
    // The image's record of the same pins says the same (image/engine/piper.lock).
    let lock = fs::read_to_string(repository().join("image/engine/piper.lock")).expect("the lock");
    let value = |key: &str| -> String {
        lock.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{key} is not in piper.lock"))
            .trim_matches('"')
            .to_owned()
    };
    assert_eq!(value("PIPER_WHEEL"), piper.files[0].name);
    assert_eq!(value("PIPER_WHEEL_URL"), piper.files[0].url);
    assert_eq!(value("PIPER_WHEEL_SHA256"), piper.files[0].sha256);
    assert_eq!(Some(value("PIPER_SOURCE_URL")), piper.source_url.clone());
    assert_eq!(Some(value("PIPER_SOURCE_SHA256")), piper.source_sha256.clone());
}

/// A stand-in for python3: makes a "virtual environment" (a copy of
/// itself), says where its packages go, "unpacks" a wheel by copying it,
/// and runs `-m piper` as the stand-in Piper.
fn fake_python(dir: &Path, piper: &Path) -> PathBuf {
    let program = dir.join("python3");
    executable(
        &program,
        &format!(
            "#!/bin/sh\nme=\"$0\"\nif [ \"$1\" = -m ] && [ \"$2\" = venv ]; then\n  for last in \"$@\"; do :; done\n  mkdir -p \"$last/bin\" && cp \"$me\" \"$last/bin/python3\" && exit 0\nfi\nif [ \"$1\" = -I ] && [ \"$2\" = -c ]; then\n  echo \"$(dirname \"$(dirname \"$me\")\")/lib/python3.14/site-packages\"; exit 0\nfi\nif [ \"$1\" = -I ] && [ \"$2\" = -m ] && [ \"$3\" = zipfile ]; then\n  mkdir -p \"$6\" && cp \"$5\" \"$6/unpacked.whl\" && exit 0\nfi\nif [ \"$1\" = -m ] && [ \"$2\" = piper ]; then\n  shift 2; exec '{}' \"$@\"\nfi\nexit 9\n",
            piper.display()
        ),
    );
    program
}

#[test]
fn piper_is_installed_on_first_use_from_its_checked_wheel_and_speaks_through_its_launcher() {
    let dir = scratch("speech-engine");
    let (catalogue, models) = tiny_speech(&dir);
    let wheel = b"PK a stand-in for a wheel, for tests only";
    fs::write(
        catalogue.join("tiny-engine.toml"),
        engine_entry(wheel, ENGINE_SOURCE),
    )
    .expect("entry");
    let loaded = SpeechCatalogue::load(&catalogue)
        .unwrap_or_else(|problems| panic!("{}", problems.join("\n")));
    let engine = loaded.voice_engine().expect("the engine").clone();
    let store = SpeechStore::in_models_folder(&models);
    let folder = store.dir(&engine.id).expect("folder");
    fs::create_dir_all(&folder).expect("folder");
    fs::write(folder.join(&engine.files[0].name), wheel).expect("the wheel");
    store.write_verified(&engine, 1).expect("record");
    // Checked but not installed: not usable.
    assert_eq!(store.state(&engine).0, InstallState::Damaged);
    assert!(store.launcher(&engine).is_none());

    let piper = fake_piper(&dir);
    let python = fake_python(&dir, &piper);
    install_engine(&python, &folder, &folder.join(&engine.files[0].name))
        .unwrap_or_else(|(code, detail)| panic!("{code}: {detail}"));
    assert!(folder.join("venv/lib/python3.14/site-packages/unpacked.whl").is_file());
    let launcher = folder.join(ENGINE_LAUNCHER);
    let script = fs::read_to_string(&launcher).expect("the launcher");
    assert!(script.contains("-m piper"), "{script}");
    assert_eq!(store.state(&engine).0, InstallState::Installed);
    assert_eq!(store.launcher(&engine), Some(launcher));

    // speech tts with no --piper speaks through the installed engine.
    let out = dir.join("answer.wav");
    let output = run_with_input(
        &[
            "speech",
            "tts",
            "--voice",
            "tiny-voice",
            "--output",
            out.to_str().expect("UTF-8"),
            "--json",
            "--speech-catalogue",
            catalogue.to_str().expect("UTF-8"),
            "--models-dir",
            models.to_str().expect("UTF-8"),
        ],
        "Ciao.".as_bytes(),
    );
    assert!(output.status.success(), "{}", stderr_text(&output));
    assert!(out.is_file());

    // Without Python, nothing is installed and the code says why.
    let refused = install_engine(Path::new("/nonexistent/python3"), &folder, &folder)
        .expect_err("refused");
    assert_eq!(refused.0.as_str(), "TUR-AI-LOCAL-030");
}

#[test]
fn without_piper_a_voice_cannot_speak_and_the_code_says_how_to_get_it() {
    let dir = scratch("speech-no-engine");
    let (catalogue, models) = tiny_speech(&dir);
    let out = dir.join("answer.wav");
    let output = run_with_input(
        &[
            "speech",
            "tts",
            "--voice",
            "tiny-voice",
            "--output",
            out.to_str().expect("UTF-8"),
            "--speech-catalogue",
            catalogue.to_str().expect("UTF-8"),
            "--models-dir",
            models.to_str().expect("UTF-8"),
        ],
        "Ciao.".as_bytes(),
    );
    assert_eq!(output.status.code(), Some(1));
    let errors = stderr_text(&output);
    assert!(errors.starts_with("TUR-AI-LOCAL-030"), "{errors}");
    assert!(errors.contains("speech ensure piper-tts-1-8-0"), "{errors}");
}

// ------------------------------------------------- stand-in engines

const MODEL_BYTES: &[u8] = b"lmgg a stand-in for a Whisper model, for tests only";
const VAD_BYTES: &[u8] = b"lmgg a stand-in for a voice detector, for tests only";
const VOICE_BYTES: &[u8] = b"a stand-in for a Piper voice, for tests only";
const CONFIG_BYTES: &[u8] = b"{\"audio\": {\"sample_rate\": 22050}}";

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

fn file_table(name: &str, bytes: &[u8]) -> String {
    format!(
        "[[files]]\nname = \"{name}\"\nurl = \"https://example.org/r/resolve/{REVISION}/{name}\"\nsize_bytes = {}\nsha256 = \"{}\"\n",
        bytes.len(),
        digest(bytes)
    )
}

const LICENCE: &str = "[licence]\nid = \"MIT\"\nurl = \"https://example.org/licence\"\nsummary = \"Use for any purpose.\"\nrestrictions = \"none\"\nevidence = \"A test entry.\"\nchecked = \"2026-11-16\"\n";

fn header(id: &str, kind: &str, engine: &str, languages: &str, extra: &str) -> String {
    format!(
        "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\nschema_version = 1\nid = \"{id}\"\nkind = \"{kind}\"\nname = \"{id}\"\npublisher = \"TUR tests\"\nengine = \"{engine}\"\nrepository = \"https://example.org/r\"\nrevision = \"{REVISION}\"\nparameters = \"0\"\nlanguages = {languages}\nmemory_bytes = 1000000\nspeed_basis = \"A test entry.\"\nnote = \"Tests.\"\ncredit = \"none\"\nchecked = \"2026-11-16\"\n{extra}"
    )
}

/// A speech catalogue of three tiny entries, installed in a models folder
/// under `dir`. Returns the catalogue and models folders.
fn tiny_speech(dir: &Path) -> (PathBuf, PathBuf) {
    let catalogue = dir.join("speech-catalogue");
    fs::create_dir_all(&catalogue).expect("catalogue");
    fs::write(
        catalogue.join("tiny-whisper.toml"),
        format!(
            "{}{}{LICENCE}",
            header(
                "tiny-whisper",
                "recognition",
                "whisper.cpp",
                "[\"mul\"]",
                "encode_ms_8_threads = 10\n"
            ),
            file_table("ggml-tiny-test.bin", MODEL_BYTES)
        ),
    )
    .expect("entry");
    fs::write(
        catalogue.join("tiny-vad.toml"),
        format!(
            "{}{}{LICENCE}",
            header("tiny-vad", "voice-activity", "whisper.cpp", "[\"mul\"]", ""),
            file_table("ggml-vad-test.bin", VAD_BYTES)
        ),
    )
    .expect("entry");
    fs::write(
        catalogue.join("tiny-voice.toml"),
        format!(
            "{}[voice]\nlocale = \"it_IT\"\nspeaker = \"test\"\nquality = \"medium\"\n\n{}{}{LICENCE}",
            header(
                "tiny-voice",
                "synthesis",
                "piper",
                "[\"it\"]",
                "default_for = [\"it\"]\nsample_rate = 22050\n"
            ),
            file_table("it_IT-test-medium.onnx", VOICE_BYTES),
            file_table("it_IT-test-medium.onnx.json", CONFIG_BYTES)
        ),
    )
    .expect("entry");
    let loaded = SpeechCatalogue::load(&catalogue)
        .unwrap_or_else(|problems| panic!("{}", problems.join("\n")));
    let models = dir.join("models");
    let store = SpeechStore::in_models_folder(&models);
    for (id, files) in [
        ("tiny-whisper", vec![("ggml-tiny-test.bin", MODEL_BYTES)]),
        ("tiny-vad", vec![("ggml-vad-test.bin", VAD_BYTES)]),
        (
            "tiny-voice",
            vec![
                ("it_IT-test-medium.onnx", VOICE_BYTES),
                ("it_IT-test-medium.onnx.json", CONFIG_BYTES),
            ],
        ),
    ] {
        let entry = loaded.get(id).expect("entry");
        let folder = store.dir(id).expect("folder");
        fs::create_dir_all(&folder).expect("folder");
        for (name, bytes) in files {
            fs::write(folder.join(name), bytes).expect("file");
        }
        store.write_verified(entry, 1).expect("record");
        assert!(store.is_installed(entry), "{id}");
    }
    (catalogue, models)
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).expect("script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// whisper-cli's stand-in: records its arguments, prints a transcript as
/// whisper-cli does with -nt -np, with a mark for silence.
fn fake_whisper(dir: &Path) -> PathBuf {
    let program = dir.join("whisper-cli");
    let record = dir.join("whisper-args.txt");
    executable(
        &program,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf ' Apri le impostazioni del suono.\\n[BLANK_AUDIO]\\n'\n",
            record.display()
        ),
    );
    program
}

/// Piper's stand-in: reads the text on standard input, records it and its
/// arguments, and writes half a second of silence at 22050 Hz to the file
/// after -f.
fn fake_piper(dir: &Path) -> PathBuf {
    let program = dir.join("piper");
    let record = dir.join("piper-call.json");
    executable(
        &program,
        &format!(
            r#"#!/usr/bin/python3
import json, struct, sys
args = sys.argv[1:]
text = sys.stdin.read()
with open("{record}", "w") as handle:
    json.dump({{"args": args, "text": text}}, handle)
out = args[args.index("-f") + 1]
samples = 11025
data = b"\x00\x00" * samples
header = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 1, 22050, 44100, 2, 16) + b"data" + struct.pack("<I", len(data))
with open(out, "wb") as handle:
    handle.write(header + data)
"#,
            record = record.display()
        ),
    );
    program
}

fn wav(path: &Path, rate: u32, seconds: f64) {
    let samples = (f64::from(rate) * seconds) as u32;
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.resize(bytes.len() + data as usize, 0);
    fs::write(path, bytes).expect("wave file");
}

fn run_with_input(arguments: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(PROGRAM)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built program runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("the text is written");
    child.wait_with_output().expect("it ends")
}

#[test]
fn recognition_runs_whisper_cli_as_designed_and_reports_the_text() {
    let dir = scratch("speech-stt");
    let (catalogue, models) = tiny_speech(&dir);
    let program = fake_whisper(&dir);
    let audio = dir.join("turn.wav");
    wav(&audio, 16_000, 1.5);
    let catalogue = catalogue.to_str().expect("UTF-8");
    let models = models.to_str().expect("UTF-8");
    let output = run(&[
        "speech",
        "stt",
        audio.to_str().expect("UTF-8"),
        "--model",
        "tiny-whisper",
        "--language",
        "it",
        "--vad",
        "--threads",
        "2",
        "--json",
        "--speech-catalogue",
        catalogue,
        "--models-dir",
        models,
        "--whisper-cli",
        program.to_str().expect("UTF-8"),
    ]);
    let document = stdout_json(&output);
    assert_valid(&report_schema(), &document, "stt");
    assert_eq!(document["kind"], "stt");
    assert_eq!(document["text"], "Apri le impostazioni del suono.");
    assert_eq!(document["language"], "it");
    assert_eq!(document["vad"], true);
    assert!((document["audio_seconds"].as_f64().expect("seconds") - 1.5).abs() < 1e-6);
    let recorded = fs::read_to_string(dir.join("whisper-args.txt")).expect("the arguments");
    let arguments: Vec<&str> = recorded.lines().collect();
    let model = format!("{models}/speech/tiny-whisper/ggml-tiny-test.bin");
    let vad = format!("{models}/speech/tiny-vad/ggml-vad-test.bin");
    assert_eq!(
        arguments,
        [
            "-m",
            model.as_str(),
            "-f",
            audio.to_str().expect("UTF-8"),
            "-l",
            "it",
            "-t",
            "2",
            "-nt",
            "-np",
            "--vad",
            "-vm",
            vad.as_str()
        ]
    );
}

#[test]
fn audio_that_is_not_sixteen_kilohertz_mono_is_refused_before_the_engine_runs() {
    let dir = scratch("speech-stt-format");
    let (catalogue, models) = tiny_speech(&dir);
    let program = fake_whisper(&dir);
    let audio = dir.join("music.wav");
    wav(&audio, 44_100, 0.5);
    let output = run(&[
        "speech",
        "stt",
        audio.to_str().expect("UTF-8"),
        "--model",
        "tiny-whisper",
        "--speech-catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--whisper-cli",
        program.to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_text(&output).starts_with("TUR-AI-LOCAL-031"),
        "{}",
        stderr_text(&output)
    );
    assert!(!dir.join("whisper-args.txt").exists(), "the engine ran");

    // A model that is not downloaded, and a missing engine.
    let output = run(&[
        "speech",
        "stt",
        audio.to_str().expect("UTF-8"),
        "--model",
        "tiny-whisper",
        "--speech-catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        dir.join("empty").to_str().expect("UTF-8"),
    ]);
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-012"));
    let good = dir.join("good.wav");
    wav(&good, 16_000, 0.5);
    let output = run(&[
        "speech",
        "stt",
        good.to_str().expect("UTF-8"),
        "--model",
        "tiny-whisper",
        "--speech-catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--whisper-cli",
        "/nonexistent/whisper-cli",
    ]);
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-030"));
}

#[test]
fn synthesis_runs_piper_as_designed_and_reports_the_sound() {
    let dir = scratch("speech-tts");
    let (catalogue, models) = tiny_speech(&dir);
    let program = fake_piper(&dir);
    let out = dir.join("answer.wav");
    let catalogue = catalogue.to_str().expect("UTF-8");
    let models = models.to_str().expect("UTF-8");
    let arguments = [
        "speech",
        "tts",
        "--voice",
        "tiny-voice",
        "--output",
        out.to_str().expect("UTF-8"),
        "--json",
        "--speech-catalogue",
        catalogue,
        "--models-dir",
        models,
        "--piper",
        program.to_str().expect("UTF-8"),
    ];
    let text = "Ho aperto le impostazioni.\nEcco il suono.";
    let output = run_with_input(&arguments, text.as_bytes());
    let document = stdout_json(&output);
    assert_valid(&report_schema(), &document, "tts");
    assert_eq!(document["voice"], "tiny-voice");
    assert_eq!(document["sample_rate"], 22050);
    assert!((document["audio_seconds"].as_f64().expect("seconds") - 0.5).abs() < 1e-6);
    let call: Value = serde_json::from_str(
        &fs::read_to_string(dir.join("piper-call.json")).expect("the call"),
    )
    .expect("JSON");
    // One line: Piper reads one utterance a line.
    assert_eq!(call["text"], "Ho aperto le impostazioni. Ecco il suono.\n");
    let voice = format!("{models}/speech/tiny-voice/it_IT-test-medium.onnx");
    let config = format!("{models}/speech/tiny-voice/it_IT-test-medium.onnx.json");
    assert_eq!(
        call["args"],
        serde_json::json!(["-m", voice, "-c", config, "-f", out.to_str().expect("UTF-8")])
    );

    // Nothing to say is refused before Piper runs.
    fs::remove_file(dir.join("piper-call.json")).expect("removed");
    let output = run_with_input(&arguments, b"   \n");
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-035"));
    assert!(!dir.join("piper-call.json").exists());
    // A recognition model is not a voice.
    let output = run_with_input(
        &[
            "speech",
            "tts",
            "--voice",
            "tiny-whisper",
            "--output",
            out.to_str().expect("UTF-8"),
            "--speech-catalogue",
            catalogue,
            "--models-dir",
            models,
        ],
        b"Ciao.",
    );
    assert!(stderr_text(&output).starts_with("TUR-AI-LOCAL-003"));
}

#[test]
fn ensure_refuses_before_downloading_and_says_so_in_events() {
    let dir = scratch("speech-ensure");
    let (catalogue, models) = tiny_speech(&dir);
    let schema = report_schema();
    // Already present: nothing is fetched.
    let output = run(&[
        "speech",
        "ensure",
        "tiny-whisper",
        "tiny-voice",
        "--json",
        "--speech-catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("small-vm").to_str().expect("UTF-8"),
    ]);
    assert!(output.status.success(), "{}", stderr_text(&output));
    let events: Vec<Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON"))
        .collect();
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_valid(&schema, event, "ensure event");
        assert_eq!(event["event"], "already-present");
    }
    // An id the catalogue does not have: a failed event and the code.
    let output = run(&[
        "speech",
        "ensure",
        "no-such-voice",
        "--json",
        "--speech-catalogue",
        catalogue.to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("small-vm").to_str().expect("UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let event: Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()
            .expect("an event"),
    )
    .expect("JSON");
    assert_valid(&schema, &event, "failed event");
    assert_eq!(event["event"], "failed");
    assert_eq!(event["code"], "TUR-AI-LOCAL-003");
}

#[test]
fn the_speech_folder_is_not_reported_as_an_unknown_model() {
    let dir = scratch("speech-folder");
    let (_, models) = tiny_speech(&dir);
    let output = run(&[
        "models",
        "--json",
        "--catalogue",
        seed_catalogue().to_str().expect("UTF-8"),
        "--models-dir",
        models.to_str().expect("UTF-8"),
        "--root",
        fixture_host("small-vm").to_str().expect("UTF-8"),
    ]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("\"speech\""), "{text}");
}

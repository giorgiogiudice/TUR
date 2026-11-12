// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The curated model catalogue: one TOML file per model, in the format of
//! `schemas/local-model.schema.json`.
//!
//! An entry says where the publisher's file is (a URL pinned to one revision
//! of the publisher's repository), its exact size and SHA-256 digest, what
//! the model can do, the memory it needs, its licence in plain words, and
//! what TUR has measured with it. The loader refuses an entry with a field
//! the format does not define, and checks the rules below, so that a model
//! is only ever downloaded and checked against a complete entry.
//!
//! Rules beyond the field types:
//!
//! * the file is named `<id>.toml`;
//! * every URL is `https`; a file's URL names the pinned revision and ends
//!   with the file's name;
//! * a model with image input has a projector file, and only such a model;
//! * an embeddings model does nothing else and names its pooling;
//! * tool calling and image input go with conversation;
//! * the memory stated is at least the files plus the context cache at the
//!   stated context;
//! * a licence with use restrictions says them in words; "none" means none.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The catalogue directory of an installed system.
pub const INSTALLED_CATALOGUE: &str = "/usr/share/tur/ai-local/models";

/// The format version of a catalogue entry.
pub const ENTRY_SCHEMA_VERSION: u32 = 1;

/// The most entries a catalogue may hold.
pub const MAX_ENTRIES: usize = 256;

/// The largest entry file read.
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024;

/// What a model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Conversation through chat completions.
    Chat,
    /// Calling tools the request offers (OpenAI-style function calling).
    Tools,
    /// Image input. Needs the projector file.
    Vision,
    /// Text embeddings.
    Embeddings,
}

/// How a model thinks before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Thinking {
    /// It answers directly.
    None,
    /// It can think first or answer directly; the service decides.
    Hybrid,
    /// It always thinks first.
    Always,
}

/// How an embeddings model turns token vectors into one vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pooling {
    /// The mean of the token vectors.
    Mean,
    /// The vector of the first token.
    Cls,
    /// The vector of the last token.
    Last,
}

impl Pooling {
    /// The value of llama-server's `--pooling` option.
    pub fn engine_value(self) -> &'static str {
        match self {
            Pooling::Mean => "mean",
            Pooling::Cls => "cls",
            Pooling::Last => "last",
        }
    }
}

/// One file of a model, as the publisher offers it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// The file name, also the name it is stored under.
    pub name: String,
    /// The publisher's download URL, pinned to one revision.
    pub url: String,
    /// The exact size in bytes.
    pub size_bytes: u64,
    /// The SHA-256 digest of the whole file, lower-case hexadecimal.
    pub sha256: String,
}

/// The licence of a model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Licence {
    /// The SPDX identifier, or a `LicenseRef-` identifier for a licence SPDX
    /// does not list.
    pub id: String,
    /// Where the licence text, or the publisher's statement of it, is.
    pub url: String,
    /// What the licence lets a person do, in plain words.
    pub summary: String,
    /// The use restrictions in plain words, or exactly "none".
    pub restrictions: String,
    /// What was read to establish the licence, and what could not be.
    pub evidence: String,
    /// When it was checked, `YYYY-MM-DD`.
    pub checked: String,
}

impl Licence {
    /// The licence restricts use.
    pub fn restricts_use(&self) -> bool {
        self.restrictions.trim() != "none"
    }
}

/// A tool-call result of a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// It worked.
    Yes,
    /// It did not happen: for a tool call, the model answered in text.
    No,
    /// It happened but was not well formed.
    Malformed,
    /// Not tested.
    NotTested,
}

/// What TUR measured with a model. Only from a real run with a report.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tested {
    /// When, `YYYY-MM-DD`.
    pub date: String,
    /// The report in this repository, for example `docs/reports/local-ai-1.md`.
    pub report: String,
    /// The CI run, as a URL.
    pub run: String,
    /// The engine and its build.
    pub engine: String,
    /// The machine.
    pub hardware: String,
    /// The context size the engine was started with.
    pub context: u32,
    /// Generated tokens per second.
    pub generation_tokens_per_second: Option<f64>,
    /// Prompt tokens per second.
    pub prompt_tokens_per_second: Option<f64>,
    /// The engine's peak resident memory.
    pub peak_memory_bytes: Option<u64>,
    /// Whether a requested tool call came back well formed.
    pub tool_call: Outcome,
    /// Whether a question about an image was answered correctly.
    pub image_input: Outcome,
    /// Whether embeddings came back.
    pub embeddings: Outcome,
    /// Anything else worth knowing.
    pub notes: String,
}

/// A catalogue entry.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// The entry format version.
    pub schema_version: u32,
    /// The stable identifier, also the file name without `.toml`.
    pub id: String,
    /// The name shown to a person.
    pub name: String,
    /// Who made and publishes the model.
    pub publisher: String,
    /// The publisher's repository the files come from.
    pub repository: String,
    /// The revision of that repository the URLs are pinned to.
    pub revision: String,
    /// The file format; only `gguf`.
    pub format: String,
    /// The quantisation of the model file, for example `Q8_0`.
    pub quantisation: String,
    /// The number of parameters, as the publisher states it.
    pub parameters: String,
    /// The longest context the model supports, in tokens.
    pub context_length: u32,
    /// The memory needed at [`Model::memory_context`].
    pub memory_bytes: u64,
    /// The context size [`Model::memory_bytes`] is given for.
    pub memory_context: u32,
    /// The size of the context cache per token, in bytes (16-bit cache).
    pub kv_bytes_per_token: u64,
    /// What the model can do.
    pub capabilities: Vec<Capability>,
    /// How it thinks before it answers.
    pub thinking: Thinking,
    /// The pooling of an embeddings model.
    pub pooling: Option<Pooling>,
    /// Languages, as BCP 47 tags, as the publisher states them.
    pub languages: Vec<String>,
    /// Further language support the publisher claims, in its words.
    pub languages_note: Option<String>,
    /// What the model is good for, in plain words.
    pub good_for: Vec<String>,
    /// When the entry was last checked against its sources, `YYYY-MM-DD`.
    pub checked: String,
    /// The model file.
    pub file: ModelFile,
    /// The image projector of a model with image input.
    pub projector: Option<ModelFile>,
    /// The licence.
    pub licence: Licence,
    /// What TUR measured.
    #[serde(default)]
    pub tested: Vec<Tested>,
}

impl Model {
    /// Its files: the model, then the projector.
    pub fn files(&self) -> Vec<&ModelFile> {
        let mut files = vec![&self.file];
        if let Some(projector) = &self.projector {
            files.push(projector);
        }
        files
    }

    /// The bytes of all its files.
    pub fn download_bytes(&self) -> u64 {
        self.files().iter().map(|file| file.size_bytes).sum()
    }

    /// It can do this.
    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// The memory it needs with a context of `context` tokens.
    pub fn memory_for_context(&self, context: u32) -> u64 {
        let base = self.memory_bytes;
        let stated = u64::from(self.memory_context);
        let asked = u64::from(context);
        if asked >= stated {
            base.saturating_add(self.kv_bytes_per_token.saturating_mul(asked - stated))
        } else {
            base.saturating_sub(self.kv_bytes_per_token.saturating_mul(stated - asked))
        }
    }
}

/// The loaded catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalogue {
    /// The directory it was read from.
    pub directory: PathBuf,
    /// The entries, by id.
    pub models: Vec<Model>,
}

impl Catalogue {
    /// The entry with this id.
    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Read and check every entry of `directory`. Every problem found is
    /// returned, not only the first.
    pub fn load(directory: &Path) -> Result<Catalogue, Vec<String>> {
        let entries = fs::read_dir(directory)
            .map_err(|error| vec![format!("{}: {error}", directory.display())])?;
        let mut problems = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    problems.push(format!("{}: {error}", directory.display()));
                    continue;
                }
            };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                problems.push("a file name in the catalogue is not valid UTF-8".to_owned());
                continue;
            };
            if name.starts_with('.') || name == "README.md" {
                continue;
            }
            if !name.ends_with(".toml") || !path.is_file() {
                problems.push(format!(
                    "{name}: the model catalogue holds one .toml file per model and nothing else"
                ));
                continue;
            }
            files.push((name.to_owned(), path));
        }
        files.sort();
        if files.len() > MAX_ENTRIES {
            problems.push(format!("more than {MAX_ENTRIES} entries"));
        }
        let mut models = Vec::new();
        for (name, path) in files {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.len() > MAX_ENTRY_BYTES => {
                    problems.push(format!("{name}: larger than {MAX_ENTRY_BYTES} bytes"));
                    continue;
                }
                Ok(_) => {}
                Err(error) => {
                    problems.push(format!("{name}: {error}"));
                    continue;
                }
            }
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    problems.push(format!("{name}: {error}"));
                    continue;
                }
            };
            match parse_entry(&name, &text) {
                Ok(model) => models.push(model),
                Err(mut found) => problems.append(&mut found),
            }
        }
        let mut ids = BTreeSet::new();
        let mut file_names = BTreeSet::new();
        for model in &models {
            if !ids.insert(model.id.clone()) {
                problems.push(format!("{}: the id is used twice", model.id));
            }
            for file in model.files() {
                if !file_names.insert(file.name.clone()) {
                    problems.push(format!("{}: file {} is named twice", model.id, file.name));
                }
            }
        }
        if problems.is_empty() {
            Ok(Catalogue {
                directory: directory.to_path_buf(),
                models,
            })
        } else {
            Err(problems)
        }
    }
}

/// Parse one entry, named `file_name`, and check its rules.
pub fn parse_entry(file_name: &str, text: &str) -> Result<Model, Vec<String>> {
    let model: Model = toml::from_str(text).map_err(|error| {
        vec![format!(
            "{file_name}: {}",
            error.to_string().lines().collect::<Vec<_>>().join(" ")
        )]
    })?;
    let problems = check(file_name, &model);
    if problems.is_empty() {
        Ok(model)
    } else {
        Err(problems)
    }
}

/// `YYYY-MM-DD`, with a month and a day in range.
pub fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &text[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    matches!(
        (digits(0..4), digits(5..7), digits(8..10)),
        (Some(_), Some(1..=12), Some(1..=31))
    )
}

/// A model identifier: lower case letters, digits, `.` and `-`, starting
/// with a letter or digit, at most 64 characters.
pub fn is_model_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

fn is_lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A plain file name: no directory, not hidden, printable.
fn is_plain_file_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && !text.starts_with('.')
        && !text.contains('/')
        && !text.chars().any(char::is_control)
}

fn check_file(id: &str, what: &str, file: &ModelFile, revision: &str, problems: &mut Vec<String>) {
    if !is_plain_file_name(&file.name) || !file.name.ends_with(".gguf") {
        problems.push(format!("{id}: {what} name {:?} is not a plain .gguf file name", file.name));
    }
    if !file.url.starts_with("https://") {
        problems.push(format!("{id}: {what} URL is not https"));
    }
    if !file.url.contains(&format!("/{revision}/")) {
        problems.push(format!("{id}: {what} URL is not pinned to revision {revision}"));
    }
    if !file.url.ends_with(&format!("/{}", file.name)) {
        problems.push(format!("{id}: {what} URL does not end with the file name"));
    }
    if file.size_bytes == 0 {
        problems.push(format!("{id}: {what} size is 0"));
    }
    if !is_lower_hex(&file.sha256, 64) {
        problems.push(format!("{id}: {what} sha256 is not 64 lower-case hexadecimal digits"));
    }
}

fn check_text(id: &str, what: &str, text: &str, problems: &mut Vec<String>) {
    if text.trim().is_empty() {
        problems.push(format!("{id}: {what} is empty"));
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        problems.push(format!("{id}: {what} has a control character"));
    }
}

/// The rules of an entry beyond its field types.
pub fn check(file_name: &str, model: &Model) -> Vec<String> {
    let mut problems = Vec::new();
    let id = model.id.as_str();
    if model.schema_version != ENTRY_SCHEMA_VERSION {
        problems.push(format!("{id}: schema_version must be {ENTRY_SCHEMA_VERSION}"));
    }
    if !is_model_id(id) {
        problems.push(format!("{file_name}: {id:?} is not a valid model id"));
    }
    if file_name != format!("{id}.toml") {
        problems.push(format!("{file_name}: the file must be named {id}.toml"));
    }
    for (what, text) in [
        ("name", &model.name),
        ("publisher", &model.publisher),
        ("quantisation", &model.quantisation),
        ("parameters", &model.parameters),
        ("licence id", &model.licence.id),
        ("licence summary", &model.licence.summary),
        ("licence restrictions", &model.licence.restrictions),
        ("licence evidence", &model.licence.evidence),
    ] {
        check_text(id, what, text, &mut problems);
    }
    if !model.licence.summary.trim_end().ends_with('.') {
        problems.push(format!("{id}: the licence summary must be sentences"));
    }
    if model.licence.restricts_use() && !model.licence.restrictions.trim_end().ends_with('.') {
        problems.push(format!(
            "{id}: licence restrictions must be \"none\" or sentences"
        ));
    }
    for (what, url) in [
        ("repository", &model.repository),
        ("licence URL", &model.licence.url),
    ] {
        if !url.starts_with("https://") {
            problems.push(format!("{id}: {what} is not https"));
        }
    }
    if !is_lower_hex(&model.revision, 40) {
        problems.push(format!("{id}: revision is not a 40-digit commit identifier"));
    }
    if model.format != "gguf" {
        problems.push(format!("{id}: format must be gguf"));
    }
    for (what, date) in [("checked", &model.checked), ("licence checked", &model.licence.checked)] {
        if !is_date(date) {
            problems.push(format!("{id}: {what} is not a YYYY-MM-DD date"));
        }
    }
    check_file(id, "file", &model.file, &model.revision, &mut problems);
    if let Some(projector) = &model.projector {
        check_file(id, "projector", projector, &model.revision, &mut problems);
    }

    let capabilities: BTreeSet<Capability> = model.capabilities.iter().copied().collect();
    if capabilities.is_empty() {
        problems.push(format!("{id}: no capability"));
    }
    if capabilities.len() != model.capabilities.len() {
        problems.push(format!("{id}: a capability is listed twice"));
    }
    let embeddings = capabilities.contains(&Capability::Embeddings);
    if embeddings && capabilities.len() != 1 {
        problems.push(format!("{id}: an embeddings model has no other capability"));
    }
    if embeddings != model.pooling.is_some() {
        problems.push(format!("{id}: pooling is given for embeddings models and only for them"));
    }
    if (capabilities.contains(&Capability::Tools) || capabilities.contains(&Capability::Vision))
        && !capabilities.contains(&Capability::Chat)
    {
        problems.push(format!("{id}: tools and vision need chat"));
    }
    if capabilities.contains(&Capability::Vision) != model.projector.is_some() {
        problems.push(format!("{id}: a projector is given for vision models and only for them"));
    }
    if embeddings && model.thinking != Thinking::None {
        problems.push(format!("{id}: an embeddings model does not think"));
    }

    if model.context_length == 0 || model.memory_context == 0 {
        problems.push(format!("{id}: context sizes must be positive"));
    }
    if model.memory_context > model.context_length {
        problems.push(format!("{id}: memory_context is longer than the model's context"));
    }
    let floor = model
        .download_bytes()
        .saturating_add(model.kv_bytes_per_token.saturating_mul(u64::from(model.memory_context)));
    if model.memory_bytes < floor {
        problems.push(format!(
            "{id}: memory_bytes {} is less than the files and the context cache, {floor}",
            model.memory_bytes
        ));
    }
    if model.languages.is_empty() {
        problems.push(format!("{id}: no language"));
    }
    for language in &model.languages {
        let valid = !language.is_empty()
            && language.len() <= 16
            && language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid {
            problems.push(format!("{id}: {language:?} is not a language tag"));
        }
    }
    if model.good_for.is_empty() {
        problems.push(format!("{id}: good_for is empty"));
    }
    for text in &model.good_for {
        check_text(id, "good_for", text, &mut problems);
    }
    for tested in &model.tested {
        if !is_date(&tested.date) {
            problems.push(format!("{id}: a test date is not YYYY-MM-DD"));
        }
        if !tested.report.starts_with("docs/reports/") || !tested.report.ends_with(".md") {
            problems.push(format!("{id}: a test report is not under docs/reports/"));
        }
        if !tested.run.starts_with("https://") {
            problems.push(format!("{id}: a test run is not an https URL"));
        }
        check_text(id, "tested engine", &tested.engine, &mut problems);
        check_text(id, "tested hardware", &tested.hardware, &mut problems);
        if !model.can(Capability::Tools) && tested.tool_call != Outcome::NotTested {
            problems.push(format!("{id}: a tool call was tested on a model without tools"));
        }
        if !model.can(Capability::Vision) && tested.image_input != Outcome::NotTested {
            problems.push(format!("{id}: image input was tested on a model without vision"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const ENTRY: &str = r#"
schema_version = 1
id = "example-1b"
name = "Example 1B"
publisher = "Example"
repository = "https://huggingface.co/example/Example-1B-GGUF"
revision = "0123456789abcdef0123456789abcdef01234567"
format = "gguf"
quantisation = "Q4_K_M"
parameters = "1B"
context_length = 32768
memory_bytes = 2000000000
memory_context = 8192
kv_bytes_per_token = 100000
capabilities = ["chat", "tools"]
thinking = "none"
languages = ["en"]
good_for = ["Short answers."]
checked = "2026-11-12"

[file]
name = "Example-1B-Q4_K_M.gguf"
url = "https://huggingface.co/example/Example-1B-GGUF/resolve/0123456789abcdef0123456789abcdef01234567/Example-1B-Q4_K_M.gguf"
size_bytes = 1000000
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

[licence]
id = "Apache-2.0"
url = "https://www.apache.org/licenses/LICENSE-2.0"
summary = "Use for any purpose."
restrictions = "none"
evidence = "Read the licence."
checked = "2026-11-12"
"#;

    #[test]
    fn a_complete_entry_loads() {
        let model = parse_entry("example-1b.toml", ENTRY).expect("valid");
        assert_eq!(model.id, "example-1b");
        assert!(model.can(Capability::Tools));
        assert!(!model.licence.restricts_use());
        assert_eq!(model.download_bytes(), 1_000_000);
        assert_eq!(model.memory_for_context(8192), 2_000_000_000);
        assert_eq!(model.memory_for_context(4096), 2_000_000_000 - 4096 * 100_000);
        assert_eq!(model.memory_for_context(16384), 2_000_000_000 + 8192 * 100_000);
    }

    #[test]
    fn broken_rules_are_all_reported() {
        let broken = ENTRY
            .replace("capabilities = [\"chat\", \"tools\"]", "capabilities = [\"tools\", \"vision\"]")
            .replace("/resolve/0123456789abcdef0123456789abcdef01234567/", "/resolve/main/")
            .replace("memory_bytes = 2000000000", "memory_bytes = 10");
        let problems = parse_entry("other-name.toml", &broken).expect_err("invalid");
        let text = problems.join("\n");
        for expected in [
            "must be named example-1b.toml",
            "not pinned to revision",
            "tools and vision need chat",
            "a projector is given for vision models",
            "memory_bytes 10 is less than",
        ] {
            assert!(text.contains(expected), "{expected} missing from:\n{text}");
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        let extra = format!("{ENTRY}\nsecret_flag = true\n");
        assert!(parse_entry("example-1b.toml", &extra).is_err());
        let extra_in_table = ENTRY.replace("[licence]\n", "[licence]\nfree = true\n");
        assert!(parse_entry("example-1b.toml", &extra_in_table).is_err());
    }

    #[test]
    fn identifiers_and_dates() {
        assert!(is_model_id("qwen3-0.6b"));
        assert!(!is_model_id("Qwen3"));
        assert!(!is_model_id("../x"));
        assert!(!is_model_id(""));
        assert!(is_date("2026-11-12"));
        assert!(!is_date("2026-13-01"));
        assert!(!is_date("12-11-2026"));
    }
}

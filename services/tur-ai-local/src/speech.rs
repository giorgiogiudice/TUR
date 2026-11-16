// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Speech on this computer: recognition (whisper.cpp) and synthesis
//! (Piper), for P@x's voice (`docs/architecture/pax-voice.md`).
//!
//! * The **speech catalogue**: one TOML file per recognition model, voice
//!   activity detector or voice (`speech/*.toml`,
//!   `schemas/speech-model.schema.json`), checked like the model catalogue:
//!   every file pinned to a revision of its publisher's repository, with its
//!   size and SHA-256.
//! * **Tiers**: whether a recognition model runs well on this computer, from
//!   a published encode time scaled to this processor (section 3.3 of the
//!   design). Every offered voice runs faster than real time on any
//!   processor TUR supports.
//! * The **speech store**: `<models folder>/speech/<id>/`, with a record of
//!   verification as for models.
//! * **Recognition**: a 16 kHz mono 16-bit WAVE file given to whisper.cpp's
//!   `whisper-cli`, once per utterance, with its environment cleared.
//! * **Synthesis**: a text given to Piper (piper1-gpl), a GPL program run as
//!   a child process; TUR links nothing of it.
//!
//! Nothing here opens a microphone or plays a sound: P@x's service does
//! that and gives this program files. Nothing is downloaded unless a person
//! asked (`speech ensure`).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::GIB;
use crate::SCHEMA_VERSION;
use crate::catalogue::{Licence, MAX_ENTRY_BYTES, ModelFile, is_date, is_model_id};
use crate::codes::AiLocalCode;
use crate::download::{self, DownloadError};
use crate::ensure::{DISK_RESERVE, EnsureError, FetchOptions};
use crate::hardware::Hardware;
use crate::store::{self, InstallState, PART_SUFFIX, VERIFIED_FILE, Verified, VerifiedFile};

/// The speech catalogue of an installed system.
pub const INSTALLED_SPEECH_CATALOGUE: &str = "/usr/share/tur/ai-local/speech";

/// The folder of the speech store inside the models folder.
pub const SPEECH_FOLDER: &str = "speech";

/// whisper.cpp's command-line recogniser, as TUR's image installs it.
pub const DEFAULT_WHISPER_CLI: &str = "/usr/libexec/tur/whisper.cpp/whisper-cli";

/// Piper's command, the console script of its virtual environment.
pub const DEFAULT_PIPER: &str = "/usr/libexec/tur/piper/bin/piper";

/// The format version of a speech entry.
pub const SPEECH_ENTRY_VERSION: u32 = 1;

/// The most entries a speech catalogue may hold.
pub const MAX_SPEECH_ENTRIES: usize = 128;

/// An utterance recognised within this many milliseconds runs well.
pub const RUNS_WELL_MS: u64 = 2500;

/// The longest text spoken at once, in bytes.
pub const MAX_SPEAK_BYTES: usize = 4000;

/// The largest audio file given to the recogniser, in bytes (about 30
/// minutes at 16 kHz).
pub const MAX_AUDIO_BYTES: u64 = 64 * 1024 * 1024;

/// How long recognition of one file may take.
pub const RECOGNITION_TIMEOUT: Duration = Duration::from_secs(300);

/// How long synthesis of one text may take.
pub const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(120);

/// The languages TUR offers a default voice for, in the order they are
/// shown.
pub const OFFERED_LANGUAGES: [&str; 5] = ["en", "it", "de", "es", "fr"];

/// The stop request of `speech ensure`, in the runtime folder.
pub const SPEECH_CANCEL_FILE: &str = "speech-ensure-cancel";

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeechKind {
    /// Speech to text.
    Recognition,
    /// Text to speech: a voice.
    Synthesis,
    /// Finds where speech is in a recording.
    VoiceActivity,
}

impl SpeechKind {
    /// The text form.
    pub fn as_str(self) -> &'static str {
        match self {
            SpeechKind::Recognition => "recognition",
            SpeechKind::Synthesis => "synthesis",
            SpeechKind::VoiceActivity => "voice-activity",
        }
    }
}

/// The program that uses an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum SpeechEngine {
    /// whisper.cpp.
    #[serde(rename = "whisper.cpp")]
    WhisperCpp,
    /// Piper (piper1-gpl).
    #[serde(rename = "piper")]
    Piper,
}

/// Who speaks in a voice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceInfo {
    /// The publisher's locale, for example `it_IT`.
    pub locale: String,
    /// The speaker's name in the publisher's list.
    pub speaker: String,
    /// The publisher's quality level: `x_low`, `low`, `medium`, `high`.
    pub quality: String,
}

/// One entry of the speech catalogue.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechEntry {
    /// The entry format version.
    pub schema_version: u32,
    /// The stable identifier, also the file name without `.toml`.
    pub id: String,
    /// What it is.
    pub kind: SpeechKind,
    /// The name shown to a person.
    pub name: String,
    /// Who made and publishes it.
    pub publisher: String,
    /// The program that uses it.
    pub engine: SpeechEngine,
    /// The publisher's repository.
    pub repository: String,
    /// The commit of that repository the URLs are pinned to.
    pub revision: String,
    /// Its size, as the publisher states it.
    pub parameters: String,
    /// Languages, as BCP 47 primary tags; `mul` for a multilingual
    /// recognition model.
    pub languages: Vec<String>,
    /// The languages it is the default voice for.
    #[serde(default)]
    pub default_for: Vec<String>,
    /// The memory it needs (an estimate; `speed_basis` says on what).
    pub memory_bytes: u64,
    /// A recognition model's encode time for one 30-second window with 8
    /// threads, in milliseconds: the reference of the speed rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_ms_8_threads: Option<u64>,
    /// Where the speed figure comes from.
    pub speed_basis: String,
    /// The word error rate on Italian FLEURS, in percent, where published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wer_it_fleurs: Option<f64>,
    /// A voice's sample rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// A voice's speaker and quality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<VoiceInfo>,
    /// What it is good for, in plain words.
    pub note: String,
    /// The attribution the licence asks for, or exactly "none".
    pub credit: String,
    /// When the entry was last checked against its sources.
    pub checked: String,
    /// Its files.
    pub files: Vec<ModelFile>,
    /// The licence, followed through every base the voice was trained from.
    pub licence: Licence,
}

impl SpeechEntry {
    /// The bytes of all its files.
    pub fn download_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size_bytes).sum()
    }

    /// Whether it offers `language` (`auto` for a recognition model).
    pub fn offers_language(&self, language: &str) -> bool {
        match self.kind {
            SpeechKind::Recognition | SpeechKind::VoiceActivity => {
                language == "auto"
                    || self.languages.iter().any(|offered| offered == "mul")
                    || self.languages.iter().any(|offered| offered == language)
            }
            SpeechKind::Synthesis => self.languages.iter().any(|offered| offered == language),
        }
    }

    /// The file whose name ends with `suffix`.
    pub fn file_ending(&self, suffix: &str) -> Option<&ModelFile> {
        self.files.iter().find(|file| file.name.ends_with(suffix))
    }

    /// Whether credit must be shown with it.
    pub fn needs_credit(&self) -> bool {
        self.credit.trim() != "none"
    }
}

/// The loaded speech catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechCatalogue {
    /// The directory it was read from.
    pub directory: PathBuf,
    /// The entries, sorted by file name.
    pub entries: Vec<SpeechEntry>,
}

impl SpeechCatalogue {
    /// The entry with this id.
    pub fn get(&self, id: &str) -> Option<&SpeechEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The entries of one kind.
    pub fn of_kind(&self, kind: SpeechKind) -> impl Iterator<Item = &SpeechEntry> {
        self.entries.iter().filter(move |entry| entry.kind == kind)
    }

    /// The default voice of a language.
    pub fn default_voice(&self, language: &str) -> Option<&SpeechEntry> {
        self.of_kind(SpeechKind::Synthesis)
            .find(|entry| entry.default_for.iter().any(|lang| lang == language))
    }

    /// Read and check every entry of `directory`. Every problem found is
    /// returned, not only the first.
    pub fn load(directory: &Path) -> Result<SpeechCatalogue, Vec<String>> {
        let listing = fs::read_dir(directory)
            .map_err(|error| vec![format!("{}: {error}", directory.display())])?;
        let mut problems = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for item in listing {
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    problems.push(format!("{}: {error}", directory.display()));
                    continue;
                }
            };
            let path = item.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                problems.push("a file name in the speech catalogue is not valid UTF-8".to_owned());
                continue;
            };
            if name.starts_with('.') || name == "README.md" {
                continue;
            }
            if !name.ends_with(".toml") || !path.is_file() {
                problems.push(format!(
                    "{name}: the speech catalogue holds one .toml file per entry and nothing else"
                ));
                continue;
            }
            files.push((name.to_owned(), path));
        }
        files.sort();
        if files.len() > MAX_SPEECH_ENTRIES {
            problems.push(format!("more than {MAX_SPEECH_ENTRIES} entries"));
        }
        let mut entries = Vec::new();
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
            match parse_speech_entry(&name, &text) {
                Ok(entry) => entries.push(entry),
                Err(mut found) => problems.append(&mut found),
            }
        }
        let mut ids = BTreeSet::new();
        let mut file_names = BTreeSet::new();
        for entry in &entries {
            if !ids.insert(entry.id.clone()) {
                problems.push(format!("{}: the id is used twice", entry.id));
            }
            for file in &entry.files {
                if !file_names.insert(file.name.clone()) {
                    problems.push(format!("{}: file {} is named twice", entry.id, file.name));
                }
            }
        }
        let mut defaults = BTreeSet::new();
        for entry in &entries {
            for language in &entry.default_for {
                if !defaults.insert(language.clone()) {
                    problems.push(format!(
                        "{}: language {language} has more than one default voice",
                        entry.id
                    ));
                }
            }
        }
        if problems.is_empty() {
            Ok(SpeechCatalogue {
                directory: directory.to_path_buf(),
                entries,
            })
        } else {
            Err(problems)
        }
    }
}

/// Parse one entry, named `file_name`, and check its rules.
pub fn parse_speech_entry(file_name: &str, text: &str) -> Result<SpeechEntry, Vec<String>> {
    let entry: SpeechEntry = toml::from_str(text).map_err(|error| {
        vec![format!(
            "{file_name}: {}",
            error.to_string().lines().collect::<Vec<_>>().join(" ")
        )]
    })?;
    let problems = check_speech_entry(file_name, &entry);
    if problems.is_empty() {
        Ok(entry)
    } else {
        Err(problems)
    }
}

fn is_lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_plain_file_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && !text.starts_with('.')
        && !text.contains('/')
        && !text.chars().any(char::is_control)
}

/// A BCP 47 primary language tag of two lower-case letters, or `mul`.
pub fn is_language_tag(text: &str) -> bool {
    text == "mul" || (text.len() == 2 && text.bytes().all(|b| b.is_ascii_lowercase()))
}

/// The rules of an entry beyond its field types.
pub fn check_speech_entry(file_name: &str, entry: &SpeechEntry) -> Vec<String> {
    let id = entry.id.as_str();
    let mut problems = Vec::new();
    if entry.schema_version != SPEECH_ENTRY_VERSION {
        problems.push(format!(
            "{id}: schema_version {} is not {SPEECH_ENTRY_VERSION}",
            entry.schema_version
        ));
    }
    if !is_model_id(id) {
        problems.push(format!("{file_name}: {id:?} is not a valid id"));
    }
    if file_name != format!("{id}.toml") {
        problems.push(format!("{file_name}: the file must be named {id}.toml"));
    }
    if !entry.repository.starts_with("https://") {
        problems.push(format!("{id}: the repository is not https"));
    }
    if !is_lower_hex(&entry.revision, 40) {
        problems.push(format!(
            "{id}: the revision is not a 40-digit lower-case commit"
        ));
    }
    if entry.files.is_empty() {
        problems.push(format!("{id}: no files"));
    }
    for file in &entry.files {
        if !is_plain_file_name(&file.name) {
            problems.push(format!("{id}: {:?} is not a plain file name", file.name));
        }
        if !file.url.starts_with("https://") {
            problems.push(format!("{id}: {} URL is not https", file.name));
        }
        if !file.url.contains(&format!("/{}/", entry.revision)) {
            problems.push(format!(
                "{id}: {} URL is not pinned to revision {}",
                file.name, entry.revision
            ));
        }
        if !file.url.ends_with(&format!("/{}", file.name)) {
            problems.push(format!("{id}: {} URL does not end with the file name", file.name));
        }
        if file.size_bytes == 0 {
            problems.push(format!("{id}: {} size is 0", file.name));
        }
        if !is_lower_hex(&file.sha256, 64) {
            problems.push(format!(
                "{id}: {} sha256 is not 64 lower-case hexadecimal digits",
                file.name
            ));
        }
    }
    if entry.languages.is_empty() {
        problems.push(format!("{id}: no language"));
    }
    for language in entry.languages.iter().chain(entry.default_for.iter()) {
        if !is_language_tag(language) {
            problems.push(format!("{id}: {language:?} is not a language tag"));
        }
    }
    for language in &entry.default_for {
        if !entry.languages.contains(language) {
            problems.push(format!(
                "{id}: it is the default for {language}, which it does not speak"
            ));
        }
    }
    if entry.memory_bytes < entry.download_bytes() {
        problems.push(format!("{id}: memory_bytes is less than its files"));
    }
    for (what, text) in [
        ("name", entry.name.as_str()),
        ("publisher", entry.publisher.as_str()),
        ("note", entry.note.as_str()),
        ("credit", entry.credit.as_str()),
        ("speed_basis", entry.speed_basis.as_str()),
        ("licence summary", entry.licence.summary.as_str()),
        ("licence restrictions", entry.licence.restrictions.as_str()),
        ("licence evidence", entry.licence.evidence.as_str()),
    ] {
        if text.trim().is_empty() {
            problems.push(format!("{id}: {what} is empty"));
        }
        if text.chars().any(|c| c.is_control() && c != '\n') {
            problems.push(format!("{id}: {what} has a control character"));
        }
    }
    if !entry.licence.url.starts_with("https://") {
        problems.push(format!("{id}: the licence URL is not https"));
    }
    if !is_date(&entry.checked) || !is_date(&entry.licence.checked) {
        problems.push(format!("{id}: a date is not YYYY-MM-DD"));
    }
    match entry.kind {
        SpeechKind::Recognition => {
            if entry.engine != SpeechEngine::WhisperCpp {
                problems.push(format!("{id}: a recognition model is used by whisper.cpp"));
            }
            if entry.encode_ms_8_threads.is_none_or(|ms| ms == 0) {
                problems.push(format!("{id}: a recognition model needs encode_ms_8_threads"));
            }
            if entry.files.len() != 1 || !entry.files[0].name.ends_with(".bin") {
                problems.push(format!("{id}: a recognition model is one .bin file"));
            }
            if entry.voice.is_some() || entry.sample_rate.is_some() || !entry.default_for.is_empty()
            {
                problems.push(format!(
                    "{id}: voice, sample_rate and default_for are for voices"
                ));
            }
        }
        SpeechKind::VoiceActivity => {
            if entry.engine != SpeechEngine::WhisperCpp {
                problems.push(format!("{id}: a voice activity detector is used by whisper.cpp"));
            }
            if entry.files.len() != 1 || !entry.files[0].name.ends_with(".bin") {
                problems.push(format!("{id}: a voice activity detector is one .bin file"));
            }
            if entry.voice.is_some() || entry.sample_rate.is_some() || !entry.default_for.is_empty()
            {
                problems.push(format!(
                    "{id}: voice, sample_rate and default_for are for voices"
                ));
            }
        }
        SpeechKind::Synthesis => {
            if entry.engine != SpeechEngine::Piper {
                problems.push(format!("{id}: a voice is used by Piper"));
            }
            let onnx = entry
                .files
                .iter()
                .filter(|file| file.name.ends_with(".onnx"))
                .count();
            let config = entry
                .files
                .iter()
                .filter(|file| file.name.ends_with(".onnx.json"))
                .count();
            if entry.files.len() != 2 || onnx != 1 || config != 1 {
                problems.push(format!(
                    "{id}: a voice is one .onnx file and its .onnx.json"
                ));
            }
            if entry.sample_rate.is_none_or(|rate| !(8000..=48000).contains(&rate)) {
                problems.push(format!("{id}: a voice needs a sample_rate"));
            }
            if entry.voice.is_none() {
                problems.push(format!("{id}: a voice needs [voice]"));
            }
            if entry.languages.len() != 1 || entry.languages[0] == "mul" {
                problems.push(format!("{id}: a voice speaks one language"));
            }
        }
    }
    problems
}

// ------------------------------------------------------------------- tiers

string_enum! {
    /// How a speech entry suits this computer.
    pub enum SpeechTier {
        /// It fits and answers within [`RUNS_WELL_MS`].
        RunsWell => "runs-well",
        /// It fits, and the wait is longer.
        Slow => "slow",
        /// It needs more memory than this computer has.
        DoesNotFit => "does-not-fit",
    }
}

/// A tier with its reasons in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TierJudgement {
    /// The tier.
    pub tier: SpeechTier,
    /// Why, in sentences.
    pub reasons: Vec<String>,
    /// The expected wait for one utterance, in milliseconds (recognition
    /// only; an estimate).
    pub expected_ms: Option<u64>,
}

/// The memory kept free beside a speech model: the larger of 1 GiB and a
/// tenth of the memory.
pub fn reserve(total: u64) -> u64 {
    GIB.max(total / 10)
}

/// The expected wait to recognise one utterance with `entry` on this
/// computer, in milliseconds: the reference encode time scaled by the
/// threads (at most 16), four times longer without AVX2 and FMA (or Arm's
/// dot product), eight times shorter with a usable Vulkan device
/// (assumed). An estimate (pax-voice.md, section 3.3).
pub fn expected_ms(entry: &SpeechEntry, hardware: &Hardware) -> Option<u64> {
    let reference = entry.encode_ms_8_threads?;
    let threads = u64::from(hardware.cpu.logical_cpus.clamp(1, 16));
    let mut ms = reference.saturating_mul(8) / threads;
    if !hardware.cpu.fast_matrix_instructions {
        ms = ms.saturating_mul(4);
    }
    if hardware.usable_open_backend == Some(true) {
        ms /= 8;
    }
    Some(ms.max(1))
}

fn seconds_text(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// How `entry` suits this computer, with its reasons.
pub fn speech_tier(entry: &SpeechEntry, hardware: &Hardware) -> TierJudgement {
    let total = hardware.memory.total_bytes;
    let room = total.saturating_sub(reserve(total));
    if entry.memory_bytes > room {
        return TierJudgement {
            tier: SpeechTier::DoesNotFit,
            reasons: vec![format!(
                "It needs about {} of memory; this computer has {} and keeps {} free for \
                 everything else.",
                crate::human_bytes(entry.memory_bytes),
                crate::human_bytes(total),
                crate::human_bytes(reserve(total))
            )],
            expected_ms: None,
        };
    }
    let mut reasons = vec![format!(
        "It needs about {} of memory; this computer has {}.",
        crate::human_bytes(entry.memory_bytes),
        crate::human_bytes(total)
    )];
    match entry.kind {
        SpeechKind::Recognition => {
            let ms = expected_ms(entry, hardware).unwrap_or(u64::MAX);
            let threads = hardware.cpu.logical_cpus.clamp(1, 16);
            reasons.push(format!(
                "Recognising what you say should take about {} on this computer's {} \
                 processors (an estimate from published figures, not a measurement).",
                seconds_text(ms),
                threads
            ));
            if !hardware.cpu.fast_matrix_instructions {
                reasons.push(
                    "This processor lacks the instructions (AVX2 and FMA) that make \
                     recognition fast, so it is about four times slower."
                        .to_owned(),
                );
            }
            if hardware.usable_open_backend == Some(true) {
                reasons.push(
                    "A graphics processor TUR can use (Vulkan) does most of the work.".to_owned(),
                );
            }
            let tier = if ms <= RUNS_WELL_MS {
                SpeechTier::RunsWell
            } else {
                reasons.push(format!(
                    "That is more than {}: you would wait after you stop speaking.",
                    seconds_text(RUNS_WELL_MS)
                ));
                SpeechTier::Slow
            };
            TierJudgement {
                tier,
                reasons,
                expected_ms: Some(ms),
            }
        }
        SpeechKind::Synthesis => {
            reasons.push(
                "Speech in this voice is made faster than it is spoken on any computer TUR \
                 supports (published figures, not a measurement)."
                    .to_owned(),
            );
            TierJudgement {
                tier: SpeechTier::RunsWell,
                reasons,
                expected_ms: None,
            }
        }
        SpeechKind::VoiceActivity => {
            reasons.push("It is small and quick on any computer.".to_owned());
            TierJudgement {
                tier: SpeechTier::RunsWell,
                reasons,
                expected_ms: None,
            }
        }
    }
}

/// What to use on this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechRecommendation<'a> {
    /// The recognition model, with its tier.
    pub recognition: Option<(&'a SpeechEntry, TierJudgement)>,
    /// The voice activity detector.
    pub vad: Option<&'a SpeechEntry>,
    /// The voice of each language, in [`OFFERED_LANGUAGES`] order.
    pub voices: Vec<(String, &'a SpeechEntry)>,
}

/// The recognition model that suits this computer best: of those that run
/// well, the largest (the most accurate); without one, the smallest that
/// fits, which is slow. The default voice of each language asked for, or
/// of every offered language.
pub fn recommend<'a>(
    catalogue: &'a SpeechCatalogue,
    hardware: &Hardware,
    language: Option<&str>,
) -> SpeechRecommendation<'a> {
    let wanted = language.unwrap_or("auto");
    let judged: Vec<(&SpeechEntry, TierJudgement)> = catalogue
        .of_kind(SpeechKind::Recognition)
        .filter(|entry| entry.offers_language(wanted))
        .map(|entry| (entry, speech_tier(entry, hardware)))
        .collect();
    let best_running = judged
        .iter()
        .filter(|(_, judgement)| judgement.tier == SpeechTier::RunsWell)
        .max_by_key(|(entry, _)| (entry.encode_ms_8_threads, entry.memory_bytes))
        .cloned();
    let recognition = best_running.or_else(|| {
        judged
            .iter()
            .filter(|(_, judgement)| judgement.tier == SpeechTier::Slow)
            .min_by_key(|(entry, _)| entry.memory_bytes)
            .cloned()
    });
    let vad = catalogue
        .of_kind(SpeechKind::VoiceActivity)
        .find(|entry| speech_tier(entry, hardware).tier != SpeechTier::DoesNotFit);
    let languages: Vec<&str> = match language {
        Some(language) if language != "auto" => vec![language],
        _ => OFFERED_LANGUAGES.to_vec(),
    };
    let voices = languages
        .into_iter()
        .filter_map(|language| {
            catalogue
                .default_voice(language)
                .filter(|voice| speech_tier(voice, hardware).tier != SpeechTier::DoesNotFit)
                .map(|voice| (language.to_owned(), voice))
        })
        .collect();
    SpeechRecommendation {
        recognition,
        vad,
        voices,
    }
}

// ------------------------------------------------------------------- store

/// The speech store: `<models folder>/speech/<id>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechStore {
    /// Its path.
    pub root: PathBuf,
}

impl SpeechStore {
    /// The speech store inside a models folder.
    pub fn in_models_folder(models: &Path) -> SpeechStore {
        SpeechStore {
            root: models.join(SPEECH_FOLDER),
        }
    }

    /// The folder of an entry; `None` for a string that is not an id.
    pub fn dir(&self, id: &str) -> Option<PathBuf> {
        is_model_id(id).then(|| self.root.join(id))
    }

    /// The path a file of an entry is stored at.
    pub fn file_path(&self, entry: &SpeechEntry, name: &str) -> Option<PathBuf> {
        self.dir(&entry.id).map(|dir| dir.join(name))
    }

    /// Whether an entry is installed, and if it is damaged, why.
    pub fn state(&self, entry: &SpeechEntry) -> (InstallState, Option<String>) {
        let Some(dir) = self.dir(&entry.id) else {
            return (InstallState::Absent, None);
        };
        if !dir.is_dir() {
            return (InstallState::Absent, None);
        }
        let verified: Option<Verified> = fs::read_to_string(dir.join(VERIFIED_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        let Some(verified) = verified else {
            let has_files = entry.files.iter().any(|file| dir.join(&file.name).exists());
            return if has_files {
                (
                    InstallState::Damaged,
                    Some("the record of verification is missing or cannot be read".to_owned()),
                )
            } else {
                (InstallState::Absent, None)
            };
        };
        for file in &entry.files {
            let recorded = verified.files.iter().find(|item| item.name == file.name);
            match recorded {
                Some(item) if item.sha256 == file.sha256 && item.size_bytes == file.size_bytes => {}
                Some(_) => {
                    return (
                        InstallState::Damaged,
                        Some(format!(
                            "{} was verified as another file than the catalogue names",
                            file.name
                        )),
                    );
                }
                None => {
                    return (
                        InstallState::Damaged,
                        Some(format!("{} is not in the record of verification", file.name)),
                    );
                }
            }
            match fs::metadata(dir.join(&file.name)) {
                Ok(metadata) if metadata.len() == file.size_bytes => {}
                Ok(metadata) => {
                    return (
                        InstallState::Damaged,
                        Some(format!(
                            "{} has {} bytes; the catalogue gives {}",
                            file.name,
                            metadata.len(),
                            file.size_bytes
                        )),
                    );
                }
                Err(error) => {
                    return (
                        InstallState::Damaged,
                        Some(format!("{}: {error}", file.name)),
                    );
                }
            }
        }
        (InstallState::Installed, None)
    }

    /// Whether an entry is installed.
    pub fn is_installed(&self, entry: &SpeechEntry) -> bool {
        self.state(entry).0 == InstallState::Installed
    }

    /// Write the record of verification of an entry whose files were just
    /// checked.
    pub fn write_verified(&self, entry: &SpeechEntry, now: u64) -> io::Result<()> {
        let dir = self
            .dir(&entry.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not an id"))?;
        let record = Verified {
            schema_version: 1,
            model: entry.id.clone(),
            files: entry
                .files
                .iter()
                .map(|file| VerifiedFile {
                    name: file.name.clone(),
                    size_bytes: file.size_bytes,
                    sha256: file.sha256.clone(),
                })
                .collect(),
            verified_at: now,
        };
        let text = serde_json::to_string_pretty(&record)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = dir.join(format!("{VERIFIED_FILE}{PART_SUFFIX}"));
        fs::write(&temporary, text)?;
        fs::rename(&temporary, dir.join(VERIFIED_FILE))
    }

    /// Remove an entry's folder. Returns the bytes freed, or `None` when the
    /// folder does not exist.
    pub fn remove(&self, id: &str) -> io::Result<Option<u64>> {
        let Some(dir) = self.dir(id) else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an id"));
        };
        if !dir.is_dir() {
            return Ok(None);
        }
        if fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the folder is a symbolic link",
            ));
        }
        let bytes = fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .filter_map(|item| item.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum();
        fs::remove_dir_all(&dir)?;
        Ok(Some(bytes))
    }
}

// ------------------------------------------------------------------ ensure

/// One step of `speech ensure`, as it is printed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpeechEnsureEvent {
    /// The format version of the document.
    pub schema_version: u32,
    /// Always `speech-ensure-event`.
    pub kind: &'static str,
    /// `plan`, `progress`, `verified`, `installed`, `already-present`,
    /// `failed` or `cancelled`.
    pub event: &'static str,
    /// The entry.
    pub model: String,
    /// The code of an outcome.
    pub code: Option<AiLocalCode>,
    /// In plain words.
    pub message: String,
    /// The file this is about.
    pub file: Option<String>,
    /// The bytes downloaded and checked so far, of the whole entry.
    pub bytes_done: Option<u64>,
    /// The bytes of the whole entry.
    pub bytes_total: Option<u64>,
    /// The bytes still to be downloaded, in a plan.
    pub bytes_to_download: Option<u64>,
    /// The free space of the disk, in a plan; null when it could not be read.
    pub free_bytes: Option<u64>,
}

impl SpeechEnsureEvent {
    /// An event with nothing but its name, the entry and the message.
    pub fn new(event: &'static str, model: &str, message: impl Into<String>) -> SpeechEnsureEvent {
        SpeechEnsureEvent {
            schema_version: SCHEMA_VERSION,
            kind: "speech-ensure-event",
            event,
            model: model.to_owned(),
            code: None,
            message: message.into(),
            file: None,
            bytes_done: None,
            bytes_total: None,
            bytes_to_download: None,
            free_bytes: None,
        }
    }

    /// With a code.
    #[must_use]
    pub fn with_code(mut self, code: AiLocalCode) -> SpeechEnsureEvent {
        self.code = Some(code);
        self
    }
}

/// Download and check every file of `entry` the speech store does not have
/// yet: the same steps as [`crate::ensure::fetch_model`] (room on the disk,
/// a resumable download from the pinned URL, size and SHA-256 checked
/// before a file gets its name, the record of verification last).
///
/// # Errors
///
/// `TUR-AI-LOCAL-028` when the disk has no room; the download's codes
/// (`-008` to `-011`); `-029` when the person asked to stop.
pub fn fetch_speech(
    store: &SpeechStore,
    entry: &SpeechEntry,
    options: &FetchOptions<'_>,
    emit: &mut dyn FnMut(SpeechEnsureEvent),
) -> Result<(), EnsureError> {
    let storage = |path: &Path, error: &io::Error| {
        EnsureError::new(
            AiLocalCode::StorageFailed,
            format!("{}: {error}", path.display()),
        )
    };
    let dir = store.dir(&entry.id).ok_or_else(|| {
        EnsureError::new(
            AiLocalCode::UnknownModel,
            format!("{:?} is not an id", entry.id),
        )
    })?;
    let total = entry.download_bytes();
    match store.state(entry).0 {
        InstallState::Installed => {
            let mut event = SpeechEnsureEvent::new(
                "already-present",
                &entry.id,
                format!(
                    "{} is already on this computer and was checked when it was downloaded.",
                    entry.name
                ),
            )
            .with_code(AiLocalCode::Installed);
            event.bytes_done = Some(total);
            event.bytes_total = Some(total);
            emit(event);
            return Ok(());
        }
        InstallState::Damaged => {
            store
                .remove(&entry.id)
                .map_err(|error| storage(&dir, &error))?;
        }
        InstallState::Absent => {}
    }
    store::create_private_dir(&dir).map_err(|error| storage(&dir, &error))?;

    let mut present: u64 = 0;
    for file in &entry.files {
        let destination = dir.join(&file.name);
        if destination.is_file() {
            present += fs::metadata(&destination).map_or(0, |metadata| metadata.len());
        } else {
            present += download::partial_bytes(&destination).min(file.size_bytes);
        }
    }
    let present = present.min(total);
    let to_download = total - present;
    let free = (options.free_space)(&dir);
    let mut plan = SpeechEnsureEvent::new(
        "plan",
        &entry.id,
        format!(
            "{} downloads {} from {}.",
            entry.name,
            crate::human_bytes(to_download),
            entry.repository
        ),
    );
    plan.bytes_done = Some(present);
    plan.bytes_total = Some(total);
    plan.bytes_to_download = Some(to_download);
    plan.free_bytes = free;
    emit(plan);
    if let Some(free) = free
        && free < to_download.saturating_add(DISK_RESERVE)
    {
        return Err(EnsureError::new(
            AiLocalCode::NoDiskSpace,
            format!(
                "{} to download and {} kept free, and {} is free on the disk of {}",
                crate::human_bytes(to_download),
                crate::human_bytes(DISK_RESERVE),
                crate::human_bytes(free),
                dir.display()
            ),
        ));
    }

    let mut done_before: u64 = 0;
    for file in &entry.files {
        let destination = dir.join(&file.name);
        if destination.is_file() {
            match download::check_existing(&destination, file) {
                Ok(()) => {
                    done_before += file.size_bytes;
                    continue;
                }
                Err(_) => {
                    fs::remove_file(&destination).map_err(|error| storage(&destination, &error))?;
                }
            }
        }
        let step = (total / 100).max(1 << 20);
        let mut next = 0u64;
        let name = file.name.clone();
        let mut progress = |bytes: u64| {
            let done = done_before + bytes;
            if done >= next || bytes == file.size_bytes {
                let mut event = SpeechEnsureEvent::new(
                    "progress",
                    &entry.id,
                    format!(
                        "{} of {} ({}%).",
                        crate::human_bytes(done),
                        crate::human_bytes(total),
                        done.saturating_mul(100) / total.max(1)
                    ),
                );
                event.file = Some(name.clone());
                event.bytes_done = Some(done);
                event.bytes_total = Some(total);
                emit(event);
                next = done.saturating_add(step);
            }
        };
        let result = download::download_resumable(
            options.agent,
            options.scheme,
            file,
            &destination,
            options.cancelled,
            &mut progress,
        );
        if let Err(error) = result {
            let code = error.code();
            return Err(match error {
                DownloadError::Cancelled => EnsureError::new(
                    code,
                    format!("{}: stopped; the partial file is kept", file.name),
                ),
                other => EnsureError::new(code, format!("{}: {other}", file.name)),
            });
        }
        done_before += file.size_bytes;
        let mut verified = SpeechEnsureEvent::new(
            "verified",
            &entry.id,
            format!(
                "{} has the size and the SHA-256 digest the catalogue gives.",
                file.name
            ),
        );
        verified.file = Some(file.name.clone());
        verified.bytes_done = Some(done_before);
        verified.bytes_total = Some(total);
        emit(verified);
    }
    store
        .write_verified(entry, store::now_seconds())
        .map_err(|error| storage(&dir, &error))?;
    let mut installed = SpeechEnsureEvent::new(
        "installed",
        &entry.id,
        AiLocalCode::Installed.entry().user_message,
    )
    .with_code(AiLocalCode::Installed);
    installed.bytes_done = Some(total);
    installed.bytes_total = Some(total);
    emit(installed);
    Ok(())
}

// ------------------------------------------------------------------- audio

/// What a WAVE file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    /// The format tag: 1 is PCM.
    pub format: u16,
    /// Channels.
    pub channels: u16,
    /// Samples a second.
    pub sample_rate: u32,
    /// Bits a sample.
    pub bits: u16,
    /// The bytes of sound.
    pub data_bytes: u64,
}

impl WavInfo {
    /// Its length in seconds.
    pub fn seconds(&self) -> f64 {
        let frame = u64::from(self.channels) * u64::from(self.bits / 8);
        if frame == 0 || self.sample_rate == 0 {
            return 0.0;
        }
        (self.data_bytes / frame) as f64 / f64::from(self.sample_rate)
    }

    /// Whether it is what Whisper models take: PCM, 16 bits, one channel,
    /// 16 kHz, and not empty.
    pub fn is_recognisable(&self) -> bool {
        self.format == 1
            && self.channels == 1
            && self.sample_rate == 16_000
            && self.bits == 16
            && self.data_bytes >= 2
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

/// Read the header of a RIFF WAVE file held in `bytes`: its `fmt ` chunk
/// and the size of its `data` chunk (a size the writer left open, as a
/// streaming writer does, counts as the rest of the file).
///
/// # Errors
///
/// A sentence saying what is not right.
pub fn parse_wav(bytes: &[u8]) -> Result<WavInfo, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("it is not a RIFF WAVE file".to_owned());
    }
    let mut at = 12usize;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = le32(bytes, at + 4).ok_or("a chunk is cut short")?;
        let body = at + 8;
        if id == b"fmt " {
            if size < 16 {
                return Err("the format chunk is too short".to_owned());
            }
            let tag = le16(bytes, body).ok_or("the format chunk is cut short")?;
            let channels = le16(bytes, body + 2).ok_or("the format chunk is cut short")?;
            let rate = le32(bytes, body + 4).ok_or("the format chunk is cut short")?;
            let bits = le16(bytes, body + 14).ok_or("the format chunk is cut short")?;
            format = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let Some((format, channels, sample_rate, bits)) = format else {
                return Err("the sound comes before its format".to_owned());
            };
            let rest = (bytes.len() - body) as u64;
            let declared = u64::from(size);
            let data_bytes = if size == u32::MAX || declared > rest {
                rest
            } else {
                declared
            };
            return Ok(WavInfo {
                format,
                channels,
                sample_rate,
                bits,
                data_bytes,
            });
        }
        // Chunks are padded to an even size.
        let advance = usize::try_from(size)
            .ok()
            .and_then(|size| size.checked_add(size & 1))
            .and_then(|size| size.checked_add(8))
            .ok_or("a chunk is too large")?;
        at = at.checked_add(advance).ok_or("a chunk is too large")?;
    }
    Err("there is no sound in it".to_owned())
}

/// Read the header of the WAVE file at `path`.
///
/// # Errors
///
/// A sentence saying what is not right, or that it could not be read.
pub fn wav_info(path: &Path) -> Result<WavInfo, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if metadata.len() > MAX_AUDIO_BYTES {
        return Err(format!(
            "{} is larger than {}",
            path.display(),
            crate::human_bytes(MAX_AUDIO_BYTES)
        ));
    }
    // The header is in the first few kilobytes; the data chunk's size is
    // checked against the whole length.
    let mut file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut head = vec![0u8; 64 * 1024];
    let mut read = 0usize;
    while read < head.len() {
        match file.read(&mut head[read..]) {
            Ok(0) => break,
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    head.truncate(read);
    let mut info = parse_wav(&head)?;
    // What parse_wav saw is the head only: the data runs to the end of the
    // file at most.
    let header_end = find_data_start(&head).unwrap_or(head.len()) as u64;
    let available = metadata.len().saturating_sub(header_end);
    let declared = declared_data_size(&head).unwrap_or(u64::from(u32::MAX));
    info.data_bytes = if declared == u64::from(u32::MAX) || declared > available {
        available
    } else {
        declared
    };
    Ok(info)
}

fn find_data_start(bytes: &[u8]) -> Option<usize> {
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let size = le32(bytes, at + 4)?;
        if &bytes[at..at + 4] == b"data" {
            return Some(at + 8);
        }
        at = at.checked_add(usize::try_from(size).ok()? + (size as usize & 1) + 8)?;
    }
    None
}

fn declared_data_size(bytes: &[u8]) -> Option<u64> {
    let start = find_data_start(bytes)?;
    le32(bytes, start - 4).map(u64::from)
}

// ---------------------------------------------------------------- programs

/// What a program run with [`run_program`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramOutput {
    /// Its exit status; `None` when a signal ended it.
    pub status: Option<i32>,
    /// What it wrote to standard output.
    pub stdout: Vec<u8>,
    /// What it wrote to standard error.
    pub stderr: Vec<u8>,
    /// It was stopped because it took too long.
    pub timed_out: bool,
}

/// Run `program` with `arguments`, an environment holding only `env`,
/// `input` on its standard input, and at most `timeout`; its output is read
/// on threads of their own so that a full pipe cannot stall it.
///
/// # Errors
///
/// The program could not be started.
pub fn run_program(
    program: &Path,
    arguments: &[String],
    env: &[(&str, &str)],
    input: Option<&[u8]>,
    timeout: Duration,
) -> io::Result<ProgramOutput> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_clear()
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(stream) = stdout {
            let _ = stream.take(16 * 1024 * 1024).read_to_end(&mut buffer);
        }
        buffer
    });
    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(stream) = stderr {
            let _ = stream.take(1024 * 1024).read_to_end(&mut buffer);
        }
        buffer
    });
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // A program that ends without reading is not an error here; its
        // status says what happened.
        let _ = stdin.write_all(input);
        drop(stdin);
    }
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if started.elapsed() >= timeout => {
                timed_out = true;
                let _ = child.kill();
                break child.wait()?;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    Ok(ProgramOutput {
        status: status.code(),
        stdout,
        stderr,
        timed_out,
    })
}

/// The last lines a program wrote to standard error, for a detail.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(5);
    lines[start..].join(" | ")
}

fn failure_detail(output: &ProgramOutput, what: &str) -> String {
    if output.timed_out {
        return format!("{what} did not finish in time and was stopped");
    }
    let status = output
        .status
        .map_or_else(|| "a signal".to_owned(), |code| format!("exit status {code}"));
    let errors = tail(&output.stderr);
    if errors.is_empty() {
        format!("{what} ended with {status}")
    } else {
        format!("{what} ended with {status}: {errors}")
    }
}

/// The transcript whisper-cli printed: its lines joined, without the marks
/// it prints for silence or noise (`[BLANK_AUDIO]`, `[Music]`, `(silence)`).
pub fn clean_transcript(text: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let marked = (line.starts_with('[') && line.ends_with(']'))
            || (line.starts_with('(') && line.ends_with(')'));
        if marked {
            continue;
        }
        for word in line.split_whitespace() {
            if word == "[BLANK_AUDIO]" {
                continue;
            }
            words.push(word);
        }
    }
    words.join(" ")
}

/// What recognition needs.
#[derive(Debug, Clone)]
pub struct Recognition<'a> {
    /// whisper.cpp's `whisper-cli`.
    pub program: &'a Path,
    /// The recognition model's file.
    pub model: &'a Path,
    /// The voice activity detector's file, if it is to be used.
    pub vad_model: Option<&'a Path>,
    /// The 16 kHz mono 16-bit WAVE file.
    pub audio: &'a Path,
    /// `auto` or a language tag.
    pub language: &'a str,
    /// Processor threads.
    pub threads: u32,
}

/// The arguments of `whisper-cli` for a recognition.
pub fn recognition_arguments(request: &Recognition<'_>) -> Vec<String> {
    let mut arguments = vec![
        "-m".to_owned(),
        request.model.to_string_lossy().into_owned(),
        "-f".to_owned(),
        request.audio.to_string_lossy().into_owned(),
        "-l".to_owned(),
        request.language.to_owned(),
        "-t".to_owned(),
        request.threads.max(1).to_string(),
        // No timestamps, nothing printed but the text.
        "-nt".to_owned(),
        "-np".to_owned(),
    ];
    if let Some(vad) = request.vad_model {
        arguments.push("--vad".to_owned());
        arguments.push("-vm".to_owned());
        arguments.push(vad.to_string_lossy().into_owned());
    }
    arguments
}

/// Recognise what is said in an audio file.
///
/// # Errors
///
/// `TUR-AI-LOCAL-030` when the program is missing, `-031` when the file is
/// not 16 kHz mono 16-bit PCM, `-032` when recognition failed.
pub fn recognise(request: &Recognition<'_>) -> Result<String, (AiLocalCode, String)> {
    if !request.program.is_file() {
        return Err((
            AiLocalCode::SpeechEngineMissing,
            format!("{} is not there", request.program.display()),
        ));
    }
    let info = wav_info(request.audio).map_err(|detail| (AiLocalCode::AudioNotUsable, detail))?;
    if !info.is_recognisable() {
        return Err((
            AiLocalCode::AudioNotUsable,
            format!(
                "format {}, {} channels, {} Hz, {} bits, {} bytes of sound; 1, 1, 16000, 16 and \
                 at least 2 are needed",
                info.format, info.channels, info.sample_rate, info.bits, info.data_bytes
            ),
        ));
    }
    let output = run_program(
        request.program,
        &recognition_arguments(request),
        &[("LANG", "C.UTF-8")],
        None,
        RECOGNITION_TIMEOUT,
    )
    .map_err(|error| {
        (
            AiLocalCode::SpeechEngineMissing,
            format!("{}: {error}", request.program.display()),
        )
    })?;
    if output.timed_out || output.status != Some(0) {
        return Err((
            AiLocalCode::RecognitionFailed,
            failure_detail(&output, "whisper-cli"),
        ));
    }
    Ok(clean_transcript(&String::from_utf8_lossy(&output.stdout)))
}

/// What synthesis needs.
#[derive(Debug, Clone)]
pub struct Synthesis<'a> {
    /// Piper's command.
    pub program: &'a Path,
    /// The voice's `.onnx` file.
    pub voice: &'a Path,
    /// The voice's `.onnx.json` file.
    pub config: &'a Path,
    /// Where the WAVE file is written.
    pub output: &'a Path,
}

/// The arguments of Piper for a synthesis: the voice, its configuration and
/// the output file; the text goes on standard input, one line.
pub fn synthesis_arguments(request: &Synthesis<'_>) -> Vec<String> {
    vec![
        "-m".to_owned(),
        request.voice.to_string_lossy().into_owned(),
        "-c".to_owned(),
        request.config.to_string_lossy().into_owned(),
        "-f".to_owned(),
        request.output.to_string_lossy().into_owned(),
    ]
}

/// The text as Piper is given it: one line (Piper reads one utterance a
/// line), with control characters made spaces.
pub fn one_line(text: &str) -> String {
    let joined: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut line = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    line.push('\n');
    line
}

/// Check a text to speak.
///
/// # Errors
///
/// `TUR-AI-LOCAL-035` with a sentence.
pub fn check_text(text: &str) -> Result<(), (AiLocalCode, String)> {
    if text.trim().is_empty() {
        return Err((AiLocalCode::TextNotUsable, "the text is empty".to_owned()));
    }
    if text.len() > MAX_SPEAK_BYTES {
        return Err((
            AiLocalCode::TextNotUsable,
            format!(
                "the text has {} bytes; at most {MAX_SPEAK_BYTES} are spoken at once",
                text.len()
            ),
        ));
    }
    Ok(())
}

/// Speak a text into a WAVE file.
///
/// # Errors
///
/// `TUR-AI-LOCAL-030` when the program is missing, `-035` for the text,
/// `-033` when synthesis failed or wrote no usable file.
pub fn synthesise(request: &Synthesis<'_>, text: &str) -> Result<WavInfo, (AiLocalCode, String)> {
    check_text(text)?;
    if !request.program.is_file() {
        return Err((
            AiLocalCode::SpeechEngineMissing,
            format!("{} is not there", request.program.display()),
        ));
    }
    let _ = fs::remove_file(request.output);
    let line = one_line(text);
    let output = run_program(
        request.program,
        &synthesis_arguments(request),
        &[("LANG", "C.UTF-8")],
        Some(line.as_bytes()),
        SYNTHESIS_TIMEOUT,
    )
    .map_err(|error| {
        (
            AiLocalCode::SpeechEngineMissing,
            format!("{}: {error}", request.program.display()),
        )
    })?;
    if output.timed_out || output.status != Some(0) {
        let _ = fs::remove_file(request.output);
        return Err((AiLocalCode::SynthesisFailed, failure_detail(&output, "piper")));
    }
    let info = wav_info(request.output).map_err(|detail| (AiLocalCode::SynthesisFailed, detail))?;
    if info.format != 1 || info.data_bytes == 0 {
        let _ = fs::remove_file(request.output);
        return Err((
            AiLocalCode::SynthesisFailed,
            "piper wrote no PCM sound".to_owned(),
        ));
    }
    Ok(info)
}

/// The threads recognition uses unless asked: the processors this program
/// may use, at most 8.
pub fn default_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|count| u32::try_from(count.get()).unwrap_or(1))
        .unwrap_or(1)
        .clamp(1, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, samples: usize) -> Vec<u8> {
        let frame = usize::from(channels) * usize::from(bits / 8);
        let data = samples * frame;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&u32::try_from(36 + data).unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(frame).unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::try_from(data).unwrap_or(0).to_le_bytes());
        bytes.resize(bytes.len() + data, 0);
        bytes
    }

    #[test]
    fn a_wave_header_is_read_and_held_to_what_whisper_takes() {
        let good = parse_wav(&wav(16_000, 1, 16, 16_000)).expect("a WAVE file");
        assert!(good.is_recognisable());
        assert!((good.seconds() - 1.0).abs() < 1e-9);
        assert!(!parse_wav(&wav(22_050, 1, 16, 100)).expect("WAVE").is_recognisable());
        assert!(!parse_wav(&wav(16_000, 2, 16, 100)).expect("WAVE").is_recognisable());
        assert!(!parse_wav(&wav(16_000, 1, 8, 100)).expect("WAVE").is_recognisable());
        assert!(!parse_wav(&wav(16_000, 1, 16, 0)).expect("WAVE").is_recognisable());
        assert!(parse_wav(b"OggS not a wave file").is_err());
        assert!(parse_wav(b"RIFF\x04\x00\x00\x00WAVE").is_err());
    }

    #[test]
    fn marks_for_silence_are_not_words() {
        assert_eq!(
            clean_transcript(" Apri le impostazioni.\n[BLANK_AUDIO]\n (silence)\n"),
            "Apri le impostazioni."
        );
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript(" Hello\n world \n"), "Hello world");
    }

    #[test]
    fn text_for_piper_is_one_line_and_bounded() {
        assert_eq!(one_line("Ciao,\nmondo\t!"), "Ciao, mondo !\n");
        assert!(check_text("   ").is_err());
        assert!(check_text(&"a".repeat(MAX_SPEAK_BYTES + 1)).is_err());
        assert!(check_text("Buongiorno.").is_ok());
    }

    #[test]
    fn the_command_lines_are_as_designed() {
        let request = Recognition {
            program: Path::new("/x/whisper-cli"),
            model: Path::new("/m/ggml-base-q5_1.bin"),
            vad_model: Some(Path::new("/m/ggml-silero-v6.2.0.bin")),
            audio: Path::new("/r/turn.wav"),
            language: "it",
            threads: 4,
        };
        assert_eq!(
            recognition_arguments(&request),
            [
                "-m",
                "/m/ggml-base-q5_1.bin",
                "-f",
                "/r/turn.wav",
                "-l",
                "it",
                "-t",
                "4",
                "-nt",
                "-np",
                "--vad",
                "-vm",
                "/m/ggml-silero-v6.2.0.bin"
            ]
        );
        let speak = Synthesis {
            program: Path::new("/p/piper"),
            voice: Path::new("/v/it_IT-paola-medium.onnx"),
            config: Path::new("/v/it_IT-paola-medium.onnx.json"),
            output: Path::new("/o/out.wav"),
        };
        assert_eq!(
            synthesis_arguments(&speak),
            [
                "-m",
                "/v/it_IT-paola-medium.onnx",
                "-c",
                "/v/it_IT-paola-medium.onnx.json",
                "-f",
                "/o/out.wav"
            ]
        );
    }
}

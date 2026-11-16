// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x's voice: the rules, the state of a voice turn, and the small pure
//! pieces the session service builds on (`docs/architecture/pax-voice.md`).
//!
//! Nothing here opens a microphone, plays a sound or runs a program; that is
//! [`crate::audio`]. Nothing here decides an action either: a spoken request
//! becomes an ordinary task that the policy engine judges like a typed one.
//! What this module adds are the rules that are about voice itself:
//!
//! * **Push-to-talk is the default**, and the only activation in this build
//!   ([`Activation`]). A wake word is off by default and cannot be turned on
//!   yet ([`VoiceSettings::apply`] refuses it).
//! * **The microphone is visibly on** whenever it listens: listening needs
//!   P@x's indicator or a caller that shows the microphone itself
//!   ([`may_listen`]).
//! * **Barge-in**: the person's key, the microphone button, Stop or (with
//!   echo cancellation) the person's voice stops P@x's speech
//!   ([`Machine`]).
//! * **Self-sound**: what is heard while P@x itself speaks is never
//!   recognised, and a turn heard while another program played sound is
//!   never sent by itself ([`Machine`], [`transcript_check`]).
//! * **The transcript first**: before anything above class 0 runs for a
//!   spoken request, what was heard has been on screen
//!   ([`TranscriptGate`]).
//! * **Class 3 never by voice; class 2 on screen** ([`approval`]).
//!
//! The words a person says are a request, never an approval: no path in
//! this build turns a transcript into a confirmation of anything.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::class::ActionClass;
use crate::faults::{Fault, TaskCode};

/// The rate of the audio P@x captures and recognises: 16 kHz, mono, 16-bit.
pub const SAMPLE_RATE: u32 = 16_000;

/// The longest voice turn. The microphone closes by itself after it.
pub const MAX_TURN: Duration = Duration::from_secs(30);

/// After speech, how much silence ends a turn started by the button.
pub const SILENCE_ENDS_TURN: Duration = Duration::from_millis(1200);

/// How long the transcript is shown before a spoken request may change
/// anything, by default.
pub const DEFAULT_SHOW_SECONDS: u32 = 2;

/// The longest a spoken task's first change waits for its transcript to be
/// sent by the person. After it, the change is not made.
pub const TRANSCRIPT_WAIT_MAX: Duration = Duration::from_secs(120);

/// How often the loudness is reported while the microphone is open.
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(100);

/// A live session ends by itself after this much silence with no task
/// running, by default.
pub const DEFAULT_SILENCE_TIMEOUT_SECONDS: u32 = 60;

/// The version of the consent text for a provider's live engine.
pub const VOICE_CONSENT_VERSION: &str = "1";

/// The longest text P@x is asked to say at once, in bytes.
pub const SPEAK_MAX_BYTES: usize = 4000;

/// What the wake word setting will say it listens to. Shown beside the
/// setting; the detector itself is not built in this version.
pub const WAKE_WORD_EXPLANATION: &str = "When the wake word is on, the microphone stays open \
while P@x is switched on and your session is unlocked. This computer listens for the words \
\"Hey P@x\" and for nothing else: the sound is examined in memory, a second at a time, and \
thrown away. Nothing is recorded or sent anywhere until the wake word is heard; then P@x \
listens to your request as if you had pressed the talk key, and the microphone is shown as \
on. The wake word is not available in this version of TUR.";

/// The languages P@x offers for speech, beside `auto`.
pub const LANGUAGES: &[&str] = &["en", "it", "de", "es", "fr"];

/// Words that stop the running task when they are all a person says.
pub const STOP_WORDS: &[&str] = &[
    "stop", "cancel", "stop it", "halt", "ferma", "fermati", "basta", "annulla", "stopp",
    "abbrechen", "para", "detente", "cancela", "arrête", "annule",
];

/// Words that let a paused task go on when they are all a person says.
pub const RESUME_WORDS: &[&str] = &[
    "continue", "go on", "resume", "carry on", "continua", "vai avanti", "riprendi",
    "weiter", "continúa", "sigue", "continue s'il te plaît", "continuez",
];

// ------------------------------------------------------------------ settings

/// How a voice turn is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Activation {
    /// The person holds P@x's key or presses the microphone. The default,
    /// and the only activation in this build.
    PushToTalk,
}

/// What happens to a transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    /// Shown, and sent: the task starts at once and its first change waits
    /// until the transcript has been on screen for the settings' seconds.
    Show,
    /// Shown, and kept until the person sends it.
    Wait,
}

impl Check {
    /// The value in documents.
    pub const fn as_str(self) -> &'static str {
        match self {
            Check::Show => "show",
            Check::Wait => "wait",
        }
    }
}

/// The recognition settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recognition {
    /// A model of local AI's speech catalogue, or `None` for the one local
    /// AI recommends for this computer.
    #[serde(default)]
    pub model: Option<String>,
}

/// The speech settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synthesis {
    /// A voice per language; a language not named takes its default voice.
    #[serde(default)]
    pub voices: std::collections::BTreeMap<String, String>,
}

/// The person's acceptance of a provider's live engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveConsent {
    /// The live provider's identifier.
    pub provider: String,
    /// The version of the text that was shown.
    pub version: String,
    /// When, in seconds since 1970.
    pub at: u64,
}

/// The live conversation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSettings {
    /// `local`, or a live provider's identifier.
    #[serde(default = "local_engine")]
    pub engine: String,
    /// The provider's model, or `None` for its entry's first.
    #[serde(default)]
    pub model: Option<String>,
    /// Silence, with no task running, that ends a session.
    #[serde(default = "default_silence_timeout")]
    pub silence_timeout_seconds: u32,
    /// Whether P@x says what it is doing while it works.
    #[serde(default = "yes")]
    pub narrate: bool,
    /// The consent to a provider's live engine, if one was given.
    #[serde(default)]
    pub consent: Option<LiveConsent>,
}

fn local_engine() -> String {
    "local".to_owned()
}

const fn default_silence_timeout() -> u32 {
    DEFAULT_SILENCE_TIMEOUT_SECONDS
}

const fn yes() -> bool {
    true
}

const fn default_show_seconds() -> u32 {
    DEFAULT_SHOW_SECONDS
}

const fn schema_version() -> u32 {
    1
}

fn auto() -> String {
    "auto".to_owned()
}

const fn push_to_talk() -> Activation {
    Activation::PushToTalk
}

const fn show() -> Check {
    Check::Show
}

impl Default for LiveSettings {
    fn default() -> Self {
        LiveSettings {
            engine: local_engine(),
            model: None,
            silence_timeout_seconds: DEFAULT_SILENCE_TIMEOUT_SECONDS,
            narrate: true,
            consent: None,
        }
    }
}

/// The voice settings (`voice.json` beside P@x's settings;
/// `schemas/pax-voice-settings.schema.json`). Everything is off until the
/// person switches it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSettings {
    /// Always 1.
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    /// Whether the person switched voice on.
    #[serde(default)]
    pub enabled: bool,
    /// `auto`, or one of [`LANGUAGES`].
    #[serde(default = "auto")]
    pub language: String,
    /// Recognition.
    #[serde(default)]
    pub recognition: Recognition,
    /// Speech.
    #[serde(default)]
    pub synthesis: Synthesis,
    /// Whether P@x's answers are spoken.
    #[serde(default)]
    pub spoken_replies: bool,
    /// How a turn is started.
    #[serde(default = "push_to_talk")]
    pub activation: Activation,
    /// Whether a wake word is listened for. Off, and cannot be turned on in
    /// this build.
    #[serde(default)]
    pub wake_word: bool,
    /// What happens to a transcript of a push-to-talk turn.
    #[serde(default = "show")]
    pub transcript_check: Check,
    /// How long a transcript is on screen before a spoken request may change
    /// anything.
    #[serde(default = "default_show_seconds")]
    pub show_seconds: u32,
    /// Live conversation.
    #[serde(default)]
    pub live: LiveSettings,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            schema_version: 1,
            enabled: false,
            language: auto(),
            recognition: Recognition::default(),
            synthesis: Synthesis::default(),
            spoken_replies: false,
            activation: Activation::PushToTalk,
            wake_word: false,
            transcript_check: Check::Show,
            show_seconds: DEFAULT_SHOW_SECONDS,
            live: LiveSettings::default(),
        }
    }
}

fn invalid(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceInvalidRequest, detail)
}

/// A catalogue identifier: 1 to 64 lower-case letters, digits, `.`, `-`
/// and `_`, beginning with a letter or digit.
pub fn is_catalogue_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'))
}

/// A voice of local AI's speech catalogue, such as `voice-it-paola-medium`.
pub fn is_voice_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl VoiceSettings {
    /// Read the settings file. A missing file is the defaults.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the file is there and cannot be read or is
    /// not valid; voice is then off.
    pub fn load(path: &Path) -> Result<VoiceSettings, Fault> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let value = crate::strict::value_from_str(&text).map_err(|_| {
                    Fault::new(
                        TaskCode::ServiceStorage,
                        format!("{}: not one JSON object", path.display()),
                    )
                })?;
                let settings: VoiceSettings = serde_json::from_value(value).map_err(|error| {
                    Fault::new(
                        TaskCode::ServiceStorage,
                        format!("{}: {error}", path.display()),
                    )
                })?;
                settings.check().map_err(|fault| {
                    Fault::new(
                        TaskCode::ServiceStorage,
                        format!("{}: {}", path.display(), fault.detail),
                    )
                })?;
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(VoiceSettings::default())
            }
            Err(error) => Err(Fault::new(
                TaskCode::ServiceStorage,
                format!("{}: {error}", path.display()),
            )),
        }
    }

    /// Write the settings file, readable by its owner only.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009`.
    pub fn save(&self, path: &Path) -> Result<(), Fault> {
        let mut text = serde_json::to_string_pretty(self)
            .map_err(|error| Fault::new(TaskCode::ServiceStorage, error.to_string()))?;
        text.push('\n');
        crate::settings::write_private(path, text.as_bytes())
    }

    /// Whether the settings are possible.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` with what is wrong; `TUR-AI-VOICE-005` for a
    /// wake word.
    pub fn check(&self) -> Result<(), Fault> {
        if self.schema_version != 1 {
            return Err(invalid("the voice settings' version is not 1"));
        }
        if self.language != "auto" && !LANGUAGES.contains(&self.language.as_str()) {
            return Err(invalid("that language is not offered for speech"));
        }
        if let Some(model) = &self.recognition.model
            && !is_catalogue_id(model)
        {
            return Err(invalid("a recognition model is named wrongly"));
        }
        for (language, voice) in &self.synthesis.voices {
            if !LANGUAGES.contains(&language.as_str()) || !is_voice_id(voice) {
                return Err(invalid("a voice is named wrongly, or for a language not offered"));
            }
        }
        if self.wake_word {
            return Err(Fault::new(
                TaskCode::VoiceWakeWordUnavailable,
                TaskCode::VoiceWakeWordUnavailable.user_message(),
            ));
        }
        if !(1..=10).contains(&self.show_seconds) {
            return Err(invalid("the transcript is shown for 1 to 10 seconds"));
        }
        if !(10..=3600).contains(&self.live.silence_timeout_seconds) {
            return Err(invalid("a live conversation ends after 10 to 3600 seconds of silence"));
        }
        if self.live.engine != "local" && !crate::settings::is_provider_id(&self.live.engine) {
            return Err(invalid("the live engine is named wrongly"));
        }
        if let Some(model) = &self.live.model
            && !crate::settings::is_model_name(model)
        {
            return Err(invalid("the live model is named wrongly"));
        }
        Ok(())
    }

    /// Apply a change: a JSON object with any member of the settings, where
    /// `null` restores that member's default. `live` and `recognition` and
    /// `synthesis` are changed member by member. Consent is not taken from
    /// here: [`VoiceSettings::accept_live`] records it.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` for an unknown member or a value of the wrong
    /// kind, or anything [`VoiceSettings::check`] refuses. The settings are
    /// unchanged then.
    pub fn apply(&self, change: &Map<String, Value>) -> Result<VoiceSettings, Fault> {
        let mut current = serde_json::to_value(self)
            .map_err(|error| Fault::new(TaskCode::ServiceStorage, error.to_string()))?;
        let defaults = serde_json::to_value(VoiceSettings::default())
            .map_err(|error| Fault::new(TaskCode::ServiceStorage, error.to_string()))?;
        let Some(object) = current.as_object_mut() else {
            return Err(invalid("the voice settings are not an object"));
        };
        for (name, value) in change {
            if name == "schema_version" || name == "live_consent_version" {
                continue;
            }
            if !object.contains_key(name) {
                return Err(invalid(format!("the voice settings have no member {name}")));
            }
            let nested = matches!(name.as_str(), "live" | "recognition" | "synthesis");
            match (nested, value) {
                (_, Value::Null) => {
                    object.insert(name.clone(), defaults[name.as_str()].clone());
                }
                (true, Value::Object(members)) => {
                    let Some(inner) = object.get_mut(name).and_then(Value::as_object_mut) else {
                        return Err(invalid("the voice settings are not as expected"));
                    };
                    for (member, member_value) in members {
                        if name == "live" && member == "consent" {
                            return Err(invalid(
                                "consent to a live provider is given with live_consent_version",
                            ));
                        }
                        if !inner.contains_key(member) {
                            return Err(invalid(format!("{name} has no member {member}")));
                        }
                        let replaced = if member_value.is_null() {
                            defaults[name.as_str()][member.as_str()].clone()
                        } else {
                            member_value.clone()
                        };
                        inner.insert(member.clone(), replaced);
                    }
                }
                (true, _) => return Err(invalid(format!("{name} must be an object"))),
                (false, other) => {
                    object.insert(name.clone(), other.clone());
                }
            }
        }
        let changed: VoiceSettings = serde_json::from_value(current)
            .map_err(|error| invalid(format!("the voice settings are not valid: {error}")))?;
        changed.check()?;
        Ok(changed)
    }

    /// Record the person's acceptance of a provider's live engine, for the
    /// version of the text they were shown.
    ///
    /// # Errors
    ///
    /// `TUR-AI-VOICE-007` if the version is not the current one.
    pub fn accept_live(&mut self, provider: &str, version: &str, at: u64) -> Result<(), Fault> {
        if version != VOICE_CONSENT_VERSION {
            return Err(Fault::new(
                TaskCode::VoiceLiveConsent,
                "the consent text shown is not the current one",
            ));
        }
        self.live.consent = Some(LiveConsent {
            provider: provider.to_owned(),
            version: version.to_owned(),
            at,
        });
        Ok(())
    }

    /// Whether the person has accepted the current text for this live
    /// provider. The local engine needs none.
    pub fn live_consent_current(&self, engine: &str) -> bool {
        engine == "local"
            || self.live.consent.as_ref().is_some_and(|consent| {
                consent.provider == engine && consent.version == VOICE_CONSENT_VERSION
            })
    }
}

/// The text a person reads before a provider's live engine is used.
pub fn live_consent_text(company: &str, terms_url: Option<&str>) -> String {
    let terms = terms_url.map_or_else(
        || format!("{company}'s terms are not reviewed by TUR."),
        |url| format!("Its terms are at {url}."),
    );
    format!(
        "In a live conversation with this engine, what your microphone picks up is streamed to \
         {company} while the conversation lasts, with P@x's instructions and the results of what \
         P@x does, so that {company}'s model can listen and answer out loud. {company} bills it \
         to your own key, by the minute of audio, within the spending limit you set for P@x. \
         {terms} The microphone is shown as on for the whole conversation, and the conversation \
         ends when you end it, when you press Stop, or after a silence. P@x still asks you on \
         screen before anything that cannot be undone, and never acts on a spoken yes for \
         anything that could harm this computer. A conversation with the local engine sends \
         nothing anywhere."
    )
}

// ------------------------------------------------------------------- triggers

/// What started a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The person held P@x's key.
    PushToTalk,
    /// The person pressed the microphone.
    Button,
    /// The wake word was heard. Not built; kept so that the rules that
    /// treat it more strictly are written and tested.
    WakeWord,
    /// A live conversation.
    Live,
}

impl Trigger {
    /// The value in documents and in the audit trail.
    pub const fn as_str(self) -> &'static str {
        match self {
            Trigger::PushToTalk => "push-to-talk",
            Trigger::Button => "button",
            Trigger::WakeWord => "wake-word",
            Trigger::Live => "live",
        }
    }

    /// Read the value a caller gave. A wake word cannot be claimed by a
    /// caller.
    pub fn from_caller(text: &str) -> Option<Trigger> {
        match text {
            "push-to-talk" | "" => Some(Trigger::PushToTalk),
            "button" => Some(Trigger::Button),
            _ => None,
        }
    }

    /// Whether the person physically started this turn at the computer.
    pub const fn by_the_person(self) -> bool {
        matches!(self, Trigger::PushToTalk | Trigger::Button)
    }
}

/// Where a request came from, as the audit trail says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Typed.
    Typed,
    /// Spoken, and sent as it was heard.
    Voice,
    /// Spoken, and changed by the person before it was sent.
    VoiceEdited,
    /// Said in a live conversation.
    Live,
}

impl Source {
    /// The value of `TUR_AI_SOURCE`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::Typed => "typed",
            Source::Voice => "voice",
            Source::VoiceEdited => "voice_edited",
            Source::Live => "live",
        }
    }

    /// Whether the words came from a microphone and were not checked by the
    /// person's own typing.
    pub const fn heard(self) -> bool {
        matches!(self, Source::Voice | Source::Live)
    }
}

// --------------------------------------------------------------- visibility

/// Who shows that the microphone is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    /// P@x's indicator shows "Listening…" under P@x's lease.
    Overlay,
    /// The program that asked shows the lit microphone itself (the ask
    /// bar). Any program of the person can say so; see pax-voice.md.
    Caller,
    /// Nothing shows it.
    None,
}

impl Indicator {
    /// The value in documents.
    pub const fn as_str(self) -> &'static str {
        match self {
            Indicator::Overlay => "overlay",
            Indicator::Caller => "caller",
            Indicator::None => "none",
        }
    }
}

/// Whether listening may begin: the microphone is only ever open while it
/// is shown as open. A live session must be shown by P@x's indicator,
/// because it lasts and the window may be closed while it does.
///
/// # Errors
///
/// `TUR-AI-VOICE-004` if nothing would show it.
pub fn may_listen(trigger: Trigger, indicator: Indicator) -> Result<(), Fault> {
    let shown = matches!(
        (trigger, indicator),
        (_, Indicator::Overlay) | (Trigger::PushToTalk | Trigger::Button, Indicator::Caller)
    );
    if shown {
        Ok(())
    } else {
        Err(Fault::new(
            TaskCode::VoiceNotVisible,
            TaskCode::VoiceNotVisible.user_message(),
        ))
    }
}

// ------------------------------------------------------------ the transcript

/// Why a transcript waits for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    /// The settings say to wait.
    Setting,
    /// Another program was playing sound while the person spoke: the words
    /// may not be the person's.
    OtherSound,
    /// A task is running; P@x does one at a time.
    TaskRunning,
    /// The turn was not started by the person at the computer.
    NotByThePerson,
}

impl WaitReason {
    /// The value in documents.
    pub const fn as_str(self) -> &'static str {
        match self {
            WaitReason::Setting => "setting",
            WaitReason::OtherSound => "other_sound",
            WaitReason::TaskRunning => "task_running",
            WaitReason::NotByThePerson => "not_by_the_person",
        }
    }
}

/// What happens to a transcript: sent at once (shown, and changes wait for
/// it to have been on screen), or kept until the person sends it. Sound
/// from another program, a turn the person did not start at the computer,
/// and a running task always keep it.
pub fn transcript_check(
    settings: &VoiceSettings,
    trigger: Trigger,
    other_sound: bool,
    task_running: bool,
) -> (Check, Option<WaitReason>) {
    if task_running {
        (Check::Wait, Some(WaitReason::TaskRunning))
    } else if other_sound {
        (Check::Wait, Some(WaitReason::OtherSound))
    } else if !trigger.by_the_person() {
        (Check::Wait, Some(WaitReason::NotByThePerson))
    } else if settings.transcript_check == Check::Wait {
        (Check::Wait, Some(WaitReason::Setting))
    } else {
        (Check::Show, None)
    }
}

/// What the gate of a spoken task says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The transcript has been shown long enough, or the person sent it:
    /// changes may run.
    Open,
    /// Not yet.
    Shut,
    /// The person took the request back: no change runs.
    Withdrawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateState {
    Until(Instant),
    Held,
    Open,
    Withdrawn,
}

/// The gate a spoken task's changes wait at: shut until its transcript has
/// been on screen for long enough, or until the person sent it. Class 0
/// never waits; the task loop asks the gate before a class 1 action.
#[derive(Debug)]
pub struct TranscriptGate {
    state: Mutex<GateState>,
}

impl TranscriptGate {
    /// A gate that opens by itself once the transcript has been shown for
    /// `shown_for` from now.
    pub fn shown_for(shown_for: Duration) -> Arc<TranscriptGate> {
        Arc::new(TranscriptGate {
            state: Mutex::new(GateState::Until(Instant::now() + shown_for)),
        })
    }

    /// A gate that opens only when [`TranscriptGate::open`] is called.
    pub fn held() -> Arc<TranscriptGate> {
        Arc::new(TranscriptGate {
            state: Mutex::new(GateState::Held),
        })
    }

    /// A gate already open: the person sent what they saw.
    pub fn sent() -> Arc<TranscriptGate> {
        Arc::new(TranscriptGate {
            state: Mutex::new(GateState::Open),
        })
    }

    /// The person sent the transcript.
    pub fn open(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if *state != GateState::Withdrawn {
            *state = GateState::Open;
        }
    }

    /// The person took the request back. There is no way back.
    pub fn withdraw(&self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = GateState::Withdrawn;
    }

    /// What the gate says now.
    pub fn state(&self) -> Gate {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match *state {
            GateState::Until(when) if Instant::now() >= when => {
                *state = GateState::Open;
                Gate::Open
            }
            GateState::Until(_) | GateState::Held => Gate::Shut,
            GateState::Open => Gate::Open,
            GateState::Withdrawn => Gate::Withdrawn,
        }
    }
}

/// What a person said during a task, waiting to be added to the
/// conversation at the task's next step (a live conversation with the local
/// engine: "redirect", "and also…").
#[derive(Debug, Default, Clone)]
pub struct Interjections {
    said: Arc<Mutex<Vec<String>>>,
}

impl Interjections {
    /// An empty queue.
    pub fn new() -> Self {
        Interjections::default()
    }

    /// Add what the person said.
    pub fn push(&self, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.said
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(text.to_owned());
        }
    }

    /// Take everything said so far.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.said.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// What a person's words mean while a task runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interjection {
    /// Stop the task.
    Stop,
    /// Let it go on.
    Resume,
    /// Anything else.
    Other,
}

fn normalise(text: &str) -> String {
    text.trim()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim()
        .to_lowercase()
}

/// Whether the words are one of the stop or resume words, and nothing more.
pub fn interjection(text: &str) -> Interjection {
    let words = normalise(text);
    if STOP_WORDS.contains(&words.as_str()) {
        Interjection::Stop
    } else if RESUME_WORDS.contains(&words.as_str()) {
        Interjection::Resume
    } else {
        Interjection::Other
    }
}

// --------------------------------------------------------------- approvals

/// How an action of a spoken request may be approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceApproval {
    /// It needs no approval (classes 0 and 1).
    NotNeeded,
    /// On screen only.
    OnScreen,
    /// By voice or on screen: only when the policy allows spoken approval,
    /// the request was started by push-to-talk in the same exchange, and no
    /// other sound was playing.
    VoiceOrScreen,
    /// Never by voice, whatever the settings: classes 3 and 4.
    Never,
}

/// The circumstances of a spoken approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpokenApproval {
    /// The person's policy allows spoken approval of class 2. It is a
    /// setting of the policy store, which does not exist yet: always false
    /// in this build.
    pub policy_allows: bool,
    /// The request was started by the person's key in the same exchange.
    pub push_to_talk_same_exchange: bool,
    /// Another program was playing sound while the approval was heard.
    pub other_sound: bool,
    /// P@x itself was speaking while it was heard.
    pub own_sound: bool,
}

impl SpokenApproval {
    /// Nothing allows a spoken approval.
    pub const NONE: SpokenApproval = SpokenApproval {
        policy_allows: false,
        push_to_talk_same_exchange: false,
        other_sound: false,
        own_sound: false,
    };
}

/// The rule of ai-control-layer.md sections 5.2 and 9: class 3 never
/// accepts voice; class 2 is approved on screen unless spoken approval is
/// allowed and the conditions hold.
pub fn approval(class: ActionClass, spoken: SpokenApproval) -> VoiceApproval {
    match class {
        ActionClass::Observe | ActionClass::Reversible => VoiceApproval::NotNeeded,
        ActionClass::Consequential => {
            if spoken.policy_allows
                && spoken.push_to_talk_same_exchange
                && !spoken.other_sound
                && !spoken.own_sound
            {
                VoiceApproval::VoiceOrScreen
            } else {
                VoiceApproval::OnScreen
            }
        }
        ActionClass::SystemCritical | ActionClass::Prohibited => VoiceApproval::Never,
    }
}

// ----------------------------------------------------------- the voice turn

/// The voice state, as `Voice` and `VoiceStateChanged` say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Voice is off, or P@x is.
    Off,
    /// Voice is on and cannot work (no models, no microphone program).
    Unavailable,
    /// Ready to listen.
    Ready,
    /// The microphone is open for a turn.
    Listening {
        /// The turn.
        turn: String,
        /// What started it.
        trigger: Trigger,
    },
    /// The microphone is closed; what was heard is being recognised.
    Recognising {
        /// The turn.
        turn: String,
    },
    /// The transcript waits for the person to send it.
    Waiting {
        /// The turn.
        turn: String,
    },
    /// P@x is speaking.
    Speaking,
    /// A live conversation; the microphone is open throughout.
    Live {
        /// The session.
        session: String,
    },
}

impl Phase {
    /// The name.
    pub const fn name(&self) -> &'static str {
        match self {
            Phase::Off => "off",
            Phase::Unavailable => "unavailable",
            Phase::Ready => "ready",
            Phase::Listening { .. } => "listening",
            Phase::Recognising { .. } => "recognising",
            Phase::Waiting { .. } => "waiting",
            Phase::Speaking => "speaking",
            Phase::Live { .. } => "live",
        }
    }

    /// Whether the microphone is open.
    pub const fn microphone_open(&self) -> bool {
        matches!(self, Phase::Listening { .. } | Phase::Live { .. })
    }
}

/// Something that happens to the voice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A person asked to listen.
    Listen {
        /// The new turn.
        turn: String,
        /// What started it.
        trigger: Trigger,
    },
    /// The turn ends: the person let go, pressed again, or it ran out.
    /// `send` recognises what was heard; otherwise it is dropped.
    Release {
        /// The turn.
        turn: String,
        /// Whether to recognise it.
        send: bool,
    },
    /// Recognition ended. `waits` says the transcript is kept for the
    /// person.
    Recognised {
        /// The turn.
        turn: String,
        /// Whether it waits for the person.
        waits: bool,
    },
    /// The person sent or dropped a waiting transcript.
    Settled {
        /// The turn.
        turn: String,
    },
    /// P@x begins to speak.
    SpeakStart,
    /// P@x's speech ended.
    SpeakEnd,
    /// Speech was detected on the microphone while P@x spoke.
    VoiceWhileSpeaking {
        /// Whether the capture is echo-cancelled, so that what was heard
        /// can be the person and not P@x itself.
        echo_cancelled: bool,
    },
    /// A live session begins.
    LiveStart {
        /// The session.
        session: String,
    },
    /// The live session ends.
    LiveEnd,
    /// Stop: the stop key, Stop, Esc, a locked session, P@x switched off.
    Stop,
}

/// What the service must do because of an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Open the microphone.
    OpenMicrophone,
    /// Close the microphone and keep what was heard, to recognise it.
    CloseAndRecognise,
    /// Close the microphone and throw away what was heard.
    CloseAndDrop,
    /// Stop P@x's speech now.
    StopSpeech,
    /// Pause the running task while the person speaks.
    PauseTask,
    /// Throw away what the microphone heard while P@x spoke.
    DropOwnSound,
}

/// The voice's state machine. It holds the rules of push-to-talk,
/// barge-in and self-sound; the service performs its effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    phase: Phase,
    /// The phase to go back to when speech ends (a live session keeps the
    /// microphone open while P@x speaks).
    speaking_in_live: Option<String>,
}

impl Machine {
    /// A machine in `phase`: `Off`, `Unavailable` or `Ready`.
    pub fn new(phase: Phase) -> Machine {
        Machine {
            phase,
            speaking_in_live: None,
        }
    }

    /// The phase now.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Whether P@x is speaking, alone or in a live session.
    pub fn speaking(&self) -> bool {
        self.phase == Phase::Speaking || self.speaking_in_live.is_some()
    }

    /// Set the resting phase (voice switched on or off, models found or
    /// not). Ignored while a turn or a session is under way, except `Off`,
    /// which ends everything.
    pub fn rest(&mut self, phase: Phase) -> Vec<Effect> {
        if phase == Phase::Off {
            let effects = self.step(Input::Stop);
            self.phase = Phase::Off;
            self.speaking_in_live = None;
            return effects;
        }
        if matches!(self.phase, Phase::Off | Phase::Unavailable | Phase::Ready) {
            self.phase = phase;
        }
        Vec::new()
    }

    /// Apply one input. Returns the effects, in order. An input that does
    /// not apply now changes nothing and returns no effect.
    pub fn step(&mut self, input: Input) -> Vec<Effect> {
        let mut effects = Vec::new();
        match (self.phase.clone(), input) {
            // Push-to-talk, or the button. Barge-in: speaking stops first.
            (Phase::Ready, Input::Listen { turn, trigger }) => {
                self.phase = Phase::Listening { turn, trigger };
                effects.push(Effect::OpenMicrophone);
            }
            (Phase::Speaking, Input::Listen { turn, trigger }) if trigger.by_the_person() => {
                effects.push(Effect::StopSpeech);
                self.phase = Phase::Listening { turn, trigger };
                effects.push(Effect::OpenMicrophone);
            }
            (Phase::Waiting { .. }, Input::Listen { turn, trigger }) if trigger.by_the_person() => {
                // A new turn replaces a transcript that was never sent.
                self.phase = Phase::Listening { turn, trigger };
                effects.push(Effect::OpenMicrophone);
            }
            (Phase::Listening { turn, .. }, Input::Release { turn: ended, send })
                if turn == ended =>
            {
                if send {
                    self.phase = Phase::Recognising { turn };
                    effects.push(Effect::CloseAndRecognise);
                } else {
                    self.phase = Phase::Ready;
                    effects.push(Effect::CloseAndDrop);
                }
            }
            (Phase::Recognising { turn }, Input::Recognised { turn: done, waits })
                if turn == done =>
            {
                self.phase = if waits {
                    Phase::Waiting { turn }
                } else {
                    Phase::Ready
                };
            }
            (Phase::Waiting { turn }, Input::Settled { turn: done }) if turn == done => {
                self.phase = Phase::Ready;
            }
            // P@x speaks only when nothing listens for a turn.
            (Phase::Ready, Input::SpeakStart) => self.phase = Phase::Speaking,
            (Phase::Live { session }, Input::SpeakStart) => {
                self.speaking_in_live = Some(session);
            }
            (Phase::Speaking, Input::SpeakEnd) => self.phase = Phase::Ready,
            (Phase::Live { .. }, Input::SpeakEnd) => self.speaking_in_live = None,
            // Self-sound: without echo cancellation, the microphone hears
            // P@x. What it heard is dropped, and P@x goes on speaking.
            (Phase::Speaking, Input::VoiceWhileSpeaking { .. }) => {
                // Outside a live session the microphone is closed while P@x
                // speaks; nothing was heard.
            }
            (Phase::Live { .. }, Input::VoiceWhileSpeaking { echo_cancelled }) => {
                if self.speaking_in_live.is_some() {
                    if echo_cancelled {
                        self.speaking_in_live = None;
                        effects.push(Effect::StopSpeech);
                    } else {
                        effects.push(Effect::DropOwnSound);
                    }
                }
            }
            (Phase::Ready | Phase::Speaking | Phase::Waiting { .. }, Input::LiveStart { session }) => {
                if self.phase == Phase::Speaking {
                    effects.push(Effect::StopSpeech);
                }
                self.phase = Phase::Live { session };
                effects.push(Effect::OpenMicrophone);
            }
            (Phase::Live { .. }, Input::LiveEnd | Input::Stop) => {
                if self.speaking_in_live.take().is_some() {
                    effects.push(Effect::StopSpeech);
                }
                self.phase = Phase::Ready;
                effects.push(Effect::CloseAndDrop);
            }
            (Phase::Listening { .. }, Input::Stop) => {
                self.phase = Phase::Ready;
                effects.push(Effect::CloseAndDrop);
            }
            (Phase::Speaking, Input::Stop) => {
                self.phase = Phase::Ready;
                effects.push(Effect::StopSpeech);
            }
            (Phase::Recognising { .. } | Phase::Waiting { .. }, Input::Stop) => {
                self.phase = Phase::Ready;
            }
            _ => {}
        }
        effects
    }
}

/// Whether a task should be paused while the person speaks: a turn started
/// while a task runs pauses it, so that P@x does not act while it is being
/// spoken to.
pub const fn pause_task_while_listening(task_running: bool) -> bool {
    task_running
}

// ---------------------------------------------------------- other programs

/// Whether a program other than P@x is playing sound, from what `pw-dump`
/// printed: a node of class `Stream/Output/Audio` in the state `running`
/// whose process is not `own_pid` and whose application is not P@x.
/// Something that cannot be read counts as sound playing: the stricter
/// reading.
pub fn other_sound_playing(pw_dump: &str, own_pid: u32) -> bool {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(pw_dump) else {
        return true;
    };
    objects.iter().any(|object| {
        if object.get("type").and_then(Value::as_str) != Some("PipeWire:Interface:Node") {
            return false;
        }
        let info = object.get("info");
        let props = info.and_then(|info| info.get("props"));
        let class = props
            .and_then(|props| props.get("media.class"))
            .and_then(Value::as_str);
        let state = info
            .and_then(|info| info.get("state"))
            .and_then(Value::as_str);
        if class != Some("Stream/Output/Audio") || state != Some("running") {
            return false;
        }
        let pid = props
            .and_then(|props| props.get("application.process.id"))
            .and_then(|pid| {
                pid.as_u64()
                    .or_else(|| pid.as_str().and_then(|text| text.parse().ok()))
            });
        let name = props
            .and_then(|props| props.get("application.name"))
            .and_then(Value::as_str);
        let ours = pid == Some(u64::from(own_pid)) || name == Some(OWN_APPLICATION_NAME);
        !ours
    })
}

/// The application name P@x's own audio streams carry.
pub const OWN_APPLICATION_NAME: &str = "P@x";

/// Whether the default capture source is PipeWire's echo canceller, from
/// what `pw-dump` printed: a node of class `Audio/Source` whose name or
/// factory says echo-cancel.
pub fn echo_cancelled_source(pw_dump: &str) -> bool {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(pw_dump) else {
        return false;
    };
    objects.iter().any(|object| {
        let props = object.get("info").and_then(|info| info.get("props"));
        let text = |key: &str| {
            props
                .and_then(|props| props.get(key))
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        text("media.class") == "Audio/Source"
            && (text("node.name").contains("echo-cancel")
                || text("factory.name").contains("echo-cancel")
                || text("library.name").contains("echo-cancel"))
    })
}

// ----------------------------------------------------------------- audio

/// A WAV file of 16-bit mono samples.
pub fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX - 36);
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// The samples and rate of a WAV file of 16-bit mono PCM, as `wav_bytes`
/// writes and Piper writes. `None` for anything else.
pub fn wav_samples(bytes: &[u8]) -> Option<(Vec<i16>, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut rate = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = at + 8;
        let end = body.checked_add(len)?.min(bytes.len());
        if id == b"fmt " {
            if end < body + 16 {
                return None;
            }
            let format = u16::from_le_bytes(bytes[body..body + 2].try_into().ok()?);
            let channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().ok()?);
            let bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().ok()?);
            if format != 1 || channels != 1 || bits != 16 {
                return None;
            }
            rate = Some(u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?));
        } else if id == b"data" {
            let rate = rate?;
            let samples = bytes[body..end]
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            return Some((samples, rate));
        }
        at = body + len + (len % 2);
    }
    None
}

/// The loudness of some samples, 0 to 1: the root mean square against full
/// scale, scaled so that ordinary speech reads about 0.3 to 0.8.
pub fn level(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|sample| {
            let value = f64::from(*sample) / 32768.0;
            value * value
        })
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let rms = (sum / samples.len() as f64).sqrt();
    (rms * 4.0).min(1.0)
}

/// Something the segmenter found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Speech began.
    SpeechStarted,
    /// An utterance ended: its samples, with a little before the start.
    Utterance(Vec<i16>),
}

/// Splits a stream of 16 kHz samples into utterances by loudness: speech
/// begins after `start_frames` loud 20 ms frames in a row and ends after
/// `end_silence` of quiet. For the local live engine, where the
/// recogniser takes one utterance at a time. Simple on purpose: Silero's
/// voice activity detection in the recogniser removes what is not speech.
#[derive(Debug, Clone)]
pub struct Segmenter {
    threshold: f64,
    start_frames: usize,
    end_frames: usize,
    loud_run: usize,
    quiet_run: usize,
    in_speech: bool,
    pending: Vec<i16>,
    current: Vec<i16>,
    before: std::collections::VecDeque<i16>,
    max_samples: usize,
}

const FRAME: usize = (SAMPLE_RATE as usize) / 50;

impl Segmenter {
    /// A segmenter with the defaults: level 0.06, 3 frames to start, 700 ms
    /// of quiet to end, at most 30 s an utterance.
    pub fn new() -> Segmenter {
        Segmenter::with(0.06, 3, Duration::from_millis(700))
    }

    /// A segmenter with these settings.
    pub fn with(threshold: f64, start_frames: usize, end_silence: Duration) -> Segmenter {
        let end_frames = usize::try_from(end_silence.as_millis() / 20).unwrap_or(35).max(1);
        Segmenter {
            threshold,
            start_frames: start_frames.max(1),
            end_frames,
            loud_run: 0,
            quiet_run: 0,
            in_speech: false,
            pending: Vec::new(),
            current: Vec::new(),
            before: std::collections::VecDeque::new(),
            max_samples: SAMPLE_RATE as usize * MAX_TURN.as_secs() as usize,
        }
    }

    /// Whether speech is under way.
    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    /// Throw away everything held: what the microphone heard while P@x
    /// spoke.
    pub fn reset(&mut self) {
        self.loud_run = 0;
        self.quiet_run = 0;
        self.in_speech = false;
        self.pending.clear();
        self.current.clear();
        self.before.clear();
    }

    /// Add samples; returns what was found.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Heard> {
        let mut found = Vec::new();
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= FRAME {
            let frame: Vec<i16> = self.pending.drain(..FRAME).collect();
            let loud = level(&frame) >= self.threshold;
            if self.in_speech {
                self.current.extend_from_slice(&frame);
                if loud {
                    self.quiet_run = 0;
                } else {
                    self.quiet_run += 1;
                }
                if self.quiet_run >= self.end_frames || self.current.len() >= self.max_samples {
                    self.in_speech = false;
                    self.quiet_run = 0;
                    self.loud_run = 0;
                    found.push(Heard::Utterance(std::mem::take(&mut self.current)));
                }
            } else {
                self.before.extend(frame.iter().copied());
                // Keep 300 ms before the start.
                while self.before.len() > FRAME * 15 {
                    self.before.pop_front();
                }
                if loud {
                    self.loud_run += 1;
                } else {
                    self.loud_run = 0;
                }
                if self.loud_run >= self.start_frames {
                    self.in_speech = true;
                    self.quiet_run = 0;
                    self.current = self.before.drain(..).collect();
                    found.push(Heard::SpeechStarted);
                }
            }
        }
        found
    }
}

impl Default for Segmenter {
    fn default() -> Self {
        Segmenter::new()
    }
}

/// Sentences of a text that is still growing: everything up to the last
/// sentence end, and the rest. P@x's words are spoken a sentence at a time
/// in a live conversation, so that it begins to speak before it has
/// finished writing.
pub fn complete_sentences(text: &str) -> (Vec<String>, String) {
    let mut sentences = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (position, (index, character)) in chars.iter().enumerate() {
        let ends = matches!(character, '.' | '!' | '?' | '…' | '\n');
        let followed_by_space = chars
            .get(position + 1)
            .is_none_or(|(_, next)| next.is_whitespace());
        if ends && followed_by_space && chars.get(position + 1).is_some() {
            let end = index + character.len_utf8();
            let sentence = text[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_owned());
            }
            start = end;
        }
    }
    (sentences, text[start..].to_owned())
}

/// The details of a voice state, for `VoiceStateChanged`.
pub fn state_details(phase: &Phase, indicator: Indicator, reason: Option<&str>) -> Value {
    let mut details = json!({
        "state": phase.name(),
        "microphone_open": phase.microphone_open(),
        "indicator": indicator.as_str(),
        "reason": reason,
    });
    match phase {
        Phase::Listening { turn, trigger } => {
            details["turn"] = json!(turn);
            details["trigger"] = json!(trigger.as_str());
        }
        Phase::Recognising { turn } | Phase::Waiting { turn } => {
            details["turn"] = json!(turn);
        }
        Phase::Live { session } => {
            details["session"] = json!(session);
        }
        _ => {}
    }
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listen(turn: &str, trigger: Trigger) -> Input {
        Input::Listen {
            turn: turn.to_owned(),
            trigger,
        }
    }

    #[test]
    fn push_to_talk_is_the_default_and_everything_starts_off() {
        let settings = VoiceSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.activation, Activation::PushToTalk);
        assert!(!settings.wake_word);
        assert!(!settings.spoken_replies);
        assert_eq!(settings.transcript_check, Check::Show);
        assert_eq!(settings.live.engine, "local");
        assert!(settings.live.consent.is_none());
        // The file of a person who never touched voice is the defaults.
        let parsed: VoiceSettings = serde_json::from_str("{}").expect("defaults");
        assert_eq!(parsed, settings);
    }

    #[test]
    fn the_wake_word_cannot_be_turned_on_in_this_build() {
        let settings = VoiceSettings::default();
        let mut change = Map::new();
        change.insert("wake_word".to_owned(), Value::Bool(true));
        let refused = settings.apply(&change).expect_err("refused");
        assert_eq!(refused.code.as_str(), "TUR-AI-VOICE-005");
        // A file that says it is on is not used: voice stays off.
        let text = r#"{"enabled": true, "wake_word": true}"#;
        let dir = std::env::temp_dir().join(format!("tur-voice-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("voice.json");
        std::fs::write(&path, text).expect("written");
        assert!(VoiceSettings::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(WAKE_WORD_EXPLANATION.contains("nothing else"));
        assert!(WAKE_WORD_EXPLANATION.contains("not available"));
    }

    #[test]
    fn settings_change_member_by_member_and_refuse_what_is_not_there() {
        let settings = VoiceSettings::default();
        let change: Map<String, Value> = serde_json::from_str(
            r#"{"enabled": true, "language": "it", "spoken_replies": true,
                "live": {"silence_timeout_seconds": 30},
                "synthesis": {"voices": {"it": "voice-it-paola-medium"}}}"#,
        )
        .expect("an object");
        let changed = settings.apply(&change).expect("applied");
        assert!(changed.enabled);
        assert_eq!(changed.language, "it");
        assert_eq!(changed.live.silence_timeout_seconds, 30);
        assert!(changed.live.narrate);
        assert_eq!(changed.synthesis.voices["it"], "voice-it-paola-medium");
        // null restores a default.
        let back: Map<String, Value> =
            serde_json::from_str(r#"{"language": null, "live": {"silence_timeout_seconds": null}}"#)
                .expect("an object");
        let restored = changed.apply(&back).expect("applied");
        assert_eq!(restored.language, "auto");
        assert_eq!(restored.live.silence_timeout_seconds, 60);
        for bad in [
            r#"{"volume": 3}"#,
            r#"{"language": "klingon"}"#,
            r#"{"live": {"consent": {"provider": "openai", "version": "1", "at": 0}}}"#,
            r#"{"live": 3}"#,
            r#"{"show_seconds": 0}"#,
            r#"{"live": {"engine": "NOT AN ID"}}"#,
            r#"{"synthesis": {"voices": {"xx": "a"}}}"#,
        ] {
            let change: Map<String, Value> = serde_json::from_str(bad).expect("an object");
            assert!(settings.apply(&change).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_provider_s_live_engine_needs_the_current_consent() {
        let mut settings = VoiceSettings::default();
        assert!(settings.live_consent_current("local"));
        assert!(!settings.live_consent_current("openai-realtime"));
        assert!(settings.accept_live("openai-realtime", "0", 1).is_err());
        settings
            .accept_live("openai-realtime", VOICE_CONSENT_VERSION, 1)
            .expect("accepted");
        assert!(settings.live_consent_current("openai-realtime"));
        assert!(!settings.live_consent_current("gemini-live"));
        let text = live_consent_text("OpenAI", Some("https://openai.com/policies"));
        assert!(text.contains("streamed to OpenAI"));
        assert!(text.contains("your own key"));
        assert!(text.contains("sends nothing anywhere"));
    }

    #[test]
    fn the_microphone_opens_only_when_it_is_shown() {
        assert!(may_listen(Trigger::PushToTalk, Indicator::Overlay).is_ok());
        assert!(may_listen(Trigger::PushToTalk, Indicator::Caller).is_ok());
        assert!(may_listen(Trigger::Button, Indicator::Caller).is_ok());
        assert!(may_listen(Trigger::PushToTalk, Indicator::None).is_err());
        // A live session lasts: only P@x's indicator will do.
        assert!(may_listen(Trigger::Live, Indicator::Caller).is_err());
        assert!(may_listen(Trigger::Live, Indicator::Overlay).is_ok());
        assert!(may_listen(Trigger::WakeWord, Indicator::Caller).is_err());
        let refused = may_listen(Trigger::Live, Indicator::None).expect_err("refused");
        assert_eq!(refused.code.as_str(), "TUR-AI-VOICE-004");
        // A caller cannot claim a wake word.
        assert_eq!(Trigger::from_caller("wake-word"), None);
        assert_eq!(Trigger::from_caller(""), Some(Trigger::PushToTalk));
    }

    #[test]
    fn push_to_talk_opens_on_press_and_closes_on_release() {
        let mut machine = Machine::new(Phase::Ready);
        assert_eq!(
            machine.step(listen("v1", Trigger::PushToTalk)),
            [Effect::OpenMicrophone]
        );
        assert!(machine.phase().microphone_open());
        // Another turn's release is not this turn's.
        assert!(
            machine
                .step(Input::Release {
                    turn: "v0".to_owned(),
                    send: true
                })
                .is_empty()
        );
        assert_eq!(
            machine.step(Input::Release {
                turn: "v1".to_owned(),
                send: true
            }),
            [Effect::CloseAndRecognise]
        );
        assert!(!machine.phase().microphone_open());
        assert_eq!(machine.phase().name(), "recognising");
        machine.step(Input::Recognised {
            turn: "v1".to_owned(),
            waits: false,
        });
        assert_eq!(machine.phase(), &Phase::Ready);
        // Dropped by Esc: nothing is recognised.
        machine.step(listen("v2", Trigger::Button));
        assert_eq!(
            machine.step(Input::Release {
                turn: "v2".to_owned(),
                send: false
            }),
            [Effect::CloseAndDrop]
        );
        // Stop closes it too.
        machine.step(listen("v3", Trigger::PushToTalk));
        assert_eq!(machine.step(Input::Stop), [Effect::CloseAndDrop]);
        assert_eq!(machine.phase(), &Phase::Ready);
        // Nothing listens while voice is off.
        let mut off = Machine::new(Phase::Off);
        assert!(off.step(listen("v4", Trigger::PushToTalk)).is_empty());
        assert!(!off.phase().microphone_open());
    }

    #[test]
    fn barge_in_the_person_s_key_stops_p_a_x_speaking_and_listens() {
        let mut machine = Machine::new(Phase::Ready);
        machine.step(Input::SpeakStart);
        assert!(machine.speaking());
        assert_eq!(
            machine.step(listen("v1", Trigger::PushToTalk)),
            [Effect::StopSpeech, Effect::OpenMicrophone]
        );
        assert!(!machine.speaking());
        // Stop alone stops the speech.
        let mut machine = Machine::new(Phase::Ready);
        machine.step(Input::SpeakStart);
        assert_eq!(machine.step(Input::Stop), [Effect::StopSpeech]);
        assert_eq!(machine.phase(), &Phase::Ready);
        // Something that is not the person cannot barge in.
        let mut machine = Machine::new(Phase::Ready);
        machine.step(Input::SpeakStart);
        assert!(machine.step(listen("v2", Trigger::WakeWord)).is_empty());
        assert!(machine.speaking());
    }

    #[test]
    fn self_sound_what_is_heard_while_p_a_x_speaks_is_not_the_person() {
        let mut machine = Machine::new(Phase::Ready);
        machine.step(Input::LiveStart {
            session: "l1".to_owned(),
        });
        machine.step(Input::SpeakStart);
        // Without echo cancellation the microphone hears P@x: dropped, and
        // P@x goes on.
        assert_eq!(
            machine.step(Input::VoiceWhileSpeaking {
                echo_cancelled: false
            }),
            [Effect::DropOwnSound]
        );
        assert!(machine.speaking());
        // With it, the person's voice interrupts P@x.
        assert_eq!(
            machine.step(Input::VoiceWhileSpeaking {
                echo_cancelled: true
            }),
            [Effect::StopSpeech]
        );
        assert!(!machine.speaking());
        assert!(machine.phase().microphone_open());
        // Ending the session stops speech and closes the microphone.
        machine.step(Input::SpeakStart);
        assert_eq!(
            machine.step(Input::LiveEnd),
            [Effect::StopSpeech, Effect::CloseAndDrop]
        );
        assert!(!machine.phase().microphone_open());
        // Outside a live session the microphone is closed while P@x speaks.
        let mut machine = Machine::new(Phase::Ready);
        machine.step(Input::SpeakStart);
        assert!(!machine.phase().microphone_open());
        assert!(
            machine
                .step(Input::VoiceWhileSpeaking {
                    echo_cancelled: true
                })
                .is_empty()
        );
    }

    #[test]
    fn a_transcript_heard_over_other_sound_is_never_sent_by_itself() {
        let settings = VoiceSettings::default();
        assert_eq!(
            transcript_check(&settings, Trigger::PushToTalk, false, false),
            (Check::Show, None)
        );
        assert_eq!(
            transcript_check(&settings, Trigger::PushToTalk, true, false),
            (Check::Wait, Some(WaitReason::OtherSound))
        );
        assert_eq!(
            transcript_check(&settings, Trigger::Button, false, true),
            (Check::Wait, Some(WaitReason::TaskRunning))
        );
        assert_eq!(
            transcript_check(&settings, Trigger::WakeWord, false, false),
            (Check::Wait, Some(WaitReason::NotByThePerson))
        );
        let waiting = VoiceSettings {
            transcript_check: Check::Wait,
            ..VoiceSettings::default()
        };
        assert_eq!(
            transcript_check(&waiting, Trigger::PushToTalk, false, false),
            (Check::Wait, Some(WaitReason::Setting))
        );
    }

    #[test]
    fn pw_dump_says_whether_another_program_plays_sound() {
        let playing = r#"[
            {"type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {
                "media.class": "Stream/Output/Audio", "application.name": "Firefox",
                "application.process.id": "4242"}}},
            {"type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {
                "media.class": "Stream/Output/Audio", "application.name": "P@x",
                "application.process.id": 77}}}
        ]"#;
        assert!(other_sound_playing(playing, 77));
        let only_ours = r#"[
            {"type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {
                "media.class": "Stream/Output/Audio", "application.name": "pw-play",
                "application.process.id": 77}}},
            {"type": "PipeWire:Interface:Node", "info": {"state": "idle", "props": {
                "media.class": "Stream/Output/Audio", "application.name": "Firefox"}}},
            {"type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {
                "media.class": "Stream/Input/Audio", "application.name": "pw-record"}}}
        ]"#;
        assert!(!other_sound_playing(only_ours, 77));
        // What cannot be read counts as sound: the stricter reading.
        assert!(other_sound_playing("not json", 77));
        let echo = r#"[{"type": "PipeWire:Interface:Node", "info": {"props": {
            "media.class": "Audio/Source", "node.name": "echo-cancel-source"}}}]"#;
        assert!(echo_cancelled_source(echo));
        assert!(!echo_cancelled_source(only_ours));
    }

    #[test]
    fn class_3_never_accepts_voice_and_class_2_is_on_screen() {
        let everything = SpokenApproval {
            policy_allows: true,
            push_to_talk_same_exchange: true,
            other_sound: false,
            own_sound: false,
        };
        for spoken in [SpokenApproval::NONE, everything] {
            assert_eq!(
                approval(ActionClass::SystemCritical, spoken),
                VoiceApproval::Never
            );
            assert_eq!(approval(ActionClass::Prohibited, spoken), VoiceApproval::Never);
            assert_eq!(approval(ActionClass::Observe, spoken), VoiceApproval::NotNeeded);
            assert_eq!(
                approval(ActionClass::Reversible, spoken),
                VoiceApproval::NotNeeded
            );
        }
        // The default, and this build: on screen.
        assert_eq!(
            approval(ActionClass::Consequential, SpokenApproval::NONE),
            VoiceApproval::OnScreen
        );
        // Spoken approval only with every condition.
        assert_eq!(
            approval(ActionClass::Consequential, everything),
            VoiceApproval::VoiceOrScreen
        );
        for broken in [
            SpokenApproval {
                policy_allows: false,
                ..everything
            },
            SpokenApproval {
                push_to_talk_same_exchange: false,
                ..everything
            },
            SpokenApproval {
                other_sound: true,
                ..everything
            },
            SpokenApproval {
                own_sound: true,
                ..everything
            },
        ] {
            assert_eq!(
                approval(ActionClass::Consequential, broken),
                VoiceApproval::OnScreen
            );
        }
    }

    #[test]
    fn the_gate_opens_after_the_transcript_was_shown_or_sent_and_never_after_a_withdrawal() {
        let gate = TranscriptGate::shown_for(Duration::from_secs(3600));
        assert_eq!(gate.state(), Gate::Shut);
        gate.open();
        assert_eq!(gate.state(), Gate::Open);
        let gate = TranscriptGate::shown_for(Duration::ZERO);
        assert_eq!(gate.state(), Gate::Open);
        let gate = TranscriptGate::held();
        assert_eq!(gate.state(), Gate::Shut);
        gate.withdraw();
        gate.open();
        assert_eq!(gate.state(), Gate::Withdrawn);
        assert_eq!(TranscriptGate::sent().state(), Gate::Open);
    }

    #[test]
    fn stop_and_resume_words_in_english_and_italian() {
        for words in ["Stop.", "stop", " Ferma!", "basta", "Annulla", "cancel"] {
            assert_eq!(interjection(words), Interjection::Stop, "{words}");
        }
        for words in ["continue", "Vai avanti.", "riprendi", "go on"] {
            assert_eq!(interjection(words), Interjection::Resume, "{words}");
        }
        for words in ["stop the music", "open the invoice", "fermati qui e poi apri"] {
            assert_eq!(interjection(words), Interjection::Other, "{words}");
        }
        let queue = Interjections::new();
        queue.push("  also the March one ");
        queue.push("");
        assert_eq!(queue.take(), ["also the March one"]);
        assert!(queue.take().is_empty());
    }

    #[test]
    fn a_wav_file_round_trips_and_odd_files_are_refused() {
        let samples: Vec<i16> = (0..1600).map(|i| ((i % 64) * 500 - 16000) as i16).collect();
        let bytes = wav_bytes(&samples, SAMPLE_RATE);
        assert_eq!(bytes.len(), 44 + 3200);
        assert_eq!(wav_samples(&bytes), Some((samples, SAMPLE_RATE)));
        assert_eq!(wav_samples(b"RIFF\0\0\0\0WAVE"), None);
        let mut stereo = wav_bytes(&[0, 0], SAMPLE_RATE);
        stereo[22] = 2;
        assert_eq!(wav_samples(&stereo), None);
        for cut in 0..44 {
            let _ = wav_samples(&bytes[..cut]);
        }
    }

    #[test]
    fn the_segmenter_finds_utterances_between_silences() {
        let quiet = vec![0i16; SAMPLE_RATE as usize / 2];
        let loud: Vec<i16> = (0..SAMPLE_RATE as usize)
            .map(|i| if i % 2 == 0 { 9000 } else { -9000 })
            .collect();
        let mut segmenter = Segmenter::new();
        assert!(segmenter.push(&quiet).is_empty());
        let started = segmenter.push(&loud);
        assert_eq!(started, [Heard::SpeechStarted]);
        assert!(segmenter.in_speech());
        let ended = segmenter.push(&vec![0i16; SAMPLE_RATE as usize]);
        assert_eq!(ended.len(), 1);
        let Heard::Utterance(samples) = &ended[0] else {
            panic!("an utterance");
        };
        // The utterance holds the speech and the moments around it.
        assert!(samples.len() >= loud.len());
        assert!(!segmenter.in_speech());
        // What was heard while P@x spoke is thrown away.
        segmenter.push(&loud);
        segmenter.reset();
        assert!(!segmenter.in_speech());
        assert!(segmenter.push(&quiet).is_empty());
        assert!(level(&loud) > 0.9);
        assert!(level(&quiet).abs() < f64::EPSILON);
    }

    #[test]
    fn p_a_x_s_words_are_spoken_a_sentence_at_a_time() {
        let (done, rest) = complete_sentences("I opened the invoice. The total is 412 euros. Sha");
        assert_eq!(done, ["I opened the invoice.", "The total is 412 euros."]);
        assert_eq!(rest, " Sha");
        let (done, rest) = complete_sentences("Version 1.5 is out");
        assert!(done.is_empty());
        assert_eq!(rest, "Version 1.5 is out");
        let (done, _) = complete_sentences("Ho aperto la fattura! Il totale è 412 euro? ");
        assert_eq!(done.len(), 2);
    }

    #[test]
    fn the_state_details_say_whether_the_microphone_is_open() {
        let listening = Phase::Listening {
            turn: "v1".to_owned(),
            trigger: Trigger::PushToTalk,
        };
        let details = state_details(&listening, Indicator::Caller, None);
        assert_eq!(details["microphone_open"], true);
        assert_eq!(details["trigger"], "push-to-talk");
        assert_eq!(details["indicator"], "caller");
        let live = Phase::Live {
            session: "l1".to_owned(),
        };
        assert_eq!(state_details(&live, Indicator::Overlay, None)["microphone_open"], true);
        assert_eq!(state_details(&Phase::Speaking, Indicator::None, None)["microphone_open"], false);
        assert_eq!(Source::VoiceEdited.as_str(), "voice_edited");
        assert!(Source::Voice.heard());
        assert!(!Source::VoiceEdited.heard());
    }
}

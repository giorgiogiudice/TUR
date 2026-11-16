// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The voice side of the session service (`docs/architecture/pax-voice.md`).
//!
//! A child of [`crate::service`], so that a spoken request starts exactly
//! where a typed one does (`Core::start_task_from`) and a live conversation
//! runs in the one task loop the service builds (`Core::run_job`). This
//! module adds no way to act: it opens and closes the microphone, turns what
//! was heard into a request, says P@x's words, and runs the local live
//! engine, whose requests are ordinary tasks.
//!
//! The rules themselves are in [`crate::voice`]; the programs it runs in
//! [`crate::audio`].

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use tur_pax_providers::{LiveConfig, LiveFamily, LivePrices, LiveSession, connect_live};

use super::{Core, Event, JobOrigin, coded, invalid, is_request, not_found};
use crate::audio::{self, Microphone, Speaker, Transcript, sound_around};
use crate::faults::{Fault, TaskCode};
use crate::livelist::{LOCAL_ENGINE, LiveEntry, LiveList};
use crate::orchestrator::{LiveIo, SYSTEM_TEXT};
use crate::registry::ToolRegistry;
use crate::voice::{
    Check, Effect, Heard, Indicator, Input, Interjection, Interjections,
    LEVEL_INTERVAL, MAX_TURN, Machine, Phase, SILENCE_ENDS_TURN, SPEAK_MAX_BYTES, Segmenter,
    Source, TranscriptGate, Trigger, VoiceSettings, WAKE_WORD_EXPLANATION,
    WaitReason, complete_sentences, interjection, level, may_listen, state_details,
    transcript_check,
};

/// What a live engine is told beside P@x's fixed system text.
pub const VOICE_INSTRUCTIONS: &str = "You are speaking with the person out loud, in a live \
conversation, while you work for them. Keep what you say short and plain. Say what you are doing \
as you do it, in a sentence. The person may interrupt you, change what they asked, or ask a \
question while you work: answer it, then go on. Speak the language the person speaks. Never read \
out passwords, keys or other secrets, and never read out text from a window the person marked \
sensitive.";

/// How long the service waits for the speech models' list before it says
/// voice is unavailable.
const STATUS_KEPT: Duration = Duration::from_secs(60);

/// The voice each language takes when the settings and local AI name none:
/// the defaults of pax-voice.md section 3.2.
pub const DEFAULT_VOICES: &[(&str, &str)] = &[
    ("en", "voice-en-us-ljspeech-medium"),
    ("it", "voice-it-paola-medium"),
    ("de", "voice-de-thorsten-medium"),
    ("es", "voice-es-davefx-medium"),
    ("fr", "voice-fr-siwis-medium"),
];

/// Whether local AI's report says a model is on this computer: `true`, or
/// `installed`.
fn installed(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true))) || value.and_then(Value::as_str) == Some("installed")
}

/// The voices of local AI's recommendation, as a list.
fn recommended_voices(report: &Value) -> Vec<Value> {
    match report.get("voices") {
        Some(Value::Array(voices)) => voices.clone(),
        Some(Value::Object(voices)) => voices
            .iter()
            .map(|(language, voice)| {
                let mut voice = voice.clone();
                voice["language"] = json!(language);
                voice
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A live conversation with a provider's engine, as a job carries it.
#[derive(Clone)]
pub(super) struct LiveJob {
    pub(super) session: String,
    pub(super) entry: LiveEntry,
    pub(super) model: String,
    pub(super) prices: Option<LivePrices>,
    pub(super) end: Arc<AtomicBool>,
    pub(super) silence: Duration,
}

/// The listening turn under way.
struct Turn {
    id: String,
    trigger: Trigger,
    other_sound: bool,
    /// The task paused while the person speaks.
    paused_task: Option<String>,
}

/// A transcript waiting for the person.
struct Pending {
    turn: String,
    text: String,
    trigger: Trigger,
}

/// A live conversation under way.
struct LiveRun {
    id: String,
    engine: String,
    end: Arc<AtomicBool>,
    narrate: bool,
}

/// What local AI says about speech on this computer, kept a minute.
#[derive(Clone)]
struct SpeechChoice {
    report: Value,
    at: Instant,
}

/// The voice's state, under one lock.
struct VoiceInner {
    settings: VoiceSettings,
    fault: Option<Fault>,
    loaded: bool,
    machine: Machine,
    indicator: Indicator,
    overlay: bool,
    counter: u64,
    microphone: Option<Microphone>,
    turn: Option<Turn>,
    pending: Option<Pending>,
    live: Option<LiveRun>,
    live_tasks: HashSet<String>,
    spoken_language: Option<String>,
    choice: Option<SpeechChoice>,
    preparing: Option<Arc<Mutex<Option<std::process::Child>>>>,
}

/// The voice of the service.
pub(super) struct VoiceCell {
    inner: Mutex<VoiceInner>,
    me: Mutex<Weak<Core>>,
    /// Bumped by every stop of P@x's speech: what was to be said before it
    /// is not said.
    generation: AtomicU64,
    /// One utterance at a time.
    speech: Mutex<()>,
    speaker: Mutex<Option<Speaker>>,
    last_level: Mutex<Option<Instant>>,
}

impl VoiceCell {
    pub(super) fn new() -> VoiceCell {
        VoiceCell {
            inner: Mutex::new(VoiceInner {
                settings: VoiceSettings::default(),
                fault: None,
                loaded: false,
                machine: Machine::new(Phase::Off),
                indicator: Indicator::None,
                overlay: false,
                counter: 0,
                microphone: None,
                turn: None,
                pending: None,
                live: None,
                live_tasks: HashSet::new(),
                spoken_language: None,
                choice: None,
                preparing: None,
            }),
            me: Mutex::new(Weak::new()),
            generation: AtomicU64::new(0),
            speech: Mutex::new(()),
            speaker: Mutex::new(None),
            last_level: Mutex::new(None),
        }
    }

    pub(super) fn attach(&self, core: &Arc<Core>) {
        *self.me.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(core);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where the voice rests: off while voice or P@x is off, otherwise ready.
fn resting_phase(inner: &VoiceInner, pax_on: bool) -> Phase {
    if !inner.settings.enabled || inner.fault.is_some() || !pax_on {
        Phase::Off
    } else {
        Phase::Ready
    }
}

fn voice_fault(code: TaskCode) -> Fault {
    Fault::new(code, code.user_message())
}

/// A field of a JSON object as text.
fn text_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut at = value;
    for key in path {
        at = at.get(*key)?;
    }
    at.as_str()
}

impl Core {
    /// The voice's state. Its lock is taken on its own or after nothing:
    /// the service's state is never locked while it is held, and it is
    /// never taken while the service's state is locked.
    fn voice_inner(&self) -> MutexGuard<'_, VoiceInner> {
        let mut inner = lock(&self.voice.inner);
        if !inner.loaded {
            inner.loaded = true;
            match VoiceSettings::load(&self.parts.paths.voice) {
                Ok(settings) => inner.settings = settings,
                Err(problem) => inner.fault = Some(problem),
            }
            let resting = resting_phase(&inner, true);
            inner.machine.rest(resting);
        }
        inner
    }

    fn me(&self) -> Option<Arc<Core>> {
        lock(&self.voice.me).upgrade()
    }

    /// The voice state in one word, for the property `Voice`.
    pub fn voice_name(&self) -> &'static str {
        self.voice_inner().machine.phase().name()
    }

    /// Whether P@x's microphone is open, for the property `MicrophoneOpen`.
    pub fn microphone_open(&self) -> bool {
        self.voice_inner().machine.phase().microphone_open()
    }

    fn voice_changed(&self, reason: Option<&str>) {
        let (name, details) = {
            let inner = self.voice_inner();
            (
                inner.machine.phase().name(),
                state_details(inner.machine.phase(), inner.indicator, reason),
            )
        };
        self.emit(Event::VoiceStateChanged {
            state: name,
            details,
        });
        self.emit(Event::StateChanged);
    }

    // ---------------------------------------------------------- settings

    /// What `GetVoiceStatus` says (pax-voice.md section 6.4).
    pub fn voice_status(&self) -> Value {
        let choice = self.speech_choice(false);
        let (settings, fault, phase, indicator, turn, session) = {
            let inner = self.voice_inner();
            (
                inner.settings.clone(),
                inner.fault.clone(),
                inner.machine.phase().clone(),
                inner.indicator,
                inner.turn.as_ref().map(|turn| turn.id.clone()),
                inner.live.as_ref().map(|live| live.id.clone()),
            )
        };
        let (report, problem) = match &choice {
            Ok(report) => (report.clone(), fault.clone()),
            Err(problem) => (Value::Null, fault.clone().or_else(|| Some(problem.clone()))),
        };
        let recognition = report.get("recognition").cloned().unwrap_or(Value::Null);
        let voice_activity = report.get("vad").cloned().unwrap_or(Value::Null);
        let voices = Value::Array(recommended_voices(&report));
        let needed = report
            .get("download_bytes_needed")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let lists = LiveList::load(&self.parts.paths.live).unwrap_or_else(|_| LiveList::empty());
        let mut engines = vec![json!({
            "id": LOCAL_ENGINE,
            "name": "On this computer",
            "kind": "local",
            "paid": false,
            "built": true,
            "transport": "local",
            "consent_current": true,
            "runs_locally": self.provider_runs_locally(),
        })];
        for entry in &lists.providers {
            let key = match self.parts.keys.is_set(&entry.keyring_item) {
                Ok(true) => "set",
                Ok(false) => "not_set",
                Err(_) => "unknown",
            };
            engines.push(entry.to_json(settings.live_consent_current(&entry.id), key));
        }
        let around = sound_around(&self.parts.paths.sound.dump);
        json!({
            "enabled": settings.enabled,
            "state": phase.name(),
            "microphone_open": phase.microphone_open(),
            "indicator": indicator.as_str(),
            "settings": settings,
            "problem": problem.map(|problem| json!({
                "error_code": problem.code.to_string(),
                "message": problem.detail,
            })),
            "recognition": recognition,
            "voice_activity": voice_activity,
            "synthesis": { "voices": voices },
            "download_bytes_needed": needed,
            "free": settings.live.engine == LOCAL_ENGINE,
            "live": {
                "available": settings.enabled && phase.name() != "off",
                "engines": engines,
                "as_of": lists.as_of,
            },
            "wake_word": {
                "available": false,
                "on": false,
                "explanation": WAKE_WORD_EXPLANATION,
            },
            "keys": { "talk": "Hold Meta+Space" },
            "capture": {
                "program_found": self.parts.paths.sound.record.exists(),
                "other_sound": around.other_sound,
                "echo_cancelled": around.echo_cancelled,
            },
            "turn": turn,
            "session": session,
        })
    }

    fn provider_runs_locally(&self) -> bool {
        let state = self.state();
        let gateway = state.settings.base_url.clone();
        state
            .settings
            .current_provider(&state.models)
            .is_some_and(|(entry, origin)| entry.runs_locally(origin, gateway.as_deref()))
    }

    /// Change the voice settings (`SetVoiceSettings`).
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` for a change the settings do not take;
    /// `TUR-AI-VOICE-005` for a wake word; `TUR-AI-VOICE-007` for a
    /// provider's live engine without the current consent;
    /// `TUR-AI-SERVICE-009` if the file cannot be written.
    pub fn set_voice_settings(&self, change: &str) -> Result<(), Fault> {
        let value: Value = crate::strict::value_from_str(change)
            .map_err(|_| invalid("the voice settings are not one JSON object"))?;
        let Some(change) = value.as_object() else {
            return Err(invalid("the voice settings are not one JSON object"));
        };
        let lists = LiveList::load(&self.parts.paths.live)?;
        let now = self.now().seconds();
        let pax_on = self.is_enabled();
        {
            let mut inner = self.voice_inner();
            if let Some(problem) = &inner.fault {
                return Err(problem.clone());
            }
            let mut changed = inner.settings.apply(change)?;
            let engine = changed.live.engine.clone();
            if engine != LOCAL_ENGINE {
                let Some(entry) = lists.entry(&engine) else {
                    return Err(voice_fault(TaskCode::VoiceLiveUnavailable));
                };
                if !entry.built {
                    return Err(voice_fault(TaskCode::VoiceLiveUnavailable));
                }
                entry.model(changed.live.model.as_deref())?;
                match change.get("live_consent_version").and_then(Value::as_str) {
                    Some(version) => changed.accept_live(&engine, version, now)?,
                    None if changed.live_consent_current(&engine) => {}
                    None => return Err(voice_fault(TaskCode::VoiceLiveConsent)),
                }
            }
            changed.save(&self.parts.paths.voice)?;
            let switched = changed.enabled != inner.settings.enabled;
            inner.settings = changed;
            inner.choice = None;
            if switched {
                let resting = resting_phase(&inner, pax_on);
                let effects = inner.machine.rest(resting);
                drop(inner);
                self.perform(&effects);
            }
        }
        self.record_change("voice", "changed");
        self.voice_changed(None);
        Ok(())
    }

    /// What local AI recommends and has for speech, kept a minute.
    fn speech_choice(&self, fresh: bool) -> Result<Value, Fault> {
        let language = {
            let inner = self.voice_inner();
            if !fresh
                && let Some(choice) = &inner.choice
                && choice.at.elapsed() < STATUS_KEPT
            {
                return Ok(choice.report.clone());
            }
            inner.settings.language.clone()
        };
        let mut arguments = vec!["speech", "recommend", "--json"];
        if language != "auto" {
            arguments.extend(["--language", language.as_str()]);
        }
        let report = crate::local::report(&self.parts.paths.ai_local, &arguments)?;
        self.voice_inner().choice = Some(SpeechChoice {
            report: report.clone(),
            at: Instant::now(),
        });
        Ok(report)
    }

    /// The recognition model in use, and whether it is on this computer.
    fn recognition_model(&self) -> Result<String, Fault> {
        let chosen = self.voice_inner().settings.recognition.model.clone();
        let report = self.speech_choice(false)?;
        let recommended = text_at(&report, &["recognition", "id"]).map(str::to_owned);
        let present = installed(
            report
                .get("recognition")
                .and_then(|recognition| recognition.get("installed")),
        );
        match (chosen, recommended) {
            (Some(model), _) => Ok(model),
            (None, Some(model)) if present => Ok(model),
            _ => Err(voice_fault(TaskCode::VoiceModelsMissing)),
        }
    }

    /// The voice for a language: the settings', local AI's, or the default.
    fn voice_for(&self, language: &str) -> String {
        let language = if language == "auto" { "en" } else { language };
        if let Some(voice) = self
            .voice_inner()
            .settings
            .synthesis
            .voices
            .get(language)
            .cloned()
        {
            return voice;
        }
        if let Ok(report) = self.speech_choice(false)
            && let Some(voice) = recommended_voices(&report)
                .iter()
                .find(|voice| text_at(voice, &["language"]) == Some(language))
                .and_then(|voice| text_at(voice, &["id"]))
        {
            return voice.to_owned();
        }
        DEFAULT_VOICES
            .iter()
            .find(|(name, _)| *name == language)
            .map_or("voice-en-us-ljspeech-medium", |(_, voice)| voice)
            .to_owned()
    }

    /// Download the speech models in use (`PrepareVoice`).
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if a download is needed and `download_bytes`
    /// is not the size the person was shown; `-004` while one runs;
    /// `TUR-AI-SERVICE-010`.
    pub fn prepare_voice(self: &Arc<Self>, options: &str) -> Result<String, Fault> {
        let options: Map<String, Value> = if options.trim().is_empty() {
            Map::new()
        } else {
            crate::strict::object_from_str(options)
                .map_err(|_| invalid("the options are not one JSON object"))?
        };
        for name in options.keys() {
            if !matches!(name.as_str(), "download_bytes" | "models" | "accept_licence") {
                return Err(invalid("an option is not known"));
            }
        }
        let report = self.speech_choice(true)?;
        let mut models: Vec<String> = match options.get("models") {
            Some(Value::Array(names)) => names
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| crate::voice::is_voice_id(name))
                .map(str::to_owned)
                .collect(),
            Some(_) => return Err(invalid("models is a list of identifiers")),
            None => {
                let mut ids = Vec::new();
                for path in [["recognition", "id"], ["vad", "id"]] {
                    if let Some(id) = text_at(&report, &path) {
                        ids.push(id.to_owned());
                    }
                }
                for voice in recommended_voices(&report) {
                    if let Some(id) = text_at(&voice, &["id"]) {
                        ids.push(id.to_owned());
                    }
                }
                ids
            }
        };
        models.sort();
        models.dedup();
        if models.is_empty() {
            return Err(invalid("local AI named no speech model for this computer"));
        }
        let needed = report
            .get("download_bytes_needed")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if needed > 0 && options.get("download_bytes").and_then(Value::as_u64) != Some(needed) {
            return Err(invalid(format!(
                "this downloads {needed} bytes from the models' publishers; say download_bytes \
                 with that size to go on"
            )));
        }
        let accept = options
            .get("accept_licence")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let slot = Arc::new(Mutex::new(None));
        {
            let mut inner = self.voice_inner();
            if inner.preparing.is_some() {
                return Err(coded(TaskCode::ServiceBusy));
            }
            inner.preparing = Some(Arc::clone(&slot));
        }
        let mut arguments = vec!["speech".to_owned(), "ensure".to_owned()];
        arguments.extend(models.iter().cloned());
        arguments.push("--json".to_owned());
        if accept {
            arguments.push("--accept-licence".to_owned());
        }
        let child = std::process::Command::new(&self.parts.paths.ai_local)
            .args(&arguments)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                self.voice_inner().preparing = None;
                return Err(Fault::new(
                    TaskCode::ServiceLocalUnavailable,
                    format!("{}: {error}", self.parts.paths.ai_local.display()),
                ));
            }
        };
        let stdout = child.stdout.take();
        *lock(&slot) = Some(child);
        let job = format!("p-{}", self.now().seconds());
        let core = Arc::clone(self);
        let watched = Arc::clone(&slot);
        let spawned = thread::Builder::new()
            .name("tur-pax-voice-prepare".to_owned())
            .spawn(move || {
                use std::io::BufRead;
                let mut last = String::new();
                if let Some(stdout) = stdout {
                    for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                        let Ok(event) = crate::strict::value_from_str(&line) else {
                            continue;
                        };
                        last = text_at(&event, &["event"]).unwrap_or_default().to_owned();
                        let model = text_at(&event, &["model"]).unwrap_or("all").to_owned();
                        core.emit(Event::VoiceProgress { model, event });
                    }
                }
                let child = lock(&watched).take();
                let ok = child.is_some_and(|mut child| {
                    child.wait().is_ok_and(|status| status.success())
                });
                core.voice_inner().preparing = None;
                core.voice_inner().choice = None;
                let event = if ok {
                    json!({ "kind": "pax-event", "event": "ready" })
                } else {
                    json!({ "kind": "pax-event", "event": "failed", "last": last })
                };
                core.emit(Event::VoiceProgress {
                    model: "all".to_owned(),
                    event,
                });
                core.voice_changed(None);
            });
        if spawned.is_err() {
            if let Some(mut child) = lock(&slot).take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.voice_inner().preparing = None;
            return Err(Fault::new(
                TaskCode::ServiceStorage,
                "the download could not be started",
            ));
        }
        Ok(job)
    }

    /// Stop the download of the speech models; what was downloaded is kept.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if none runs.
    pub fn cancel_voice_preparation(&self) -> Result<(), Fault> {
        let slot = self
            .voice_inner()
            .preparing
            .clone()
            .ok_or_else(|| not_found("speech download"))?;
        let _ = crate::local::report(
            &self.parts.paths.ai_local,
            &["speech", "cancel", "--json"],
        );
        let child = lock(&slot).take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    // -------------------------------------------------------- the turn

    /// Whether voice may be used now: P@x on and ready, voice on, the
    /// session unlocked.
    fn voice_allowed(&self) -> Result<(), Fault> {
        {
            let state = self.state();
            if let Some(problem) = &state.settings_fault {
                return Err(problem.clone());
            }
            if let Some(refusal) = state.settings.refusal() {
                return Err(refusal);
            }
            if state.locked == Some(true) {
                return Err(coded(TaskCode::ServiceSessionLocked));
            }
        }
        let inner = self.voice_inner();
        if let Some(problem) = &inner.fault {
            return Err(problem.clone());
        }
        if !inner.settings.enabled {
            return Err(voice_fault(TaskCode::VoiceDisabled));
        }
        Ok(())
    }

    /// Show "Listening…" on P@x's indicator, or rely on the caller. Returns
    /// who shows it.
    fn show_listening(&self, shown_by_caller: bool) -> Indicator {
        if self.parts.events.indicator_listening(true) {
            self.voice_inner().overlay = true;
            Indicator::Overlay
        } else if shown_by_caller {
            Indicator::Caller
        } else {
            Indicator::None
        }
    }

    fn hide_listening(&self) {
        let shown = {
            let mut inner = self.voice_inner();
            inner.indicator = Indicator::None;
            std::mem::take(&mut inner.overlay)
        };
        if shown {
            self.parts.events.indicator_listening(false);
        }
    }

    /// Open the microphone for a turn (`StartListening`).
    ///
    /// # Errors
    ///
    /// See pax-voice.md, section 6.1.
    pub fn start_listening(self: &Arc<Self>, options: &str) -> Result<String, Fault> {
        let options: Map<String, Value> = if options.trim().is_empty() {
            Map::new()
        } else {
            crate::strict::object_from_str(options)
                .map_err(|_| invalid("the options are not one JSON object"))?
        };
        let mut trigger = Trigger::PushToTalk;
        let mut shown_by_caller = false;
        for (name, value) in &options {
            match (name.as_str(), value) {
                ("trigger", Value::String(text)) => {
                    trigger = Trigger::from_caller(text)
                        .ok_or_else(|| invalid("the trigger is push-to-talk or button"))?;
                }
                ("shown_by_caller", Value::Bool(shown)) => shown_by_caller = *shown,
                _ => return Err(invalid("an option is not known or has the wrong type")),
            }
        }
        self.voice_allowed()?;
        {
            let inner = self.voice_inner();
            if !matches!(
                inner.machine.phase(),
                Phase::Ready | Phase::Speaking | Phase::Waiting { .. }
            ) {
                return Err(coded(TaskCode::ServiceBusy));
            }
        }
        self.recognition_model()?;
        let indicator = self.show_listening(shown_by_caller);
        if let Err(refused) = may_listen(trigger, indicator) {
            self.hide_listening();
            return Err(refused);
        }
        let other_sound = sound_around(&self.parts.paths.sound.dump).other_sound;
        let id = {
            let mut inner = self.voice_inner();
            inner.counter += 1;
            format!("v-{}-{}", self.now().seconds(), inner.counter)
        };
        let effects = {
            let mut inner = self.voice_inner();
            inner.indicator = indicator;
            inner.pending = None;
            inner.machine.step(Input::Listen {
                turn: id.clone(),
                trigger,
            })
        };
        if effects.contains(&Effect::StopSpeech) {
            self.stop_speech_now();
        }
        if !effects.contains(&Effect::OpenMicrophone) {
            self.hide_listening();
            return Err(coded(TaskCode::ServiceBusy));
        }
        let activity = Arc::new(Mutex::new((false, Instant::now())));
        let microphone = match self.open_microphone(&id, Some(Arc::clone(&activity))) {
            Ok(microphone) => microphone,
            Err(problem) => {
                self.voice_inner().machine.step(Input::Stop);
                self.hide_listening();
                self.voice_changed(Some("microphone"));
                return Err(problem);
            }
        };
        // A task that runs is paused while the person speaks to P@x.
        let paused_task = {
            let mut state = self.state();
            state.running.as_mut().map(|running| {
                running.control.pause();
                running.id.clone()
            })
        };
        {
            let mut inner = self.voice_inner();
            inner.microphone = Some(microphone);
            inner.turn = Some(Turn {
                id: id.clone(),
                trigger,
                other_sound,
                paused_task,
            });
        }
        self.voice_changed(None);
        self.watch_turn(&id, trigger, activity);
        Ok(id)
    }

    fn open_microphone(
        &self,
        turn: &str,
        activity: Option<Arc<Mutex<(bool, Instant)>>>,
    ) -> Result<Microphone, Fault> {
        let me = lock(&self.voice.me).clone();
        let turn = turn.to_owned();
        Microphone::open(
            &self.parts.paths.sound.record,
            Box::new(move |loudness| {
                if loudness >= 0.06
                    && let Some(activity) = &activity
                {
                    *lock(activity) = (true, Instant::now());
                }
                if let Some(core) = me.upgrade() {
                    let due = {
                        let mut last = lock(&core.voice.last_level);
                        if last.is_none_or(|at| at.elapsed() >= LEVEL_INTERVAL) {
                            *last = Some(Instant::now());
                            true
                        } else {
                            false
                        }
                    };
                    if due {
                        core.emit(Event::VoiceLevel {
                            turn: turn.clone(),
                            level: loudness,
                        });
                    }
                }
            }),
        )
    }

    /// End a turn by itself: at its longest, after a silence once speech
    /// was heard (the button), or when the capture program ended.
    fn watch_turn(self: &Arc<Self>, turn: &str, trigger: Trigger, activity: Arc<Mutex<(bool, Instant)>>) {
        let core = Arc::downgrade(self);
        let turn = turn.to_owned();
        let started = Instant::now();
        let _ = thread::Builder::new()
            .name("tur-pax-voice-turn".to_owned())
            .spawn(move || {
                loop {
                    thread::sleep(Duration::from_millis(100));
                    let Some(core) = core.upgrade() else {
                        return;
                    };
                    let (still, ended) = {
                        let inner = core.voice_inner();
                        let still = matches!(
                            inner.machine.phase(),
                            Phase::Listening { turn: open, .. } if *open == turn
                        );
                        let ended = inner
                            .microphone
                            .as_ref()
                            .is_some_and(Microphone::ended);
                        (still, ended)
                    };
                    if !still {
                        return;
                    }
                    let (heard, last) = *lock(&activity);
                    let silent =
                        trigger == Trigger::Button && heard && last.elapsed() >= SILENCE_ENDS_TURN;
                    if ended {
                        let _ = core.stop_listening(&turn, false);
                        core.voice_changed(Some("microphone"));
                        return;
                    }
                    if started.elapsed() >= MAX_TURN || silent {
                        let _ = core.stop_listening(&turn, true);
                        return;
                    }
                }
            });
    }

    /// Close the microphone (`StopListening`): recognise what was heard
    /// (`send`), or drop it.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if that turn is not listening.
    pub fn stop_listening(self: &Arc<Self>, turn: &str, send: bool) -> Result<(), Fault> {
        let (effects, microphone, info) = {
            let mut inner = self.voice_inner();
            let effects = inner.machine.step(Input::Release {
                turn: turn.to_owned(),
                send,
            });
            if effects.is_empty() {
                return Err(not_found("listening turn"));
            }
            (effects, inner.microphone.take(), inner.turn.take())
        };
        let samples = microphone.map(Microphone::close).unwrap_or_default();
        self.hide_listening();
        self.voice_changed(Some(if send { "sent" } else { "discarded" }));
        let Some(info) = info else {
            return Ok(());
        };
        if !effects.contains(&Effect::CloseAndRecognise) {
            self.resume_paused(info.paused_task.as_deref());
            return Ok(());
        }
        let core = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("tur-pax-voice-recognise".to_owned())
            .spawn(move || core.recognised(info, &samples));
        if spawned.is_err() {
            self.voice_inner().machine.step(Input::Stop);
            self.voice_changed(Some("recognition"));
        }
        Ok(())
    }

    fn resume_paused(&self, task: Option<&str>) {
        let Some(task) = task else {
            return;
        };
        let mut state = self.state();
        if let Some(running) = state.running.as_mut()
            && running.id == task
            && !running.paused
            && !running.paused_by_lock
        {
            running.control.resume();
        }
    }

    fn recognise(&self, samples: &[i16]) -> Result<Transcript, Fault> {
        let model = self.recognition_model()?;
        let language = self.voice_inner().settings.language.clone();
        let runtime = self
            .parts
            .paths
            .runtime
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        audio::recognise(
            &self.parts.paths.ai_local,
            &runtime,
            samples,
            &model,
            &language,
        )
    }

    /// What was heard becomes a request, waits for the person, or speaks to
    /// the task that runs.
    fn recognised(self: &Arc<Self>, turn: Turn, samples: &[i16]) {
        let result = self.recognise(samples);
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(problem) => {
                self.voice_inner().machine.step(Input::Recognised {
                    turn: turn.id.clone(),
                    waits: false,
                });
                self.resume_paused(turn.paused_task.as_deref());
                self.emit(Event::VoiceTranscript {
                    turn: turn.id.clone(),
                    details: json!({
                        "text": "", "final": true, "empty": true,
                        "error_code": problem.code.to_string(), "message": problem.detail,
                    }),
                });
                self.voice_changed(Some("recognition"));
                return;
            }
        };
        self.voice_inner().spoken_language = Some(transcript.language.clone());
        let mut details = json!({
            "text": transcript.text,
            "final": true,
            "language": transcript.language,
            "model": transcript.model,
            "seconds": transcript.seconds,
            "audio_seconds": transcript.audio_seconds,
            "other_sound": turn.other_sound,
            "empty": transcript.text.is_empty(),
        });
        if transcript.text.is_empty() || !is_request(&transcript.text) {
            self.voice_inner().machine.step(Input::Recognised {
                turn: turn.id.clone(),
                waits: false,
            });
            self.resume_paused(turn.paused_task.as_deref());
            details["empty"] = json!(true);
            self.emit(Event::VoiceTranscript {
                turn: turn.id.clone(),
                details,
            });
            self.voice_changed(Some("nothing_heard"));
            return;
        }
        // Spoken to a task that runs: stop it, let it go on, or keep the
        // words for the person.
        let running = self.state().running.as_ref().map(|running| running.id.clone());
        if let Some(task) = running {
            match interjection(&transcript.text) {
                Interjection::Stop => {
                    let _ = self.cancel_task(&task);
                    details["check"] = json!("none");
                    details["reason"] = json!("stopped_the_task");
                    self.voice_inner().machine.step(Input::Recognised {
                        turn: turn.id.clone(),
                        waits: false,
                    });
                }
                Interjection::Resume => {
                    self.resume_paused(turn.paused_task.as_deref());
                    details["check"] = json!("none");
                    details["reason"] = json!("resumed_the_task");
                    self.voice_inner().machine.step(Input::Recognised {
                        turn: turn.id.clone(),
                        waits: false,
                    });
                }
                Interjection::Other => {
                    self.resume_paused(turn.paused_task.as_deref());
                    self.keep_for_person(&turn, &transcript.text, WaitReason::TaskRunning, &mut details);
                }
            }
            self.emit(Event::VoiceTranscript {
                turn: turn.id.clone(),
                details,
            });
            self.voice_changed(None);
            return;
        }
        let other_sound =
            turn.other_sound || sound_around(&self.parts.paths.sound.dump).other_sound;
        details["other_sound"] = json!(other_sound);
        let (check, reason) = {
            let inner = self.voice_inner();
            transcript_check(&inner.settings, turn.trigger, other_sound, false)
        };
        match (check, reason) {
            (Check::Show, _) => {
                let seconds = self.voice_inner().settings.show_seconds;
                let origin = JobOrigin {
                    source: Source::Voice,
                    trigger: Some(turn.trigger),
                    gate: Some(TranscriptGate::shown_for(Duration::from_secs(u64::from(
                        seconds,
                    )))),
                    interjections: None,
                    live: None,
                };
                self.voice_inner().machine.step(Input::Recognised {
                    turn: turn.id.clone(),
                    waits: false,
                });
                details["check"] = json!(Check::Show.as_str());
                details["show_seconds"] = json!(seconds);
                match self.start_task_from(&transcript.text, &[], true, origin) {
                    Ok(task) => details["task"] = json!(task),
                    Err(problem) => {
                        details["error_code"] = json!(problem.code.to_string());
                        details["message"] = json!(problem.detail);
                    }
                }
            }
            (Check::Wait, reason) => {
                self.keep_for_person(
                    &turn,
                    &transcript.text,
                    reason.unwrap_or(WaitReason::Setting),
                    &mut details,
                );
            }
        }
        self.emit(Event::VoiceTranscript {
            turn: turn.id.clone(),
            details,
        });
        self.voice_changed(None);
    }

    fn keep_for_person(&self, turn: &Turn, text: &str, reason: WaitReason, details: &mut Value) {
        let mut inner = self.voice_inner();
        inner.machine.step(Input::Recognised {
            turn: turn.id.clone(),
            waits: true,
        });
        inner.pending = Some(Pending {
            turn: turn.id.clone(),
            text: text.to_owned(),
            trigger: turn.trigger,
        });
        details["check"] = json!(Check::Wait.as_str());
        details["reason"] = json!(reason.as_str());
    }

    /// The person sends what was heard, or their correction of it
    /// (`SendTranscript`).
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if no transcript of that turn waits;
    /// `TUR-AI-SERVICE-006` for a text that cannot be a request; the
    /// refusals of `StartTask`.
    pub fn send_transcript(self: &Arc<Self>, turn: &str, text: &str) -> Result<String, Fault> {
        let pending = {
            let inner = self.voice_inner();
            match &inner.pending {
                Some(pending) if pending.turn == turn => Pending {
                    turn: pending.turn.clone(),
                    text: pending.text.clone(),
                    trigger: pending.trigger,
                },
                _ => return Err(not_found("transcript")),
            }
        };
        let source = if text.trim() == pending.text.trim() {
            Source::Voice
        } else {
            Source::VoiceEdited
        };
        let origin = JobOrigin {
            source,
            trigger: Some(pending.trigger),
            // The person sent it on screen: what they saw is what runs.
            gate: Some(TranscriptGate::sent()),
            interjections: None,
            live: None,
        };
        let task = self.start_task_from(text, &[], true, origin)?;
        {
            let mut inner = self.voice_inner();
            inner.pending = None;
            inner.machine.step(Input::Settled {
                turn: turn.to_owned(),
            });
        }
        self.voice_changed(None);
        Ok(task)
    }

    // ------------------------------------------------------------ speech

    /// Say a text in the local voice (`Speak`).
    ///
    /// # Errors
    ///
    /// `TUR-AI-VOICE-001` while voice is off; `TUR-AI-SERVICE-006` for an
    /// empty or long text.
    pub fn speak(self: &Arc<Self>, text: &str) -> Result<(), Fault> {
        if text.trim().is_empty() || text.len() > SPEAK_MAX_BYTES {
            return Err(invalid("the text to say is empty or longer than 4000 bytes"));
        }
        self.voice_allowed()?;
        let language = self.speaking_language();
        self.say(text.to_owned(), language);
        Ok(())
    }

    fn speaking_language(&self) -> String {
        let inner = self.voice_inner();
        if inner.settings.language == "auto" {
            inner
                .spoken_language
                .clone()
                .unwrap_or_else(|| "en".to_owned())
        } else {
            inner.settings.language.clone()
        }
    }

    /// Say something, after what is already being said. Nothing is said
    /// while the microphone is open for a turn, or after a stop.
    fn say(self: &Arc<Self>, text: String, language: String) {
        let generation = self.voice.generation.load(Ordering::SeqCst);
        let core = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("tur-pax-voice-say".to_owned())
            .spawn(move || core.say_now(&text, &language, generation));
    }

    fn say_now(self: &Arc<Self>, text: &str, language: &str, generation: u64) {
        let _one = lock(&self.voice.speech);
        let current = || self.voice.generation.load(Ordering::SeqCst) == generation;
        if !current() {
            return;
        }
        let voice = self.voice_for(language);
        let runtime = self
            .parts
            .paths
            .runtime
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        let Ok((samples, rate)) =
            audio::synthesise(&self.parts.paths.ai_local, &runtime, text, &voice)
        else {
            return;
        };
        if !current() {
            return;
        }
        let allowed = {
            let mut inner = self.voice_inner();
            inner.machine.step(Input::SpeakStart);
            inner.machine.speaking()
        };
        if !allowed {
            return;
        }
        self.voice_changed(None);
        if let Ok(mut speaker) = Speaker::start(&self.parts.paths.sound.play, rate) {
            speaker.write(&samples);
            speaker.finish();
            *lock(&self.voice.speaker) = Some(speaker);
            loop {
                thread::sleep(Duration::from_millis(50));
                let mut slot = lock(&self.voice.speaker);
                match slot.as_mut() {
                    None => break,
                    Some(speaker) if !speaker.playing() => {
                        slot.take();
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
        self.voice_inner().machine.step(Input::SpeakEnd);
        self.voice_changed(None);
    }

    /// Stop P@x's speech now, and drop what was still to be said.
    fn stop_speech_now(&self) {
        self.voice.generation.fetch_add(1, Ordering::SeqCst);
        let speaker = lock(&self.voice.speaker).take();
        if let Some(speaker) = speaker {
            speaker.stop();
        }
    }

    /// `StopSpeaking`.
    pub fn stop_speaking(&self) {
        self.stop_speech_now();
        let changed = {
            let mut inner = self.voice_inner();
            match inner.machine.phase() {
                Phase::Speaking => !inner.machine.step(Input::Stop).is_empty(),
                Phase::Live { .. } => {
                    inner.machine.step(Input::SpeakEnd);
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.voice_changed(Some("stop"));
        }
    }

    /// P@x's words between steps: said in a live conversation with the
    /// local engine, when narration is on.
    pub(super) fn voice_narrate(&self, task: &str, text: &str) {
        let narrate = {
            let inner = self.voice_inner();
            inner.live_tasks.contains(task) && inner.live.as_ref().is_some_and(|live| live.narrate)
        };
        if narrate && let Some(core) = self.me() {
            let language = core.speaking_language();
            let (sentences, rest) = complete_sentences(text);
            for sentence in sentences {
                core.say(sentence, language.clone());
            }
            if !rest.trim().is_empty() {
                core.say(rest.trim().to_owned(), language);
            }
        }
    }

    /// A task ended: its answer is said if the person asked for spoken
    /// answers, or in a live conversation with the local engine.
    pub(super) fn voice_task_finished(&self, job: &super::Job, answer: Option<&str>) {
        let speak = {
            let mut inner = self.voice_inner();
            let live = inner.live_tasks.remove(&job.id);
            let provider_live = job.origin.live.is_some();
            !provider_live
                && (live || (inner.settings.enabled && inner.settings.spoken_replies))
        };
        if let (true, Some(answer), Some(core)) = (speak, answer, self.me())
            && !answer.trim().is_empty()
        {
            let language = core.speaking_language();
            let text: String = answer.chars().take(SPEAK_MAX_BYTES / 4).collect();
            core.say(text, language);
        }
        // A live engine of a provider ends with its task.
        if job.origin.live.is_some() {
            self.end_live_run("ended");
        }
    }

    // --------------------------------------------------- live conversation

    /// Start a live conversation (`StartLiveSession`).
    ///
    /// # Errors
    ///
    /// See pax-voice.md, section 6.1.
    pub fn start_live_session(self: &Arc<Self>, options: &str) -> Result<String, Fault> {
        let options: Map<String, Value> = if options.trim().is_empty() {
            Map::new()
        } else {
            crate::strict::object_from_str(options)
                .map_err(|_| invalid("the options are not one JSON object"))?
        };
        let mut engine = None;
        let mut shown_by_caller = false;
        for (name, value) in &options {
            match (name.as_str(), value) {
                ("engine", Value::String(text)) => engine = Some(text.clone()),
                ("shown_by_caller", Value::Bool(shown)) => shown_by_caller = *shown,
                _ => return Err(invalid("an option is not known or has the wrong type")),
            }
        }
        self.voice_allowed()?;
        let settings = self.voice_inner().settings.clone();
        let engine = engine.unwrap_or_else(|| settings.live.engine.clone());
        {
            let inner = self.voice_inner();
            if inner.live.is_some() || !matches!(inner.machine.phase(), Phase::Ready | Phase::Speaking | Phase::Waiting { .. }) {
                return Err(coded(TaskCode::ServiceBusy));
            }
        }
        if self.state().running.is_some() {
            return Err(coded(TaskCode::ServiceBusy));
        }
        // The provider's engine, checked before anything opens.
        let provider = if engine == LOCAL_ENGINE {
            self.recognition_model()?;
            None
        } else {
            let lists = LiveList::load(&self.parts.paths.live)?;
            let entry = lists
                .entry(&engine)
                .filter(|entry| entry.built)
                .cloned()
                .ok_or_else(|| voice_fault(TaskCode::VoiceLiveUnavailable))?;
            if !settings.live_consent_current(&engine) {
                return Err(voice_fault(TaskCode::VoiceLiveConsent));
            }
            let model = entry.model(settings.live.model.as_deref())?.clone();
            if !self.parts.keys.is_set(&entry.keyring_item)? {
                return Err(coded(TaskCode::KeyNotSet));
            }
            Some((entry, model))
        };
        // A live conversation lasts: only P@x's indicator may show it.
        let indicator = self.show_listening(shown_by_caller);
        if let Err(refused) = may_listen(Trigger::Live, indicator) {
            self.hide_listening();
            return Err(refused);
        }
        let id = {
            let mut inner = self.voice_inner();
            inner.counter += 1;
            format!("l-{}-{}", self.now().seconds(), inner.counter)
        };
        let end = Arc::new(AtomicBool::new(false));
        let interjections = Interjections::new();
        let effects = {
            let mut inner = self.voice_inner();
            inner.indicator = indicator;
            let effects = inner.machine.step(Input::LiveStart {
                session: id.clone(),
            });
            inner.live = Some(LiveRun {
                id: id.clone(),
                engine: engine.clone(),
                end: Arc::clone(&end),
                narrate: settings.live.narrate,
            });
            effects
        };
        if effects.contains(&Effect::StopSpeech) {
            self.stop_speech_now();
        }
        self.voice_changed(None);
        let silence = Duration::from_secs(u64::from(settings.live.silence_timeout_seconds));
        let started = match provider {
            None => {
                let core = Arc::clone(self);
                let session = id.clone();
                thread::Builder::new()
                    .name("tur-pax-voice-live".to_owned())
                    .spawn(move || core.live_local(&session, &end, &interjections, silence))
                    .map(|_| ())
                    .map_err(|error| Fault::new(TaskCode::ServiceStorage, error.to_string()))
            }
            Some((entry, model)) => {
                let origin = JobOrigin {
                    source: Source::Live,
                    trigger: Some(Trigger::Live),
                    gate: Some(TranscriptGate::sent()),
                    interjections: None,
                    live: Some(LiveJob {
                        session: id.clone(),
                        prices: model.prices.as_ref().map(crate::livelist::ListedPrices::to_live),
                        model: model.name.clone(),
                        entry,
                        end,
                        silence,
                    }),
                };
                self.start_task_from("(a live conversation)", &[], true, origin)
                    .map(|_| ())
            }
        };
        if let Err(problem) = started {
            self.end_live_run("error");
            return Err(problem);
        }
        self.emit(Event::LiveEvent {
            session: id.clone(),
            event: json!({ "kind": "started", "engine": engine }),
        });
        Ok(id)
    }

    /// End the live conversation (`EndLiveSession`).
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if none runs.
    pub fn end_live_session(&self) -> Result<(), Fault> {
        let end = self
            .voice_inner()
            .live
            .as_ref()
            .map(|live| Arc::clone(&live.end))
            .ok_or_else(|| not_found("live conversation"))?;
        end.store(true, Ordering::SeqCst);
        self.stop_speech_now();
        Ok(())
    }

    /// Close a live conversation's microphone and say it ended.
    fn end_live_run(&self, reason: &str) {
        let (run, effects, microphone) = {
            let mut inner = self.voice_inner();
            let run = inner.live.take();
            let effects = if run.is_some() {
                inner.machine.step(Input::LiveEnd)
            } else {
                Vec::new()
            };
            (run, effects, inner.microphone.take())
        };
        let Some(run) = run else {
            return;
        };
        run.end.store(true, Ordering::SeqCst);
        if let Some(microphone) = microphone {
            microphone.close();
        }
        if effects.contains(&Effect::StopSpeech) {
            self.stop_speech_now();
        }
        self.hide_listening();
        self.emit(Event::LiveEvent {
            session: run.id,
            event: json!({ "kind": "ended", "reason": reason, "engine": run.engine }),
        });
        self.voice_changed(Some(reason));
    }

    /// The local live engine: the microphone open for the whole session,
    /// each utterance recognised on this computer and made a request of the
    /// person's own model, P@x's words said by the local voice.
    fn live_local(
        self: &Arc<Self>,
        session: &str,
        end: &Arc<AtomicBool>,
        interjections: &Interjections,
        silence: Duration,
    ) {
        let microphone = match self.open_microphone(session, None) {
            Ok(microphone) => microphone,
            Err(_) => {
                self.end_live_run("microphone");
                return;
            }
        };
        let echo_cancelled = sound_around(&self.parts.paths.sound.dump).echo_cancelled;
        let mut segmenter = Segmenter::new();
        let mut last_voice = Instant::now();
        let mut reason = "person";
        let mut current_task: Option<String> = None;
        loop {
            thread::sleep(Duration::from_millis(50));
            if end.load(Ordering::SeqCst) {
                break;
            }
            if microphone.ended() {
                reason = "microphone";
                break;
            }
            let running = self.state().running.as_ref().map(|running| running.id.clone());
            if running.is_none() {
                current_task = None;
            }
            let samples = microphone.take();
            let speaking = self.voice_inner().machine.speaking();
            if speaking && !echo_cancelled {
                // Self-sound: what is heard while P@x speaks is P@x.
                segmenter.reset();
                last_voice = Instant::now();
                continue;
            }
            for heard in segmenter.push(&samples) {
                match heard {
                    Heard::SpeechStarted => {
                        last_voice = Instant::now();
                        if speaking {
                            // Barge-in: the person speaks over P@x.
                            self.voice_inner()
                                .machine
                                .step(Input::VoiceWhileSpeaking { echo_cancelled });
                            self.stop_speech_now();
                            self.emit(Event::LiveEvent {
                                session: session.to_owned(),
                                event: json!({ "kind": "interrupted" }),
                            });
                        }
                    }
                    Heard::Utterance(utterance) => {
                        last_voice = Instant::now();
                        self.live_utterance(session, &utterance, interjections, &mut current_task);
                    }
                }
            }
            if running.is_none()
                && !segmenter.in_speech()
                && !speaking
                && last_voice.elapsed() >= silence
            {
                reason = "silence";
                break;
            }
        }
        drop(microphone);
        if let Some(task) = current_task {
            let _ = self.cancel_task(&task);
        }
        self.end_live_run(reason);
    }

    fn live_utterance(
        self: &Arc<Self>,
        session: &str,
        samples: &[i16],
        interjections: &Interjections,
        current_task: &mut Option<String>,
    ) {
        let Ok(transcript) = self.recognise(samples) else {
            return;
        };
        if transcript.text.is_empty() || !is_request(&transcript.text) {
            return;
        }
        self.voice_inner().spoken_language = Some(transcript.language.clone());
        self.emit(Event::LiveEvent {
            session: session.to_owned(),
            event: json!({ "kind": "heard", "text": transcript.text, "final": true }),
        });
        if let Some(task) = current_task.clone() {
            match interjection(&transcript.text) {
                Interjection::Stop => {
                    let _ = self.cancel_task(&task);
                    *current_task = None;
                }
                Interjection::Resume => {
                    let _ = self.resume();
                }
                Interjection::Other => interjections.push(&transcript.text),
            }
            return;
        }
        let seconds = self.voice_inner().settings.show_seconds;
        let origin = JobOrigin {
            source: Source::Live,
            trigger: Some(Trigger::Live),
            gate: Some(TranscriptGate::shown_for(Duration::from_secs(u64::from(seconds)))),
            interjections: Some(interjections.clone()),
            live: None,
        };
        if let Ok(task) = self.start_task_from(&transcript.text, &[], true, origin) {
            self.voice_inner().live_tasks.insert(task.clone());
            *current_task = Some(task);
        }
    }

    /// Connect a provider's live engine for a job, with the task's tools,
    /// and open its microphone.
    pub(super) fn connect_live(
        &self,
        live: &LiveJob,
        registry: &ToolRegistry,
    ) -> Result<(Box<dyn LiveSession>, ServiceLiveIo<'_>), Fault> {
        let family: LiveFamily = live
            .entry
            .family
            .parse()
            .map_err(|_| voice_fault(TaskCode::VoiceLiveUnavailable))?;
        let tools = registry.definitions()?;
        let config = LiveConfig {
            family,
            model: live.model.clone(),
            base_url: live.entry.base_url.clone(),
            key_header: live.entry.key_header.clone(),
            voice: live.entry.voices.first().cloned(),
            instructions: format!("{SYSTEM_TEXT}\n\n{VOICE_INSTRUCTIONS}"),
            tools,
            prices: live.prices.clone(),
            transcription_model: live.entry.transcription_model.clone(),
        };
        let key = self.parts.keys.key(&live.entry.keyring_item)?;
        let session = connect_live(&config, key).map_err(|error| Fault {
            code: error.kind().error_code(),
            detail: error.to_string(),
        })?;
        let microphone = self.open_microphone(&live.session, None)?;
        let echo_cancelled = sound_around(&self.parts.paths.sound.dump).echo_cancelled;
        Ok((
            session,
            ServiceLiveIo {
                core: self,
                session: live.session.clone(),
                microphone,
                speaker: None,
                rate: 0,
                until: Instant::now(),
                echo_cancelled,
                silence: live.silence,
                last_voice: Instant::now(),
                end: Arc::clone(&live.end),
                locked_at_start: self.state().locked == Some(true),
            },
        ))
    }

    /// Stop everything voice does: the turn, the speech, the live
    /// conversation. Returns whether anything was under way.
    pub(super) fn voice_stop_all(&self, reason: &str) -> bool {
        let (turn, live, speaking, pending) = {
            let inner = self.voice_inner();
            (
                inner.turn.as_ref().map(|turn| turn.id.clone()),
                inner.live.is_some(),
                inner.machine.speaking(),
                inner.pending.is_some(),
            )
        };
        let mut any = false;
        if turn.is_some() {
            let (microphone, info) = {
                let mut inner = self.voice_inner();
                inner.machine.step(Input::Stop);
                (inner.microphone.take(), inner.turn.take())
            };
            if let Some(info) = info {
                self.resume_paused(info.paused_task.as_deref());
            }
            if let Some(microphone) = microphone {
                microphone.close();
            }
            self.hide_listening();
            any = true;
        }
        if speaking {
            self.stop_speech_now();
            let mut inner = self.voice_inner();
            if *inner.machine.phase() == Phase::Speaking {
                inner.machine.step(Input::Stop);
            }
            any = true;
        }
        if pending {
            let mut inner = self.voice_inner();
            inner.pending = None;
            inner.machine.step(Input::Stop);
            any = true;
        }
        if live {
            self.end_live_run(reason);
            any = true;
        }
        if any {
            self.voice_changed(Some(reason));
        }
        any
    }

    /// The voice's resting state follows P@x's switch.
    pub(super) fn voice_follow_switch(&self) {
        let pax_on = self.is_enabled();
        let effects = {
            let mut inner = self.voice_inner();
            let resting = resting_phase(&inner, pax_on);
            inner.machine.rest(resting)
        };
        self.perform(&effects);
        self.voice_changed(None);
    }

    fn perform(&self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::StopSpeech => self.stop_speech_now(),
                Effect::CloseAndDrop | Effect::CloseAndRecognise => {
                    let microphone = self.voice_inner().microphone.take();
                    if let Some(microphone) = microphone {
                        microphone.close();
                    }
                    self.hide_listening();
                }
                Effect::OpenMicrophone | Effect::PauseTask | Effect::DropOwnSound => {}
            }
        }
    }
}

/// The microphone and the speaker of a live conversation with a provider's
/// engine, for the task loop.
pub(super) struct ServiceLiveIo<'c> {
    core: &'c Core,
    session: String,
    microphone: Microphone,
    speaker: Option<Speaker>,
    rate: u32,
    /// Until when P@x's voice is heard, from what was written.
    until: Instant,
    echo_cancelled: bool,
    silence: Duration,
    last_voice: Instant,
    end: Arc<AtomicBool>,
    locked_at_start: bool,
}

impl ServiceLiveIo<'_> {
    fn speaking(&self) -> bool {
        Instant::now() < self.until + Duration::from_millis(200)
    }
}

impl LiveIo for ServiceLiveIo<'_> {
    fn take_audio(&mut self) -> Vec<i16> {
        let samples = self.microphone.take();
        if level(&samples) >= 0.06 {
            self.last_voice = Instant::now();
        }
        if self.speaking() && !self.echo_cancelled {
            // Self-sound: what the microphone hears now is P@x.
            return Vec::new();
        }
        samples
    }

    fn play(&mut self, samples: &[i16], rate: u32) {
        if self.speaker.is_none() || self.rate != rate {
            if let Some(old) = self.speaker.take() {
                old.stop();
            }
            self.speaker = Speaker::start(&self.core.parts.paths.sound.play, rate).ok();
            self.rate = rate;
        }
        if let Some(speaker) = self.speaker.as_mut() {
            speaker.write(samples);
            let length = Duration::from_millis(samples.len() as u64 * 1000 / u64::from(rate.max(1)));
            self.until = self.until.max(Instant::now()) + length;
            self.core.voice_inner().machine.step(Input::SpeakStart);
        }
    }

    fn stop_playing(&mut self) -> u64 {
        self.until = Instant::now();
        self.core.voice_inner().machine.step(Input::SpeakEnd);
        self.speaker.take().map_or(0, Speaker::stop)
    }

    fn heard(&mut self, text: &str, final_text: bool) {
        self.last_voice = Instant::now();
        self.core.emit(Event::LiveEvent {
            session: self.session.clone(),
            event: json!({ "kind": "heard", "text": text, "final": final_text }),
        });
    }

    fn said(&mut self, text: &str, final_text: bool) {
        self.core.emit(Event::LiveEvent {
            session: self.session.clone(),
            event: json!({ "kind": "said", "text": text, "final": final_text }),
        });
    }

    fn should_end(&mut self) -> Option<&'static str> {
        if self.end.load(Ordering::SeqCst) {
            Some("person")
        } else if self.microphone.ended() {
            Some("microphone")
        } else if !self.locked_at_start && self.core.state().locked == Some(true) {
            Some("locked")
        } else if !self.speaking() && self.last_voice.elapsed() >= self.silence {
            Some("silence")
        } else {
            None
        }
    }
}


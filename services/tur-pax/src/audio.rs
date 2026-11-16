// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The microphone, the speaker and local speech, as separate programs.
//!
//! P@x links nothing of the sound server or of the speech engines. It runs:
//!
//! * PipeWire's own `pw-record --raw` to capture 16 kHz mono 16-bit audio,
//!   read from its standard output into memory ([`Microphone`]);
//! * PipeWire's own `pw-play --raw` to play P@x's voice, written to its
//!   standard input ([`Speaker`]);
//! * PipeWire's `pw-dump` to see whether another program is playing sound
//!   and whether the capture is echo-cancelled ([`sound_around`]);
//! * local AI's speech commands, `tur-ai-local speech stt` and `speech tts`
//!   ([`recognise`], [`synthesise`]), which run whisper.cpp and Piper.
//!
//! **What is kept.** The capture is held in memory. Recognition needs a
//! file: it is written readable by its owner only, in the session's private
//! runtime folder, and deleted as soon as local AI returns, whatever it
//! returned. A synthesised answer's file is deleted after it was read.
//! Nothing here writes a log of what was heard or said.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::faults::{Fault, TaskCode};
use crate::voice::{
    MAX_TURN, OWN_APPLICATION_NAME, SAMPLE_RATE, SPEAK_MAX_BYTES, echo_cancelled_source, level,
    other_sound_playing, wav_bytes, wav_samples,
};

/// The programs of the sound server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundPrograms {
    /// `pw-record`.
    pub record: PathBuf,
    /// `pw-play`.
    pub play: PathBuf,
    /// `pw-dump`.
    pub dump: PathBuf,
}

impl Default for SoundPrograms {
    fn default() -> Self {
        SoundPrograms {
            record: PathBuf::from("/usr/bin/pw-record"),
            play: PathBuf::from("/usr/bin/pw-play"),
            dump: PathBuf::from("/usr/bin/pw-dump"),
        }
    }
}

/// The stream properties P@x's own streams carry, so that the sound
/// settings name them and P@x can tell its own sound from another's.
fn stream_properties(what: &str) -> String {
    serde_json::json!({
        "application.name": OWN_APPLICATION_NAME,
        "media.name": what,
    })
    .to_string()
}

fn microphone_fault(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::VoiceNoMicrophone, detail)
}

fn speech_fault(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::VoiceSpeechFailed, detail)
}

/// The microphone, open. Closing it ends the capture program.
pub struct Microphone {
    child: Child,
    buffer: Arc<Mutex<Vec<i16>>>,
    taken: Arc<AtomicU64>,
    reader: Option<JoinHandle<()>>,
    ended: Arc<AtomicBool>,
    opened: Instant,
}

impl Microphone {
    /// Open the microphone. `on_level` is called about ten times a second
    /// with the loudness of what was just heard.
    ///
    /// # Errors
    ///
    /// `TUR-AI-VOICE-003` if the capture program cannot be started.
    pub fn open(
        program: &Path,
        on_level: Box<dyn Fn(f64) + Send>,
    ) -> Result<Microphone, Fault> {
        let mut child = Command::new(program)
            .args([
                "--raw",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                "--media-role",
                "Communication",
                "--properties",
            ])
            .arg(stream_properties("P@x is listening"))
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| microphone_fault(format!("{}: {error}", program.display())))?;
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(microphone_fault("the capture program gave no output"));
        };
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let taken = Arc::new(AtomicU64::new(0));
        let ended = Arc::new(AtomicBool::new(false));
        let reader = {
            let buffer = Arc::clone(&buffer);
            let ended = Arc::clone(&ended);
            thread::Builder::new()
                .name("tur-pax-microphone".to_owned())
                .spawn(move || {
                    // A tenth of a second at a time.
                    let mut chunk = vec![0u8; (SAMPLE_RATE as usize / 10) * 2];
                    let mut odd: Option<u8> = None;
                    let most = SAMPLE_RATE as usize * MAX_TURN.as_secs() as usize * 4;
                    loop {
                        let read = match stdout.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(read) => read,
                        };
                        let mut bytes: Vec<u8> = Vec::with_capacity(read + 1);
                        if let Some(first) = odd.take() {
                            bytes.push(first);
                        }
                        bytes.extend_from_slice(&chunk[..read]);
                        if bytes.len() % 2 == 1 {
                            odd = bytes.pop();
                        }
                        let samples: Vec<i16> = bytes
                            .chunks_exact(2)
                            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                            .collect();
                        on_level(level(&samples));
                        let mut held = buffer.lock().unwrap_or_else(PoisonError::into_inner);
                        // Never more than a few turns' worth in memory.
                        if held.len() < most {
                            held.extend_from_slice(&samples);
                        }
                    }
                    ended.store(true, Ordering::SeqCst);
                })
                .map_err(|error| microphone_fault(error.to_string()))?
        };
        Ok(Microphone {
            child,
            buffer,
            taken,
            reader: Some(reader),
            ended,
            opened: Instant::now(),
        })
    }

    /// Whether the capture program ended by itself: no microphone, or the
    /// sound server went away.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// How long it has been open.
    pub fn open_for(&self) -> Duration {
        self.opened.elapsed()
    }

    /// Take what was heard since the last call: for a live conversation,
    /// which streams it.
    pub fn take(&self) -> Vec<i16> {
        let samples =
            std::mem::take(&mut *self.buffer.lock().unwrap_or_else(PoisonError::into_inner));
        self.taken
            .fetch_add(samples.len() as u64, Ordering::SeqCst);
        samples
    }

    /// Close the microphone and return what was heard and not taken.
    pub fn close(mut self) -> Vec<i16> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        std::mem::take(&mut *self.buffer.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// P@x's voice, playing.
pub struct Speaker {
    child: Child,
    stdin: Option<ChildStdin>,
    rate: u32,
    written: u64,
    started: Instant,
}

impl Speaker {
    /// Start a stream at `rate` samples a second, mono.
    ///
    /// # Errors
    ///
    /// `TUR-AI-VOICE-006` if the playing program cannot be started.
    pub fn start(program: &Path, rate: u32) -> Result<Speaker, Fault> {
        let mut child = Command::new(program)
            .args(["--raw", "--rate"])
            .arg(rate.to_string())
            .args([
                "--channels",
                "1",
                "--format",
                "s16",
                "--media-role",
                "Communication",
                "--properties",
            ])
            .arg(stream_properties("P@x is speaking"))
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| speech_fault(format!("{}: {error}", program.display())))?;
        let stdin = child.stdin.take();
        Ok(Speaker {
            child,
            stdin,
            rate: rate.max(1),
            written: 0,
            started: Instant::now(),
        })
    }

    /// Add samples to what is played.
    pub fn write(&mut self, samples: &[i16]) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if stdin.write_all(&bytes).is_ok() {
            self.written += samples.len() as u64;
        }
    }

    /// No more samples: the program ends once it has played what it has.
    pub fn finish(&mut self) {
        self.stdin = None;
    }

    /// Whether it is still playing.
    pub fn playing(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// How long all that was written lasts.
    pub fn length(&self) -> Duration {
        Duration::from_millis(self.written * 1000 / u64::from(self.rate))
    }

    /// Stop at once. Returns how many milliseconds were played, as far as
    /// can be told: the time since the start, at most what was written.
    pub fn stop(mut self) -> u64 {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        let played = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        played.min(self.written * 1000 / u64::from(self.rate))
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What the sound server says about the sound around P@x.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundAround {
    /// Another program is playing sound (or it could not be told: the
    /// stricter reading).
    pub other_sound: bool,
    /// The capture source is PipeWire's echo canceller.
    pub echo_cancelled: bool,
}

/// Ask `pw-dump` once.
pub fn sound_around(program: &Path) -> SoundAround {
    let output = Command::new(program)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            SoundAround {
                other_sound: other_sound_playing(&text, std::process::id()),
                echo_cancelled: echo_cancelled_source(&text),
            }
        }
        _ => SoundAround {
            other_sound: true,
            echo_cancelled: false,
        },
    }
}

/// What local AI heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// The words.
    pub text: String,
    /// The language recognised or asked for.
    pub language: String,
    /// The recognition model.
    pub model: String,
    /// How long recognition took.
    pub seconds: f64,
    /// How long the audio was.
    pub audio_seconds: f64,
}

/// The folder for the one file recognition needs: private to the person,
/// emptied at log-out.
fn voice_folder(runtime: &Path) -> PathBuf {
    runtime.join("tur-pax").join("voice")
}

static FILES: AtomicU64 = AtomicU64::new(0);

fn private_file(runtime: &Path, stem: &str) -> PathBuf {
    let number = FILES.fetch_add(1, Ordering::SeqCst);
    voice_folder(runtime).join(format!("{stem}-{}-{number}.wav", std::process::id()))
}

/// Recognise what was heard with local AI. The audio file is deleted
/// before this returns, whatever happened.
///
/// # Errors
///
/// `TUR-AI-VOICE-006` with local AI's code and detail; `TUR-AI-SERVICE-009`
/// if the private file cannot be written.
pub fn recognise(
    ai_local: &Path,
    runtime: &Path,
    samples: &[i16],
    model: &str,
    language: &str,
) -> Result<Transcript, Fault> {
    let file = private_file(runtime, "heard");
    crate::settings::write_private(&file, &wav_bytes(samples, SAMPLE_RATE))?;
    let path = file.to_string_lossy().into_owned();
    let result = crate::local::report(
        ai_local,
        &[
            "speech", "stt", &path, "--model", model, "--language", language, "--vad", "--json",
        ],
    );
    let _ = std::fs::remove_file(&file);
    let report = result.map_err(|fault| speech_fault(fault.to_string()))?;
    let text = report
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| speech_fault("local AI's report has no text"))?;
    #[allow(clippy::cast_precision_loss)]
    let audio_seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
    Ok(Transcript {
        text: text.trim().to_owned(),
        language: report
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or(language)
            .to_owned(),
        model: model.to_owned(),
        seconds: report.get("seconds").and_then(Value::as_f64).unwrap_or(0.0),
        audio_seconds,
    })
}

/// Say a text in a local voice: local AI writes a WAV file, which is read
/// and deleted. Returns the samples and their rate.
///
/// # Errors
///
/// `TUR-AI-VOICE-006`.
pub fn synthesise(
    ai_local: &Path,
    runtime: &Path,
    text: &str,
    voice: &str,
) -> Result<(Vec<i16>, u32), Fault> {
    if text.trim().is_empty() || text.len() > SPEAK_MAX_BYTES {
        return Err(speech_fault("the text to say is empty or too long"));
    }
    let file = private_file(runtime, "say");
    let directory = voice_folder(runtime);
    let _ = std::fs::DirBuilder::new()
        .recursive(true)
        .create(&directory);
    let result = run_synthesis(ai_local, &file, text, voice);
    let _ = std::fs::remove_file(&file);
    result
}

fn run_synthesis(
    ai_local: &Path,
    file: &Path,
    text: &str,
    voice: &str,
) -> Result<(Vec<i16>, u32), Fault> {
    let path = file.to_string_lossy().into_owned();
    let mut child = Command::new(ai_local)
        .args(["speech", "tts", "--voice", voice, "--output", &path, "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| speech_fault(format!("{}: {error}", ai_local.display())))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|error| speech_fault(error.to_string()))?;
    if !output.status.success() {
        let fault = crate::local::fault_of(&String::from_utf8_lossy(&output.stderr));
        return Err(speech_fault(fault.to_string()));
    }
    let bytes = std::fs::read(file).map_err(|error| speech_fault(error.to_string()))?;
    wav_samples(&bytes).ok_or_else(|| speech_fault("the voice's file is not 16-bit mono"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
        path
    }

    fn temporary(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tur-pax-audio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    #[test]
    fn the_microphone_reads_what_the_capture_program_prints_and_ends_it() {
        let dir = temporary("mic");
        // 0.2 s of a loud square wave, then it keeps the pipe open.
        let record = script(
            &dir,
            "pw-record",
            "head -c 6400 /dev/zero | tr '\\000' '\\100'; exec sleep 30",
        );
        let levels = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&levels);
        let microphone = Microphone::open(
            &record,
            Box::new(move |value| seen.lock().unwrap_or_else(PoisonError::into_inner).push(value)),
        )
        .expect("opened");
        let started = Instant::now();
        while levels.lock().map(|levels| levels.len()).unwrap_or(0) < 2
            && started.elapsed() < Duration::from_secs(10)
        {
            thread::sleep(Duration::from_millis(20));
        }
        let samples = microphone.close();
        assert_eq!(samples.len(), 3200);
        assert!(samples.iter().all(|sample| *sample == 0x4040));
        assert!(levels.lock().map(|levels| levels.iter().all(|l| *l > 0.5)).unwrap_or(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_capture_program_is_a_microphone_fault() {
        let refused = Microphone::open(Path::new("/nonexistent/pw-record"), Box::new(|_| {}))
            .err()
            .expect("refused");
        assert_eq!(refused.code.as_str(), "TUR-AI-VOICE-003");
    }

    #[test]
    fn recognition_deletes_the_audio_whatever_local_ai_answered() {
        let dir = temporary("stt");
        let runtime = dir.join("runtime");
        // Local AI's stand-in: checks the file is there and answers.
        let good = script(
            &dir,
            "tur-ai-local",
            r#"test -s "$3" || exit 3; echo '{"schema_version":1,"kind":"stt","model":"whisper-base-q5_1","language":"it","text":" Apri la fattura. ","audio_seconds":1.0,"seconds":0.4}'"#,
        );
        let transcript =
            recognise(&good, &runtime, &[0; 16000], "whisper-base-q5_1", "auto").expect("heard");
        assert_eq!(transcript.text, "Apri la fattura.");
        assert_eq!(transcript.language, "it");
        assert!((transcript.audio_seconds - 1.0).abs() < 1e-9);
        let left: Vec<_> = std::fs::read_dir(voice_folder(&runtime))
            .map(|entries| entries.flatten().collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
        let bad = script(&dir, "failing", "echo 'TUR-AI-LOCAL-032' >&2; echo 'failed' >&2; exit 1");
        let refused = recognise(&bad, &runtime, &[0; 1600], "m", "en").expect_err("refused");
        assert_eq!(refused.code.as_str(), "TUR-AI-VOICE-006");
        assert!(refused.detail.contains("TUR-AI-LOCAL-032"));
        let left: Vec<_> = std::fs::read_dir(voice_folder(&runtime))
            .map(|entries| entries.flatten().collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn synthesis_reads_the_voice_s_file_and_deletes_it() {
        let dir = temporary("tts");
        let runtime = dir.join("runtime");
        let wav = dir.join("fixture.wav");
        std::fs::write(&wav, wav_bytes(&[1, 2, 3, 4], 22_050)).expect("written");
        // Arguments: speech tts --voice V --output FILE --json
        let tts = script(
            &dir,
            "tur-ai-local",
            &format!("cat > /dev/null; cp {} \"$6\"; echo '{{}}'", wav.display()),
        );
        let (samples, rate) =
            synthesise(&tts, &runtime, "Ciao.", "voice-it-paola-medium").expect("said");
        assert_eq!(samples, [1, 2, 3, 4]);
        assert_eq!(rate, 22_050);
        let left: Vec<_> = std::fs::read_dir(voice_folder(&runtime))
            .map(|entries| entries.flatten().collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
        assert!(synthesise(&tts, &runtime, "  ", "v").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_the_sound_server_other_sound_is_assumed() {
        let around = sound_around(Path::new("/nonexistent/pw-dump"));
        assert!(around.other_sound);
        assert!(!around.echo_cancelled);
    }
}

<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# P@x's voice: speaking to P@x, P@x speaking, live conversation

**Status:** first slice written 16 November 2026 (branch `feat/pax-voice`).
**Nothing in it has run**: GitHub Actions is disabled for the owner's
account, so no test, build or measurement of this slice has been made. Every
speed in this document is an estimate from the engines' own published
figures, said so where it appears.
**Design it follows:** [ai-control-layer.md](ai-control-layer.md) section 9
"Voice", the threat "Instructions in audio" (section 7) and the owner's
decisions (section 23); [local-ai.md](local-ai.md) work item 5;
[pax-service.md](pax-service.md); [tur-pax-ui.md](tur-pax-ui.md); the
companion design on branch `pax-companion` (PR #139).
**Report:** [pax-voice-1](../reports/pax-voice-1.md).

The owner's requests (16 November 2026), in order:

1. "Add to P@x the voice functionality where the user can talk to it
   directly and it will recognize the speech."
2. "Support voice models also in response … enable voice responses and even
   start a normal conversation with the agent as it works for them, using
   models like gpt live or similar ones."
3. "Support local models as well and have basic low compute need speech
   recognition and text to speech models to run it for no money."
4. "Make sure it supports voice to voice like gpt live as well so it can
   work and be compatible with all types of technologies."

---

## 1. What the person gets

| Level | What it is | Default | Costs |
|---|---|---|---|
| **Speaking to P@x** | Hold P@x's key (Meta+Space), speak, let go: what was heard is shown, then becomes a request like a typed one. Or press the microphone in the ask bar | Off until the person switches voice on; then push-to-talk | Nothing: recognition on this computer |
| **P@x answers out loud** | P@x's answers are spoken, in a local voice | Off | Nothing: a local voice |
| **Live conversation** | The person and P@x talk naturally while P@x works: P@x says what it is doing, the person can interrupt, redirect, ask; P@x goes on working | Off; started by the person, ends by itself after silence | Local engine: nothing. A provider's live model: the person's own key, billed per minute of audio |

**The free voice comes first.** With no key and no account, on any computer
TUR runs on, voice works with local models only: a small recognition model
and a small voice, downloaded once on first use (the size is shown before
anything is downloaded), nothing in the image. Provider voices and live
models are listed after, marked "optional" and "paid with your own key".

**Always true, whatever the engine:**

- The microphone is open only while P@x is visibly listening: the ask bar's
  microphone is lit and P@x's indicator shows "Listening…"; for a live
  session, for the whole session. `MicrophoneOpen` on `org.tur.Agent1` says
  so to every program.
- Stop (Meta+Esc, the Stop button, the bar's microphone, Esc) closes the
  microphone, stops P@x's speech and, for a live session, ends it.
- Audio is never written anywhere but the session's private runtime folder,
  and is deleted the moment it has been recognised. It is never in the
  audit trail, a log or a diagnostic bundle. It leaves the computer only in
  a live session with a provider's engine the person chose, after consent
  that names the company.
- A voice request goes through the policy engine exactly as a typed one.
  **Class 3 never accepts voice. Class 2 is confirmed on screen.** Voice
  never approves anything in this build (no approval path exists yet,
  ai-control-layer.md section 23).

## 2. Architecture

```text
 tur-pax-ui (ask bar, microphone button, transcript)   P@x's indicator   the companion
        │ org.tur.Agent1: StartListening, StopListening,      ▲ listening     ▲ MicrophoneOpen,
        │ SendTranscript, Speak, StopSpeaking,                │ (lease)       │ VoiceStateChanged
        │ StartLiveSession, EndLiveSession                    │               │
        ▼                                                     │               │
 tur-pax serve ── voice.rs: the rules and the state machine ──┴───────────────┘
        │  microphone.rs: pw-record (PipeWire) → 16 kHz mono PCM in memory
        │  speaker.rs:    pw-play (PipeWire) ← WAV from the local voice, or a
        │                 live provider's audio
        │  the one task loop (orchestrator.rs), with the request's origin
        │       ├─ typed / voice turn: Orchestrator::run, same policy, same audit
        │       └─ live session:   Orchestrator::run_live (provider engine) or
        │                          run with spoken interjections (local engine)
        ▼
 tur-ai-local speech …   (separate program, as for local language models)
        ├─ stt:  whisper.cpp's whisper-cli, a Whisper model, Silero VAD
        └─ tts:  Piper (piper1-gpl, a GPL program, run as a child process)
 tur-pax-providers live.rs  (the only code that talks to a provider)
        ├─ openai-realtime  (WebSocket; OpenAI, Azure, compatible servers by configuration)
        ├─ gemini-live      (WebSocket)
        └─ scripted         (tests)
```

**Why recognition and synthesis live in `tur-ai-local`** and not in a new
service: local-ai.md (work item 5) put them there; the service already
downloads publisher files pinned by SHA-256, reads the hardware, sorts models
into tiers and builds the llama.cpp engine from source in the image, and
whisper.cpp is the same ggml family built the same way. A second service
would duplicate all of it. `tur-ai-local` stays a program that P@x runs;
it opens no microphone and plays nothing.

**Why the microphone is opened by `tur-pax`** and not by the window: the
service decides whether listening is allowed (P@x on, voice on, the session
unlocked, the indicator shown), owns the turn from the first sample to the
task, and must stop at Stop even if the window has gone. The window shows;
the service listens. The capture is PipeWire's own `pw-record --raw`
(MIT), a separate program, so TUR links nothing of PipeWire's. There is no
desktop portal for a host program's microphone; a host program uses
PipeWire directly, and PipeWire's own access control applies.

## 3. Engines (checked 16 November 2026)

### 3.1 Speech recognition: whisper.cpp

| | |
|---|---|
| Engine | whisper.cpp **v1.9.5** (2026-10-06), MIT, github.com/ggml-org/whisper.cpp; last push 2026-11-16, ~55k stars. Programs `whisper-cli` and `whisper-server`; built-in voice activity detection (`--vad`, Silero) |
| Fedora 45 | `whisper-cpp-1.8.3-4.fc45` ships **libraries only** (no `whisper-cli`, no `whisper-server`; `WHISPER_BUILD_EXAMPLES=OFF`), with AVX, AVX2, FMA, F16C off and no Vulkan. Not usable for TUR as it is |
| TUR's build | Built from the v1.9.5 source in the image's engine stage beside llama.cpp, with `GGML_BACKEND_DL=ON`, `GGML_CPU_ALL_VARIANTS=ON`, `GGML_NATIVE=OFF`, `GGML_VULKAN=ON`, `BUILD_SHARED_LIBS=ON`; into `/usr/libexec/tur/whisper.cpp/` (section 9) |
| Server | `whisper-server` has no Unix socket and no access key, and its `/load` takes any path. TUR does not run it in this slice; it runs `whisper-cli` once per utterance |
| Models | `ggerganov/whisper.cpp` on Hugging Face at commit `5359861c739e955e79d9a303bcbc70fb988958b1`; Whisper weights are MIT (openai/whisper README) |
| Voice activity | Silero VAD v6.2.0 in ggml form, `ggml-org/whisper-vad` at `9ffd54a1e1ee413ddf265af9913beaf518d1639b`, 885 098 bytes, MIT |

**Compared and not chosen:** Vosk (Apache-2.0, in Fedora as `vosk-api`
0.3.50; Italian small model 48 MB at 16.9 % WER on Common Voice: lighter but
clearly worse than Whisper small; a candidate for a future wake word);
Moonshine (MIT, but **no Italian recognition**); Distil-Whisper (MIT,
**English only**); NVIDIA Parakeet TDT 0.6B v3 (CC-BY-4.0, 25 European
languages, Italian FLEURS WER 3.0, runs in whisper.cpp 1.9's `parakeet-cli`:
**offered as an option** since the owner's decision 10, never the default,
with its credit shown);
sherpa-onnx (Apache-2.0, not in Fedora).

### 3.2 Speech synthesis: Piper

| | |
|---|---|
| Engine | **piper1-gpl v1.8.0** (2026-09-04), GPL-3.0-or-later, github.com/OHF-Voice/piper1-gpl (the original rhasspy/piper, MIT, was archived on 2025-10-06 and points here). Active (last push 2026-11-15); "looking for maintainers" |
| Boundary | A GPL program, run as a child process (`python3 -m piper -m VOICE -f OUT.wav`, text on standard input). TUR links nothing of it (AGENTS.md, licence boundary) |
| Fedora 45 | `python3-piper-tts-1.4.2-5.fc45`: **too old** for the voices retrained in November 2026, whose cards say "Requires Piper 1.5 or higher". `python3-onnxruntime-1.26.0-13.fc45` and `espeak-ng-1.52.0-4.fc45` are packaged |
| TUR's install | **On first use, with the voices, not in the image** (the owner's decision 11): the `piper-tts` 1.8.0 wheel from PyPI (34 131 442 bytes), pinned by its SHA-256 in the speech catalogue (`piper-tts-1-8-0`, kind `engine`) with its source archive for the GPL's offer, unpacked by `tur-ai-local speech ensure` into a private virtual environment that uses Fedora's onnxruntime and pathvalidate, which the image keeps (they are MIT; section 9) |
| Voices | `rhasspy/piper-voices` at commit `e9ab671f2a0e50a701b34361136ba3e6a3a9eb0c`. Piper grants no licence of its own over voices: each voice's licence is its dataset's, followed through every base voice it was trained from. Only voices clean through the whole chain are offered |

**Voices offered** (Piper's own "commercial use" list, read 16 November 2026):

| Voice | Language | Licence chain | Default |
|---|---|---|---|
| `en_US-ljspeech-medium` | English (US) | Public domain, trained from scratch | **English** |
| `en_GB-cori-medium` | English (UK) | CC BY 4.0 (Hi-Fi TTS) | |
| `it_IT-paola-medium` | Italian | CC0 (retrained 2026-11-13) on a CC BY 4.0 base (Hi-Fi TTS 92) | **Italian** |
| `de_DE-thorsten-medium` | German | CC0 on a CC0 base | German |
| `es_ES-davefx-medium` | Spanish | CC0 on a CC0 base | Spanish |
| `fr_FR-siwis-medium` | French | CC BY 4.0 on a CC BY 4.0 base | French |

**Not offered:** `en_US-lessac-medium` (research only, no commercial use),
`en_US-libritts_r-medium` (fine-tuned from lessac), `en_US-amy-medium` (no
documented licence), `it_IT-riccardo-x_low` (M-AILABS; licence page down,
not on Piper's list). Coqui XTTS-v2 (non-commercial), MeloTTS (no Italian),
Kokoro-82M (Apache-2.0 weights but Italian voices graded C, training data
partly synthetic from closed models), Mimic 3 (AGPL, unmaintained).

### 3.3 Tiers: what this computer runs well

Recognition is the heavy part; every offered voice runs faster than real
time on any processor TUR supports (Piper's medium voices: real-time factor
0.36 to 0.77 on a Raspberry Pi 4, sherpa-onnx's published figures; an
ordinary laptop is several times faster). The tiers are `tur-ai-local`'s:
*runs well*, *slow*, *does not fit*, each with its reasons in words.

| Recognition model (catalogue id) | File | Memory (estimate from the whisper.cpp README) | Italian WER, FLEURS (Whisper paper, full-precision model) | Offered as |
|---|---|---|---|---|
| `whisper-tiny-q5-1` | `ggml-tiny-q5_1.bin`, 32 152 673 B | ~270 MB | 29.8 % | the smallest machines (2 processors); good enough for English commands, weak in Italian |
| `whisper-base-q5-1` | `ggml-base-q5_1.bin`, 59 707 625 B | ~320 MB | 17.9 % | **the free tier: any PC with 4 processors** |
| `whisper-small-q5-1` | `ggml-small-q5_1.bin`, 190 085 487 B | ~850 MB | 9.8 % | ordinary laptops with AVX2 and 8 processors |
| `whisper-large-v3-turbo-q5-0` | `ggml-large-v3-turbo-q5_0.bin`, 574 041 195 B | ~1.6 GB | not published as a table | desktops with 16 processors, or a Vulkan graphics processor |
| `parakeet-tdt-0-6b-v3-q4-k` (optional) | `ggml-parakeet-tdt-0.6b-v3-q4_k.bin` (`ggml-org/parakeet-GGUF` @ `35156454…`), 415 611 879 B | estimate | 3.0 % | never the default; offered for Italian and 24 other European languages; CC BY 4.0, credit "Parakeet TDT 0.6B v3 by NVIDIA" shown; run by `parakeet-cli`, whose output format is undocumented |

Voice-activity detection: `silero-vad-v6.2.0` (885 098 B, MIT). Voices
(catalogue ids): `voice-en-us-ljspeech-medium`, `voice-en-gb-cori-medium`,
`voice-it-paola-medium`, `voice-de-thorsten-medium`,
`voice-es-davefx-medium`, `voice-fr-siwis-medium`; each about 63 MB plus
its 5 KB configuration, 22 050 Hz. The free tier's whole download (base,
the detector and the English and Italian voices): about 188 MB.

What the rule gives on the recorded machines of the tests
(`services/tur-ai-local/tests/speech.rs`): on the 2-processor virtual
machine (4 GB) only `whisper-tiny-q5-1` runs well (2.5 s expected) and is
recommended, `whisper-base-q5-1` is "slow" (5.9 s); on the 16-thread desktop
with a Vulkan card `whisper-large-v3-turbo-q5-0` runs well (1.7 s) and is
recommended. All estimates: nothing was measured by TUR.

**Speed rule** (estimates; nothing measured by TUR): whisper.cpp encodes a
30-second window whatever the utterance's length, so an utterance's wait is
about one encode plus a short decode. From the community benchmark the
project points to (issue #89; i7-11800H, 8 threads, 2022 build): encode
tiny 0.62 s, base 1.48 s, small 4.83 s, large 27.7 s; a 2026 Ryzen 7 6800H
encodes base.en in 0.66 s. The rule scales the 8-thread figure by threads
(at most 16) and multiplies by 4 without AVX2/FMA; a usable Vulkan device
divides by 8 (assumed). *Runs well*: the expected wait is 2.5 s or less;
*slow*: more; *does not fit*: the model's memory is more than the total with
the reserve. The default is the best model that runs well; with none, the
smallest that fits, said to be slow.

## 4. The rules, as built (`services/tur-pax/src/voice.rs`)

| Rule | How it holds |
|---|---|
| Push-to-talk is the default | `activation` is `push-to-talk` in the settings and nothing else exists in this build. The microphone opens on `StartListening` from a person's key or button, closes on `StopListening` |
| A wake word is optional and off | `wake_word` is `false` by default. Turning it on is refused with `TUR-AI-VOICE-005` in this build: no detector is built (section 10). The explanation the setting will show is in `voice.rs` (`WAKE_WORD_EXPLANATION`) |
| The microphone is visibly on | Listening needs P@x's indicator (a lease on `org.tur.PaxOverlay1` showing "listening") or a caller that says it shows the microphone itself (`shown_by_caller`, which the ask bar sets). A live session needs the indicator. `MicrophoneOpen` and `VoiceStateChanged` are sent before the first sample is read |
| Barge-in | Pressing the talk key, the microphone, Stop or Esc while P@x speaks stops its speech at once (`pw-play` is ended). In a live session, the person's speech stops P@x's speech when an echo-cancelled source is in use; otherwise only the key and the button do (section 4.2) |
| Self-sound | While P@x itself speaks, what the microphone hears is not recognised. While another program plays sound (any running PipeWire output stream not P@x's own, from `pw-dump`), a voice turn is never sent by itself: it waits for the person to send it on screen |
| The transcript first | Before anything above class 0 runs for a spoken request, what was heard has been shown: the transcript is sent to the window (`VoiceTranscript`), and the task's first class 1 action waits until it has been on screen for `show_seconds` (2 s), or until the person sends it, as the settings say (`transcript_check`: `show`, `wait`). Class 0 runs at once |
| Class 3 never by voice | `voice::approval` returns `Never` for class 3 and 4, whatever the settings |
| Class 2 on screen | `voice::approval` returns `OnScreen` for class 2 unless spoken approval is allowed by the policy (it cannot be in this build: the setting belongs to the policy store, which does not exist) and the request came by push-to-talk in the same exchange with no other sound playing. Today nothing above class 1 runs at all (pax-service.md section 4) |
| Source in the audit | `task_started` carries `TUR_AI_SOURCE` = `typed`, `voice`, `voice_edited` (the person changed the transcript before sending) or `live`, and `TUR_AI_VOICE_TRIGGER`. The trail keeps no words, as before |
| Nothing kept | The capture is held in memory; the file given to the recogniser is written mode 0600 under `$XDG_RUNTIME_DIR/tur-pax/voice/` and deleted when it returns, whatever happened |

### 4.1 The voice turn

```text
ready ──StartListening──▶ listening ──StopListening(send)──▶ recognising ──▶ (transcript)
  ▲                          │  └─ 30 s, or 1.2 s of silence after speech (button only)
  │                          └─StopListening(discard), Stop, Esc, lock ──▶ ready
  │
  └── shown and sent ◀── waiting (transcript_check "wait", other sound, or a task running)
      or sent at once: a task with origin voice; its class 1 actions wait until the
      transcript has been on screen for show_seconds
```

A spoken request while a task runs: the task is paused while the person
speaks; "stop", "ferma", "basta", "annulla", "cancel" end it; "continue",
"vai avanti", "riprendi" resume it; anything else is shown and kept in the
bar, and the task resumes (one task at a time, as for typed words).

### 4.2 Barge-in and echo

Without echo cancellation a microphone hears P@x's own voice. TUR does not
pretend otherwise: in a live session with the local engine, speech detected
while P@x speaks interrupts P@x only when TUR's echo canceller is there.
The image loads PipeWire's own (`libpipewire-module-echo-cancel` with the
WebRTC canceller; configuration only, the owner's decision 9:
`image/rootfs/usr/share/pipewire/pipewire.conf.d/60-tur-echo-cancel.conf`).
It adds two devices and changes no default; when `pw-dump` shows both,
P@x captures from `tur-echo-cancel-source` and plays into
`tur-echo-cancel-sink`, so the canceller removes P@x's own voice. Whether
Fedora 45's PipeWire has the WebRTC canceller is not verified. Otherwise P@x's
speech is interrupted by the key, the microphone button or Stop, and what
the microphone heard during P@x's speech is dropped. A provider's live
engine does its own turn detection on the audio it is sent; the same
rule applies to what TUR sends it: while P@x's audio plays without echo
cancellation, the microphone's audio is not sent.

## 5. Live conversation

One session interface, whatever the engine (`tur-pax-providers`,
`live.rs`): audio in, audio out, transcripts both ways, the person
interrupting, and tool calls. **Every tool call a live model makes goes
through the same `handle` of the task loop as a text task's**: decided by
the policy engine, recorded, performed only if allowed, its result framed as
data. A live model never acts directly.

| Engine | What it is | Built |
|---|---|---|
| `local` | The cascade: local recognition of each utterance (energy segmentation, then whisper.cpp), the person's local language model through the normal task loop, P@x's words spoken by Piper sentence by sentence. Mid-task utterances are added to the conversation between steps | Yes (this slice) |
| `openai-realtime` | OpenAI's Realtime API over WebSocket; the same family serves Azure OpenAI and any server that speaks it, by configuration | Adapter written, against the documentation; never run |
| `gemini-live` | Google's Gemini Live API over WebSocket | Adapter written, against the documentation; never run |
| `scripted` | A fake live provider: audio frames in, events and tool calls out, for tests | Yes |

The providers' details, prices and what was verified are in section 8 and
in the report.

**Live safety.** The microphone mark is on for the whole session; the
session has a visible end (the bar's "End conversation", Meta+Esc, Esc in
the bar) and ends by itself after `silence_timeout_seconds` (60 by default)
with no speech and no task running. Self-sound as in section 4.2. While a
live session runs P@x does not read aloud the content of windows marked
sensitive or excluded (agent-policy.md's lists): text a tool returns from
such a window is not spoken, whatever the model says. Transcripts of what
the person said go into the task as their request, like typed words; the
audit keeps the source (`live`) and no words; audio is not stored.

**Spending.** A provider's live audio is billed per audio token. Its usage
events are added to the day's record like a text task's, and the day's
limit stops the session (`TUR-AI-SPEND-001`). The status shows the
estimate per minute from the model list's audio prices.

**Consent.** A provider's live engine streams the person's voice to that
company. It is offered only after a consent text that says so, names the
company and links its terms, recorded per provider in the voice settings
(`live.consent`); the text's version is `VOICE_CONSENT_VERSION`.

## 6. org.tur.Agent1: what voice adds

Interface version stays 1: these are additions. Method names avoid the
words the interface test forbids (no method approves or confirms an action).

### 6.1 Methods

| Method | In | Out | What it does | Refused with |
|---|---|---|---|---|
| `GetVoiceStatus` | | `s status` | Section 6.4 | never |
| `SetVoiceSettings` | `s settings` | | A JSON object with any member of the settings (6.4); `null` restores a default. A provider live engine needs `live_consent_version` with the current version | `SERVICE-006`, `-009`, `VOICE-005` (wake word), `VOICE-007` (consent) |
| `PrepareVoice` | `s options` | `s job` | Downloads the speech models in use or named (`models`, a list of catalogue ids) through `tur-ai-local speech ensure`; `download_bytes` must be the total the person was shown when anything is to be downloaded. Progress as `VoiceProgress` | `-006`, `-004`, `SERVICE-010` |
| `CancelVoicePreparation` | | | Asks it to stop; what was downloaded is kept | `-007` |
| `StartListening` | `s options` | `s turn` | Opens the microphone. `options`: `trigger` (`push-to-talk`, `button`), `shown_by_caller` (b) | `SERVICE-001`/`-002`/`-003`/`-008`, `VOICE-001` (voice off), `-002` (no speech models), `-003` (no microphone program or device), `-004` (not visible), `SERVICE-004` (already listening) |
| `StopListening` | `s turn`, `b send` | | Closes the microphone; `send` recognises what was heard, otherwise it is dropped | `-007` |
| `SendTranscript` | `s turn`, `s text` | `s task` | The person sends what was heard (or their correction of it) as a request. A changed text is `voice_edited` | `-006`, `-007`, the refusals of `StartTask` |
| `Speak` | `s text` | | Says a text in the local voice (the window's "Read aloud") | `VOICE-001`, `-002`, `-006` |
| `StopSpeaking` | | | Stops P@x's speech | never |
| `StartLiveSession` | `s options` | `s session` | Starts a live conversation. `options`: `engine` (`local` or a live provider's identifier; the settings' choice otherwise), `shown_by_caller` | as `StartListening`, and `VOICE-007`, `KEY-*`, `TUR-AI-PROVIDER-*` |
| `EndLiveSession` | | | Ends it: the microphone closes, speech stops, a running task is cancelled | `-007` |

`CancelTask("")` (the stop key) now also closes the microphone, stops
P@x's speech and ends a live session, and succeeds when any of them was
under way.

### 6.2 Signals

| Signal | Arguments | When |
|---|---|---|
| `VoiceStateChanged` | `s state`, `s details` | The voice state changed. `details`: `state`, `microphone_open` (b), `turn`, `trigger`, `session`, `indicator` (`overlay` or `caller`), `reason` (why it ended), `error_code`, `message` |
| `VoiceLevel` | `s turn`, `d level` | While the microphone is open, about ten times a second: the loudness, 0 to 1. For the window's meter; it carries no sound |
| `VoiceTranscript` | `s turn`, `s details` | What was heard: `text`, `final` (b), `language`, `model`, `seconds` (recognition time), `audio_seconds`, `check` (`show`: a task follows; `wait`: it waits for `SendTranscript`), `reason` for waiting (`setting`, `other_sound`, `task_running`), `other_sound` (b), `task` (when one was started), `empty` (b: nothing was heard) |
| `VoiceProgress` | `s model`, `s event` | A step of `PrepareVoice`: the `ensure-event`s of `tur-ai-local speech ensure`, then `ready` or `failed` |
| `LiveEvent` | `s session`, `s event` | A live session: `kind` = `started`, `heard` (the person's words, `text`, `final`), `said` (P@x's words, `text`, `final`), `interrupted`, `usage` (`audio_seconds_in`, `audio_seconds_out`, `cost`, `currency`), `ended` (`reason`: `person`, `silence`, `stop`, `limit`, `error`; `error_code`, `message`) |

Tool calls of a live session are reported as for any task: `TaskStarted`,
`ActionProposed`, `ActionCompleted`, `TaskFinished`.

### 6.3 Properties

| Property | Type | Value |
|---|---|---|
| `State` | `s` | Now also `listening` while the microphone is open and no task runs |
| `Voice` | `s` | `off`, `unavailable`, `ready`, `listening`, `recognising`, `waiting`, `speaking`, `live` |
| `MicrophoneOpen` | `b` | Whether P@x's microphone is open. What the companion's *listening* state and any indicator follow |

### 6.4 The JSON documents

**Voice settings** (`$XDG_CONFIG_HOME/tur/pax/voice.json`, mode 0600;
`schemas/pax-voice-settings.schema.json`): `schema_version` 1, `enabled`
(false), `language` (`auto`, or `en`, `it`, `de`, `es`, `fr`), `recognition`
(`model`: a catalogue id or `null` for the recommendation), `synthesis`
(`voices`: an object language → voice id; missing → the default),
`spoken_replies` (false), `activation` (`push-to-talk`), `wake_word`
(false), `transcript_check` (`show`; `wait` is the setting "Always wait for
me to press Send"), `show_seconds` (2), `live` (`engine`
`local`, `model` `null`, `silence_timeout_seconds` 60, `narrate` true,
`consent`: `null` or `{provider, version, at}`).

**Voice status** (`GetVoiceStatus`): `enabled`, `state`, `microphone_open`,
`settings` (as above), `language` (`setting`; `system`, the session's
language when P@x speaks it; `effective`), `problem` (`error_code`,
`message` or `null`), `recognition` (local AI's recommendation: `id`,
`name`, `installed`, `download_bytes`, `tier`, `reasons`, `expected_ms`,
and for a model weak in the language asked `weak_in` and a plain
`warning`), `alternatives` (the other recognition models that fit, better
Italian first, Parakeet with its credit), `engines` (Piper, downloaded with
the voices), `voice_activity`, `synthesis` (`voices`: per language the voice,
`installed`, `download_bytes`, `licence`, `credit`), `download_bytes_needed`
(the total `PrepareVoice` would fetch), `free` (true: everything in use
runs on this computer at no cost), `live` (`engines`: each with `id`,
`name`, `kind` `local` or `provider`, `paid`, `transport`, `consent_current`,
`consent_text`, `key`, `price_per_minute`, `currency`, `built`;
`suggestion`: with the person's key for a provider that has a live engine,
and the local engine in use, that provider's cheapest live model and its
price a minute), `wake_word`
(`available` false, `on` false, `explanation`), `keys` (`talk`: "Hold
Meta+Space"), `capture` (`program_found`, `other_sound`), `turn`, `session`.

## 7. Desktop

- **Ask bar.** A microphone button at the right of the field (it replaces
  "Speaking to P@x is coming later"). Press: listening starts; press again
  or Enter: it is sent; Esc: dropped. Holding P@x's key does the same as
  holding the button. While listening: the lit microphone, a level meter and
  "Listening…"; then "Recognising…"; then the transcript in the field,
  editable, with "Sent in 2 s · Esc to cancel" or "Press Enter to send".
  The bar offers "Talk with P@x" (a live conversation) when it is set up.
- **The overlay** shows *listening* through P@x's lease while the
  microphone is open (it already draws the state).
- **The companion** (PR #139, not merged): the interface change it needs is
  given as text in the report, section "For the companion's engineer"; the
  branch was not edited.
- **Settings and setup**: "Speak to P@x" (switch, free, local), the language,
  the voice with its licence and credit, "P@x answers out loud", live
  conversation with its engines (local first; providers marked optional and
  paid). P@x's setup offers the free local voice after the model.

## 8. Live engines (checked 16 November 2026)

The provider layer's side is [pax-providers.md](pax-providers.md),
section 16. The list P@x offers is configuration:
`services/tur-pax/data/live.json` (`schemas/pax-live-providers.schema.json`),
installed as `/usr/share/tur/pax/live.json`, an administrator's
`/etc/tur/pax/live.json` first. No model name is compiled in.

| Backend | Status in this build |
|---|---|
| Local cascade (whisper.cpp → P@x's model through the task loop → Piper) | **Implemented** (`voice_service.rs`, `live_local`). Not run |
| OpenAI Realtime over WebSocket (`gpt-realtime-2.1`, `gpt-realtime-2.1-mini`) | **Implemented** (`openai-realtime` family), conformance-tested against a scripted server only. Not run against OpenAI |
| Gemini Live over WebSocket (`gemini-3.8-live`) | **Implemented** (`gemini-live` family), conformance-tested against recorded shapes only. Two wire details unverified (pax-providers.md 16.2). Not run against Google |
| xAI Grok Voice Agent (`grok-voice-3.0`) | **Configured only**: an entry of the `openai-realtime` family at `wss://api.x.ai/v1/realtime`; its cumulative transcript events are not read |
| Azure OpenAI realtime | **Configured only**: the `openai-realtime` family with the resource's address and the `api-key` header; no entry ships, the person's administrator adds one |
| llama.cpp's `llama-server` `/v1/realtime` (b12431) with LiquidAI LFM3-Audio-2B or LFM3-Omni-2B | **Configured only**: `ws://` to this computer is accepted for this; no entry ships. Turn-based, English voice chat (LFM3-Audio), and the LFM Open License is free only under 10 M USD of annual revenue |
| OpenAI Realtime over WebRTC or SIP; OpenAI GPT-Live (`/v1/live/sessions`) | **Not implemented**: WebRTC needs a DTLS/SRTP stack; GPT-Live is another protocol |
| Amazon Nova 2 Sonic | **Absent**: a SigV4-signed HTTP/2 event stream, not a WebSocket |
| Anthropic | **Absent**: no live audio API exists; the Messages API takes no audio |
| Kyutai Moshi, MiniCPM-o 4.5, Qwen3-Omni and other open speech-to-speech models | **Not offered**: every full-duplex open model found needs a graphics card with 12 to 24 GB, speaks no Italian, and calls no tools; none runs in real time on an ordinary processor |

**Measured: nothing.** No live engine has been run with TUR.

**Estimated cost of a minute** (both sides speaking the whole minute, from
the listed prices; a floor, because providers count the conversation again
with every answer): `gpt-realtime-2.1` 0.096 USD, `gpt-realtime-2.1-mini`
0.030 USD, `gemini-3.8-live` 0.029 USD. The day's limit applies.

**Expected latency of the local cascade** (estimates from the published
figures above and local-ai.md's measurements; not measured):

| Computer | Recognition of a 5 s utterance | P@x's model, first words | First sentence spoken | Honest verdict |
|---|---|---|---|---|
| 2 processors, 4 GB (the test VM) | tiny: ~2.5 s | Qwen3 0.6B: several seconds of prompt reading (P@x's instructions and tools are about 2 000 tokens) | ~0.5 s | Too slow for conversation; push-to-talk requests work, slowly |
| 4 to 8 processors with AVX2, 8 to 16 GB | base or small: 1.5 to 5 s | Qwen3 4B: 3 to 8 s the first turn, less once the prompt is cached | ~0.4 s | Usable as turn-taking, not as a natural conversation |
| 16 threads and a Vulkan card, 32 GB | turbo: ~1.7 s | a 26B mixture of experts on the card: 1.5 to 3 s | ~0.3 s | Close to conversational |

Small local models also call tools less reliably than large ones: with
Qwen3 0.6B a live conversation will often answer instead of acting. The
provider engines answer in well under a second, at the price above.

## 9. The image

The engine stage gains whisper.cpp v1.9.5 with `whisper-cli` and
`parakeet-cli` (as for llama.cpp). The image keeps Fedora's
`python3-onnxruntime` and `python3-pathvalidate` (MIT) for Piper, installs
the speech catalogue at `/usr/share/tur/ai-local/speech/`, the list of live
engines at `/usr/share/tur/pax/live.json`, and PipeWire's echo canceller
configuration. **No model, no voice and no Piper is in the image**: Piper
and its embedded espeak-ng (GPL) come on first use with the voices.

## 10. Not built in this slice

- The wake word: the setting exists and is refused when turned on.
- A provider's recognition service for single turns (only live engines).
- Spoken approval of class 2: the setting belongs to the policy store, which
  does not exist; `voice::approval` is written and tested for when it does.
- Reading aloud from windows marked sensitive: the session service reads no
  window in this build (it passes no accessibility bridge), so nothing from
  such a window can reach P@x's speech; when it does read windows, the rule
  must be enforced on what is said, and a test written.
- A male Italian voice: none clean through its training chain was found
  (owner's decision 3: a follow-up).
- WebRTC transports (OpenAI's Realtime over WebRTC needs a DTLS/SRTP
  stack; the WebSocket transport is used).
- A resident process holding the GlobalShortcuts portal's press and release
  of the talk key (section 7 of the report).
- Streaming recognition while the person speaks: the transcript appears
  when they stop.

## 11. The owner's decisions (16 November 2026)

The owner answered the open points with "I trust your choices"; they are
recorded here and in the report, section 9, and applied in the code.

| # | Question | Decision | Where |
|---|---|---|---|
| 1 | Default language | Recognition follows the system's language when P@x speaks it (an Italian system defaults to Italian); otherwise the model detects it. A language the person chooses comes first | `voice::system_language`, `effective_language`; `Paths::locale` |
| 2 | The wake word on by default | Never: a standing rule. It stays off and refused until a detector exists, and then is only ever turned on by the person | `voice.rs` |
| 3 | Default voices | English `en_US-ljspeech-medium`, Italian `it_IT-paola-medium`. A male Italian voice is a follow-up | speech catalogue |
| 4 | The smallest machines | The tiers stay: tiny on 2 processors, base on 4 to 8. Where tiny is chosen, setup says plainly that its Italian is weak and offers base | `speech recommend` (`warning`, `alternatives`); the voice setup |
| 5 | Transcript before the first change | 2 s on screen, plus the setting "Always wait for me to press Send" (`transcript_check` `wait`) | `voice.rs`; setup and settings |
| 6 | Spoken replies | Local voice by default; still off until the person turns them on | `voice.rs` |
| 7 | Live engine | Local by default. With the person's key for a provider, that provider's cheapest live model is suggested with its price a minute (OpenAI `gpt-realtime-2.1-mini`, Google `gemini-3.8-live`) | `GetVoiceStatus` `live.suggestion`; setup |
| 8 | Silence timeout | 60 s | `voice.rs` |
| 9 | Echo cancellation | Yes: PipeWire's echo canceller, configuration only | section 4.2 |
| 10 | Parakeet TDT 0.6B v3 | Yes, as an optional recognition model for Italian and other European languages, never the default, with its CC BY 4.0 credit shown | speech catalogue `parakeet-tdt-0-6b-v3-q4-k`; `parakeet-cli` |
| 11 | Piper in the image | No: Piper (with espeak-ng) is downloaded on first use with the voices, checked by its digest, with its source pinned for the GPL's offer | speech catalogue `piper-tts-1-8-0`; section 9 |

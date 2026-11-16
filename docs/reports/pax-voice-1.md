<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# pax-voice-1 — P@x's voice: speaking to P@x, spoken answers, live conversation, free and local first

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-voice-1
Subsystem:        AI control layer: voice (ai-control-layer.md §9);
                  services/tur-pax (session service, task loop),
                  services/tur-pax-providers (live sessions),
                  services/tur-ai-local (speech), desktop/apps/tur-pax-ui,
                  image (engine stage), CI (pax-voice workflow)
Build ID:         none. NOTHING WAS BUILT: GitHub Actions is disabled for
                  the owner's account (support ticket open) and nothing is
                  compiled on development machines (AGENTS.md)
Source revision:  branch feat/pax-voice from origin/main d686e7e; the pull
                  request's head commit is given in the hand-back

SUMMARY
- What changed: P@x can be spoken to (hold Meta+Space or press the
  microphone in the ask bar), recognises speech on this computer with
  whisper.cpp, shows what it heard, and runs it as an ordinary task under
  the same policy and audit (marked "voice" in the trail). P@x can answer
  out loud in a local Piper voice (off by default). A live conversation
  lets the person talk with P@x while it works, through the local cascade
  (free, private) or a provider's speech-to-speech engine (OpenAI Realtime,
  Gemini Live; the person's own key, consent naming the company); every
  tool call of a live model goes through the same policy engine as a text
  model's. The free local voice is first everywhere and needs no account.
- Why it changed: the owner's four requests of 16 November 2026 (pax-voice.md,
  top); the accepted design of ai-control-layer.md §9 and local-ai.md item 5.
- NOTHING HAS RUN. No unit test, no integration test, no build, no
  measurement. Every figure below is from the projects' own documentation
  or an estimate, and says so.
```

## 1. What was built

| Part | Where |
|---|---|
| The design and the frozen interface additions | `docs/architecture/pax-voice.md` (new); `services/tur-pax/interfaces/org.tur.Agent1.xml` (11 methods, 5 signals, 2 properties; version stays 1) |
| The rules and the voice turn's state machine: push-to-talk only, wake word off and refused, the microphone only while shown, barge-in, self-sound, other programs' sound, the transcript first, class 3 never by voice, class 2 on screen, settings and consent | `services/tur-pax/src/voice.rs` (new) |
| The microphone (`pw-record --raw`), the speaker (`pw-play --raw`), `pw-dump`, local recognition and synthesis through `tur-ai-local`; audio in memory, the one file deleted at once | `services/tur-pax/src/audio.rs` (new) |
| The session service's voice: turns, transcripts, spoken answers and narration, the local live engine, a provider live engine's microphone and speaker, the indicator lease | `services/tur-pax/src/voice_service.rs` (new, a child of `service.rs`), `service.rs`, `dbus.rs`, `daemon.rs`, `settings.rs` |
| The task loop: a request's origin (`TUR_AI_SOURCE`, `TUR_AI_VOICE_TRIGGER`), the transcript gate before class 1, the person's words added mid-task, `run_live` (a live engine's tool calls through `handle`, its usage against the day's limit) | `services/tur-pax/src/orchestrator.rs` |
| The live engines as configuration | `services/tur-pax/src/livelist.rs`, `services/tur-pax/data/live.json`, `schemas/pax-live-providers.schema.json` (new) |
| Nine codes `TUR-AI-VOICE-001` to `-009` | `services/tur-pax/src/faults.rs` |
| The provider-neutral live session (`LiveSession`, `LiveEvent`, `LiveConfig`, `LivePrices`), a blocking WebSocket client on rustls (no new crate), the OpenAI Realtime and Gemini Live families, a scripted fake | `services/tur-pax-providers/src/{live,live_openai,live_gemini,ws}.rs` (new); `docs/architecture/pax-providers.md` §16 |
| Speech in local AI: catalogue (4 Whisper models, Silero VAD, 6 voices), tiers per computer, `speech models|recommend|ensure|cancel|remove|stt|tts`, codes `TUR-AI-LOCAL-030` to `-035` | `services/tur-ai-local/{speech/,src/speech.rs,src/cli.rs,src/codes.rs}`; `schemas/{speech-model,ai-local-speech}.schema.json`; `docs/architecture/local-ai.md` §12 |
| The image: whisper.cpp v1.9.5 (`whisper-cli`, `parakeet-cli`) built from its pinned source like llama.cpp; Fedora's onnxruntime and pathvalidate for Piper; the speech catalogue; the list of live engines; PipeWire's echo canceller configuration. No model, no voice, no Piper (first use) | `image/Containerfile`, `image/engine/{whisper-cpp,piper}.{sh,lock}`, `image/files.tsv`, `image/README.md` |
| The window: microphone button, level meter, transcript in the field, "Talk with P@x", the live foot with End, spoken and live rows, the setup's voice step (free local voice first; providers "optional · paid with your own key") | `desktop/apps/tur-pax-ui/**`; `docs/architecture/tur-pax-ui.md` §13 |
| Tests (none run) | `services/tur-pax/src/{voice,audio,livelist}.rs` unit tests; `services/tur-pax/tests/voice.rs` (new), `tests/service.rs` (six voice tests), `tests/agent1_interface.rs`; `services/tur-pax-providers/tests/live.rs` and `ws.rs` units; `services/tur-ai-local/tests/speech.rs`; `desktop/apps/tur-pax-ui/tests/tst_voice.qml` and unit tests |
| Integration, by hand only | `.github/workflows/pax-voice.yml` (`workflow_dispatch` only), `services/tur-ai-local/ci/speech_ci.py`, `tests/pax/voice_e2e.py` |

## 2. Engines and licences (checked 16 November 2026)

| | Version | Licence | Health | Fedora 45 | How TUR gets it |
|---|---|---|---|---|---|
| whisper.cpp | v1.9.5 (2026-10-06), tag commit `d1be6fde…` | MIT | active (push 2026-11-16) | `whisper-cpp-1.8.3-4.fc45`: libraries only, no `whisper-cli`, AVX/AVX2/FMA off, no Vulkan | built from source in the image, CPU variants + Vulkan |
| Whisper weights (ggml, `ggerganov/whisper.cpp` @ `5359861c…`) | tiny/base/small q5_1, large-v3-turbo q5_0 | MIT | | | downloaded on first use, SHA-256 pinned |
| Silero VAD v6.2.0 (ggml) | | MIT | | | downloaded on first use |
| Piper (piper1-gpl) | v1.8.0 (2026-09-04); rhasspy/piper archived 2025-10-06 | GPL-3.0-or-later, run as a separate program, never linked | active, "looking for maintainers" | `python3-piper-tts-1.4.2`: too old for the retrained voices | on first use with the voices: the PyPI wheel (34 131 442 B), SHA-256 pinned, with its source archive for the GPL's offer, in a private venv on Fedora's onnxruntime 1.26 |
| Parakeet TDT 0.6B v3 (ggml, `ggml-org/parakeet-GGUF` @ `35156454…`, q4_k, 415 611 879 B) | | CC-BY-4.0 (credit shown) | | | optional, downloaded on first use; run by whisper.cpp 1.9's `parakeet-cli` |
| Voices (`rhasspy/piper-voices` @ `e9ab671f…`) | en_US ljspeech (public domain), en_GB cori (CC BY 4.0), it_IT paola (CC0 on a CC BY 4.0 base), de thorsten (CC0), es davefx (CC0), fr siwis (CC BY 4.0) | per voice, the whole training chain | | | downloaded on first use; CC BY credits shown |
| Not offered | lessac (research only), libritts_r (from lessac), amy (no licence), riccardo x_low (unverified), Coqui XTTS (non-commercial), Moshi and other open speech-to-speech models (GPU-only, no Italian, no tools) | | | | |

Compared for recognition and not chosen: Vosk (worse Italian), Moonshine
(no Italian), Distil-Whisper (English only), sherpa-onnx.

## 3. Sizes and expected speed (estimates; nothing measured)

| Tier (the rule of pax-voice.md §3.3) | Recognition | Its download | Expected wait for a 5 s utterance | Voices |
|---|---|---|---|---|
| 2 processors, 4 GB | `whisper-tiny-q5-1` | 32 MB | ~2.5 s | ~63 MB each |
| 4 to 8 processors with AVX2 (the free tier's default) | `whisper-base-q5-1` | 60 MB | 1 to 6 s | |
| 8+ processors | `whisper-small-q5-1` | 190 MB | 2 to 5 s | |
| 16 threads or a Vulkan card | `whisper-large-v3-turbo-q5-0` | 574 MB | ~1.7 s | |

The free tier's whole first download (base, the detector, the English and
Italian voices): about 188 MB. Live conversation latency by tier, and the
providers' price per minute (0.096 / 0.030 / 0.029 USD for gpt-realtime-2.1,
-mini, gemini-3.8-live): pax-voice.md §8.

## 4. Tests written, and what they are meant to prove (NONE HAS RUN)

| Test | What |
|---|---|
| `voice.rs` unit tests | push-to-talk default and everything off; wake word refused and unloadable; settings change member by member; consent per provider; the microphone opens only when shown (a live session only with the indicator); press/release; barge-in by the key and by Stop; self-sound with and without echo cancellation; a transcript heard over other sound waits; `pw-dump` parsing; class 3 and 4 never by voice, class 2 on screen unless every condition holds; the gate; stop/resume words in English and Italian; WAV; the segmenter; sentences |
| `tests/voice.rs` (task loop, scripted provider) | source in the audit, no words; a spoken task observes at once and changes nothing until the transcript is sent; a withdrawn request changes nothing and does not wait; voice adds no authority (class 2 and 3 not performed); mid-task words reach the model after the results; a live engine's tool calls decided like a text model's (class 0 and 1 performed, class 3 refused and answered), its audio billed; barge-in stops P@x's voice and cancels the engine's answer; a session ends by the person and by Stop |
| `tests/service.rs` voice (stand-in programs) | voice off until switched on; a spoken turn heard, shown, a task marked voice, the audio file gone; other sound makes it wait and the corrected text is `voice_edited`; Stop closes the microphone and nothing is recognised; the microphone never opens unseen; a provider's live engine needs consent naming the company |
| `tur-pax-providers/tests/live.rs` | conformance per family, a scripted session, a loopback WebSocket session, repeated keys refused, no key in errors |
| `tur-ai-local/tests/speech.rs` | catalogue rules (pinned URLs, digests, licences "none" for every offered voice, EN and IT defaults); tiers on the recorded machines; WAV checks; `stt`/`tts` with stand-in engines; JSON against the schema |
| Desktop | `tst_voice.qml`, unit tests, walks of six new states |
| `pax-voice` workflow, by hand | TUR's own synthetic speech (Piper, English and Italian) through every Whisper model: transcript, word error rate, wait, peak memory; then through `tur-pax serve` with the scripted provider: transcript → task → what the model was sent → the audit |

## 5. NOT RUN

- **Everything**, until GitHub Actions returns: every unit and integration
  test above, `cargo build`, clippy, rustfmt, the desktop build and its
  screenshot comparison, the image build with the new speech stage, the
  `pax-voice` workflow.
- **A real microphone and real speakers**; PipeWire on the image;
  `pw-record --raw` / `pw-play --raw` as the service runs them.
- **Real hardware**: no speed or memory figure is TUR's; the tiers are the
  rule of §3.3 applied to the recorded test machines.
- **Any provider's live engine**: no OpenAI, Google or xAI session was opened.
  The one CI key (Anthropic) cannot be used: Anthropic has no live audio API.
- **KWin**: whether the Space key's release reaches the ask bar that took
  focus while it was held; the overlay lease under KWin.

## 6. Known limitations

- Rust code was written without a compiler. Expect compile errors, clippy
  findings and a rustfmt patch on the first run; `scripts/dev/ci.py fixes`
  applies CI's lockfile and format patches. The lockfiles were edited by hand
  (two dependency names added to `tur-pax-providers` in eleven lockfiles; no
  new crate).
- The desktop's picture references must be regenerated
  (`desktop/ci/update-references.py`) after looking at what CI renders.
- No streaming recognition: the transcript appears when the person stops.
- The wake word, a provider's single-turn recognition, WebRTC, GPT-Live, spoken
  approval, the GlobalShortcuts portal's press and release: not built.
- The local live engine's barge-in by voice relies on PipeWire's echo
  canceller, which the image now configures; it is not verified that
  Fedora 45's PipeWire ships the WebRTC canceller it names.
- Gemini Live: two wire details unverified; its sessions end at about 10 to
  15 minutes (session resumption not built).
- Piper with espeak-ng (GPL) is not in the image; it is downloaded on first
  use from PyPI, and its source archive is pinned for the offer the GPL
  requires. The image build gets longer (whisper.cpp compiled from source).
- `local-ai.yml` (existing, heavy) runs on this pull request because
  `services/tur-ai-local/**` changed.

## 7. For the companion's engineer (PR #139, branch `pax-companion`, not edited)

The character's *listening* state today means "the ask bar is open". With
voice it should mean what it says. The interface it needs exists on
`org.tur.Agent1` now:

1. Read the property **`MicrophoneOpen`** (b) at start, and follow it in
   `org.freedesktop.DBus.Properties.PropertiesChanged` on `/org/tur/Agent1`
   (sent with every `StateChanged`). While it is true the character shows
   *listening* (two arcs, core larger), whatever the ask bar does.
2. Listen to **`VoiceStateChanged(s state, s details)`**: `state` is `off`,
   `unavailable`, `ready`, `listening`, `recognising`, `waiting`,
   `speaking`, `live`; `details.microphone_open` is authoritative.
   Suggested mapping: `listening` and `live` → *listening*; `recognising` →
   *thinking*; `waiting` → *waiting* ("P@x is waiting for you": a transcript
   waits to be sent); `speaking` → *telling*.
3. Order of states: *listening* while the microphone is open must outrank
   *idle*, *noticing* and *telling*; *acting*, *observing* and *waiting*
   still come first.
4. Add `org.tur.Agent1.MicrophoneOpen` and `VoiceStateChanged` to "What it
   reads" (§6) and to the card "What is P@x watching?": the character reads
   whether the microphone is open, never the sound.
5. The character never calls `StartListening`; pressing it opens the ask
   bar as today, whose microphone does.

## 8. Decision-record text (for an ADR; docs/adr/README.md not edited)

> **ADR-00xx: P@x's voice: local first, push-to-talk, live engines behind the provider interface**
>
> *Status:* proposed, 16 November 2026. *Context:* the owner asked for
> speech recognition, spoken answers, live conversation with providers'
> speech-to-speech models, and a free local path (pax-voice.md).
> ai-control-layer.md §9 requires local by default, push-to-talk, barge-in,
> self-sound suppression, class 3 never by voice, class 2 on screen.
>
> *Decision.* (1) Recognition with whisper.cpp and synthesis with Piper run on
> the computer as separate programs driven by `tur-ai-local`; models and
> voices are downloaded on first use from their publishers, pinned by
> SHA-256, chosen per hardware tier; none is in the image. whisper.cpp is
> built from source in the image like llama.cpp; Piper, whose Fedora package
> is too old, is downloaded on first use with the voices, from its pinned
> wheel, and never put in the image. (2) The session service opens the
> microphone with PipeWire's `pw-record` only while P@x's indicator or the
> asking window shows it; push-to-talk is the only activation; the wake word
> is not built. (3) A spoken request is an ordinary task with its origin in
> the audit; its first action above class 0 waits until the transcript has
> been shown or sent; sound from another program makes the transcript wait
> for the person. (4) Live speech-to-speech engines are a second provider
> interface (`LiveSession`) in `tur-pax-providers`, with families as data
> (OpenAI Realtime, Gemini Live) over a WebSocket client written on the
> rustls already in use; every tool call of a live model is handled by the
> same task loop and policy engine; consent per provider names the company
> the voice is streamed to; usage counts against the day's limit. (5) The
> free local live engine is a cascade through the normal task loop.
>
> *Consequences.* No audio leaves the computer unless the person chose a
> provider's live engine. The image grows by whisper.cpp and Fedora's
> onnxruntime; no GPL speech program is in it. PipeWire's echo canceller is
> configured so that the person can interrupt P@x by voice. Small computers
> get a weak model (tiny) and slow live turns. The interface version of
> `org.tur.Agent1` stays 1 (additions only). *Rollback:* voice is off by
> default; removing it is reverting the pull request; nothing else depends
> on it.

## 9. Owner decisions (16 November 2026)

The owner answered with "I trust your choices". These are the owner's
decisions, recorded in [pax-voice.md](../architecture/pax-voice.md) section
11, and applied in the code where they change a default (second commit of
this pull request).

| # | Decision | Applied in |
|---|---|---|
| 1 | Recognition follows the system's language when P@x speaks it (an Italian system defaults to Italian); automatic detection otherwise | `voice::system_language`, `effective_language`; the session's locale in `Paths::locale`; `GetVoiceStatus` `language`; the voice setup says "follows your system" |
| 2 | The wake word is never on by default (a standing rule); it stays built off and refused | unchanged |
| 3 | Voices: English `en_US-ljspeech-medium`, Italian `it_IT-paola-medium`. **Follow-up:** a male Italian voice clean through its training chain | unchanged; section 6 |
| 4 | Tiers kept (tiny on 2 processors, base on 4 to 8); where tiny is chosen the setup says plainly that its Italian is weak and offers base | `speech recommend`: `recognition.weak_in`, `recognition.warning`, `alternatives`; the voice setup |
| 5 | The 2 s on screen before the first change stays, plus a setting "Always wait for me to press Send" | `transcript_check` `wait`; setup and settings |
| 6 | Spoken replies in a local voice by default (still off until turned on) | unchanged |
| 7 | Live engine local by default; with the person's key for a provider, that provider's cheapest live model is suggested with its price a minute (OpenAI `gpt-realtime-2.1-mini`, about 0.030 USD; Google `gemini-3.8-live`, about 0.029 USD) | `GetVoiceStatus` `live.suggestion`; the voice setup; a service test |
| 8 | Silence timeout 60 s | unchanged |
| 9 | PipeWire's echo canceller loaded, configuration only | `image/rootfs/usr/share/pipewire/pipewire.conf.d/60-tur-echo-cancel.conf`; P@x captures from and plays into its devices when `pw-dump` shows them (`voice::echo_cancelled_source`, `ECHO_SOURCE`, `ECHO_SINK`) |
| 10 | Parakeet TDT 0.6B v3 offered as an optional, better recognition model for Italian and other European languages, with its CC BY 4.0 credit | speech catalogue `parakeet-tdt-0-6b-v3-q4-k`; `parakeet-cli` built with whisper.cpp; `speech stt` runs it; listed in `alternatives` |
| 11 | Piper (and its espeak-ng) downloaded on first use with the voices, checked by digest, not in the image; the source stays pinned for the GPL's offer | speech catalogue `piper-tts-1-8-0` (kind `engine`); `speech ensure` installs it in a private virtual environment; the image keeps only Fedora's onnxruntime and pathvalidate |

Not verified, from these decisions: that Fedora 45's PipeWire ships the
WebRTC canceller the configuration names; what `parakeet-cli` prints (its
output is cleaned by rule and checked by the `pax-voice` workflow); that the
Piper wheel runs on Fedora 45's Python with its onnxruntime.

## 10. Next safe action

When Actions returns: let the pull request's workflows run, apply CI's
lockfile and rustfmt patches, fix what the compiler and clippy find, run
`pax-voice` by hand, then regenerate the desktop pictures. Then measure on
the physical test PC with a real microphone (milestone A8's exit criterion:
a hands-free task on the physical test PC).

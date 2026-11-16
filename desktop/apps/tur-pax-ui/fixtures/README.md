<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# tur-pax-ui's fixture of org.tur.Agent1

What the real P@x session service (`tur-pax serve`, with the scripted
provider and the stand-in for local AI) answered and signalled in CI. The
stand-in service of the tests (`tests/fixture/agent.cpp`) answers from these
files, so every picture and every test of tur-pax-ui shows what the service
really says.

**Nobody writes these files by hand.** The job `pax-ui-end-to-end` of the
desktop workflow records them on every run (`desktop/tests/pax-ui/e2e.py
record`) and fails when they differ from the files here, apart from times and
identifiers. When it fails:

```sh
desktop/apps/tur-pax-ui/fixtures/update.py RUN_ID   # a run of the desktop workflow
git diff desktop/apps/tur-pax-ui/fixtures            # what the service now says differently
```

| File | What it is | Recorded from |
|---|---|---|
| `agent1/replies.json` | Replies and refusals, keyed by method and arguments (JSON arguments in a canonical form) | `ListModels`, `ListLocalModels`, `GetConsentText` (Anthropic, DeepSeek, OpenAI, local AI), `ListProviderModels`, `GetGenerationSettings`, `SetGenerationSettings` refused, `HasKey`, `Enable` with another version, `SetKey` with what cannot be a key, `ListActivity`, `UseLocalModel` |
| `agent1/status/*.json` | `GetStatus` in each state: `off` (never switched on), `no-key`, `ready`, `limit-reached`, `priced` (a model whose price TUR knows), `local-consent` (local AI chosen, waiting for consent) | |
| `agent1/tasks/*.json` | Every signal of a task, in order: `two-tools`, `refused` (a class 2 step), `stop` (cancelled while its sandboxed command ran), `spend-limit`, `priced` | `StartTask`, `CancelTask` |
| `agent1/local/*.json` | Every `LocalModelProgress` of making a model ready: `qwen3-0.6b` (downloaded, ready), `ministral-3-3b` (a licence that restricts use), `gemma-4-e2b` (cancelled) | `UseLocalModel`, `CancelLocalModel` |

## voice/: written by hand

**One exception to the rule above.** P@x's voice (docs/architecture/
pax-voice.md) needs a microphone, so the job cannot record what the service
says while it listens. `voice/` holds what the design (section 6) says the
service sends, written by hand; every file says so in `hand_written`. It is
outside `agent1/`, so `e2e.py record` neither writes it nor compares it, and
`update.py` leaves it alone. When the service's voice can be recorded (a
stand-in microphone in the `pax` workflow), these files become recordings
like the others.

| File | What it is |
|---|---|
| `voice/status/*.json` | `GetVoiceStatus`: `off` (voice not switched on; the free local models and their sizes, Piper as an engine downloaded with the voices, the system's language Italian, a provider live engine suggested because its key is set), `small` (the same on a 2-processor computer: Whisper tiny with the warning that its Italian is weak, Whisper base and Parakeet as alternatives; no system language P@x speaks; no suggestion), `unavailable` (on, models not downloaded), `ready` |
| `voice/turns/*.json` | One spoken turn, signal by signal: `show` (heard, shown, a task follows: the recorded `two-tools`), `wait` (heard while another program played sound: it waits to be sent), `empty` (nothing was heard). Signals marked `after_stop` come after the person stops speaking (`StopListening`) |
| `voice/live.json` | A live conversation: heard and said, an interruption, its end (held until `EndLiveSession`, marked `after_end`) |
| `voice/prepare.json` | `VoiceProgress` of `PrepareVoice`: the free local models downloaded and checked |

How the stand-in plays them, and what it decides itself (nothing but moving
between recorded statuses and holding a task until it is stopped), is in
[tur-pax-ui.md](../../../../docs/architecture/tur-pax-ui.md), section 7.

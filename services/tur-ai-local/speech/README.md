<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# The speech catalogue

One TOML file per speech model or voice, in the format of
`schemas/speech-model.schema.json`, loaded by `src/speech.rs` (which also
enforces the rules below). Installed to `/usr/share/tur/ai-local/speech/`.
Nothing here is downloaded until a person asks: `tur-ai-local speech
ensure`, run by P@x's `PrepareVoice` after the person was shown the size.
The design is `docs/architecture/pax-voice.md`.

| Kind | Engine | Entries |
|---|---|---|
| `recognition` | whisper.cpp's `whisper-cli` | `whisper-tiny-q5-1`, `whisper-base-q5-1` (the free tier), `whisper-small-q5-1`, `whisper-large-v3-turbo-q5-0` |
| `voice-activity` | whisper.cpp (`--vad`) | `silero-vad-v6.2.0` |
| `synthesis` | Piper (piper1-gpl, a GPL program run as a child process) | `voice-en-us-ljspeech-medium` (English default), `voice-en-gb-cori-medium`, `voice-it-paola-medium` (Italian default), `voice-de-thorsten-medium`, `voice-es-davefx-medium`, `voice-fr-siwis-medium` |

## How an entry is written

- `id` follows the model catalogue's rule (lower-case letters, digits, `.`,
  `-`); the publisher's file names are kept as they are in `files`.
- Every file's URL is `https`, names the repository's pinned commit
  (`revision`, 40 hexadecimal digits) and ends with the file's name; its
  size and SHA-256 are the repository's own metadata (the Hugging Face API's
  tree with LFS digests). For a small file that is not in LFS (a Piper
  voice's `.onnx.json`), the SHA-256 was computed from the file fetched at
  the pinned commit.
- Sources read, 16 November 2026:
  - Whisper models: `huggingface.co/ggerganov/whisper.cpp` at
    `5359861c739e955e79d9a303bcbc70fb988958b1`; licence from the
    openai/whisper README ("code and model weights are released under the
    MIT License").
  - Silero VAD: `huggingface.co/ggml-org/whisper-vad` at
    `9ffd54a1e1ee413ddf265af9913beaf518d1639b`; snakers4/silero-vad is MIT.
  - Piper voices: `huggingface.co/rhasspy/piper-voices` at
    `e9ab671f2a0e50a701b34361136ba3e6a3a9eb0c`; each voice's MODEL_CARD;
    piper1-gpl's `docs/LICENSING.md` ("no license grant of its own over
    voice models") and `docs/COMMERCIAL_USE.md`.

## Voices: licences through the whole training chain

A Piper voice's licence is its dataset's, and the dataset of every base
voice it was fine-tuned from. Only voices clean through the whole chain
are listed, and each has `restrictions = "none"`; a voice that needs
attribution says so in `credit`, which TUR shows with the voice.

Several voices were retrained between 9 and 13 November 2026 so that they
no longer descend from the research-only Lessac voice. Their file names did
not change: **only the SHA-256 in the entry proves the retrained copy.**
They need Piper 1.5 or later; Fedora 45's `python3-piper-tts` is 1.4.2, so
TUR installs piper-tts 1.8.0 (pax-voice.md, section 9).

Left out: `en_US-lessac-medium` (Lessac Blizzard 2013 licence: research
only); `en_US-libritts_r-medium` (fine-tuned from lessac);
`en_US-amy-medium` (no documented licence); `it_IT-riccardo-x_low`
(M-AILABS; the licence page is down and the voice is not on Piper's list).

## Speed and memory

`memory_bytes` and `encode_ms_8_threads` are estimates, each with its basis
in the entry (`speed_basis`); TUR has not measured them. The `pax-voice`
workflow (`.github/workflows/pax-voice.yml`, started by hand) measures
latency and peak memory for each recognition model; its results replace
the estimates in a later change.

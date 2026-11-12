# local-ai-1 — Local AI, first slice: engine, catalogue, tur-ai-local and the proof in CI

```text
TUR DEVELOPMENT REPORT

Task ID:          local-ai-1
Subsystem:        Local AI (ADR-0021; docs/architecture/local-ai.md)
Build ID:         not applicable: this task builds no image
Source revision:  feat/local-ai, pull request 65; measured commits and runs
                  in "Runs" below

SUMMARY
- What changed: a new component, services/tur-ai-local (binary
  tur-ai-local), that reads the hardware, keeps a curated catalogue of
  five Apache-2.0 GGUF models pinned to their publishers' revisions,
  recommends a model that fits and refuses one that does not, downloads
  and checks size and SHA-256 before a model can be used, and serves it
  with llama.cpp's llama-server on 127.0.0.1 only, with an access key,
  until it is idle or stopped. Three read-only P@x tools. Two schemas. A
  workflow, local-ai, that proves it with Fedora 45's own llama-cpp in a
  Fedora 45 container, measures every model, compares llama.cpp's own
  build and Fedora's Ollama, and runs P@x's adapters against it.
- Engine decision: llama.cpp's llama-server (section 6 of the design).
  Fedora 45's build works but is slow on the processor (no AVX2 kernels),
  has no Vulkan and drags in about 5.3 GB of ROCm.
- What it does not show: nothing ran on a graphics processor, on a TUR
  image, on battery, or on the test PC. P@x has no local preset and no
  Chat Completions adapter; its existing adapters were run against the
  local server's compatible endpoints.

FILES
- Added: services/tur-ai-local/ (Cargo.toml, Cargo.lock; src: lib, main,
  codes, hardware, catalogue, fit, store, download, engine, serve, cli;
  tests: agent_tools, catalogue, cli, common; tests/fixtures: two hosts,
  a stand-in engine, README; catalogue: five entries and README;
  pax/agent-tools.json, pax/agent-tools.adapter.json; ci/local_ai_ci.py);
  schemas/local-model.schema.json; schemas/ai-local-report.schema.json;
  .github/workflows/local-ai.yml; docs/reports/local-ai-1.md
- Modified: docs/architecture/local-ai.md (sections 6 to 9),
  docs/dependencies/upstream-projects.md (Local AI section, S44 to S49),
  docs/dependencies/licence-review.md (two rows of section 5, question 15),
  THIRD_PARTY_NOTICES.md (models and CUDA are not included)
- Not modified: crates/tur-core (the AI family was there; codes are
  TUR-AI-LOCAL-001 to -027), image/, services/tur-pax*, tests/pax,
  services/tur-catalogue, catalogue/*.toml, desktop/

BUILD
- cargo clippy --all-targets --all-features -- -D warnings; cargo test
  (workflow "rust", job services/tur-ai-local); cargo build --release
  (workflow "local-ai", three jobs)
- Result: see "Runs"

TESTS
- Unit and integration tests of services/tur-ai-local: workflow "rust".
  The supervisor is tested with a stand-in engine (a Python script):
  serve, status, the key file's mode, a refused second server, a refused
  removal while serving, stop, and stop when idle.
- Workflow "local-ai" against the real engine and real models: section 2.
- On a TUR image, a graphics processor, the test PC, on battery: NOT RUN.

DIAGNOSTICS
- Artifacts local-ai-fedora-evidence, local-ai-compare-evidence,
  local-ai-pax-evidence of each local-ai run (JSON of every check and
  measurement, engine log tails, package lists with sizes and licences,
  P@x transcripts and audit trails)
- New error IDs: TUR-AI-LOCAL-001 to -027

RECOVERY
- Revert the merge. Nothing is in the image; no boot path, no stored data
  of the system is touched. Models a person downloaded stay in
  $XDG_DATA_HOME/tur/models until removed.

KNOWN LIMITATIONS
- Section 5 below, and section 8 of the design.

NEXT SAFE ACTION
- Decide the engine build for the image (section 4), then put
  tur-ai-local, its catalogue and the engine in the image; give P@x a
  Chat Completions adapter and a "local" preset that reads the key file.
```

## 1. Runs

See the table at the end of this section for every run of the `local-ai`
workflow; the measured numbers below come from run
[43277339192](https://github.com/giorgiogiudice/TUR/actions/runs/43277339192)
on commit `0f4bb50`, whose code is the final code except for two changes
made because of what it showed: progress lines are no longer printed when
standard error is not a terminal (the proof's two refusal checks of an
imported file failed only because a progress line came before the code),
and the speed measurement no longer reuses the prompt cache (its prompt
speeds were meaningless and are not reported). The final run is in
section 6.

## 2. The proof with Fedora's engine

Job "Fedora 45 llama-server, driven by tur-ai-local", run 43277339192:
`llama-cpp-b9840-2.fc45` installed from Fedora 45's repositories into
`quay.io/fedora/fedora:45@sha256:fa773d10…`, the container on the host
network, everything driven by `tur-ai-local` as the runner's user.

| Check | Result |
|---|---|
| `hardware`, `recommend`, `models`, `status` answer in JSON | pass |
| `pull qwen3-0.6b`: downloaded from Hugging Face at the pinned revision (639 446 688 bytes, 5.3 s) and checked; `verify` re-hashed it in 0.6 s | pass |
| A copy with one byte changed, imported with `--from-file`: refused, nothing kept | refused with `TUR-AI-LOCAL-010` and nothing kept, but the check failed: a progress line preceded the code on standard error (fixed, see section 1) |
| The same file 4 096 bytes short: refused (`-009`) before anything was copied | pass |
| A catalogue entry with the right URL and a size one byte larger: refused from the announced length before the body was read (0.2 s), nothing kept | pass |
| The same with a wrong SHA-256: the whole file downloaded (4.8 s), refused (`-010`), nothing kept | the code was there, the check failed on the progress line (fixed) |
| `serve` on 0.0.0.0 without `--allow-network`: refused (`-016`) | pass |
| `serve`: ready in 1.8 s; listening on 127.0.0.1 only (from `/proc/net/tcp`) | pass |
| `status` while serving: running, port, engine memory; the key is in no report | pass |
| A request without the key: 401; `/health` without it: 200 | pass |
| The API on the runner's own network address: connection refused | pass |
| A chat completion; a streamed one (12 chunks, first after 1.1 s, `[DONE]`) | pass |
| A tool call (`get_weather`), the answer after the tool result, a streamed tool call | all yes |
| Idle stop after 30 s without a request: stopped by itself 30.5 s after the last request (`-020`); nothing listens on the port; `status` says not running; the key file is gone | pass |

## 3. Every model, Fedora's build and llama.cpp's own build

Same runner type (GitHub-hosted ubuntu-24.04: AMD EPYC 7763, 4 logical
processors, 2 cores, AVX2 and FMA, 16.8 GB, no graphics processor), same
`tur-ai-local`, context 8192, `--reasoning off`. Generation speed is the
second of two identical requests of up to 128 tokens after a ~250-word
prompt (`timings.predicted_per_second` of llama-server). Peak memory is the
engine's `VmHWM`. "Sensible" is the answer to "What is the capital of
France? Answer with one word." Image input: a 64×64 red PNG and "What
colour is this image?".

| Model | Licence | Download | Fedora build: generation tok/s | Fedora build: peak memory | llama.cpp build: generation tok/s | llama.cpp build: peak memory | Sensible | Tool call | Streamed tool call | Image | Embeddings |
|---|---|---|---|---|---|---|---|---|---|---|---|
| qwen3-0.6b | Apache-2.0 | 0.64 GB | 8.75 | 1.73 GB | 52.70 | 1.66 GB | yes (both) | yes (both) | yes (both) | – | – |
| granite-4.0-1b | Apache-2.0 | 1.02 GB | 3.71 | 1.84 GB | 31.35 | 2.46 GB | yes | yes | yes | – | – |
| qwen3-vl-2b-instruct | Apache-2.0 | 1.55 GB | 3.72 | 2.68 GB | 32.87 | 3.28 GB | yes | yes | yes | yes (both) | – |
| qwen3-4b | Apache-2.0 | 2.50 GB | 1.61 | 3.87 GB | 14.85 | 5.56 GB | yes | yes | yes | – | – |
| qwen3-embedding-0.6b | Apache-2.0 | 0.64 GB | – | 1.72 GB | – | 1.65 GB | – | – | – | – | yes, 1024 dimensions (both) |

Every model answered "Paris", every chat model returned a well-formed tool
call to `get_weather` with `{"city": "Paris"}`, used the tool's result in its
next answer, and streamed a well-formed tool call, on both builds. The
vision model named the colour red on both builds.

**Fedora's build is 6 to 9 times slower on this processor** (8.75 against
52.70 tokens per second for the smallest model; 1.61 against 14.85 for the
4B model). Its build log shows why: one processor library, built with AVX,
AVX2, FMA and F16C turned off. Its memory is lower for the larger models,
because llama.cpp's processor variants keep a repacked copy of the weights.

**Memory estimates.** Three catalogue estimates were below what llama.cpp's
own build used (Granite 2.25 GB against 2.46, Qwen3-VL 3.35 against 3.28
with little margin, Qwen3 4B 4.25 against 5.56). They were raised to 10 %
above the higher measured peak: 2.75, 3.65 and 6.15 GB. The two 0.6B models'
estimates (2.15 GB) stayed above both peaks.

**Prompt speed is not reported** for this run: the second request reused
the prompt cache of the first, so it processed one token. The final run
sends `cache_prompt: false` (section 6).

## 4. The engine comparison

| | Fedora 45 `llama-cpp-b9840-2.fc45` | llama.cpp's release b9840 (ubuntu-x64) | Fedora 45 `ollama-base-0.24.0-4.fc45` |
|---|---|---|---|
| Installed size | 113 packages, 5 451 MB in a plain Fedora 45 container; of which llama-cpp and the ROCm/HIP packages it requires: 21 packages, 5 297 MB (rocm-llvm-static 2 353 MB, rocsolver 1 126 MB, rocblas 1 022 MB, llama-cpp 254 MB, …); the rest is Perl, Git and others pulled in by `hipcc` | a 15.8 MB archive, SHA-256 checked | 4 packages, 69 MB, of which ollama-base 47 MB (systemd, already in Kinoite, was the rest) |
| Licences | `MIT AND Apache-2.0 AND LicenseRef-Fedora-Public-Domain`; ROCm: MIT, BSD, `Apache-2.0 WITH LLVM-exception OR NCSA` (per package in the artifact) | MIT | Fedora's 18-licence tag (section 6.1 of the design) |
| Backends found by `tur-ai-local` | `cpu`, `hip` | `cpu-*` variants | (not served by tur-ai-local) |
| `--version` | `version: 0 (unknown)` | `version: 9840 (8c146a836)` | |
| ROCm on a machine without AMD graphics | logs "failed to initialize ROCm: no ROCm-capable device is detected" and runs on the processor | | |
| qwen3-0.6b generation | 8.75 tok/s | 52.70 tok/s | 37.58 tok/s |
| qwen3-0.6b peak memory | 1.73 GB | 1.66 GB | 1.86 GB |
| Tool call | yes | yes | **refused**: "qwen3-local does not support tools" |
| Chat answer | "Paris" | "Paris" | "The capital of France is Paris.\nAnswer:\nParis\n\nThe capital of France is Paris…" repeated: the publisher's GGUF imported with `ollama create` got no chat template |
| Listens on | 127.0.0.1 (with the key) | 127.0.0.1 (with the key) | 127.0.0.1:11434, no authentication |

Ollama with the publisher's own file needs a hand-written template to
converse and call tools; with its registry's packaging it would not, but
that is the registry TUR does not want to depend on.

## 5. P@x against the local model

To be completed from the `pax` job (section 6).

## 6. The final run

To be completed.

## 7. Limits and what is not proven

- Nothing ran on a graphics processor: the runner has none, and Fedora's
  build has no Vulkan. Vulkan device detection was checked only with
  llvmpipe (counted as not usable, correctly) and a hand-written summary.
- Speeds are of a shared 4-processor cloud runner; they rank builds and
  models, they do not predict a person's computer.
- One model is served at a time; no router mode.
- Battery, foreground priority and per-application permissions (ADR-0021,
  items 8 and 9) are not implemented.
- The idle stop was proved with 30 seconds, not the default 600.
- P@x's sandbox cannot see whether a model is running (`TUR-AI-LOCAL-027`).
- The licence of three models rests on their card metadata: their
  repositories hold no licence file (licence review, question 15).

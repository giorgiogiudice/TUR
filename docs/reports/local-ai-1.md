# local-ai-1 — Local AI, first slice: engine, catalogue, tur-ai-local and the proof in CI

```text
TUR DEVELOPMENT REPORT

Task ID:          local-ai-1
Subsystem:        Local AI (ADR-0021; docs/architecture/local-ai.md)
Build ID:         not applicable: this task builds no image
Source revision:  eaa906cd8704 (feat/local-ai, pull request 65): the commit
                  the final measurements come from. The commit that adds
                  the final text of this report follows it and changes no
                  code.

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
  build and Fedora's Ollama, and runs P@x's own adapters against it.
- Engine decision: llama.cpp's llama-server (design, section 6). Fedora
  45's build works, but is 5.5 to 20 times slower on the processor (no AVX2
  kernels), has no Vulkan and installs about 5.3 GB of ROCm.
- P@x: its own OpenAI (Responses) and Anthropic (Messages) adapters,
  unchanged, completed real tasks against the local server with Qwen3 4B:
  tool calls proposed, allowed, performed in P@x's sandbox, answered.
- What it does not show: nothing ran on a graphics processor, on a TUR
  image, on battery or on the test PC. P@x has no Chat Completions
  adapter and no "local" preset.

FILES
- Added: services/tur-ai-local/ (Cargo.toml, Cargo.lock; src: lib, main,
  codes, hardware, catalogue, fit, store, download, engine, serve, cli;
  tests: agent_tools, catalogue, cli, common; tests/fixtures: two hosts,
  a stand-in engine, README; catalogue: five entries and README;
  pax/agent-tools.json, pax/agent-tools.adapter.json; ci/local_ai_ci.py);
  schemas/local-model.schema.json; schemas/ai-local-report.schema.json;
  .github/workflows/local-ai.yml; docs/reports/local-ai-1.md
- Modified: docs/architecture/local-ai.md (status, sections 2, 5, and new
  sections 6 to 9), docs/dependencies/upstream-projects.md (Local AI
  section, sources S46 to S51), docs/dependencies/licence-review.md (two
  rows of section 5, question 15), THIRD_PARTY_NOTICES.md (models and
  CUDA are not included)
- Not modified: crates/tur-core (the AI family existed; the codes are
  TUR-AI-LOCAL-001 to -027), image/, services/tur-pax*, tests/pax,
  services/tur-catalogue, catalogue/, desktop/, ai-control-layer.md,
  specification.md

BUILD
- cargo clippy --all-targets --all-features -- -D warnings; cargo test
  --all-targets; cargo test --doc (workflow "rust", job
  services/tur-ai-local); cargo build --release (workflow "local-ai")
- Result: pass on eaa906c (rust 43296747004, local-ai 43296747035)
- Warnings: none from the compiler or clippy

TESTS
- Unit tests: 23 in the library. Pass.
- Integration tests: 17 in three files. Pass.
    tests/cli.rs          7  every read-only command's JSON on two recorded
                             machines against the schema; refusals with
                             their codes; import, verify, a changed file
                             refused, remove; serve, status, stop and the
                             idle stop with a stand-in engine (key file
                             0600, key never printed, second server and
                             removal while serving refused, request
                             without the key refused, port closed after)
    tests/agent_tools.rs  5  the tool manifest and adapters with P@x's
                             loader, against the built program, run with
                             an empty environment as in the sandbox
    tests/catalogue.rs    5  the seed catalogue against its schema and
                             rules; pinned URLs; measured results have
                             their report
- Workflow "local-ai", run 43296747035: 30 of 30 required checks of the
  proof with Fedora's engine; every catalogue model on Fedora's build and
  on llama.cpp's own build; Fedora's Ollama; P@x and a harness. Pass.
- On a TUR image, on a graphics processor, on the test PC, on battery,
  as a systemd user unit: NOT RUN.

DIAGNOSTICS
- Artifacts of each local-ai run: local-ai-fedora-evidence,
  local-ai-compare-evidence, local-ai-pax-evidence (every check and
  measurement as JSON, engine log tails, package lists with sizes and
  licences, P@x transcripts and audit trails)
- New error IDs: TUR-AI-LOCAL-001 to -027

RECOVERY
- Revert the merge. Nothing is in the image; no boot path and no system
  data is touched. Models a person downloaded stay in
  $XDG_DATA_HOME/tur/models until removed.

KNOWN LIMITATIONS
- Section 7 below and section 8 of the design.

NEXT SAFE ACTION
- Decide the engine build for the image (section 8 below), then put
  tur-ai-local, its catalogue, its tool set and the engine in the image;
  give P@x a Chat Completions adapter and a "local" preset that reads the
  key file; cap or drop the 4 096-character limit in the read-only
  command tool's schema (section 6).
```

> **Later (TUR-010):** the tool files this report puts under `pax/` were moved to the component's top level (pax-5 for tur-apps and tur-ai-local, TUR-010 for tur-update), where P@x discovers every component's tools, and the image installs them.

## 1. Runs

| Commit | Workflow | Run | Result | What it showed |
|---|---|---|---|---|
| `15068a9` | rust | 43274532272 | fail | Clippy: `large_enum_variant` on `StopOutcome`. Everything else of the library compiled at the first attempt |
| `15068a9` | local-ai | 43274477107 | fail, no job | The workflow was invalid: `runner.temp` used in a job's `env`, where the `runner` context does not exist |
| `e3d7847`, `119361a` | all | 4327554…, 4327558… | cancelled | Replaced by the next push; the cancelled Fedora job had already shown the 113 packages of `llama-cpp` |
| `0f4bb50` | rust | 43277339149 | fail | One test: a progress line on standard error came before the code of a refused import |
| `0f4bb50` | local-ai | 43277339192 | fail | The proof: 28 of 30 required checks passed; the two refusals were made with the right codes, but a progress line preceded them (fixed). Every model measured on both builds: the source of the catalogue's `[[tested]]` records. P@x: every request rejected by llama-server (section 6). Its prompt speeds were meaningless (cache reuse) and are not reported |
| `ac8880c` | none | | | The pull request conflicted with main (`upstream-projects.md`), so no workflow ran |
| `495a2c7` | rust | 43290846986 | fail | rustfmt only; every test passed. Merge of main |
| `495a2c7` | local-ai | 43290847045 | cancelled | Replaced by the next push |
| **`eaa906c`** | **rust** | **43296747004** | **pass** | every component |
| **`eaa906c`** | **local-ai** | **43296747035** | **pass** | the measured run, sections 2 to 5 |
| `eaa906c` | gate | 43296747113 | pass | |

Not run: `pax`, `boot`, `image`, `desktop`, `update`, `windows-runtime`: no
file they watch changed. The merge commit `495a2c7` has git's default
message, without the session lines: rewriting it needed a force push, which
was not allowed here.

## 2. The proof with Fedora's engine

Job "Fedora 45 llama-server, driven by tur-ai-local", run 43296747035:
`llama-cpp-b9840-2.fc45` from Fedora 45's repositories in
`quay.io/fedora/fedora:45@sha256:fa773d10…`, the container on the host
network, everything driven by `tur-ai-local` as the runner's user. All 30
required checks passed.

| Check | Result |
|---|---|
| `hardware`, `recommend`, `models`, `status` answer in JSON | pass |
| `pull qwen3-0.6b`: downloaded from Hugging Face at the pinned revision (639 446 688 bytes, 17.3 s) and checked; `verify` re-hashed it in 0.4 s. (Later runs restore it from the cache keyed by its SHA-256; this run's cache key was new.) | pass |
| A copy with one byte changed, imported with `--from-file`: refused (`TUR-AI-LOCAL-010`), nothing kept | pass |
| The copy 4 096 bytes short: refused (`-009`) before anything was copied | pass |
| A catalogue entry with the publisher's URL and a size one byte larger: refused from the announced length before the body was read (0.1 s), nothing kept | pass |
| The same with a wrong SHA-256: the whole file received (13.4 s), refused (`-010`), nothing kept | pass |
| `serve --host 0.0.0.0` without `--allow-network`: refused (`-016`) | pass |
| `serve`: ready in 1.8 s; listening on 127.0.0.1 only (`/proc/net/tcp`) | pass |
| `status` while serving: running, the port, the engine's memory; the key appears in no report | pass |
| A request without the key: 401. `/health` without it: 200 | pass |
| The API on the runner's own network address (10.1.1.102): connection refused | pass |
| A chat completion; a streamed one: 12 chunks, the first after 1.0 s, `[DONE]` | pass |
| A tool call to `get_weather`, the answer after the tool result, a streamed tool call | yes, yes, yes |
| Idle stop with `--idle-stop 30`: stopped by itself 31.8 s after the last request (`-020`); nothing listens on the port; `status` says not running; the key file is gone | pass |

The runner's own hardware, read by `tur-ai-local hardware` with
`vulkan-tools` and Mesa installed: AMD EPYC 7763, 4 logical processors (2
cores), AVX2 and FMA; 16.8 GB; a Hyper-V display device; Vulkan lists one
device, llvmpipe, of type CPU, which is counted as not usable, so
`usable_open_backend` is false.

## 3. Every model, on Fedora's build and on llama.cpp's own build

Same runner type, same `tur-ai-local`, context 8192, `--reasoning off`.
Speeds are llama-server's own `timings` of the second of two identical
requests, sent with `cache_prompt: false`: a prompt of 159 to 181 tokens
(a ~250-word passage) and up to 128 generated tokens. Peak memory is the
engine's `VmHWM`. "Sensible": the answer to "What is the capital of France?
Answer with one word." Image input: a 64×64 red PNG and "What colour is
this image?".

| Model | Licence | Download | Fedora build: generation / prompt tok/s | Fedora build: peak memory | llama.cpp build: generation / prompt tok/s | llama.cpp build: peak memory | Sensible | Tool call | Streamed tool call | Image | Embeddings |
|---|---|---|---|---|---|---|---|---|---|---|---|
| qwen3-0.6b | Apache-2.0 | 0.64 GB | 9.61 / 16.56 | 1.73 GB | 53.09 / 293.54 | 1.66 GB | yes | yes | yes | – | – |
| granite-4.0-1b | Apache-2.0 | 1.02 GB | 4.31 / 5.94 | 1.84 GB | 29.92 / 114.38 | 2.46 GB | yes | yes | yes | – | – |
| qwen3-vl-2b-instruct | Apache-2.0 | 1.55 GB | 4.34 / 6.20 | 2.68 GB | 32.93 / 122.80 | 3.29 GB | yes | yes | yes | yes | – |
| qwen3-4b | Apache-2.0 | 2.50 GB | 1.94 / 2.43 | 3.87 GB | 14.99 / 47.87 | 5.56 GB | yes | yes | yes | – | – |
| qwen3-embedding-0.6b | Apache-2.0 | 0.64 GB | – | 1.72 GB | – | 1.65 GB | – | – | – | – | yes, 1 024 dimensions |

On both builds every chat model answered "Paris", returned a well-formed
tool call `get_weather {"city": "Paris"}`, used the tool's result in its
next answer and streamed a well-formed tool call; the vision model named
the colour red; the embeddings model returned 1 024-dimensional vectors.
Run 43277339192 gave the same outcomes and generation speeds within 20 %.

**Fedora's build is 5.5 to 7.7 times slower at generating and 18 to 20
times slower at reading the prompt.** Its spec builds one processor library
with AVX, AVX2, FMA and F16C turned off. For P@x, whose requests carry
1 000 to 3 000 tokens of instructions and tool definitions, the prompt
speed decides: with Qwen3 4B on Fedora's build one such request took about
12 minutes in run 43277339192 (1.5 to 1.8 prompt tokens per second).

**Memory.** Fedora's build used less for the larger models, because
llama.cpp's processor variants keep a repacked copy of the weights. Three
catalogue estimates were below llama.cpp's own build's peaks in run
43277339192 and were raised to 10 % above the higher peak (Granite 2.25 →
2.75 GB, Qwen3-VL 3.35 → 3.65 GB, Qwen3 4B 4.25 → 6.15 GB). In the final
run every peak is below its estimate.

**The recommendation follows the engine.** With Fedora's engine installed
`tur-ai-local recommend` on the runner names Qwen3 0.6B (`engine_cpu:
single`, budget 0.8 GB); with no engine or llama.cpp's own build it names
Qwen3 4B (budget 3 GB), with Qwen3-VL 2B for images and Qwen3 Embedding for
search.

The catalogue's `[[tested]]` records are from run 43277339192 (generation
speed, peak memory, tool call, image, embeddings; no prompt speed). They
were not replaced by the final run's numbers, because changing the
catalogue starts the whole workflow again.

## 4. The engine comparison

| | Fedora 45 `llama-cpp-b9840-2.fc45` | llama.cpp's release b9840 (ubuntu-x64) | Fedora 45 `ollama-base-0.24.0-4.fc45` |
|---|---|---|---|
| Installed | 113 packages, 5 451 MB in a plain Fedora 45 container; of which `llama-cpp` and the ROCm/HIP packages it requires: 21 packages, 5 297 MB (`rocm-llvm-static` 2 353 MB, `rocsolver` 1 126 MB, `rocblas` 1 022 MB, `llama-cpp` 254 MB, …); the rest is Perl, Git and others that `hipcc` pulls in, and Python for the test | a 15.8 MB archive, SHA-256 checked | 4 packages, 69 MB: `ollama-base` 47 MB, the rest systemd (already in Kinoite) |
| Licences (Fedora's tags, per package in the artifact) | `MIT AND Apache-2.0 AND LicenseRef-Fedora-Public-Domain`; ROCm: MIT, BSD-2/3-Clause, 0BSD, `(Apache-2.0 WITH LLVM-exception OR NCSA) AND NCSA AND MIT` | MIT | the 18-licence tag in the design, 6.1 |
| Backends `tur-ai-local` finds | `cpu`, `hip` | `cpu-*` variants | – |
| `--version` | `version: 0 (unknown)` | `version: 9840 (8c146a836)` | 0.24.0 |
| Without AMD graphics | logs "failed to initialize ROCm: no ROCm-capable device is detected" and runs on the processor | | |
| qwen3-0.6b generation / prompt | 9.61 / 16.56 tok/s | 53.09 / 293.54 tok/s | 35.16 / 246.46 tok/s |
| qwen3-0.6b peak memory | 1.73 GB | 1.66 GB | 1.86 GB |
| Tool call | yes | yes | **refused**: "qwen3-local does not support tools" |
| Chat | "Paris" | "Paris" | "The capital of France is Paris.\nAnswer:\nParis…" repeated until the token limit: the publisher's GGUF imported with `ollama create` got no chat template |
| Listens on | 127.0.0.1, key required | 127.0.0.1, key required | 127.0.0.1:11434, no authentication |

Ollama with the publisher's own file would need a hand-written template to
converse and call tools; Ollama's registry packages them, but that is the
registry and the cloud integration TUR does not want to depend on.

## 5. P@x against the local model

`tur-pax run`, built from main, with its own adapters and the
`base_url` of the local server, the shipped tool sets of `tur-catalogue`,
`tur-device-manager` and `tur-ai-local`, P@x's test fixture tool, the real
bubblewrap sandbox, and `--no-commands` (section 6). Two requests: the
`tests/pax` scenario `01-two-tools` ("My laptop has hardware trouble. What
is wrong with it, and how would I run Discord?") and "Which AI model could
run locally on this computer, and why?". Qwen3 0.6B on Fedora's engine;
Qwen3 4B on llama.cpp's own build, because Fedora's build reads a P@x
prompt too slowly (section 3).

| Model, engine | Adapter | Request | Outcome | Tools P@x performed | Seconds |
|---|---|---|---|---|---|
| qwen3-4b, llama.cpp build | OpenAI (Responses, `/v1/responses`) | two-tools | completed | `device_list_problems` | 141.5 |
| qwen3-4b, llama.cpp build | OpenAI | local AI | completed | `ai_local_recommend` | 111.4 |
| qwen3-4b, llama.cpp build | Anthropic (Messages, `/v1/messages`) | two-tools | completed | `device_list_problems`, `catalogue_route` | 116.3 |
| qwen3-4b, llama.cpp build | Anthropic | local AI | completed | `ai_local_recommend` | 112.8 |
| qwen3-0.6b, Fedora build | OpenAI | two-tools | completed, no tool call: "I'm not sure about the hardware trouble…" | none | 151.7 |
| qwen3-0.6b, Fedora build | OpenAI | local AI | completed, no tool call; the answer invents a model named "AI Local Models" | none | 26.2 |
| qwen3-0.6b, Fedora build | Anthropic | two-tools | completed, no tool call; generic installation advice | none | 45.4 |
| qwen3-0.6b, Fedora build | Anthropic | local AI | completed, no tool call; the answer repeats the question | none | 14.9 |

With Qwen3 4B every task completed through P@x's whole loop: the model's
tool calls were read by the unchanged adapters, passed the policy engine as
class 0, ran in the sandbox (`tur-ai-local recommend` included, from
`/usr/share/tur/ai-local/models`), and the answers used the results: "Qwen3
4B… requires about 6.2 GB of memory, which fits within the available
memory, leaving 1.7 GB for other system processes…". For the hardware
question it chose the real machine's `device_list_problems` over the
fixture laptop's tool. Qwen3 0.6B never called a tool through P@x: it can
converse but not act, as ADR-0021 expects P@x to say.

The **Chat Completions harness** (`ci/local_ai_ci.py harness`, not P@x:
same tools, schema checks, no sandbox, no policy engine): Qwen3 4B called
`device_get_inventory`, `device_list_problems` and `catalogue_search
{"query": "Discord"}` for the scenario, and `ai_local_recommend` for the
local AI question; Qwen3 0.6B called `ai_local_recommend` for the second
and nothing for the scenario. Every tool call made was well formed; none
named a tool that was not offered, repeated a key or broke its schema.

## 6. Findings

1. **P@x's read-only command tool breaks llama-server's tool grammar.** Its
   schema allows `argv` items of up to 4 096 characters; llama.cpp turns
   that into `char{0,4096}` and refuses the grammar ("number of repetitions
   exceeds sane defaults"), so every request that offers the tool is
   rejected (`TUR-AI-PROVIDER-004`, run 43277339192). The workflow runs
   tur-pax with `--no-commands`. Either the schema's limit is lowered
   (under ~2 000) or a local provider is told to leave the tool out.
2. **Fedora 45's `llama-cpp` is not fit to ship as it is**: section 4.
3. **The catalogue cannot live in `catalogue/models/`**: the application
   catalogue's loader refuses subdirectories (design, section 8).
4. **`/metrics` waits for the engine's main loop.** While a long prompt is
   processed, llama-server answers the supervisor's metrics request only
   after up to minutes; the supervisor gives up after 10 s and counts that
   as activity, so a busy engine is never stopped, but the engine logs a
   cancelled task every 10 s.
5. **Ollama's import of a publisher's GGUF** loses the chat template and
   tool support (section 4).

## 7. Limits and what is not proven

- Nothing ran on a graphics processor: the runner has none, and Fedora's
  build has no Vulkan. Vulkan detection was checked only with llvmpipe
  (counted as not usable, correctly) and with a hand-written summary.
- Speeds are of a shared 4-processor cloud runner. They rank builds and
  models; they do not predict a person's computer.
- One model is served at a time; llama-server's router mode is not used.
- Battery, foreground priority and per-application permissions (ADR-0021,
  items 8 and 9) are not implemented. Local speech (item 7) is not started.
- The idle stop was proved with 30 seconds, not the default 600.
- P@x's sandbox cannot see whether a model is running (`TUR-AI-LOCAL-027`).
- The licence of three models rests on their card metadata; their
  repositories hold no licence file (licence review, question 15).
- The P@x runs were made with the shipped tools of three components and the
  fixture tool, not with the full tool set of every component.
- `tur-ai-local` was not run as a systemd user unit or in a desktop session.

## 8. Decisions the owner may want to take or reverse

1. **The engine build for the image**: Fedora's `llama-cpp` as it is
   (works, proved, 5.3 GB, slow), a TUR build of the same pinned llama.cpp
   with processor variants and Vulkan and without HIP, or a change to
   Fedora's spec (design, section 6.2).
2. **Five models, all Apache-2.0**; LFM (restrictive licence) left out;
   Gemma 4 E2B and Ministral 3 3B left out for size and run time only.
3. **Thinking off by default** (`--reasoning off`) for speed on a processor.
4. **An access key on loopback too**, which every client must read from the
   runtime folder.
5. **Port 39281** as the default.
6. **The idle time of 600 s.**
7. **The catalogue's location** in `services/tur-ai-local/catalogue/`.

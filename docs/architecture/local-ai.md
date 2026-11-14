# Local AI

**Status:** first slice built, 12 November 2026; models by hardware and
`ensure`, 13 November 2026 (issue #84). Decision: [ADR-0021](../adr/0021-local-ai.md).
Sections 1 to 5 are the design. Section 6 records the engine decision and its
evidence, section 7 what is built (`services/tur-ai-local`), section 8 what
deviates from the design and what is open, section 9 what the image holds,
section 10 the second list of models and the tiers, section 11 `ensure`.
Measurements: [report local-ai-1](../reports/local-ai-1.md),
[report pax-6](../reports/pax-6.md).

TUR runs AI models on the computer itself, for anyone who wants AI without an
account, a key, a bill or sending anything to a company. P@x can use a local
model, and so can any application that speaks the common OpenAI-compatible API.

## 1. What the person sees

- **Setup and Settings → AI** offer three choices, any combination of which
  may be on: *no AI*, *local AI on this computer*, *a cloud service with my
  own key*. Local AI is presented first, with an honest line on what this
  computer can run well.
- **Choosing a model.** The application centre lists curated models with:
  what it is good at (conversation, writing, coding, translation, images),
  download size and memory needed, expected speed on *this* computer, whether
  P@x can act with it (tool calling) and see with it (image input), and its
  licence in plain words. A recommended model is preselected for the
  hardware.
- **Using it.** P@x works with the chosen model. A simple chat window and the
  P@x ask bar both use it. Other applications can be allowed to use it.
- **Privacy line, always true:** "Local AI runs on this computer. What you
  ask and what it answers do not leave it."

## 2. Components

| Component | Role |
|---|---|
| `tur-ai-local` | A user service that manages engines and models: download, verify, start on demand, stop when idle, memory checks, the local API, permissions per application |
| Inference engines | Open-source, installed from Fedora or the upstream release, pinned; chosen by measurement. **Chosen: llama.cpp's `llama-server`** (section 6) |
| Model catalogue | One entry per model with source, digest, size, licence, capabilities, hardware needs, tested results. **Built** in `services/tur-ai-local/catalogue/`, format `schemas/local-model.schema.json`; not yet in the application catalogue (section 8) |
| P@x adapter | The OpenAI-compatible adapter (ADR-0004 amendment) pointed at the local API; the provider entry `local` is preset |
| Settings pages | AI → Local models; Storage shows model files; Privacy shows which applications may use local AI |

## 3. Rules

- Off and silent until switched on; no background download.
- Loopback or socket only by default; network serving is an explicit,
  visible, per-session choice.
- Never ship a model in the image; download from the publisher's official
  source; check the digest; show the licence before download.
- Never ship vendor-proprietary compute libraries; use them if the person
  installed them.
- Stop models when idle; do not start large models on low battery unless
  asked; yield memory to the foreground.
- All AI control layer rules apply to P@x with a local model, unchanged.

## 4. What is honest to promise

- Conversation, writing, summarising, translating and help with code: good
  with current open models on ordinary recent computers.
- P@x acting through tools: works with models that call tools reliably;
  measured and published per model.
- P@x operating applications by sight: needs a model with image input and
  stronger hardware; measured, not promised.

## 5. Work items

1. Choose and package the engine(s); licence review; `tur-ai-local` with the
   local API, model download and verification, memory checks; CI with a small
   model on the processor.
2. The model catalogue and its schema; curated first list with licences.
3. P@x's `local` provider preset; the scenario suite against a local model
   in CI.
4. Settings and application-centre pages (desktop tasks).
5. Local speech (recognition and synthesis) in the same service.
6. Measurements on the test PC's graphics card.

Items 1 and 2 are done in their first slice (sections 6 and 7). Item 3 is
done: the preset `local` exists and the `local-ai` workflow runs P@x through
it against `tur-ai-local` (section 8, item 3).

## 6. The engine: llama.cpp's `llama-server`

### 6.1 What was compared

Both candidates were read at their sources on 12 November 2026: the
projects' repositories and documentation at the version Fedora ships, and
Fedora 45's package metadata, spec files and build logs (sources in
[upstream-projects.md](../dependencies/upstream-projects.md), S46 to S51).

| | llama.cpp (`llama-server`) | Ollama |
|---|---|---|
| Licence of the engine | `MIT` | `MIT` |
| What it bundles | cpp-httplib (MIT), nlohmann/json (MIT), stb_image (MIT or public domain), miniaudio (public domain or MIT-0), sheredom/subprocess.h (Unlicense). Fedora tags the package `MIT AND Apache-2.0 AND LicenseRef-Fedora-Public-Domain` | Its own copy of llama.cpp's ggml, and vendored Go modules: Fedora's tag lists 18 licences (`Apache-2.0 AND BSD-2-Clause AND BSD-3-Clause AND … AND MIT AND … AND Zlib`). **Upstream's Linux release bundles NVIDIA's CUDA libraries** (1.5 GB archive): never redistributable by TUR |
| Packaged in Fedora 45 | Yes: `llama-cpp-b9840-2.fc45` (f45 and f45-updates; built 2026-07-16). Rawhide has b11232 | Yes: `ollama-0.24.0-4.fc45` (built 2026-07-17), with subpackages `ollama-base`, `ollama-vulkan`, `ollama-rocm`. Rawhide has 0.35.1 |
| Fedora's build: processor | **One baseline x86-64 build**: the spec turns AVX, AVX2, AVX-512, FMA and F16C off, and the Koji build log shows `Setting GGML_NATIVE_DEFAULT to OFF` and a CPU backend variant with no extra instruction set. llama.cpp's fast processor kernels are not used | Seven processor variants chosen at run time (`sse42`, `sandybridge`, `haswell`, `skylakex`, `icelake`, `alderlake`, `x64`) |
| Fedora's build: Vulkan | **Off** in F45 (spec: "broken on stx halo b9781", "Needs a maintainer") | On (`ollama-vulkan`, 6 MB) |
| Fedora's build: ROCm (AMD) | On, and **required**: `Requires: hipblas rocblas`. The `llama-cpp` RPM is 227 MB (HIP kernels for 22 GPU targets); `rocblas` is a 304 MB RPM | On, in its own subpackage (`ollama-rocm`, 44 MB); `ollama-base` alone runs on the processor |
| CUDA | Not built (Fedora does not ship CUDA) | Not built by Fedora; bundled upstream |
| Current release, project health | Builds several times a day (b12319 on 2026-11-12) and a versioned release v0.9.0 (2026-11-04); MIT; 134k stars | v0.40.16 (2026-11-09); MIT; 187k stars. Fedora's 0.24.0 is 16 minor versions behind |
| OpenAI-compatible API | `/v1/chat/completions` with streaming, tool calls (Jinja templates, on by default) and image input (`--mmproj`); `/v1/completions`; `/v1/embeddings` (`--embedding`); `/v1/responses`; Anthropic's `/v1/messages`; `/v1/models` | `/v1/chat/completions` with streaming, tools (no `tool_choice`) and base64 images; `/v1/completions`; `/v1/embeddings`; `/v1/responses`; `/v1/models`; Anthropic compatibility |
| Model format | GGUF: the publisher's file, used as it is | GGUF, imported into Ollama's own store (`ollama create` copies it); by default pulled from Ollama's registry |
| Load and unload on demand | `--sleep-idle-seconds` unloads when idle; router mode (`--models-dir`, `--models-max`) loads models on request | Loads on request; unloads after `OLLAMA_KEEP_ALIVE` (5 minutes) |
| Memory | Fixed at start: the memory-mapped model, the context cache of `--ctx-size`, and a host prompt cache of up to 8 GiB by default (`--cache-ram`; TUR sets 0) | Managed by the server; several models may be resident |
| Security defaults | Binds 127.0.0.1:8080; a Unix socket when the host ends in `.sock`; `--api-key` / `--api-key-file`; `--offline`; the web interface can be switched off | Binds 127.0.0.1:11434; **no authentication** on the local API; **cloud models and web search on by default**: a request to a cloud model through the local API is forwarded to ollama.com (`OLLAMA_NO_CLOUD=1` turns that off); Fedora's unit runs it as a system service under its own user with one shared model store |

Measured in CI on the same runner (report [local-ai-1](../reports/local-ai-1.md),
sections 3 and 4): Fedora's `llama-cpp` installs 21 packages and 5.3 GB with
its ROCm dependencies, against 47 MB for `ollama-base`; with the smallest
model it generated 8.75 tokens per second, llama.cpp's own build of the same
version 52.70 and Fedora's Ollama 37.58; Ollama refused tools for the
publisher's GGUF file it had imported, which also lost its chat template.

### 6.2 Decision

**`llama-server` from llama.cpp**, started by `tur-ai-local` as a child
process of the person's session, one model at a time.

- It does what TUR needs with nothing to switch off: a per-user process, the
  publisher's GGUF file used directly, an access key, a Unix socket,
  `--offline`, no registry, no cloud forwarding. Ollama is a system daemon
  with its own store and registry, no authentication, and cloud features
  that are on by default; each would have to be switched off and kept off.
- Its API is a superset of what P@x needs, including the two APIs P@x's
  adapters already speak (OpenAI Responses and Anthropic Messages).
- One licence, MIT, and a short list of permissive bundled files.
- Ollama runs llama.cpp's ggml underneath; llama.cpp directly is one layer
  less, and no lag behind llama.cpp's support for new models.

**Which build.** The image installs Fedora's packages (ADR-0001), and the
proof in CI uses exactly that: `llama-cpp-b9840-2.fc45` in a Fedora 45
container. Fedora's F45 build has three problems for TUR:

1. no AVX2/FMA kernels: on the processor it is slower than llama.cpp's own
   build of the same version (measured side by side in the report);
2. no Vulkan backend: no open graphics acceleration at all;
3. ROCm is a hard dependency, which adds hundreds of megabytes to every
   image whether or not the computer has an AMD graphics card.

So the decision is **llama.cpp, version pinned, from Fedora's `llama-cpp`
package, with its spec changed to build the processor variants
(`GGML_BACKEND_DL`, `GGML_CPU_ALL_VARIANTS`) and Vulkan and to move ROCm into
a subpackage**, as Fedora's own `ollama` package already does. Until Fedora
carries that, the image can install `llama-cpp-b9840-2.fc45` as it is (works,
proved in CI, slow, large) or TUR builds the same pinned version with those
options (route "built by TUR"). Section 9 lists what each needs. The owner
may want to choose between them explicitly.

## 7. What is built: `tur-ai-local`

A standalone program (`services/tur-ai-local`, its own Cargo workspace): a
library and the binary `tur-ai-local`. Every command prints text, or JSON
with `--json` in the format of `schemas/ai-local-report.schema.json`.

| Command | What it does |
|---|---|
| `hardware` | Memory (MemTotal, MemAvailable), the processor and the features that decide inference speed (AVX2, FMA, AVX-512, AMX; Arm's dot product, SVE, i8mm), the DRM graphics cards with vendor, driver and own memory where the driver publishes it, the Vulkan devices from `vulkaninfo --summary` (a software device such as llvmpipe does not count), and the engine's backend libraries. Read-only: `/proc`, `/sys` and `vulkaninfo`. |
| `models` | The catalogue, with download size, memory needed at the context, whether it fits, whether it is downloaded, and the licence in plain words. `--catalogue-only` says nothing about downloads, for P@x's sandbox, which cannot see the person's data folder. |
| `recommend` | Every model that converses in a tier, with its reasons (section 10), and the model for conversation with P@x that fits best (rules in `src/fit.rs`): among the models that fit in the memory free now plus a reserve (the larger of 1 GiB and a tenth of the memory), converse and call tools, and run well, the largest; without one that runs well, the smallest that fits, said to be slow. Also a model for images and one for embeddings. `models` gives each model's tier too. |
| `check MODEL` | Whether one model fits: `TUR-AI-LOCAL-004` fits, `-006` not in the memory free now, `-005` too large for this computer. |
| `pull MODEL` | Downloads each file from the catalogue's URL (https only, redirects too) to `<name>.part`, counting and hashing; refuses a file whose announced or received size, or whose SHA-256, differs (`-009`, `-010`) and deletes it; writes `verified.json` only when every file matches. `--from-file` imports a file the person has, with the same checks. A model too large for this computer is not downloaded. A licence with use restrictions needs `--accept-licence`. |
| `verify MODEL` | Hashes the downloaded files again. |
| `remove MODEL` | Deletes the model's folder; refused while it is served. |
| `serve MODEL` | Refuses a non-loopback address without `--allow-network` (`-016`), a model not downloaded or changed (`-012`, `-025`), one that does not fit now (`-005`, `-006`), a second server (`-018`), a busy port (`-017`). Writes a random 256-bit access key to `$XDG_RUNTIME_DIR/tur-ai-local/api-key` (mode 0600) and starts `llama-server` with the model, `--host 127.0.0.1 --port 39281` (or a Unix socket), `--api-key-file`, `--ctx-size 8192`, `--parallel 1`, `--cache-ram 0`, `--metrics`, `--no-webui`, `--no-slots`, `--offline`, `--reasoning off` (`--reasoning on` lets a model think first) and, for an embeddings model, `--embedding --pooling …`, with its environment cleared. With `setpriv` installed the kernel stops the engine if the supervisor dies. It writes `state.json` once the engine answers `/health`, then reads the engine's own counters (`/metrics`) and stops it after `--idle-stop` seconds (default 600) without activity (`-020`), or when asked (`-021`). |
| `status` | Whether a model is served, the address and port, the key file (never the key), the engine's resident and peak memory, idle time and health. In P@x's sandbox the runtime folder is invisible and `status` says so (`-027`) instead of claiming that nothing runs. |
| `stop` | Asks the supervisor to stop and waits for it. |
| `ensure` | Everything a person's choice of a model needs, in one step (section 11): licence, memory, disk space, a resumable checked download from the publisher, the engine started; its progress as JSON lines. |
| `cancel` | Asks a running `ensure` to stop; what it downloaded is kept. |
| `codes` | The registry: `TUR-AI-LOCAL-001` to `-029`, in the existing AI family of `tur-core` (no new family was needed). `-028` not enough disk space, `-029` stopped on request. |

**The Settings switch.** "Let other devices on my network use local AI"
(Settings → AI → Local models) maps to `serve --allow-network --host
0.0.0.0`, or the address of one interface. It is off by default, holds for
the running session only and is shown while it is on. Without it
`tur-ai-local` refuses every address that is not loopback, `0.0.0.0` and `::`
included. The access key is required on every address.

**Models.** `services/tur-ai-local/catalogue/`: the first list of five
entries below, and the second list of section 10; all Apache-2.0, each
pinned to a commit of the publisher's own Hugging Face repository, with the
publisher's size and SHA-256.

| Id | Publisher | File | Size | Can do |
|---|---|---|---|---|
| `qwen3-0.6b` | Qwen | `Qwen3-0.6B-Q8_0.gguf` | 639 MB | chat, tools |
| `granite-4.0-1b` | IBM | `granite-4.0-1b-Q4_K_M.gguf` | 1.02 GB | chat, tools |
| `qwen3-vl-2b-instruct` | Qwen | `Qwen3VL-2B-Instruct-Q4_K_M.gguf` and `mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf` | 1.11 + 0.45 GB | chat, tools, image input |
| `qwen3-4b` | Qwen | `Qwen3-4B-Q4_K_M.gguf` | 2.50 GB | chat, tools |
| `qwen3-embedding-0.6b` | Qwen | `Qwen3-Embedding-0.6B-Q8_0.gguf` | 639 MB | embeddings |

How each licence was established, and the models left out and why, are in
the entries and in `services/tur-ai-local/catalogue/README.md`.

**P@x tools.** `services/tur-ai-local/agent-tools.json` and
`agent-tools.adapter.json`, at the component's root where P@x finds every
component's tools (pax-orchestrator.md, section 4.2): `ai_local_status`,
`ai_local_models` and `ai_local_recommend`, class 0, without arguments,
performed by `/usr/bin/tur-ai-local … --json`. Nothing that downloads,
starts, stops or opens the network is a tool. (They sat in `pax/` until P@x's
tests discovered tool sets instead of listing them.)

**P@x's provider.** The preset `local` of P@x's model list
(pax-providers.md, section 14.8): `http://127.0.0.1:39281/v1`, the key read
from `$XDG_RUNTIME_DIR/tur-ai-local/api-key` for each task (never kept in
the password store: it changes with every `serve`), the catalogue's chat
models that call tools, data terms "runs on this computer".

## 8. Deviations and open points

1. **The model catalogue is not in `catalogue/models/`.** The application
   catalogue's loader (`services/tur-catalogue`) refuses any subdirectory of
   `catalogue/` ("the catalogue is one flat directory of .toml files"), its
   tests load that directory, and the `pax` workflow copies it whole to
   `/usr/share/tur/catalogue`. A `catalogue/models/` directory would have
   broken the application catalogue. The entries live in
   `services/tur-ai-local/catalogue/` and install to
   `/usr/share/tur/ai-local/models/`. Moving them needs the application
   catalogue's loader to skip `models/` first.
2. **Fedora's llama-cpp build** (section 6.2): slow processor build, no
   Vulkan, ROCm required. To be raised with Fedora's maintainers.
3. **P@x through the `local` preset: done.** P@x has a Chat Completions
   family (`openai-chat`) and the preset `local`, with a key source that
   reads `tur-ai-local`'s key file. The workflow's `pax` job no longer runs a
   harness of its own: it runs `tur-pax run --provider local --model
   qwen3-4b` against `tur-ai-local` serving Qwen3 4B on llama.cpp's own
   build, with every component's tools and the read-only command tool in
   the real sandbox (`tests/pax/local_model.py`). Its read-only command tool
   no longer breaks llama-server's tool grammar: every shipped tool schema
   is held to llama.cpp's limit (pax-orchestrator.md, section 4.4).
   Results: `docs/reports/pax-5.md`.
4. **Status from P@x.** The sandbox shows neither the runtime folder nor the
   loopback network, so `ai_local_status` cannot see a running model. A D-Bus
   interface (`org.tur.AiLocal1`) is the planned answer.
5. **Battery and foreground** (ADR-0021, item 9) are not implemented in this
   slice. **Permissions per application** (item 8): the access key is the
   mechanism; giving it to an application is not built.
6. **The tiers' speeds** are measurements of the 4-processor CI runner
   scaled by a rule, or the rule alone (section 10), until the test PC's
   measurements replace them. A graphics card is counted only for an
   engine with the Vulkan backend, which Fedora 45's has not; nothing was
   measured on a graphics processor.
7. **The second list's memory needs** are estimates (section 10); only
   Ministral 3 3B is run by the local-ai workflow, and Gemma 4 E2B measured
   in its comparison job.

## 9. What the image holds

Since report [TUR-010](../reports/TUR-010.md) (issue #85) the image carries
local AI, as the engine decision of issue #74 asked
([image/README.md](../../image/README.md), "The local AI engine"):

- `/usr/bin/tur-ai-local` (a line in `image/programs.tsv`), the model
  catalogue at `/usr/share/tur/ai-local/models/*.toml` (`image/files.tsv`),
  and the P@x tool set at `/usr/share/tur/pax/tools/tur-ai-local/`;
- **the engine, built by TUR**: llama.cpp b9840, the version Fedora 45
  packages, compiled from its source in a build stage of the image with
  `GGML_BACKEND_DL=ON`, `GGML_CPU_ALL_VARIANTS=ON` (fourteen x86-64
  variants, the best loaded at start) and `GGML_VULKAN=ON`, without HIP/ROCm
  or CUDA, in `/usr/libexec/tur/llama.cpp/` with `/usr/bin/llama-server` a
  link to it. `tur-ai-local` follows the link to read the backends, so its
  recommendations count the processor variants and Vulkan. The proposal to
  Fedora (processor variants, Vulkan on, ROCm in a subpackage) is work for a
  person; when Fedora ships it, the stage can give way to the package;
- P@x's model list at `/usr/share/tur/pax/models.json`, so the `local`
  preset exists on the installed system;
- nothing more for `setpriv`, `vulkaninfo` and `df`: the image already has
  `util-linux`, `vulkan-tools` and `coreutils`;
- no model: models are downloaded at the person's request.

Not yet: a systemd user unit and `org.tur.AiLocal1`, so that `status` works
from P@x's sandbox (section 8, item 4).

The boot test asks `tur-ai-local check` about Qwen3 0.6B and Qwen3 4B on
the installed system and holds each to the verdict: a model that fits is
downloaded from its publisher, served on the image's engine and asked by
P@x through the `local` preset; one that does not is refused with
`TUR-AI-LOCAL-006` (by `serve`) or `-005` (by `pull`). The results are in
TUR-010.

## 10. The second list, and the tiers

Added on 13 November 2026 (issue #84): the owner asked for Gemma 4 E2B and
other models, offered by what the computer can run, for privacy.

| Id | Publisher, repository at the pinned revision | File(s) | Size | SHA-256 | Quantisation, context | Can do | Memory needed at 8 192 (estimate) |
|---|---|---|---|---|---|---|---|
| `gemma-4-e2b` | Google DeepMind, `google/gemma-4-E2B-it-qat-q4_0-gguf` @ `675cff42a74c774d6cb76f76d8eacb49b48c9b93` | `gemma-4-E2B_q4_0-it.gguf`, `gemma-4-E2B-it-mmproj.gguf` | 3 349 516 256 + 986 833 664 | `fa401b55…dec6634`, `021059cc…e6beb14` | Q4_0 (QAT), 131 072 | chat, tools, image input; thinking switchable | 7.41 GB |
| `gemma-4-e4b` | Google DeepMind, `google/gemma-4-E4B-it-qat-q4_0-gguf` @ `4b4a2c1d584be7264f87aac328a1bc739ce81b6c` | `gemma-4-E4B_q4_0-it.gguf`, `gemma-4-E4B-it-mmproj.gguf` | 5 154 941 280 + 991 552 256 | `676c3507…53fbaee`, `7498a37c…d65578` | Q4_0 (QAT), 131 072 | chat, tools, image input; thinking switchable | 10.11 GB |
| `ministral-3-3b` | Mistral AI, `mistralai/Ministral-3-3B-Instruct-2512-GGUF` @ `eb599d408350ea2bb60452cb86be7c7b2fc28227` | `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf`, `…-BF16-mmproj.gguf` | 2 147 023 008 + 841 501 856 | `9ed150d4…3fc5f8`, `70ca26e4…519ee0` | Q4_K_M, 262 144 | chat, tools, image input | 6.58 GB |
| `gemma-4-26b-a4b` | Google DeepMind, `google/gemma-4-26B-A4B-it-qat-q4_0-gguf` @ `d1c082be9cf3c8a514acf63b8761f4b41935842e` | `gemma-4-26B_q4_0-it.gguf`, `gemma-4-26B-it-mmproj.gguf` | 14 439 363 584 + 1 194 828 160 | `3eca3b8f…eca51d`, `a359953a…dd4f3b` | Q4_0 (QAT), 262 144; mixture of experts, 3.8B active | chat, tools, image input; thinking switchable | 23.59 GB |
| `qwen3-30b-a3b` | Qwen, `Qwen/Qwen3-30B-A3B-GGUF` @ `e4d4bafdfb96a411a163846265362aceb0b9c63a` | `Qwen3-30B-A3B-Q4_K_M.gguf` | 18 556 685 824 | `0d003f66…aa8f5` | Q4_K_M, 32 768; mixture of experts, 3.3B active | chat, tools; thinking switchable | 27.86 GB |

The full digests are in the entries. Sizes and digests are the
repositories' own metadata (the Hugging Face API's file list with LFS
SHA-256; the pinned `resolve` URLs answered with the same size and digest in
`x-linked-size` and `x-linked-etag`); nothing was downloaded to read them.

**Licences, read for this list.** Gemma 4 is under the **Apache License
2.0 alone**: the cards declare `apache-2.0` with the link
`ai.google.dev/gemma/docs/gemma_4_license`, which redirects to the Apache
2.0 text; Google's Gemma Terms of Use (last modified 2026-04-01) say they
apply to the models in their appendix and "For Gemma 4 terms, see the Gemma
4 license", so the Gemma Prohibited Use Policy, part of those terms, does not
apply to Gemma 4. Restrictions: none. Ministral 3 3B is Apache-2.0, and its
card adds under the licence: "You must not use this model in a manner that
infringes, misappropriates, or otherwise violates any third party's rights,
including intellectual property rights." TUR shows that sentence as a
restriction, so its download asks for acceptance (a decision the owner may
reverse: the sentence restates the law). Qwen3 30B A3B: the Apache-2.0 text,
byte for byte the file of Qwen3 4B. Left out, and why: the catalogue
README.

**Memory.** Estimates, not measurements: the files, the 16-bit context cache
at 8 192 tokens (Gemma 4's sliding-window layers keep only their window),
512 MiB for the engine and 300 MB for one image, all times 1.4: the ratio
llama.cpp's own build showed for Qwen3 4B in CI (its processor kernels keep a
repacked copy of the weights). The loader refuses an estimate below the
files plus the cache. A model that would exhaust the memory is still refused
by every command (`TUR-AI-LOCAL-005`, `-006`).

**Tiers** (`src/fit.rs`, `fit::tiers`). Every model that converses is put in
one of three tiers for this computer, each with its reasons in plain words:

| Tier | When |
|---|---|
| `runs-well`, "fits and runs well" | It fits in the memory (total, with the reserve), and its expected generation speed is 8 tokens a second or more: faster than a person reads |
| `slow`, "fits but will be slow" | It fits, and the speed is below that |
| `does-not-fit`, "does not fit" | It needs more than the total memory with the reserve |

`fits_now` says whether it also fits in the memory free now; the reasons
say "Close some programs first" when it does not.

**Speed.** Generation reads the active weights once per token, so it is
bandwidth over bytes: bytes per token are the model file, or for a mixture
of experts and Gemma's per-layer embeddings the catalogue's new
`active_weight_bytes` (file × active share the card states). Bandwidth:

| Device | Bytes a second | Source |
|---|---|---|
| Processor with AVX2/FMA (or Arm dot product) and an engine with processor variants | 9 GB × logical processors, at most 60 GB | CI run 43277339192: 14.85 tokens/s from 2.50 GB and 32.87 from 1.11 GB on 4 processors, about 36.5 GB/s |
| Processor without them, or Fedora 45's single build | 1 GB × logical processors, at most 16 GB | the same run: 1.61 and 3.72 tokens/s, about 4 GB/s |
| A graphics card with memory of its own | 150 GB, if the model's files and cache fit in that memory (only `amdgpu` reports it) | assumed, half of a mid-range card; not measured |
| A built-in graphics processor | 40 GB | assumed; not measured |

A catalogue measurement for the same kind of engine build is used instead
of the rule, scaled from the 4-processor runner to this processor
(`basis: measured`). A graphics processor counts only when the engine has
the Vulkan backend; Fedora 45's build has none, and the reason says so.

On the hardware fixtures of the tests (the 4 GB virtual machine, the
constructed 32 GB, 16-processor desktop with an 8 GB card and an engine
without Vulkan):

| Model | 4 GB VM, 2 processors | 32 GB desktop, 16 processors (28.7 GB free) |
|---|---|---|
| qwen3-0.6b | runs well, not now | runs well (measured, ×60/36) |
| qwen3-4b | does not fit | runs well, 24.7 tokens/s (measured) |
| ministral-3-3b | does not fit | runs well, 15.2 tokens/s (measured in this work: 9.11 on the 4-processor runner, ×60/36) |
| gemma-4-e2b | does not fit | runs well, 42.6 tokens/s (measured in this work: 25.55 on the 4-processor runner, ×60/36) |
| gemma-4-e4b | does not fit | runs well, 20.7 tokens/s |
| gemma-4-26b-a4b | does not fit | runs well, 27.5 tokens/s; **recommended** |
| qwen3-30b-a3b | does not fit | runs well, 29.9 tokens/s, but not in the memory free now |

## 11. `ensure`: a chosen model, ready

`tur-ai-local ensure MODEL [--json] [--accept-licence] [--context N]
[--no-serve] [--port P] [--idle-stop S] [--start-timeout S] [--reasoning
on|off|auto]` does, in order, and says each step as one JSON object a line
(`ensure-event`, `schemas/ai-local-report.schema.json`):

1. **Refuses before anything** a licence with restrictions not accepted
   (`-013`), a context longer than the model's, a model that can never fit
   this computer (`-005`).
2. **`plan`**: what is already there (whole files and partial `.part`
   files), what is left to download, and the free space of the disk of the
   models folder (`df --output=avail -B1`). Less free than what is left
   plus 1 GiB: refused (`-028`), nothing downloaded.
3. **The download**, from the catalogue's pinned `https` URL only
   (redirects too), going on from a partial file with an HTTP range request
   (the bytes there are hashed again first; a server that answers with the
   whole file is read from the start). **`progress`** every 1 % (or 1 MiB).
   A broken connection keeps the partial file for the next attempt (`-008`);
   a size or a SHA-256 that does not match deletes it (`-009`, `-010`).
   **`verified`** for each file, **`installed`** with the record of
   verification; **`already-present`** instead when the model was verified
   before.
4. **The engine** (unless `--no-serve`): a model already served with this
   context is **`ready`** at once; another is stopped first (**`stopping`**);
   the memory free now is checked (`-006`); **`starting`** runs
   `tur-ai-local serve` in a process group of its own, which outlives
   `ensure`; **`ready`** with its state once it answers.
5. **Stop**: `tur-ai-local cancel` writes a request in the runtime folder;
   `ensure` sees it between two pieces of the download or while the engine
   starts, stops (stopping the engine it started), keeps what it
   downloaded, and ends with **`cancelled`** (`-029`). A failure ends with
   **`failed`** and its code, which is also the first line of its standard
   error.

Nothing downloads unless a person asks: `ensure` is a command, and P@x's
service runs it only for `UseLocalModel`, after the person was told the
download size ([pax-service.md](pax-service.md), section 2.1).

**Tests.** `tests/ensure.rs`: the library against an HTTP server on the
test machine serving the tiny model of known digest (plain `http` to a
loopback address allowed for these tests only; no command offers it): a
whole download, events and record; already present, nothing fetched; a
connection cut at 20 bytes, then resumed with `range: bytes=20-`; a server
that answers a range with the whole file; a wrong digest, nothing kept; a
stop after the first piece, the partial file kept and finished next time;
too little disk, nothing asked of the server. The command with the stand-in
engine: a model present is served (`already-present`, `starting`, `ready`),
asked again it is `ready` at once; a restricted licence and a model too
large are refused before any download, with a `failed` event; `cancel`
leaves its request; every event follows the schema. The local-ai workflow's
`pax` job runs one real `ensure` of Ministral 3 3B from Mistral's
repository, then a P@x task through it.

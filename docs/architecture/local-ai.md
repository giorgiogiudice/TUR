# Local AI

**Status:** first slice built, 12 November 2026. Decision: [ADR-0021](../adr/0021-local-ai.md).
Sections 1 to 5 are the design. Section 6 records the engine decision and its
evidence, section 7 what is built (`services/tur-ai-local`), section 8 what
deviates from the design and what is open, and section 9 what the image needs.
Measurements: [report local-ai-1](../reports/local-ai-1.md).

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

Items 1 and 2 are done in their first slice (sections 6 and 7); item 3 is
proved in CI with P@x's existing adapters and a harness, without the preset
(section 8).

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
| `recommend` | The model for conversation with P@x that fits best (rules in `src/fit.rs`): it fits in the memory free now plus a reserve (the larger of 1 GiB and a tenth of the memory), converses and calls tools, and its file is within a speed budget set from the processor and the engine's build (an engine with one processor library and no variants, like Fedora 45's, gets the budget of a processor without the fast kernels: it answered 6 to 9 times more slowly in CI). Also a model for images and one for embeddings. |
| `check MODEL` | Whether one model fits: `TUR-AI-LOCAL-004` fits, `-006` not in the memory free now, `-005` too large for this computer. |
| `pull MODEL` | Downloads each file from the catalogue's URL (https only, redirects too) to `<name>.part`, counting and hashing; refuses a file whose announced or received size, or whose SHA-256, differs (`-009`, `-010`) and deletes it; writes `verified.json` only when every file matches. `--from-file` imports a file the person has, with the same checks. A model too large for this computer is not downloaded. A licence with use restrictions needs `--accept-licence`. |
| `verify MODEL` | Hashes the downloaded files again. |
| `remove MODEL` | Deletes the model's folder; refused while it is served. |
| `serve MODEL` | Refuses a non-loopback address without `--allow-network` (`-016`), a model not downloaded or changed (`-012`, `-025`), one that does not fit now (`-005`, `-006`), a second server (`-018`), a busy port (`-017`). Writes a random 256-bit access key to `$XDG_RUNTIME_DIR/tur-ai-local/api-key` (mode 0600) and starts `llama-server` with the model, `--host 127.0.0.1 --port 39281` (or a Unix socket), `--api-key-file`, `--ctx-size 8192`, `--parallel 1`, `--cache-ram 0`, `--metrics`, `--no-webui`, `--no-slots`, `--offline`, `--reasoning off` (`--reasoning on` lets a model think first) and, for an embeddings model, `--embedding --pooling …`, with its environment cleared. With `setpriv` installed the kernel stops the engine if the supervisor dies. It writes `state.json` once the engine answers `/health`, then reads the engine's own counters (`/metrics`) and stops it after `--idle-stop` seconds (default 600) without activity (`-020`), or when asked (`-021`). |
| `status` | Whether a model is served, the address and port, the key file (never the key), the engine's resident and peak memory, idle time and health. In P@x's sandbox the runtime folder is invisible and `status` says so (`-027`) instead of claiming that nothing runs. |
| `stop` | Asks the supervisor to stop and waits for it. |
| `codes` | The registry: `TUR-AI-LOCAL-001` to `-027`, in the existing AI family of `tur-core` (no new family was needed). |

**The Settings switch.** "Let other devices on my network use local AI"
(Settings → AI → Local models) maps to `serve --allow-network --host
0.0.0.0`, or the address of one interface. It is off by default, holds for
the running session only and is shown while it is on. Without it
`tur-ai-local` refuses every address that is not loopback, `0.0.0.0` and `::`
included. The access key is required on every address.

**Models.** `services/tur-ai-local/catalogue/`: five entries, all Apache-2.0,
each pinned to a commit of the publisher's own Hugging Face repository, with
the publisher's size and SHA-256.

| Id | Publisher | File | Size | Can do |
|---|---|---|---|---|
| `qwen3-0.6b` | Qwen | `Qwen3-0.6B-Q8_0.gguf` | 639 MB | chat, tools |
| `granite-4.0-1b` | IBM | `granite-4.0-1b-Q4_K_M.gguf` | 1.02 GB | chat, tools |
| `qwen3-vl-2b-instruct` | Qwen | `Qwen3VL-2B-Instruct-Q4_K_M.gguf` and `mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf` | 1.11 + 0.45 GB | chat, tools, image input |
| `qwen3-4b` | Qwen | `Qwen3-4B-Q4_K_M.gguf` | 2.50 GB | chat, tools |
| `qwen3-embedding-0.6b` | Qwen | `Qwen3-Embedding-0.6B-Q8_0.gguf` | 639 MB | embeddings |

How each licence was established, and the models left out and why, are in
the entries and in `services/tur-ai-local/catalogue/README.md`.

**P@x tools.** `services/tur-ai-local/pax/`: `ai_local_status`,
`ai_local_models` and `ai_local_recommend`, class 0, without arguments,
performed by `/usr/bin/tur-ai-local … --json`. Nothing that downloads,
starts, stops or opens the network is a tool. Like `tur-update`'s, the files
sit one directory below the component, because `tests/pax/e2e.py` and
tur-pax's own tests list the components whose tools sit directly in their
directory; P@x loads them by default once those lists are extended.

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
3. **P@x has no `local` preset and no Chat Completions adapter yet.** The
   workflow runs P@x's existing OpenAI (Responses) and Anthropic (Messages)
   adapters against `llama-server`'s compatible endpoints, and a minimal
   Chat Completions harness of its own. P@x will also need a key source that
   reads the key file.
4. **Status from P@x.** The sandbox shows neither the runtime folder nor the
   loopback network, so `ai_local_status` cannot see a running model. A D-Bus
   interface (`org.tur.AiLocal1`) is the planned answer.
5. **Battery and foreground** (ADR-0021, item 9) are not implemented in this
   slice. **Permissions per application** (item 8): the access key is the
   mechanism; giving it to an application is not built.
6. **The recommendation's speed budget** is a rule of thumb until the test
   PC's measurements replace it; graphics processors are reported, not used.

## 9. What the image needs

Nothing of this change is in the image (`image/` was not touched). To ship it:

- `/usr/bin/tur-ai-local` (a line in `image/programs.tsv`) and the model
  catalogue at `/usr/share/tur/ai-local/models/*.toml`;
- the P@x tool set at `/usr/share/tur/pax/tools/tur-ai-local/`;
- the engine, pinned: Fedora's `llama-cpp-b9840-2.fc45` as it is (slow on the
  processor, no Vulkan, and ROCm's hipBLAS and rocBLAS with it; installed
  size in the report), or the same llama.cpp version built with
  `GGML_BACKEND_DL=ON`, `GGML_CPU_ALL_VARIANTS=ON`, `GGML_VULKAN=ON` and
  without HIP, preferably as a change to Fedora's spec;
- nothing more for `setpriv` and `vulkaninfo`: the image already has
  `util-linux` 2.42.4 and `vulkan-tools` 1.4.350.0
  ([image-licences.csv](../dependencies/image-licences.csv));
- a systemd user unit, started on request and not at login, once the D-Bus
  interface exists;
- no model: models are downloaded at the person's request.

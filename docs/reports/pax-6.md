# pax-6 — Models: local models by hardware with `ensure`, model selection and generation settings for every provider

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-6 (issue #84)
Subsystem:        AI control layer: P@x's model selection and generation
                  settings (tur-pax, tur-pax-providers); local AI's
                  catalogue, tiers and ensure (tur-ai-local)
Build ID:         not applicable: this task builds no image of its own
Source revision:  pull request 87 (feat/pax-models); CI runs below

SUMMARY
- What changed: tur-ai-local gains five models (Gemma 4 E2B, E4B and
  26B A4B, Ministral 3 3B Instruct, Qwen3 30B A3B), a tier for every model
  on this computer (runs well, slow, does not fit) with plain reasons from
  measured or estimated speed by engine build, processor and graphics card,
  and `ensure`/`cancel`: one call that checks the licence, the memory and
  the disk, downloads from the publisher with resume, checks size and
  SHA-256, puts the model in place and starts the engine, with its progress
  as JSON lines. The provider layer gains generation settings (temperature,
  top-p, top-k, stop, effort, thinking budget) written in each family's
  own fields and refused where a family has none, and the model lists of
  the providers. P@x gains per-model declarations of what each model
  accepts (ranges, defaults), the person's choice per model kept in its
  settings and checked against them, six methods and one signal on
  org.tur.Agent1 (ListProviderModels, GetGenerationSettings,
  SetGenerationSettings, ListLocalModels, UseLocalModel, CancelLocalModel;
  LocalModelProgress) and `tur-pax models`.
- Why it changed: the owner asked for Gemma 4 E2B and other local models
  offered by hardware, downloaded and put in place automatically when the
  person chooses one, and for model selection with temperature, effort and
  every knob for every provider.
- What it does not show: any cloud provider answering (none was called);
  any graphics processor (none in CI); Gemma 4 26B A4B, Gemma 4 E4B and
  Qwen3 30B A3B running (too large for the runner or not measured); the
  image with any of it (image/ was not edited).

FILES
- Added: services/tur-ai-local/catalogue/{gemma-4-e2b,gemma-4-e4b,
  gemma-4-26b-a4b,ministral-3-3b,qwen3-30b-a3b}.toml;
  services/tur-ai-local/src/ensure.rs; services/tur-ai-local/tests/ensure.rs;
  services/tur-pax-providers/src/{generation,listing}.rs;
  services/tur-pax-providers/tests/listing.rs;
  services/tur-pax/src/{tuning,local}.rs; tests/pax/stub-tur-ai-local;
  docs/reports/pax-6.md
- Modified: services/tur-ai-local/src/{catalogue,cli,codes,download,fit,
  lib}.rs, tests/{catalogue,cli}.rs, catalogue/README.md;
  services/tur-pax-providers/src/{anthropic,openai,google,openai_chat,
  config,http,model,probe,lib}.rs, tests/{conformance,config_and_schema}.rs,
  tests/data/full-openai-chat.json; services/tur-pax/src/{settings,service,
  dbus,daemon,cli,faults,lib}.rs, interfaces/org.tur.Agent1.xml,
  data/models.json, tests/{service,agent1_interface}.rs;
  schemas/{pax-provider-config,pax-models,pax-settings,local-model,
  ai-local-report}.schema.json; tests/pax/{service_e2e,local_model}.py;
  .github/workflows/local-ai.yml; docs/architecture/{pax-providers,
  pax-service,local-ai}.md; docs/dependencies/upstream-projects.md

BUILD
- Command: the workflows "rust" (clippy -D warnings, cargo test, every
  component), "pax", "local-ai", "image", "boot"
- Result: see "Runs"

TESTS
- Unit and integration ("rust"): see "Runs"
- Service over D-Bus ("pax", service scenarios): see "Runs"
- Real ensure and a P@x task through it ("local-ai", job pax,
  ministral-3-3b): see "The real ensure"
- Against a cloud provider: NOT RUN (not allowed for this task)
- Hardware, graphics processor: NOT RUN

RECOVERY
- Rollback: revert the merge. Settings files with `generation`, model lists
  with `settings` or `reasoning_style`, and catalogue entries with
  `active_weight_bytes` cannot be read by the old build and must be
  reverted with it. Downloaded models stay usable by the old `serve`.
```

## Runs

| Workflow | Run | Commit | Result |
|---|---|---|---|
| `local-ai` | 43341975820 | `68f720e` | Job "ministral-3-3b, ensure": pass, the real download (below). The other jobs were cancelled by the next push |
| `local-ai` | 43346627935 | `46635e8` | "ministral-3-3b, ensure": pass (from the cache: `already-present`, `starting`, `ready`; the task completed). "Engine comparison": pass, Gemma 4 E2B measured (below) with the first list |
| `pax` | 43344490864 | `af32da3` | Pass: e2e 393 checks; session service 140 checks (103 before), the scenarios `models-and-settings` and `local-model` included |
| `rust` | 43344490832 | `af32da3` | Fail: clippy (a needless dereference in `fit.rs`), a test variable shadowing a helper, rustfmt. tur-pax: every test passed (239 unit, 24 in `service`); only formatting |
| `rust` | 43346130236 | `4ac4cab` | Fail: one test of `ensure` gave the tiny model a context it does not have (exit 2 instead of 1); the providers' `check` test expected "model discovery: no" |
| `rust` | 43346627945 | `46635e8` | Pass: tur-pax, tur-ai-local, tur-pax-providers and every other component |
| `image`, `boot` | 43341975862, 43341975821 and later | every commit | Fail before building anything of TUR: `quay.io/fedora/fedora-kinoite@sha256:f857f6c5…` (the pin in `image/base.lock`) is "manifest unknown" at quay.io; see "Limits" |
| all | see the pull request | the head commit | the final state is recorded in the pull request's description |

**Gemma 4 E2B on llama.cpp b9840** (run 43346627935, job "Engine
comparison", the table of `local_ai_ci.py`, llama.cpp's own CPU build on
the 4-processor runner): downloaded from Google's repository and checked in
54.1 s (4.34 GB), loaded in 2.5 s; 128 tokens at 25.55 tokens a second, the
prompt at 125.0; a well-formed tool call, streamed too, and an answer that
used the tool result; the colour of a red image named (image input through
the projector); peak memory 5.68 GB against the estimate of 7.41 GB. So the
pinned engine version runs Gemma 4. The measurement is in its catalogue
entry, and its tier is now measured.

## The new models

All read on 2026-11-13 from the publishers' own Hugging Face repositories:
revision, file list, sizes and LFS SHA-256 from the repository metadata
(the pinned `resolve` URLs answered `x-linked-size` and `x-linked-etag`
with the same values). Nothing was downloaded on the development machine.

| Id | Repository @ revision | File: size, SHA-256 | Quantisation, context | Can do | Licence |
|---|---|---|---|---|---|
| `gemma-4-e2b` | `google/gemma-4-E2B-it-qat-q4_0-gguf` @ `675cff42a74c774d6cb76f76d8eacb49b48c9b93` | `gemma-4-E2B_q4_0-it.gguf` 3 349 516 256, `fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634`; `gemma-4-E2B-it-mmproj.gguf` 986 833 664, `021059cce659fe7f9170d5599761d7bbaf644b798dab9503aca30dc43e6beb14` | Q4_0 (QAT), 131 072 | chat, tools, image input, thinking switchable | Apache-2.0, no restriction |
| `gemma-4-e4b` | `google/gemma-4-E4B-it-qat-q4_0-gguf` @ `4b4a2c1d584be7264f87aac328a1bc739ce81b6c` | `gemma-4-E4B_q4_0-it.gguf` 5 154 941 280, `676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee`; `gemma-4-E4B-it-mmproj.gguf` 991 552 256, `7498a37cb619e55f2fcf87eb931f56e99389ed6d432e4c5c66110694c0d65578` | Q4_0 (QAT), 131 072 | as E2B | Apache-2.0, no restriction |
| `ministral-3-3b` | `mistralai/Ministral-3-3B-Instruct-2512-GGUF` @ `eb599d408350ea2bb60452cb86be7c7b2fc28227` | `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf` 2 147 023 008, `9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8`; `Ministral-3-3B-Instruct-2512-BF16-mmproj.gguf` 841 501 856, `70ca26e41ecea9da997e2887462ad68b848fbf64da011ff4107bbe7353519ee0` | Q4_K_M, 262 144 | chat, tools, image input | Apache-2.0; the card's added sentence shown as a restriction |
| `gemma-4-26b-a4b` | `google/gemma-4-26B-A4B-it-qat-q4_0-gguf` @ `d1c082be9cf3c8a514acf63b8761f4b41935842e` | `gemma-4-26B_q4_0-it.gguf` 14 439 363 584, `3eca3b8f6d7baf218a7dd6bba5fb59a56ee25fe2d567b6f5f589b4f697eca51d`; `gemma-4-26B-it-mmproj.gguf` 1 194 828 160, `a359953a076b877db30c31dbbb4c6d93b4a6e017ee5db5784247e4d4c0dd4f3b` | Q4_0 (QAT), 262 144; 3.8B active of 25.2B | as E2B | Apache-2.0, no restriction |
| `qwen3-30b-a3b` | `Qwen/Qwen3-30B-A3B-GGUF` @ `e4d4bafdfb96a411a163846265362aceb0b9c63a` | `Qwen3-30B-A3B-Q4_K_M.gguf` 18 556 685 824, `0d003f6662faee786ed5da3e31b29c978de5ae5d275c8794c606a7f3c01aa8f5` | Q4_K_M, 32 768; 3.3B active of 30.5B | chat, tools, thinking switchable | Apache-2.0, no restriction |

**Gemma 4's licence, read.** The cards declare `apache-2.0` with
`license_link: https://ai.google.dev/gemma/docs/gemma_4_license`; that
address redirects to `https://ai.google.dev/gemma/apache_2`, the Apache
License 2.0 text. The Gemma Terms of Use (`ai.google.dev/gemma/terms`, last
modified 2026-04-01) begin: "The terms below apply to Gemma models listed in
the Appendix at bottom of this page. For Gemma 4 terms, see the Gemma 4
license." The Gemma Prohibited Use Policy (last modified 2024-02-21) is part
of those terms. So Gemma 4 is Apache-2.0 without a prohibited-use policy,
and the catalogue says "none" for restrictions.

**Ministral 3 3B's licence, read.** Apache-2.0 in the card header and under
"License", followed by: "You must not use this model in a manner that
infringes, misappropriates, or otherwise violates any third party's rights,
including intellectual property rights." Shown as a restriction; its
download needs acceptance.

Memory needs are estimates (local-ai.md, section 10): 7.41, 10.11, 6.58,
23.59 and 27.86 GB at 8 192 tokens.

**Left out**, all with licences that were confirmed or not reviewed:
LiquidAI LFM (licence `lfm1.0` not reviewed: excluded for that reason);
Gemma 4 12B and 31B, Qwen3 8B/14B/32B, Ministral 3 8B/14B and the
Reasoning variants (Apache-2.0, left out to keep the list short); Phi (no
small official GGUF); SmolLM2 (superseded, no tool-calling claim).

## The tiers

Rules in `services/tur-ai-local/src/fit.rs` and local-ai.md, section 10:
**does not fit** when the model needs more than the total memory with the
reserve; otherwise **runs well** at 8 tokens a second or more and **slow**
below, from bandwidth over the bytes read per token. Processor: 9 GB/s per
logical processor up to 60 GB/s with the fast kernels and an engine with
processor variants, 1 GB/s up to 16 GB/s without (from local-ai-1's
measurements); a graphics card with its own memory 150 GB/s and a built-in
one 40 GB/s (assumed, not measured), only with a Vulkan engine and room in
the card's memory. A catalogue measurement for the same kind of build is
scaled to this processor and used instead (`basis: measured`).

On the tests' machines (engine without Vulkan; the desktop has an 8 GB AMD
card, which therefore does not count):

| Model | 4 GB VM, 2 processors (AVX2) | 16 GB, 16 processors (unit test) | 32 GB desktop, 16 processors, 28.7 GB free |
|---|---|---|---|
| qwen3-0.6b | runs well, not now | runs well (measured) | runs well (measured) |
| granite-4.0-1b | does not fit | runs well (measured) | runs well (measured) |
| qwen3-vl-2b-instruct | does not fit | runs well (measured) | runs well (measured) |
| qwen3-4b | does not fit | runs well, about 24.7 tokens/s (measured) | runs well (measured) |
| ministral-3-3b | does not fit | runs well, 15.2 (measured here) | runs well, 15.2 (measured) |
| gemma-4-e2b | does not fit | runs well, 42.6 (measured here) | runs well, 42.6 (measured) |
| gemma-4-e4b | does not fit | runs well, 20.7; **recommended** | runs well, 20.7 |
| gemma-4-26b-a4b | does not fit | does not fit | runs well, 27.5; **recommended** |
| qwen3-30b-a3b | does not fit | does not fit | runs well, 29.9, not in the memory free now |

With Fedora 45's single build on 2 processors every model is slow
(Qwen3 4B 0.8 tokens/s, measured and scaled); with a Vulkan engine and the
8 GB card, Gemma 4 E2B and E4B run on the card (`graphics-card`). These are
`fit::tests` and `tests/cli.rs::the_recommendation_follows_memory_and_processor`.
The measurement of Ministral 3 3B in this work (9.11 tokens/s on the
4-processor runner) is about half of what the rule estimates for it (16.8),
because it was read with a 2 684-token prompt: the rule is optimistic for
long prompts, which P@x sends.

## Generation settings per family

The table of pax-providers.md, section 15.1, with its sources (read on
2026-11-13; Anthropic from the Claude API reference bundled with the
development tooling, cached 2026-11-03, which cites the platform pages):

| Setting | Anthropic Messages | OpenAI Responses | Google Interactions | Chat Completions |
|---|---|---|---|---|
| temperature, top-p | refused (API `2026-10-21` rejects them) | `temperature`, `top_p`; a model declares them only with effort `none` | refused (not in `generation_config`; ignored by Gemini 3.6+) | `temperature`, `top_p` |
| top-k | refused | refused (no field) | refused | `top_k` |
| stop | `stop_sequences` | refused (no field) | `generation_config.stop_sequences` | `stop` |
| effort | `output_config.effort` `low`…`max`; `none` → `thinking: {type: disabled}` | `reasoning.effort` `none`…`max` | `generation_config.thinking_level` `minimal`…`xhigh` | `reasoning_effort`, or llama.cpp's `chat_template_kwargs.enable_thinking` |
| thinking budget | refused (removed on current models) | refused | refused (`thinking_budget` not in the reference; cannot be combined with a level) | `thinking_budget_tokens`, llama.cpp only, not offered by a preset |
| output limit | `max_tokens` | `max_output_tokens` | `generation_config.max_output_tokens` | `max_tokens` |
| context (local) | – | – | – | `tur-ai-local ensure --context` |

Thinking tokens: counted as output in every family; `openai-chat` takes the
largest of `completion_tokens`, `total_tokens − prompt_tokens` and the
reasoning tokens. Tested per family in `tests/conformance.rs`
(`generation_settings_reach_the_wire_as_the_family_takes_them`,
`with_nothing_set_no_generation_field_is_sent`), unit tests of
`generation.rs`, `openai_chat.rs`, `tuning.rs`, `settings.rs`, and
`tests/service.rs::generation_settings_are_checked_kept_and_sent_in_the_family_s_fields`.

## The real `ensure` and P@x through it

**Run 43341975820** (commit `68f720e`), local-ai job "P@x through the local
preset (ministral-3-3b, ensure)", GitHub runner (AMD EPYC 7763, 4 logical
processors, 16.8 GB, no graphics processor), llama.cpp's own b9840 CPU
build:

`tur-ai-local ensure ministral-3-3b --json --accept-licence --reasoning auto
--idle-stop 3600 --start-timeout 900 --engine <llama-server>` printed 108
events: `plan` ("Ministral 3 3B Instruct downloads 3.0 GB from
https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF.",
2 988 524 864 bytes to download, 108 216 127 488 bytes free), 103
`progress`, `verified` for each of the two files (size and SHA-256 as the
catalogue gives), `installed` (`TUR-AI-LOCAL-023`), `starting`, `ready`
(`TUR-AI-LOCAL-026`, http://127.0.0.1:39281). 27 s from start to ready,
download included. Then `tur-ai-local verify` hashed both files again:
pass. The engine's resident memory when ready: 5.40 GB (the estimate is
6.58 GB). The model was cached (`local-ai-model-ministral-3-3b-<sha256>`);
the next run (43344490859) found it there: `already-present`, `starting`,
`ready`.

**The P@x task through it**: `tur-pax run "<two-tools scenario>" --provider
local --model ministral-3-3b` (the preset's defaults sent: temperature 0.05,
no thinking switch for this model, context 8 192), the key from
tur-ai-local's file, every component's tools and the read-only command tool
in the real sandbox. Completed in 159.6 s, 4 requests, 12 548 tokens in and
254 out: the model called `device_list_problems`, `catalogue_search` and
`catalogue_route` (all class 0, allowed, run) and answered with the
runner's firmware finding and Discord's Flatpak route. Every required check
of `tests/pax/local_model.py` passed (provider `local`, family
`openai-chat`, nothing above class 1 decided or performed, no grammar error,
the key in no file or output). llama-server read the first request at 34.8
tokens a second (2 684 tokens in 77.2 s) and generated at 9.1. The second
run's task: completed in 190.4 s, 2 requests, one tool call.

**What it does not show**: a download resumed after a real interruption (the
library tests do that against a local server), a cancel of a real download,
a model other than Ministral 3 3B, the Fedora engine with these models.

## Breaking changes

1. `Request` has `generation`; `ProviderConfig` has `generation`;
   `ProviderEntry` has `reasoning_style` and `settings`; `ModelEntry` has
   `settings`; `Settings` has `generation`; `Paths` has `ai_local` and
   `listing`; `Model` (tur-ai-local) has `active_weight_bytes`;
   `Recommendation` has `engine_vulkan` and `tiers` and is no longer `Eq`;
   `fit::recommend` takes an `EngineBuild`; `DownloadError` has
   `Cancelled`. Struct literals and exhaustive matches must name them.
2. The `local` preset sends generation defaults with every request
   (`enable_thinking: false`, and the publishers' recommended sampling for
   Qwen3, Gemma 4 and Ministral 3); before, it sent none.
3. `recommend` chooses by tier: on the 32 GB test desktop it now names
   Gemma 4 26B A4B, not Qwen3 4B.
4. All four adapters declare `model_discovery: yes`.
5. org.tur.Agent1 has six more methods and one more signal; `GetStatus` has
   `generation` and `local_model`. The interface version stays 1 (additions
   only).
6. `openai-chat` output tokens can be higher than `completion_tokens` when a
   service reports reasoning apart from it.
7. The local-ai workflow measures only the first list and Gemma 4 E2B in
   its tables; its `pax` job is a matrix (artifacts
   `local-ai-pax-evidence-<model>`).

## Decisions the owner may want to reverse

1. **Ministral 3 3B's card sentence is shown as a licence restriction**, so
   its download asks for acceptance; it restates the law.
2. **Effort is the one reasoning knob**, mapped per family; a numeric
   thinking budget is offered nowhere by default (every current API made it
   legacy or removed it).
3. **Temperature, top-p and top-k are refused for Anthropic and Google**
   rather than sent and ignored.
4. **A model without a declaration takes only an output limit.**
5. **Local models think only when asked** (`effort` default `none`), and are
   started with `--reasoning auto` so the request decides.
6. **Stop-sequence limits of 4 where the provider documents none**
   (Anthropic, Google, Mistral), 8 for llama.cpp.
7. **Tiers' threshold: 8 tokens a second**; memory estimates × 1.4.
8. **The download size must be named by the caller** of UseLocalModel (the
   proof the person was told), not confirmed on a trusted screen, which
   does not exist.
9. **The provider's own model list is kept a day** and asked only on
   request.

## Limits

- **No cloud provider was called.** Every family's fields are the
  documentation's (pax-providers.md, 15.5 lists what was not verified).
- **Gemma 4** was not run by P@x; E2B is measured only by the local-ai
  comparison job's table (llama.cpp b9840 own build), see "Runs". E4B,
  26B A4B and Qwen3 30B A3B were not run anywhere: their memory needs and
  speeds are estimates.
- **No graphics processor** anywhere; the graphics-card tier is a rule.
  The engine TUR would install (Fedora 45's) has no Vulkan backend, and
  `tur-ai-local serve` passes no `--n-gpu-layers` (current llama.cpp
  offloads automatically with `-ngl auto`; whether b9840 does was not
  checked).
- **The tiers' rule overestimates** speed for long prompts (above).
- **`tur-ai-local ensure` cancel** is a file the command polls between
  pieces; a stalled connection is cut by the 120-second read timeout, not
  by the cancel. The service ends the program after 20 s.
- **`free_space` uses `df`**; without it the check is skipped and the plan
  says `free_bytes: null`.
- **Stop-sequence limits** of Anthropic and Google are TUR's, not the
  providers'.
- **OpenRouter, Together and Ollama** declare no settings: only the output
  limit can be set for their models.
- **The `image` and `boot` workflows fail on this pull request for a reason
  outside it**: the pinned base image digest
  `quay.io/fedora/fedora-kinoite@sha256:f857f6c5…` (`image/base.lock`) is
  gone from quay.io ("manifest unknown"); the same workflows passed on main
  hours earlier. `image/**` is not this change's to edit.

## Follow-ups

- Measure the second list on the test PC, with and without its graphics
  card; replace the estimates and the bandwidth assumptions.
- A Vulkan build of the engine for the image (local-ai.md, section 9) so
  that the graphics-card tier means anything on TUR.
- The settings page (desktop) for model choice, settings and local models,
  on the new methods.
- Read the parameter pages of OpenRouter, Together and Ollama and declare
  their models' settings.
- Check against a real provider, with `pax-live`, that each family accepts
  the fields of section 15.1.

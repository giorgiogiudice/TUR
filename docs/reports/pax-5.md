# pax-5 — P@x integration: discovered tool sets, grammar-safe schemas, the `local` preset

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-5
Subsystem:        AI control layer: P@x's tool sets as every component ships
                  them, tool schemas a local model server accepts, P@x with
                  TUR's own local AI (tur-ai-local)
Build ID:         not applicable: this task builds no image of its own (the
                  image and boot workflows ran on the pull request)
Source revision:  pull request 79 (fix/pax-integration-2); first code commit
                  b996788, CI runs below

SUMMARY
- What changed: P@x's tests (services/tur-pax/tests/tools.rs,
  tests/pax/e2e.py) and the pax workflow no longer list components or
  tool names: they discover every tool set by one rule
  (services/<component>/agent-tools.json, the adapters beside it) and hold
  invariants over the whole set. tur-apps, tur-ai-local and
  tur-diagnostics' report tools move to their components' roots; tur-update
  does not (see "Deviations"). Every shipped tool schema is held to
  llama.cpp's grammar limit, found in its source at b9840; five bounds were
  lowered. P@x gains the preset `local` for tur-ai-local, with a key source
  that reads the key file tur-ai-local writes, `tur-pax run --provider`,
  `--no-key`, and the provider id in the audit trail. The local-ai
  workflow runs P@x itself through the preset against tur-ai-local serving
  Qwen3 4B, with the read-only command tool on.
- Why it changed: issues #74, #75, #67, #48, #72: components had to park
  their tools because P@x's tests hard-coded every component and tool
  name; the command tool made every request to llama-server fail
  (TUR-AI-PROVIDER-004); P@x could not reach tur-ai-local (port, key).
- What it does not show: the command tool being chosen by the local model
  (it was offered and accepted by the engine's grammar; Qwen3 4B did not
  call it in the three tasks); any other local model; the image with
  tur-apps or tur-ai-local in it (image/ was not edited).

FILES
- Added: tests/pax/local_model.py; docs/reports/pax-5.md
- Moved: services/tur-apps/pax/agent-tools*.json and
  services/tur-ai-local/pax/agent-tools*.json to the components' roots;
  services/tur-diagnostics/agent-tools.next*.json over
  agent-tools*.json
- Modified: services/tur-pax/{src/{registry,settings,service,textui,
  orchestrator}.rs, data/models.json, tests/{tools,run_cli,service,
  orchestrator,common/mod}.rs}; services/tur-pax-providers/src/{secret,
  lib}.rs; services/tur-{apps,ai-local,diagnostics}/tests/agent_tools.rs;
  services/tur-apps/tests/interfaces.rs (one path);
  services/tur-diagnostics/agent-tools.json (description 4000 -> 1999);
  services/tur-runtime/agent-tools.json (path 4096 -> 1999);
  schemas/pax-models.schema.json; tests/pax/e2e.py;
  .github/workflows/{pax,local-ai}.yml; docs/architecture/{pax-orchestrator,
  pax-providers,pax-service,local-ai}.md
- Removed: services/tur-diagnostics/agent-tools.next.json,
  agent-tools.next.adapter.json (now the shipped files)

BUILD
- Command: the workflows "rust" (clippy -D warnings, cargo test, every
  component), "pax", "image", "boot", "local-ai"
- Result: see "Runs"
- Warnings: none from the compiler or clippy; rustfmt corrections from CI
  applied

TESTS
- Unit and integration ("rust"): see "Runs"
- End to end, scripted provider ("pax"): pass, 393 checks in 19 scenarios
  and the tool-set checks, 0 failed; session service 103 checks
- On the installed system ("boot"): pass
- Against a real local model ("local-ai", job pax): pass; what the model
  did is in "P@x against Qwen3 4B"
- Against a cloud provider: NOT RUN (not allowed for this task)
- Hardware, graphics processor: NOT RUN

DIAGNOSTICS
- Log paths: the runs below; artifacts "pax-evidence",
  "local-ai-pax-evidence" (transcripts, audit trails, llama-server's log)
- Relevant error IDs: TUR-AI-PROVIDER-012 (the key file is missing, or not
  the person's alone); TUR-AI-PROVIDER-004 (what llama-server returned for
  a schema over its grammar limit, before this change)

RECOVERY
- Rollback procedure: revert the merge. The settings and model list gain
  one optional field (`runtime_key_file`); a model list that uses it cannot
  be read by the old build and must be reverted with it.

KNOWN LIMITATIONS
- Listed under "Limits" below.

NEXT SAFE ACTION
- The image lines below; then move tur-update's tools to its root together
  with the two counts in tests/image/smoke.sh and tests/boot/report.py.
```

## Runs

| Workflow | Run | Commit | Result |
|---|---|---|---|
| `pax` | 43318287134 | `efe12d4` | Pass. `tests/pax/e2e.py`: 393 checks, 0 failed. "tool sets" (7 checks) discovered `tur-ai-local, tur-apps, tur-catalogue, tur-device-manager, tur-diagnostics, tur-runtime, tur-storage`, all installed byte for byte and loaded by `tur-pax manifest validate`; "controls" checked the 7 programs the adapters name, built from their components by discovery. Session service 103 checks |
| `local-ai` | 43318287048 | `efe12d4` | Pass: jobs `fedora`, `compare` (unchanged) and `pax` (new) |
| `image` | 43318287495 | `efe12d4` | Pass |
| `boot` | 43318287103 | `efe12d4` | Pass (the guest's e2e with the discovering `tool_set` and the class-based "everywhere" check) |
| `apps`, `windows-runtime`, `reports` | 43318287491, 43318287023, 43318287127 | `efe12d4` | Pass |
| `rust` | 43318287392 | `efe12d4` | Fail, two causes: rustfmt in `tur-diagnostics`, `tur-pax-providers` and `tur-pax` (applied from CI's patch), and `tools::tool_files_live_only_at_the_root_of_a_component` and `…every_shipped_tool_schema_fits…` never finished: the walk of `services/` followed the device fixtures' links, which lead in circles (fixed: links are not followed). Every other test passed: tur-pax 228 unit, `run_cli` 11 (2 new), `service` 20 (1 new), `orchestrator` 41, 9 of 11 in `tools`; tur-pax-providers 113 unit (1 new); tur-diagnostics, tur-apps, tur-ai-local, tur-runtime, tur-update pass |
| all | see the pull request | the head commit | the final state is recorded in the pull request's description |

## What is built, and what proves it

| Part | Built | Proved by |
|---|---|---|
| One rule for tool sets, discovered | `read_tool_sets` (unchanged) used by the tests on `services/` | `tools::the_source_tree_is_read_as_a_directory_of_tool_sets`, `…tool_files_live_only_at_the_root_of_a_component`; `e2e.py` "tool sets"; `pax` workflow "Discover the tool sets" |
| Invariants over the whole set | `tools.rs` | `…every_tool_has_a_class_and_every_offered_tool_only_observes` (offered ⇒ class 0; unavailable = exactly the tools above class 0), `…no_adapter_sends_or_changes_anything_unless_its_class_says_so` (46 words; held to P@x's test service, whose class 2/3 tools pass only because of their class), `…every_shipped_program_and_path_is_in_the_system_view…`, `…the_shipped_adapters_use_only_what_the_published_format_describes`; `e2e.py` "everywhere": every performed action is class ≤ 1 by the manifest that declares it (no list of names) |
| Per-tool behaviour kept, by name | `tools.rs` | `…every_shipped_adapter_builds_the_command_line_of_its_program`, `…a_value_from_the_model_cannot_become_an_option…`, `…a_directory_of_tool_sets_is_read_component_by_component` (now reads the components' files rather than constants) |
| Parked sets moved | renames; component tests' paths | `tur-apps`, `tur-ai-local` (`where_the_tool_set_is_and_is_not` rewritten), `tur-diagnostics` (one pair, the report tools included) `tests/agent_tools.rs` |
| Grammar limit | five bounds lowered to 1999 | `tools::the_grammar_check_knows_llama_cpp_s_limits`, `…every_shipped_tool_schema_fits_llama_cpp_s_tool_grammar` (every manifest wherever it is, P@x's four built-in tools, the test fixture); `local-ai` pax job: "llama-server reported no grammar error for P@x's tools" with 19 and 20 tools offered, the command tool among them |
| `FileKeySource` | `tur-pax-providers/src/secret.rs` | unit `the_file_source_reads_a_key_only_its_owner_can_read` (0600 read; 0640, 0604, a link, a bad key, an empty and an oversized file refused, each naming the file and never the key) |
| Preset `local`, `runtime_key_file` | `data/models.json`; `ProviderEntry`; `settings::runtime_key`; `timeouts_for` | `settings::tests::the_shipped_list_loads_with_its_presets`, `…the_local_preset_offers_the_catalogue_s_chat_models_that_call_tools` (reads `tur-ai-local/catalogue/*.toml`), `…the_key_of_the_local_preset_is_read_from_tur_ai_local_s_file_only` (consent text: "stays on this computer: no company receives it"; refused entries) |
| The service reads the key file | `service.rs` (status, start, run, `ListModels`) | `service::a_model_served_by_tur_ai_local_is_sent_the_key_from_its_file` (no file: `file_missing`, not ready, `TUR-AI-PROVIDER-012`, nothing sent; file: `in_file`, `Bearer` from the file, nothing in the keyring or any file of P@x, `TUR_AI_PROVIDER_ID=local`) |
| `tur-pax run --provider/--model/--models`, `--no-key` | `textui.rs` | `run_cli::a_provider_of_the_model_list_is_sent_the_key_its_model_server_wrote`, `…no_key_sends_no_credential_and_the_trail_names_the_family` (no `Authorization`; refused for `anthropic`; usage errors) |
| Provider id in the trail (#72) | `TaskRequest::provider_id`; `TUR_AI_PROVIDER_ID` on `task_started` and `task_finished`; `ListActivity.provider_id` | the two `run_cli` tests, the service test, the `local-ai` pax job |
| P@x itself against tur-ai-local | `tests/pax/local_model.py`; `local-ai.yml` job `pax` | run 43318287048 |

## The grammar limit, from llama.cpp's source

Read at the tag **b9840** (the version the workflow pins), files fetched
from `ggml-org/llama.cpp`:

- `src/llama-grammar.cpp`, line 13: `#define MAX_REPETITION_THRESHOLD 2000`.
  Line 652: `if (min_times > MAX_REPETITION_THRESHOLD || (has_max && max_times > MAX_REPETITION_THRESHOLD)) throw … "number of repetitions exceeds sane defaults, please reduce the number of repetitions"`.
  Line 494, in `handle_repetitions`: `if (n_prev_rules * total_rules >= MAX_REPETITION_THRESHOLD) throw …`, where `total_rules` is the maximum count (or the minimum when there is none) and `n_prev_rules` the number of rules the repeated item made (1 for a rule reference, the new symbols of a group).
- `common/json-schema-to-grammar.cpp`: a string with `minLength`/`maxLength` becomes `"\"" char{min,max} "\""` (line 975, `build_repetition` at line 17); an array with `maxItems` becomes `item ("," space item){min-1,max-1}` (line 959). The chat code (`common/chat.cpp`, the PEG parsers) builds each tool's argument grammar through the same converter (`builder.add_schema`, `peg-parser.cpp` line 1872).

So a string bound must be **at most 1999** (`1 × 2000 >= 2000` is
refused: exactly 2000 fails too), an array's `maxItems` at most 667 (a
group of up to three rules). Lowered: argv items 4096, `runtime_inspect_file`
path 4096, `diag_report_prepare` description 4000, `keep_note` text 4000,
`propose_reminder` message and `propose_scheduled_task` request 2000 — all
to 1999. No bound had to stay above the limit, so no tool is left out for
any provider and the `grammar_limited` alternative was not built.

## P@x against Qwen3 4B (local-ai, run 43318287048, job pax)

`tur-ai-local serve qwen3-4b` with llama.cpp's own b9840 CPU build on the
runner (AMD EPYC 7763, 4 logical processors, 16.8 GB), 127.0.0.1:39281,
context 8192, a new key in `$WORK/run/tur-ai-local/api-key`. P@x:
`tur-pax run "<request>" --json --provider local --model qwen3-4b --models
services/tur-pax/data/models.json --tools <every discovered set>` with
`XDG_RUNTIME_DIR=$WORK/run`, no key in the environment, the command tool on,
`--max-steps 8 --max-seconds 1500`, the real bubblewrap sandbox. 19 tools
offered (20 with the fixture).

| Task | Request | Outcome | Seconds | Requests | Tokens in / out | Tool calls the model made (decision, result) | Answer |
|---|---|---|---|---|---|---|---|
| two-tools | "My laptop has hardware trouble. What is wrong with it, and how would I run Discord?" | completed | 293.2 | 2 | 6 409 / 268 | `device_list_problems` {} (class 0, allowed, ok); `catalogue_route` {"id": "com.discord.discord"} (class 0, allowed, ok) | Named the runner's real finding (firmware `regulatory.db` not loaded, error -2) and Flatpak as Discord's recommended route, native and Windows as alternatives |
| local-ai | "Which AI model could run locally on this computer, and why?" | completed | 141.9 | 2 | 6 230 / 257 | `ai_local_recommend` {} (class 0, allowed, ok) | Qwen3 4B, with the memory (16.8 GB, 12.6 available, about 6.2 needed), processor and engine reasons from the tool's output |
| readonly-command (`tests/pax` scenario 03) | "Which system is this, and what does my report say?" (a report file in the granted `~/Documents`) | completed | 32.9 | 2 | 5 470 / 89 | `diag_get_status` {} (class 0, allowed, ok) | Ubuntu 24.04.5, kernel 7.0.0-1017-azure, x86_64, "no failed units". **It did not read the person's report**: it called no command and presented the system status as "the report". Wrong, and recorded as such |

What the model did not do: it never called `run_readonly_command`, so no
command ran in the sandbox in this job (the scripted `pax` and `boot`
workflows run commands in the sandbox; this job proves the engine accepts
the tool's schema). In two-tools it used the real machine's
`device_list_problems`, not the fixture laptop's tool the scripted scenario
names. Every decision was class 0 and allowed; nothing above class 1 was
proposed. The trail of every task: `TUR_AI_PROVIDER=openai-chat` on every
record, `TUR_AI_PROVIDER_ID=local` on `task_started` and `task_finished`.
The key from the file is in no transcript, error output, trail or file of
the run. llama-server's log has no grammar error. Its own timings: the
first request read 2 722 prompt tokens in 162.8 s (16.7 tokens per
second; local-ai-1 measured 47.9 with short prompts), so the provider
layer's two-minute wait for a first byte would have failed it; the preset's
address is on this computer, and such a provider now waits ten minutes.
Later requests reused the cached prefix (20 to 969 new tokens). Generation
4.9 to 6.4 tokens per second. Peak memory 5.62 GB.

## Lines `image/programs.tsv` will need

`image/` was not edited. To ship the two tool sets now at their roots, with
their programs (each line: component, program, path; tab-separated):

```text
services/tur-apps	tur-apps	/usr/bin/tur-apps
services/tur-ai-local	tur-ai-local	/usr/bin/tur-ai-local
```

With them, `image/programs.sh` installs
`/usr/share/tur/pax/tools/tur-apps/` and `/usr/share/tur/pax/tools/tur-ai-local/`
by itself. Each also needs, per `image/programs.tsv`'s header, its
directory in `image/Containerfile.containerignore` (`!services/tur-apps/`,
`!services/tur-ai-local/`) and a `COPY services/<component>/
/src/services/<component>/` in `image/Containerfile`; tur-ai-local also its
model catalogue at `/usr/share/tur/ai-local/models/` and the engine
(local-ai.md, section 9). The image installs no P@x model list today
(nothing in `image/` names `models.json`): for the `local` preset to exist
there, `services/tur-pax/data/models.json` must be installed as
`/usr/share/tur/pax/models.json`. Then the
counts in `tests/image/smoke.sh` ("eleven programs", "five components have
their assistant tools installed") and `tests/boot/report.py`
(`tool_manifests`: five) change with them. `tur-update` already has its
line; its tool set moves to its root together with those counts (six).

## Deviations from the brief

1. **tur-update's tools stay in `services/tur-update/pax/`.** `tur-update`
   is in `image/programs.tsv`; at its root its tools would be installed into
   the image at once, and `tests/image/smoke.sh` ("five components have
   their assistant tools installed") and `tests/boot/report.py`
   (`len(manifests) == 5`, required) would fail the `image` and `boot`
   workflows. This change may not edit `tests/boot/**` and does not own
   `tests/image/**`. The exception is named in `tools.rs`
   (`NOT_YET_AT_THE_ROOT`) with that reason; its grammar is checked there
   anyway. Moving it is two renames, one path in each of its
   `tests/agent_tools.rs` and `tests/interfaces.rs`, deleting the
   exception, and the two counts.
2. **Files outside the listed paths, edited because the brief required
   it:** `services/tur-runtime/agent-tools.json` (the path bound 4096 →
   1999, item 2: "any similar bounds in shipped tool schemas") and
   `services/tur-apps/tests/interfaces.rs` (one path, which broke with the
   move).
3. **The local-ai pax job no longer runs P@x's Responses and Messages
   adapters (`local_ai_ci.py pax`, with `--no-commands`) nor Qwen3 0.6B on
   Fedora's build.** It runs the `local` preset only, with Qwen3 4B; the
   `fedora` and `compare` jobs are unchanged. Qwen3 4B is now cached
   (`local-ai-model-qwen3-4b-<sha256>`) as 0.6B already was.

## Breaking changes

1. `ProviderEntry` has `runtime_key_file`; `Paths` has `runtime`;
   `TaskRequest` has `provider_id`; `Recorded` (tests) has `authorization`.
   Struct literals must name them.
2. `TaskArgs.key_env` is `Option<String>`.
3. `task_started` and `task_finished` carry `TUR_AI_PROVIDER_ID`;
   `GetStatus.key` can be `in_file` or `file_missing`; `ListModels` gives
   `key_source` and `runtime_key_file`; `ListActivity` gives `provider_id`.
4. Schema bounds: a model's argv item, a note, a reminder's message, a
   scheduled task's request, a report description and an inspected path
   longer than 1 999 characters are refused (`TUR-AI-TOOL-002`) where they
   were accepted.
5. The `diag_report_prepare` and `diag_report_list` tools are offered by
   default, and on the image (tur-diagnostics is in it).
6. A provider whose address is this computer waits ten minutes, not two,
   for a response to begin.

## Decisions the owner may want to reverse

1. **The 46 words** an adapter below class 2 may not use, compared exactly
   after dashes and `=`; `update`, `start`, `write` and `output` are among
   them.
2. **Every offered tool must be class 0** (the existing stricter rule kept);
   a component cannot yet ship a class 1 tool with an adapter.
3. **1999 everywhere**, the largest bound llama.cpp accepts, rather than a
   rounder number.
4. **The key file is a field of the provider entry**, honoured for an entry
   added by hand too, but only at an address of this computer.
5. **No `grammar_limited` switch.**
6. **`tested_with_tur` is false for the `local` models**, though Qwen3 4B now
   ran through P@x in CI.
7. **local-ai is triggered** by `services/tur-pax/data/models.json` and
   `tests/pax/local_model.py`, not by every change to tur-pax (a run takes
   about an hour on its longest job).
8. **The `pax` workflow is triggered by `services/**`**, since any
   component may ship tools.

## Limits

- One local model, one run, three tasks; a real model's choices vary.
- The command tool was offered to and accepted by llama-server, never
  called by the model.
- The scripted e2e calls none of the newly discovered tools (tur-apps,
  tur-ai-local, the two report tools): they are covered by their
  components' own tests, which run them against the built programs.
- `ai_local_status` cannot see a served model from P@x's sandbox
  (`TUR-AI-LOCAL-027`, unchanged).
- A tool's output can be up to 256 KiB; with tur-ai-local's 8 192-token
  context a large result can overflow it. Not seen in this run; P@x has no
  per-provider budget for tool results.
- Not tested on the image: P@x with `local` (tur-ai-local is not in it).

## Follow-ups

- The image lines above; tur-update's move with the two counts.
- `docs/architecture/problem-reports.md`, `update-manager.md`,
  `app-install-backend.md` and the reports apps-1, update-1, local-ai-1
  still describe the parked locations (not this change's files).
- A budget for tool output against the model's declared context.
- A scripted scenario for the newly discovered tools, on the runner only.

## Found on the way

- The read-only command tool was not the only schema over llama.cpp's
  limit: `runtime_inspect_file`, `diag_report_prepare`, `keep_note` and the
  two proposals were too (the proposals at exactly 2 000, which the
  repetition check refuses).
- llama-server read P@x's first request at 16.7 tokens per second here, a
  third of the 47.9 measured with short prompts in local-ai-1, so the
  provider layer's two-minute first-byte timeout was too short for a model
  on the processor.
- tur-device-manager's fixtures contain circular links: any test that walks
  `services/` must not follow links.

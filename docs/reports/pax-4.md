# pax-4 — P@x with any OpenAI-compatible service: DeepSeek and others, as data

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-4
Subsystem:        AI control layer: the provider layer's fourth adapter
                  family (OpenAI-compatible Chat Completions), providers as
                  data with their data terms and declared capabilities, the
                  presets, consent per provider, the manual real-provider
                  workflow
Build ID:         not applicable: this task builds no image
Source revision:  f01cd2c (the code; this report is the commit after it)
                  (feat/pax-openai-compatible, pull request 69)

SUMMARY
- What changed: tur-pax-providers gains the adapter family `openai-chat`,
  the Chat Completions API that DeepSeek, Mistral, xAI, Groq, OpenRouter,
  Together and local model servers offer. A provider is an entry of data
  (identifier, name, family, address, keyring item, whether it needs a key,
  per-model capabilities and prices, data terms), and the model list ships
  presets for those seven services and two servers on this computer
  (Ollama, llama.cpp's llama-server). The consent text (version 2) says,
  per provider, what its terms say as TUR read them and when, or that they
  are "not reviewed by TUR"; an entry added by hand is never shown as
  reviewed. A model whose entry declares no tool calling is offered no
  tools and the person is told why; a one-request probe can check tool
  calling. pax-live can run against DeepSeek and allows 8 requests and
  24 000 tokens a run.
- Why it changed: ADR-0004's amendment of 12 November 2026 (DeepSeek, and
  any compatible service by configuration) and ADR-0021 (P@x with a local
  model through the compatible adapter); pax-3's list "What makes a fourth
  adapter family hard".
- What it does not show: NO COMPATIBLE SERVICE WAS CALLED, here or anywhere
  in the development of this change. Every service in every test is a
  script written by hand from documentation. The DeepSeek secret does not
  exist yet; pax-live was not started.

FILES
- Added: services/tur-pax-providers/src/{openai_chat,probe}.rs;
  services/tur-pax-providers/tests/{openai_chat.rs,common/openai_chat.rs,
  data/full-openai-chat.json}; tests/pax/scenarios/{18-openai-chat-adapter,
  19-openai-chat-repeated-key}.json; docs/reports/pax-4.md
- Modified: services/tur-pax-providers/src/{model,config,provider,http,
  toolcalls,cli,lib,spend,anthropic,openai,google}.rs and
  tests/{conformance,secrets,cli,config_and_schema,common/mod}.rs;
  services/tur-pax/src/{settings,consent,service,spending}.rs,
  data/models.json, tests/{service.rs,common/mod.rs};
  schemas/pax-provider-config.schema.json, schemas/pax-models.schema.json;
  tests/pax/{e2e,fake_provider,live,live_ping}.py;
  .github/workflows/pax-live.yml; docs/architecture/{pax-providers,
  pax-service}.md
- Removed: nothing

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test --all-targets --all-features; cargo test --doc (workflow "rust",
  every component); cargo build --locked (workflow "pax"; the image)
- Result: see "Runs"
- Warnings: none from the compiler or clippy

TESTS
- Unit and integration tests, workflow "rust": pass. tur-pax-providers 229
  (was 176), tur-pax 413 (was 405); every other component unchanged and
  passing
- End to end, workflow "pax": pass, 388 checks in 19 scenarios (18 and 19
  new, through the compatible family), then 103 checks of the session
  service
- On the installed system, workflow "boot": pass, the same 388 checks
- Against a real provider: NOT RUN (pax-live not started; DEEPSEEK_KEY does
  not exist)
- Against Ollama, llama-server, or the local AI service: NOT RUN
- Hardware, performance: NOT RUN

DIAGNOSTICS
- Log paths: the GitHub Actions runs below; artifact "pax-evidence"
- Diagnostic bundle path: none
- Relevant error IDs: none new; TUR-AI-PROVIDER-001 to -012 cover the
  family. The notice that a model cannot call tools has no code.

RECOVERY
- Rollback procedure: revert the merge. The settings file gains no field;
  a person's consent of version 1 is no longer current, so P@x asks again
  once (that is intended). A model list with the new fields cannot be read
  by the old build: it must be reverted with it.
- Recovery tested: a model list with a bad entry is refused whole and
  leaves P@x without one (settings::tests, existing behaviour).

KNOWN LIMITATIONS
- docs/architecture/pax-providers.md, section 14.7; the list at the end
  of this report.

NEXT SAFE ACTION
- Create the secret DEEPSEEK_KEY and start pax-live with provider deepseek;
  then the settings pages that add a provider and run the probe (D-19).
```

## Runs

The code is at `f01cd2c`; the commit that adds this report changes no
code and its runs are on the pull request.

| Workflow | Run on `f01cd2c` | Result |
|---|---|---|
| `rust` | 43285884754 | Pass, every component. `tur-pax-providers`: 229 tests (112 unit, 8 cli, 6 config and schema, 88 conformance = 22 x 4 families, 9 openai_chat, 2 secrets, 4 doc), was 176. `tur-pax`: 413 tests in 16 targets (226 unit, 175 integration, 12 doc), was 405; the invariant sweep 186 s. Clippy `-D warnings` clean |
| `pax` | 43285884632 | Pass: 388 checks in 19 scenarios, 0 failed (was 349 in 17; 18 `openai-chat-adapter` and 19 `openai-chat-repeated-key` are new; the "everywhere" check requires all four families); session service 103 checks, 0 failed |
| `image` | 43285884454 | Pass |
| `boot` | 43285884315 | Pass. On the installed system `tests/pax/e2e.py`: 388 checks in 19 scenarios, 0 failed. Two informational failures that are not this change's: see "Found on the way" |
| `gate` | 43285884576 | Pass |
| `pax-live` | not run | Starts by hand only; `DEEPSEEK_KEY` does not exist |

The commits of the pull request:

| Commit | What | Result |
|---|---|---|
| `896c227` | The `openai-chat` family in tur-pax-providers | no run alone (no pull request yet) |
| `5521ca5` | Providers as data, terms, capabilities, presets, consent | `rust` 43281994714: every test and clippy passed in both changed components; failed only on rustfmt. `pax` 43281994569 pass (349 + 103, before the new scenarios). `image` 43281994750, `boot` 43281994554 pass. `gate` 43281994998 failed on `rust` |
| `e60bf19` | End-to-end scenarios, pax-live, docs | pushed with the next commit |
| `f01cd2c` | Formatting from CI | all pass, table above |

Two pushes of code. The only failure was formatting; no test of mine
failed in CI and no defect was found by CI.

## What each part is, and what proves it

| Part | Built | Proved by |
|---|---|---|
| Request mapping: system, user, assistant, tool messages; tools with JSON Schema; `tool_choice: "auto"` | `openai_chat::request_body` | `openai_chat::tests::{a_plain_request_has_only_the_documented_fields, tools_are_function_tools_with_the_schema_unchanged, a_tool_round_trip_uses_tool_calls_and_tool_messages, images_are_data_urls_and_not_allowed_in_tool_results, parts_in_the_wrong_role_are_rejected_before_anything_is_sent}`; conformance `openai_chat::plain_answer`, `::tool_result_goes_back_as_tool_output` |
| Streamed tool-call deltas reassembled by index, cut at awkward points | `ChatDecoder::tool_delta`, `fill`; `toolcalls::ToolCalls::discard` | conformance `openai_chat::{tool_call_with_split_arguments, two_tool_calls_in_one_turn, arguments_delivered_whole_are_one_complete_call}`; `tests/openai_chat.rs::tool_call_text_cut_inside_an_escape_and_bytes_cut_inside_a_character` (cut inside `\"` and `ö`, the stream one byte at a time); unit `calls_without_an_index_are_told_apart_by_their_identifier`, `arguments_before_the_name_wait_for_it_and_a_call_without_one_is_not_delivered` |
| Strict reading | the same parser as every family; two names, two identifiers, non-text arguments also undeliverable | conformance `openai_chat::{a_repeated_key_in_streamed_arguments_is_a_malformed_response, a_repeated_key_in_whole_arguments_is_a_malformed_response, garbage_is_a_malformed_response}`; `tests/openai_chat.rs::a_repeated_key_split_across_deltas_at_any_depth_discards_the_response`; unit `a_call_the_service_describes_two_ways_is_never_delivered`; tur-pax `tests/wires.rs::a_repeated_key_discards_the_whole_response_through_every_adapter` and `::a_repeated_key_at_depth_is_refused_through_every_adapter` (now four families); `pax` scenario 19 |
| Usage for the day limit; a marked estimate without it | `stream_options.include_usage`; `Usage::estimated`; `ChatDecoder::estimate`; the `Decoder::done` and `interrupted` hooks | `tests/openai_chat.rs::a_service_that_sends_no_usage_is_counted_at_a_marked_estimate`, `::a_reported_usage_is_taken_as_it_is_wherever_the_service_puts_it`; unit `a_response_without_usage_is_counted_at_a_marked_estimate`, `the_estimate_counts_text_high_and_images_at_a_fixed_figure`; `model::tests::usage_adds_and_saturates`; `provider::tests::the_done_marker_and_an_interruption_reach_the_decoder` |
| Finish reasons: `stop`, `tool_calls`, `length` (discarded whole), `content_filter` (refusal), DeepSeek's two, errors | `ChatDecoder::reason` | unit `finish_reasons_map_to_the_neutral_ones`; conformance `openai_chat::{output_limit_is_reported, refusal_is_a_normal_result, error_event_mid_stream, stream_cut_mid_way}`; `tests/openai_chat.rs::the_endings_a_service_can_give`; tur-pax `tests/wires.rs::a_response_that_does_not_end_well_is_discarded_through_every_adapter` |
| Errors and rate limits; 429 with retry-after | `openai_chat::classify`, `error_from`; the shared transport | conformance `openai_chat::{rate_limit_then_success, rate_limit_reports_the_delay_the_provider_gave, overload_is_retried_up_to_the_limit, authentication_failure_is_not_retried, rejected_request_is_not_retried, silence_times_out, unreachable_provider_is_a_network_error}`; `tests/openai_chat.rs::a_rate_limit_with_retry_after_is_waited_for_then_succeeds`; unit `error_bodies_map_to_kinds` |
| Image input where declared | `image_url` parts with a `data:` URL; refused in a tool result | unit `images_are_data_urls_and_not_allowed_in_tool_results` |
| Reasoning never an answer, never an instruction, sent back only where required | opaque part; `send_back_reasoning` | `tests/openai_chat.rs::reasoning_is_never_text_and_goes_back_only_where_the_service_requires_it`, `::the_reasoning_field_of_other_servers_is_kept_the_same_way`; unit `reasoning_goes_back_only_where_the_service_requires_it`; `pax` scenario 18 (`check_openai_chat_adapter`: the reasoning went back with every assistant turn and is in no transcript or trail) |
| The key: only in its header; none for a keyless server | `connect_without_key`; `Transport` with an optional key | `tests/secrets.rs` (all four families); `tests/openai_chat.rs::a_server_that_takes_no_key_is_sent_no_credential`; `tests/cli.rs::a_server_that_takes_no_key_is_sent_none` |
| The tool probe | `probe.rs`; `check --probe-tools` | `probe::tests`; `tests/cli.rs::the_tool_probe_reports_whether_the_model_called_the_tool` |
| The family has no address of its own | `default_base_url` returns `Option`; `validate` | `config::tests::every_company_family_has_a_public_endpoint_and_the_compatible_one_none`, `::reasoning_is_sent_back_only_by_the_family_that_reads_the_setting`; `tests/config_and_schema.rs` |
| Providers as data | `ProviderEntry` with `key_required`, `send_back_reasoning`, `data_terms`; `ModelEntry.capabilities`; the schema | `settings::tests::a_compatible_provider_is_data_with_terms_capabilities_and_an_optional_key`, `::the_shipped_list_loads_with_its_presets` |
| Keyring items take any provider identifier | `is_keyring_item` | `settings::tests::every_provider_identifier_is_a_keyring_item_and_the_set_is_safe` |
| Capabilities checked | `Abilities`; `run_job` offers no tools to a model that declares none; `GetStatus.abilities`, `ListModels.can_act/can_see/notices` | `tests/service.rs::a_model_that_cannot_call_tools_is_offered_none_and_the_person_is_told` |
| Consent and privacy text per provider; consent per provider id | `consent::consent_text` (version 2); `Core::consent` | `consent::tests` (five); `tests/service.rs::a_listed_compatible_provider_shows_its_terms_and_runs_through_its_family` (including: another provider asks again), `::a_model_server_on_this_computer_needs_no_key_and_says_so`, `::a_provider_added_by_hand_is_used_by_its_identifier_and_keyring_item` |
| End-to-end through the family | `fake_provider.render_openai_chat`; scenarios 18, 19 | `pax` workflow; the "everywhere" check now requires all four families |
| Nothing above class 1, over every family | unchanged engine | tur-pax `tests/wires.rs::nothing_above_class_one_runs_through_any_adapter` (four families, streamed and whole); `tests/orchestrator.rs` sweep and `tests/service.rs` invariants, unchanged; `pax` `everywhere` |
| pax-live: provider input, 8 requests / 24 000 tokens | `pax-live.yml`, `live.py`, `live_ping.py` | NOT RUN (manual only) |

## Presets

docs/architecture/pax-providers.md, section 14.8, has every preset with
its base URL, models, example prices, the pages it was verified in and
the pages its data terms were read from. All pages were read on
2026-11-12 by reading the services' own documentation (no API was
called).

| Preset | Endpoint and tools verified in | Data terms |
|---|---|---|
| DeepSeek | <https://api-docs.deepseek.com> (base URL `https://api.deepseek.com`, `/chat/completions`, Bearer; tool calls and their streaming; `reasoning_content` and the rule that it must go back when tools are offered; finish reasons; error codes; `: keep-alive`) | Privacy policy (2026-02-10), terms of use (2026-03-27), open platform terms (2026-04-29): stores prompts with the account, no fixed retention, may train with an opt-out described for the chat product, processed in the People's Republic of China |
| Mistral | <https://docs.mistral.ai/resources/migration-guides.md> and the API reference | Privacy policy, commercial terms, regional inference, ZDR |
| xAI | <https://docs.x.ai/developers/model-capabilities/legacy/chat-completions.md>, function calling | Security FAQ of its documentation; legal pages not read |
| Groq | <https://console.groq.com/docs/openai>, tool use | Your data, services agreement |
| OpenRouter | <https://openrouter.ai/docs/api_reference/overview.md>, parameters, streaming | Its privacy guides; legal page not read |
| Together | <https://docs.together.ai/docs/inference/openai-compatibility.md>, function calling | Privacy and security, ZDR; legal page not read |
| Ollama | <https://docs.ollama.com/api/openai-compatibility.md>, FAQ (127.0.0.1:11434) | Runs on this computer |
| llama-server | llama.cpp `tools/server/README.md`, master (127.0.0.1:9931, no key by default) | Runs on this computer |

**Left out:** no candidate. Each documents an OpenAI-compatible Chat
Completions endpoint with tool calling.

**Surprises the brief did not expect, from the documentation:**

- DeepSeek's model identifiers are now `deepseek-flash` (V4.1 Flash,
  images) and `deepseek-v4-pro` (no images); `deepseek-chat` and
  `deepseek-reasoner` were retired on 2026-07-24. Thinking mode is on by
  default, and with `tools` the reasoning of every earlier turn **must** be
  sent back or the API answers 400; the older rule (sending it back is an
  error) is reversed. Its prices differ by time of day; the list gives the
  peak rates.
- llama.cpp's `llama-server` now listens on port **9931** by default, not
  8080.
- Ollama lists `tool_choice` as not supported.
- xAI calls Chat Completions a legacy endpoint and sends each tool call
  whole in one chunk.

## Breaking changes

1. `tur_pax_providers::ProviderId` has a fourth variant, `OpenAiChat`
   (`"openai-chat"`). An exhaustive match elsewhere must decide it.
2. `config::default_base_url` returns `Option<&'static str>`.
3. `Usage` has a field `estimated`; struct literals must name it.
4. `ProviderConfig` has a field `send_back_reasoning`; struct literals must
   name it.
5. `ProviderEntry` has `key_required`, `send_back_reasoning`, `data_terms`;
   `ModelEntry` has `capabilities`.
6. `consent::Subject` has `terms` and `runs_locally`;
   `CONSENT_TEXT_VERSION` is `2`, so every person is asked again once.
7. `GetStatus.key` can be `not_needed`; `GetStatus` gains `abilities`;
   `ListModels` gains fields. The D-Bus interface's methods and signatures
   are unchanged.
8. `tests/pax/live_ping.py` takes `PROVIDER MODEL` (it took `MODEL`);
   `pax-live` takes `provider` and its `model` defaults to `default`.

## Decisions the owner may want to reverse

1. **The family enum stays an enum.** `ProviderId` names adapter families,
   which are code; providers are data in `tur-pax`. An enum of companies no
   longer exists anywhere a provider is chosen.
2. **Reasoning goes back only with `send_back_reasoning`**, set by the
   DeepSeek preset alone, and in the field it came in.
3. **No usage is counted high**: two bytes a token, 3 000 tokens an image,
   the whole output limit. It may stop a day early with a service that
   sends no usage (Groq and llama-server document none).
4. **A model that declares no tool calling gets a conversation, not a
   refusal**: no tools, no command, and a notice.
5. **Nothing declared means tools are offered**, as before (the local
   presets declare nothing).
6. **Images in tool results are refused** for the family, though DeepSeek
   accepts them.
7. **`tool_choice: "auto"` is sent** whenever tools are offered, though it
   is the documented default and Ollama lists it as unsupported.
8. **`max_tokens`**, not `max_completion_tokens`.
9. **Data terms are TUR's summary**, shown with the date and the line that
   the provider's own terms govern. xAI's, OpenRouter's and Together's
   come from their documentation's data pages; their legal policies were
   not read.
10. **A reviewed entry reached through a gateway address is "not
    reviewed"**, and a local preset reached through a non-loopback address
    is not "on this computer".
11. **The consent version is one number for every provider.** A change to
    one provider's data terms in the list does not ask again by itself.
12. **The trail names the adapter family** (`openai-chat`), not the
    provider (`deepseek`), as before for the other families.
13. **Keyring items** gained `.` and a length of 64.
14. **DeepSeek prices are the peak rates** (off-peak is half).
15. **pax-live: DeepSeek's output limit is 1 200 tokens a response**
    (Anthropic's stays 400), because its models think before they answer by
    default and count that as output.

## Limits and what is not proven

1. No compatible service has been called. The scripts are written from
   documentation; a real service may differ (pax-providers.md 14.7 lists
   what was not verified per service).
2. `tur-pax run` (the command line) has no `--no-key`: a keyless server is
   reachable through the session service, and through
   `tur-pax-providers check --no-key`.
3. The probe is not run by the service when a person adds a model: no
   page adds one yet (D-19). It is a library function and a command.
4. Screen control does not exist, so "no screen control without image
   input" is a flag and a sentence, not a withheld feature.
5. A response cut before any usage arrives is counted at the estimate by
   this family only; the other three families count it at zero, as before.
6. Mistral's reasoning mode (thinking parts in `content`) is not used and
   would not be replayed as Mistral asks.
7. The day ledger does not record that a day holds estimates; the task's
   usage does (`"estimated": true`).

## Proposed text for ai-control-layer.md

Not edited here. Proposed as section 24, "Resolutions from the
OpenAI-compatible family" (or as rows of the section the owner chooses):

| # | Point | Resolution |
|---|---|---|
| 1 | Section 10 lists one adapter per provider | Adapters are per API family (ADR-0004, amended): Messages, Responses, Interactions, and OpenAI-compatible Chat Completions. A provider is an entry of data naming its family, address, key item, models, capabilities and data terms. |
| 2 | "The adapter reports the provider's declared capabilities" | The family declares what its format can carry; each provider entry declares, per model, tool calling, streamed tool calls, image input and context size. P@x offers no tools to a model that declares no tool calling, and says so; it will not offer screen control to one without image input. A one-request probe can check tool calling. |
| 3 | The spend limit relies on reported usage | A family that may not report usage asks for it, and counts a response without it at a marked high estimate, so that no response costs nothing to a limit. |
| 4 | Reasoning a service streams beside the answer | Never the answer, never an instruction; kept as an opaque part; sent back only where the service's documentation requires it (DeepSeek with tools). |
| 5 | Section 8: what the provider keeps | Said per provider in the consent text, from data terms TUR read, with the date; otherwise "not reviewed by TUR", always so for a provider added by hand. A model on this computer is said to stay on it. |
| 6 | Keys | A service on this computer may need none; no credential is then sent. Only the compatible family can be used without a key. |

## Follow-ups worth an issue

1. Create `DEEPSEEK_KEY`, run `pax-live` with `provider: deepseek`, record
   what the real service sends (usage, reasoning, tool-call deltas) and
   correct the scripts where it differs.
2. A settings page (D-19) to add a provider by hand, show its data terms
   or "not reviewed", and run the probe, storing the result.
3. Ask again when a provider's reviewed data terms change (a per-provider
   consent version, such as the review date in the record).
4. Read the legal privacy policies of xAI, OpenRouter and Together, and
   settle Mistral's training default for paid API use.
5. `tur-pax run --no-key` for local servers on the command line.
6. Name the provider, not only the family, in the audit trail.
7. When `tur-ai-local` serves its API, a preset for it and the local
   scenario run of ADR-0021.
8. A maintained model list (from pax-3), now with data terms to keep.

## Found on the way

- The brief's expectations of DeepSeek (`deepseek-chat`, reasoning never
  sent back) and llama.cpp (port 8080) were out of date; see "Presets".
- `boot` reports two informational failures on every run, not caused by
  this change: a folder granted as `/home/<name>/Documents` is refused
  because `/home` is a link on the image (`TUR-AI-SANDBOX-002`, "name the
  folder itself"), and two SELinux `nnp_transition` denials when
  `tur-diag` starts `bootc` with no new privileges (as in pax-3).
- The `[DONE]` marker was swallowed by the event stream before any
  adapter saw it. Chat Completions has no other end of stream, so the
  decoder now has a `done` hook (and an `interrupted` one for counting a
  cut response).

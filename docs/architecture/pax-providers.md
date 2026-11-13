# P@x provider layer

**Part of the [AI control layer](ai-control-layer.md)**, sections 3, 7, 8 and 10
**Component:** `services/tur-pax-providers`
**Related decisions:** [ADR-0004](../adr/0004-ai-provider-model.md), [ADR-0005](../adr/0005-languages-and-workspace.md), [ADR-0008](../adr/0008-assistant-tool-protocol.md)

This document describes the provider layer as implemented: the neutral
conversation model and why it has the shape it has, how each provider maps onto
it, what each adapter declares it can do, the error codes, how untrusted content
is marked, what was verified in each provider's documentation, and what is
known to be missing.

**Nothing here has been tested against a real provider by this work.** The
adapters are written from the providers' published documentation, read on
2026-11-12, and tested against a local server that replays responses written
by hand from that documentation. No request has been sent to Anthropic,
OpenAI, Google, DeepSeek or any other compatible service by the tests, and no
key exists in CI outside the manual workflow `pax-live`. Every statement
below about a provider's behaviour is a statement about its documentation.

**Four adapter families, any number of providers** (ADR-0004, amended 12
November 2026). The fourth family, `openai-chat`, speaks the
OpenAI-compatible Chat Completions API that DeepSeek, Mistral, xAI, Groq,
OpenRouter, Together and local model servers (Ollama, llama.cpp's
`llama-server`) offer. A *provider* is data: an entry naming its family, its
address, its key's keyring item, its models and what they can do, and its
data terms ([section 14](#14-the-openai-compatible-family-and-its-presets)).

---

## 1. What the layer is

One interface, four adapters, no policy.

```text
orchestrator ── Request ──▶ Provider::stream ──▶ EventStream (an iterator)
                                   │
                    adapter: neutral request → the provider's JSON
                             the provider's event stream → neutral events
                                   │
                              HTTPS to the provider
```

The layer sends a conversation and streams back what the model said and which
tools it wants to call. It does not decide whether a tool call may run: that is
the policy engine (`services/tur-pax`), and every tool call this layer
reports is a proposal with no authority. It does not run tools, keep
conversation history, or store keys.

The interface is blocking. A stream is an iterator; the orchestrator runs a
call on a thread of its own. There is no async runtime.

| Module | Content |
|---|---|
| `model` | The neutral conversation model and the stream events |
| `provider` | The `Provider` trait, `Capabilities`, `EventStream`, `connect` |
| `anthropic`, `openai`, `google`, `openai_chat` | The adapters, one for each family |
| `probe` | A one-request check that a model calls tools |
| `error` | `ProviderError` and the `TUR-AI-PROVIDER-*` registry |
| `secret` | `ApiKey`, `KeySource`, `EnvKeySource` |
| `config` | The configuration file format |
| `generation` | The generation settings and the fields each family takes them in (section 15) |
| `listing` | The models a provider offers, from its own list (section 15.4) |
| `spend` | `SpendMeter`: usage per task, cost, the task limit |
| `http`, `sse`, `toolcalls` | Private: transport and retries, the event-stream parser, tool-call assembly |

## 2. Which API of each provider, and why

| Provider | API | Reason |
|---|---|---|
| Anthropic | Messages API, `POST /v1/messages`, API version `2026-10-21` | The provider's one conversation API. The version is the current one and is pinned in the `anthropic-version` header. |
| OpenAI | Responses API, `POST /v1/responses` | The provider's guide says: "While Chat Completions remains supported, Responses is recommended for all new projects." Chat Completions is not used. |
| Google | Gemini Interactions API, `POST /v1beta/interactions` | The provider's overview says the Interactions API is generally available as of June 2026 and "the recommended interface for all new projects", and that `generateContent` "is now considered legacy" though it "remains fully supported". |
| Any compatible service (`openai-chat`) | Chat Completions, `POST <base URL>/chat/completions` | The interface DeepSeek and the other services of section 14 document as their OpenAI-compatible one. The base URL is the service's, as it documents it (`https://api.deepseek.com`, `https://api.groq.com/openai/v1`, `http://127.0.0.1:11434/v1`); the family has none of its own and a configuration without one is refused. This is not used for OpenAI itself, whose own family stays the Responses API. |

The task that produced this layer named `generateContent` for Google. That was
written before the documentation was read. The documentation at
`ai.google.dev` now redirects to `aistudio.google.com`, is organised around the
Interactions API, and no longer gives the `generateContent` streaming format,
error format or thought-signature rules in its guides. An adapter for
`generateContent` could therefore not have been verified against current
documentation, and would have been new code against an interface its provider
calls legacy. The Interactions API was used instead. This is a deviation from
the brief and is flagged for review.

OpenAI and Google both keep conversations on their servers by default. Both
adapters send `store: false` on every request and carry the whole conversation
each time: see [section 9](#9-privacy-what-each-provider-keeps).

## 3. The neutral model

### 3.1 Shape

```text
Request   model, system?, messages[], tools[], max_output_tokens
Message   role (user | assistant), content[]
Part      Text | Image | ToolCall | ToolResult | Opaque
Tool      name, description, input_schema (JSON Schema, type object)

StreamEvent   TextDelta
              ToolCallStart / ToolCallArgumentsDelta / ToolCallEnd
              Opaque
              Usage
              Finish(EndTurn | ToolUse | OutputLimit | Refusal(error) | Error(error))
```

### 3.2 Why this shape

Each choice below is where the three interfaces differ, and what was chosen so
that none of them is a second-class fit.

| Choice | Reason |
|---|---|
| **Two roles, no system role.** The system text is a field of the request. | All three take instructions outside the conversation: `system`, `instructions`, `system_instruction`. A system role inside the message list would have to be hoisted out for every provider. It also keeps a rule simple: nothing untrusted can be in the instructions, because nothing but the orchestrator's own text is ever put in that field. |
| **A tool result is a part of a user message**, not a role of its own. | Anthropic puts `tool_result` blocks in a user message. OpenAI and Google use separate items (`function_call_output`, `function_result`) in a flat list. A part of a user message maps to both; a separate role would not map to Anthropic without merging. |
| **A tool result carries the tool name** as well as the call identifier. | Google's `function_result` requires `name`. The other two ignore it. |
| **Tool-call arguments are always a JSON object with one reading**, and a call is delivered complete only if they parse as one, strictly: no object in them, at any depth, repeats a key. The text they were read from is delivered with the call (`ToolCallEnd.arguments_text`). | Anthropic's `input` and Google's `arguments` are objects; OpenAI's `arguments` is a JSON string. All three stream them as fragments of JSON text. The policy engine needs an object. JSON does not say what a repeated key means, and a lenient parser keeps the last value: one value could be shown to the user and another acted on. See [section 4.3](#43-strict-reading). |
| **An opaque part**, tagged with its provider, that is sent back unchanged. | All three return a record of the model's reasoning that they expect in the next request, each in its own form: Anthropic `thinking` and `redacted_thinking` blocks with a signature, OpenAI `reasoning` items with `encrypted_content`, Google `thought` steps with a signature. Anthropic rejects a tool-use turn whose thinking blocks were changed. The orchestrator must not interpret these, so they are opaque. They are tagged so that an adapter sends back only its own. |
| **Events arrive in the provider's order, and `Turn::collect` keeps that order** in the assistant message. | Anthropic requires thinking blocks to stay where they were relative to the tool call. OpenAI requires a reasoning item to precede the call it led to. Google's documentation says to send the model's steps back "exactly as received". |
| **A tool call has a start, argument fragments and an end**, numbered from 0 within a response. | That is the common outline of `content_block_*` (Anthropic), `response.output_item.*` with `response.function_call_arguments.*` (OpenAI) and `step.*` (Google). The providers' own indices differ in meaning and are not exposed. |
| **Usage is a snapshot of the response so far**, not an increment. | Anthropic reports cumulative counts twice; OpenAI and Google report once at the end. A snapshot is the only form all three can produce. |
| **Input tokens include cached ones; output tokens include reasoning.** | Anthropic reports uncached input separately from cache reads and writes; OpenAI reports a total with a cached part; Google reports thought tokens beside output tokens. The neutral counts are the totals that are billed as input and as output. |
| **One finish event, always.** A refusal and an error are finish reasons that carry a typed error. | Section 10 of the design requires a refusal, a rate limit, a timeout and a malformed response to be normal results. A stream that always ends the same way cannot be mishandled by forgetting a case. |
| **Refusal is separate from error.** | Anthropic reports a refusal as a stop reason, OpenAI as a refusal content part or a `content_filter` incomplete response, Google as a "generation blocked" error code. The orchestrator tells the user something different for a refusal than for a failure. |
| **The output limit is a field of the request.** | Anthropic requires `max_tokens`. The other two accept a limit. One required field is simpler than an optional one with per-provider defaults. |
| **Images are base64 with a media type.** | That is what all three accept inline (Anthropic `source.base64`, OpenAI a `data:` URL, Google `data` with `mime_type`). URLs and file uploads are provider-specific and are not modelled. |

### 3.3 The stream contract

`Provider::stream` returns at once and sends nothing. The request is sent when
the stream is first polled. The stream then yields zero or more events and
**exactly one `Finish`**, and nothing after it. This holds for every outcome,
including a request that could not be built, a connection that could not be
made and a stream cut half-way.

On `Refusal` and on `Error`, what was streamed in that response is incomplete
and must be discarded. Anthropic's documentation says so explicitly for a
refusal that arrives mid-stream; the same rule is applied to all three.

### 3.4 Tool definitions and the tool manifest

A `ToolDefinition` is a name, a description and a JSON Schema: the three fields
a tool has in the reviewed manifest of `services/tur-pax`. It is built with
`ToolDefinition::new(tool.name(), tool.description(), tool.input_schema().clone())`.

This crate does not depend on `tur-pax`. The dependency will run the other
way: `tur-pax` owns all communication with providers (ADR-0004) and will
depend on this crate, so a dependency from here to there would be circular. The
constructor takes the three plain values instead.

The name must be acceptable to all three providers, so it is limited to 1 to 64
characters from `A-Z`, `a-z`, `0-9`, `_` and `-`. Manifest tool names
(`^[a-z][a-z0-9_]{0,63}$`) always qualify.

## 4. Mapping, per provider

### 4.1 Request

| Neutral | Anthropic | OpenAI | Google |
|---|---|---|---|
| Endpoint | `POST /v1/messages` | `POST /v1/responses` | `POST /v1beta/interactions` |
| Key | header `x-api-key` | header `Authorization: Bearer` | header `x-goog-api-key` |
| Version | header `anthropic-version: 2026-10-21` | none | path `v1beta` |
| `model` | `model` | `model` | `model` |
| `system` | `system` | `instructions` | `system_instruction` |
| `max_output_tokens` | `max_tokens` | `max_output_tokens` | `generation_config.max_output_tokens` |
| Streaming | `stream: true` | `stream: true` | `stream: true` |
| Stateless | always | `store: false` | `store: false` |
| Tool | `{name, description, input_schema}` | `{type: "function", name, description, parameters, strict: false}` | `{type: "function", name, description, parameters}` |
| User text | `{type: "text"}` block | `input_text` part of a `user` message | `text` content of a `user_input` step |
| Image | `{type: "image", source: {type: "base64", media_type, data}}` | `{type: "input_image", detail: "auto", image_url: "data:…"}` | `{type: "image", mime_type, data}` |
| Assistant text | `text` block | `{role: "assistant", content: "…"}` | `model_output` step |
| Tool call | `tool_use` block `{id, name, input}` | `function_call` item `{call_id, name, arguments}` (a JSON string) | `function_call` step `{id, name, arguments}` |
| Tool result | `tool_result` block `{tool_use_id, content[], is_error}` | `function_call_output` item `{call_id, output}` | `function_result` step `{call_id, name, result[], is_error}` |
| Opaque | `thinking` / `redacted_thinking` block, as received | `reasoning` item, as received | `thought` step, as received |

Adapter behaviour that follows from the providers' rules:

- **Anthropic.** Neighbouring messages of one role are sent as one, because the
  API takes alternating turns. Within a user turn the `tool_result` blocks are
  placed first, as the API requires. Empty text blocks are left out.
- **OpenAI.** A user message becomes one `function_call_output` item for each
  tool result, followed by one message item with the text and images. A tool
  output that is all text is sent as a string; one with an image is sent as a
  list of `input_text` and `input_image` parts. The API has no field that marks
  a failed call, so `is_error` is not sent: the text has to say that the call
  failed. `strict: false` is sent because without it the API tries to rewrite
  the schema for strict mode; the schema goes as the manifest has it.
- **Google.** A user message becomes one `function_result` step for each tool
  result, followed by one `user_input` step.
- **All three.** A part in the wrong role (a tool call in a user message, an
  image in an assistant message) is rejected locally with
  `TUR-AI-PROVIDER-004` before anything is sent. An opaque part from another
  provider is left out.

Generation settings (temperature, top-p, top-k, stop sequences, reasoning
effort, a thinking budget) are sent only when a configuration sets them, in
the fields each family documents, and a setting a family has no place for
is refused before anything is sent: [section 15](#15-generation-settings-and-model-lists).
With none set, nothing is sent and the model's defaults apply, as before. No
tool-choice setting is sent to the three companies' families: several current
models reject a forced tool choice.

### 4.2 Stream

| Neutral event | Anthropic | OpenAI | Google |
|---|---|---|---|
| `TextDelta` | `content_block_delta` with `text_delta` | `response.output_text.delta` | `step.delta` with `text` |
| `ToolCallStart` | `content_block_start` with a `tool_use` block | `response.output_item.added` with a `function_call` item | `step.start` with a `function_call` step |
| `ToolCallArgumentsDelta` | `input_json_delta` | `response.function_call_arguments.delta` | `step.delta` with `arguments_delta` |
| `ToolCallEnd` | `content_block_stop` | `response.output_item.done` (its `arguments` are the text the call is read from; the deltas must add up to the same arguments) | `step.stop` |
| `Opaque` | `content_block_stop` of a thinking block, assembled from `thinking_delta` and `signature_delta` | `response.output_item.done` with a `reasoning` item | `step.stop` of a `thought` step, assembled from `thought_signature` and `thought_summary` |
| `Usage` | `message_start`, `message_delta` | `response.completed`, `response.incomplete`, `response.failed` | `interaction.completed` |
| `Finish` | `message_stop` | `response.completed`, `response.incomplete`, `response.failed`, `error` | `interaction.completed`, `error` |
| Ignored | `ping`, unknown types | `response.created`, `response.in_progress`, content part events, unknown types | `interaction.created`, `interaction.status_update` (unless failed or cancelled), unknown types, the `[DONE]` marker |

| Finish reason | Anthropic `stop_reason` | OpenAI | Google `status` |
|---|---|---|---|
| `EndTurn` | `end_turn`, `stop_sequence` | `response.completed` with no function call | `completed` |
| `ToolUse` | `tool_use` | `response.completed` with at least one function call | `requires_action` with at least one function call |
| `OutputLimit` | `max_tokens`, `model_context_window_exceeded` | `response.incomplete`, reason `max_output_tokens` | `incomplete`, `requires_continuation` |
| `Refusal` | `refusal` (detail from `stop_details`) | a `refusal` content part; `response.incomplete` with `content_filter`; a failed response with a policy code | an error with a "generation blocked" code (`safety`, `recitation`, `language`, `prohibited_content`, `spii`, `blocklist`, `content_blocked`, the `image_*` codes) |
| `Error` | an `error` event; an unrecognised stop reason, `pause_turn` included | `response.failed`; an `error` event; an unrecognised incomplete reason | an `error` event; `failed`; `cancelled`; an unrecognised status |

OpenAI has no stop reason for tool use: the adapter reports `ToolUse` when the
response completed and contained a function call.

The event-stream parser accepts chunks split anywhere, both line endings,
comment lines (the keep-alive some providers send), and events and fields it
does not know. Every adapter ignores event types it does not know, as all three
providers ask. A stream that closes before the provider's final event is a
`Network` error if any event had arrived, and a `MalformedResponse` if none
had. Data that is not a JSON object, bytes that are not UTF-8, and an event
larger than 8 MiB are `MalformedResponse`.

A tool call whose arguments do not parse as a JSON object is never delivered as
complete. If the response then claims a normal end, the finish is a
`MalformedResponse` error. If the response was cut at the output limit or
refused, the incomplete call is expected and the provider's reason stands.

### 4.3 Strict reading

Everything a provider sends is read through one parser, `strict.rs`, which
refuses an object that repeats a key at any depth. It is the parser the policy
engine uses for manifests, standing approvals and its command line
(`tur_pax::strict` re-exports it), so the provider layer and the engine cannot
read one text in two ways.

| What is read strictly | Where | A repeated key gives |
|---|---|---|
| The data of every event of a stream | `provider::parse_data` | The stream ends there with `MalformedResponse` (`TUR-AI-PROVIDER-007`) |
| The argument text of every tool call | `ToolCalls::end` | The call is not delivered; a normal end of the response becomes `MalformedResponse` |

The first row is what makes a call whose arguments arrive as a JSON object
inside an event safe to take from the parsed event. Google sends such a call
when a `function_call` step starts with its arguments complete; the adapter
also accepts an Anthropic `tool_use` block that starts with a non-empty
`input`, which the streaming reference does not describe. In both cases the
text delivered with the call is the object written out again.

Where a provider sends the arguments twice, as fragments and as one text
(OpenAI's deltas and the finished item), both must read as the same
arguments. Fragments that are not one JSON object, or that say something else
than the complete text, make the call undeliverable.

**The effect on a task.** A response in which any call repeats a key is a
malformed response, and the orchestrator discards a malformed response whole
(addendum A section 10): none of its calls is decided or run, the unremarkable
ones included. This is stricter than rejecting the one call. It is the same
treatment a call with arguments that are not JSON has always had, and it
leaves no reading of the ambiguous text to anyone.

Before this, arguments were parsed with `serde_json`, which keeps the last
value of a repeated key, and the orchestrator worked round it by rebuilding
the argument text from the streamed fragments and parsing it again. A call
whose arguments arrived whole had no fragments, so every such call was
refused: with the Google adapter, no call that started with its arguments
could ever run.

## 5. Capabilities

Declared by each adapter and printed by `tur-pax-providers check`. The rows are
the table of section 10 of the design, with three additions the orchestrator
needs.

| Capability | Required | Anthropic | OpenAI | Google |
|---|---|---|---|---|
| Streaming conversation | Yes | yes | yes | yes |
| Tool calling with JSON Schema inputs | Yes | yes | yes | yes |
| Several tool calls in one response | – | yes | yes | yes |
| Image input | Yes | yes | yes | yes |
| Images in tool results | – | yes | yes | yes |
| Native computer control, offered by the provider | No | `computer_toolset_20260801` | `computer` | `computer_use` |
| Native computer control, implemented here | No | no | no | no |
| Model discovery | No | yes | yes | yes |
| Speech services | No | no | no | no |

The `openai-chat` family declares what the *format* can carry: streaming,
tool calling, several calls in one response and image input in a user
message, yes; images in tool results, no (the reference: "For tool
messages, only type `text` is supported", so a tool result with an image is
refused before anything is sent); native computer control, none. What one
model of one service can do is declared by its provider entry, not by the
adapter, and checked before tools are offered
([section 14.4](#144-declared-capabilities-are-checked)).

**Native computer control** is recorded, not implemented: screen control is out
of scope for this layer. What each provider documents today:

| Provider | Tool | Shape |
|---|---|---|
| Anthropic | `{"type": "computer_toolset_20260801"}` | No name and no display size. Optional `configs` to switch member tools off. The model's calls are ordinary `tool_use` blocks whose `name` is a member (`screenshot`, `left_click`, `type`, `zoom`, …) and which carry `"toolset_name": "computer"`; each result must repeat `toolset_name`. Several calls per turn are usual. No beta header. The earlier `computer_20251124` tool is rejected by current models on the provider's own API. |
| OpenAI | `{"type": "computer"}` | The model returns `computer_call` items with an ordered `actions` array (`click`, `type`, `screenshot`, …). The client returns a `computer_call_output` item with a `computer_screenshot`. |
| Google | `{"type": "computer_use", "environment": "browser" \| "mobile" \| "desktop"}` | Adds predefined functions. Options: `excluded_predefined_functions`, `enable_prompt_injection_detection`, `disabled_safety_policies`. |

All three can ask for several actions in one turn. Each action must still pass
the policy engine one by one.

**Model discovery**: `list_models` asks a provider for the models a key may
use ([section 15.4](#154-the-models-of-a-provider)). What it names is shown
beside the model list; model names remain configuration, and nothing a
provider says about capabilities or prices is taken.

## 6. Errors

Every failure is a `ProviderError` with one of these kinds. The code is stable
and the message is what the user is shown. An error may also carry the
provider, the HTTP status, the retry delay the provider gave, and a detail,
which is the provider's own message cut to 300 characters.

| Code | Kind | Message | Retried |
|---|---|---|---|
| `TUR-AI-PROVIDER-001` | Authentication failed | The provider did not accept the API key, or the key is not permitted to do this. | No |
| `TUR-AI-PROVIDER-002` | Rate limited | The provider is limiting requests for this key. Try again in a moment. | Yes |
| `TUR-AI-PROVIDER-003` | Overloaded | The provider is overloaded or temporarily unavailable. | Yes |
| `TUR-AI-PROVIDER-004` | Request rejected | The provider rejected the request. | No |
| `TUR-AI-PROVIDER-005` | Refusal | The model declined to respond to this request. | No |
| `TUR-AI-PROVIDER-006` | Network | The provider could not be reached, or the connection was lost. | Yes |
| `TUR-AI-PROVIDER-007` | Malformed response | The provider sent a response that could not be understood. | No |
| `TUR-AI-PROVIDER-008` | Timed out | The provider did not respond in time. | Yes |
| `TUR-AI-PROVIDER-009` | Quota exhausted | The provider account has no remaining credit or quota. | No |
| `TUR-AI-PROVIDER-010` | Spend limit reached | The spending limit set for this task was reached. | No |
| `TUR-AI-PROVIDER-011` | Configuration invalid | The provider configuration is not valid. | No |
| `TUR-AI-PROVIDER-012` | Key unavailable | No usable API key is available for this provider. | No |

How the providers' errors map:

| Kind | Anthropic | OpenAI | Google |
|---|---|---|---|
| Authentication failed | 401 `authentication_error`, 403 `permission_error` | 401, 403 | 401 `authentication`, 403 `permission_denied` |
| Rate limited | 429 `rate_limit_error` | 429 (`slow_down`, `rate_limit_exceeded`) | 429 `rate_limit_exceeded`, `too_many_requests` |
| Overloaded | 500 `api_error`, 529 `overloaded_error`, other 5xx | 500, 503 `server_is_overloaded`, other 5xx | 500 `api_error`, 503 `service_unavailable` |
| Request rejected | 400 `invalid_request_error`, 404, 409, 413, other 4xx | 400, 404, other 4xx | 400 `invalid_request`, `failed_precondition`, 404, other 4xx |
| Timed out | 504 or `timeout_error`; a client timeout | 408, 504; a client timeout | 504 `deadline_exceeded`; a client timeout |
| Quota exhausted | 402 `billing_error`; 429 with `details.error_code` `enforced_spend_limit_reached`; 400 whose message starts "You have reached your specified" | 429 `insufficient_quota`, `credit_balance_exhausted` | 402 `payment_required`, 429 `quota_exceeded` |
| Malformed response | – | – | the "generation error" codes, such as `malformed_function_call` |

A status the table does not name is classified by its class: 401 and 403 as
authentication, 402 as quota, 408 and 504 as timeout, 429 as rate limit, 5xx as
overloaded, anything else as rejected.

**Retries.** Only the four kinds marked above are retried, and only before any
of the response has been delivered. A request is sent at most
`retry.max_attempts` times. If the provider gave a `retry-after`, that delay is
used; otherwise the delay starts at `retry.initial_delay_ms` and doubles, with
jitter, up to `retry.max_delay_ms`. If the provider asks for a longer delay
than that ceiling, the request is not retried and the error carries the delay,
so that the orchestrator can tell the user when to try again. A rejected
request, an authentication failure, an exhausted quota and a refusal are never
retried. Redirects are not followed.

A quota problem is kept apart from a rate limit because the providers report
both as 429 and only one of them clears by waiting. Anthropic documents that
its spend-cap 429 has no `retry-after` and "keeps failing until access
resumes"; OpenAI documents that retrying a quota error does not restore access.

## 7. Untrusted content

Rule (design sections 2 and 7): content is data, not commands. Only the user's
request sets the task. This layer supports the rule in one way: it gives the
orchestrator exactly one place for untrusted content, the tool result, and it
sends a tool result only in the position the provider documents for tool
output. It never concatenates tool output into the system text or into a user
text.

| Provider | Where untrusted content goes | What the provider says |
|---|---|---|
| Anthropic | A `tool_result` block, in the user turn that follows the call | "Keep untrusted content inside `tool_result` blocks rather than `system` prompts or plain user `text` blocks." The same page warns that content from outside sources may carry instructions (indirect prompt injection). |
| OpenAI | A `function_call_output` item, tied to its call by `call_id` | The function-calling guide defines this item as the place for a function's output. No statement about untrusted content was found in the pages read. |
| Google | A `function_result` step, tied to its call by `call_id` and `name` | The function-calling guide defines this step as the place for a function's result. No statement about untrusted content was found in the pages read. The computer-use tool has an `enable_prompt_injection_detection` option, which this layer does not use. |

The conformance suite checks this for each adapter: a tool result containing an
instruction appears in the request exactly once, in that position, and the
system field is unchanged.

Limits of this, stated plainly:

- A provider marking tool output as such is not a security boundary. A model
  can still be misled by what a tool returns. The safeguards are outside the
  model: the policy engine and the trusted confirmation path.
- The layer cannot tell trusted text from untrusted text. If the orchestrator
  puts file content in a user text part, it is sent as user text. The rule for
  the orchestrator is: anything that did not come from the person goes in a
  tool result.
- Text the model itself produced (assistant text, tool-call arguments) is sent
  back as the model's own turn, as every provider requires.

## 8. Secrets

- The key is an `ApiKey`. `Debug` and `Display` print `[redacted]`. The type
  implements neither `Clone` nor `Serialize`, and its memory is overwritten when
  it is dropped.
- A key arrives through the `KeySource` trait. `EnvKeySource` reads an
  environment variable and is for development only. The session service in
  `tur-pax` takes the key from the Secret Service (`tur-pax/src/keyring.rs`,
  [pax-service.md](pax-service.md), section 5), holding it as this crate
  does: `is_usable_key` checks the form of a text without keeping it, and
  `Zeroizing` is re-exported so that the bytes read from the store are
  overwritten too.
- The key is sent in a header, never in a URL or a body. The header value is
  marked sensitive for the HTTP client.
- A base URL must be `https`, except for a loopback host, so that a key is not
  sent unencrypted over a network.
- The configuration format has no field for a key, and rejects unknown fields.
- Errors never contain the HTTP client's own error text, only a fixed
  description. A provider's error message is included as the detail, after every
  occurrence of the key has been removed from the response body or the event
  data it came from.
- A test runs every kind of outcome against every adapter with a known key,
  against a server that repeats the key in its error messages, renders every
  event and error with `Debug` and `Display`, and asserts that the key is in
  none of them. It also asserts that it saw every error kind and every event
  type.

Limits: the HTTP client copies the header value into its own buffers, which
are not overwritten. A key held in the environment of a process (the
development source) is readable by other processes of the same user.

## 9. Privacy: what each provider keeps

Section 8 of the design says what leaves the machine. Two of the three
interfaces also keep it by default, which the design did not anticipate.

| Provider | Default | What the adapter does |
|---|---|---|
| Anthropic | The Messages API is stateless: the conversation is sent with each request. Retention is governed by the account's terms. The reference material bundled with the development tooling says that some current models are not available to accounts with zero data retention; this was not checked against the provider's own pages. | Nothing to switch off. |
| OpenAI | A response is stored: `store` "defaults to true", and stored data is kept "for at least 30 days". | Sends `store: false` on every request and carries the whole conversation each time. Reasoning items come back with `encrypted_content` in this mode and are sent back as opaque parts. |
| Google | An interaction is stored (`store=true`): 55 days on the paid tier, 1 day on the free tier. | Sends `store: false` on every request and carries the whole conversation each time, with the model's `thought` steps as opaque parts, as the documentation requires for stateless use. |

`store: false` is what the documentation offers. It is not a statement about
what a provider logs for abuse monitoring or under its terms. The providers'
terms of use and data-retention terms were not reviewed for this work; ADR-0004
asks for that when an adapter is written, and it is still to be done before the
consent text of section 8 is written.

## 10. Configuration, spend and the command-line tool

**Configuration.** `schemas/pax-provider-config.schema.json`; the loader
`ProviderConfig::from_json` is authoritative. Fields: `schema_version`,
`provider`, `base_url` (optional), `model`, `max_output_tokens`, `timeouts`,
`retry`, `prices` (optional), `task_limit` (optional), `generation`
(optional, section 15). `model` and
`max_output_tokens` are required and have no default. There are no default
prices. A test checks that no source file of the crate contains a model name.

`base_url` is how the tests point an adapter at the fake server, and how a
compatible gateway can be used. The `openai-chat` family needs one; it also
reads `send_back_reasoning` (default `false`), which no other family
accepts. A service that takes no key is reached with
`connect_without_key`, which only the `openai-chat` family allows; no
credential header is sent then.

**Spend.** A `SpendMeter` adds up the usage of every request of a task and, if
prices are configured, converts it to a cost. A stream read through
`SpendMeter::meter` is cut, and finishes with `TUR-AI-PROVIDER-010`, when the
task passes its limit; a task already over its limit sends nothing more.

What this can and cannot do: the meter acts on the usage the provider reports.
Anthropic reports input tokens at the start of a response and output tokens at
the end; OpenAI and Google report both at the end. So the meter stops a task
between responses, not in the middle of one. A single response is bounded by
`max_output_tokens`, which is why that field is required. The cost is an
estimate for display; the provider's invoice is the authority. The per-day
limit of the design is kept by the session service in `tur-pax`
([pax-service.md](pax-service.md), section 9): this layer has no state
across tasks. Prices are configuration there too, from a model list.

**Command line.**

```text
tur-pax-providers check --config <file> [--ping] [--probe-tools]
                        [--key-env <VARIABLE> | --no-key]
```

`tur-pax run` and `tur-pax chat` take a configuration file
(`--provider-config`), or a provider of the model list by its identifier
(`--provider ID --model NAME [--models FILE]`), whose entry then gives the
family, the address and the key source as in the session service. The key
comes from the environment (`--key-env`, `TUR_PAX_API_KEY` by default),
from the entry's key file (`runtime_key_file`, the `local` preset), or not at
all: for an entry that takes none, or with `--no-key` (`openai-chat` only).
The provider's identifier is written to the audit trail as
`TUR_AI_PROVIDER_ID` beside the family in `TUR_AI_PROVIDER`
(pax-orchestrator.md, section 7).

`check` validates a configuration and prints the endpoint, the API and the
capabilities the adapter declares. It makes no network call. With `--ping` it
reads the key from an environment variable (`TUR_PAX_API_KEY` by default) and
sends one request of at most 64 output tokens, which the provider bills. With
`--probe-tools` it sends the tool probe of section 14.4 (at most 512 output
tokens, billed). `--no-key` sends no key, for a server that takes none
(`openai-chat` only). Exit status: 0 valid (and the requests succeeded, the
probe verified), 1 invalid configuration, 2 usage error, 3 ping failed or no
adapter could be built, 4 the probe did not show that the model calls tools.

## 11. Known gaps

- **Not run against any real provider.** See the top of this document.
- **Native computer control** is not implemented.
- **Reasoning is not shown.** Thinking summaries are kept inside opaque parts
  and are not surfaced as events.
- **Server-side tools** (web search, code execution, provider-side MCP) are not
  requested, and their blocks and items are ignored if they appear.
- **Provider-side MCP is deliberately not used.** ADR-0008 makes MCP the tool
  protocol with `tur-pax` as the gateway. All three providers also offer a
  connector that lets the provider call an MCP server itself. That would bypass
  the policy engine, so the adapters send tools as plain function definitions
  built from the manifest. The translation is three field names per provider.
- **Anthropic refusal fallback** (`fallbacks`, a beta) is not requested. A
  refusal is reported as a refusal.
- **OpenAI's `phase` label is not carried.** The API may label an assistant
  message `commentary` or `final_answer`, and the SDK's documentation asks for
  the label to be sent back on later requests for some models, saying that
  dropping it can degrade performance. The neutral model has no place for it,
  so assistant text is sent back without it.
- **Anthropic eager tool-input streaming** (`eager_input_streaming`) is not
  set, because a gateway behind a custom base URL may reject the field. Tool
  inputs are buffered by the API until each parameter is complete.
- **History must stay append-only.** Anthropic checks that a thinking block is
  sent back in the conversation that produced it; a request whose earlier turns
  were edited is rejected on current models. An orchestrator that compacts or
  rewrites history must drop the opaque parts of the turns it changes.
- **Resuming a cut stream** is not attempted. The failure is reported and the
  orchestrator decides.
- **A request cannot be cancelled while it waits to retry.** Dropping the
  stream closes the connection once the wait is over.
- **Line endings.** The event-stream parser does not treat a bare carriage
  return as a line ending. None of the three providers sends one.
- **`retry-after` as a date** is not read; only the seconds form is.
- **Tool schemas are sent unchanged.** Google documents that it supports "a
  subset of the OpenAPI schema" for parameters. Whether every keyword of the
  manifest's subset, `additionalProperties` among them, is accepted was not
  verified. The same holds for every compatible service.
- **The compatible family** has its own list in
  [section 14.7](#147-what-is-not-verified-or-not-built).

## 12. What was verified, and what was not

All pages were read on **2026-11-12**.

### Anthropic

| Verified | Source |
|---|---|
| Event types and order; `ping`; the `error` event; unknown event types to be ignored; `text_delta`, `input_json_delta`, `thinking_delta`, `signature_delta`; cumulative usage in `message_delta`; raw stream examples for text and tool use | <https://platform.claude.com/docs/en/build-with-claude/streaming> |
| Status codes and error types; the error body; `request-id`; which errors the SDK retries; `retry-after` | <https://platform.claude.com/docs/en/api/errors> |
| `retry-after` in seconds; the spend-cap 429 and its `details.error_code`; the 400 for a user-set spend limit | <https://platform.claude.com/docs/en/api/rate-limits> |
| The list of stop reasons | <https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons> (the first 100,000 characters of the page) |
| The refusal response, `stop_details`, its categories; a refusal may arrive mid-stream and partial output is to be discarded | <https://platform.claude.com/docs/en/build-with-claude/refusals-and-fallback> |
| `tool_use` and `tool_result` blocks; results first in the user turn; `is_error`; images in results; the warning about untrusted content | <https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls> |
| Thinking blocks must be passed back unchanged with tool use; `redacted_thinking`; omitting `thinking` is valid | <https://platform.claude.com/docs/en/build-with-claude/thinking> |
| The image block; media types; size limits | <https://platform.claude.com/docs/en/build-with-claude/vision> |
| API version `2026-10-21`; sampling parameters rejected on it; the `x-api-key` header; the computer toolset | The curl examples on the pages above, and the reference material for the Claude API bundled with the development tooling, which cites <https://platform.claude.com/docs/en/api/migration-2026-10-21> |

Not verified: where `stop_details` sits in a streamed refusal (the adapter
reads it from the `delta` of `message_delta` and from the event itself); the
exact events of a stream with a `redacted_thinking` block.

### OpenAI

`platform.openai.com/docs` redirects to `developers.openai.com/api/docs`. The
API reference pages there are several megabytes each and were read only in
part, so the official SDK's type definitions were used for field names.

| Verified | Source |
|---|---|
| "Responses is recommended for all new projects"; `store`; replaying reasoning items when stateless | <https://developers.openai.com/api/docs/guides/migrate-to-responses> |
| Function tools; `function_call` and `function_call_output` items; `call_id`; the streaming events for function calls with examples; reasoning items must be passed back with tool outputs | <https://developers.openai.com/api/docs/guides/function-calling> |
| Streaming over server-sent events; events identified by `type` | <https://developers.openai.com/api/docs/guides/streaming-responses> |
| Status codes; `Retry-After` on 429 and 503; `slow_down`, `server_is_overloaded`; quota errors do not clear by retrying | <https://developers.openai.com/api/docs/guides/error-codes>, <https://developers.openai.com/api/docs/guides/rate-limits> |
| `encrypted_content` on reasoning items in stateless mode | <https://developers.openai.com/api/docs/guides/reasoning> |
| The `computer` tool, `computer_call`, `computer_call_output` | <https://developers.openai.com/api/docs/guides/tools-computer-use> |
| Input item shapes: `input_text`, `input_image` with a data URL, `function_call_output` with a list | <https://developers.openai.com/api/reference/resources/responses/methods/create> (the first 100,000 characters) |
| Event types and their fields; the `error` event; `incomplete_details.reason`; the error codes of a failed response; usage fields; the refusal part; `store` "defaults to true"; `detail` required on `input_image`; a message as `{role, content}` | The type definitions of the official Python SDK, <https://github.com/openai/openai-python>, `src/openai/types/responses/`, branch `main` |

Not verified: the raw framing of the stream (whether each event has an
`event:` line; the adapter reads the `type` inside the data, so either works);
the exact JSON nesting of an HTTP error body (the guide names the fields
`message`, `type`, `param`, `code` under `error`); the `type` and `code` of a
401, which is classified by status.

### Google

`ai.google.dev/gemini-api/docs` and `ai.google.dev/api` redirect to
`aistudio.google.com/docs` and `aistudio.google.com/api-reference`.

| Verified | Source |
|---|---|
| The Interactions API is generally available and recommended; `generateContent` is legacy and supported; storage, retention and `store=false` | <https://aistudio.google.com/docs/interactions> |
| Event types and order; raw stream examples, the `[DONE]` marker included; `arguments_delta`; `thought_signature`; `requires_action`; the `error` event; unknown events to be ignored | <https://aistudio.google.com/docs/streaming> |
| Function tools; `function_call` and `function_result`; stateless function calling with `store: false` and the model's steps sent back "exactly as received" | <https://aistudio.google.com/docs/function-calling> |
| Error codes and statuses; the "generation blocked" and "generation error" codes; the error body; the error event | <https://aistudio.google.com/docs/api-errors> |
| Thought steps and signatures | <https://aistudio.google.com/docs/thought-signatures>, <https://aistudio.google.com/docs/thinking> |
| The image content block | <https://aistudio.google.com/docs/image-understanding> |
| The `computer_use` tool | <https://aistudio.google.com/docs/computer-use> |
| Request fields, `generation_config.max_output_tokens`, step types, statuses, usage fields, delta types | <https://aistudio.google.com/api-reference/interactions> |

Not verified:

- **How a blocked generation arrives in a stream.** The documentation lists the
  codes and the error event but does not say which carries a block. The adapter
  accepts a blocked code in an `error` event, in the `errors` of a failed
  interaction, and in a failed status update. The script in the conformance
  suite uses the `error` event.
- **A `retry-after` header.** None is documented. The transport honours one if
  it is sent.
- **The delta for function arguments.** The reference and the raw examples say
  type `arguments_delta` with the text in `arguments`; one SDK sample on the
  streaming page reads `partial_arguments` from a delta of type `arguments`.
  The adapter accepts both.
- **A tool result with no content.** It is sent as an empty `result` list.
  Whether the API accepts that was not verified.
- **Whether thought tokens are part of `total_output_tokens`.** The usage
  example on the streaming page adds up only if they are counted beside it, so
  the adapter adds them.
- **`generateContent`** was not implemented and its current wire format was not
  verified.

## 13. Tests

All tests run in CI and nowhere else. None contacts a provider.

| Test | What it shows |
|---|---|
| `tests/conformance.rs` | The provider conformance suite of sections 10 and 12 of the design. Twenty-four neutral-level tests, each run against all four adapter families; the last two are the generation settings of section 15. |
| `tests/listing.rs` | The list of models of each family: request, key header, every page, a list that is not one (section 15.4). |
| `tests/openai_chat.rs` | What the compatible family must do beyond that (section 14.6). |
| `tests/secrets.rs` | The key is in no error, event or debug rendering, and is sent only in the documented header. |
| `tests/config_and_schema.rs` | The schema and the loader agree; the examples load; no model name is compiled in. |
| `tests/cli.rs` | The built binary: `check` makes no network call; `--ping` makes one; the key is never printed. |
| Unit tests in each module | The request body of each adapter; error classification; the event-stream parser at every chunk boundary; retry and backoff; tool-call assembly; configuration rules. |

The conformance suite uses a fake server in `tests/common/`: the standard
library's `TcpListener`, one thread, one connection at a time. It answers each
request with the next scripted reply, sending event streams with chunked
transfer encoding in pieces of seven bytes, and records what it was sent. The
scripts are in `tests/common/{anthropic,openai,google,openai_chat}.rs`, one
set per family, written by hand from the documentation listed above and in
section 14; none is a recording of a real service.

| Conformance test | Asserts, for each adapter |
|---|---|
| Plain answer | The text, the usage and a normal finish; the request goes to the documented path with the key in the documented header and only the documented fields; a keep-alive comment and an unknown event type in the stream are ignored |
| Tool call with split arguments | Start, at least two argument fragments that add up to the arguments, end; the reasoning content comes first, unchanged; the tools were offered |
| Two tool calls in one turn | Both calls, in order, with distinct identifiers; one has no arguments |
| A repeated key in streamed arguments | The call is announced but never delivered; the finish is `MalformedResponse`, `TUR-AI-PROVIDER-007`; the collected turn holds no call; not sent again |
| A repeated key in arguments delivered whole | The same, with the key repeated inside a nested object: in the `input` of the block (Anthropic), in the `arguments` string of the finished item (OpenAI), in the `arguments` object of the step (Google) |
| Arguments delivered whole | One start, no fragment, one complete call with nested arguments; the text delivered with the call, read strictly, is the call's arguments |
| Refusal | A `Refusal` finish with `TUR-AI-PROVIDER-005`; not retried |
| Rate limit, then success | The second attempt succeeds; the same body was sent twice |
| Rate limit beyond the ceiling | Not retried; the error carries the provider's delay |
| Overload | Retried up to the configured number of attempts, then reported |
| Authentication failure | `TUR-AI-PROVIDER-001`; sent once |
| Rejected request | `TUR-AI-PROVIDER-004`; sent once |
| Stream cut mid-way | The text so far, then a `Network` error; not sent again |
| Garbage bytes | Five kinds of garbage, each a `MalformedResponse` |
| Error event mid-stream | The text so far, then an `Overloaded` error |
| Output limit | An `OutputLimit` finish |
| Silence | A `TimedOut` error |
| Unreachable provider | A `Network` error |
| Tool result goes back as tool output | The follow-up request has the reasoning content unchanged, the tool result in the provider's tool-output position, the untrusted text exactly once, and the system field unchanged |
| Spend limit | The stream is cut with `TUR-AI-PROVIDER-010`; the next request is not sent |
| Spend under the limit | Usage and cost add up over two requests |
| Capabilities | The adapter declares the required rows; nothing is sent |
| Generation settings reach the wire | The family's own fields for every setting it takes, at the exact JSON path; every setting it has no place for refused by the loader; nothing sent before |
| Nothing set, nothing sent | No temperature, top-p, top-k, stop, thinking, effort, reasoning or template field in the request |

For the compatible family, "the reasoning content unchanged" in the tool
round trip reads: the reasoning is kept as an opaque part and, with the
default configuration, *not* sent back; "arguments delivered whole" reads:
one delta that holds all of the argument text, the only form Chat
Completions has.

## 14. The OpenAI-compatible family and its presets

### 14.1 Mapping

Sources: OpenAI's Chat Completions reference
(<https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create.md>,
<https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events.md>),
its function-calling guide in chat mode
(<https://developers.openai.com/api/docs/guides/function-calling?api-mode=chat>),
the type definitions of the official Python SDK (`src/openai/types/chat/`,
`completion_usage.py`, `_streaming.py`), and DeepSeek's API documentation
(<https://api-docs.deepseek.com>), all read on 2026-11-12.

| Neutral | Chat Completions |
|---|---|
| Endpoint | `POST <base URL>/chat/completions` |
| Key | `Authorization: Bearer`; none at all for a service whose entry says `key_required: false` |
| `system` | The first message, `{"role": "system"}` |
| `max_output_tokens` | `max_tokens` (the reference calls it deprecated in favour of `max_completion_tokens`; DeepSeek and Ollama document `max_tokens`, and Ollama does not list the other) |
| Streaming | `stream: true`, `stream_options: {"include_usage": true}` |
| Tool | `{"type": "function", "function": {name, description, parameters}}`; `tool_choice: "auto"` when tools are offered (the documented default; a forced choice is refused by DeepSeek in thinking mode). `strict` and `parallel_tool_calls` are not sent |
| User text and images | One `user` message: a string, or text and `image_url` parts with a `data:` URL when there is an image |
| Assistant text and calls | One `assistant` message: `content` (null when there is only a call), `tool_calls` with `arguments` as a JSON string |
| Tool result | A `tool` message `{tool_call_id, content}` for each result, placed before the user's text, text only; `is_error` is not sent (no field for it) |
| Reasoning (opaque) | Kept from `reasoning_content` or `reasoning`; sent back in the same field on the assistant message only with `send_back_reasoning` |

| Neutral event | Chat Completions |
|---|---|
| `TextDelta` | `choices[0].delta.content` (a string; in a list of parts, only `text` parts) |
| `ToolCallStart` | The first delta of a call that names it; argument text that came before the name waits for it |
| `ToolCallArgumentsDelta` | `delta.tool_calls[].function.arguments`, by `index` |
| `ToolCallEnd` | Every open call ends when the stream ends |
| `Opaque` | The reasoning so far, delivered before the text or call that follows it |
| `Usage` | Any chunk with a `usage` object (OpenAI: a last chunk with no choices; DeepSeek: the last chunk with the finish; OpenRouter: a chunk that repeats the finish) |
| `Finish` | `data: [DONE]`, with the first `finish_reason` seen |

| `finish_reason` | Neutral |
|---|---|
| `stop`, `tool_calls` | `ToolUse` if a call was delivered, `EndTurn` otherwise; a `refusal` text makes it `Refusal` |
| `length` | `OutputLimit`: the task loop discards the response whole |
| `content_filter` | `Refusal` (`TUR-AI-PROVIDER-005`) |
| `insufficient_system_resource`, `aborted` (DeepSeek), `error` (OpenRouter) | `Error`, `Overloaded` |
| `function_call`, anything else, none before `[DONE]` | `Error`, `MalformedResponse` |

A chunk with an `error` object (the reference: "An error after streaming
begins can arrive as a stream event"; OpenRouter documents the shape) ends
the stream with that error; a status number in its `code` is read as one.
Errors before the stream use the statuses of section 6 and the `error`
object's `code` and `type`; DeepSeek's 402 "Insufficient Balance" is
`QuotaExhausted`, its 422 `RequestRejected`. A stream that closes before
`[DONE]` is a `Network` error.

**Choices.** One is asked for (no `n`); a choice with another index is
ignored.

**Calls without an index.** The reference requires `index`. A call that
comes without one is told apart by its `id`; a delta with neither belongs to
the last call.

### 14.2 Strict reading

As for every family (section 4.3): every chunk is parsed strictly; the
argument text of every call is read strictly when the stream ends; a call
that repeats a key at any depth, or whose text is not one JSON object, is
not delivered, and a normal end of the response is then
`MalformedResponse`, so the task loop discards the whole response. Three
more ways to say two things about one call do the same: a second, different
name; a second, different identifier (or one that arrives after the call was
started without one); arguments that are not text. A call that never gets a
name is not delivered either.

### 14.3 Usage and the limit of a day

Every request asks for usage in the stream. When a response finishes
without any, the adapter delivers an estimate, marked
(`Usage.estimated`, written `"estimated": true` wherever a usage is written
out): one token for every two bytes of the request as sent, without image
data; 3 000 tokens for each image; and the whole output limit. It is meant
to be high: a response nobody counted would otherwise cost nothing to the
task's limit and to the day's. The same estimate is delivered before an
error chunk and before a stream that is cut after something arrived. A sum
with an estimate in it is an estimate. Groq and llama.cpp's server do not
document usage in streams, so this is the path their responses are
expected to take.

### 14.4 Declared capabilities are checked

A provider entry declares `tool_calling`, `streaming_tool_calls`,
`image_input` and `context_tokens`, for the provider and, overriding it,
for each model. The session service acts on them:

- A model that declares no tool calling is offered **no tools and no
  command**. The task runs as a conversation and the person is told, as a
  notice of the task: "This model cannot call tools, as its provider's entry
  declares, so P@x can talk with it but cannot act with it: no tool or
  command is offered to it." `GetStatus` (`abilities`) and `ListModels`
  (`can_act`, `can_see`, `notices`) say it before the person chooses.
- A model that declares no image input is marked `can_see: false` with the
  sentence "This model takes no images, as its provider's entry declares, so
  P@x will not offer it control of the screen." This build has no screen
  control to withhold; the flag is there for when it has.
- Nothing declared: tools are offered, as before.

**The probe.** `tur_pax_providers::probe_tool_calling` (and
`tur-pax-providers check --probe-tools`) sends one request that offers one
harmless tool, `tur_probe`, and asks for it to be called; the call is
looked at, never performed. Verdicts: `verified`, `not_called`,
`inconclusive` (cut at the output limit, refused, failed). It is billed and
is made only when asked. The settings page that would run it when a person
adds a model does not exist yet; the command line does.

### 14.5 Reasoning

DeepSeek, xAI and llama.cpp's server stream the model's reasoning in
`reasoning_content`; Ollama, Groq and OpenRouter in `reasoning`. Decided
here:

- It is **never text of the answer**: it produces no `TextDelta`, is not in
  the answer, the transcript, the activity or the trail, and is never shown
  as P@x's words.
- It is **never an instruction**: it is kept as an opaque part of the
  assistant turn, which the orchestrator does not read.
- It is **sent back only where the service's documentation requires it**:
  `send_back_reasoning: true` in the configuration, set by the DeepSeek
  preset only. DeepSeek's thinking-mode guide
  (<https://api-docs.deepseek.com/guides/thinking_mode>): "for requests
  carrying the tools parameter, the reasoning_content must be fully passed
  back to the API in all subsequent requests — even for turns where the
  model did not perform a tool call. If your code does not correctly pass
  back reasoning_content, the API will return a 400 error." It goes back in
  the field it came in. Every other service: kept, never sent.
- A list of content parts with a `thinking` part (Mistral's reasoning
  models with `reasoning_effort`, which P@x does not set) contributes only
  its `text` parts; the thinking part is dropped, not sent back. Mistral
  asks for the whole message to be replayed in that mode; it is not used.

### 14.6 Tests

`tests/openai_chat.rs` (hand-written scripts in
`tests/common/openai_chat.rs`; no real service):

| Test | Shows |
|---|---|
| `reasoning_is_never_text_and_goes_back_only_where_the_service_requires_it` | DeepSeek's `reasoning_content`: not in the text; an opaque part first; sent back in the next request only with `send_back_reasoning` |
| `the_reasoning_field_of_other_servers_is_kept_the_same_way` | Ollama's `reasoning` and its whole-call delta |
| `tool_call_text_cut_inside_an_escape_and_bytes_cut_inside_a_character` | Argument text cut inside `\"` and `ö`; the stream sent one byte at a time |
| `a_repeated_key_split_across_deltas_at_any_depth_discards_the_response` | A nested repeated key across two deltas, beside an unremarkable call |
| `a_service_that_sends_no_usage_is_counted_at_a_marked_estimate` | The estimate, its figures, the task limit acting on it, a cut stream counted |
| `a_reported_usage_is_taken_as_it_is_wherever_the_service_puts_it` | OpenRouter's repeated finish and its usage chunk |
| `the_endings_a_service_can_give` | `content_filter`, an error chunk with status 200, `insufficient_system_resource`, no `[DONE]` |
| `a_rate_limit_with_retry_after_is_waited_for_then_succeeds` | A 429 with `retry-after: 1` is waited for |
| `a_server_that_takes_no_key_is_sent_no_credential` | No `Authorization` header; the companies' families refuse it |

Also: the conformance suite (all 22 tests, `openai_chat` module), the key
tests of `tests/secrets.rs`, the command line (`tests/cli.rs`: ping,
probe, `--no-key`), unit tests in `openai_chat.rs` and `probe.rs`; in
`tur-pax`, `tests/wires.rs` runs the loop through all four families
(including "nothing above class 1 runs through any adapter"), and
`tests/service.rs` the service with a listed compatible provider, a model
without tool calling and a keyless local server; the `pax` workflow runs
scenarios 18 and 19 through the family.

### 14.7 What is not verified or not built

- **No compatible cloud service has been called.** llama.cpp's `llama-server` has, through the `local` preset (`docs/reports/pax-5.md`). The DeepSeek preset can be run
  by hand with `pax-live` once the secret `DEEPSEEK_KEY` exists.
- **Streamed tool calls** are shown in documentation by OpenAI, DeepSeek,
  Groq and Together. Mistral's FAQ says the delta structure is the same but
  shows no tool-call chunk; xAI sends each call whole in one chunk; Ollama's
  documentation does not say (its source sends each call whole);
  llama.cpp's README does not say.
- **Usage in streams** is documented by OpenAI, DeepSeek, xAI (two
  conflicting descriptions), OpenRouter (always), Together (a usage-only
  chunk) and Ollama (`include_usage` ticked); Mistral's pages conflict;
  Groq and llama.cpp document none: the estimate applies.
- **`tool_choice`**: Ollama lists it as not supported, llama.cpp does not
  document it for chat; `"auto"` is sent anyway, as the documented default.
  Whether either rejects it was not verified.
- **`max_tokens`** is deprecated by OpenAI in favour of
  `max_completion_tokens`; which compatible services accept which was not
  verified beyond DeepSeek and Ollama (`max_tokens`).
- **Images in tool results** are refused by this family although DeepSeek
  documents them; the format's reference does not.
- **Retry-After** is documented by Groq (429), OpenRouter (429, 503, some
  402) and Mistral (429); not by DeepSeek, xAI or Together. Backoff applies.
- **Service-specific extras** are not used: DeepSeek's `thinking` object
  (its `reasoning_effort`, which the same page documents, is: section 15),
  `user_id` cache isolation and beta `strict` tools; Mistral's
  `prompt_mode`; Groq's `reasoning_format`; OpenRouter's routing and `zdr`.
- **The settings page** that adds a provider, shows its terms and runs the
  probe is the desktop's (D-19); the interface gives it the data.

### 14.8 Presets

In `services/tur-pax/data/models.json`. Every page was read on
**2026-11-12**. Prices are **examples** in USD per million tokens (input /
output / cached input) and change without notice; the consent text of each
shows its data terms as summarised in the file, with the date.

| Preset | Base URL | Models (context; image input) | Example prices | Verified in | Data terms read from |
|---|---|---|---|---|---|
| `deepseek` | `https://api.deepseek.com` | `deepseek-flash` (1M; yes), `deepseek-v4-pro` (1M; no) | peak rates: 0.30 / 1.20 / 0.006 and 1.32 / 3.96 / 0.044; off-peak half; shown with `$` and no currency named | <https://api-docs.deepseek.com/quick_start/pricing>, `/api/create-chat-completion`, `/guides/tool_calls`, `/guides/thinking_mode`, `/quick_start/error_codes`, `/quick_start/rate_limit`, `/updates` | Privacy policy (updated 2026-02-10) <https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html>, terms of use (2026-03-27), open platform terms (2026-04-29) |
| `mistral` | `https://api.mistral.ai/v1` | `mistral-small-2603` (256k; yes), `mistral-large-4` (1M; yes) | 0.15 / 0.60; 0.70 / 2.10 / 0.07 | <https://docs.mistral.ai/resources/migration-guides.md> ("The Mistral Chat Completions API follows the same request structure as OpenAI"), `/inference/pricing.md`, model cards | Privacy policy <https://legal.mistral.ai/terms/privacy-policy>, commercial terms, regional inference, zero data retention |
| `xai` | `https://api.x.ai/v1` | `grok-4.3` (1M; yes), `grok-4.7` (500k; yes) | under 200K-token prompts: 1.25 / 2.50 / 0.20; 2.00 / 6.00 / 0.50 | <https://docs.x.ai/developers/model-capabilities/legacy/chat-completions.md> (a "legacy endpoint"), `/tools/function-calling.md`, `/pricing.md` | Security FAQ in its documentation <https://docs.x.ai/developers/faq/security.md>; its legal pages were not read |
| `groq` | `https://api.groq.com/openai/v1` | `openai/gpt-oss-120b` (131 072; no), `qwen/qwen3.8-27b` (preview; 131 072; yes) | 0.15 / 0.60; 0.80 / 4.00 | <https://console.groq.com/docs/openai>, `/tool-use`, `/models`, `/vision` | <https://console.groq.com/docs/your-data>, services agreement (2026-06-22) |
| `openrouter` | `https://openrouter.ai/api/v1` | `mistralai/mistral-small-2603` (262 144; yes), `x-ai/grok-4.3` (1M; yes) | 0.15 / 0.60; 1.25 / 2.50 (shown with `$`) | <https://openrouter.ai/docs/api_reference/overview.md>, `/parameters.md`, `/streaming.md` | Data collection, logging, provider logging and ZDR guides; its legal privacy page was not read |
| `together` | `https://api.together.ai/v1` | `openai/gpt-oss-120b` (131 072; no), `Qwen/Qwen3.5-9B` (262 144; yes) | 0.15 / 0.60; 0.17 / 0.25 | <https://docs.together.ai/docs/inference/openai-compatibility.md>, function-calling guides, `/serverless/models.md` | Privacy and security, zero data retention; its legal privacy page was not read |
| `ollama` | `http://127.0.0.1:11434/v1` | none listed: any model the person has | none: runs here | <https://docs.ollama.com/api/openai-compatibility.md>, `/faq.md` (binds 127.0.0.1:11434 by default) | "Runs on this computer"; no key |
| `llama-server` | `http://127.0.0.1:9931/v1` | none listed | none: runs here | `tools/server/README.md` of llama.cpp, branch `master` (`--port` "default: 9931", `--host` 127.0.0.1, `--api-key` "default: none"; function calling with `--jinja`, now enabled by default) | "Runs on this computer"; no key |
| `local` | `http://127.0.0.1:39281/v1` | `qwen3-4b`, `qwen3-vl-2b-instruct` (image input), `granite-4.0-1b`, `qwen3-0.6b`: the chat models of `tur-ai-local`'s catalogue that call tools (8 192, its default context; output limit 1 024, P@x's choice) | none: runs here | TUR's own `tur-ai-local` ([local-ai.md](local-ai.md), section 7): `serve` listens on 127.0.0.1:39281 and writes its access key to `$XDG_RUNTIME_DIR/tur-ai-local/api-key` | "Runs on this computer"; key from the file `tur-ai-local` writes |

Every cloud preset declares tool calling (from the pages above), and
DeepSeek alone sends its reasoning back. The `ollama` and `llama-server`
presets declare nothing about tools or images: that depends on the model
loaded and how the server was started, and the probe is the way to find out.
If `llama-server` is started with `--api-key`, its entry must be changed to
need a key.

**`local`: TUR's own local AI.** The preset for `tur-ai-local`, which starts
llama.cpp's `llama-server` for one model of its catalogue at a time
([local-ai.md](local-ai.md)). Its address is the one `tur-ai-local serve`
uses unless told another, `127.0.0.1:39281`, with `/v1` for the Chat
Completions path; `tur-ai-local status` reports the same port and base URL
(`base_url` in its state). Its models are the catalogue ids of the models
whose capabilities include `chat` and `tools`
(`services/tur-ai-local/catalogue/`), and a test holds the two lists
together (`settings::tests::the_local_preset_offers_the_catalogue_s_chat_models_that_call_tools`);
`qwen3-vl-2b-instruct` declares image input, the others none. It declares
tool calling, streamed tool calls and a context of 8 192 tokens, what
`tur-ai-local serve` gives a model unless told another. Its data terms are
`runs_locally`: the consent text says that what P@x sends stays on this
computer and no company receives it.

It **needs a key** (`key_required`, the default) and **does not keep it in the
password store**: `runtime_key_file: "tur-ai-local/api-key"` names the file,
relative to the session's runtime directory, that `tur-ai-local serve` writes
each time it starts a model (a new random 256-bit key, mode 0600). P@x reads
it for each task through `FileKeySource` (`tur-pax-providers/src/secret.rs`)
and `settings::runtime_key`: only for an address of this computer, only a
regular file that no one but its owner can read, the error naming the file
and never its content. Why not the keyring: the key changes with every start
of the engine, is worthless once the engine stops, and lives in the person's
own runtime folder, which is emptied at log-out; a copy in the password store
would be stale after the next start. A missing file means no model is
served: the task does not start (`TUR-AI-PROVIDER-012`) and `GetStatus` says
`file_missing` (pax-service.md, section 5). A model on the processor reads a
P@x request (instructions and tool definitions, thousands of tokens) for a
minute or more before it answers, so a provider whose address is this
computer is given ten minutes to begin a response (`response_ms` 600 000)
instead of two.

Proved in CI by the `local-ai` workflow (`pax` job): `tur-pax run --provider
local --model qwen3-4b` against `tur-ai-local` serving Qwen3 4B on llama.cpp's
own b9840 build, every component's tools and the read-only command tool
offered, in the real sandbox (`tests/pax/local_model.py`; the results are in
`docs/reports/pax-5.md`).

**Left out:** none of the five named candidates was left out; each
documents an OpenAI-compatible Chat Completions endpoint with tool calling.
Two kinds of statement were not settled: for Mistral, whether paid API use
is trained on by default (its policy and its terms read differently); for
xAI, OpenRouter and Together, only their documentation's data pages were
read, not their legal privacy policies. The consent text shows what was
read, with its date and source.

**The local AI service of ADR-0021** (`tur-ai-local`) is the preset
`local`, above.

## 15. Generation settings and model lists

Added on 13 November 2026 (issue #84). Every page below was read on
**2026-11-13**, except where a row names the Claude API reference bundled
with the development tooling (cached 2026-11-03), which cites the
platform pages listed. **No request was sent to any provider**: what each
family is sent is checked against a local server that records it
(`tests/conformance.rs`, `tests/listing.rs`).

### 15.1 What each family is sent

`Generation` (`src/generation.rs`) is part of a configuration
(`generation` in `schemas/pax-provider-config.schema.json`) and of every
request made from it. Only what is set is sent. A setting a family's
interface has no place for is **refused** by the loader
(`TUR-AI-PROVIDER-011`), never dropped in silence.

| Setting | Anthropic Messages (API `2026-10-21`) | OpenAI Responses | Google Interactions | Chat Completions (`openai-chat`) |
|---|---|---|---|---|
| temperature | refused | `temperature` | refused | `temperature` |
| top-p | refused | `top_p` | refused | `top_p` |
| top-k | refused | refused | refused | `top_k` (llama.cpp; a model of a service that lacks it does not declare it) |
| stop sequences | `stop_sequences` | refused | `generation_config.stop_sequences` | `stop` |
| effort `none` | `thinking: {"type": "disabled"}` | `reasoning: {"effort": "none"}` | refused | `reasoning_effort: "none"`, or with llama.cpp `chat_template_kwargs: {"enable_thinking": false}` |
| effort `minimal` | refused | `reasoning.effort` | `generation_config.thinking_level` | `reasoning_effort`, or `enable_thinking: true` |
| effort `low` to `xhigh` | `output_config: {"effort": …}` | `reasoning.effort` | `generation_config.thinking_level` | `reasoning_effort`, or `enable_thinking: true` |
| effort `max` | `output_config.effort` | `reasoning.effort` | refused | `reasoning_effort`, or `enable_thinking: true` |
| thinking budget | refused | refused | refused | `thinking_budget_tokens`, with `enable_thinking` only |
| output limit | `max_tokens` (thinking counts in it) | `max_output_tokens` (reasoning counts in it) | `generation_config.max_output_tokens` (thoughts count in it) | `max_tokens` |

| Family | Why, from its documentation | Sources |
|---|---|---|
| Anthropic | On API version `2026-10-21` "a Messages request that sends `temperature`, `top_p` or `top_k` returns a 400 error on every model". Effort is `output_config.effort`, `low` to `max` (no `minimal`); `budget_tokens` is removed on current models and adaptive thinking is the default; `{type: "disabled"}` is accepted by some models only (Claude Opus 5.5 and Sonnet 5.5 reject it, so the model list offers them no `none`). | The Claude API reference bundled with the development tooling, citing <https://platform.claude.com/docs/en/api/migration-2026-10-21>, <https://platform.claude.com/docs/en/build-with-claude/effort>, <https://platform.claude.com/docs/en/build-with-claude/extended-thinking> |
| OpenAI | `reasoning.effort` is `none`, `minimal`, `low`, `medium`, `high`, `xhigh` or `max`, "Not all reasoning models support every value", default `medium`; `temperature` 0 to 2 and `top_p`, "When reasoning effort is not `none`, remove `temperature`, `top_p`, and `top_logprobs`"; Responses has no `top_k` and no `stop`; `max_output_tokens` includes reasoning. | <https://developers.openai.com/api/reference/resources/responses/methods/create.md>, <https://developers.openai.com/api/docs/guides/reasoning>, <https://developers.openai.com/api/docs/guides/latest-model>, the SDK's `response_create_params.py` and `shared/reasoning_effort.py` |
| Google | The Interactions reference's `generation_config` lists `stop_sequences`, `thinking_level` (`minimal` to `xhigh`, per model), `thinking_summaries`, `max_output_tokens` and `seed`; `temperature`, `top_p` and `top_k` are not in it ("Gemini 4 and Gemini 3.6+ models ignore custom values …"; "In future model generations, supplying these parameters returns an HTTP 400 error"); `thinking_budget` is not in it, and "You cannot use both `thinking_level` and the legacy `thinking_budget` parameter in the same request". Gemini 4 cannot switch thinking off. | <https://aistudio.google.com/api-reference/interactions-api>, <https://aistudio.google.com/docs/thinking>, <https://aistudio.google.com/docs/text-generation>, <https://aistudio.google.com/docs/whats-new-gemini-3.6> |
| openai-chat | The Chat Completions reference: `temperature` 0 to 2, `top_p`, `stop` (1 to 4 strings), `reasoning_effort` (as Responses). DeepSeek: `reasoning_effort` `none`, `low`, `high`, `max` (`none` switches thinking off), temperature "has no effect in thinking mode", up to 16 stop sequences, `max_tokens` up to 393 216. llama.cpp's server: `temperature`, `top_k`, `top_p`, `stop` and `chat_template_kwargs` (README: "For example: `{"enable_thinking": false}`"); `thinking_budget_tokens` is in its source, not its README. Mistral, xAI and Groq: `reasoning_effort` with per-model values; xAI rejects `stop` on reasoning models. | <https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create.md>; <https://api-docs.deepseek.com/api/create-chat-completion>, `/guides/thinking_mode`; <https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md>; <https://docs.mistral.ai/api/endpoint/chat>, `/capabilities/reasoning`; <https://docs.x.ai/developers/model-capabilities/text/reasoning.md>; <https://console.groq.com/docs/reasoning> |

**How a compatible service is told to reason** is a field of its provider
entry, `reasoning_style`: `reasoning_effort` (the default) or
`enable_thinking`, llama.cpp's chat-template switch, which the presets
`llama-server` and `local` use.

### 15.2 What a model accepts

What one model accepts, in which range, and with which default, is
declared by the model list (`settings` of a provider entry and of a model,
`schemas/pax-models.schema.json`), and the session service holds the
person's choice to it (`services/tur-pax/src/tuning.rs`;
[pax-service.md](pax-service.md), section 2.5). A declaration the family
cannot be sent is refused when the list is read. The example list declares:

| Provider, model | Accepts (range, P@x's default) | Notes |
|---|---|---|
| `anthropic`, all three | effort `low` to `max`; stop up to 4; output limit 1 to 128 000 | No temperature, top-p or top-k (version `2026-10-21`). Provider default effort: `medium` for Opus 5.5 and Haiku 5.5, `high` for Sonnet 5.5. No `none`: Opus 5.5 and Sonnet 5.5 reject `disabled`. The stop limit is TUR's own: none was found documented. |
| `google`, `gemini-3.5-flash-lite` | thinking level `minimal`, `low`, `medium`, `high`; stop up to 4; output 1 to 65 536 | Its default is `minimal`. Stop count not documented: TUR's limit. |
| `google`, `gemini-4-carbon` | level `low`, `medium`, `high` | Thinking cannot be switched off; default `medium`. |
| `openai`, `gpt-6.1-sol` | effort `low`, `medium`, `high` | "does not support `none` or `minimal`"; whether it takes `xhigh` or `max` was not stated, so they are not offered. No temperature: it never runs at `none`. |
| `openai`, `gpt-6-luna` | effort `none`, `low`, `medium`, `high`; temperature 0 to 2 and top-p 0 to 1 **only with effort `none`** | Supports `none` per the latest-model guide. |
| `deepseek`, both | effort `none`, `low`, `high`, `max`; temperature 0 to 2, top-p 0.01 to 1, only with `none`; stop up to 16; output 1 to 393 216 | Default effort `high`. |
| `mistral` | temperature 0 to 1 (the page recommends 0 to 0.7 and states no wider range), top-p 0 to 1, stop up to 4 (TUR's limit); `mistral-small-2603` effort `none`, `high`; `mistral-large-4` `low`, `medium`, `high` | |
| `xai` | temperature 0 to 1 ("Values above 1 are sampled at 1"), top-p 0 to 0.95; no stop (rejected by reasoning models); `grok-4.7` effort `low`, `medium`, `high`, `xhigh` | `grok-4.3`'s efforts are not on the page read: none offered. |
| `groq` | temperature 0 to 2, top-p 0 to 1, stop up to 4; `openai/gpt-oss-120b` effort `low`, `medium`, `high`; `qwen/qwen3.8-27b` `none`, `low`, `medium`, `high` | "Values outside a model's supported set are rejected with a 400." |
| `llama-server` | temperature 0 to 2, top-p 0 to 1, top-k 0 to 1 000, stop up to 8 | Whatever model the person loads; no effort: it depends on the model's template. |
| `local`, every model | temperature, top-p, top-k, stop up to 8, output 1 to 8 192, context 2 048 to the model's own (at most 131 072), 8 192 by default | Qwen3 (0.6B, 4B, 30B A3B): effort `none` or `medium` (thinking off or on), default `none`; defaults temperature 0.7, top-p 0.8, top-k 20 (the cards' non-thinking settings). Gemma 4: the same switch; defaults 1.0, 0.95, 64 (its card). Ministral 3 3B: temperature default 0.05 (its card: "below 0.1"). |
| `openrouter`, `together`, `ollama` | nothing declared | Their parameter pages were not read for this work: only the output limit can be set. |

### 15.3 Thinking tokens and the limit of a day

Every family counts the model's reasoning as output, and so does the
meter: Anthropic's `output_tokens` include thinking; OpenAI's
`output_tokens` include `output_tokens_details.reasoning_tokens`; Google's
thought tokens are added to the output (`total_thought_tokens` is beside
`total_output_tokens`, and "pricing is the sum of output tokens and thinking
tokens"); for `openai-chat` the output is the largest of
`completion_tokens`, `total_tokens` less the prompt, and the reasoning
tokens, so a service that reports reasoning apart from the completion is
still counted (`openai_chat::tests::reasoning_is_counted_as_output_whatever_figure_carries_it`).
The day's and the task's limits act on these counts as before; a higher
effort spends more of them, and `GetGenerationSettings` says so. Thinking
counts in each family's output limit, which still bounds one response.

### 15.4 The models of a provider

`list_models` (`src/listing.rs`):

| Family | Request | Key | Names | Next page |
|---|---|---|---|---|
| Anthropic | `GET /v1/models?limit=1000` | `x-api-key`, `anthropic-version` | `data[].id`, `display_name` | `has_more`, `after_id=<last_id>` |
| OpenAI | `GET /v1/models` | Bearer | `data[].id` | none documented |
| Google | `GET /v1beta/models?pageSize=1000` | `x-goog-api-key` | `models[].name` without `models/`, `displayName` | `nextPageToken`, `pageToken` |
| openai-chat | `GET <base URL>/models` | Bearer, or none | `data[].id`, or a bare list | none |

Sources: <https://developers.openai.com/api/reference/resources/models/methods/list.md>,
<https://aistudio.google.com/api-reference/models/list-models>,
<https://api-docs.deepseek.com/api/list-models>,
<https://docs.mistral.ai/api/endpoint/models>, llama.cpp's README; for
Anthropic, the Models API in the bundled Claude API reference. At most 20
pages and 2 000 names; a name that cannot be a model name is left out; a
name listed twice is kept once. The session service asks only when the
person asks (`ListProviderModels` with `refresh`), keeps the answer for a
day, and shows it beside the model list; a model only the provider names
has no declared settings and no known price.

### 15.5 Not verified

- **No provider was called.** Field names and values are the
  documentation's.
- Whether OpenAI rejects or ignores `temperature` at an effort other than
  `none` (the guide says to remove it); TUR refuses it either way.
- Whether `gpt-6.1-sol` takes `xhigh` or `max`; Anthropic's and Google's
  limits on the number of stop sequences.
- Whether llama.cpp b9840, the pinned engine, honours `enable_thinking:
  true` when started with `--reasoning off` (its current source does);
  P@x's `UseLocalModel` starts a model that can think with
  `--reasoning auto`, so the request's switch decides.
- `thinking_budget_tokens` is in llama.cpp's source, not its README, and no
  shipped preset offers it.
- The model lists' answers were written from the documentation; none was
  read from a provider.

# Local AI

**Status:** design, 12 November 2026. Decision: [ADR-0021](../adr/0021-local-ai.md).
Nothing described here is built yet.

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
| Inference engines | Open-source, installed from Fedora or the upstream release, pinned; chosen by measurement (llama.cpp and Ollama are the first candidates; both must be verified for licence, packaging and current state) |
| Model catalogue | Data in the application catalogue: one entry per model with source, digest, size, licence, capabilities, hardware needs, tested results |
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

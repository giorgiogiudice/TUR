# ADR-0021: Local AI — models that run on the computer, for P@x and for every application

- **Status:** Accepted
- **Date:** 2026-11-12
- **Amends:** [ADR-0004](0004-ai-provider-model.md) ("A local-model adapter is not required now")
- **Design:** [local AI](../architecture/local-ai.md)

## Context

The owner requires TUR to run AI models locally, for everyone who wants AI
without sending anything to a company, and to present this as a headline
feature. ADR-0004 chose a cloud provider with the user's own key and left local
models for later, because local models then fell short at reliable screen
control and needed strong hardware. Both are still partly true; neither is a
reason to leave local AI out. Many people want private AI for writing,
summarising, translating, coding and conversation, where today's local models
are good, and an assistant that works offline.

## Decision

1. **Local AI is a built-in, opt-in feature of TUR**, offered in setup and in
   Settings like the assistant itself. Nothing runs and nothing is downloaded
   until the person switches it on.
2. **One local model service** (`tur-ai-local`) runs open-source inference
   engines that TUR installs from Fedora's repositories or their upstream
   releases (candidates: llama.cpp, Ollama; the choice is made by measurement
   in the implementing change). It serves an **OpenAI-compatible API on the
   local machine only**, so P@x and ordinary applications use local models the
   same way they use a cloud service.
3. **P@x can use a local model as its provider**, through the
   OpenAI-compatible adapter of ADR-0004's amendment. Every rule of the AI
   control layer applies unchanged. P@x tells the person honestly what the
   chosen model can do: a model without reliable tool calling can converse but
   not act; a model without image input cannot operate applications by sight.
   Mixed use is allowed: a local model for private conversation and a cloud
   model, only if the person chooses, for demanding tasks.
4. **Models are not shipped in the image.** They are large and their licences
   differ. The person chooses from a **curated list** in the application
   centre, which shows for each model: size, the memory it needs, what it is
   good at, whether it can call tools and see images, and its licence in plain
   words (some model licences restrict use; they are shown, never hidden). A
   model is downloaded from its publisher's official source and its digest
   checked before use. Any other compatible model file can be added by hand
   and is marked "not reviewed by TUR".
5. **TUR picks a model that fits the computer.** The service measures memory,
   graphics memory and processor, recommends a model that runs at a usable
   speed, and refuses to start one that would exhaust memory.
6. **Acceleration uses what TUR may ship**: the processor everywhere; the
   graphics card through open interfaces (Vulkan first, and the open compute
   stacks Fedora provides). Vendor-proprietary compute libraries are never
   shipped; where a person installs one from its vendor, the service uses it.
7. **Speech stays local by default** (ADR-0004), and the same service hosts the
   local speech recognition and synthesis models, so P@x with a local model is
   fully offline.
8. **Access is local and controlled.** The API listens only on the loopback
   interface or a socket, never on the network unless the person turns that on
   explicitly. Which applications may use it is a permission the person sees
   in Settings.
9. **Resources are the person's.** The service stops models when idle,
   releases memory, never starts on battery below a threshold unless asked,
   and yields to the foreground (TUR's "nothing runs until you need it").

## Consequences

- TUR is useful with AI for people who will not or cannot use a cloud service:
  no account, no key, no cost per use, nothing leaves the computer.
- P@x's ability depends on the model and the hardware, and is published like
  application compatibility: a table of tested models with what P@x can and
  cannot do with each, measured by the same scenario suite the cloud providers
  run.
- The image stays small; model storage lives under the person's data and is
  shown in Storage with a one-step removal.
- Building and testing a local inference service is in scope for CI (small
  models on the processor); measurements on real graphics hardware wait for
  the test PC and are recorded as not run until then.
- Licences of inference engines and of each listed model are reviewed and
  recorded in `docs/dependencies/` before they are offered.

## Verification

- The P@x scenario suite runs against a small local model in CI, through the
  local service, with the same pass criteria as for cloud providers.
- On the test PC: tokens per second and memory for each listed model, on the
  processor and on the graphics card; the recommendation matches what runs.
- Nothing listens on a network interface by default (tested).

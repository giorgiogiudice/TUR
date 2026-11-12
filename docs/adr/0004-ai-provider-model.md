# ADR-0004: AI layer — optional, provider-agnostic, user-supplied key

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** none in revision 1.0; see the AI control layer design

## Context

A core requirement added after revision 1.0 of the specification: TUR can be driven
entirely by AI. A user adds an API key, speaks to the system, and an assistant
carries out tasks, seeing and controlling applications from every runtime.

This decision fixes how TUR relates to model providers. How the assistant controls
the system, and what it is permitted to do, is specified in the AI control layer
design.

## Options considered

1. **One provider, deeply integrated.** Simplest to build and test; ties the OS to
   one vendor's availability, pricing and terms.
2. **Provider-agnostic behind one interface, with the user's own key.** More
   interface design; no lock-in, and the user chooses who sees their data.
3. **Local models only.** Best for privacy; current local models fall well short on
   reliable screen control, and the hardware demand is high.

## Decision

The AI layer is optional, off by default, and provider-agnostic. The user supplies
their own API key. The first supported providers are **Anthropic (Claude), OpenAI
and Google (Gemini)**.

## Consequences

- One service, `tur-pax`, owns all communication with model providers. No other
  TUR component contacts a provider.
- A provider adapter implements a common interface: streaming conversation, tool
  calling, image input, and a declared set of capabilities. The agent uses a
  provider's native computer-control tooling where it has one, and falls back to a
  generic screenshot-and-tools loop where it does not.
- Model names are configuration, discovered from the provider where possible. They
  are never compiled in.
- API keys are stored in the system keyring and never written to profiles, logs or
  diagnostic bundles.
- Speech recognition, wake-word detection and speech synthesis run locally by
  default, so audio does not leave the machine. A provider's speech services may be
  offered as an explicit option.
- Each provider adapter needs its own conformance tests, and behaviour will differ
  between providers. The compatibility of each provider is published, like
  application compatibility.
- A local-model adapter is not required now. The interface must not prevent one.
  *(Superseded on 12 November 2026 by [ADR-0021](0021-local-ai.md): local AI is a built-in feature.)*
- TUR without a key is a complete product. No boot, install, update or recovery
  path may depend on the AI layer.

## Verification

- Check each provider's current API documentation, terms of use and data-retention
  terms when its adapter is written; do not rely on recalled API shapes.
- A conformance suite that runs the same scripted tasks against every adapter.

## Amendment: DeepSeek and any compatible provider (12 November 2026)

The owner requires P@x to work with **DeepSeek** and to be adjustable for the
other kinds of AI API keys widely available. Three named providers are not
enough.

- **Adapters are by API family, not by company.** Besides the three native
  adapters (Anthropic Messages, OpenAI Responses, Google), TUR has an adapter
  for the **OpenAI-compatible Chat Completions** API, which DeepSeek and many
  other services offer. It is configured, not compiled: a provider is a
  configuration entry naming the adapter family, the base URL, the model, the
  keyring item that holds its key, and its declared capabilities.
- **DeepSeek is the first named preset** of that adapter. Others (for example
  Mistral, xAI, Groq, OpenRouter, Together, or a local server such as Ollama or
  llama.cpp) are presets or entries the person adds in settings. A preset is
  data and can be added without a new release.
- **Capabilities decide what P@x may attempt with a provider**, and are checked
  against the live service where it can be done cheaply: tool calling,
  streaming of tool calls, image input (needed for screen control), and the
  context size. A model without tool calling can converse but cannot act; a
  model without image input cannot control applications by sight. The person
  is told which, before choosing.
- **Data terms are shown per provider**, as for the first three: whether the
  service keeps requests, for how long, whether it trains on them, and where
  it processes them, with the date the terms were read. Where a service
  offers no way to ask it not to keep requests, the consent text says so. A
  provider entry the person adds by hand is shown as "terms not reviewed by
  TUR".
- Every rule of the AI control layer applies unchanged to every provider: the
  policy engine, the action classes, the sandbox, the discard-whole rule for
  malformed or refused responses, and the spending limit.
- Conformance tests run the same scripted tasks against every adapter family;
  a preset is tested against the real service only where a key is available.


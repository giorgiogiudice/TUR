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
- TUR without a key is a complete product. No boot, install, update or recovery
  path may depend on the AI layer.

## Verification

- Check each provider's current API documentation, terms of use and data-retention
  terms when its adapter is written; do not rely on recalled API shapes.
- A conformance suite that runs the same scripted tasks against every adapter.

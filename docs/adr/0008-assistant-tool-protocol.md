# ADR-0008: Assistant tools use the Model Context Protocol

- **Status:** Proposed
- **Date:** 2026-11-12
- **Specification sections:** AI control layer (addendum A) §3, §10, §11

## Context

The AI layer is provider-agnostic (ADR-0004): Anthropic, OpenAI and Google first.
Each provider has its own format for describing tools to a model. Addendum A
defines a UOS tool manifest that each service ships, and leaves the translation to
each provider's format to the provider adapters.

The Model Context Protocol (MCP) is an open protocol for exposing tools to
AI models. It is supported by all three initial providers and by a wide range of
client software. A tool is described there by a name, a description and a JSON
Schema for its input, which is what the UOS manifest already contains.

## Options considered

1. **A UOS-only tool format,** translated per provider. Full control; every
   adapter carries translation code; third-party tools must be written for UOS.
2. **MCP as the tool protocol,** with the policy engine as a gateway in front of
   it. One format for every provider; existing MCP servers can be offered to the
   assistant; UOS tools are usable by other clients the user chooses.
3. **MCP directly, with no gateway.** Simplest, and unsafe: the protocol's own
   hints about whether a tool is destructive are supplied by the tool and cannot
   be trusted.

## Decision

Adopt option 2.

- UOS services' tools are exposed to models as MCP tools.
- **`uos-agent` is the only MCP client the model's requests pass through, and the
  policy engine decides every call.** A tool's action class comes from the
  reviewed UOS manifest, never from hints a tool supplies about itself.
- The UOS tool manifest stays as the reviewed source of truth, extended with the
  action class and the D-Bus method. The MCP description is generated from it.
- A third-party MCP server added by the user is untrusted: every tool it offers is
  class 2 (ask first) until the user sets otherwise, and it can never be class 0
  for anything that leaves the machine.

## Consequences

- Provider adapters no longer translate tool formats individually where the
  provider speaks MCP.
- The assistant can use tools beyond UOS's own, which is much of what makes an
  assistant useful, at the price of a new threat: a malicious or compromised tool
  server. It is handled as untrusted content is in addendum A §7: its output is
  data, and its tools sit behind confirmation.
- Small agents inside a Windows guest or the Android runtime can expose those
  environments' applications to the assistant the same way (ADR-0007).
- The A0 policy engine and manifest work is unaffected: the manifest fields map
  directly.

## Verification

- Check the current MCP specification and each provider's current support before
  implementing; confirm how each handles tool results as untrusted input.
- An injection test in which a tool server returns instructions in its results.

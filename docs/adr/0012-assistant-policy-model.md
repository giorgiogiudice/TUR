# ADR-0012: The assistant's authority is enforced by a pure decision function outside the model

- **Status:** Proposed
- **Date:** 2026-11-12
- **Specification sections:** §12, §29.3, §29.4; AI control layer (addendum A) sections 2, 5, 6, 7 and 17

## Context

P@x can reach every application and can run commands. What it is allowed to do
without asking has to be decided somewhere, and that place is the target of every
attack on the assistant: text on a web page, a file name, a tool result, a
recalled memory or a compromised provider can all put words into the model's
mouth. Addendum A states the principle ("the model is not a security boundary")
and the rules (five action classes, the trusted path, repair sessions, standing
approvals, user policy). It does not say what form the thing that applies the
rules takes, or who holds the facts it decides from.

Constraints and what is known:

- The rules are few and their inputs are small enumerations: the kind of action,
  its class, whether a person is present, whether the session is locked, whether
  the trusted path exists, whether a repair session is open, the user's mode and
  lists. The whole input space can be enumerated in a test.
- Several inputs are grants of authority: a standing approval, a repair session,
  the user policy, "the trusted path is available", "this window belongs to the
  user". If the assistant's own process could write any of them, it could
  approve itself.
- The engine exists (`services/uos-agent`, milestone A0). It was reviewed
  independently ([policy-engine-review-1](../reviews/policy-engine-review-1.md));
  the review found defects in rules and in parsing, and none that came from the
  form chosen here.

What is not known: whether the compositor can provide the five guarantees of the
trusted path, and what the component that owns the policy inputs will be. Neither
exists yet.

## Options considered

1. **Instruct the model.** Put the rules in the system prompt and trust the model
   to follow them. No code to write. It fails the stated principle outright:
   whoever controls the model's input controls the decision, and nothing can be
   tested exhaustively.
2. **Checks inside each tool and each way of acting.** Every tool, the command
   runner and the screen driver each decide for themselves. Enforcement sits next
   to the effect, which is good, and class 3 must be enforced by the owning
   service in any case. But the rules that cut across actions (nobody present,
   locked, no trusted path, observe-only, the user's lists) would be written many
   times, by different people, with no single place to test or review.
3. **A stateful policy service.** One service that remembers approvals, open
   sessions and the task so far, and answers "may I?". It can express more (for
   example "approved for the rest of this task"), which is exactly the standing
   authority the design refuses. State makes the answer depend on history, so
   the input space can no longer be enumerated, and the service's own store
   becomes something to protect.
4. **A pure decision function, with its inputs owned elsewhere.** One function,
   `decide(action, context, user policy, time) -> decision`, with no I/O, no
   clock and no memory between calls. Every fact that grants authority is an
   input, supplied by a separate system component that the assistant's process
   can read from and cannot write to. The owning services still enforce class 3
   themselves.

## Decision

The assistant's authority is enforced by option 4: a pure, deterministic decision
function in `services/uos-agent`, outside the model, whose invariants are tested
over its whole enumerated input space; and every input that grants authority (the
user policy, standing approvals, the repair-session record, the trusted-path
state and the owner of a target window) is owned and supplied by a separate
system component, changed only through system settings on the trusted path.

## Consequences

Easier:

- The rules live in one place and are stated as a decision table
  ([agent-policy.md](../architecture/agent-policy.md)). A reviewer reads one
  function.
- Invariants such as "no input yields `allow` for class 3 or 4" are checked on
  every input, not on examples. A change that breaks one fails CI.
- There is no approval to steal or replay inside the engine. An `ask` is
  answered outside it, for one action, and the next call asks again.
- The model's output reaches the engine only as a proposal and as the
  "consequential" hint, which can only make a decision stricter.

Harder, and what must now be true elsewhere:

- **The engine decides; it does not enforce.** Every action must pass through
  it, the caller must obey the decision and must run exactly the action that was
  decided and shown. That is the orchestrator's obligation and is not tested
  here.
- **The engine believes its inputs.** A forged standing approval, a repair
  session record written by the assistant, or "trusted path available" set
  without the compositor guarantees defeats it completely. The owning component
  (addendum A section 17, item 5) is therefore part of the security boundary and
  must exist before any class 2 or privileged action is released. Until it does,
  the trusted-path input stays false and the assistant is limited to classes 0
  and 1.
- **Statelessness moves some rules to the owner of the state.** "A repair
  session ends when the session locks" cannot be remembered by a pure function.
  The engine treats a session as closed while locked; the owner of the record
  must mark it ended so that it stays closed after unlock. The same holds for a
  clock set back.
- **A class is only as true as its source.** A tool's class comes from a manifest
  the service wrote; a command's class comes from the sandbox it will run in; a
  typed privileged operation's from the helper's confinement. Each of those must
  make the class true. Class 3 must also fail inside the owning service when
  called directly.
- **Accessibility and screen actions remain a heuristic.** No function can tell
  what a click does. The engine asks when it cannot tell, and the real bound is
  that protected surfaces reject emulated input.
- Every new way of acting, context fact or policy setting must be added to the
  exhaustive sweep in the same change, or the word "exhaustive" stops being
  true.

## Verification

- `services/uos-agent/tests/invariants.rs` enumerates the input space and
  asserts each invariant on every decision; `tests/review.rs` holds a regression
  test for each finding of the first review. Both run in CI on every change.
- The first review checked that the tests bite: for the five most important
  invariants, a one-line change that breaks the invariant was pushed, and the
  failing run recorded, then reverted.
- Before the trusted-path input may be true: the five compositor guarantees of
  addendum A section 6, verified against KWin as shipped (open question 1 of
  the addendum).
- Before standing approvals, repair sessions or a user policy file are honoured
  outside tests: the owning component exists, and a test shows that the
  assistant's process cannot write its store.
- Before milestone A5: the privileged helper confines each typed operation as
  the engine's table states, and refuses what the engine refuses when called
  directly.
- A second independent review when the orchestrator exists, covering the path
  from a decision to the action that actually runs.

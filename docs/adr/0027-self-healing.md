# ADR-0027: Self-healing — a declarative fault catalogue run by TUR's own service; P@x only asks

- **Status:** Proposed
- **Date:** 2026-11-13
- **Specification sections:** §6.3 (two-layer errors), §6.5 (upload is an
  explicit act), §29 (diagnostics and codes); `AGENTS.md` (the AI layer is
  optional; nothing uploaded silently; no authority not granted)
- **Related:** [ADR-0012](0012-assistant-policy-model.md),
  [ADR-0014](0014-boot-health-and-rollback.md),
  [ADR-0023](0023-publishing-signed-updates.md),
  [ADR-0024](0024-problem-reports.md); design
  [self-healing.md](../architecture/self-healing.md)

## Context

Bet 5 of [breakthroughs.md](../breakthroughs.md): a fault seen on one
computer is noticed, explained and repaired where that is safe, the repair
is checked to have held, and when the cause is in TUR the person may send a
report that turns into a signed update which rolls back by itself if it is
worse. The parts existed separately: `tur-diag` and problem reports
(ADR-0024), signed publishing (ADR-0023), automatic rollback (ADR-0014), the
policy engine (ADR-0012). Nothing connected them.

Constraints:

- The AI layer is optional and off by default; nothing in boot, install,
  update or recovery may depend on it. So the loop's core cannot be P@x.
- Nothing is uploaded silently; by default the person is asked every time.
- A repair must be reversible or explicitly approved; disks, boot, firmware
  and encryption need approval through a path P@x cannot operate.
- P@x performs nothing above class 1 (no trusted session). Its tools run as
  programs in a read-only sandbox that cannot change anything, and it had no
  way to ask a TUR service to act: the D-Bus services the manifests name do
  not exist.

## Options considered

1. **P@x drives the loop** (it notices, diagnoses and repairs with its own
   tools). Rejected: it would make healing depend on the optional AI layer,
   and P@x cannot change anything today.
2. **A healing service in TUR with a fixed, declarative catalogue**, like the
   compatibility autopilot's remedies: each entry a check kind, a code, an
   explanation, remedies of fixed kinds with class and undo, the verification
   (the same check run again), when to offer a report, and sources. Chosen.
3. **Free-form remedies** (shell snippets in the catalogue). Rejected: a
   catalogue entry could then do anything; with fixed kinds the validator can
   hold every entry to the class and reversibility rules.
4. How P@x asks for a repair:
   a. *Show the service's socket inside the read-only sandbox.* Rejected: it
      breaks the sandbox's one promise (the program cannot change anything)
      and the e2e test's socket canaries.
   b. *A D-Bus system service.* The intended end state of the tool protocol
      (ADR-0008), but it needs a D-Bus server in `tur-diag` and a policy for
      it; too much for this slice.
   c. *A `request` adapter kind in `tur-pax`:* after the engine allows the
      call, `tur-pax` itself sends the tool's name and validated input, as
      one line of JSON, to a socket under `/run/tur/` named by the adapter
      file, and returns the answer. Chosen: small, testable, and the service
      keeps the decision. It is the D-Bus method call in all but transport;
      moving to D-Bus later changes only the adapter.
5. Who may use the socket: only P@x (peer check), or anyone. Chosen: anyone,
   because the service grants a request only what it would do by itself — a
   class 0/1, reversible remedy of the catalogue, for a fault its check finds
   at that moment, at most three times a boot. Class 2 is refused on the
   socket; it needs `sudo tur-diag heal apply … --yes`, which P@x cannot
   operate.
6. Standing consent to send reports. Not adopted: ADR-0024's switch is not
   built, and this ADR adds none. A report is prepared by the person (or
   previewed by P@x) and sent only by the person, every time.
7. How a fix links back: the receiver's `fixed_in` (ADR-0024) tells the
   person's `report status`; the image also carries the codes it addresses
   (`/usr/share/tur/heal/fixes.json`), and the service checks such a fault
   again after the update and says "fixed" or "happened again". Chosen, both.

## Decision

TUR's healing service (`tur-diag heal scan --repair`, `tur-heal.service` on
a timer, root, no P@x) runs a fault catalogue compiled into `tur-diag`.
Checks and remedies are fixed kinds; a remedy is applied by the service
itself only when the entry marks it automatic, which the validator allows
only for reversible class 0/1 remedies; the repair is verified by running
the check again, and checked to have held at the next scan. A fault whose
cause is in TUR and that nothing repairs ends as an offered report, prepared
and sent only by the person under ADR-0024. `tur-heal-request.socket` accepts
requests to apply or undo class 0/1 remedies; `tur-pax` gains the `request`
adapter kind and P@x four tools (two class 0 reads, two class 1 requests).
Each image lists the codes it fixes; after an update the service checks
those faults again and tells the person.

## Consequences

- Healing works with P@x switched off; P@x adds explanation and asks.
- A remedy is reviewable data with a source; adding a fault means adding a
  check or remedy kind in Rust only when no existing kind fits.
- `tur-pax` offers class 1 tools for the first time. The engine's rules are
  unchanged; the tool protocol has a second performer, documented in
  `schemas/pax-tool-adapter.schema.json`. A `request` adapter can only name a
  socket under `/run/tur/`, and carries the validated input and nothing else.
- Anyone on the computer can trigger, through the socket, what the service
  would do by itself; a person who undoes a remedy stops the service from
  applying it again until the fault is resolved.
- Restarting a failed service is automatic; switching network time back on
  and restoring the resolver file are not, because the person may have
  chosen them. That line is the owner's to move.
- The state file (`/var/log/tur/heal/state.json`) is readable by everyone and
  must stay free of personal data; copies of replaced files are root's only.

## Verification

- Upstream behaviour each check and remedy relies on is cited in the
  catalogue with the date it was read (systemctl, timedatectl,
  systemd-resolved, journalctl, coredumpctl; systemd's per-connection
  instance names).
- The `heal` workflow injects each fault into a booted TUR image and checks
  detection, repair, verification, refusal of class 2 without approval,
  undo, the report sent only after confirmation and free of planted personal
  values, and the fix link across a restart. The `pax` workflow proves the
  P@x path end to end with a scripted provider. Report:
  [heal-1](../reports/heal-1.md).

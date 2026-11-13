<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# heal-1 — Self-healing, first slice (ADR-0027)

```text
TUR DEVELOPMENT REPORT

Task ID:          heal-1 (bet 5 of docs/breakthroughs.md; ADR-0027, Proposed;
                  pull request 108)
Subsystem:        tur-diagnostics (tur-diag heal), image units, tur-pax
                  (request adapters), boot tests (heal workflow), P@x
                  end-to-end test
Build ID:         RESULTS-PENDING
Source revision:  RESULTS-PENDING (feat/self-healing)

SUMMARY
- What changed: TUR's healing service. A fault catalogue (five faults),
  compiled into tur-diag, each entry a check of a fixed kind, a stable
  code, a plain explanation, remedies of fixed kinds with class and undo,
  the notices, when a report is offered, and sources. tur-heal.service on a
  timer runs it as root without P@x: detect, apply what is reversible and
  marked automatic, verify by running the check again, check at the next
  scan that the repair held, offer a report where the cause is in TUR, and,
  after an update whose fixes list names the code, say "fixed" or
  "happened again". A socket (tur-heal-request.socket) takes requests to
  apply or undo class 0/1 remedies, from P@x or a person; class 2 needs the
  person, as administrator, with confirmation. tur-pax gains a "request"
  adapter kind; P@x gains four tools (two class 0, two class 1).
- Why it changed: bet 5; the parts existed separately (ADR-0014, -0023,
  -0024, the policy engine) and nothing connected them.

FILES
- Added: services/tur-diagnostics/src/heal.rs, src/heal/ (catalogue,
  checks, remedies, state, engine, machine, tests), src/heal_cli.rs,
  heal/catalogue.json; schemas/heal-catalogue.schema.json,
  heal-fixes.schema.json; image/rootfs: tur-heal.service, tur-heal.timer,
  tur-heal-request.socket, tur-heal-request@.service, user
  tur-heal-notify.path/.service, their .wants links, tmpfiles.d/tur-heal.conf,
  usr/share/tur/heal/fixes.json; tests/boot/heal.py; guest heal-agent,
  heal-pax, heal-receiver; tests/pax/scenarios/20-heal-request.json;
  .github/workflows/heal.yml; docs/architecture/self-healing.md;
  docs/adr/0027-self-healing.md; this report
- Modified: services/tur-diagnostics (error.rs: TUR-DIAG-HEAL-001..011;
  main.rs: `heal` subcommand, report preparation shared; agent tools and
  their test), services/tur-pax (adapter.rs: request kind; orchestrator.rs:
  performing it; tests/tools.rs), schemas/pax-tool-adapter.schema.json,
  tests/pax/e2e.py (stand-in of the service), .github/workflows/pax.yml,
  .github/workflows/gate.yml (heal in the list), tests/image/smoke.sh (an
  adapter may name a socket: two lines), tests/boot/guest/Containerfile and
  guest-agent (dispatch to the new agent), docs/architecture/diagnostics.md,
  pax-orchestrator.md
- Not touched: image/Containerfile, image/programs.tsv (tur-diag is already
  in the image), docs/adr/README.md
- Removed: none

BUILD
- Command: CI only (rust, image, heal, boot, rollback, pax, reports, ...)
- Result: RESULTS-PENDING
- Warnings: RESULTS-PENDING

TESTS
- Unit tests: RESULTS-PENDING
- Integration tests (P@x end to end, scenario heal-request): RESULTS-PENDING
- Boot tests (heal workflow, booted guest, fault injection): RESULTS-PENDING
- Hardware tests: NOT RUN (no hardware runner)
- Performance tests: NOT RUN
- Desktop delivery of notices (gdbus to the notification service): NOT RUN

DIAGNOSTICS
- Artifacts: tur-heal (guest reports of both boots: every state file, the
  P@x audit trail, the stand-in receiver's log and the upload), pax-evidence
- Relevant error IDs: TUR-DIAG-HEAL-001..011 (new)

RECOVERY
- Rollback procedure: revert the pull request. The image gains units and a
  tmpfiles entry; nothing under /etc. On a running system: `systemctl
  disable --now tur-heal.timer tur-heal-request.socket`; every remedy
  applied can be undone with `tur-diag heal undo` (restart: nothing to
  undo; deleted logs: cannot be undone, which is why that is class 2).
- Recovery tested: undo of the resolver repair (file restored byte for
  byte) and of network time in unit tests; undo of the resolver repair in
  the guest and in the P@x end-to-end test.

KNOWN LIMITATIONS
- See "Not done" below.

NEXT SAFE ACTION
- Owner: the open decisions below.
```

RESULTS-PENDING

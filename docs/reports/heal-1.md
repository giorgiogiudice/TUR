<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# heal-1 — Self-healing, first slice (ADR-0027)

```text
TUR DEVELOPMENT REPORT

Task ID:          heal-1 (bet 5 of docs/breakthroughs.md; ADR-0027, Proposed;
                  pull request 108); also issue 57, items 1 and 2
Subsystem:        tur-diagnostics (tur-diag heal; status), image units,
                  tur-pax (request adapters), boot tests (heal workflow),
                  P@x end-to-end test
Build ID:         20261113.g40fd09a389af (version 0.1.0-dev), the image the
                  heal workflow built and booted at a9fad7f
Source revision:  feat/self-healing; proven at a9fad7f, then 83583a1
                  (formatting only) and the report; final runs in "CI runs"

SUMMARY
- What changed: TUR's healing service. A fault catalogue of five faults,
  compiled into tur-diag; each entry is a check of a fixed kind, a stable
  code, a plain explanation, remedies of fixed kinds with class and undo,
  the person's notices, when a report is offered, and upstream sources.
  tur-heal.service, on a timer, runs it as root without P@x: detect, apply
  what is reversible and marked automatic, verify by running the check
  again, confirm at the next scan that the repair held, offer a report
  where the cause is in TUR, and after an update whose fixes list names the
  code, say "fixed" or "happened again". tur-heal-request.socket takes
  requests to apply or undo class 0/1 remedies, from P@x or a person;
  class 2 needs the person, as administrator, with confirmation. tur-pax
  gains a "request" adapter kind; P@x gains four tools (two class 0, two
  class 1).
- Issue 57: tur-diag starts bootc only as root (no SELinux denial in P@x's
  sandbox; no policy rule added); where systemctl cannot reach the service
  manager, `status` uses the failed-unit count the healing service made as
  root, with its time.
- Why it changed: bet 5. The parts existed separately (ADR-0014, -0023,
  -0024, the policy engine) and nothing connected them.

FILES
- Added: services/tur-diagnostics/src/heal.rs, src/heal/ (catalogue,
  checks, remedies, state, engine, machine, tests), src/heal_cli.rs,
  heal/catalogue.json; schemas/heal-catalogue.schema.json,
  heal-fixes.schema.json; image/rootfs: tur-heal.service, tur-heal.timer,
  tur-heal-request.socket, tur-heal-request@.service, user
  tur-heal-notify.path and .service, their .wants links,
  tmpfiles.d/tur-heal.conf, usr/share/tur/heal/fixes.json (empty list);
  tests/boot/heal.py; guest heal-agent, heal-pax, heal-receiver;
  tests/pax/scenarios/20-heal-request.json; .github/workflows/heal.yml;
  docs/architecture/self-healing.md; docs/adr/0027-self-healing.md; this
  report
- Modified: services/tur-diagnostics (error.rs: TUR-DIAG-HEAL-001..011;
  main.rs: `heal`, report preparation shared; identity.rs and
  collectors.rs: issue 57; agent tools and their test); services/tur-pax
  (adapter.rs: the request kind; orchestrator.rs: performing it;
  tests/tools.rs); schemas/pax-tool-adapter.schema.json; tests/pax/e2e.py
  (stand-in of the service); .github/workflows/pax.yml, gate.yml (heal
  added); tests/image/smoke.sh (an adapter may name a socket: 3 lines);
  tests/boot/guest/Containerfile and guest-agent (dispatch);
  docs/architecture/diagnostics.md, pax-orchestrator.md
- Not touched: image/Containerfile, image/programs.tsv (tur-diag is
  already in the image), docs/adr/README.md
- Removed: none

BUILD
- Command: CI only.
- Result: image built and booted (image, boot, heal, iso workflows at
  a9fad7f). rust at a9fad7f: clippy and every test passed in
  services/tur-diagnostics and services/tur-pax; the job failed only on
  rustfmt, fixed in 83583a1.
- Warnings: none from clippy (-D warnings).

TESTS
- Unit tests: tur-diagnostics 132 passed, 1 ignored, and its integration
  suites (agent tools, bundle round trip, doc tests) all passed; tur-pax 254
  unit tests and all its integration suites passed (rust run 43494372380).
- Integration tests: P@x end to end (pax workflow, run 43494372418 at
  a9fad7f; 43480677597 at 4617891): all checks passed, the new scenario
  heal-request included. In the booted guest the same e2e test passed with
  392 checks in 20 scenarios; there the heal-request scenario is recorded as
  NOT RUN, because it needs the stand-in that only the pax workflow starts.
- Boot tests: heal workflow, run 43494372499: 31 checks, all required ones
  passed (see "What the guest test proves"). boot, run 43494372645: passed,
  including "no AVC denial when tur-diag starts bootc from a process that
  may gain no new privileges: 0 denial(s)" (issue 57; that check is
  informational, and is the denial the issue reported).
- Hardware tests: NOT RUN (no hardware runner).
- Performance tests: NOT RUN. The heal guest run took 710 s, most of it
  booting; a scan is a handful of commands.
- Desktop delivery of notices (gdbus to the notification service): NOT RUN
  (the guest has no graphical session).

DIAGNOSTICS
- Artifacts: tur-heal (both boots' reports: every state file, the P@x audit
  trail and provider requests, the stand-in receiver's request log and the
  upload, notices, the serial log); pax-evidence.
- Relevant error IDs: TUR-DIAG-HEAL-001..011 (new).

RECOVERY
- Rollback procedure: revert the pull request. The image gains units and a
  tmpfiles entry, nothing under /etc. On a running system:
  `systemctl disable --now tur-heal.timer tur-heal-request.socket`. Every
  remedy can be undone with `tur-diag heal undo`, except deleting logs
  (class 2, which is why it needs confirmation) and starting a service
  (nothing to undo).
- Recovery tested: undo of the resolver repair (the original file back,
  same SHA-256) in the guest and in the P@x e2e test; undo of network time
  in unit tests.

KNOWN LIMITATIONS
- See "Not proven" and "Follow-ups" below.

NEXT SAFE ACTION
- Owner: the open decisions below; then review and merge.
```

## What the guest test proves (heal run 43494372499)

The product image (with the boot test's agent) was installed to a disk,
prepared by the boot test's first two boots and booted twice more. TUR's
own units did the work; the agent only injected faults, started
`tur-heal.service` with `systemctl start` (as the timer would; the timer was
masked during the test so that runs happened only when the agent started
them), and acted as the person or as P@x.

| Step | Injected | What happened | Checks |
|---|---|---|---|
| Product | nothing | timer and socket wanted by the image's `/usr/lib` links and active; the session's notice watcher wanted; the first scan, through systemd, found none of the five faults | P1, P2 |
| Service | chronyd killed with SIGKILL, systemd's own restart off for that boot (runtime drop-in) | the service found it failed, started it by itself, verified it runs; the next scan found that the repair held | S1, S2 |
| Network time | `timedatectl set-ntp false` | found, left for a person. P@x (installed `tur-pax`, scripted provider, the image's tools) read the faults and the explanation at class 0, asked for the repair at class 1 (allowed by the engine), and asked for deleting logs, which the service refused with TUR-DIAG-HEAL-003; the refusal reached the model. NTP on again, recorded as a request from UID 1000 | T1–T4 |
| Resolver | `/etc/resolv.conf` replaced by a file naming 192.0.2.53 | found, left for a person. `tur-diag heal apply` as the test user (through the socket) restored the link (label `net_conf_t`); `heal undo` put back the same file (same SHA-256); applied again, the link is the original | R1–R4 |
| Disk | `/var` filled to 734 MB free | found. Through the socket: refused (HEAL-003). As root without confirmation: refused (HEAL-004). Confirmed: logs rotated and vacuumed, the check said still full, exit 4, one application recorded. Freed by other means: cleared | D1–D5 |
| Crash | `tur-diag` killed with SIGABRT | `coredumpctl` recorded it; the service found it, tried nothing, offered a report. The stand-in receiver saw 0 requests after the scan, after preparing, and after an unconfirmed send (TUR-DIAG-REPORT-004); 1 after the person sent with `--yes`. The upload is byte for byte the prepared report (16 763 bytes). None of the planted values (host name, account name, a public address in a log line the user wrote, the network card's MAC address, the machine ID) is in it; the redaction report shows host names (55), the machine ID (50), user names (8) and IPv4 (2) replaced. report.json: TUR-DIAG-CRASH-001, crash, diag, person | C1–C6 |
| Fix link | a fixes list naming TUR-DIAG-CRASH-001, over the image's own (tmpfs), and the crash's record marked as seen on a previous build | same boot (crash present): "happened again", report offered again. After a restart (no crash): resolved as fixed by the update, and the person who sent the report is told "The problem you reported is fixed" | F1–F3 |
| Notices | | all expected notices present, with the commands they offer | N1 |

### Not proven

- **A real update.** The fix link was proven with the fixes list mounted
  over the image's and one record's build rewritten (test plumbing, said so
  in heal-agent). An update from one built image to another that lists a
  code has not been run.
- **MAC address.** C5 counts 0 MAC replacements: the address was not in the
  bundle collected as the test user, so check C4 holds for it trivially.
- **Desktop notifications** (`heal notify`, `tur-heal-notify.path`): NOT RUN.
- **Deleting logs freed space**: it did not; the journal was 16 MB, and
  rotation made it 27.6 MB. The test proves the class-2 path and the
  "not repaired" verdict, not that the remedy helps.
- **The real receiver**: the upload went to a stand-in on the guest; the
  receiver's own tests (reports workflow) are unchanged and passed.
- **Hardware**: NOT RUN.

## CI runs

| Commit | Run | Result | Then |
|---|---|---|---|
| 558438d, 4617891 | rust 43480677463 | tur-diagnostics: clippy and tests passed, rustfmt failed; tur-pax cancelled | rustfmt applied |
| 4617891 | pax 43480677597, reports 43480677472, boot, image, desktop | passed | |
| 4617891 | heal 43480677457 | failed: the crash was never recorded, because Rust's SIGSEGV handler (stack-overflow detection) resets the signal and returns, so a SIGSEGV sent with `kill` does not end the program; and `systemctl is-enabled` reports "disabled" for units enabled by `/usr/lib` links | SIGABRT; check the links |
| a9fad7f | heal 43494372499, boot 43494372645, image 43494372469, iso 43494373017, pax 43494372418, reports 43494372817 | passed | |
| a9fad7f | rust 43494372380 | every test passed; rustfmt failed (tur-diagnostics, tur-pax) | 83583a1 |
| FINAL | see the pull request | | |

## Open decisions for the owner

1. **What is automatic.** Only restarting a failed system service on the
   catalogue's list. Network time and the resolver file wait for a person,
   because they may be deliberate. Moving that line is a catalogue change.
2. **The socket is open to every account.** Justified because it grants
   only what the service does by itself (class 0/1, reversible, fault
   present now, three times a boot). The alternative is a peer check
   (P@x's session only), which would lock out the person's own command.
3. **The `request` adapter kind in tur-pax** is an interface change to the
   tool protocol (ADR-0008, ADR-0027). It touches `adapter.rs`,
   `orchestrator.rs` and `tests/tools.rs`, which another work item (#56,
   #43, #33) is also changing; a D-Bus performer could replace it later
   without changing the tools.
4. **Fixed codes are listed by hand** in `/usr/share/tur/heal/fixes.json` by
   the pull request that fixes them. The receiver's `mark-fixed` could feed
   it, and tur-update could show the codes before updating.
5. **No standing consent.** Reports are sent only by the person, every
   time. ADR-0024's "send automatically" switch stays unbuilt.
6. **ADR number.** 0027 was free on main when written; another open pull
   request may take it.

## Follow-ups

- Issue 57, items 3 and 4: a collector for boot health's `history.jsonl`
  and evidence directory (`/var/lib/tur/boot-health`, outside the
  collectors' allow-list today), and a line in the bundle when the user
  cannot read `tur-boot-health`'s journal records.
- User-scope faults: a scan in the session (failed user services such as
  PipeWire, a broken user Flatpak remote, a full home).
- More system faults, each with a check, a remedy, a source and a
  fault-injection test.
- Notices in Settings and in P@x's window; proof of desktop delivery.
- An end-to-end update test where the second image lists a fixed code.

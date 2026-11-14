<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# heal-2 — Self-healing: the person is always kept aware (ADR-0027, owner's rule)

```text
TUR DEVELOPMENT REPORT

Task ID:          heal-2 (ADR-0027, the owner's decision of 14 November
                  2026; pull request 117, based on pull request 108)
Subsystem:        tur-diagnostics (tur-diag heal notify, history, decline;
                  notices), image user units, boot tests (heal workflow)
Build ID:         20261113.g0649765eb57c (version 0.1.0-dev), the image the
                  heal workflow built and booted at 5ae57e3
Source revision:  feat/self-healing-awareness; proven at 5ae57e3. One later
                  commit holds two small fixes found in that run and this
                  report; it is NOT yet proven (see "After the proven run")

SUMMARY
- What changed: the owner's rule "the user is always kept aware" made a
  property of the design. Every repair TUR makes by itself is a desktop
  notice, and so is whether it held at the next scan; every fault waiting
  for the person, every "the problem you reported is fixed" and "it
  happened again" is one too; every notice points to the history. The
  notice program (tur-diag heal notify, in the person's session, started at
  every graphical sign-in and on every change of the healing state) marks
  a notice seen only when the desktop's notification service accepted it:
  without a session it waits. tur-diag heal history lists everything in
  plain words, newest first, with who acted. tur-diag heal decline records
  that the person does not want to send an offered report. P@x gets the
  history as a class 0 tool.
- Why it changed: the owner confirmed that only restarting a failed system
  service is automatic, and asked that the person always be kept aware.

FILES
- Added: tests/boot/guest/rootfs/usr/libexec/tur-boot-test/heal-notifications
  (a stand-in notification service, standard library only);
  image/rootfs/usr/lib/systemd/user/graphical-session.target.wants/
  tur-heal-notify.service (link); this report
- Modified: services/tur-diagnostics (src/heal/engine.rs: notices for
  automatic repairs and "held", history; state.rs: who acted, the account;
  src/heal_cli.rs: notify, history, decline; main.rs: a report not
  confirmed is recorded; agent tools: heal_history; tests);
  image/rootfs/usr/lib/systemd/user/tur-heal-notify.service;
  tests/boot/heal.py, heal-agent, heal-pax; tests/boot/guest/Containerfile;
  .github/workflows/heal.yml (lint list); docs/adr/0027-self-healing.md (the
  owner's decision and the rule); docs/architecture/self-healing.md
  (section 4 rewritten), pax-orchestrator.md (tool table)
- Not touched: services/tur-pax, desktop/, image/Containerfile,
  image/programs.tsv, docs/adr/README.md
- Removed: none

BUILD
- Command: CI only.
- Result: at 5ae57e3 the image built with the new tur-diag and booted
  (image 43520110470, boot 43520110505, heal 43520110415, rollback
  43520110721).
- Warnings: clippy has NOT RUN on this branch (see TESTS).

TESTS
- Unit tests: NOT RUN. The rust workflow (run 43520110458) was still
  queued, more than three hours after the push, when this report was
  written. Three new unit tests (notices for an automatic repair and its
  "held"; the history's order and who acted; the person's own request) and
  the changed ones are unproven, as are clippy and rustfmt for the new
  code. The program itself compiled (the image build) and ran (below).
- Integration tests: pax 43520110422 passed (the heal-request scenario
  with the new manifest); reports 43520110611 passed.
- Boot tests: heal 43520110415: 35 checks, all passed, the seven awareness
  checks A1–A7 among them (below). boot 43520110505 and rollback
  43520110721 passed with the new user unit in the image.
- Hardware tests: NOT RUN.
- Performance tests: NOT RUN.
- Plasma's own notification service: NOT RUN (a stand-in on the person's
  real session bus took the calls).
- More than ten notices waiting at once (the summing-up notice): NOT RUN.
- tur-diag heal decline: NOT RUN in the guest and without a unit test.
- release: cancelled by a later push of pull request 108's branch, not a
  failure of this change. iso: still running when this was written.

DIAGNOSTICS
- Artifacts: tur-heal of run 43520110415: guest/phase-a/notify/
  recorded.jsonl (every Notify call), pending-*.json (what waited),
  history-a.json and .txt, the same for phase-b.
- Relevant error IDs: none new.

RECOVERY
- Rollback procedure: revert the pull request. For one account:
  `systemctl --user mask tur-heal-notify.path tur-heal-notify.service`
  stops the notices; the history stays readable.
- Recovery tested: not applicable.

KNOWN LIMITATIONS
- See "Not proven" below.

NEXT SAFE ACTION
- Lead: let rust run on the head; apply its rustfmt patch if it has one
  (`scripts/dev/ci.py fixes RUN services/tur-diagnostics`); retarget the
  pull request to main once 108 is merged.
```

## What the guest test proves (heal run 43520110415, commit 5ae57e3)

The test user's own session manager ran (linger), with no graphical session
and no notification service at first. The notice program was the image's
(`tur-heal-notify.service`, `tur-heal-notify.path`, both from the image);
the "sign-in" of the test is test plumbing: a stand-in notification service
takes `org.freedesktop.Notifications` on the user's session bus,
`WAYLAND_DISPLAY` is set in the user manager, and the unit is started as
`graphical-session.target` would start it.

| Check | What happened |
|---|---|
| A1 | chronyd was killed and restarted by TUR by itself, and a second scan found the repair held, all with no graphical session. Two notices waited (`repaired`, `held`). The notice program run with no session bus printed "0 notice(s) shown; 2 wait for the next session" and marked nothing seen. The unit itself did not run (its condition: no display) |
| A2 | at the sign-in both were shown, in order: "chronyd.service had stopped working. TUR started it again and checked that it is running. TUR did this by itself; …", then "The repair TUR made by itself held: chronyd.service still works at its next check." Nothing waited afterwards |
| A3 | from then on the path unit showed each new event without the test asking: 13 notices for 13 events, one each, none twice: network time found and repaired (by P@x), the resolver found, repaired, undone, repaired (by the person), the disk found and not repaired, the crash's report offered, "happened again", offered again. Nothing waited at the end |
| A4 | all 13 notices end with `tur-diag heal history` |
| A5 | after the restart, with no session, "the problem you reported is fixed" was not shown (0 calls) and waited; at the next sign-in it was shown, once |
| A6 | `tur-diag heal history`, as the test user: 18 entries, newest first; who acted: TUR, P@x, you, you as administrator; kinds: found, repaired, held, undone, not repaired, report offered, report sent, report not sent (sending not confirmed), happened again, gone |
| A7 | P@x (scripted provider) called `heal_history` first; the service's own repair of chronyd was in what it was given |

The history as the person reads it (run 43520110415, shortened):

```text
2026-11-14T00:09:48Z  Fixed by the update to 0.1.0-dev: A TUR program crashed has not happened again.
2026-11-14T00:09:07Z [you]  A report was not sent: sending was not confirmed: tur-diag crashed.
2026-11-14T00:09:07Z [you]  You sent a report to TUR's developers: tur-diag crashed (report R-bbc5800be05068a0).
2026-11-14T00:09:07Z  A report to TUR's developers is offered for: A TUR program crashed (tur-diag).
2026-11-13T23:58:42Z [you, as administrator]  Not repaired: The system disk is almost full (/var). The check still finds the problem.
2026-11-13T23:58:42Z [you]  Undone, as asked: the repair of Name lookups bypass the system resolver.
2026-11-13T23:58:41Z [P@x (or another program of account 1000)]  Repaired: The clock is not kept right (network time). …
2026-11-13T23:58:33Z [TUR]  Checked again later: the repair of A system service stopped working held.
2026-11-13T23:58:27Z [TUR]  Repaired: A system service stopped working (chronyd.service). Clears the failed state of the stopped service and starts it once. No setting is changed. Checked afterwards: it works.
```

## After the proven run

Two defects seen in that run are fixed in the commit after 5ae57e3, which
has not been through CI:

1. The history named the one who made a repair as the one who later checked
   that it held ("[P@x] Checked again later: … held"). The check is TUR's;
   the line now says TUR.
2. The test waited ten minutes in its crash step: `wait` waited for the
   whole pipeline, whose `sleep 600` was stopped only afterwards. The order
   is swapped (test only).

## Not proven

- **Unit tests, clippy and rustfmt** for this change: the rust workflow had
  not started.
- **Plasma's notification service.** The calls are typed and follow the
  freedesktop specification, and a stand-in answered them; that Plasma
  shows them, and how they look, is not proven. `graphical-session.target`
  starting the unit at a real sign-in is not proven either: the test started
  the unit itself.
- **The summing-up notice** for more than ten waiting notices, and
  **`tur-diag heal decline`**: written, not run.
- **P@x telling the person.** The tool exists and its description asks for
  it; whether a real model does it is not proven, and no model was called.
  The guarantee is the desktop notice.
- **Several accounts.** Each account keeps its own "seen" list, so each is
  told; not tested with two.
- **Hardware**: NOT RUN.

## Follow-ups (other owners)

- Settings: a page that lists the history and the waiting faults
  (`tur-diag heal history --json`, `heal status --json`), with the
  buttons the notices only name as commands.
- P@x's window (tur-pax-ui) and the session service (tur-pax): show the
  history entries that are new since the window was last opened; today
  P@x has the tool and nothing makes it call it.
- Inside P@x's sandbox the person's own report decisions (sent, declined)
  are not visible, because the home directory is hidden there; the tool
  shows the system's part of the history.
- Notification actions (buttons) instead of commands in the text, once a
  resident session program exists to receive them.

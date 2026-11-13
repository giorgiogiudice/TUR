<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop-6 — tur-pax-ui: P@x's window (D-17)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-6 (task D-17 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop programs (desktop/apps/tur-pax-ui), P@x's keys
                  (desktop/kit/pax), desktop CI
Build ID:         CI only (desktop workflow; no image build: the image does
                  not install desktop programs yet, issue #59)
Source revision:  feat/pax-ui, pull request 102; head HEAD-PENDING

SUMMARY
- What changed: P@x has a window. tur-pax-ui is the ask bar (Meta+Space);
  the conversation panel, in which P@x's words and each step appear as the
  service signals them (the tool in plain words, the policy's decision and
  why, what it returned marked as data and not instructions), with Stop
  and Pause and the stop key on the button while a task runs, and the
  task's and the day's spending against the day's limit; P@x's record of
  what it did; and P@x's setup: P@x switched on with the consent text and
  its version; local AI with the hardware tiers, download sizes, licences,
  real download progress and Cancel; or a provider (DeepSeek and the other
  presets) with its data terms or "not reviewed by TUR", the key sent to
  the service and kept nowhere here; the model, and the generation
  settings it accepts, checked; the limits. P@x off, not set up, or not
  there is said plainly. P@x's three keys are bound by its desktop file
  from commands.json, and the check of that now reads every group of a
  desktop file.
- Why: P@x had a working session service and no user interface at all.
- What it does not do: voice; guide mode; the approval sheet (D-18); the
  settings pages (D-19); setup screens 6 and 7 (no screen control and no
  trusted screen yet); anything above class 1 (the service does none).

FILES
- Added: desktop/apps/tur-pax-ui/ (program; QML module Tur.PaxUi; data/;
  dbus/org.tur.PaxUi1.xml; fixtures/ with the recording of org.tur.Agent1
  and update.py; tests/ with the stand-in service, QML and unit tests, the
  pictures and the end-to-end QML test); desktop/tests/pax-ui/e2e.py;
  desktop/tests/screenshots/paxui-*.png (58); docs/architecture/
  tur-pax-ui.md; this report and docs/reports/assets/desktop-6/
- Modified: desktop/CMakeLists.txt (one add_subdirectory);
  desktop/ALLOWED_IMPORTS (Tur.PaxUi; LayerShellQt for the program only;
  Tur.Kit.Pax for tur-pax-ui); desktop/kit/pax/commands.json (ask, stop and
  keys bound); desktop/kit/pax/PaxCommands.qml (regenerated);
  desktop/kit/pax/gen_commands.py (every group of a desktop file);
  desktop/ci/screenshots.sh; .github/workflows/desktop.yml (job
  pax-ui-end-to-end; the workflow also runs on services/tur-pax and
  tests/pax)
- Removed: none. Not touched: services/**, desktop/apps/tur-centre/**,
  image/**, docs/adr/README.md, desktop-experience.md (text proposed below)

BUILD
- desktop workflow, builder image (Fedora 45, Qt 6.11.2), warnings as
  errors, and again with AddressSanitizer and UBSan: RUNS-PENDING

TESTS
- Unit and QML tests: RUNS-PENDING
- Screenshots: RUNS-PENDING
- End to end against the real tur-pax serve: RUNS-PENDING
- Hardware tests: NOT RUN. Under KWin: NOT RUN for tur-pax-ui. On the
  image: NOT RUN (not installed).

RECOVERY
- Revert the pull request. Nothing in boot, install or the image changes.

KNOWN LIMITATIONS
- See "Not verified" and docs/architecture/tur-pax-ui.md, section 12.

NEXT SAFE ACTION
- Owner: look at the pictures; decide the decisions below.
- Image owner: install the program and its files (tur-pax-ui.md, section 9).
```

RESULTS-PENDING

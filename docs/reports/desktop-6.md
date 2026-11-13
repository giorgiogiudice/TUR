<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop-6 — tur-pax-ui: P@x's window (D-17)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-6 (task D-17 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop programs (desktop/apps/tur-pax-ui), the kit's
                  P@x keys (desktop/kit/pax), desktop CI
Build ID:         RESULTS-PENDING
Source revision:  RESULTS-PENDING (feat/pax-ui, pull request 102)

SUMMARY
- What changed: P@x has a window. tur-pax-ui is the ask bar (Meta+Space),
  the conversation panel in which P@x's words and each step appear as the
  service signals them (the tool in plain words, the policy's decision
  and why, what it returned marked as data and not instructions), Stop and
  Pause with the stop key on the button while a task runs, the task's and
  the day's spending against the day's limit; P@x's record of what it did;
  and P@x's setup: switching P@x on with the consent text and its version,
  local AI with the hardware tiers, sizes, licences and real download
  progress and cancel, or a provider with its data terms and the key sent
  to the service and kept nowhere here, the model and the generation
  settings it accepts, the limits. P@x off, not set up, or not there is
  said plainly. P@x's three keys are bound by its desktop file from
  commands.json, and the check now reads every group of a desktop file.
- Why: P@x had a working session service and no user interface at all.
- What it does not do: voice; guide mode; the approvals sheet (D-18); the
  settings pages (D-19); screens 6 and 7 of the setup (no screen control,
  no trusted screen yet); anything above class 1 (the service does none).

FILES
- Added: desktop/apps/tur-pax-ui/ (program, QML module Tur.PaxUi, data,
  D-Bus interface, fixture of org.tur.Agent1 and its update script, tests,
  the stand-in service); desktop/tests/pax-ui/e2e.py;
  desktop/tests/screenshots/paxui-*.png (58); docs/architecture/
  tur-pax-ui.md; this report and docs/reports/assets/desktop-6/
- Modified: desktop/CMakeLists.txt (one add_subdirectory),
  desktop/ALLOWED_IMPORTS (Tur.PaxUi, LayerShellQt for the program,
  Tur.Kit.Pax for tur-pax-ui), desktop/kit/pax/commands.json (ask, stop
  and keys bound), desktop/kit/pax/PaxCommands.qml (regenerated),
  desktop/kit/pax/gen_commands.py (every group of a desktop file),
  desktop/ci/screenshots.sh (P@x's window), .github/workflows/desktop.yml
  (job pax-ui-end-to-end; runs on services/tur-pax and tests/pax too)
- Removed: none. Not touched: services/**, desktop/apps/tur-centre/**,
  image/**, docs/adr/README.md, desktop-experience.md (proposed text below)
```

RESULTS-PENDING

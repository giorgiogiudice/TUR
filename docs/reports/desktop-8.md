<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop-8 — P@x's character on the bar, first slice

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-8 (the owner's wish of 14 November 2026: "P@x to
                  be a sort of old Clippy, kind of omnipresent also on the
                  GUI, especially in the bottom bar"; issue #134)
Subsystem:        P@x on the desktop: a new program,
                  desktop/apps/tur-pax-companion; one property each on
                  tur-pax-overlay and tur-pax-ui, and where tur-pax-ui's
                  ask bar opens; desktop CI (two scripts, one more test
                  program under KWin)
Build ID:         not applicable: no image is built for this change
Source revision:  the head of pax-companion (pull request 139, opened
                  against #111's branch, ci/mirror-every-pin, because
                  main's desktop workflow cannot build its builder image
                  until #111 lands). CI RESULTS: see "CI runs" below

SUMMARY
- What changed: P@x has a character, a small flame, that lives in the
  bottom bar. It is a standalone program that draws its own layer-shell
  surface over the bar; nothing runs inside Plasma. It shows one of twelve
  states, says one sentence in a bubble with at most two actions when
  something happened that the person should hear of, and opens P@x's ask
  bar beside itself when pressed. It is bound by thirteen rules, each held
  by a test (docs/architecture/tur-pax-companion.md, section 3): it never
  takes the keyboard by itself, never covers a window, never blocks a
  click, never makes a sound, speaks first only about facts it read itself
  and only once, is quiet in one press, can be moved, hidden and brought
  back, never contradicts P@x's indicator, never starts P@x, and makes no
  suggestion unless the person switched suggestions on.
- Why: the owner's wish; desktop-experience.md, principles 3, 4, 6 and 7.
- org.tur.PaxOverlay1 gained Activity (what the pill says), so that the
  character can say the same. org.tur.PaxUi1 gained Mode (what the window
  shows), and the ask bar now opens beside the character when the character
  is on the bar. Both are additions.

FILES
- Added: desktop/apps/tur-pax-companion/ (CMakeLists.txt; src/{companion,
  companionsettings,placement,busproperty,agentsource,healsource,
  companionservice,surfaces}.{h,cpp}, main.cpp; qml/{Main,Stage,Character,
  StateMark,Leaf,Bubble,NoticeCard,MenuCard,WatchingCard,SettingsCard,
  Words}.qml; dbus/org.tur.PaxCompanion1.xml; data/{org.tur.PaxCompanion.
  desktop,org.tur.PaxCompanion1.service}; fixtures/{README.md,heal/
  notices.json}; tests/{CMakeLists.txt,tst_main.cpp,CompanionHost.qml,
  tst_character.qml,tst_stage.qml,tst_walks.qml,shots/render_companion.qml,
  unit/tst_logic.cpp,unit/tst_bus.cpp});
  desktop/tests/compositor/tst_companion.cpp;
  docs/architecture/tur-pax-companion.md; this report; reference pictures
  desktop/tests/screenshots/companion-*
- Modified: desktop/CMakeLists.txt (one line), desktop/ALLOWED_IMPORTS (the
  program's own names; no new library), desktop/ci/{compositor.sh,
  screenshots.sh}, desktop/tests/compositor/CMakeLists.txt,
  desktop/apps/README.md; tur-pax-overlay: src/{busadaptor,overlayservice}.
  {h,cpp}, dbus/org.tur.PaxOverlay1.xml, tests/unit/tst_bus.cpp;
  tur-pax-ui: src/{main.cpp,surface.h,surface.cpp,uiservice.h,
  uiservice.cpp}, dbus/org.tur.PaxUi1.xml;
  docs/architecture/{tur-pax-ui,tur-pax-overlay}.md
- Removed: none. Not touched: services/ (every proposal for a service is
  text), desktop/kit (no token, no control and no command was added),
  desktop/data, image/, .github/workflows/, docs/adr/README.md

BUILD
- Command: desktop/ci/build.sh ci and ci-sanitize (workflow desktop)
- Result: see "CI runs"
- Warnings: a warning fails the build and qmllint

TESTS
- See "What was tested" below.
- Hardware tests: NOT RUN.

DIAGNOSTICS
- Artifacts: desktop-build-test (test and lint logs), desktop-screenshots
  (the pictures), desktop-compositor (KWin's logs, the test logs, the
  screenshots KWin took, report.md)
- Relevant error IDs: none new. The character shows the service's own
  codes (TUR-AI-*, and the healing service's fault codes) and adds none

RECOVERY
- Rollback procedure: revert the pull request. Nothing installs or starts
  the program; the two properties and the ask bar's place go with it.
- Recovery tested: not applicable. The character changes nothing on the
  computer but its own settings file.

KNOWN LIMITATIONS
- See "Not run" and "Known gaps" below. The bar under KWin is a stand-in:
  Plasma's own panel was not run.

NEXT SAFE ACTION
- Owner: look at the pictures; decide the questions under "For the owner".
```

## What was built

The design is [tur-pax-companion.md](../architecture/tur-pax-companion.md).
In short:

| Part | Built |
|---|---|
| The character | A flame (TUR's own, the kit's `PaxFlame` drawing), twelve states, each with its own shape or mark and its own words; fixed palette; nothing loops unless P@x works or controls; three honest signals (today's limit, the battery, "this can be pressed") |
| Where it lives | A layer-shell strip over the bar, input only on the character and what is open; the bar's thickness from a probe surface and the layer-shell protocol alone; no applet, no KWayland |
| The rules | R1 to R13, as code (`Companion`) and tests |
| The bubble | Who says it, one sentence, a code, at most two actions; folds into a mark after 12 s; "Show me" opens the whole notice in place, or P@x's panel |
| The hand-over | One press opens `tur-pax-ui`'s ask bar beside the character (`Anchor`); the bubble makes room when the bar opens (`Mode`) |
| Quiet, hide, drag, menu | Quiet for an hour, until tomorrow, until I say; hide and bring back; drag along the bar; "What is P@x watching?"; a switch per kind of notice; Stop while P@x works |
| Sources | `org.tur.Agent1` through the recorded stand-in; the healing service's notices through a stand-in for `tur-diag heal notices --json`; P@x's indicator and window; the battery |
| Suggestions | Off by default; the rules for them; one plain rule (most of today's limit used) |

## For the owner

From [tur-pax-companion.md](../architecture/tur-pax-companion.md), section 9:

1. **The character's name**: none of its own; it is P@x.
2. **Notices on by default**: all five kinds (a repair TUR made, a problem
   waiting, P@x waiting for an answer, a task that ended or was refused, an
   update that undid itself).
3. **Suggestions on by default, ever**: no, recommended as a standing rule.
4. **A seat for it** at the end of TUR's default panel (a 48 px spacer, as
   data).
5. **Over a full-screen window**: it stays (built as the default, as
   asked); the recommendation is "steps aside" as the default.
6. **With P@x off**: absent by default.
7. **A key for "Go to P@x on the bar"** (`Meta+Alt+Space` proposed).
8. Whether the two added properties, and the ask bar opening beside the
   character, stand.

A decision record is needed (a new program of P@x's with its own interface;
the standing rule on suggestions). Its text is section 12 of the design.

## The healing service's notices

The character reads `tur-diag heal notices --json` and never marks a notice
seen. Today that means two announcements of one event (the desktop's
notification and the bubble). The handshake that makes it one, with
`tur-diag heal notify` still the only writer of what was seen and the
notification as the fallback whenever the character cannot speak, is
proposed as text in section 7 of the design; nothing under
`services/tur-diagnostics` was changed.

## Licence placement

All of tur-pax-companion is the project's own, side A: a `QGuiApplication`
on `Tur.Kit` and `Tur.Kit.Pax`, linking Qt (LGPL-3.0), `libturpaxui` (the
project's) and, by the program only, LayerShellQt (LGPL-2.1/3.0, read at
Plasma 6.8.2 for the overlay). Side C stays empty: no applet, no KWin
script. A Plasma applet was weighed and not written (design, section 2).
`boundary.py` passes on the sources and on what the binaries link.

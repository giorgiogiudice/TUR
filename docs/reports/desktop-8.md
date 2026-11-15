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

## The owner's decisions (15 November 2026)

The owner answered the first draft's eight questions ("I trust your
choices"); [tur-pax-companion.md](../architecture/tur-pax-companion.md),
section 9, records them and what each changed:

1. **No name of its own**: it is P@x.
2. **All five kinds of notice on by default** (a repair TUR made, a problem
   waiting, P@x waiting for an answer, a task that ended or was refused, an
   update that undid itself). As built.
3. **Suggestions never on by default**, as a standing rule; the one rule
   built (most of the day's spending limit used) stays off by default. As
   built.
4. **Over a full-screen window it steps aside** by default (it leaves with
   the bar and comes back only while P@x controls, sees the screen or waits
   for an answer); one switch keeps it. **Changed**: the default was "stay";
   `CompanionSettings.overFullScreen` is now false for a new person, and the
   unit, stage and KWin tests hold the new default.
5. **The 48 px seat** at the end of TUR's default panel: a follow-up after
   pull request #128 (the desktop programs in the image) merges.
6. **With P@x off it is absent** by default; "asleep on the bar" is the
   person's choice. As built.
7. **Meta+Alt+Space is P@x's fifth key**, "Go to P@x on the bar".
   **Changed**: the command `attend` in `desktop/kit/pax/commands.json`
   (bound: true, verified: false like the other four), bound by the action
   `attend` of the character's desktop file; `PaxCommands.qml` regenerated;
   `gen_commands.py --check` passes with five commands and four files that
   bind them. P@x's indicator's list of keys and P@x's window's line of keys
   show it, so their reference pictures were taken again.
8. **The interface additions stand** (`org.tur.PaxOverlay1.Activity`,
   `org.tur.PaxUi1.Mode`, the ask bar beside the character).

A decision record is needed (a new program of P@x's with its own interface;
the standing rule on suggestions). Its text is section 12 of the design.

## The healing service's notices: the handshake

Adopted by the owner with the decisions above, and built on the
character's side (design, section 7):

- `org.tur.PaxCompanion1.Announce(s source, t id) -> b taken`. "heal" is the
  one source. The character runs `tur-diag heal notices --json` itself,
  finds the notice by its id and answers true only once the bubble is on
  the screen (two frames drawn since it was set, at most 3 s, else it takes
  the bubble back and answers false). It answers false at once when it is
  hidden, asleep, quiet, already speaking, busy with the person, has spoken
  its number for the hour, the kind is switched off, or the notice is not
  waiting; it keeps nothing it refused.
- The character no longer watches the healing service's state file and no
  longer reads its notices by itself (`--heal-state` is gone). Until
  `tur-diag heal notify` calls `Announce`, the character says nothing of the
  healing service's and the notification shows every notice: one
  announcement, nothing lost.
- `tur-diag heal notify` stays the only writer of the seen list. Its side
  of the handshake is written as text for pull request #117's component in
  the design, section 7; nothing under `services/` was changed.

## Licence placement

All of tur-pax-companion is the project's own, side A: a `QGuiApplication`
on `Tur.Kit` and `Tur.Kit.Pax`, linking Qt (LGPL-3.0), `libturpaxui` (the
project's) and, by the program only, LayerShellQt (LGPL-2.1/3.0, read at
Plasma 6.8.2 for the overlay). Side C stays empty: no applet, no KWin
script. A Plasma applet was weighed and not written (design, section 2).
`boundary.py` passes on the sources and on what the binaries link.

## What was tested

**Nothing yet. At the time of writing, no workflow has started on this pull
request: its runs have been queued behind about fifty others for an hour.
Nothing below has been compiled, run or looked at. This section says what
was written and is waiting; it is to be replaced by what the runs show.**

Written, not yet run (the design, section 10, says what each proves):

| Test | Kind | State |
|---|---|---|
| `pax-companion-unit-logic` (26 functions) | Qt Test | NOT RUN |
| `pax-companion-bus` (11 functions; private bus, the recorded stand-in for `org.tur.Agent1`, the program itself) | Qt Test | NOT RUN |
| `pax-companion-qml`: `tst_character`, `tst_stage`, `tst_walks` (27 scenes: contrast and gold in both variants, keyboard in both directions, three text sizes and lengths) | Qt Quick Test | NOT RUN |
| `pax-companion-starts-dark`, `-light`, `-hide-without-a-bus`, `-desktop-file` | ctest | NOT RUN |
| `pax-overlay-bus`, extended for `Activity` | Qt Test | NOT RUN |
| qmllint, the sanitizer build, `boundary.py --linked` | CI | NOT RUN |
| Pictures: 27 scenes and the sheet of states, dark and light (56) | `screenshots` job | NOT RENDERED. No reference picture exists; the design's pictures do not exist yet |
| `tst_compositor_companion`, under KWin, on a stand-in bar | `compositor` job | NOT RUN |

Run on the development machine, because they are plain Python and compile
nothing: `desktop/ci/boundary.py` (clean: 821 files), `gen_commands.py
--check`, `gen_tokens.py --check`, and the syntax of the two shell scripts.

## Not run

- **Everything above**, until CI reaches it.
- **Plasma's own panel**: the builder image has KWin and not Plasma's shell;
  the compositor test's bar is a stand-in. A floating panel, a panel at
  another edge, one that hides, two panels: all untested.
- **The real image and a real session**: the program is not installed and
  nothing starts it (design, section 8).
- **The real `tur-pax` and the real `tur-diag`**: recorded answers of the
  first, a hand-written fixture of the second's output.
- **A screen reader on the real program**; the walks read Qt's
  accessibility interfaces.
- **Voice**: not built in P@x.
- **More than one screen; a battery.**

## Known gaps

The design's section 11: two announcements of one repair until the healing
service's handshake exists; the program stays in memory while P@x is off;
no seat in the default panel; the healing fixture is written by hand; "step
aside" shares the top layer with the bar; no announcement to a screen
reader; no source for "an update undid itself".

## Follow-ups

1. Bring the runs to green; accept the reference pictures after looking at
   every one of them; replace "What was tested" with what was measured.
2. Into the image with the other desktop programs, after pull request #128:
   the program, its library, its launcher entry and bus activation file, in
   #128's `image` component (the design's section 8 lists them), the seat in
   `desktop/data`'s panel, and `tur-pax` starting the character when P@x is
   switched on.
3. The healing service's handshake (design, section 7), with pull request
   #117's component.
4. Record `fixtures/heal/notices.json` from the `heal` workflow.
5. Retarget this pull request to `main` once #111 has landed.

## CI runs

| Workflow | Run | Commit | Result |
|---|---|---|---|
| desktop | 43750686749 | b5a3be4 | queued; not started when this was written |
| pax, rust, iso, gate | 43750686695, 43750686662, 43750686773, 43750686631 | b5a3be4 | queued |

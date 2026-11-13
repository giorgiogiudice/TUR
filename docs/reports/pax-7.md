# pax-7 — P@x through the accessibility tree: the tur-pax-a11y bridge, measured on Qt, GTK, Firefox and Wine

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-7
Subsystem:        AI control layer: tier 2 (the accessibility tree); the
                  policy engine's accessibility actions; P@x's task loop;
                  CI under KWin
Build ID:         not applicable: this task builds no image of its own (the
                  image and boot workflows ran on the pull request)
Source revision:  pull request 93 (feat/pax-accessibility); commits and
                  runs under "Runs". The code is 84377f8; the commit
                  that adds the last figures to this report follows it
                  and changes no code

SUMMARY
- What changed: P@x can read and operate other applications' windows
  through AT-SPI. A new program, tur-pax-a11y, is the only process that
  connects to the accessibility bus; the task loop starts it as a child on
  a pipe and offers a model eleven typed tools (tur-pax run --interface).
  Each call's target is read from the bus, decided by the policy engine
  with new accessibility operations and roles, recorded, read and decided
  again by the bridge, shown with P@x's pointer resting on it, and only
  then done. Nothing above class 1 runs: Send, Delete, a line break typed
  and the like are refused with a plain message, and nothing happens.
- Why: the owner's founding requirement that the AI can "see and fully
  control all the apps"; addendum A, section 4, tier 2.
- Measured under a headless KWin in CI, with the real session bus, the
  real accessibility bus (at-spi2-core 2.62.1, by activation) and P@x's
  real indicator: P@x read, focused, pressed and typed in GTK 3 and GTK 4
  (gnome-text-editor); read, focused and pressed in Qt Quick (tur-settings)
  and Firefox 160; was refused Send, Delete draft and a line break; was
  stopped mid-action by switching its indicator off. It could NOT open
  the Appearance page of tur-settings (Qt Quick list rows offer no press
  action), could NOT type into a Firefox page field (Firefox answers
  "done" and nothing arrives), and Wine 11.20 exposes nothing.
```

## What was built

| Part | Where |
|---|---|
| The bridge, `tur-pax-a11y` (`serve`, `survey`): AT-SPI over D-Bus with `tur_pax::bus`, no client library linked | `services/tur-pax-a11y/` (new component: `atspi`, `identity`, `tree`, `overlay`, `act`, `serve`, `survey`) |
| The interface tools, the protocol types, `Interface`, `BridgeInterface` (child process, stop polling, kill), the window list screened against the user's policy | `services/tur-pax/src/interface.rs` (new) |
| The policy engine's accessibility actions: `UiOperation`, `UiRole` (from AT-SPI's 131 roles), `UiDetail` (role, description, default control, editable, typed text, other identifiers); the rules of `pax-accessibility.md` §5.2; malformed bridge actions; every identifier matched against the lists; the audit summary (operation, role, characters; never names or text) | `services/tur-pax/src/{action,engine,audit,lib}.rs` |
| The registry's interface tools (offered only on request; their names reserved) and the task loop's resolve → decide → perform path, the owner from the bus, unattended tasks rejected, a stop from the bridge ending the task | `services/tur-pax/src/{registry,orchestrator}.rs` |
| Five `TUR-AI-INTERFACE-*` codes | `services/tur-pax/src/faults.rs` |
| `tur-pax run|chat --interface [--interface-bridge FILE]`; the session service passes `NoInterface` | `services/tur-pax/src/{textui,service}.rs` |
| Tests | `services/tur-pax/tests/interface.rs` (new), `tests/invariants.rs` (21 bridge actions × 11 applications added to the sweep), unit tests in `engine.rs`, `interface.rs` and every module of the bridge |
| CI | `.github/workflows/pax.yml`, job `accessibility`; `tests/pax/a11y/` (`Containerfile`, `run.sh`, `harness.py`, `mail_app.py`, `page.html`) |
| Documents | `docs/architecture/pax-accessibility.md` (new), `docs/architecture/agent-policy.md`, this report |

How it is kept away from P@x's model and the command sandbox, the
classification rules, the stop paths and the limits:
[pax-accessibility.md](../architecture/pax-accessibility.md), sections 2, 5,
7 and 9.

## Runs

All on pull request 93. The runners were shared with other work; several
runs waited an hour in the queue.

| Commit | Workflow, run | Result | What it showed |
|---|---|---|---|
| eb347ab, c9abb02 | pax 43368158111, 43368293121 | fails | `tur-pax` did not compile: `Read` not imported for the bridge's reader. The session test failed because the binary was missing |
| 405f7cb | pax 43368692234 | fails | **The first run of the whole chain under KWin**: the accessibility bus came up by activation of at-spi-bus-launcher; Qt, GTK 3, GTK 4 and Firefox appeared on it; 68 of 69 checks passed; Send, Delete draft and a line break refused; P@x stopped mid-action. Failed: the Wine phase (`wineboot` not on the path). Found: the test read standard error with two readers (lost and merged log lines, so screenshots were taken at the wrong moment); Firefox answered `GetActions` with key bindings (`;;`) where names belong; a label and its field shared a name ("Search terms"), which was ambiguous; highlights accumulated |
| cae3a74 | pax 43373170495 | fails | With those fixed: Firefox pressed "Preview" (the page's script ran: "Previewed: "), but typed text did not arrive although Firefox answered "done". Wine: the test's `ps` is not in the image |
| ac6c446 | rust 43374066112 | fails | Clippy: a complex tuple type (`tree.rs`); `tests/tools.rs` matched `Performer` without the new variant; rustfmt |
| f34006b | pax 43374530238 | **pass** | 69 checks, 0 hard failures (table below). rust: `services/tur-pax` passed clippy and every test; `services/tur-pax-a11y` passed clippy and its 23 unit tests and failed only on rustfmt of one line |
| 84377f8 | pax 43375930833, rust 43375930829, image 43375930839, boot 43375930812, gate 43375930823 | **pass** | Every workflow on the commit green. rust: every component, clippy `-D warnings`, all tests (tur-pax with the new sweep, `tests/interface.rs`; tur-pax-a11y's unit tests). pax: the end-to-end and session tests unchanged and passing; the accessibility job 69 checks, 0 hard failures, reads now photographed too (every screenshot of every task but the stop shows P@x) |

## Results under KWin

Fedora 45 in the desktop builder image with the test's additions
(`tests/pax/a11y/Containerfile`): KWin 6.8.2, Qt 6.11.2, at-spi2-core and
at-spi2-atk 2.62.1, GTK 3.24.52, GTK 4.24.1, gnome-text-editor 51.0,
Firefox 160.0, Wine 11.20. KWin headless on its virtual backend at 1280 ×
800, one output, scale 1. Accessibility switched on through
`org.a11y.Status.IsEnabled`; tur-settings also started with
`QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1`.

### Coverage per toolkit (run 43375930833, `coverage.json`, `survey-*.json`; run 43374530238 gave the same figures but one)

What `tur-pax-a11y survey` read of the one window of each application:
elements visited after pruning what is not shown, and of them how many have
a name, a position (`GetExtents`, window coordinates, non-empty) and take
text; which interfaces they offer; which actions. "Act" is what P@x's tasks
did through the bridge in the same run.

| Toolkit (application) | Elements | Named | Positioned | Editable | Interfaces (elements offering) | Actions offered (count) | Read | Focus | Press | Type |
|---|---|---|---|---|---|---|---|---|---|---|
| Qt Quick (tur-settings) | 27 | 22 | 27 | 1 | Action 27, Component 27, Text 4, Value 2, EditableText 1 | SetFocus 13, Increase 4, Decrease 4, Press 1 | yes | **yes** | **no** for list rows (only SetFocus offered); 1 element has Press | not tried (the search field's change is not a person's edit for Qt, limit 3) |
| GTK 3 (test window, at-spi2-atk) | 14 | 10 | 14 | 3 | Component 14, Text 7, Action 5, EditableText 3, Value 2 | click 3, activate 2 | yes | – | **yes** (Bold; the application saw it) | **yes** (Message holds "hello"; the application saw it) |
| GTK 4 (gnome-text-editor) | 83 | 12 | 55 | 1 | Component 83, Action 78, Text 3, EditableText 1 | click 7, clipboard.copy/cut/paste 3 each, menu.popup 3, selection.delete 3 | yes | – | – | **yes** ("hello" in the document, read back) |
| Firefox 160 (Gecko) | 101 | 51 | 86 (85 in the earlier run) | 2 | Action 101, Component 101, Text 67, EditableText 67, Selection 1 | clickAncestor 39, click 24, press 21, jump 6, activate 2, switch 1 | yes (page and browser) | yes (the field took the keyboard) | **yes** (Preview: the page's script ran) | **no**: answered "done", nothing arrived (read back) |
| Wine 11.20 (notepad, Wine's own) | **nothing on the bus**: before and after notepad the bus listed only kwin and tur-pax-overlay; notepad.exe and wineserver were running and the window was on screen (screenshot `wine-notepad.png`) | | | | | | no | no | no | no |

Positions were exact where looked at: the highlight sat on the GTK 3
"Bold" button, the GTK 3 Message field, gnome-text-editor's text view (a
GTK 4 window with client-side decoration), tur-settings' "Appearance" row
and Firefox's "Preview" button and search field (screenshots below).

### The tasks (runs 43374530238 and 43375930833, the same results)

| Task | Request | Decisions (class, decision) | Outcome | What happened |
|---|---|---|---|---|
| settings-appearance | "Open the Appearance page in Settings and tell me the current theme." | 0 allowed (list), 0 allowed (find "Appearance": 1 match), 1 allowed (press), 1 allowed (focus), 0 allowed (find "desktop is set to") | completed | The press failed with `TUR-AI-INTERFACE-003`: "the element offers no action to press through the accessibility tree; it offers: SetFocus". Focus worked. The page did not open; the theme row was not found. **The task as asked was not achieved.** |
| mail-bold-and-type | "make it bold and type hello into the message" | 0, 1, 1 allowed | completed | The application logged `clicked:Bold` and `text:Message:hello` |
| mail-send-refused | "Press Send." | 2 refused, `TUR-AI-POLICY-006` | completed | Nothing performed; the bridge never pointed; the application saw nothing |
| mail-delete-refused | "Click Delete." ("Delete draft") | 2 refused, `TUR-AI-POLICY-006` | completed | The same |
| mail-type-enter-refused | "Type ok and press Enter" (`"ok\n"`) | 2 refused, `TUR-AI-POLICY-006` | completed | The same |
| editor-hello | "Type 'hello' into the open text editor." | 0, 1, 0 allowed | completed | The field the editor gives the keyboard was found with no name given; "hello" read back from it |
| firefox-page | "type hello into the search terms and preview it" | 0 allowed, 1 allowed (type), 1 allowed (press Preview), **2 refused (Send)**, 0 allowed | completed | Typing failed (read back, `TUR-AI-INTERFACE-003`); Preview pressed (status "Previewed: "); Send refused |
| stop-while-acting | "Press Bold twice." | 1 allowed | **cancelled, `TUR-AI-INTERFACE-004`** | The test switched the indicator off (`Hide`, 7 ms) when the bridge said the pointer had arrived on Bold; the bridge stopped during its 600 ms rest; Bold was never pressed; the second press was never decided |

For every task the trail began and ended with the task, every proposal was
a decision or a rejection, nothing was performed before its decision, and
no decision above class 1 was anything but refused (checked by the test for
each).

### Screenshots

Run 43375930833. Taken from KWin (`ScreenShot2.CaptureActiveScreen`) when the bridge logged
that P@x's pointer had arrived, 0.3 s later (the highlight fades in), inside
the bridge's 600 ms rest before acting. In the artifact `pax-accessibility`
of the run, `screenshots/`. Looked at:

| File | Shows |
|---|---|
| `mail-bold-and-type-2.png` | P@x's pointer on the GTK 3 "Bold" button, "Nothing has been sent." still showing (before the press); the window framed in gold; the pill "P@x is controlling · Stop: Meta+Esc · Keys: Meta+/" |
| `mail-bold-and-type-3.png` | The pointer and highlight on the Message field |
| `settings-appearance-2.png` | The pointer resting on tur-settings' "Appearance" row, the row highlighted exactly, caption "P@x: focus" (the press before it failed) |
| `editor-hello-1.png` | gnome-text-editor's text view (a GTK 4 window drawing its own decoration) highlighted under its header bar, the pointer resting on it, caption "P@x: type", the document still empty; in the next frame it holds "hello" |
| `firefox-page-2.png`, `-3.png` | The highlight on the search field (focused, empty) and on "Preview", the pointer on it |
| `stop-while-acting-1.png` | After `Hide`: no indicator, no pointer (43 sampled gold pixels: the window icon) |
| `wine-notepad.png` | Wine's notepad on screen through Wine's Wayland driver, with nothing of it on the accessibility bus |

A screenshot is counted as showing P@x at an element when it has more than
100 sampled gold pixels (#E8B44C ± 24) above a baseline taken with the
indicator holding a lease and pointing at nothing; that baseline had no
gold in this run (the indicator had not drawn its pill yet), so the
threshold was 100. The pictures listed above were looked at, not only
counted. One thing seen in them: the caption beside a highlight can cover
the next control (on the GTK 3 window it covers "Send").
In run 43374530238 the last read of three tasks was photographed after
the task had ended (no gold); from 84377f8 reads rest 600 ms as actions do,
and every one was photographed with P@x on screen.

## What the tests prove

| Property | Test |
|---|---|
| The class comes from the bus, never from the model; Send and Delete are class 2 even when the model calls the button a label | `tests/interface.rs`: `send_and_delete_are_refused_and_nothing_happens`; the engine sweep |
| Nothing above class 1 runs, for every interface tool, in every situation | `tests/interface.rs`: `nothing_above_class_one_is_performed_through_the_interface_in_any_situation` (11 tools × 9 targets × 4 policies × attended or not × locked or not); `tests/invariants.rs` over 1 137 600 decisions; the bridge's `second_decision` tests |
| Typing is class 1 into a field; class 2 with a line break, into a password field, a field that takes no text, a terminal, "Amount to transfer" | `tests/interface.rs`, `src/engine.rs`, `tests/invariants.rs`; CI `mail-type-enter-refused` |
| Excluded, administrator and unknown-owner windows | `tests/interface.rs`; the sweep |
| An unresolved target is never decided; an unattended task does not use the interface | `tests/interface.rs` |
| The stop control kills a bridge that never answers, within the bound | `tests/interface.rs`: `the_stop_control_abandons_a_call_in_flight_and_kills_the_bridge` |
| Switching the indicator off stops an action in flight | CI `stop-while-acting`; `overlay::tests` (only the lease's overlay) |
| Every decision is in the audit trail before anything runs; no title, name or typed text in it | `tests/interface.rs`: `an_ordinary_press_is_decided_from_the_bus_recorded_and_performed`, `typing_is_class_1_…`; CI trail checks for every task |

## Not proven

- Nothing ran on the TUR image, in a Plasma session, on hardware, with a
  person, or at a scale other than 1. The session service does not offer
  the interface yet (`NoInterface`), so `CancelTask` reaching the bridge is
  proven with the task loop's `SharedControl` path in a Rust test only.
- KDE applications built on Qt Widgets (Dolphin, Kate, System Settings) were
  not measured; Qt was measured with TUR's own Qt Quick program.
- Expanding, collapsing, selecting and scrolling were proven against the
  stand-in bridge only, not under KWin.
- Only one window per application; no multi-window application, no dialog,
  no menu opened.
- No real model was called; how a model uses these tools is not known.
- Firefox's chrome (menus, tabs) was read, not operated.
- The stop key is not bound; stopping by `Hide` is proven, by a key is not.

## Decisions the owner may want to reverse

1. **A separate program on a pipe**, not a module of tur-pax and not a D-Bus
   service (pax-accessibility.md, section 2).
2. **The bridge decides again by itself** with the built-in lists and the
   strictest context, and refuses a target that changed in any way since
   the decision.
3. **Focusing or scrolling an element without a name is class 1**; pressing
   or selecting one is class 2.
4. **A line break, a tab, any control character, a line or paragraph
   separator or a direction mark in typed text makes typing class 2.**
5. **Every identifier of an application counts**: its desktop file, its
   program, its Flatpak identifier, its name on the bus; one match on a list
   is enough.
6. **No overlay, no accessibility**: the bridge reads and acts only while
   P@x's indicator holds its lease and is shown, and rests 600 ms on the
   element before acting.
7. **When an application takes the text later or not at all**, the bridge
   sets the field's whole text (what it was, with the new text at the
   caret) once, then reports failure if the text is still not there.
8. **`ui_list_windows` is decided as reading the registry**
   (`org.a11y.atspi.Registry`), with excluded and administrator windows
   left out and sensitive windows' titles withheld.
9. **The CI job lives in the pax workflow**, using the desktop builder image
   from the cache; it triggers on tur-settings, the overlay, the kit and
   the builder as well.

## Proposed text for ai-control-layer.md (not edited)

A new section after 23:

> ## 24. Resolutions from building the accessibility bridge
>
> Building P@x's way of acting through the accessibility tree
> ([pax-accessibility.md](pax-accessibility.md), [report pax-7](../reports/pax-7.md))
> settled further points. Where an earlier section says otherwise, this
> section governs.
>
> | # | Point | Resolution |
> |---|---|---|
> | 1 | Section 19, item 1: the command sandbox denies the session bus, and the accessibility bus is reached through it. | The accessibility tier runs in one program of its own, `tur-pax-a11y`, a child of the task loop on a pipe, outside the sandbox. The task loop never connects to the accessibility bus; the model is offered typed tools and never names a bus, a path, an interface or a method. Commands are unchanged: no bus. |
> | 2 | Section 5.1: "a click is only a click". | The bridge reads what the click is on: the element's role, name, description and states, and the application's identifiers. An accessibility action states its operation (read, focus, scroll, expand, collapse, select, press, type); the engine's rules for it are in agent-policy.md, section 2.2. Typing a line break, into a password field or a terminal, and pressing the default control of a dialog are class 2. |
> | 3 | Who an application is. | Every identifier found (desktop file, Flatpak identifier, program, name on the bus) is matched against the lists; one match is enough. |
> | 4 | Between the decision and the action. | The bridge reads the target again, refuses if it changed in any way, and decides again by itself with the strictest context before it acts. |
> | 5 | Sections 16.6 and 19, item 3: the pointer shows where P@x acts; access only while the indicator is shown. | The bridge works only while it holds a lease on the indicator and the indicator is shown. It frames every window it reads, sends P@x's pointer to every element it acts on, and rests there 600 ms before acting. |
> | 6 | Section 6, item 5, and section 23, item 2: stopping. | Cancelling a task kills the bridge at once. Switching the indicator off, or the indicator going, ends the lease, and the bridge abandons what it is doing at its next step. |
> | 7 | Section 15, question 2: a bridge from Windows accessibility to AT-SPI. | None: Wine 11.20 as Fedora 45 ships it puts nothing on the accessibility bus (measured, pax-7). Windows programs stay on tier 3. |
> | 8 | Section 4, coverage table: "Native Linux (Qt, GTK): Tier 2 Yes". | Measured per toolkit (pax-7): GTK 3 and GTK 4 read, press and type; Qt Quick reads and focuses, but its list rows offer no press action; Firefox reads, focuses and presses, and its page fields take no text through AT-SPI. Tier 2 is "where the application's controls expose it", per control. |
> | 9 | Section 17, item 8: an unattended task uses no accessibility actions. | Enforced by the task loop: the call is rejected before it is resolved. |

In section 4's coverage table, "Windows applications under Wine: Tier 2
Not assumed" becomes "No (measured, pax-7)", and section 15, question 2, is
answered by item 7.

## Follow-ups

1. **Desktop kit**: `Kit.ListRow` (and any `T.ItemDelegate` used as
   navigation) should declare `Accessible.onPressAction: control.clicked()`
   so that tur-settings' page list can be opened through the accessibility
   tree; then the Settings task of this report can complete. Owner: the
   desktop work.
2. **tur-settings' search field**: react to a text change made through the
   accessibility tree (Qt Quick sets `text` without `textEdited`).
3. **The session service** offers the interface once the overlay's owner
   check exists (tur-pax-overlay.md, section 6) and a setting turns it on.
4. **Firefox**: report upstream that `EditableText.InsertText` and
   `SetTextContents` on a content field answer success and do nothing in
   Firefox 160 under at-spi2-atk; until then, typing into web pages needs
   tier 3.
5. **The engine rule** for unattended accessibility actions (rejected by the
   task loop today) in the decision table itself.
6. **Measure Qt Widgets applications** (Kate, Dolphin, System Settings) and
   a GTK application with a menu and a dialog; exercise expand, collapse,
   select and scroll under KWin.
7. **Exclude P@x's own indicator** from what P@x lists and reads.
8. **Bind the stop key** (desktop-experience.md; addendum A, section 23,
   item 2) to the same stop.
9. **Switch accessibility on in the image** (`org.a11y.Status.IsEnabled`)
   when P@x is switched on, so that Qt and Firefox expose their trees.

## Anything found wrong

- Qt Quick: a `T.ItemDelegate` with the role `ListItem` exposes no press
  action (`qaccessiblequickitem.cpp`, `actionNames`); TUR's settings window
  cannot be navigated through the accessibility tree.
- Firefox 160: `GetActions` answers key bindings in the name field through
  at-spi2-atk; `EditableText` answers success without effect.
- at-spi2-atk answers `InsertText` with true whatever happened (ATK's insert
  returns nothing), so a client must read the text back.
- tur-pax-overlay is itself on the accessibility bus, with a window.

# tur-pax-overlay: P@x on the screen

How P@x's presence on the screen is built: the activity indicator, P@x's own
pointer, its highlights and guide mode's caption; the D-Bus interface that
drives them; how a rectangle inside another program's window becomes a place
on the screen under Wayland, and the limits of that; and what the overlay is
not. The design is [desktop-experience.md](desktop-experience.md), sections
9.3 to 9.6, 9.13 and 14.1; the rules behind it are the AI control layer's
([ai-control-layer.md](ai-control-layer.md), 8, 16.6 and 19). The decision on
window positions is [ADR-0020](../adr/0020-overlay-window-positions.md).

Status: task D-15 of the design's plan. Built and tested in CI under a real
KWin; not in the image (no desktop program is yet: issue #51). Report:
[desktop-4.md](../reports/desktop-4.md).

## 1. What it is

`tur-pax-overlay` is a standalone Qt Quick program. Under KWin it puts one
layer-shell surface on every output, on the overlay layer, which is KWin's
topmost. Each surface is anchored to every edge of its output, has an empty
input region and asks for no keyboard. It is mapped only while there is
something to draw and unmapped the moment there is not, and screens that are
added or removed are followed.

It draws:

- **The pill** of the activity indicator, at the top centre of every output,
  in the designed states, and, while P@x is switched on, at all times: idle
  included, it shows P@x's stop combination and the key of the list of its
  keys (section 5.1).
- **The list of P@x's keys**, under the pill, when asked for.
- **The frame** round the window P@x is reading or acting in, or at the edge
  of the screen.
- **P@x's pointer**, a gold flame that travels; and a click shown as a ring.
- **Highlights** round rectangles, with dimming of the rest of the window and
  a caption beside them.

It does nothing else. It injects no input, reads no screen content, holds no
authority, and changes nothing in any program.

## 2. Structure

Code: `desktop/apps/tur-pax-overlay/`.

| Part | Files | What it is |
|---|---|---|
| What is shown | `src/overlaystate.*` | The state every surface draws from, in global logical pixels; P@x's pointer's journey |
| Who may draw | `src/overlayservice.*` | Leases, checks of every call, windows and rectangles in them, keeping a highlight on its window |
| The bus | `src/busadaptor.*`, `src/bustypes.*`, `dbus/org.tur.PaxOverlay1.xml` | Section 3 |
| Where windows are | `src/windowlocator.*` (the interface), `src/kwinlocator.*` (KWin) | Section 4 |
| The surfaces | `src/surfaces.*` | One layer-shell surface per output |
| The program | `src/main.cpp` | The command line, the log |
| What is drawn | `qml/OverlaySurface.qml`, `ActivityPill.qml`, `StateGlyph.qml`, `PaxPointer.qml`, `PointerRing.qml`, `HighlightMark.qml`, `DimLayer.qml`, `CaptionBubble.qml`, `ScreenFrame.qml`, `Words.qml` | Section 5 |

The QML and the logic are one module, `Tur.PaxOverlay` (`libturpaxoverlay`),
which links nothing of KDE's, so that its tests run under the offscreen
platform. The program adds the two things only a Wayland session has:
layer-shell surfaces (LayerShellQt) and the window locator (KWayland).

```text
tur-pax-overlay [--hide] [--commands] [--no-dbus] [--check]
```

`--hide` tells a running overlay to switch off and exits (0 also when none is
running). `--commands` shows the list of P@x's keys on the running overlay, or
takes it away. `--check` loads the surfaces, reports any QML warning and exits.

## 3. org.tur.PaxOverlay1

Session bus; name `org.tur.PaxOverlay1`; path `/org/tur/PaxOverlay1`. The
XML is `desktop/apps/tur-pax-overlay/dbus/org.tur.PaxOverlay1.xml`; it is
**proposed, not frozen**. The adaptor is written by hand; two tests compare
what the running program offers on the bus with the XML (one under the
offscreen platform, one under KWin).

```text
Methods   Acquire(s purpose) -> s lease
          Release(s lease)
          SetActivity(s lease, s state, a{sv} detail)
               state: idle | listening | working | observing | acting | waiting | paused
               detail: scope (s: window | screen); pid (u), app_id (s), title (s)
          MovePointer(s lease, s screen, d x, d y, a{sv} options)          options: slow
          PointAt(s lease, s screen, (dddd) rect, a{sv} options)           options: slow, anchor
          PointInWindow(s lease, u pid, s app_id, s title, (dddd) rect, a{sv} options)
          PulsePointer(s lease, s kind)                                     kind: click
          HidePointer(s lease)
          HighlightRect(s lease, s screen, (dddd) rect, a{sv} options) -> u id
          HighlightInWindow(s lease, u pid, s app_id, s title, (dddd) rect, a{sv} options) -> u id
               options: dim (b), duration (d), caption (s), step (s), point (b), slow (b), anchor (s)
          SetCaption(s lease, u id, s text, as controls)
          Clear(s lease, u id)                                              0: every highlight of the lease
          Hide()
          ShowCommands(b shown)                                             the list of P@x's keys
Signals   PointerArrived(s lease)  HighlightLost(u id, s reason)  LeaseEnded(s lease, s reason)
          StopRequested()  PauseRequested()  ResumeRequested()  GuideControl(u id, s control)
Property  Shown (b)  Screens (as)  CommandsShown (b)
```

**Coordinates.** Logical pixels, as KWin and Qt count them. A `screen` is an
output's name and the coordinates are relative to it; an empty `screen`
means global coordinates in KWin's layout of all outputs. A rectangle "in a
window" is relative to the top left corner of the window's content, which is
what `org.tur.Settings1.Reveal` returns.

**Anchors.** Where the pointer's tip goes on a rectangle: `center` (the
default), or `start` and `end`, the middle of the left or right edge inset by
a quarter of the height.

**Errors**, each message beginning with its code: `NoLease`
(`TUR-DESKTOP-PAXOVERLAY-001`: the caller holds no such lease; another
connection's lease is answered the same way, so nobody learns whether a token
is in use), `InvalidArgument` (002: an unknown state, scope, screen, anchor
or pulse; a rectangle that is not finite, has no area or lies beyond 100,000
px; a caption or step over 300 characters), `WindowNotFound` (003: no window
matches, with the reason the locator gives), `UnknownHighlight` (004),
`LimitReached` (005: 8 leases in all, 16 highlights a lease).

**Signals go to one connection.** `PointerArrived`, `HighlightLost` and
`LeaseEnded` are sent as unicast signals to the connection that holds the
lease, so that a lease's token is not told to anyone else on the bus. The
other four are declared for the design's buttons and are never sent (section
7).

**Shown** is true while P@x is on (some lease is held, whatever the state)
and a frame of the overlay's surface has reached the compositor on every
output. **Screens** lists the
outputs where that is so. Both send `PropertiesChanged`.

**Differences from the design's sketch** (14.1):

| Sketch | As built | Why |
|---|---|---|
| `MovePointer(lease, screen, x, y, options)` | The same, and `PointAt` and `PointInWindow` beside it | P@x points at rectangles: at a row that `Reveal` returned, in a window |
| — | `Hide()` without a lease | Switching P@x's presence off must always be possible and instant (section 6) |
| — | `LeaseEnded` | A lease holder must learn that its lease ended (by `Hide`, for instance) so that, under the design's lease rule, capture and input stop |
| — | `Screens` | What "every screen concerned" means, checkable |
| — | `ShowCommands`, `CommandsShown` | The owner's answer 17.1.2: the list of P@x's keys one action away from the indicator |
| Idle: no pill | Idle with a lease held: the pill, with the stop keys | The owner's answer 17.1.2 |
| `HighlightLost` reasons "window moved, covered or closed" | `resized`, `minimized`, `covered`, `closed`, `expired` | A highlight follows its window when it moves (section 4) |
| `SetCaption(..., as controls)` | `controls` accepted and not drawn | Section 7 |

## 4. Where a window is

`Reveal` says where the row is in the window. Where the window is, only KWin
knows. What KWin 6.8 offers an ordinary client, the options and the choice
are in [ADR-0020](../adr/0020-overlay-window-positions.md); in short:

- **Mechanism.** The Plasma window-management protocol
  (`org_kde_plasma_window_management`, version 21 in KWin 6.8.2), read with
  KWayland's client library. KWin offers it to every client that is not
  sandboxed. For each managed window it gives the pid, the application
  identifier, the title, the frame geometry, the client-area geometry, the
  minimised and active states and the stacking order, in global logical
  pixels, with change events.
- **The sum.** A window is found by pid, application identifier and title
  (0 or empty: any; several matches: the one stacked highest). A rectangle in
  it is placed at the client area's top left corner plus the rectangle.
- **Kept true.** A highlight in a window follows the window when it moves.
  It is removed, with `HighlightLost`, when the window changes size (the
  content may have moved inside it: the rectangle may no longer be the row),
  is minimised, closes, or when another window stacked above it overlaps the
  highlight ("covered"). The frame round a window follows it too.

**Limits, stated plainly.**

1. KWin sends geometry as whole logical pixels. A window at a fractional
   position (possible at a fractional scale) can be off by up to half a
   logical pixel. The compositor job measures what this comes to
   ([desktop-4.md](../reports/desktop-4.md)).
2. The client area is the window's content when the window has a
   server-side decoration, which KWin gives every Qt program that asks
   (TUR's do). A program that draws its own decoration and a shadow has its
   content inset in its surface by the shadow; the rectangle would be off by
   that much. Not measured.
3. Scrolling inside the window is not seen: a row that scrolls moves away
   from its highlight. TUR's own programs highlight themselves for that
   reason (design 9.5, mechanism 1); `tur-settings` does when asked with
   `highlight`, and in gold with `pax`.
4. Two windows with the same pid, application identifier and title are told
   apart only by stacking.
5. "Covered" is judged by rectangles, not by pixels: a translucent window or
   a shadow above counts as covering.
6. A sandboxed overlay would be refused the protocol; every lookup would fail
   with `WindowNotFound` and a message saying why.
7. The protocol is KDE's own and may change between Plasma releases.
8. The overlay sees every window's title and pid. It uses them only to find
   the window it is asked for.

**The `control` key of `Reveal`.** To point at what a person presses rather
than at the middle of a row, `tur-settings`' `Reveal` result gained one key:
`control` (dddd), the row's control (its button, list or switch) in the same
window coordinates, when the row has one. It is the only change this task
made to `tur-settings` (with its test in `tests/unit/tst_bus.cpp`).

## 5. What is drawn

The overlay's palette is fixed (`Tur.Kit.Pax`, `PaxTheme.overlay*`): it is
drawn over content TUR does not control and must read on any of it. Two
tokens were added to the kit for it: `pax.overlay.quietText` (#EEF1F6) and
`pax.overlay.paused` (#A9B4C6).

**The pill** (design 9.3). 36 px high, at the top centre of every output, 12
px from the top. Every state has its own icon and its own words, so that
colour is never the only difference:

| State | Icon | Words | Look |
|---|---|---|---|
| idle, P@x off (no lease) | — | — | Nothing is drawn |
| idle, P@x on | flame | P@x is on | Dark, gold outline |
| listening | microphone | Listening… | Dark, gold outline |
| working | flame | P@x is working | Dark, gold outline |
| observing | eye | P@x can see this window / this screen | Dark, gold outline |
| acting | flame | P@x is controlling | Solid gold, dark text, dark outline, white hairline |
| waiting | hand | P@x is waiting for you | Dark, gold outline |
| paused | pause sign | P@x is paused | Dark, grey dashed outline |

In every state the words go on: "· Stop: Meta+Esc · Keys: Meta+/" (section
5.1); at 200 % text, or on a narrow screen, the pill grows a line rather than
leave the screen. The brief's "thinking" is the design's *working*; its "stopped" is the
design's *paused* (the stop control pauses, 9.3). Where several leases set a
state, the one that tells the person most is shown: acting, waiting,
observing, working, listening, paused, idle. The repair-session segment and
private mode of the design's table are not drawn (section 10).

### 5.1 P@x's keys, at all times

The owner's answer of 12 November 2026 (desktop-experience.md 17.1, answer 2):
while P@x is switched on, its stop combination is on screen at all times, not
only while it acts, and the full list of its keyboard commands is one action
away from the indicator; the list shown and the bindings come from one
source, so that they cannot disagree.

- **One source**: `desktop/kit/pax/commands.json` (stop `Meta+Esc`, ask
  `Meta+Space`, talk `Hold Meta+Space`, the list `Meta+/`). `gen_commands.py`
  generates `Tur.Kit.Pax`'s `PaxCommands` singleton from it, which the pill
  and the list read; the `boundary` job runs `gen_commands.py --check`, which
  fails if the generated file is stale or if a desktop file of P@x's programs
  binds one of the commands to other keys (with `--self-test` proving that it
  would catch that).
- **"On"** is "a lease is held": `tur-pax` takes one when P@x is switched on.
  With a lease held and nothing happening, the pill reads "P@x is on · Stop:
  Meta+Esc · Keys: Meta+/".
- **One action away**: the pill names the key of the list (`Meta+/`), and
  `ShowCommands(true)` or `tur-pax-overlay --commands` puts the list under the
  pill. The list cannot be clicked open from the pill, which takes no input.
- **What is not true yet, stated plainly**: **nothing binds any of these keys
  yet.** `commands.json` says `"bound": false` for each; the stop key belongs
  to the stop control (a global shortcut or the input witness, design 9.8),
  the others to `tur-pax-ui` (D-17). Whether `Meta+/` and `Meta+Space` are
  free in Plasma 6.8's defaults is not verified (`"verified": false`). The
  indicator shows what the bindings will be; the check makes sure a binding,
  when one is written, says the same.
- Tests: `tst_overlay.qml` reads `commands.json` as it is and checks that
  the pill shows its stop keys and list key in every state, idle included,
  and that the list shows every command with its keys and words; at 200 %
  text the pill and the list stay on the screen with no word cut; contrast
  of the idle pill and of the list is in `tst_overlay_contrast.qml`; under
  KWin, the idle pill and the list are photographed.

**The frame.** Observing: 2 px gold, solid. Acting: 3 px gold, solid.
Waiting: 2 px gold, dashed. Paused: 2 px grey, dashed. Round the window when
the scope is a window that can be found; otherwise at the screen's edge, and
the pill then says "this screen". **Each line has a 1 px dark line beside
it**, outside a window's frame and inside the screen's edge: gold alone is
1.9:1 on white, below the 3:1 a line that carries meaning needs. The design
names only the gold line.

**P@x's pointer** (9.4). P@x's flame, tilted 45 degrees so that its tip is
the hot spot: gold (#E8B44C) with a dark outline (#0B1220, 1.5 px) and a white
hairline outside that, and the logo's inner flame (dark) and core (gold)
inside. The flame is `Tur.Kit.Pax`'s new `PaxFlame`, **a first draft** of the
flat mark the owner allowed (17.1, answer 5), redrawn by hand from the flame
in `branding/tur-logo.png` for the owner to look at; the older `PaxMark` is
left as it was, so the kit's and the settings' pictures do not change until
the owner has decided. The pointer is 1.5 times the cursor size (`XCURSOR_SIZE`, 24 when
unset: 36 px). A tag "P@x" trails it at rest. Its first appearance starts
below the pill of the screen it is going to and fades in as it travels.

*Its journey*: 250 ms plus 0.45 ms a pixel, at least 250 and at most 700 ms;
twice that with `slow`; times the desktop's animation factor. It decelerates
into the target (out-cubic) along a quadratic curve that bends to one side by
a fifth of the distance, at most 120 px. There is one pointer for all outputs;
each surface draws it from the same global position, so it leaves one screen
and enters the next at the same moment. *With reduced motion* it fades out
(100 ms), reappears at the target, fades in (100 ms) and the target gets a
ring.

*A click* (`PulsePointer`, "click") is a gold ring with a dark line outside,
growing from the tip from 12 to 32 px and fading. It is shown, never
performed.

**A highlight** (9.5). A 3 px gold line with a 1 px dark line outside it, 4
px clear of the target, 6 px corner radius: the measures of `tur-settings`'
own highlight, plus the dark line. It fades in (`Motion.fadeAppear`). With
`point`, the pointer travels to it first and the highlight appears when it
arrives. With `dim`, the rest of the window (for a highlight in a window) or
of the screen is dimmed by a fifth. With `duration`, it goes after that many
seconds (`HighlightLost`, "expired").

**The caption** (9.6). Beside the highlight, never over it: to its right if
there is room on that screen, else to its left, else below, else above. At
most 320 px wide. The step ("Step 2 of 3") in grey above one sentence. Dark
with a gold border. On the screen of the highlight only.

**Contrast.** `tests/tst_overlay_contrast.qml` checks every mark over every
surface colour of both of TUR's variants and over white, with WCAG's
thresholds: words 4.5:1 against what is behind them; a line must have one
stroke that stands 3:1 from both its neighbours; a filled shape must have one
layer that stands 3:1 from the content round it. The colours are read from
the components. Measured values are in the report.

## 6. Who may draw

**In this version any program on the session bus can take a lease, and so
make gold appear.** Gold means "P@x" by convention (design, 5.1), and this
slice does not enforce the convention: `tur-pax`'s `org.tur.Agent1` does not
exist yet, and nothing here depends on it. The rest is enforced:

- every call but `Acquire` and `Hide` names a lease and is answered only for
  the bus connection that took it;
- a lease ends, and everything drawn under it is removed at once, when its
  connection leaves the bus (crash included), on `Release`, and on `Hide`;
- when the last lease ends nothing of the overlay is left on screen;
- limits on leases, highlights and text keep one caller from filling the
  screen.

**Switching off.** `Hide()` needs no lease and is never refused; neither is
`tur-pax-overlay --hide`, and killing the process removes every surface with
it. The design's 9.3 says that "the overlay cannot be told to hide while a
lease is held", as part of the lease rule ("no indicator, no access"). That is
kept in its effect and not in its letter: hiding ends every lease (holders
get `LeaseEnded` with "hidden", and `Shown` turns false), so a screen
component that follows the lease rule loses its access the moment the
indicator goes. The person's wish to switch P@x's presence off wins.

**Proposed owner check, for when `org.tur.Agent1` exists.** `Acquire` is
honoured only for the connection that owns `org.tur.Agent1` (and, under the
design's lease rule, the screen component's name), checked against the bus
daemon (`GetNameOwner`, and the connection's process through
`GetConnectionCredentials`, whose executable must be the installed
`tur-pax`); every other caller gets `NoLease`. Gold in `tur-settings` (its
`pax` option, issue #51) would be honoured on the same condition, by asking
the overlay or the bus the same question. What this does and does not give:
it stops a program from making gold appear by mistake or by casual misuse.
It does not stop software running as the user, which can impersonate the
name after killing `tur-pax`, draw its own layer-shell surface in gold, or
inject input (AI control layer, 19). The overlay is not, and must not be
presented as, a security boundary.

## 7. What the overlay is not, and departures from the design

- **Not a security boundary.** It is a client surface. KWin 6.8 lets any
  unsandboxed program in the session draw over everything, capture the
  screen and inject input (AI control layer, 19). The overlay can be killed,
  imitated and drawn over. The compositor-drawn indicator of the
  specification needs KWin work (9.13; section 11 below).
- **It never takes input away from the person.** Every pixel lets input
  through: the surfaces are `Qt::WindowTransparentForInput`, for which Qt
  6.11 sends KWin an empty input region (`qwaylandwindow.cpp`,
  `updateInputRegion`), and their keyboard interactivity is "none". Nothing
  on them is a control. Tested under KWin with clicks on P@x's pointer and
  highlight, and keys typed while it is shown.
- **The pill has no buttons.** The design's pill has Stop, Pause and Resume,
  and its caption has Back, Repeat, Do it for me and Stop. A button needs a
  surface that takes clicks, and the brief requires that every pixel of the
  overlay lets them through; a button that cannot be pressed must not be
  shown. So the words are drawn and the buttons are not; the signals for
  them are declared and never sent. Where they should live is a decision for
  the owner (section 11): a small input-taking surface for the pill only, as
  the design has it, or the conversation panel's pill (`tur-pax-ui`, D-17).
  The pill's words name the stop shortcut and the key of the list in every
  state (section 5.1); **no program handles those keys yet**.
- **The dark line beside every gold line** (sections 5): an addition, for
  contrast on light content.
- **A highlight follows its window** when the window moves; the design
  removed it. Moving a window does not move what is inside it.
- **A frame for a window that cannot be found** goes to the screen's edge and
  the pill says "this screen": saying more than is true is the safe error for
  an indicator of what P@x can see.

## 8. Libraries and licences

| Library | Licence, as read | Used for | Linked by |
|---|---|---|---|
| Qt 6.11: Core, Gui, Qml, Quick, Quick Controls, Shapes, DBus | LGPL-3.0-only | Everything | `libturpaxoverlay`, the program |
| LayerShellQt 6.8 | Every file of `src/`: `LGPL-2.1-only OR LGPL-3.0-only OR LicenseRef-KDE-Accepted-LGPL`; `wlr-layer-shell-unstable-v1.xml`: MIT; build files: BSD-3-Clause (tag v6.8.2) | The layer-shell surfaces | The program |
| KWayland 6.8, client library | All 75 files of `src/client`: `LGPL-2.1-only OR LGPL-3.0-only OR LicenseRef-KDE-Accepted-LGPL` (tag v6.8.2). The protocols it is generated from, `plasma-window-management.xml` and `fake-input.xml` of plasma-wayland-protocols 1.23.0: LGPL-2.1-or-later | Window positions; in the compositor tests also injected input | The program; `tst_compositor` |
| `tur-kit` (`Tur.Kit`, `Tur.Kit.Pax`) | The project's | Tokens, labels, motion | Both |

No GPL-only Qt module is used; Qt Wayland Compositor is not (the overlay is a
client). No KWin script, effect or plugin and no Plasma applet. KWayland is
exported by CMake as `Plasma::KWaylandClient`, which the boundary check's
line against libplasma (`Plasma*`) caught; that line now names libplasma's
targets, and KWayland is allowed by name to this program and its compositor
tests only ([ADR-0020](../adr/0020-overlay-window-positions.md)).

## 9. Tests

| Test | Where | What it proves |
|---|---|---|
| `pax-overlay-starts` | ctest, offscreen | The program loads its surfaces with no QML warning |
| `pax-overlay-unit-logic` | ctest | Leases answered only for their holder; arguments and limits checked; screen coordinates moved to their screen; a rectangle in a window lands at the client area plus the rectangle; a highlight follows its window and is dropped when resized, minimised, covered or closed; the loudest state shown; the frame round the window or "this screen"; ending a lease or `Hide` removes everything; a highlight that waits for the pointer; the pointer's journey takes 250 to 700 ms (twice when slow) and jumps with reduced motion |
| `pax-overlay-qml` (`tst_overlay.qml`) | ctest, offscreen | Each state's words and icon are its own; the frame by state; the highlight's measures; the caption beside the highlight and never over it, on its screen only; dimming leaves the highlight clear; one pointer across two screens; nothing on a surface is a control or takes the keyboard |
| `pax-overlay-qml` (`tst_overlay_contrast.qml`) | ctest, offscreen | Section 5, contrast |
| `pax-overlay-bus` | ctest, private bus, offscreen | What the running program offers is the XML; leases and their errors; `Shown` and `Screens`; signals go to the holder alone; it vanishes when its owner leaves the bus, on `Hide` and on `--hide`; `--hide` with nothing running |
| `settings-bus` (extended) | ctest | `Reveal`'s `control` is where the control is drawn, inside the row |
| Reference pictures | `screenshots` job | Every state's pill and frame, a guided step and a click, over a stand-in window in both variants, at 1x and 1.5x |
| `tst_compositor` | `compositor` job, under KWin | Section 9.1 |

### 9.1 Under KWin

The `compositor` job (`desktop/ci/compositor.sh`) runs `kwin_wayland` from
the builder image (Fedora 45's KWin, the one the TUR image ships), headless:
the virtual backend, Mesa's software renderer through `/dev/udmabuf`, a
private session bus. `tst_compositor` is a Wayland client of it; it starts
`tur-settings` and `tur-pax-overlay` as Wayland clients and runs, for five
arrangements (dark and light at scale 1 and 1.5, and two outputs at scale 1):

1. KWin's view of `tur-settings`' window: application identifier and title.
2. The running overlay's introspection against the XML.
3. A surface on every output: the acting frame at every edge and the pill at
   the top centre of each, in KWin's screenshots; `Screens` lists every
   output.
4. The chain: `Search("night light")` → `Reveal` (without the program's
   highlight) → `HighlightInWindow` with `point` → P@x's pointer travels to
   the row → the highlight appears round it. The pointer's position and the
   highlight's four edges are measured in KWin's screenshots against the row
   `Reveal` reported plus the client area KWin reports, to within 1.5 logical
   pixels. Then `PointInWindow` to the row's `control`, and a click injected
   on the pixels of P@x's pointer that lie over the button: it presses the
   button (tur-settings starts its stand-in for System Settings).
5. A window of the test's own has the keyboard; the overlay shows itself over
   it, frames it, highlights in it and points at it; keys typed reach the
   window, which never loses the keyboard; a click on P@x's pointer reaches
   the window at that point.
6. With two outputs: the pointer crossing to the second, and a highlight
   there.
7. It vanishes: on `Hide` (no gold left in any screenshot), when its owner's
   process is killed, and when the overlay's own process is killed.

Three facts of KWin 6.8 the job had to meet (report desktop-4): the virtual
backend renders with OpenGL only, and without a GPU through Mesa's software
device and `/dev/udmabuf`; `--scale` does not set an output's scale (KWin's
output configuration does, so the job writes it); and Fedora's
`kwin_wayland` carries a file capability that a container's unprivileged
user cannot execute, so a copy without it is run.

Input is injected with KWin's fake-input protocol, which KWin 6.8 offers to
any unsandboxed client and whose authentication request is a no-op ("TODO:
make secure", `src/backends/fakeinput/fakeinputbackend.cpp`). Screenshots come
from `org.kde.KWin.ScreenShot2`, which has no permission check in 6.8.2. Both
facts are the AI control layer's section 19 seen from the other side.

## 10. Not built

- (Built since, TUR-011: install rules, the D-Bus activation file
  `data/org.tur.PaxOverlay1.service` and the image. The session bus starts
  the overlay when something calls `org.tur.PaxOverlay1`.)
- The lease rule's other half: the screen component that holds a lease and
  stops when it ends (milestone A2).
- The owner check (section 6).
- The pill's buttons and the guide controls (section 7).
- The repair-session segment of the pill; the chip on the screens P@x is not
  acting on ("P@x is working on the left screen ←"); the pointer parked
  beside the pill while a protected surface is shown; pausing when the target
  screen is disconnected. Each needs a caller that knows these things
  (`tur-pax`, the approval sheet).
- High contrast: the design's 4 px line in the scheme's highlight colour.
- The overlay and the lock screen: KWin renders only lock-screen windows
  while locked, so the overlay disappears; not tested (`--no-lockscreen`).
- Above full-screen windows: the overlay layer is above them in KWin; not
  tested.

## 11. For the owner, and upstream

Decisions the owner may want to reverse:

0. "On" means "a lease is held", and the list's key is `Meta+/` (both
   section 5.1); the pill of every state carries the stop keys and the list's
   key.

1. `Hide` is never refused, even while a lease is held (section 6).
2. No buttons on the pill (section 7).
3. The dark line beside gold lines (section 5).
4. Any session program can take a lease in this slice (section 6).
5. Highlights follow a moved window.

Text proposed for the design's section 9.13 and for an upstream proposal to
KWin (the design's D-24) is in the report.

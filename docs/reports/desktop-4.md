# desktop-4 — tur-pax-overlay: P@x on the screen, and the first tests under KWin (D-15)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-4 (task D-15 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop (desktop/apps/tur-pax-overlay, desktop/tests/
                  compositor, desktop/kit/pax), desktop CI
Build ID:         not applicable: this task builds no image
Source revision:  5e167bd (feat/pax-overlay, pull request 54), the commit
                  the results below come from. The commit that adds the
                  final figures to this report follows it and changes no
                  code.

SUMMARY
- What changed: P@x has a presence on the screen. tur-pax-overlay is a
  standalone Qt Quick program that, under KWin, puts a layer-shell surface
  on the overlay layer of every output and draws the activity indicator
  (the pill in the designed states and the frame), P@x's own pointer, which
  travels, highlights with dimming and a caption, and the list of P@x's
  keys. Every pixel lets input through and it never takes the keyboard. It
  is driven over org.tur.PaxOverlay1 with leases, and vanishes when its
  owner leaves the bus, when told to, and when it exits. A rectangle that
  tur-settings reports inside its window is placed on the screen with the
  window's position from KWin's Plasma window-management protocol.
- The owner's answers of 12 November 2026 (desktop-experience.md 17.1),
  which arrived during the task: while P@x is on, the pill shows the stop
  combination at all times, idle included, and the key of the full list,
  which is drawn under the pill when asked for; the keys come from one
  source (desktop/kit/pax/commands.json), which the bindings are checked
  against. P@x's pointer is drawn with a first draft of a flat flame from
  the logo.
- Why: the owner's requirement that P@x "point the exact lines with its
  pointer", and the AI control layer's rule that the person can always see
  when P@x observes or acts.
- The first CI job that runs TUR's desktop programs under a real KWin
  (Fedora 45's kwin 6.8.2, headless): the chain "search a setting, reveal
  it, P@x's pointer travels to the row, the highlight appears round it, a
  click on P@x's pointer presses the button under it" passes at scale 1 and
  1.5, in both variants and with two outputs, measured in KWin's
  screenshots.
- What it does not do: it holds no authority and is not a security
  boundary; any session program can take a lease in this slice; nothing
  binds P@x's keys yet; the pill has no buttons.
```

## Runs

The desktop workflow, pull request 54. Every run built the builder image
from its cache key (the first rebuilt it with `kwayland-devel`).

| Run | Commit | Result | What it showed |
|---|---|---|---|
| 43251305162 | 2970206 | fails | **Everything compiled at the first attempt** with `-Werror`, and `qmllint` was clean. 32 of 33 test programs passed. One logic test failed for a real fault: when the pointer had already arrived (frozen motion), ending its lease did not take the pointer away, because "the lease whose journey is under way" had already been cleared. 40 new pictures had no reference; the first were looked at: the dark pictures had a white desktop under the stand-in window, and the dimming left the corners outside the highlight's round corners undimmed. The compositor job could not start its script (not executable) |
| 43253330483 | ca68f0f | fails | The pointer belongs to its lease until hidden. The compositor job: `kwin_wayland: Operation not permitted` — Fedora's binary carries a file capability (CAP_SYS_NICE) and an unprivileged user in a container cannot run it. A copy, which carries none, is run instead |
| 43255107059 | 09f36de | fails | The owner's answers built in. **KWin ran headless on the runner** (virtual backend, Mesa's software renderer through `/dev/udmabuf`); tur-settings and the overlay ran as its clients; KWin sees `tur-settings` as `org.tur.Settings`; the introspection under KWin matched the XML. Every screenshot came back empty: the test read KWin's `a{sv}` answer as a map, and an asynchronous call delivers it as a `QDBusArgument` |
| 43256783580 | 2ef077e | fails | Screenshots arrived. At scale 1: **the highlight measured 0 px from where Reveal plus KWin said the row is, on all four sides**; the click on P@x's pointer pressed the button (`kcm_nightlight`); keys typed while the overlay was shown reached the probe window, which never lost the keyboard. After Hide, 159 gold pixels remained: KWin's decoration draws Wayland's generic window icon, which is gold. The "1.5x" arrangement turned out to be 1920 x 1200 at scale 1: `kwin_wayland --scale` does not decide an output's scale in 6.8 (below). Two QML tests were wrong about the new pill words |
| 43258475145 | 04dd098 | fails | A real fractional scale through KWin's output configuration. At 1.5: the highlight 0, 0, +0.33, +0.33 px from its expected edges; click and keyboard tests pass. One check photographed the previous frame (the overlay at 1.5 in software takes more than 300 ms to redraw a full output), so the picture checks now wait for the frame they look for. Two outputs: pass. Build, lint and tests: pass. The 40 pictures were accepted after looking at them |
| 43260161348 | 86678f8 | fails | The 40 references matched; build, lint and tests passed. Under KWin, two of five arrangements failed once in the same place: the screenshot was taken when the pointer had arrived and the highlight was still fading in (200 ms), so its line was not found. The check now waits for the line as well as the pointer. At exit KWayland's fake-input object sends a request its bound version lacks and KWin answers with a protocol error; the test now leaves the object alone |
| — | 9f5c9a5 | no run | Main had moved (ADR-0021's index line conflicted with ADR-0020's), so GitHub made no merge commit and ran nothing. Merged |
| 43266086183 | 5e167bd | **pass** | Every workflow on the head commit green: desktop (boundary, builder, build-test, screenshots, compositor), rust, gate. Build-test: 821 test functions passed, 1 skipped (the kit's, as before), 34 tests, no compiler or qmllint warning, linked libraries within the boundary. Screenshots: 203 of 203 match (40 new). Under KWin: 11 of 11 functions in each one-output arrangement (the second-output test skipped there), 12 of 12 with two outputs |

## What was built, and what proves it

| Built | Proven by |
|---|---|
| **The program**: one layer-shell surface per output, on the overlay layer, anchored to every edge, empty input region, no keyboard interactivity; mapped only while there is something to draw | Under KWin: a surface on every output (`Screens` lists both outputs; the frame and pill photographed on each); the overlay is not among KWin's windows; clicks and keys pass through (below). Offscreen: `pax-overlay-starts` |
| **org.tur.PaxOverlay1** as the XML states it (13 methods, 7 signals, 3 properties) | `pax-overlay-bus` (offscreen) and `tst_compositor` (under KWin) compare the running program's introspection with the XML |
| **Leases**: answered only for their holder; signals to the holder alone; everything drawn under a lease goes with it | `pax-overlay-unit-logic`; `pax-overlay-bus` (another connection's calls refused; `LeaseEnded` reaches the holder and not a second connection that listens for it) |
| **Vanishes**: when told (Hide, `--hide`), when its owner leaves the bus or dies, when it exits | `pax-overlay-bus` (Shown false after each); under KWin, in screenshots: after Hide, after SIGKILL of the owner's process, after SIGKILL of the overlay, no gold of the overlay's is left |
| **The indicator**: pill words and icon per state, the frame by state, the loudest state shown | `tst_overlay.qml`, `pax-overlay-unit-logic`; reference pictures of every state in both variants at 1x and 1.5x |
| **P@x's keys at all times while on**; the list one action away; one source | `gen_commands.py --check` and `--self-test` in the boundary job; `tst_overlay.qml` reads `commands.json` and checks the pill in all seven states and every row of the list; 200 % text; under KWin, the idle pill and the list photographed |
| **The pointer**: travels in 250 to 700 ms, twice when slow; jumps and rings with reduced motion; one pointer across outputs | `pax-overlay-unit-logic` (measured journeys); `tst_overlay.qml` (two surfaces); under KWin, the pointer photographed crossing to the second output and at rest there |
| **Highlights** at the designed measures, dimming, caption beside and never over, waiting for the pointer | `tst_overlay.qml`; `pax-overlay-unit-logic`; under KWin, measured |
| **A rectangle in a window lands on the window's content** | `pax-overlay-unit-logic` (with a stand-in locator); under KWin: **0 px** at scale 1 (three arrangements), **≤ 0.33 px** at 1.5 (two), on all four edges, measured by scanning KWin's screenshots towards the row from outside it |
| **Follows a moved window; dropped when resized, minimised, covered or closed** | `pax-overlay-unit-logic` with a stand-in locator. **Not** exercised under KWin (no window was moved or resized there) |
| **Click-through** | Under KWin, five arrangements: a click injected on the pixels of P@x's pointer, inside a highlight, over the "Open in System Settings" button of the Night Light row, pressed that button (tur-settings started its stand-in for System Settings with `kcm_nightlight`); a click on P@x's pointer over the probe window reached it at the exact point (196,76 for 196,76) |
| **Never takes the keyboard** | Under KWin: with the overlay acting, framing, dimming, highlighting and pointing over the probe window, three keys typed reached it ("tur") and it never lost the keyboard (no focus-out, KWin still reports it active) |
| **Contrast**: every mark over every surface colour of both variants and over white | `tst_overlay_contrast.qml`: lowest line or shape 7.44:1 (paused frame and pill over dark surface.overlay); pill words 9.77:1 or more; gold alone on white 1.9:1 is shown to fail, which is why every gold line has a dark one |
| **`Reveal`'s `control`** in tur-settings | `settings-bus`: where the control is drawn, inside the row; absent for a row without a control |

Input was injected with KWin's fake-input protocol (`org_kde_kwin_fake_input`,
through KWayland): KWin 6.8 offers it to any unsandboxed client and its
authentication is a no-op. Screenshots came from `org.kde.KWin.ScreenShot2`
(`CaptureScreen`, native resolution), which has no permission check in 6.8.2.

## Measurements

| Arrangement | Output (logical) | Picture | Highlight edges vs expected (px) | Click on the pointer reached | Gold before / after Hide (window icon) |
|---|---|---|---|---|---|
| dark, 1x | 1280 x 800 | 1280 x 800 | 0, 0, 0, 0 | button; probe at 196,76 | 25 274 / 159 (159) |
| light, 1x | 1280 x 800 | 1280 x 800 | 0, 0, 0, 0 | button; probe at 196,76 | 25 274 / 159 (159) |
| dark, 1.5x | 1280 x 800 | 1920 x 1200 | 0, 0, 0.33, 0.33 | button; probe at 196,76 | 55 668 / 367 (367) |
| light, 1.5x | 1280 x 800 | 1920 x 1200 | 0, 0, 0.33, 0.33 | button; probe at 196,76 | 55 673 / 367 (367) |
| two outputs, 1x | 2 x 1280 x 800 | 1280 x 800 each | 0, 0, 0, 0 | button; probe at 196,76 | 47 667 / 159 (159) |

(Run 43266086183, artifact `desktop-compositor`, `observations.txt`; the
same figures as run 43258475145. The pill's share of gold pixels: 85.8 % at
1x, 86.3 % at 1.5x. The button under P@x's pointer was pressed in all five
arrangements.) The 0.33 px at 1.5 is one device
pixel at the right and bottom edges: the measurement is in device pixels and
a logical edge at x.0 falls inside a device pixel at 1.5.

## Observed under KWin (issue #51)

- **The application identifier KWin sees** for tur-settings is
  `org.tur.Settings`, resource name `tur-settings`, title "Settings". KWin
  gives it a server-side decoration; the client area is the window's content,
  so `Reveal`'s rectangle plus the client area's corner is exact.
- **Window activation on `Reveal`**: with another window holding the
  keyboard, `Reveal` did **not** activate tur-settings; KWin marked it as
  demanding attention instead. Expected: tur-settings asks to be raised
  without an activation token, and KWin's focus-stealing prevention refuses.
  A caller that wants the window in front must pass a token (xdg-activation);
  the interface has no way to today.
- tur-settings' window has no icon of its own: KWin's decoration shows
  Wayland's generic icon, a gold "W". It is gold enough to count as P@x's
  colour in a screenshot (159 pixels at 1x).
- Qt reports the 1.5 output as `devicePixelRatio` 2 on its `QScreen` and
  draws windows at 1.5 through the fractional-scale protocol.
- KWin logs "error in client communication" once per run, when the test's
  owner process is killed.

## Facts found about KWin 6.8 (all in its source at v6.8.2)

- `kwin_wayland --virtual` renders with OpenGL only. Without a GPU, KWin uses
  Mesa's software EGL device with buffers shared through `/dev/udmabuf`
  (`src/core/gpumanager.cpp`, `RenderDevice::createSoftwareDevice`). The
  GitHub runner's kernel has `/dev/udmabuf`; the job gives it to the
  container and nothing else.
- `--scale` does not set an output's scale: the output configuration store
  (`src/outputconfigurationstore.cpp`) chooses a scale from the physical size
  where its configuration has none, which for a virtual output is 1. The job
  starts KWin once to write `kwinoutputconfig.json`, sets the scale in it, and
  starts KWin again.
- Fedora's `kwin_wayland` has a file capability (CAP_SYS_NICE); in a
  container an unprivileged user cannot execute it. A copy without the
  capability runs.
- The restricted Wayland interfaces (window management, fake input,
  screencast, …) are refused only to sandboxed clients
  (`src/wayland_server.cpp`); fake input's `authenticate` is a no-op;
  ScreenShot2 has no permission check. Any unsandboxed program in the session
  can see every window, inject input and take screenshots — the AI control
  layer's section 19, confirmed.

## Not verified

- **Nothing of this is in the image** (no install rules for any desktop
  program, issue #51); not run in a Plasma session, on real hardware, with a
  GPU, with a real cursor theme, with a person.
- **No key is bound.** The pill shows `Meta+Esc` and `Meta+/`; nothing
  handles them. Whether `Meta+/` and `Meta+Space` are free in Plasma 6.8's
  defaults is not checked.
- Following a moved window, and dropping a highlight on resize, minimise,
  cover and close, under KWin: tested with a stand-in locator only.
- The overlay above a full-screen window, while the session is locked
  (the job runs `--no-lockscreen`), with a screen added or removed while it
  runs, at scale 2, with three or more outputs, with high contrast.
- A program with client-side decoration (its content inset by its shadow).
- The clients ran Qt Quick's software renderer (`QT_QUICK_BACKEND=software`);
  KWin ran Mesa's software OpenGL. The OpenGL path of the clients was not run.
- The pointer's journey on screen was photographed once in flight (fading in,
  part way); its timing was measured offscreen only.
- Reduced motion under KWin.
- The flat flame is a first draft for the owner; nobody has approved it.

## Decisions the owner may want to reverse

1. `Hide()` is never refused, even while a lease is held: switching P@x's
   presence off ends every lease (design 9.3 said the overlay "cannot be told
   to hide while a lease is held").
2. No buttons on the pill or the caption: every pixel of the overlay lets
   input through, as the brief requires.
3. A 1 px dark line beside every gold line (contrast on white).
4. Any session program can take a lease in this slice; the proposed owner
   check is in tur-pax-overlay.md, section 6.
5. A highlight follows a moved window (the design removed it).
6. "On" means "a lease is held"; the list of keys is on `Meta+/`; the pill of
   every state carries the stop keys and the list key.
7. The window locator uses KDE's Plasma window-management protocol through
   KWayland (ADR-0020), which needed the boundary check's line against
   libplasma narrowed to libplasma's names.

## Proposed text for other documents

For **desktop-experience.md**:

- 9.3, "Implementation": the frame, pointer, highlight *and pill* surfaces
  have an empty input region; the pill's buttons are not on the overlay
  (owner decision 2).
- 9.3, measure 1: "Hiding is never refused; it ends every lease, and `Shown`
  turns false, so the screen component loses its access when the indicator
  goes."
- 9.3 and 9.5: "Every gold line has a 1 px dark line (#0B1220) beside it."
- 9.3, the table: "Idle (P@x on) — Flame · 'P@x is on' — no frame"; and every
  pill ends "· Stop: Meta+Esc · Keys: Meta+/" (17.1.2).
- 9.5, mechanism 2: the client-area geometry comes from the Plasma
  window-management protocol (version 21; `client_geometry` since 18), read
  with KWayland; `getWindowInfo` gives only the frame and needs an
  identifier the protocol supplies (ADR-0020). A highlight follows a moved
  window and is removed when the window is resized, minimised, covered or
  closed.
- 9.13, "Highlight in other applications": "Yes, approximate: client-area
  geometry in whole logical pixels; scrolling inside the window is not seen |
  A supported way to learn where a rectangle of another window is, or to ask
  the compositor to mark it". And a fifth proposal (below).
- 14.1, `tur-pax-overlay`: "KWayland's client library (LGPL-2.1-only OR
  LGPL-3.0-only, all files of src/client at v6.8.2), exported as
  `Plasma::KWaylandClient`; ADR-0020". The interface as in tur-pax-overlay.md,
  section 3.
- 15.3, `compositor`: "KWin 6.8's virtual backend renders with OpenGL only;
  without a GPU it needs Mesa's software device and `/dev/udmabuf`; an
  output's scale comes from `kwinoutputconfig.json`, not `--scale`; Fedora's
  `kwin_wayland` must be run as a copy in a container (file capability)."

For **ai-control-layer.md**, 16.6, first paragraph: "drawn by the compositor"
becomes "drawn by tur-pax-overlay, a client surface over every screen
(section 19, item 3)"; and a line: "While P@x is on, its stop combination is
on screen at all times (section 23)."

For **issue #51**: the application identifier is verified
(`org.tur.Settings`); activation on `Reveal` does not happen without a token
(observed); the `pax` option's sender check is proposed in
tur-pax-overlay.md, section 6.

## Proposed for upstream (KWin; for D-24)

> **A supported way to mark a region of another client's window.**
> Assistive and teaching tools need to draw attention to a control inside
> another application's window. On KWin 6.8 a separate client can do it only
> by reading the window's client geometry from the Plasma window-management
> protocol (refused to sandboxed clients, and KDE's own), adding the
> control's window-relative rectangle (from the application or the
> accessibility tree), and drawing in a full-screen overlay layer-shell
> surface. This works — we measured it to within a device pixel at scale 1
> and 1.5 — but the geometry is in whole logical pixels, scrolling and reflow
> inside the window are invisible, the tool must follow geometry events, and
> a sandboxed tool cannot do it at all. Proposal: a small protocol, granted
> like the screencast portal, in which a client asks the compositor to mark a
> rectangle given relative to a toplevel it identifies (by the
> foreign-toplevel or window-management identifier), with a style hint; the
> compositor draws the mark, moves it with the window, and tells the client
> when the window moves, resizes, or covers it.

## Follow-ups worth an issue

1. Bind P@x's keys (D-17 and the stop control), from `commands.json`; verify
   `Meta+/` and `Meta+Space` are free in Plasma 6.8.
2. The owner check on `Acquire` once `org.tur.Agent1` exists; the same answer
   for tur-settings' `pax` option.
3. Where the pill's buttons live (owner decision 2).
4. An activation token through `Reveal` (`activation_token` option), so that
   P@x can bring the settings window forward.
5. A window icon for tur-settings (KWin shows Wayland's generic gold "W").
6. Compositor tests to add: a window moved and resized under the overlay; a
   full-screen window; the lock screen; a screen hot-plugged; scale 2; the
   clients' OpenGL path.
7. Install rules and a D-Bus service file for the overlay (with #51's).
8. The flat flame: the owner's verdict on the first draft, then PaxMark.

## Anything found wrong

- tur-settings has no window icon of its own (above).
- design 9.3's lease rule and the brief's "switched off instantly" disagree;
  resolved as decision 1.
- design 15.3 assumed `kwin_wayland --virtual --scale` gives a scaled output;
  it does not in 6.8.
- The kit's text-scale and contrast walks cannot see Shapes (the harness
  judges Rectangle borders); the overlay's contrast test therefore judges the
  colours each component declares, not the painted pixels.

## Files

- Added: `desktop/apps/tur-pax-overlay/` (CMakeLists.txt; `dbus/`; `src/`:
  bustypes, windowlocator, overlaystate, overlayservice, busadaptor,
  kwinlocator, surfaces, main; `qml/`: 11 files; `tests/`: tst_main.cpp,
  tst_overlay.qml, tst_overlay_contrast.qml, shots/render_overlay.qml,
  unit/tst_logic.cpp, unit/tst_bus.cpp); `desktop/tests/compositor/`
  (tst_compositor.cpp, probewindow.*, CMakeLists.txt); `desktop/ci/
  compositor.sh`; `desktop/kit/pax/` (commands.json, gen_commands.py,
  PaxCommands.qml, PaxFlame.qml); `desktop/tests/screenshots/overlay-*.png`
  (40); `docs/architecture/tur-pax-overlay.md`; `docs/adr/0020-overlay-
  window-positions.md`; this report.
- Modified: `desktop/CMakeLists.txt` (one add_subdirectory);
  `desktop/ALLOWED_IMPORTS` (the overlay's module and its two libraries; the
  libplasma forbid narrowed, ADR-0020); `desktop/ci/boundary.py` (three
  self-test cases); `desktop/ci/builder/Containerfile` (`kwayland-devel`);
  `desktop/ci/in-builder.sh` (one device may be passed; shared memory 1 GB);
  `desktop/ci/screenshots.sh` (the overlay's pictures);
  `desktop/kit/tokens/tokens.json` and `desktop/kit/pax/PaxTheme.qml` (two
  overlay colours, four contrast pairs); `desktop/kit/pax/CMakeLists.txt`;
  `.github/workflows/desktop.yml` (the `compositor` job; the keys check);
  `docs/adr/README.md` (index line).
- In tur-settings, the one change the positioning needed: `Reveal` returns
  `control` (the row's control) — `qml/Shell.qml`, `src/settingshost.cpp`,
  `dbus/org.tur.Addressable1.xml`, with its test in `tests/unit/tst_bus.cpp`.

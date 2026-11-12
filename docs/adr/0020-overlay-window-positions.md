# ADR-0020: P@x's overlay finds other programs' windows through the Plasma window-management protocol, read with KWayland

- **Status:** Proposed
- **Date:** 2026-11-12
- **Specification sections:** §9 (P@x on the desktop), §16.6 of the AI
  control layer; [ADR-0003](0003-licence.md),
  [ADR-0016](0016-desktop-and-user-interface.md). Design:
  [desktop experience](../architecture/desktop-experience.md), 9.3 to 9.5,
  9.13, 14.1; as built: [tur-pax-overlay](../architecture/tur-pax-overlay.md)

## Context

P@x points at a row of a settings page with its own pointer and draws a
highlight round it. `tur-settings` reports where the row is, but only
relative to its own window (`org.tur.Settings1.Reveal`: `rect` in the
window's logical pixels, with `pid`, `app_id` and `window_title`). Under
Wayland no client knows where any window is on the screen, its own included;
only the compositor does. The overlay, a separate program that draws over
every screen, therefore has to ask KWin.

What KWin 6.8 offers an ordinary client, read in its source at the tag
v6.8.2 (the Plasma release Fedora 45 ships) on 12 November 2026:

| Mechanism | What it gives | Who may use it |
|---|---|---|
| Plasma window-management protocol, `org_kde_plasma_window_management` version 21 (`src/wayland/plasmawindowmanagement.cpp`) | Every managed window with an identifier, pid, application identifier, title, **frame geometry and client-area geometry in global logical pixels** (`client_geometry` since version 18), minimised and active state, and the stacking order, with change events | Any client that is not sandboxed: it is in `KWinDisplay::restrictedInterfaces`, which is enforced only when `client->isSandboxed()` (`src/wayland_server.cpp`) |
| D-Bus `org.kde.KWin.getWindowInfo(uuid)` (`src/dbusinterface.cpp`) | Frame geometry (x, y, width, height), pid, resource class and name, caption and more, for a window **named by its KWin identifier** | Any program on the session bus. There is no call that lists windows: the identifier has to come from somewhere else (the protocol above, or `queryWindowInfo`, which makes the person click a window) |
| A KWin script or effect | Anything | Code inside KWin, under KWin's licence (GPL-2.0-or-later): excluded by the licence boundary |
| The target program says where it is | Nothing: it does not know | — |

## Options considered

1. **The target draws, the overlay does not point.** TUR's own programs
   already highlight themselves (design, 9.5, mechanism 1; `tur-settings`
   draws a gold line when `pax` is true). Least privilege: the overlay needs
   to know nothing. But P@x's pointer cannot travel to the row, which is what
   the owner asked for, and programs that are not TUR's cannot be marked at
   all.
2. **`getWindowInfo` over D-Bus.** No new library. But it gives the frame,
   not the content, so a decoration of unknown size lies between the two; it
   needs the window's identifier, which only the protocol (or the person, by
   clicking) can provide; and it gives no stacking order and no change
   events, so the overlay would have to poll and could not tell that the row
   was covered.
3. **The Plasma window-management protocol, read with KWayland's client
   library.** Gives the content's geometry exactly where `Reveal`'s
   rectangle is measured from, change events, and the stacking order. KWin
   offers it to any unsandboxed client. KWayland (Plasma, LGPL-2.1-only OR
   LGPL-3.0-only, all 75 files of `src/client` checked) is linked
   dynamically. The protocol is KDE's own, not a standard; KDE treats it as
   part of Plasma. The overlay sees every window's title and pid.
4. **Generate the protocol's client code from its XML with Qt's Wayland
   scanner.** Same protocol, no KWayland; but the generated code, made from
   an LGPL-2.1-or-later file, would be compiled into a program under the
   project's licence, where linking a shared library keeps the two apart.
5. **A KWin script that reports positions.** Code inside a GPL host; and the
   design forbids it (section 4).

## Decision

The overlay finds a window by pid, application identifier and title in the
Plasma window-management protocol, read with KWayland's client library, and
places a rectangle "in the window" at the window's client-area geometry plus
the rectangle. The use is confined to one file of one program
(`desktop/apps/tur-pax-overlay/src/kwinlocator.cpp`), behind an interface
(`WindowLocator`) that a supported compositor interface can replace.
TUR's own programs keep highlighting themselves where that is enough.

To allow it, `desktop/ALLOWED_IMPORTS` allows `KWayland`,
`Plasma::KWaylandClient` and `libKWaylandClient.so.6` to
`desktop/apps/tur-pax-overlay/` and to the compositor tests, and the line
`forbid cmake Plasma*`, which was written against libplasma and also caught
KWayland's `Plasma::` namespace, becomes forbids of libplasma's, Plasma
Quick's, Plasma5Support's and Plasma Activities' names. Nothing else of
Plasma's becomes allowed: whatever is not allowed by name is refused.

## Consequences

- P@x's pointer can travel to a row and the highlight can be drawn round it,
  for TUR's programs and for any program whose window can be told apart by
  pid, application identifier and title.
- The overlay can see the title, pid and geometry of every window in the
  session. It does nothing with them but find the one it is asked for, logs
  only what it finds, and sends nothing anywhere. This is a property of KWin
  6.8 for every unsandboxed program; the overlay adds no power that other
  software in the session lacks.
- If the overlay were ever sandboxed (Flatpak), KWin would refuse it the
  protocol and every window lookup would fail with `WindowNotFound`.
- Limits, stated where they matter (tur-pax-overlay.md, section 4): KWin
  sends integer logical coordinates, so a window placed at a fractional
  position is off by up to half a logical pixel; the content of a window
  that draws its own decoration and shadow (client-side decoration) is not
  where the client area says, by the size of the shadow; a rectangle stays
  right only while the window does not change size (a resize drops the
  highlight); two windows with the same pid, application identifier and
  title are told apart only by stacking (the higher one is taken).
- A KDE-private protocol may change between Plasma releases. The compositor
  job runs against the KWin of the builder image, which is Fedora 45's, and
  fails if the protocol stops answering.
- The proposal to KWin of the design's 9.13 gains an item: a supported,
  documented way for a client to learn where a rectangle of another
  program's window is, or to ask the compositor to mark it.

## Verification

- KWin's source at v6.8.2: `src/wayland_server.cpp` (`restrictedInterfaces`,
  `allowInterface`), `src/wayland/plasmawindowmanagement.cpp` (`s_version`
  21, `send_client_geometry`), `src/dbusinterface.cpp`
  (`clientToVariantMap`, `getWindowInfo`, `queryWindowInfo`),
  `src/org.kde.KWin.xml`.
- Licences: `plasma/kwayland` at v6.8.2, `src/client/*` (75 files);
  `plasma/layer-shell-qt` at v6.8.2, `src/*`; `libraries/plasma-wayland-protocols`
  at v1.23.0, `plasma-window-management.xml` and `fake-input.xml`.
- The compositor job (`desktop/ci/compositor.sh`) measures, in screenshots
  KWin takes, that a highlight placed this way lands on the row
  `tur-settings` reported, at scale 1 and 1.5, and fails if it does not.

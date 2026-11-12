# ADR-0016: Desktop and user interface — standalone programs, Plasma by configuration, one identity

- **Status:** Proposed
- **Date:** 2026-11-12
- **Specification sections:** §9 (all), §6.1, §6.3, §27.1, §28.4, §28.5, §30;
  AI control layer §6, §8, §16.6; [ADR-0003](0003-licence.md),
  [ADR-0007](0007-app-centric-routing.md),
  [ADR-0011](0011-application-centre-and-stores.md),
  [ADR-0015](0015-seamless-windows-guest.md). Design:
  [desktop experience](../architecture/desktop-experience.md) (addendum C)

## Context

Nothing the user sees in TUR has been designed. The image shows KDE's first-run
wizard with Fedora's name and wallpaper, then a stock Plasma desktop.

Three constraints decide how the desktop can be built.

1. **The licence boundary.** TUR's own code is under a noncommercial licence
   that cannot apply to code running inside a GPL program (ADR-0003, AGENTS.md).
   The licence review found that the desktop work described in specification §9
   "is largely plugin work": Plasma applets, look-and-feel packages, System
   Settings modules and KWin scripts and effects would all have to be GPL
   (finding F1), and asked for a decision.
2. **What the platform offers a separate program.** Verified on 12 November
   2026 against Plasma 6.8.2, KWin and Qt 6.11.2 (sources in the design,
   section 19):
   - KWin gives any client layer-shell surfaces, including a topmost overlay
     layer and surfaces that let input through; emulated input and screen
     capture through the portals; window geometry over D-Bus; capture exclusion
     for ordinary windows; default shortcuts and window rules as configuration.
   - KWin has no second pointer or seat, no surface immune to emulated input,
     and no privileged prompt. In 6.8 any unsandboxed process in the session
     can inject input and capture the screen without a prompt.
   - Plasma's look and layout can be set by data: a global theme of metadata
     and defaults only, colour schemes, icon themes, wallpapers, a Plymouth
     theme, INI configuration for the login manager, and a pre-seeded panel for
     new accounts. The launcher's search, the system tray, notifications and a
     listing inside System Settings are all reachable over D-Bus or by desktop
     files, without a plugin.
   - KDE's first-run wizard and welcome window can be extended only by QML
     that runs inside them.
3. **One identity.** ADR-0007 dropped two of the three "visual modes" of §9.2.

## Options considered

1. **Theme and extend Plasma with plugins.** Applets for the assistant, a
   look-and-feel package with scripts, System Settings modules, KWin effects
   for the pointer and highlight. The usual way to make a distribution's
   desktop, and the quickest to an integrated result. Every such piece would be
   GPL, so the desktop experience could be used commercially by anyone, which
   is what the owner's licence is meant to prevent; and each piece is tied to
   interfaces KDE changes between releases.
2. **Replace Plasma's shell with one of TUR's own.** A TUR panel, launcher,
   notification surface and settings, as standalone layer-shell programs. Full
   control, and no licence question. It means rebuilding what KDE maintains
   well, against the project's rule to integrate before building and against
   the priority order (a reliable system before a distinctive one).
3. **Plasma as shipped, arranged by configuration; TUR's additions as
   standalone programs.** Everything TUR writes is its own executable that
   talks to Plasma, KWin and the system over D-Bus and standard Wayland
   protocols. Plasma's own surfaces are configured, not modified.

## Decision

Adopt option 3.

1. **Standalone programs.** Every user-interface component TUR writes is a
   separate Qt 6 / QML program under the project licence: `tur-setup`,
   `tur-welcome`, `tur-centre`, `tur-settings`, `tur-diagnostics`,
   `tur-pax-ui`, `tur-pax-overlay`, `tur-pax-approve`, `tur-confirm`,
   `tur-installer`, with a shared module `tur-kit`, and two session services
   in Rust without a window, `tur-notifier` and `tur-search`. Each Qt program
   is a `QGuiApplication` (a `QApplication` loads the GPL widget style), links
   only LGPL Qt modules and LGPL libraries, dynamically, and imports only what
   `desktop/ALLOWED_IMPORTS` lists. A CI job enforces this.
2. **Configuration, not code, for look and layout.** The desktop's colours,
   fonts, icons, wallpaper, default panel, window rules, login and lock
   screens, boot splash, default shortcuts, launcher search registration and
   the entry in System Settings are data files. They contain nothing that a
   GPL host executes through its programming interface. They are the project's
   own work; which terms the project gives them, and the brand artwork in
   particular, is for the legal review.
3. **Upstream-licensed code only where unavoidable, and there is none
   today.** Code that must run inside a GPL host lives in
   `desktop/upstream-licensed/<host>/` under the host's licence, with the
   reason recorded as an amendment to this ADR. The directory starts empty.
   Two reserves are named: a panel layout script for the global theme, if the
   pre-seeded panel proves unreliable; and patches to KWin for features TUR
   needs, which are contributed upstream and are not TUR code.
4. **KDE's first-run wizard and welcome window are replaced, not extended.**
5. **One visual identity**, dark by default with a light variant, defined once
   as tokens in `tur-kit` from which the Plasma colour schemes are generated.
   Blue is the system's accent, gold is reserved for P@x, and the silver frame
   is reserved for the trusted screen and the brand. Applications from other
   ecosystems keep their own look inside their windows.
6. **TUR's settings are a standalone program beside KDE System Settings.**
   Every settings page and row has a stable, registered address
   (`tur.pax.privacy#private-mode`), resolvable over D-Bus, from the command
   line and from a link. KDE's own settings are addressed by module, and by
   row through a tested mapping onto the accessibility tree.
7. **P@x's visible surfaces are clients of the compositor.** The indicator,
   pointer and highlights are layer-shell surfaces; capture and input are
   permitted only while the indicator holds a lease. The trusted confirmation
   is shown by a separate session outside the user's compositor, because KWin
   cannot protect a surface inside it. This item is conditional on a prototype
   passing the trusted-path tests and on the owner's answers to the questions
   in section 17 of the design.
8. **Build and test.** CMake; QML with C++ only for typed D-Bus proxies,
   models and window set-up; Rust stays in the services. CI in a pinned Fedora
   45 container is the only compiler. Every program runs from fixtures, and CI
   returns a picture of every page so that agents and the owner can see what
   was built.
9. **Fedora's marks are replaced by two packages of TUR's own**, `tur-release`
   and `tur-logos`, with `ID=tur` and `ID_LIKE=fedora`, before any image is
   published.
10. **The application centre adds no verdict of its own.** It shows the
    router's options, ratings with their named sources, caveats and "no
    route" answers as the catalogue gives them, through one fixed vocabulary
    that maps one-to-one onto the catalogue's values. Applications from
    another system are marked as such in the launcher by data in their
    desktop entries and icons, never by code in the launcher.

## Consequences

- Finding F1 of the licence review is answered: no part of the desktop
  experience has to be GPL. The cost is that TUR cannot change what Plasma
  itself draws. The panel's internals, the login screen, the lock screen and
  System Settings are KDE's, in KDE's words, with TUR's colours.
- There are two settings programs. They are cross-linked and share one search.
- The assistant's button in the panel is a tray item, not an applet.
- Three guarantees the AI control layer expects from the compositor (a
  compositor-drawn indicator, a pointer of P@x's own, a prompt immune to
  emulated input on the desktop) are not available from KWin 6.8. The design
  supplies each another way or states the interim behaviour, and sections 4,
  6, 8 and 16.6 of the AI control layer are amended. Proposals to KWin follow.
- One program, `tur-pax-overlay`, uses interfaces that KDE treats as its own
  (KWin's D-Bus interface and the Plasma window-management protocol). They are
  public and documented, and the use is confined to that program.
- A new workflow, a builder image and a desktop stage in the boot test are
  added. The builder image pins Qt and the fonts, on which screenshot
  references depend.
- AGENTS.md is amended: the list of GPL-only Qt modules is longer than it
  says; several KDE frameworks contain GPL files and must not be linked or
  imported; `desktop/` holds C++ as well as QML.
- Each rebase onto a new Plasma release needs the verified facts checked
  again. The end-to-end tests exist to catch what moved.

## Verification

- **Toolchain** (task D-01): that a `QGuiApplication` gets the LGPL
  `org.kde.breeze` style and the system colours through Kirigami in the image;
  that pages can be rendered to images under the offscreen platform.
- **Data-only layout** (D-04, D-05): the boot test logs a new account in and
  finds the intended panel, wallpaper and colour scheme, and no KDE welcome
  window, on every image build.
- **No Fedora marks** (D-03): the image test and the screenshots contain none;
  the boot test passes with the release package replaced.
- **Licence boundary:** the `boundary` job fails on a GPL identifier outside
  `desktop/upstream-licensed/`, on a forbidden import or module, and on a
  `QApplication`.
- **Trusted screen** (D-16): emulated input from the user's session reaches
  nothing on it; it is absent from a capture taken in the user's session; the
  user's processes cannot open the virtual input device node. If the prototype
  fails, item 7 is revisited and P@x stays limited to classes 0 and 1.
- **Addressing** (D-07, D-20): `tur-settings <address>` reveals the row; every
  mapped KDE row resolves to exactly one control in the image's System
  Settings.
- **Legal review** (§17.2): the terms for theme data and brand artwork; LGPL
  obligations of TUR's programs; whether output of Qt's QML compilers carries
  any obligation; the statement of origin with respect to Fedora.
- **At each Plasma rebase:** layer-shell availability, the input and capture
  interfaces and who may use them, capture exclusion, window rules, the
  global-theme package format, the login manager's configuration keys.

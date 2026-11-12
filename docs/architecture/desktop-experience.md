# Desktop experience

**Specification addendum C — Revision 1 (12 November 2026)**
**Applies to:** specification revision 1.0
**Related decision:**
[ADR-0016](../adr/0016-desktop-and-user-interface.md) (Proposed)
**Builds on:** the [AI control layer](ai-control-layer.md) (addendum A), the
[Android runtime](android-runtime.md) (addendum B), the
[seamless Windows guest](windows-guest.md) and the
[catalogue and router](catalogue-and-routing.md)

This addendum designs what a person sees and touches in TUR: the look, the
first five minutes, the desktop, the application centre, P@x's interface, the
settings, and the moments at which the system has to explain itself. It also
gives the engineering plan to build and test them. It replaces §9.2 of the
specification and lists every other amendment in
[section 18](#18-amendments-to-specification-revision-10).

Nothing here is implemented. No code accompanies this document.

**The project's name.** The project was called TUR until 12 November 2026
([ADR-0009](../adr/0009-project-name.md)). This document uses the new names
throughout: `tur-…` programs, `org.tur.*` D-Bus names, `TUR-…` error codes,
`tur-pax` for the assistant's service. Other documents still say `tur-…` until
the mechanical rename; `tur-diag` here is `tur-diag` there.

**How to read the claims in this document.** Four kinds of statement appear.

- **Verified.** Checked against the upstream source on 12 November 2026:
  KWin, Plasma and Qt at the versions in the image (Plasma 6.8.2, Qt 6.11.2),
  Fedora's published terms and package definitions. Each has a reference such
  as [K3] to [section 19](#19-sources). "Verified" means read in source code or
  documentation. Nothing was run: no agent may build or boot anything
  (AGENTS.md).
- **Assumption.** Not verified. Marked as such, to be verified before or while
  implementing (§0, rule 14).
- **Design judgement.** A choice made here on the designer's reasoning. No
  user research has been done for TUR, and none is claimed. Judgements are the
  first things to revisit when real people use the system.
- **Exact wording.** Text set as a quotation or inside a drawing is the text
  that appears on screen, in British English, as the source strings for
  translation.

---

## 1. Purpose and scope

**In scope**

- One visual identity for TUR, in a dark and a light variant.
- Boot splash, first-run setup, the first desktop and the introduction to P@x.
- The desktop layout: what is Plasma configuration and what TUR adds.
- The application centre ([ADR-0011](../adr/0011-application-centre-and-stores.md)).
- Every surface of P@x ([AI control layer](ai-control-layer.md)): summoning,
  conversation, indicator, pointer, highlight, approvals, the trusted
  confirmation, repair sessions, memory, schedules, privacy, spending, setup.
- TUR's settings and the scheme that makes every setting addressable.
- Updates and rollback, diagnostics, storage prompts, devices, the installer.
- Removal of Fedora's marks.
- Accessibility and internationalisation requirements.
- The programs and theme packages to build, with the licence side of each;
  repository layout, build, continuous integration, tests and an ordered task
  list.

**Out of scope**

- The services behind the user interface (`tur-pax`, the catalogue and router,
  `tur-update`, `tur-storage`, `tur-device`, `tur-diag`, the installer core).
  Where the user interface needs something from one of them, it is stated as a
  requirement on that service.
- How Windows and Android windows are brought onto the desktop (the desktop
  bridge; [android-runtime.md](android-runtime.md), section 7;
  [windows-guest.md](windows-guest.md), section 6). This document says how
  those windows and their launcher entries are presented once they are there.
- The catalogue and the router
  ([catalogue-and-routing.md](catalogue-and-routing.md)). The application
  centre displays their answers and adds none of its own.
- The voice pipeline. Only its visible states are designed here.
- Final artwork. The logo exists; icons, wallpaper and the boot mark are
  described, and the two drawings in `desktop-assets/` are sketches.

## 2. Principles

1. **Say what is true.** The interface states what happened, what is known and
   who says so. It shows no percentage it cannot compute, no rating without a
   source and no reassurance it cannot back.
2. **One system, not four.** Applications from every ecosystem are found,
   installed, started and switched between in the same places, under their own
   names and icons.
3. **Calm by default.** TUR does not interrupt, advertise, nag or animate for
   its own sake. P@x is off until asked for and is never promoted.
4. **The person decides, and can tell who is asking.** What the system says,
   what P@x says and what an application says are visually different. Anything
   that asks for approval says exactly what it will do.
5. **Plasma where Plasma is good.** The panel, launcher, window management,
   notifications, file manager and most settings are KDE's, configured, not
   rewritten. TUR builds only what makes the parts one system
   (README, "How it is built").
6. **Standalone programs, standard interfaces.** Everything TUR writes for the
   desktop is its own program that talks over D-Bus and Wayland protocols
   ([section 4](#4-the-licence-boundary-on-the-desktop)).
7. **Usable by everyone it can be.** Keyboard, screen reader, contrast and
   reduced motion are requirements with tests, not a later pass
   ([section 13](#13-accessibility-and-internationalisation)).
8. **Nothing starts until it is needed.** No TUR user-interface program runs
   in an idle session except a notifier, and nothing of P@x runs while P@x is
   off (§30.3).

## 3. State of upstream, November 2026

What the platform offers a separate, unprivileged program, as verified for
this design. The detail and the sources are in section 19; "since" gives the
Plasma release.

### 3.1 KWin on Wayland (Plasma 6.8)

| Capability | State | Reference |
|---|---|---|
| Layer-shell surfaces (`zwlr_layer_shell_v1` version 5) for any client, including the overlay layer, which is KWin's topmost | **Exists.** Not restricted. LayerShellQt (LGPL) gives a QML interface: layer, anchors, margins, exclusive zone, keyboard interactivity, screen | [K1] |
| A surface that lets input through | **Exists.** An empty input region; Qt sends one for `Qt::WindowTransparentForInput` | [K1] |
| Client surfaces while the session is locked | Only lock-screen windows are rendered | [K1] |
| Emulated input for a client: remote-desktop portal with libei | **Exists** since 6.1. A standing grant without a dialog exists since 6.3 (`kde-authorized`). Since 6.7 a "Remote Control Started" notification is shown regardless | [K2] |
| Emulated touch as well as pointer and keyboard | **Exists** in KWin's libei backend | [K2] |
| Who may inject input or capture the screen without a prompt | In 6.8, **any process in the session that is not sandboxed**. The desktop-file allow-lists of 6.7 and earlier were removed; KWin's private input, screen-cast, window-management and screenshot interfaces are now denied only to sandboxed clients | [K2] [K4] |
| A second pointer, or a second seat with its own focus | **Does not exist.** Emulated devices feed the one seat. An upstream issue is open, with no implementation | [K3] |
| Telling emulated from physical input | Inside KWin every event carries its device. **Nothing is exposed** to clients, and no KWin filter uses it | [K3] |
| A surface immune to emulated input | **Does not exist**, for any surface, the lock screen and authentication dialogs included | [K3] [K9] |
| Excluding a window from screen capture | **Exists** since 6.6; as a window rule since 6.7; honoured by screenshots since 6.7. Applies to ordinary windows only, not layer-shell surfaces, and hides the window from every consumer | [K4] |
| A client marking its own surface as not to be captured | A draft protocol, not merged | [K4] |
| Screen cast of a screen, a window or a region; cursor hidden, embedded or as metadata | **Exists.** The portal shows a tray item while a stream is consumed | [K5] |
| Geometry of another program's window, in screen coordinates | **Exists** over D-Bus (`getWindowInfo`: frame geometry) and in the Plasma window-management protocol (frame and client-area geometry, stacking order) | [K6] |
| Drawing a rectangle or a second pointer through a built-in effect | **Does not exist.** The highlight effect marks whole windows | [K11] |
| A default global shortcut for a program | **Exists** as configuration: `X-KDE-Shortcuts=` in the program's desktop file. A system-wide `kglobalshortcutsrc` is not read | [K7] |
| Window rules shipped by a distribution | **Exists.** `/etc/xdg/kwinrulesrc` is read; rules can force keep-above, position, layer, exclusion from task bar and switcher, and capture exclusion. Ordinary windows only | [K8] |
| Replacing the authentication agent | polkit accepts one agent per session; KDE's is a separate program started by a unit that a distribution can replace (the replacement is an **assumption**) | [K9] |
| A compositor-drawn or otherwise privileged prompt | **Does not exist.** No session-lock or secure-surface protocol is implemented | [K9] [K10] |
| Running KWin with a single program and no desktop shell | **Exists**: `kwin_wayland --exit-with-session`, `--no-lockscreen`, `--no-global-shortcuts`, `--locale1` | [K12] |

The consequence for P@x is drawn in
[section 9](#9-pxs-interface): everything the user needs to *see* can be built
today; the guarantees the trusted path needs cannot come from KWin 6.8 and are
obtained another way; two things (an independent pointer, and input immunity
for a window on the desktop) need work in KWin itself.

### 3.2 Configuring Plasma without writing plugins

| Need | How, and whether it is data or code | Reference |
|---|---|---|
| Default colours, icons, cursor, wallpaper, window decoration, splash | A global theme whose package holds only `metadata.json` and `contents/defaults` (and optionally `contents/colors` and previews). **Data.** The session writes these defaults at login, and they take precedence over a distribution's `kdeglobals` | [P1] [P2] |
| Which global theme is the default | `kdeglobals`, `[KDE] LookAndFeelPackage=`. **Data** | [P2] |
| Light by day, dark by night | Plasma switches between two chosen global themes on Night Light's schedule, since 6.5. The configuration keys were **not verified** | [P13] |
| The panel and its contents | Either a layout script in the global theme (JavaScript run by the shell: **code**), or a complete applets file for new users in `/etc/skel/.config/` (**data**). A system-wide applets file in `/etc/xdg` is **not read**: the shell opens the file without the cascade | [P3] |
| The launcher's icon | The applet's default icon name is `start-here-kde-symbolic`; an icon theme that supplies it rebrands the launcher. **Data** | [P3] |
| Defaults on an image-based system | KDE merges same-named files along `XDG_CONFIG_DIRS`; `[$i]` makes a key, group or file immutable. `/etc` is merged three ways on update and locally changed files win, so defaults belong under `/usr` | [P4] |
| Colour schemes | `.colors` files in `/usr/share/color-schemes/`. **Data.** The default Plasma style follows the colour scheme, so no desktop theme of TUR's own is needed | [P5] |
| Icon and cursor themes, wallpapers | Directories of images with an index or metadata file. **Data** | [P6] |
| Login screen | Plasma Login Manager (since 6.6) is configured by an INI file under `/usr/lib/plasmalogin/`, applies the system's global theme, and **does not accept QML themes** | [P7] |
| Lock screen | Wallpaper through `kscreenlockerrc`. **Data.** Its interface is Plasma's | [P7] |
| Boot splash | A Plymouth theme for the `two-step` module is an INI file and images. **Data.** The default is set in `plymouthd.defaults` and the initramfs must be regenerated in the image build | [P8] |
| First-run setup | `plasma-setup` (since 6.6): a unit that runs before the display manager when `/etc/plasma-setup-done` is absent, logs a system user in to a full Plasma session, and creates the account with `useradd`. Its pages are QML packages (**code** inside a GPL program); "Welcome to Plasma Desktop" and the wallpaper are compiled in; "Powered by" shows the system's `NAME` | [P9] |
| KDE's welcome window | Suppressed by a system-wide `plasma-welcomerc`. **Data.** Extra pages would be QML inside the program (**code**) | [P10] |
| Tray item, notifications | StatusNotifierItem and the notification service, both over D-Bus | [P12] |
| Results in the launcher's search | A D-Bus search provider: a desktop file in `/usr/share/krunner/dbusplugins/` naming a service that implements `org.kde.krunner1`. The provider is a separate process; Kickoff uses the same list | [P12] |
| An entry in KDE System Settings that opens another program | A desktop file in `/usr/share/plasma/systemsettings/externalmodules/`. System Settings starts the program and shows "… is an external application and has been automatically launched". **Data** | [S2] |

### 3.3 Addressing settings in KDE

- A settings module has an identifier such as `kcm_kscreen`. `systemsettings
  kcm_kscreen` opens it; a second invocation switches a running System Settings
  to it; `systemsettings://kcm_kscreen` does the same from a link [S1].
- A few modules accept arguments that select an item, for example a network
  connection by UUID or an application in the notifications module [S3].
- **No control inside a module can be addressed from outside.** There is a
  mechanism for highlighting settings that differ from their defaults, and an
  upstream discussion in which a maintainer said scrolling to a search match
  "would require a ton of engineering" [S4].
- The accessibility tree is the only way in. Qt exposes Qt Quick controls over
  AT-SPI with roles, names, states, actions, extents relative to the window,
  and, since Qt 6.8, an identifier that defaults to the control's QML `id`
  [S5]. The quality of System Settings' tree in 6.8 was **not assessed**.

### 3.4 Fedora's marks

- Fedora's trademark guidelines (text last changed 5 December 2024) allow
  software that mixes or modifies Fedora's packages to be distributed provided
  that the `fedora-logos`, `fedora-release` and `fedora-release-notes` packages
  are removed, and they offer the "Fedora Remix" mark under conditions. Any
  other use of a Fedora mark for derived work is not permitted [F1].
- `fedora-logos` is restricted by copyright as well: its licence allows
  redistribution only as part of an accepted Fedora spin [F2].
- Fedora ships `generic-release` and `generic-logos` as mark-free
  replacements, and its bootc documentation shows the swap [F3].
- No ruling from Fedora's legal team specific to bootc images was found.
  Statements by Fedora contributors in 2026 say that a publicly published
  derived image must have the branding swapped [F6].

The complete checklist is [section 12](#12-replacing-fedoras-marks).

### 3.5 Qt and KDE libraries a TUR program may link

- **Qt 6.11.** Core, Gui, Qml, Quick, Quick Controls, Layouts, Dialogs, Shapes,
  Effects, Svg, DBus, Network, Multimedia, TextToSpeech, WebSockets and the
  Wayland client integration are available under LGPL-3.0. **GPL-only**, and so
  forbidden to TUR's programs: Virtual Keyboard, Wayland Compositor, Quick 3D,
  Quick 3D Physics, Quick Timeline, Graphs, Charts, Data Visualization, Lottie,
  HTTP Server, GRPC, MQTT, CoAP, Network Authorization, Canvas Painter [Q1].
  AGENTS.md names five of these; the full list is longer
  ([section 18](#18-amendments-to-specification-revision-10)).
- **KDE Frameworks 6.31.** Kirigami, KCoreAddons, KNotifications,
  KStatusNotifierItem, KColorScheme, KWindowSystem, KI18n and LayerShellQt are
  LGPL throughout their shipped code. **Several frameworks described as LGPL
  contain GPL-only files:** Kirigami Addons (three components), the QML plugin
  of KSvg, the icon dialog of KIconThemes, parts of KIO, libplasma and Purpose.
  The Breeze widget style has no LGPL option at all [Q2].
- **Which Qt Quick style a program gets under Plasma.** A program that is a
  `QApplication` gets `org.kde.desktop`, which paints through the widget style
  and therefore loads the GPL Breeze style plugin at run time. A program that
  is a `QGuiApplication` gets `org.kde.breeze` (qqc2-breeze-style, LGPL, pure
  QML) when it is installed [Q3].

## 4. The licence boundary on the desktop

TUR's own code is under a noncommercial licence that cannot be applied to code
running inside a GPL program (AGENTS.md, "Licence boundary rules";
[licence review](../dependencies/licence-review.md), section 4.3 and finding
F1). Finding F1 said that the desktop work of specification §9 was "largely
plugin work". This design answers it: **none of the desktop is built as a
plugin.** Every component is on one of three sides.

| Side | What it is | Licence | Where it lives |
|---|---|---|---|
| **A. TUR programs** | Standalone executables written by the project. They link Qt (LGPL modules only) and LGPL libraries dynamically, and reach Plasma, KWin and system services over D-Bus, Wayland protocols and the command line | The project licence | `desktop/` |
| **B. Configuration and theme data** | INI and JSON files, colour schemes, images, icon and cursor themes, a Plymouth theme, desktop files, window rules. No statement in them is executed by a GPL host through its own programming interface | The project's own work. Which licence the project gives them is its choice and a question for the legal review; the brand artwork needs trademark-style terms, not a software licence | `desktop/data/`, `branding/` |
| **C. Upstream-licensed code** | Code that a GPL program loads and runs through its programming interface: a Plasma layout script, a splash or lock-screen QML file, a KWin script or effect, a System Settings module, a setup-wizard page, a Plasma applet | The host's licence, GPL-2.0-or-later for Plasma and KWin | `desktop/upstream-licensed/<host>/`, each with its own `LICENSE` and a recorded justification |

**Side C is empty in this design.** Each place where a plugin would have been
the usual answer has an answer on side A or B:

| Usual answer (side C) | This design |
|---|---|
| Panel layout script in a global theme | A complete applets file for new accounts, as data ([section 7.1](#71-panel)) |
| Plasma applet for the assistant | A tray item over D-Bus, and layer-shell surfaces ([section 9](#9-pxs-interface)) |
| KWin effect for the pointer and highlight | A layer-shell overlay client ([9.3](#93-the-activity-indicator) to [9.5](#95-highlighting)) |
| KWin script for window positions | KWin's D-Bus interface and window-management protocol ([9.5](#95-highlighting)) |
| System Settings modules | A standalone settings program, listed in System Settings by a desktop file ([section 10](#10-settings)) |
| Pages added to KDE's setup wizard and welcome window | Standalone programs that replace them ([section 6](#6-the-first-five-minutes)) |
| KRunner plugin | A D-Bus search provider ([section 7.2](#72-launcher)) |
| QML theme for the login or lock screen | Not possible for the login manager, and not needed: both follow the global theme's data |
| Splash screen QML | No splash; the desktop appears when it is ready ([6.4](#64-the-first-desktop)) |

Two side C pieces are held **in reserve**, to be used only if the data route
proves unreliable in the boot test, each needing its own entry in ADR-0016:

1. A layout script of about fifteen lines for the global theme, if a
   pre-seeded applets file does not produce the intended panel on a new
   account or breaks across a Plasma update.
2. Contributions to KWin itself for the features of
   [section 9.13](#913-what-can-be-built-today-and-what-is-blocked-upstream).
   These are upstream patches under KWin's licence, sent upstream, and are not
   part of TUR's code at all.

Rules that follow, for every implementation task:

- **A TUR program is a `QGuiApplication`**, never a `QApplication`, so that it
  does not load the GPL widget style. It uses Qt Quick Controls with the
  `org.kde.breeze` style in the session and the Basic style outside it
  ([section 5.7](#57-consistency-with-other-applications)).
- **Allowed imports** are listed in `desktop/ALLOWED_IMPORTS`, and a CI check
  fails on any QML import or linked library outside it. Not on the list, and
  not to be added without an ADR: Kirigami Addons, `org.kde.ksvg`, the
  KIconThemes QML plugin, KIO, libplasma and `org.kde.plasma.*`, Purpose, and
  every GPL-only Qt module of section 3.5.
- **No file is started from a copy of an upstream file.** A colour scheme that
  begins as a copy of Breeze's is a modification of an LGPL file. TUR's are
  written from the format's key names with TUR's own values.
- **A TUR program uses public interfaces.** Where this design uses an interface
  that KDE treats as its own (the Plasma window-management protocol, KWin's
  D-Bus interface), the component inventory says so, and the use is confined to
  one program so that it can be replaced ([licence review](../dependencies/licence-review.md),
  section 4.5).
- **Tools are not linked.** `qmltestrunner`, `qmllint` and the shader tool are
  GPL programs that run at build or test time. Whether code generated by Qt's
  QML compilers carries any obligation is a question for the legal review;
  until answered, TUR uses only the default QML cache and no ahead-of-time
  compilation to C++.

## 5. Visual direction

One identity, in a dark and a light variant. ADR-0007 dropped two of the three
"visual modes" of specification §9.2; this section replaces §9.2 with one
polished default. Everything in this section is a design judgement unless
marked verified.

### 5.1 Where it comes from

The logo (`branding/tur-logo.png`) has four elements, and each is given one
job. Colours were sampled from the image.

| In the logo | Sampled | Job in the interface |
|---|---|---|
| The black ground | `#000000` | TUR is dark by default. Surfaces are near-black with a blue cast, not grey |
| The blue lotus and the infinity sign | `#00A4FA`, shading to `#004C8D`, highlight `#6AF2FD` | **Blue is the system.** The accent of TUR itself: focus, selection, primary buttons, links, progress |
| The golden flame and ring | `#E5B24E`, tip `#F8BF3D`, core `#FDF682` | **Gold is P@x.** P@x's pointer, highlight, indicator, tray item and name. Gold is used for nothing else, so that gold on screen always means the assistant |
| The brushed-metal lettering | `#CDD3DC`, shading to `#687A93` | **Silver is the frame.** Used only where TUR presents itself or asks for trust: the boot mark, the login screen's wordmark, the frame of the trusted screen. Never on ordinary controls |

Two consequences shape everything else. First, the interface is quiet: large
dark surfaces, one blue accent, almost no decoration, so that the two reserved
colours can carry meaning. Second, the logo's gloss (glows, bevels, lens
flares) stays in the logo. The interface is flat. A glossy control ages in a
year and reads badly at small sizes.

### 5.2 Palette

Tokens are named by role. `tur-kit` defines them once; the Plasma colour
schemes `TurDark.colors` and `TurLight.colors` are generated from the same
table, so that TUR's programs and KDE's applications agree.

| Token | Role | Dark | Light |
|---|---|---|---|
| `surface.base` | Window background | `#0B0F17` | `#F3F5F9` |
| `surface.raised` | Lists, cards, text fields | `#131A27` | `#FFFFFF` |
| `surface.overlay` | Menus, dialogs, tooltips, sheets | `#1B2434` | `#FFFFFF` |
| `surface.sunken` | Technical detail, command text | `#070A10` | `#E7EBF2` |
| `line.subtle` | Separators (decoration only) | `#27324A` | `#D3D9E4` |
| `line.control` | Borders of fields, switches, check boxes | `#6B7A95` | `#6F7C93` |
| `text.primary` | Body text, titles | `#EEF1F6` | `#10151F` |
| `text.secondary` | Supporting text | `#A9B4C6` | `#465368` |
| `text.disabled` | Unavailable controls | `#657086` | `#8A94A6` |
| `accent` | Primary button fill, switch on, progress | `#2AA9F8` | `#0A68C4` |
| `accent.on` | Text and icons on `accent` | `#04111E` | `#FFFFFF` |
| `accent.text` | Links, focus ring | `#5BBEFF` | `#0A5CB0` |
| `selection` | Selected row or text | `#1267C4` | `#0A68C4` |
| `selection.on` | Text on `selection` | `#FFFFFF` | `#FFFFFF` |
| `pax` | P@x's name, icons and lines on a surface | `#E8B44C` | `#7A5200` |
| `pax.fill` | Fill of P@x's pointer, pill and tray glyph | `#E8B44C` | `#E8B44C` |
| `pax.on` | Text and outline on `pax.fill` | `#1A1204` | `#1A1204` |
| `positive` | Works, done | `#4CCB8F` | `#157347` |
| `warning` | Needs attention; a repair session | `#FF9A52` | `#A34A00` |
| `negative` | Failed; destroys data | `#FF7A7A` | `#B3261E` |

The accent is the lotus blue eased slightly towards neutral (`#2AA9F8`, from
the sampled `#00A4FA`) so that a filled button does not glare on a dark
surface. Warning is orange, not yellow, to stay apart from P@x's gold; no
status is ever shown by colour alone ([section 13](#13-accessibility-and-internationalisation)).

**Fixed palettes** that do not follow the variant:

| Use | Colours |
|---|---|
| Trusted screen ([9.8](#98-the-trusted-confirmation-prompt-class-3)) | Field `#0A2A4A`, frame `#C9D1DB`, text `#EEF1F6`, supporting text `#A9B4C6`, link `#5BBEFF`, error `#FF7A7A` |
| P@x's overlay (pointer, pill, frame, highlight) | Fill `#E8B44C`, outline `#0B1220`, hairline `#FFFFFF`, text `#1A1204`. Fixed because it sits over content TUR does not control |
| Boot splash | Ground `#000000`, mark in the logo's colours, text `#A9B4C6` |
| High contrast, for TUR's system surfaces | Ground `#000000`, text and lines `#FFFFFF`, focus and selection `#FFE600`, links `#00E5FF` |

**Contrast.** Ratios by the WCAG 2.2 formula, computed for this document. The
requirement is 4.5:1 for text and 3:1 for the boundary of a control and for
graphics that carry meaning [A2]. The table becomes a unit test in `tur-kit`:
a token change that breaks a ratio fails the build.

| Pair | Dark, on base / raised / overlay | Light, on base / raised |
|---|---|---|
| `text.primary` | 16.9 / 15.4 / 13.8 | 16.7 / 18.3 |
| `text.secondary` | 9.2 / 8.3 / 7.4 | 7.1 / 7.8 |
| `accent.text` | 9.4 / 8.5 / 7.6 | 6.1 / 6.6 |
| `pax` (as text or line) | 10.1 / 9.2 / 8.2 | 6.3 / 6.9 |
| `positive` | 9.4 / 8.5 / 7.6 | 5.4 / 5.9 |
| `warning` | 9.1 / 8.3 / 7.4 | 5.4 / 5.9 |
| `negative` | 7.6 / 6.9 / 6.2 | 6.0 / 6.5 |
| `line.control` (needs 3) | 4.4 / 4.0 / 3.6 | 3.9 / 4.2 |
| `accent` as a control (needs 3) | 7.4 / 6.8 / 6.0 | 5.1 / 5.5 |
| `selection` against its surroundings (needs 3) | 3.4 / 3.1 / — | 5.1 / 5.5 |
| `text.disabled` (exempt) | 3.9 | 2.8 |

| Pair | Ratio |
|---|---|
| `accent.on` on `accent` | 7.4 dark, 5.5 light |
| `selection.on` on `selection` | 5.6 dark, 5.5 light |
| `pax.on` on `pax.fill` | 9.8 |
| Overlay outline `#0B1220` against fill `#E8B44C` | 9.9 |
| Trusted screen: text, supporting text, frame, link, error on `#0A2A4A` | 12.9, 7.0, 9.4, 7.1, 5.8 |
| High contrast: white, yellow, cyan on black | 21.0, 16.6, 13.7 |
| `text.primary` and `text.secondary` on `surface.sunken` | 17.5 and 9.5 dark; 15.3 and 6.5 light |

`selection` is not used on `surface.overlay` in the dark variant; menus mark
the current item with `accent` at 6.0:1 instead. White text on the dark
variant's `accent` would be 2.6:1, which is why `accent.on` is dark.

A drawing of both variants is in
[desktop-assets/palette.svg](desktop-assets/palette.svg).

### 5.3 Typography

| Use | Family | Fedora 45 package | Licence |
|---|---|---|---|
| Interface | Inter (variable) | `rsms-inter-fonts` 4.1 | OFL-1.1 |
| Commands, codes, technical detail | JetBrains Mono | `jetbrains-mono-fonts` 2.304 | OFL-1.1 |
| Scripts Inter lacks | Noto Sans and Noto Sans CJK, by fontconfig fallback | `google-noto-sans-vf-fonts`, `google-noto-sans-cjk-vf-fonts` | OFL-1.1 |
| Emoji | Noto Color Emoji | `google-noto-color-emoji-fonts` | OFL-1.1 and Apache-2.0 |

All four are in Fedora 45 and are redistributable under their licences [T1].
The first two are added to the image; whether the Kinoite image already
carries the CJK fonts was not verified. Inter is set as the desktop's general
font and JetBrains Mono as its fixed-width font in the global theme's defaults
(Plasma's own defaults are Noto Sans 10 pt and Hack 10 pt [T1]), so KDE's
applications and TUR's use the same faces.

Why Inter: it is neutral, built for interfaces at small sizes, has tabular
figures, and its plain geometry sits well beside the wide, squared lettering
of the wordmark without imitating it. The wordmark itself is artwork: "TUR" is
never typeset in a font to stand in for the logo. The Red Hat typefaces are
close in spirit to the wordmark but carry another company's name; none of the
wide geometric faces that would echo it more strongly (Orbitron, Michroma,
Saira) is packaged in Fedora 45 [T1].

| Style | Size and weight | Use |
|---|---|---|
| Display | 22 pt, 600 | Setup, installer and trusted-screen titles |
| Title | 15 pt, 600 | Window and page titles |
| Heading | 12 pt, 600 | Section headings, the title of a sheet or message |
| Body | 10 pt, 400 | Everything else |
| Strong | 10 pt, 600 | The one thing in a paragraph that must be seen |
| Small | 9 pt, 400 | Supporting lines, captions. Nothing is smaller |
| Label | 9 pt, 600, capitals, letter-spacing +8 % | Group labels on the trusted screen and the destructive confirmation only; echoes the logo's tag line |
| Mono | 10 pt, 400 | Technical detail |

Sizes are in points and follow the user's font setting and scale factor. Line
height is 1.4 for body text. A message is at most 72 characters wide. Numbers
that change (progress, time left, cost) use tabular figures so they do not
shift.

### 5.4 Shape, space and icons

- **Radius.** 6 px for controls, 10 px for cards and list containers, 14 px
  for sheets, dialogs and P@x's pill. These sit beside Breeze's rounded
  controls without a visible seam.
- **Lines.** 1 px. Depth is shown by the four surface tones, not by shadows,
  inside windows. Only floating things (menus, sheets, P@x's overlay) have a
  shadow.
- **Space.** A 4 px grid; 8 px between related things, 16 px between groups,
  24 px page margins.
- **Targets.** At least 36 px high for pointer use, 44 px where touch is
  likely (setup, installer, trusted screen), comfortably above WCAG 2.2's
  24 px minimum [A2].
- **Focus.** A 2 px ring in `accent.text`, 2 px clear of the control, on every
  focusable thing, never removed.
- **Icons.** Breeze's, loaded from the system icon theme at run time, not
  copied into TUR's programs. TUR draws icons only for its own concepts: the
  lotus mark (launcher, system logo), the flame (P@x and its states), the five
  rating icons of [section 8.2](#82-words-the-application-centre-uses), and
  neutral glyphs for the kinds of application. **No vendor's logo** is used to
  mean "Windows", "Android" or "macOS": those are other companies' marks, and
  the label in words is clearer anyway.
- **The lotus's curve.** The one decorative shape. It appears large and faint
  in the wallpaper and the setup background, and nowhere inside application
  windows.

### 5.5 Motion

- Three durations: 120 ms for a control changing state, 200 ms for something
  appearing or leaving, 320 ms for a sheet or panel. Ease out. Nothing bounces.
- Nothing loops except an indicator of real activity: a spinner, the
  "Listening" pulse, a progress bar.
- P@x's pointer has its own timing ([9.4](#94-pxs-pointer)), because its
  purpose is to be followed by eye.
- **Reduced motion.** TUR follows the desktop's animation-speed setting. In
  the session, durations come from Kirigami's units, which scale with it. Qt
  6.11 has no reduced-motion hint of its own (it arrives in 6.12), so
  `tur-kit` reads the desktop's setting, `AnimationDurationFactor`, for the
  programs that do not use Kirigami [A1]. At zero: no movement, cross-fades of
  at most 100 ms, P@x's pointer fades between positions.
- Effects that need shaders are decoration only. The software renderer used in
  safe graphics mode and in tests does not draw them [Q6], so nothing may
  depend on one to be understood.

### 5.6 Tone of voice

TUR speaks plainly, in full sentences, and assumes an intelligent adult who is
not a specialist.

| Rule | Instead of | Write |
|---|---|---|
| Say what happened, then what to do | An error occurred. | The update was not applied. Your computer is unchanged. |
| Name the thing | The operation failed on the device. | TUR could not open "Windows (C:)" for writing. |
| TUR is the subject when TUR acted | The volume has been mounted read-only. | TUR opened it read-only to protect your data. |
| No blame, no alarm, no cheer | Oops! You entered an invalid phrase!! | That is not the phrase shown above. |
| No filler | Please note that you may simply… | _(say the thing)_ |
| Buttons are verbs that complete the title | OK · Cancel | Erase disk and install · Back |
| Numbers only when real | 73 % complete | Copying the system image · 2.1 GiB of 5.4 GiB |
| Never offer the unsafe thing as the fix | Force mount anyway | _(not offered: §6.3)_ |
| Technical terms stay in the technical layer | NTFS volume has the dirty bit set | Windows did not shut this disk down properly. |

**Who is speaking.** Three voices, each with its own container, never mixed:

- *The system* speaks in the third person about itself and about P@x: "TUR
  went back to version 0.3.2." "P@x is asking to send this email." System text
  about P@x is in a bordered card that carries the TUR mark.
- *P@x* speaks in the first person, in the conversation only: "I'll turn on
  Night Light." What P@x writes is the model's output and is never shown as
  system text or placed on an approval surface.
- *An application* speaks in its own window.

**The two-layer message** (§6.3). Every error and every notable event uses
one component, `Message`, from `tur-kit`, so that they read alike everywhere:

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ !  The Windows volume was not mounted for writing.      ← what happened  │
│                                                                          │
│    Reason                                                                │
│    The volume appears to be hibernated or in an unsafe state.            │
│                                                                          │
│    What to do                                                            │
│    Start Windows, turn off Fast Startup and hibernation, shut down       │
│    fully, then try again.                                                │
│                                                                          │
│    [Try again]   [Create a support bundle]   [Ask P@x to look into it]   │
│                                                                          │
│    Technical detail ▸                             TUR-STORAGE-NTFS-004   │
└──────────────────────────────────────────────────────────────────────────┘
```

- The first layer is the registry's user message: one sentence of what
  happened, a reason, an action. The severity is an icon and a word for screen
  readers, not only a colour.
- The code is always visible, small, and can be selected and copied. It is the
  one piece of jargon on the first layer, because it is what a person quotes
  when asking for help.
- "Technical detail" is closed by default and opens in place: command or
  component, exit status, device identifiers, evidence. It is in the fixed-width
  face on `surface.sunken`, with a "Copy" button.
- Buttons: the safe action first; "Create a support bundle" whenever the
  message is an error; "Ask P@x to look into it" only when P@x is on.
- The text comes from the code's registry entry (§29.3). User-interface code
  does not write error text.

### 5.7 Consistency with other applications

| Applications | What TUR does | What it leaves alone |
|---|---|---|
| **TUR's own, in the session** (`tur-centre`, `tur-settings`, `tur-welcome`, `tur-diagnostics`, `tur-pax-ui`) | Qt Quick Controls with the `org.kde.breeze` style and Kirigami's page and layout components, so they take the system colour scheme, font, icon theme and animation speed, and look like the KDE applications beside them. `tur-kit` adds TUR's tokens and the components KDE has no equivalent for | — |
| **TUR's own, outside the session** (`tur-setup`, `tur-installer`, `tur-confirm`, `tur-pax-overlay`) | Qt Quick Controls' Basic style, themed entirely by `tur-kit` with the fixed palettes. They run before a user exists, on another screen, or over other content, and must not depend on a user's theme or on Plasma | — |
| **KDE applications** | TUR's colour scheme, fonts, icon overrides and wallpaper through the global theme. Breeze's window decoration, coloured by the scheme | Their layout and behaviour |
| **GTK applications** | Whatever Plasma already does to pass the colour scheme, dark preference and fonts to GTK | — |
| **Windows applications**, in the compatibility layer or inside Windows | The window frame (KWin's decoration, where the application accepts one), cursor theme, scale factor, and the application's own name and icon in the panel and task switcher | Everything inside the window. A Windows application keeps Windows' controls, fonts, file dialogs and its own dark or light theme. TUR does not restyle it |
| **Android applications** | The same frame, name and icon; the dark or light preference passed into Android (an **assumption**) | Everything inside the window |

The line is deliberate (§9.5): theme what frames an application, never what is
inside it. A half-restyled Windows application is worse than an honest one.

`org.kde.breeze` for a `QGuiApplication` is what Plasma's platform theme
selects [Q3]. Two things are **assumptions to verify** in the first build
task: that `qqc2-breeze-style` is in, or can be added to, the image, and that
Kirigami takes the system colour scheme with it. If either fails, the
fallback is Qt Quick Controls' Basic style themed by `tur-kit` everywhere,
reading the colour scheme through KColorScheme. The `org.kde.desktop` style is
not a fallback: it requires a `QApplication` and paints through the GPL widget
style ([section 4](#4-the-licence-boundary-on-the-desktop)).

## 6. The first five minutes

Today the image shows KDE's setup wizard on Fedora's wallpaper with "Welcome
to Plasma Desktop" and "Powered by Fedora Linux", and then the Plasma login
screen ([TUR-003 report](../reports/TUR-003.md)). This section replaces that
sequence, screen by screen.

### 6.1 Starting

**The boot splash** is a Plymouth theme, `tur`, for the `two-step` module:
an INI file and images, no script [P8].

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│                                                                              │
│                                    ❂                                         │
│                              (the lotus mark)                                │
│                                                                              │
│                                    ◌                                         │
│                          Starting the desktop                                │
│                                                                              │
│                                                                              │
│                                                        Esc: show details     │
└──────────────────────────────────────────────────────────────────────────────┘
```

- Black ground, so that there is no flash between the firmware and the splash.
  The firmware's own logo is not kept (`UseFirmwareBackground=false`): the
  first thing a person sees after the manufacturer's screen is TUR's mark.
- The lotus mark alone, without the wordmark, at a size that is sharp on a
  1024 px wide screen and on a 4K one (supplied at two sizes).
- A spinner and one line of text: the current stage, in words. **No
  percentage and no bar** (§27.1).
- Esc switches to the text view, which is Plymouth's own behaviour.

The line of text is sent by the boot-stage reporter (a system service outside
this document) with Plymouth's message interface. The words for each stage of
§27.1:

| Stages | Shown |
|---|---|
| `BOOT-030` to `BOOT-040`, `BOOT-060`, `BOOT-070` | Starting TUR |
| `BOOT-050` | _(the passphrase prompt)_ Unlock your disk to continue |
| `BOOT-080` | Starting system services |
| `BOOT-090` | Finding devices |
| `BOOT-100` | Starting graphics |
| `BOOT-110`, `BOOT-120` | Starting the desktop |
| Diagnostic mode | The stage identifier, the words and the time: `BOOT-090 Finding devices · 4.2 s` |
| The health gate gave up on a new version | The new version did not start properly. Returning to the previous version. |

That the `two-step` module shows messages sent this way in the position drawn
is an **assumption to verify** in the boot test, which today takes its first
screenshot only at the login screen and will take one during start-up.

The theme is copied into the initramfs, so changing it changes a boot-critical
file: the image build regenerates the initramfs [P8], and the change is
accepted only when the boot test passes (AGENTS.md, priority 1). If the theme
cannot be loaded, Plymouth falls back to text, and the machine still starts.

**The start-up menu** is GRUB's text menu, hidden unless the previous start
failed or a key is held. Its entries are titled by the system's name and
version, which follow from the release package ([section 12](#12-replacing-fedoras-marks)).
It is not themed: a text menu is the most robust thing to show when something
is wrong.

### 6.2 First-run setup

`tur-setup` replaces `plasma-setup`.

**Why replace and not reskin.** KDE's wizard has a hard-coded title and
wallpaper, shows the system's name after "Powered by", and can be extended
only by QML pages that run inside it, which would be GPL-side code [P9]. Its
page order and wording cannot be changed by configuration. A standalone
program is also smaller: `plasma-setup` logs a system user in to a complete
Plasma session to show one window.

**How it runs** (the pattern is `plasma-setup`'s [P9]; the details are
**assumptions to verify** in task D-06):

- A system unit runs before the login manager when `/etc/tur-setup-done` does
  not exist and the machine was not started from a live medium.
- It logs a dedicated system account in, once, to a session that consists of
  KWin and `tur-setup` and nothing else (`kwin_wayland --exit-with-session`,
  with `--no-lockscreen` and `--no-global-shortcuts` [K12]). No panel, no
  launcher, no way out except the buttons on screen.
- A polkit rule lets that account, and only it, set the locale and keyboard,
  the time zone and the host name, connect to a network, and create the first
  user.
- `plasma-setup` is removed from the image, or its unit is left disabled; the
  boot test's flag file changes name accordingly.
- When it finishes it writes the flag, removes its automatic login, and the
  login manager starts normally.

**The screens.** Every screen has the same frame: the faint lotus curve on the
dark ground, the step's title, "Back" and a primary button at the bottom
right, and at the bottom left an **Accessibility** button and a power button
(Restart, Shut down). The accessibility menu is available from the first
screen and offers: screen reader, larger text, high contrast, reduce motion,
on-screen keyboard. Choices made there carry into the user's account.

**Screen 1. Language.**

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│                           Welcome to TUR                                     │
│                                                                              │
│             Choose your language to begin.                                   │
│                                                                              │
│             [ Search languages                           ]                   │
│             ┌──────────────────────────────────────────────┐                 │
│             │ English (United Kingdom)                   ✓ │                 │
│             │ English (United States)                      │                 │
│             │ Deutsch                                      │                 │
│             │ Español                                      │                 │
│             │ Français                                     │                 │
│             │ Italiano                                     │                 │
│             │ …                                            │                 │
│             └──────────────────────────────────────────────┘                 │
│                                                                              │
│  [Accessibility]  [Power]                                      [ Continue ]  │
│                                                TUR 0.4.0 · build 20261112    │
└──────────────────────────────────────────────────────────────────────────────┘
```

Each language is written in itself. The screen changes language as soon as one
is chosen. The build line is small and is there for the person on the other
end of a support request.

**Screen 2. Keyboard.**

> **Your keyboard**
> Choose the layout that matches the keys in front of you.
> [list of layouts for the chosen language, then all others]
> Try it here: [                              ]
> [Back]   [Continue]

**Screen 3. Network.** Optional, and says so.

> **Connect to a network**
> TUR works without a network. Connect now if you want updates and the
> application centre to be ready when you reach the desktop.
> [list of wireless networks; "Wired: connected" when it is]
> [Back]   [Skip for now]   [Continue]

**Screen 4. Time zone.**

> **Where are you?**
> TUR uses this for the clock only.
> [ Search for a city or region        ]
> Europe / Rome · the time there is now 10:42
> [Back]   [Continue]

The time zone is chosen by the user. It is not looked up from the network
address: that would contact a service nobody asked for (README, "Private by
default").

**Screen 5. Your account.**

```text
                           Your account

             Your name          [ Marta Kowalski                 ]
             User name          [ marta                          ]
                                Used for your home folder. Lower-case letters and digits.
             Password           [ ••••••••••                     ] [Show]
             Password again     [ ••••••••••                     ]

             This account can change the system. TUR will ask for this password
             before anything that affects disks, start-up or security.

             ☐ Sign in without asking for the password
               Anyone who can reach this computer could then use it.

  [Accessibility]  [Power]                               [ Back ]   [ Continue ]
```

- The user name is suggested from the name and can be edited. Errors appear
  under the field as the user types, in words: "That user name is already
  used by the system."
- There is no password-strength meter and no composition rule beyond a
  minimum length, which is stated before it is violated. A weak password gets
  one sentence, not a refusal: "This password is short. Anyone who guesses it
  can change the system."
- The first account is an administrator. How the account is created
  (`useradd`, as KDE's wizard does, or the accounts service) is a choice for
  the implementation task; the encryption of the home folder is not decided
  here.

**Screen 6. Privacy.** One screen that asks nothing. It states the position
the rest of the system keeps.

> **Private by default**
> - TUR does not collect information about you or how you use this computer.
> - There is no TUR account, and nothing to sign up for.
> - If something goes wrong, TUR can write a report for you. It stays on this
>   computer until you decide to give it to someone.
> - P@x, the assistant, is off. If you turn it on, TUR shows you exactly what
>   is sent to the AI service you choose.
>
> Applications you install have their own rules. The application centre tells
> you what each one can reach.
> [Back]   [Continue]

This screen is a promise, and [section 12.3](#123-network-contact-nobody-asked-for)
lists what must be true in the image for it to be an honest one.

**Screen 7. Appearance.**

> **Dark or light?**
> ◉ Dark   ○ Light   ○ Light by day, dark at night
> [two small previews of the desktop]
> You can change this at any time in Settings.
> [Back]   [Finish]

**Finishing.** "Setting up your account…" with a spinner and the activity in
words, then the session ends and the user is signed in to their new account
once, without being asked for the password they typed a minute ago.

A failure at any step is a two-layer message with a `TUR-SETUP-*` code (a new
family) and "Try again"; the wizard never leaves the machine without an
account and without a way to try again.

### 6.3 Signing in

From the second start the user meets Plasma Login Manager. TUR configures it
and adds no code: it does not accept themes [P7].

- It applies TUR's global theme, so its colours and font are TUR's.
- Its wallpaper is TUR's, set in `/usr/lib/plasmalogin/plasmalogin.conf.d/`
  with the key upstream reads (`[Greeter][Wallpaper][org.kde.image][General]
  Image=`; Fedora's file sets a key that upstream does not read [P7]).
- The lock screen gets the same wallpaper through `kscreenlockerrc`.

The words on these two screens are KDE's.

### 6.4 The first desktop

Plasma's splash screen is turned off (a setting in the global theme's
defaults). A splash would either show KDE's artwork or need a QML file inside
a GPL program; and the honest signal that the desktop is ready is the desktop.

What the user sees: TUR's wallpaper, one panel along the bottom
([section 7.1](#71-panel)), and one window in the middle of the screen,
`tur-welcome`. KDE's welcome window is suppressed by configuration [P10].

```text
┌ Welcome ─────────────────────────────────────────────────────────────── ✕ ┐
│                                                                           │
│  This is your desktop, Marta.                                             │
│                                                                           │
│  ┌───────────────────────┐ ┌───────────────────────┐ ┌──────────────────┐ │
│  │ Find applications     │ │ Meet P@x              │ │ Make it yours    │ │
│  │                       │ │                       │ │                  │ │
│  │ Linux, Windows and    │ │ TUR's assistant can   │ │ Displays, sound, │ │
│  │ Android applications  │ │ do things for you and │ │ printers and the │ │
│  │ are in one place.     │ │ show you how. It is   │ │ rest are in      │ │
│  │ TUR tells you what is │ │ off until you turn it │ │ Settings.        │ │
│  │ known about each one  │ │ on, and it needs a    │ │                  │ │
│  │ before you install.   │ │ key from an AI        │ │                  │ │
│  │                       │ │ service you choose.   │ │                  │ │
│  │ [Open the application │ │ [Set up P@x]          │ │ [Open Settings]  │ │
│  │  centre]              │ │ [Not now]             │ │                  │ │
│  └───────────────────────┘ └───────────────────────┘ └──────────────────┘ │
│                                                                           │
│  Windows was found on this computer. Its files are in Files, under        │
│  "Windows (C:)". TUR opens them without changing them.          [Show me] │
│                                                                           │
│  You can open this window again from the launcher: Welcome.               │
└───────────────────────────────────────────────────────────────────────────┘
```

- Three cards, no pages, no carousel, no "next". Closing the window is always
  right.
- The line about Windows appears only when the storage service reports
  another operating system's volume. A matching line appears when the
  first-start self-test (§6.6) found something the user should know: "One
  thing needs your attention: the wireless network device has no driver.
  [Open Devices]". The welcome window is the first place the system keeps its
  promise to explain itself.
- The window opens once, at the first login of each new account, and never
  again by itself.

### 6.5 The offer to meet P@x

"Meet P@x" on the welcome window is the only offer TUR makes. "Not now" closes
the card for good; P@x remains reachable from its tray item and from Settings.
"Set up P@x" starts the flow of [section 9.12](#912-setting-up-px), whose
first screen carries the text:

> **Meet P@x**
> P@x is the assistant built into TUR. You can ask it, by typing or speaking,
> to do things on this computer, to fix problems, or to show you how to do
> something yourself.
> P@x works through an AI service that you choose and pay for directly. TUR
> does not run one and does not see what you ask.
> You stay in charge: things that can be undone, P@x does. Before sending,
> buying, deleting or installing, it asks. Disks, start-up and security it can
> never change without you approving on a screen it cannot reach.
> [Not now]   [Continue]

If the user leaves the setup part-way, nothing is enabled and nothing is
stored except, if they reached that step, the key in the password store, which
"Not now" offers to remove.

## 7. The desktop layout

The desktop is Plasma's, arranged once and well. TUR adds four things to it,
none of them a plugin: a tray item, a search provider, a set of desktop files,
and P@x's overlay.

### 7.1 Panel

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│                              (TUR's wallpaper)                               │
│                                                                              │
│                                                                              │
│ ╭──────────────────────────────────────────────────────────────────────────╮ │
│ │ ❂ │ ▣ Applications  ▣ Files  ▣ Web  ▣ Settings │ ▣ ▣ …  │ ▴ ♪ ⌁ P │ 10:42│ │
│ ╰──────────────────────────────────────────────────────────────────────────╯ │
└──────────────────────────────────────────────────────────────────────────────┘
   launcher   pinned applications                  running    tray: hidden     clock
                                                              items, sound,
                                                              network, P@x
```

One panel, at the bottom, floating, as Plasma draws it. From left to right:
the launcher with the lotus mark; four pinned applications (the application
centre, Files, the web browser, Settings); running applications; the system
tray; the clock. Nothing else.

Design judgements: the bottom panel is kept because it is what people coming
from Windows and from other Plasma systems expect, and "one polished default"
means choosing the familiar arrangement and making it good. Discover is
unpinned, because the application centre is the one place for software.
Virtual-desktop and activity switchers are not on the panel by default; they
remain in Plasma for those who want them.

**How it is set.** The shell does not read a system-wide applets file, so
there are two routes [P3]. TUR uses the data route:

- `/etc/skel/.config/plasma-org.kde.plasma.desktop-appletsrc`, a complete
  layout for a new account: the panel, its applets in order, the pinned
  applications, the wallpaper plugin. KDE's own setup wizard configures its
  session's panel the same way [P3].
- The launcher's icon is not set in that file at all. The launcher asks the
  icon theme for `start-here-kde-symbolic` [P3], and TUR's icon theme supplies
  the lotus mark under that name.

That a hand-written applets file gives exactly this panel on a new account,
and survives a Plasma update, is an **assumption** that the end-to-end test
checks on every image build. If it fails, the reserve is a layout script in
the global theme ([section 4](#4-the-licence-boundary-on-the-desktop)).
Existing accounts are never re-laid-out by an update: a person's panel is
theirs.

### 7.2 Launcher

The launcher is Plasma's Kickoff: favourites, all applications by category,
power controls, and a search field that is focused as soon as the menu opens.

- **Every application is in it, under its own name and icon**, whatever it
  runs as. The entries are written by the service that hosts the application:
  the desktop bridge (§9.3), Waydroid for Android
  ([android-runtime.md](android-runtime.md), 7.1), `tur-guest` for
  applications inside Windows ([windows-guest.md](windows-guest.md), 6.7).
- **An application from another system says so, quietly.** Its name and icon
  come from software TUR does not control, and the guest design requires
  every such entry to be marked, so that nothing inside Windows can pass
  itself off as, say, the web browser. Two markings, both made of data in the
  desktop entry, so no code runs in the launcher:
  - the entry's description line, which Kickoff shows under the name, reads
    "Windows application" or "Android application";
  - a small neutral corner mark is drawn into the icon by the host when it
    re-encodes the icon: a square for Windows, a rounded rectangle for
    Android. Not a vendor's logo ([5.4](#54-shape-space-and-icons)).

  The same icon is used in the panel and the task switcher, so the mark
  travels with the window. How the application runs in detail is on its page
  in the application centre and in "About this application" (7.3). That
  Kickoff shows the description line in its default view is an **assumption
  to verify**; the corner mark does not depend on it.
- **Favourites** for a new account: the application centre, Files, the web
  browser, Settings, Welcome.
- **Search finds more than what is installed.** `tur-search` is a search
  provider: a small program, started on demand over D-Bus, that implements
  the `org.kde.krunner1` interface and is announced by a desktop file. Kickoff
  and KRunner (`Alt+Space`) both use it, with no code inside either [P12].
  It answers with:

  | Typed | Result | Opens |
  |---|---|---|
  | The name of an application that is not installed, or another name people use for it | "Microsoft 365 · See it in the application centre" | The application's page |
  | A setting of TUR's | "Private mode · Settings ▸ P@x ▸ Privacy" | That row, highlighted ([section 10](#10-settings)) |
  | A `TUR-…` error code | "TUR-STORAGE-NTFS-004 · The Windows volume was not mounted for writing" | The code's explanation |
  | A sentence ending in "?" or starting with "ask" (only when P@x is on) | "Ask P@x: why is the fan so loud?" | The ask bar, with the question |

  Results from `tur-search` come after installed applications. It answers
  from local data only and never contacts a store while the user types.

### 7.3 Windows and task switching

Plain Plasma, with these defaults set by configuration:

| Behaviour | Setting |
|---|---|
| `Alt+Tab` | Thumbnail grid of windows, most recently used first |
| `Meta+W` | Overview of all windows, with search |
| `Meta` | Opens the launcher |
| Dragging a window to an edge | Tiles it to a half or a quarter |
| Window buttons | Minimise, maximise, close, on the right |
| Decoration | Breeze, coloured by TUR's scheme |

TUR adds nothing to window management. What it must get right is identity:

- A window from any ecosystem appears in the panel, in `Alt+Tab` and in the
  overview with **its application's name and icon**, never as "Wine" or
  "Waydroid", and windows of one application group together. This depends on
  each runtime giving its windows a stable application identifier that matches
  a desktop file ([android-runtime.md](android-runtime.md) 7.1 marks the exact
  identifier as an assumption).
- Right-clicking an application's panel icon offers, besides Plasma's own
  entries, **"About this application"**: an action in the application's
  desktop file, written by whichever service wrote the entry (it already
  records the runtime there, as `X-TUR-Runtime`), that opens its page in the
  application centre. That page says how it runs, which version of which runtime it uses,
  where its data is and what it can reach. This is how TUR meets §9.5's
  requirement to show the runtime environment "without cluttering every
  window".
- Applications that cannot resize (many phone applications) open at their
  natural size and KWin does not offer to maximise them. Whether KWin honours
  this for every runtime is to be checked by the bridge's tests.
- Windows of applications inside the Windows guest are ordinary windows to
  KWin: they tile, switch and move between virtual desktops like any other.
  The guest design lists the rough edges to expect and measure (menus and
  tooltips that appear misplaced, one soft-looking screen when two screens
  have different scale factors, no drag and drop between a guest window and
  a host window). The desktop does not hide them; the application's page
  states them as caveats. A guest application's own tray icon is not shown,
  and its notifications are not forwarded at first: the application is
  reached again through its launcher entry.

### 7.4 Notifications

TUR's programs use the desktop notification service; Plasma shows the
notifications, keeps the history and applies Do Not Disturb [P12]. TUR adds no
notification surface of its own.

| From | When | Priority | Stays until dismissed |
|---|---|---|---|
| TUR | An update is ready | Normal | No; once a day while pending |
| TUR | The system went back to the previous version | **High** | **Yes** |
| TUR | A disk was opened read-only for a safety reason | Normal | No |
| TUR | A device needs attention | Low | No |
| TUR | A support bundle is ready | Normal | No |
| TUR | Windows needs you: a prompt on the Windows screen, or a restart to finish updating | Normal | Yes |
| TUR | Windows could not be woken as it was left and was started afresh (`TUR-WINDOWS-GUEST-017`) | Normal | No |
| P@x | P@x is waiting for you (an approval or a question) | Normal | Yes |
| P@x | A task finished, or was stopped by a limit | Low | No |
| P@x | While you were away: something was done or deferred | Normal | Yes |
| P@x | Continue on the trusted screen | **High** | Yes |

Rules: a notification says what happened in its title and offers at most three
actions, each a verb. High priority is used only when the user's data or
approval is at stake, so that it keeps its meaning. Each notification names
its sender ("TUR" or "P@x") through its desktop entry, so the user can silence
either in Plasma's notification settings; the two high-priority ones above
are exempt from Do Not Disturb. P@x never sends a notification to bring
itself to mind.

### 7.5 System tray

Plasma's tray, with Plasma's items. TUR's programs add items through the
StatusNotifierItem interface [P12], and each exists only while it has
something to say:

| Item | Present | Shows |
|---|---|---|
| **P@x** | Always, unless the user hides it | P@x's state ([9.3](#93-the-activity-indicator)). Click: the conversation. Menu: Pause, Private mode, Settings |
| **Updates** | While an update is downloading, ready, or was rolled back | The state in one line. Click: Settings ▸ Updates |
| **Windows** | While the Windows guest holds memory: running or paused. Gone once it is saved to disk or shut down | Running: "Windows is running · 2 applications · using 6.2 GB of memory". Paused: "Windows is resting · no window open · holding 6.2 GB of memory · will be put away in 8 minutes". Menu: Keep Windows ready; Free the memory now; Show the Windows screen; Shut Windows down; Settings |
| **Android** | While the Android system is running | "Android is running · 1 application · using 1.4 GB of memory". Menu: Stop Android; Settings |

The last two exist so that the cost of a background system is never hidden
(README: "Windows, Android and the assistant cost nothing until you use them,
and step back when you stop"). The Windows item's three states are the guest
design's running, paused and saved
([windows-guest.md](windows-guest.md), 7.4), in words: running, resting, put
away. "Keep Windows ready" carries its consequence where it is switched:
"Windows stays in memory so that its applications open at once and keep
working in the background. Uses memory all the time." Figures in the item are
read from the service; the ones above are placeholders.

The tray's P@x item is a convenience, not the activity indicator. A tray item
can be hidden; the indicator of [9.3](#93-the-activity-indicator) cannot.

### 7.6 Configuration and additions

**Plain Plasma configuration (side B: data).**

| What | File |
|---|---|
| Global themes "TUR Dark" and "TUR Light": colours, icons, cursor, fonts, wallpaper, decoration, no splash | `/usr/share/plasma/look-and-feel/org.tur.dark/`, `org.tur.light/` (`metadata.json`, `contents/defaults`) |
| Colour schemes | `/usr/share/color-schemes/TurDark.colors`, `TurLight.colors` |
| Icon theme: the lotus mark as the launcher and system logo; TUR's own application icons | `/usr/share/icons/tur/`, `tur-dark/` (inheriting Breeze) and `hicolor` |
| Wallpaper, with a light and a dark image | `/usr/share/wallpapers/Tur/` |
| Default global theme, fonts, task-switcher and window defaults, no KDE welcome window, the "About this system" logo and name | `kdeglobals`, `kwinrc`, `plasma-welcomerc`, `kcm-about-distrorc` and others, as system defaults |
| The panel for new accounts | `/etc/skel/.config/plasma-org.kde.plasma.desktop-appletsrc` |
| Rules for TUR's own windows (approval sheet above others and excluded from capture) | `/etc/xdg/kwinrulesrc`, marked immutable |
| Login and lock-screen wallpaper | `/usr/lib/plasmalogin/plasmalogin.conf.d/10-tur.conf`, `kscreenlockerrc` |
| Boot splash | `/usr/share/plymouth/themes/tur/` |
| Default shortcuts for TUR's programs | `X-KDE-Shortcuts=` in their desktop files |
| TUR Settings listed inside KDE System Settings | `/usr/share/plasma/systemsettings/externalmodules/org.tur.Settings.desktop` |
| "Ask P@x about this file" in the file manager | `/usr/share/kio/servicemenus/org.tur.Pax.Ask.desktop` |
| The search provider's announcement | `/usr/share/krunner/dbusplugins/org.tur.Search.desktop` |
| Handlers for `.exe`, `.msi`, `.apk` and package files | `MimeType=` in `org.tur.Centre.desktop` |

Where the system defaults live (`/etc/xdg`, or a directory under `/usr` added
to `XDG_CONFIG_DIRS` as Fedora's `kde-settings` does) is decided in task D-04
by what reaches all three kinds of session: the user's, the login screen's
and the setup wizard's. On an image-based system `/usr` is the better home,
because a file in `/etc` that the owner has edited stops receiving the
image's changes [P4].

**What TUR adds (side A: programs).**

| Addition | Program |
|---|---|
| The application centre | `tur-centre` |
| TUR's settings | `tur-settings` |
| First-run setup and the welcome window | `tur-setup`, `tur-welcome` |
| Notifications for updates, storage and devices; every tray item, P@x's included | `tur-notifier` |
| Launcher search | `tur-search` |
| Support bundles | `tur-diagnostics` |
| Everything of P@x that is visible | `tur-pax-ui`, `tur-pax-overlay`, `tur-pax-approve`, `tur-confirm` |

No Plasma applet, look-and-feel script, KWin script or effect, KRunner plugin
or System Settings module is among them.

## 8. The application centre

`tur-centre` is the one place to find, install, update and remove applications
from every ecosystem ([ADR-0011](../adr/0011-application-centre-and-stores.md)).
It is a standalone program. It holds no routing logic and writes no verdict of
its own: it shows what the catalogue and the router say
([catalogue-and-routing.md](catalogue-and-routing.md),
[ADR-0007](../adr/0007-app-centric-routing.md)) and asks other services to act.
Discover is removed from the default panel and launcher favourites but stays
installed until `tur-centre` covers Flatpak and system updates (task D-11).

What exists today is the catalogue (32 seed entries), the router and their
command-line tool. Store adapters, installation, a D-Bus service and TUR's own
compatibility tests do not exist yet. The design below is complete; section
8.10 says what each part waits for.

### 8.1 Information architecture

```text
Application centre
├─ Home          a short, honest front page: what needs attention, "Bring an
│                application you already have", where the ratings come from
├─ Search        one field, always visible; every source at once
├─ Categories    the catalogue's ten categories
├─ Installed     everything installed, whatever it runs as
├─ Updates       application updates; the system update is shown, not managed
├─ Sources       the stores and repositories TUR searches; opt-in stores;
│                setting up Windows and Android
└─ Open a file   an installer or package the user already has
```

The window has a side list (the seven entries above), a search field in the
header that is focused when the window opens, and one content area. There are no
tabs inside tabs. Every view is reachable with the keyboard alone, and each view
and each application page has an address (`tur-centre:app/<id>`,
`tur-centre:search?q=…`, `tur-centre:sources/google-play`) so that a
notification, the launcher or P@x can open it directly. `<id>` is the
catalogue's application identifier, for example `com.microsoft.office`.

The Home page carries one standing sentence for as long as it is true: "TUR
has not tested applications itself yet. Every rating here is what the named
source reports."

### 8.2 Words the application centre uses

These terms are fixed. They appear in the same form in search results, on the
application page, in the "open a file" sheet, in P@x's approval card and in the
published compatibility matrix. Each maps one-to-one onto a value of the
catalogue or the router, so that no meaning is added or lost on the way to the
screen.

**How an application runs** (the router's `route`). The label never names an
engine unless the engine is something the user may already know.

| Route | Label | One-line explanation shown under it |
|---|---|---|
| `flatpak`, `native` | Linux application | Made for Linux. Runs directly. |
| `distro-container` | Linux application, from _Ubuntu_ | Made for another Linux system. Runs in a small copy of that system. |
| `proton` | Windows game, through Proton | Runs without Windows, through Proton, the compatibility layer used by the Steam Deck. |
| `wine` | Windows application, in the compatibility layer | Runs without Windows. Fastest and lightest, but not every application works. |
| `windows-guest` | Windows application, in Windows | Runs inside a real Windows that TUR starts in the background (a virtual machine). Needs your own Windows licence and more memory. |
| `android-runtime` | Android application | Runs in the Android system. Android must be switched on. |
| `web` | Web application | Opens the service's website in its own window. |
| `darling` | macOS application (experimental) | An experiment. Most macOS applications do not run. |
| No route | Cannot run on TUR | See [8.8](#88-when-there-is-no-good-route). |

**How well it works** (the catalogue's `rating`). Five values. No stars, no
percentages and no medals: a rating that a store supplies for an application's
quality is a different thing and is labelled with the store's name.

| Rating | Label | Icon (never colour alone) |
|---|---|---|
| `good` | Works well | Tick in a circle |
| `usable` | Works, with minor problems | Tick with a small exclamation mark |
| `poor` | Works badly | Exclamation mark in a triangle |
| `broken` | Does not work | Cross in a circle |
| `unknown` | No reports | Question mark in a dashed circle |

**According to whom** (the record's `reported_by`). Every rating is shown with
its source, the source's own word, and the date. The application centre never
shows a rating without them, which is the catalogue's honesty rule carried to
the screen.

| `reported_by` | Wording |
|---|---|
| `external` | Reported by _WineHQ AppDB_ as "_Silver_" · version _CC 2024_ · checked _12 November 2026_ · Not tested by TUR |
| `tur-test` | Tested by TUR on _date_ with version _x_ · See the report |
| `nobody` | Nobody has reported on this way of running it. It may still work. |

When a route has several records (Elden Ring through Proton has one from
ProtonDB and one from Valve's Steam Deck review), the page shows the rating the
router returns for the option and "All reports (2)", which lists every record
with its source. A source's rating is evidence from its contributors'
machines, not a promise, and the "All reports" list says so in one line.

**What stands in the way** (the option's `outlook` and its caveats).

| Router value | Shown as |
|---|---|
| Outlook `clear` | Nothing extra. `clear` is not a promise; the rating says what is known |
| Outlook `doubtful` | "May not work", with the reason the router gives |
| Outlook `blocked` | "Cannot be used on this computer", greyed, last in the list, not selectable, with the caveat that says why |
| Caveat, `blocker` | Cross icon; always shown, first |
| Caveat, `obstacle` | Triangle icon; "probably will not work fully" |
| Caveat, `warning` | Triangle icon |
| Caveat, `note` | Information icon; under "More" when there are more than four caveats |
| Prerequisite | "Needed first", with what must be installed, switched on or supplied |
| `recommended` | The option shown in "How it will run"; marked "Recommended" in the list of ways |

The reason, every caveat and every prerequisite are the router's text. The
application centre arranges them and writes none.

### 8.3 Search

One field. Typing searches the catalogue on the machine at once (the
catalogue's search forgives typing mistakes and knows other names: "MS Office"
finds Microsoft 365) and each enabled store as it answers. Results never wait
for the slowest source.

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ☰  Application centre        [ office                               ⌕ ]      │
├───────────────┬──────────────────────────────────────────────────────────────┤
│ Home          │ 2 applications in TUR's catalogue for "office"               │
│ Search      ● │ Show: [All] [Linux] [Windows] [Android] [Web]                │
│ Categories    │                                                              │
│ Installed     │ ▣ Microsoft 365                                   [Details]  │
│ Updates    3  │   Microsoft · Office suite: Word, Excel, PowerPoint and      │
│ Sources       │   others.                                                    │
│ Open a file   │   Windows application, in Windows · ? No reports             │
│               │   Needed first: Windows set up; your own Windows licence     │
│               │ ──────────────────────────────────────────────────────────── │
│               │ ▣ LibreOffice                              Installed  [Open] │
│               │   The Document Foundation · Office suite.                    │
│               │   Linux application · ? No reports                           │
│               │ ──────────────────────────────────────────────────────────── │
│               │ Also found in Flathub, not in TUR's catalogue              ▸ │
│               │                                                              │
│               │ Still searching: Steam                                       │
└───────────────┴──────────────────────────────────────────────────────────────┘
```

(Both rows are seed entries. The route and rating on each row are those of the
option the router recommends for this machine; the drawing assumes a machine
with enough memory and no Windows set up.)

Rules:

- **One entry per application** (ADR-0011, rule 3). An application that exists
  as a Linux, a Windows and an Android edition is one row, showing the route
  the router recommends. The other ways are on its page.
- **Order is the catalogue's**: by how well the name matches, then by name, so
  the same search always gives the same list. An installed application is
  marked, not moved. An application that does not work is still listed, so
  that the user learns it from TUR and not after an hour of trying.
- **Classes.** Some entries stand for a class of applications that share an
  answer. Searching "banking app" finds "Banking and payment applications for
  Android", whose page explains why they cannot run
  ([8.8](#88-when-there-is-no-good-route)). An application with an entry of
  its own takes precedence over its class.
- **Results from stores** that are not in the catalogue are listed separately,
  under the store's name, with "No reports": the catalogue knows nothing about
  them, and the application centre does not pretend to.
- **A store that is not added** still contributes what the catalogue knows.
  The row says where the application is and offers the opt-in. Nothing is ever
  installed from a store the user has not added.
- **A store that fails** produces one quiet line under the results, not a
  dialog: "Steam did not answer. Results from Steam are missing. Try again".
- **Offline:** "You are offline. Showing the applications TUR already knows
  about." The catalogue is on the machine, so search still works.
- **Nothing found:**

  > **Nothing found for "photothing".**
  > If you have its installer or package, TUR can look at the file and tell you
  > whether it will run.
  > [Open a file…]   [Tell the project you need it]

  "Tell the project you need it" opens the project's issue page in the browser.
  Nothing is sent by the application centre itself.

The same search is offered in the launcher through a search provider
([section 7.2](#72-launcher)); a result there opens the application's page.

### 8.4 An application's page

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ← Results                    [ office                               ⌕ ]      │
├──────────────────────────────────────────────────────────────────────────────┤
│  ▣▣   Microsoft 365                                      [   Set up…    ]    │
│  ▣▣   Microsoft · Productivity                                               │
│                                                                              │
│ ┌ How it will run ─────────────────────────────────────────────────────────┐ │
│ │ Windows application, in Windows             Recommended      [Change…]   │ │
│ │ Runs inside a real Windows that TUR starts in the background (a virtual  │ │
│ │ machine). Needs your own Windows licence and more memory.                │ │
│ │                                                                          │ │
│ │ ? No reports                                                             │ │
│ │ Nobody has reported on this way of running it. It may still work.        │ │
│ │                                                                          │ │
│ │ Needed first                                                             │ │
│ │  • Windows set up on this computer                        [Set up…]      │ │
│ │  • Your own Windows licence                                              │ │
│ │                                                                          │ │
│ │ Before you install                                                       │ │
│ │  (the router's caveats: the memory a Windows guest needs, compared with  │ │
│ │   this computer's; that an account with the publisher is needed; …)      │ │
│ │  • TUR does not supply this application. You get it from Microsoft,      │ │
│ │    with your own account.                                                │ │
│ └──────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│ Other ways to run it                                                         │
│  ○ Windows application, in the compatibility layer · May not work            │
│    ✕ Does not work · Reported by WineHQ AppDB as "Garbage" · version         │
│      365 Business · checked 12 November 2026 · Not tested by TUR             │
│  ○ Web application · ? No reports                                            │
│    Opens office.com in its own window.                                       │
│                                                                              │
│ About this application …                                                     │
│                                                                              │
│ Details ▸  Publisher's website · Editions · Licence · Where each fact was    │
│            checked · What it can reach on this computer · Where its data is  │
└──────────────────────────────────────────────────────────────────────────────┘
```

(Microsoft 365 as the seed catalogue records it: the compatibility layer is
rated `broken` by one external source, so the router lists it first, marks it
doubtful, and recommends the Windows guest after it; the web edition is listed
because nothing lighter is clear.)

The "How it will run" box is the centre of the page and has a fixed order:
route, one-line explanation, rating with its source, what is needed first, then
the caveats. It is above any screenshots and the description, because it is
what a store on another system cannot tell the user.

- **The route** is the option the router marks `recommended`. "Change…" and the
  list of other ways show every option the router returned, in the router's
  order, each with its own rating, outlook, caveats and prerequisites. Choosing
  one pins it for this application ("Always run it this way"); the router then
  recommends the pinned route whenever it can work, and says so when it cannot.
- **A compatibility layer is always listed before Windows itself**, even when
  the layer is rated as not working and Windows is recommended. That is the
  router's rule, kept on screen, because the layer costs nothing to try.
- **"Before you install"** lists the option's caveats, blockers first. If there
  are none the box says "Nothing known stands in the way." and never invents
  reassurance.
- **The primary button** says what will happen next: "Install" when nothing is
  needed first; "Set up…" when a prerequisite is missing; "Open the publisher's
  page" when the user must fetch the installer; "Open in _Steam_" for a store
  that sells it; nothing at all when there is no route.
- **"Where each fact was checked"** lists the entry's references with their
  dates. Anyone can see what a claim rests on.
- **Once installed** the button becomes "Open", with "Remove" and "Repair" in a
  menu, and the page gains "On this computer": installed version, the route in
  use, the runtime version pinned for it (§15.2), where its data is, how much
  space it uses.
- **What the application can reach** is stated for every route in the same
  words: files (which folders), devices (camera, microphone, USB), network. For
  a Flatpak this comes from its permissions; for a Windows application from its
  isolation profile or the folders shared with Windows; for Android from its
  granted permissions.

### 8.5 Installing and updating

Pressing "Install" starts at once when nothing else is needed. Progress replaces
the button; the page stays usable.

```text
Downloading · 310 MB of 1.1 GB                       [Cancel]
▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░
```

- A bar with numbers is shown only when the total is known. Otherwise the line
  is "Downloading…" or "Setting up the Windows compatibility layer…" with an
  indeterminate indicator and the current activity (§6.1: no invented
  percentages).
- Cancelling is offered while it is safe and says what it leaves behind:
  "Cancelled. Nothing was installed."
- A failure is a two-layer message ([section 5.6](#56-tone-of-voice)) with the
  `TUR-*` code, and the page returns to its state before the attempt.

When something must happen first, one sheet says so before anything is
downloaded. Examples of the complete text:

> **This needs Android to be switched on**
> Android applications run in an Android system that TUR keeps switched off
> until you want it. Switching it on downloads the Android system. It uses
> memory only while an Android application is open.
> Some Android applications will not run on any computer: most banking and
> payment applications, and those built only for phone processors.
> [Cancel]   [Switch on Android and install]

> **You get this application from its publisher**
> TUR does not supply Adobe Photoshop. Download the installer from Adobe's
> page, then choose the file here. You need your own licence.
> [Open Adobe's page]   [Choose the installer…]   [Cancel]

> **This is sold by Steam**
> Buying and downloading happen in Steam, with your Steam account. TUR does not
> see your payment details.
> [Open in Steam]   [Cancel]

> **The compatibility layer for games is fetched from its publisher**
> Windows games run through Proton. TUR does not include Proton; your computer
> downloads it from its publisher the first time it is needed.
> [Cancel]   [Download and continue]

The publisher's page is the `download_page` of the catalogue entry, a page for
a person to read. TUR never fetches an installer the user did not choose.
Sizes come from the source at the time; a size that is not known is not stated.

For an application that runs in Windows, installing means one of three things,
and the sheet says which
([windows-guest.md](windows-guest.md), section 9.1): the installer the user
chose is started inside Windows and its window appears on the desktop; or
Windows' own package manager installs it, after that source's terms are shown;
or the publisher's sign-in page opens in a Windows browser window, where the
user signs in. The sign-in is always the user's.

**Updates.** The Updates view lists application updates grouped by source, with
"Update all" and a per-application "Update". Above the list, one card reports
the operating system: "TUR 0.4.0 is ready. Restart to use it. [Open Updates in
Settings]". The system update is controlled only in Settings
([section 11.1](#111-updates-and-rollback)), so that there is one owner.
Applications inside Windows update themselves, or through Windows; the view
says so and does not list them as TUR's to update.

A compatibility-layer update is offered per application, never applied to every
application silently (§15.2):

> **A newer compatibility layer is available for 3 applications**
> They work today with the version they have. You can move them one at a time
> and go back if something stops working.
> [Choose applications…]   [Not now]

**Removing.** "Remove" asks one question, with neither button focused by
default:

> **Remove Adobe Photoshop?**
> ◉ Keep its settings and documents
> ○ Remove its settings and data too (1.4 GB)
> [Cancel]   [Remove]

### 8.6 Sources, and adding a store that belongs to someone else

The Sources view lists every source with its state and, for a store with an
account, who is signed in.

```text
Sources
 On         Flathub                        Linux applications
 On         System packages                Part of TUR; updated with the system
 On         Other Linux systems            Packages for Ubuntu, Arch and others, in containers
 Not set up Steam                          Windows and Linux games          [Set up]
 Not set up Epic, GOG and Amazon           Through an open launcher         [Set up]
 Off        Android: open-source store     Needs Android switched on        [Switch on Android]
 Optional   Google Play                    Not part of TUR                  [Add…]
 ─────────────────────────────────────────────────────────────────────────────
 Ways of running applications
 Ready      Windows compatibility layer
 Not set up Windows (virtual machine)      Needs your Windows 11 Pro licence [Set up…]
 Off        Android                                                         [Switch on]
 ─────────────────────────────────────────────────────────────────────────────
 Cannot be added
            Microsoft Store, Apple App Store
            These stores do not allow another system to install from them.
```

A proprietary store is never in the image and never enabled by default
(ADR-0011, rule 1). Adding one is a guided flow with one decision in it. For
Google Play the screens are:

**1. What it is.**

> **Add Google Play**
> Google Play is Google's store for Android applications. It is not part of
> TUR. If you add it, TUR downloads an Android system that includes Google's
> services from its publisher, and you sign in with your own Google account.
>
> **Before you decide**
> - Applications that check for a certified Android device will still refuse to
>   run. That includes most banking and payment applications, and some games
>   and video services. Adding Google Play does not change this.
> - Google's services will run inside Android and exchange data with Google
>   under Google's privacy policy, not under TUR's.
> - You will register this computer with Google yourself, on Google's website,
>   with your account.
> - You can remove Google Play again at any time.
>
> [Cancel]   [Continue]

**2. Google's terms.**

> **Google's terms**
> Using Google Play means accepting Google's terms. TUR is not a party to them.
> Google Terms of Service · Google Play Terms of Service · Google Privacy
> Policy
> ☐ I have read and accept Google's terms.
> [Back]   [Download and add Google Play]

The button is disabled until the box is ticked. The links open in the browser.

**3. Download.** "Downloading the Android system with Google's services from
_publisher_ · 640 MB of 1.4 GB", then "Checking the download", then "Switching
Android to the new system". The image is fetched from its publisher at this
moment and checked against the pinned digest
([ADR-0006](../adr/0006-android-runtime.md)).

**4. Register this computer.**

> **Register this computer with Google**
> Google asks you to register a device that it has not certified. Copy the
> number below, open Google's registration page, sign in and paste it.
> `4f2a 9c1e 7b03 55d8`   [Copy]
> [Open Google's registration page]
> Registration can take some time to take effect. If Google Play says the
> device is not registered, wait and open it again.
> [I have registered it]

**5. Sign in.** The Play Store opens as an Android window and the user signs in
there. TUR never sees the password. The flow ends with: "Google Play is added.
Search in the application centre now includes it."

The registration procedure and what Google's terms permit on an uncertified
device are **assumptions to verify** before this flow is built (ADR-0011,
verification). If the verification fails, the flow is not shipped.

Through P@x the same flow is used: P@x can open it and explain each step, but
ticking the acceptance box and signing in are the user's acts. Adding a store is
class 2; accepting terms on the user's behalf is not something P@x does.

**Removing** Google Play (Sources ▸ Google Play ▸ Remove):

> **Remove Google Play?**
> TUR returns to the Android system without Google's services. Applications you
> installed from Google Play stay, but those that need Google's services may
> stop working.
> [Cancel]   [Remove Google Play]

### 8.7 "I have a file"

A person who already has an installer should not need to know what kind of file
it is. Four entry points lead to the same sheet: opening an `.exe`, `.msi`,
`.apk`, `.flatpakref`, `.AppImage`, `.rpm` or `.deb` in the file manager (the
application centre is registered as the handler for those types; this is a
desktop-file entry, not a plugin); dropping the file on the application centre's
window; the "Open a file" view; and asking P@x.

Nothing in the file is executed to produce the sheet. Two questions are asked
of two components: what the file is (inspection, in `tur-runtime`), and
whether the catalogue knows the application it belongs to. Where the catalogue
knows it, the catalogue's answer leads, as ADR-0007 requires.

```text
┌ SetupPhotoThing-2.6.exe ─────────────────────────────────────────────────────┐
│ This is the installer of a Windows application.                              │
│                                                                              │
│ What TUR knows about it                                                      │
│   TUR's catalogue does not know this application, so nobody's report is      │
│   available. It may work. If it does not, you can remove it without          │
│   affecting anything else.                                                   │
│                                                                              │
│ How TUR will run it                                                          │
│   ◉ In the compatibility layer   No Windows needed. Fastest.    Recommended  │
│   ○ In Windows                   A real Windows, in the background.          │
│                                  Not set up on this computer.                │
│                                                                              │
│ Where it came from                                                           │
│   Downloaded today from downloads.example.com.                               │
│   Signed by: Example Software Ltd.                                           │
│   Install only software you trust. TUR keeps this application's Windows      │
│   files separate from your other applications.                               │
│                                                                              │
│ Technical detail ▸                                    [Cancel]   [Install]   │
└──────────────────────────────────────────────────────────────────────────────┘
```

- When the catalogue does know the application, the second block is the "How
  it will run" box of 8.4 for it, with the rating and its source, and the
  choices are the router's options.
- When it does not, the compatibility layer is tried first. If the application
  then fails to start, or the user says it is unusable, the application centre
  offers Windows as the other way, with "not tested in Windows"
  ([windows-guest.md](windows-guest.md), section 5).
- "Where it came from" states only what is known: the download origin when the
  file manager or browser recorded one, and a signature when the file carries
  one that was checked. When neither is known it says "TUR does not know where
  this file came from."
- "Technical detail" shows the inspection result: format, architecture,
  subsystem, the reason codes of [runtime-manager.md](runtime-manager.md) and
  the `TUR-RUNTIME-*` code if any.
- For an `.apk` while Android is off, the primary button is "Switch on Android
  and install" and the sheet includes the text of the first sheet in 8.5.
- For a package of another Linux distribution (`.deb`, an Arch package), the
  sheet says "This package was made for _Debian or Ubuntu_. TUR will run it in a
  small copy of that system." and names the container it will use.
- A program that needs a Windows kernel driver gets the guest route with the
  statement that device drivers work only for devices handed to Windows.

### 8.8 When there is no good route

The router never returns an empty answer. When nothing can work it returns "no
route" first, with a reason that repeats each blocker and the least-bad
alternative, followed by the blocked routes. The application centre shows
exactly that, in a fixed pattern: what TUR cannot do, why in a sentence a
non-specialist can repeat, what the person can do instead, the evidence, and
the code.

```text
┌ Valorant ────────────────────────────────────────────────────────────────────┐
│ ✕ Cannot run on TUR                                                          │
│                                                                              │
│ This game needs anti-cheat software that works only inside Windows itself.   │
│ It will not start through Proton, and not in the Windows that TUR runs in    │
│ the background.                                                              │
│                                                                              │
│ What you can do                                                              │
│  • If Windows is installed on this computer beside TUR, start Windows to     │
│    play it.                                       [How to start Windows]     │
│  • Look for games like it that run on TUR.        [Search similar games]     │
│                                                                              │
│ Ways that were considered                                                    │
│  ✕ Windows game, through Proton · Cannot be used on this computer            │
│    Does not work · Reported by AreWeAntiCheatYet as "Denied" · checked       │
│    12 November 2026 · Not tested by TUR                                      │
│                                                                              │
│ Where each fact was checked ▸        TUR-DESKTOP-CATALOGUE-011 · Detail ▸    │
└──────────────────────────────────────────────────────────────────────────────┘
```

(Valorant as the seed catalogue records it.)

The cases the router distinguishes, with the first sentence each is given on
screen. The sentence comes from the router's reason; the table fixes the
plain-language form it must take.

| Case (router) | "Why" sentence | "What you can do" (the router's alternative) |
|---|---|---|
| Kernel anti-cheat | This game needs anti-cheat software that works only inside Windows itself. | Start Windows, if it is installed beside TUR (dual boot). Search similar games. |
| Play Integrity (for example the class "Banking and payment applications for Android") | This application checks that it is running on a phone certified by Google. No computer can pass that check. | Use it on a certified phone or tablet. Use the service's website. [Open the website] |
| ARM-only code, no translation | This application was built only for phone processors. TUR does not have, and may not ship, the software that would translate it. | Look for another edition. [Search the application centre] |
| macOS edition only | macOS applications run on Apple's computers. An experimental macOS layer exists in TUR and is switched off. | Look for its Windows or Linux edition. Use a Mac. [Switch on the experimental macOS layer] |
| This machine cannot run it (processor architecture; no virtualisation) | The reason, in the router's words, with the `TUR-WINDOWS-GUEST-*` code where the guest is what is missing. | The router's alternative. |
| A file that is not an application | TUR does not recognise this file as an application. | Technical detail says what the file is. |

Three neighbouring cases are not "no route" and must not look like it:

- **A route that is doubtful.** The page shows the rating with its source and
  "May not work". The button is "Install anyway", and the sheet says "If it
  does not work you can remove it without affecting anything else."
- **A route that works badly for a stated reason.** Protected video in an
  Android application (hardware-attested copy protection) is an obstacle, not
  a blocker: the page says the application will probably run at reduced
  quality or refuse, and points to the web edition, which the router lists
  beside it.
- **A game where only Windows itself would do, but nothing forbids trying.**
  Games are never offered the background Windows, because it has no usable
  graphics acceleration; the page says so in one line rather than leaving the
  user to wonder why that way is missing.

### 8.9 Setting up Windows and Android

Both are ways of running applications that cost something and are off until
the user wants them. Each has one guided flow, reached from Sources, from the
"Set up…" button of an application that needs it, or through P@x, which can
open the flow and point at each step but cannot take the steps marked as the
user's.

**Windows.** The flow is the eleven steps of
[windows-guest.md](windows-guest.md), section 7.2. The application centre owns
their presentation. The first page must let a person decide before anything is
downloaded:

> **Run Windows applications in a real Windows**
> Some Windows applications do not work in TUR's compatibility layer. For
> those, TUR can run a real Windows in the background and show its
> applications as ordinary windows. You will not see a Windows desktop.
>
> **What you need**
> - Your own licence for Windows 11 Pro. A Home licence cannot be used.
> - The Windows installation file, which you download from Microsoft.
> - _n_ GB of free disk space and _n_ GB of memory. This computer has _n_ and
>   _n_.
>
> **What to expect**
> - Office and business applications are what this runs well.
> - Games, video editing and anything that needs a graphics card run badly or
>   not at all.
> - Windows only works while one of its windows is open. With no window open
>   it is put to sleep, so mail does not arrive and nothing synchronises.
> - Windows sends data to Microsoft under Microsoft's privacy terms, not TUR's.
>
> Setting up takes some time, most of it unattended. You can remove Windows
> again at any time; your licence is not affected.
>
> _(When the application that started this has another way: "You can also use
> Microsoft 365 as a web application, without Windows. [Use the web
> application]")_
> [Cancel]   [Continue]

The sizes and the time are supplied by `tur-guest` from its checks of this
machine; none is written into the page, because every figure in the guest
design is a target not yet measured. The later steps follow the patterns of
this document: the host check is a list of two-layer results
(`TUR-WINDOWS-GUEST-001`, `-002`); the edition step refuses Home with
`TUR-WINDOWS-GUEST-005` and its reason; the licence step links Microsoft's
terms and has an unticked box, like Google's terms in 8.6; the installation
step shows progress by phase with no percentage, and offers "Show the Windows
screen" for the person who wants to watch. TUR never supplies a product key
and never presents skipping the key as a way to use Windows without a licence.

**Starting.** A Windows application is started like any other. If Windows is
asleep or stopped, a small card appears under the application's own name and
icon until its window opens:

```text
┌──────────────────────────────────────────────┐
│ ▣  Word                                      │
│    Waking Windows…                 [Cancel]  │
└──────────────────────────────────────────────┘
```

The line is "Waking Windows…" from sleep and "Starting Windows… This takes
longer the first time after the computer starts." from cold. The same card
serves Android ("Starting Android…"). It is a component of `tur-kit`, shown by
the service that is doing the starting.

**While it runs.** The tray item of [section 7.5](#75-system-tray) shows the
state and its cost. A Windows prompt that cannot be shown as an application
window, and a restart that Windows needs to finish updating
(`TUR-WINDOWS-GUEST-027`), arrive as notifications with one action each: "Show
the Windows screen" and "Restart Windows now".

**Android** has the one-sheet opt-in of 8.5 and, for Google Play, the flow of
8.6.

### 8.10 How it is built

| Aspect | Decision |
|---|---|
| Program | `tur-centre`, standalone Qt Quick application, project licence |
| Talks to | The catalogue and router; `tur-runtime` (inspection, installing, launching); `org.tur.Guest1` (Windows: status, setup, installation); `org.tur.Update1` (read-only status); the desktop notification service; `org.tur.Agent1` only to hand a request to P@x |
| Catalogue interface | No D-Bus service exists yet. Until one does, `tur-centre` runs `tur-catalogue search` and `tur-catalogue route` with `--json`, as `tur-diagnostics` runs `tur-diag`. `org.tur.Catalogue1` is this document's proposed name for the later service; the JSON documents stay the same |
| Requirements on the catalogue and router | (1) Reasons, caveats and prerequisites carry a stable identifier and their parameters as well as English text, so that the application centre can show them translated ([section 13.2](#132-internationalisation-requirements)). (2) Host facts are detected by a service, not stated by the caller. (3) The catalogue on the machine can be updated without a system update, with a signature |
| Store adapters | Not in the user interface. Each adapter is a backend of the catalogue service, so that P@x and the command line get the same answers |
| Flatpak | Through that service, which uses `flatpak` or `libflatpak` (LGPL-2.1-or-later); the user interface does not link it |
| File handling | A desktop entry with `MimeType=` for the package types and `Exec=tur-centre --open %f`; a second entry is offered in "Open with" |
| Without the services | Runs against a fixture directory (`--fixture DIR`). The fixtures are the output of `tur-catalogue route --json` for seed entries, kept in the repository; a test in the Rust workflow regenerates them and fails if they differ, so the user interface is always tested against the router's true shapes |
| Buildable now | Search, application pages, "no route" pages, the fixed vocabulary: everything that only reads the catalogue |
| Waits for | Installing and updating: store adapters and `tur-runtime` as a service. Windows setup: milestones W4 and W5 of the guest design (its setup flow and the frozen `org.tur.Guest1`). Google Play: ADR-0011's verification |

## 9. P@x's interface

The [AI control layer](ai-control-layer.md) says what P@x may do. This section
designs what the user sees and touches, and states for each element whether it
can be built on KWin 6.8 as shipped. The findings it relies on are in
[section 3](#3-state-of-upstream-november-2026).

P@x is off until the user turns it on. With P@x off, none of the programs in
this section runs, and the only trace on the desktop is one tray item that the
user can hide (9.1).

### 9.1 Summoning P@x

| Way | What happens | Built with |
|---|---|---|
| **Keyboard:** tap `Meta+Space` | The ask bar opens with the text field focused | `X-KDE-Shortcuts=Meta+Space` in the desktop file of `tur-pax-ui`: configuration, read by KDE's shortcut service [K7]. Whether `Meta+Space` is free in Plasma 6.8's defaults is an **assumption to verify**; the fallback is `Meta+Shift+Space` |
| **Voice:** hold `Meta+Space` and speak; release to send | Push-to-talk, the default (AI control layer §9). The indicator shows "Listening" while held | Needs press and release events. The GlobalShortcuts portal has `Activated` and `Deactivated` signals; whether KDE's implementation delivers the release for a held combination is an **assumption to verify** |
| **Voice:** wake word | Off by default; the user turns it on in Settings ▸ P@x ▸ Voice | `tur-pax` (voice pipeline, not part of this document) |
| **Panel:** the P@x tray item | Click toggles the conversation panel. Its menu: Pause, Private mode, Settings | A StatusNotifierItem over D-Bus, exported by `tur-notifier`: a standard interface, no Plasma applet |
| **Launcher:** type `ask how do I…` or any sentence ending in `?` | A result "Ask P@x: how do I…" | The search provider of [section 7.2](#72-launcher) |
| **File manager:** right-click ▸ "Ask P@x about this file" | The ask bar opens with the file attached | A service-menu desktop file under `/usr/share/kio/servicemenus/`: configuration that starts a program (**assumption**: the path and format are as in Plasma 6.8) |
| **An error message:** "Ask P@x to look into it" | Starts a task with the error code and its technical detail attached | The two-layer message component of `tur-kit`; shown only when P@x is on |

When P@x is off, the tray item is a hollow outline of the flame. Clicking it
shows one small card and nothing else:

> **P@x is off**
> P@x is TUR's assistant. It can do things on this computer for you and show
> you how to do them yourself. It needs a key from an AI provider that you
> choose.
> [Set up P@x]   [Hide this button]

TUR never reminds the user that P@x is off.

### 9.2 The conversation surface

One program, `tur-pax-ui`, with one surface in two sizes.

- **The ask bar.** A single line, 640 px wide, centred horizontally, above the
  panel. It appears on `Meta+Space` and disappears on `Esc` or when it loses
  focus while empty. It is for one request: "open the scanner", "why is the
  fan loud?", "show me where to change the wallpaper".
- **The conversation panel.** 420 px wide, along the right edge of the screen
  the user is working on, from the top of the screen to the panel. It opens
  when an answer needs more than two lines, when a task starts, or when the
  user clicks the tray item. It does not push windows aside unless the user
  pins it.

```text
                                         ┌ P@x ─────────────────── ≡  ─  ✕ ┐
                                         │ Claude · this task 0.04 USD     │
                                         │ [Show me ◉│○ Do it for me]      │
                                         ├─────────────────────────────────┤
                                         │                    You, 10:42   │
                                         │  Make the screen warmer in the  │
                                         │  evening                        │
                                         │                                 │
                                         │ P@x                             │
                                         │  I'll turn on Night Light and   │
                                         │  set it to start at sunset.     │
                                         │                                 │
                                         │  ✓ Opened Settings ▸ Night Light│
                                         │  ✓ Turned on Night Light        │
                                         │  ● Setting the start time…      │
                                         │    Details ▸                    │
                                         │                                 │
                                         ├─────────────────────────────────┤
                                         │ [ Ask or tell P@x…     (mic) > ]│
                                         │ [■ Stop  Meta+Esc]  [❙❙ Pause]  │
                                         └─────────────────────────────────┘
```

**What is in it.**

- *The header* names the provider in use, shows what the current task has cost
  so far (9.11) and holds the guide-mode switch (9.6).
- *The user's words and P@x's words* are visually different and are never
  mixed with system text. P@x speaks in the first person. Everything the
  system says about P@x, such as an approval card, a limit that was reached or
  a refusal by policy, is in the third person, in a bordered card with the TUR
  mark. The rule exists so that a person can tell what the model said from
  what the system decided ([section 5.6](#56-tone-of-voice)).
- *Steps.* Each action P@x takes is one line in plain words, with a tick when
  done. "Details" opens the technical layer: the tool or command, a summary of
  its arguments, the action class and the audit record's identifier. A command
  is always shown in full.
- *Stop and Pause* are always visible while a task runs, in the same place,
  with the keyboard combination written on the button.
- *The text field* accepts typing at any time, including while P@x works:
  "slow down", "go back", "stop".

**While P@x works on the screen** the panel must not cover what P@x is working
on. It collapses to a pill at the top right of that screen showing the current
step and a Stop button, and returns when the task ends or needs the user.

**Results.** A finished task ends with a summary card: what was done, what was
changed (with "Undo" where the system can undo it), what was deferred and why.

**Implementation.** `tur-pax-ui` is a standalone Qt Quick program. The surface
is a layer-shell surface (top layer, anchored right, keyboard interactivity
"on demand") through LayerShellQt, which KWin offers to any client [K1]. It
talks to `org.tur.Agent1` on the session bus. Buildable today.

### 9.3 The activity indicator

The rule from the AI control layer (§8): the user can always see when P@x is
observing and when it is controlling. The indicator has two parts that always
agree: a **pill** at the top centre of each screen concerned, and a **frame**
around the screen or window concerned.

| State | When | Pill | Frame | Tray item |
|---|---|---|---|---|
| **Idle** | P@x is on and doing nothing | None | None | Flame, solid |
| **Listening** | Push-to-talk held, or wake word heard | Microphone · "Listening…" | None | Flame with a microphone |
| **Working** | A task runs with tools and commands only; nothing on screen is read or driven | Flame · "P@x is working" · [Stop] | None | Flame, animated |
| **Observing** | Screen content or the accessibility tree is being read | Eye · "P@x can see this window" (or "this screen") · [Pause] | 2 px gold line, solid | Flame with an eye |
| **Acting** | P@x is driving an application through the accessibility tree or the screen | Pointer · "P@x is controlling · Stop: Meta+Esc" · [Stop] | 3 px gold line, solid; P@x's pointer is visible | Flame with a pointer |
| **Waiting for you** | An approval or an answer is needed | Hand · "P@x is waiting for you" | 2 px gold line, dashed | Flame with a dot |
| **Paused** | The user paused, or the stop control was pressed | Pause sign · "P@x is paused" · [Resume] [End task] | 2 px grey line, dashed | Flame with a pause sign |
| **Repair session** | Added to any state while a repair session is open | An orange segment: "Repair session · 12:41 left · [End]" | — | An orange dot |
| **Private mode** | The user suspended all observation | None; observing and acting cannot occur | None | Flame crossed out |

Every state differs from the others by icon and by words, and the frame by
line style. Colour is never the only difference. The pill is quiet when P@x
is quiet and loud when it is not: dark with a gold outline while P@x observes,
solid gold with dark text while it acts, grey and dashed when paused (drawn in
[desktop-assets/pax-pointer.svg](desktop-assets/pax-pointer.svg)). With
reduced motion nothing pulses or slides. The pill is 36 px high; on a screen
the user is not looking at (another monitor) it is identical, so that a glance
at any screen answers the question.

**Where the frame goes.** By default P@x reads one window: the frame is drawn
around that window and the pill says "this window". When a task needs the whole
screen, the frame is at the screen's edge and the pill says "this screen". The
first is the privacy default (9.9).

**Implementation.** `tur-pax-overlay`, a standalone Qt Quick program, draws the
pill, the frame, P@x's pointer and the highlights. It uses layer-shell surfaces
on the overlay layer, one per screen, which is the topmost layer in KWin and is
above full-screen windows [K1]. The frame and pointer surfaces have an empty
input region, so they never take a click from the user; only the pill accepts
input. Buildable today.

**What this does not give, stated plainly.** The specification asks for a
*compositor-drawn* indicator that P@x cannot disable. A layer-shell surface is
drawn by a client. Three measures stand in for the compositor:

1. **No indicator, no access.** The component that captures the screen and
   injects input (milestone A2) must hold a lease from `tur-pax-overlay`. If
   the overlay is not on the session bus, or does not confirm that the pill is
   mapped on every screen concerned, capture and input stop. The overlay cannot
   be told to hide while a lease is held.
2. **A second indicator that TUR does not draw.** When P@x's access goes
   through the desktop portals, Plasma shows its own tray item for the screen
   cast and, since 6.7, a "Remote Control Started" notification [K2] [K5].
3. **The lock screen.** KWin renders only lock-screen windows while the session
   is locked [K1], so the indicator disappears; and an attended task is paused
   whenever the session is locked, so nothing is being observed either.

These hold against a misled model. They do not hold against other software
running as the user, which could remove the overlay and imitate its bus name.
A compositor-enforced indicator needs work in KWin
([9.13](#913-what-can-be-built-today-and-what-is-blocked-upstream)).

### 9.4 P@x's pointer

**How it looks.** P@x's pointer is not an arrow. It is the flame from the
logo, tilted so that its tip is the hot spot: a teardrop with a gold fill
(`#E8B44C`), a dark outline (`#0B1220`, 1.5 px) and a white hairline outside
that, so that it is visible on any background. It is one and a half times the
size of the user's cursor and follows the cursor-size setting. A small tag
reading "P@x" trails it while it is at rest. A click is shown as a gold ring
that expands from the tip and fades.

```text
   user's cursor          P@x's pointer            a click
       ↖                     ◢◣                      ◎
      (system theme,        ◥██◣  P@x              ◢◣
       unchanged)             ◥◤                   ◥◤
```

A drawing is in
[desktop-assets/pax-pointer.svg](desktop-assets/pax-pointer.svg). The user's
cursor theme is not changed, so the two can never be confused: one is the
arrow the user has always had, the other is gold, flame-shaped and labelled.

**How it moves.** It travels; it does not jump. A move takes between 250 ms
and 700 ms depending on distance, along a gentle curve, decelerating into the
target, so that the eye can follow it. "Slow down" doubles every duration. With
reduced motion it fades out and fades in at the target, and the target gets a
ring.

**Across monitors.** There is one P@x pointer. It leaves a screen at the edge
that faces the next screen in the arrangement and enters the other at the
matching point, at each screen's own scale. A screen that P@x is acting on
shows the "controlling" pill; every other screen shows a small chip at the edge
nearest the action: "P@x is working on the left screen ←". If the target
screen is disconnected mid-task, P@x pauses.

**What it never does.** It never enters a protected surface. While an approval
sheet, an authentication dialog or the trusted screen is showing, the pointer
is parked beside the pill, which reads "P@x is waiting for you".

**How it is built, and the limit.** The pointer is a sprite drawn by
`tur-pax-overlay`. That part is buildable today. What KWin does not have is a
second pointer or a second seat: emulated input feeds the one seat every
physical device feeds, and there is one cursor [K3]. So:

- When P@x acts through **tools, commands or the accessibility tree**, nothing
  moves the user's cursor. P@x's pointer shows where it is acting, and the
  user's cursor stays theirs. This already matches the specification.
- When P@x acts through **the screen** (emulated pointer and keyboard, the only
  way to operate Windows and many Android applications), the emulated events
  move the user's one cursor and type into the one focused window. The user's
  cursor *is* taken for the length of each action. The interim rule is
  turn-taking, with the person always winning:
  - P@x's pointer travels to the target first, so the user sees what is about
    to happen.
  - The real cursor is then placed at the target, the action is delivered, and
    the cursor is returned to where the user left it.
  - If the user moves the mouse, presses a key or touches the screen while P@x
    is acting through the screen, P@x pauses at once and says "You took over,
    so P@x paused. [Resume]". Detecting the user's pointer is possible from the
    cursor position carried in the screen-cast stream's metadata [K5];
    detecting a physical key press needs the witness service of
    [9.8](#98-the-trusted-confirmation-prompt-class-3) or work in KWin.
  - The user cannot work in one window while P@x works through the screen in
    another. They can while P@x works through tools or the accessibility tree.

The real thing, P@x's pointer and keyboard focus existing beside the user's so
that both can work at once, needs multi-seat support in KWin, which is an open
upstream issue with no implementation [K3].

One experiment is worth making before that. KWin's emulated-input backend
accepts touch devices as well as pointers [K2]. If P@x's clicks are sent as
taps, the user's cursor need not move at all. Whether applications from each
ecosystem respond to an emulated tap as they do to a click, and what KWin does
with the cursor while touch is in use, are **not verified**; the experiment
belongs to milestone A2. It would not give P@x its own keyboard focus.

### 9.5 Highlighting

P@x can mark a control, a row or a region without acting on it (class 0).

**How it looks.** A rounded rectangle around the target: 3 px gold line with a
1 px dark line outside it, 6 px corner radius, 4 px clear of the target. For a
single row in a long list, the rest of the window is dimmed by 20 % so the row
stands out. A caption bubble sits beside the highlight, never over it, with
the step's instruction and the guide controls of 9.6. In a high-contrast
scheme the line is 4 px in the scheme's highlight colour.

**How it is built.** Two mechanisms, because TUR controls some applications
and not others.

1. **TUR's own applications highlight themselves.** `tur-settings`,
   `tur-centre` and the other TUR programs implement `org.tur.Addressable1`
   ([section 10.3](#103-the-addressing-scheme)). Asked to reveal an address,
   the program opens the page, scrolls the row into view, draws the highlight
   inside its own window (so it stays attached while the user scrolls or
   resizes) and returns the row's rectangle. This is exact, and buildable
   today.
2. **Other applications are highlighted from outside** by `tur-pax-overlay`.
   The target's rectangle is known either in screen coordinates (when P@x
   found it in a screenshot) or relative to its window (when it came from the
   accessibility tree: on Wayland a client knows positions only within its own
   window). In the second case the overlay adds the window's position, which
   it reads from KWin: `org.kde.KWin.getWindowInfo` returns a window's frame
   geometry in screen coordinates, and the Plasma window-management protocol
   also gives the client-area geometry and the stacking order [K6]. The
   overlay re-reads both several times a second and removes the highlight if
   the window moves, is covered or loses the target. Buildable today on KWin
   6.8; less exact than mechanism 1, and dependent on the application exposing
   an accessibility tree.

No KWin script or effect is needed for either. KWin's own "highlight window"
effect highlights whole windows only, and there is no built-in way to mark a
rectangle from outside [K11].

### 9.6 Guide mode: "show me" and "do it for me"

A switch in the panel's header, and two phrases P@x understands at any time.

**Show me.** P@x opens the right place and points; the user does the clicking.
P@x changes nothing beyond opening windows.

```text
 ┌ Settings ▸ Night Light ──────────────────────────────────────────────┐
 │  Night Light                                                         │
 │ ╔══════════════════════════════════════════════════════╗             │
 │ ║ Switching times:   [ Always off                  ▾ ] ║  ◢◣ P@x     │
 │ ╚══════════════════════════════════════════════════════╝  ◥◤         │
 │  Day colour temperature     ─────────●───                ┌──────────────────────────┐
 │  Night colour temperature   ───●─────────                │ Step 2 of 3              │
 │                                                          │ Open this list and choose│
 └──────────────────────────────────────────────────────────│ "Sunset and sunrise".    │
                                                            │ [Back] [Repeat]          │
                                                            │ [Do it for me]  [Stop]   │
                                                            └──────────────────────────┘
```

- Each step has one highlight and one sentence. P@x moves to the next step
  when it sees the step done (the control's state changed), not on a timer.
- "Back" returns to the previous step; "Repeat" replays the pointer's movement
  and, with voice on, the sentence.
- If the user does something else, P@x waits. After a while the caption adds
  "Still here when you are ready." It does not nag and it does not take over.

**Do it for me.** P@x performs the steps itself, under the usual rules. Its
pointer travels to each control before acting on it, at guide speed, so the
user can learn by watching. Anything of class 2 or above still stops for
approval.

The user can switch at any step: "do it for me" hands the rest to P@x; "let me"
or moving the mouse hands it back. "Slow down", "repeat that" and "go back"
work in both.

Buildable today for TUR's own settings (mechanism 1 of 9.5). For KDE System
Settings it depends on the mapping of [section 10.4](#104-kdes-settings).

### 9.7 Approving a proposed action (class 2)

A class 2 action stops and asks. The request appears in two places: as a card
in the conversation, which gives the context, and as the **approval sheet**,
which is where the decision is made.

```text
            ┌ TUR · P@x is asking ──────────────────────────────────────┐
            │                                                           │
            │ Send this email?                                          │
            │                                                           │
            │   To        marta.kowalski@example.org                    │
            │   Subject   Minutes of Tuesday's meeting                  │
            │   Text      "Hello Marta, the minutes are attached. …"    │
            │             Show all (1,204 characters)                   │
            │   Attached  minutes-2026-11-10.pdf (212 kB)               │
            │                                                           │
            │ This cannot be undone once sent.                          │
            │                                                           │
            │ You asked: "send Marta the minutes"                       │
            │                                                           │
            │              [ Don't send ]        [ Send ]               │
            │ This approval is for this one action only.                │
            └───────────────────────────────────────────────────────────┘
```

Rules for the sheet:

- **The title is the action as a question, with its verb**: "Send this email?",
  "Delete these 14 files permanently?", "Install Krita?", "Buy this for
  12.99 EUR?", "Run this command?". Never "Are you sure?" and never "Allow
  P@x?".
- **The body is the exact action**, produced by the policy engine, not by the
  model: the tool and its parameters set out as fields, or the command with
  every argument, never shortened, with every character that is not printable
  ASCII shown as an escape ([agent-policy.md](agent-policy.md), section 3).
  For installing, the "How it will run" block of
  [section 8.4](#84-an-applications-page).
- **The consequence** is one sentence from the tool's manifest: what cannot be
  undone, who else is affected, what it costs.
- **"You asked"** quotes the user's own request, so that an action that does
  not follow from it is obvious. P@x's explanation of *why* is in the
  conversation, not in the sheet: the sheet holds no text written by the model.
- **Neither button is the default.** Enter and Space do nothing until the user
  moves focus to a button. The buttons accept input 600 ms after the sheet
  appears, so that a click or key press already on its way cannot approve.
  The labels are the action's verb and its refusal, never "OK" and "Cancel".
- **One action per approval.** There is no "always allow" on the sheet. The
  only standing relaxation is the per-application setting in Settings ▸ P@x ▸
  Approvals (AI control layer §5.3), and the sheet links to it only after the
  same kind of action has been approved several times: "You can let P@x do
  this in Thunderbird without asking. Open the setting".
- **No timeout approves.** If nobody answers, the task waits, the pill reads
  "P@x is waiting for you", and after five minutes a notification says so once.
- **With spoken approval switched on**, the sheet still appears and says
  "Say 'yes, send it' or choose below"; the conditions of §9 of the AI control
  layer are checked by `tur-pax`.

**Implementation.** The sheet is its own small program, `tur-pax-approve`,
with its own application identifier, so that window rules can apply to it. It
is an ordinary window (not a layer-shell surface) because KWin's rules and its
capture exclusion apply only to ordinary windows [K4] [K8]. A rule shipped in
`/etc/xdg/kwinrulesrc` forces it to stay above other windows, keeps it out of
the task bar and the task switcher, and excludes it from screen capture
(`excludefromcapture`, available as a rule since Plasma 6.7 [K4]). The rule is
marked immutable, so a file in the user's configuration does not override it. It
receives the pending action from the component that owns policy (AI control
layer §17, item 5; called `tur-pax-guard` here) and returns the decision to
that component directly, not through `tur-pax`.

**How far P@x is kept from the sheet today.** KWin 6.8 cannot make a surface
ignore emulated input [K3]. The protection is in TUR's own code:

1. While an approval is pending, the policy engine releases no accessibility
   or screen action at all, and the input component withdraws its devices.
2. The accessibility executor refuses any target in a TUR protected program.
3. The sheet is excluded from capture by the window rule, and P@x's default
   capture is a single other window in any case (9.9).
4. Commands that P@x runs are sandboxed without access to the compositor or
   to the session's control interfaces (a requirement on the command runner:
   in Plasma 6.8 any unsandboxed process in the session can inject input and
   capture the screen without a prompt [K2]).

This stops a misled model from approving its own request through any of its
four ways of acting. It does **not** stop other software running as the user,
and it does not meet section 6 of the AI control layer as written, which
requires the compositor to provide the protection. Whether this level is
enough for class 2 is the first question for the owner
([section 17](#17-open-questions-for-the-owner)); the alternative that works
today is to give class 2 approvals on the trusted screen of 9.8 as well.

### 9.8 The trusted confirmation prompt (class 3)

Class 3 operations (disks, start-up, firmware, encryption, system update and
rollback, device hand-off, security policy), opening a repair session, and
changes that relax P@x's own limits are approved only here.

**The problem.** Section 6 of the AI control layer asks the compositor for
five guarantees. On KWin 6.8, verified in its source [K3] [K4]:

| # | Guarantee | KWin 6.8 |
|---|---|---|
| 1 | Emulated input is distinguishable | Inside KWin, yes: every event carries its device. Nothing is exposed to clients, and no filter uses it |
| 2 | Protected surfaces ignore emulated input | **No.** Not for any surface, including the lock screen |
| 3 | Protected surfaces are not captured | **Partly.** A window can be excluded from all capture (6.6; by rule from 6.7). Not a layer-shell surface, and not "from P@x only" |
| 4 | Protected surfaces cannot be imitated | **No.** There is no compositor-drawn or otherwise privileged prompt |
| 5 | A stop control the assistant cannot remap or suppress | **Partly.** A global shortcut exists, but its binding is in a file the user's processes can write |

**The design: a trusted screen outside the user's session.** TUR does not
wait for KWin. The prompt is shown by a separate, minimal graphical session
that belongs to a dedicated system account, on its own virtual terminal, with
its own compositor instance. It is the mechanism of "Switch user": the display
leaves the user's session, shows another, and comes back. The login screen is
itself such a session: on this image it runs as its own account with its own
`kwin_wayland` ([TUR-003 report](../reports/TUR-003.md)).

Why this gives the guarantees without any change to KWin:

- **Input (1, 2).** P@x's emulated input, and that of any program in the
  user's session, is delivered by the user's compositor to the user's windows.
  The trusted screen is a client of a different compositor instance, owned by
  a different account, whose sockets the user's processes cannot open. While
  the trusted screen is displayed, the user's compositor does not have the
  display or the input devices.
- **Capture (3).** Screen capture in the user's session captures what the
  user's compositor renders. The trusted screen is not rendered by it.
- **Imitation (4).** See "What makes it unmistakable" below.
- **Stop (5).** See "The stop control" below.

**What the user sees.**

1. In the desktop, a notice in the conversation and a notification:

   > **Continue on the trusted screen**
   > P@x is asking for something only you can approve. TUR will switch to the
   > trusted screen. P@x cannot see it or use it.
   > [Switch now]   [Cancel the request]

   The switch is made by the user's click or key press, or automatically after
   the notice has been visible for three seconds when the user started the task
   in the last minute. It never happens without the notice.
2. The screen changes. The desktop, P@x's indicator and P@x's pointer are
   gone, because that session is no longer displayed.

```text
╔══════════════════════════════════════════════════════════════════════════════╗
║  ❂ TUR · Trusted screen                       Tuesday 12 November · 10:42    ║
║ ──────────────────────────────────────────────────────────────────────────── ║
║  Your security phrase                                                        ║
║     ┌────────┐                                                               ║
║     │ (your  │   heron on a red bicycle                                      ║
║     │picture)│                                                               ║
║     └────────┘   If this is not your phrase and picture, do not type         ║
║                  anything. Press Cancel.                                     ║
║ ──────────────────────────────────────────────────────────────────────────── ║
║  P@x is asking to                                                            ║
║                                                                              ║
║     Go back to TUR 0.3.2                                                     ║
║                                                                              ║
║  What will happen                                                            ║
║     The computer restarts into version 0.3.2. Your files and applications    ║
║     stay as they are. System settings changed since the update (network      ║
║     connections for all users, accounts, passwords) go back to what they     ║
║     were. Version 0.4.0 is kept.                                             ║
║                                                                              ║
║  Asked by     P@x, in the task you started at 10:39                          ║
║  Service      TUR updates (org.tur.Update1 · Rollback)                       ║
║                                                                              ║
║  Password for Marta   [ ••••••••••            ]     or touch the fingerprint ║
║                                                     reader                   ║
║                                                                              ║
║                 [ Cancel ]                      [ Approve and restart ]      ║
║                                                                              ║
║  P@x cannot see this screen or use it.                Technical detail ▸     ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

3. After "Approve" or "Cancel" the display returns to the desktop, where a
   notification states the outcome.

Rules for the content:

- **Everything on the trusted screen comes from the service that will perform
  the operation**, not from P@x: the operation's name, its target with a
  stable identity, and its consequences. The model's words do not appear,
  with one exception: the stated purpose of a repair session, which is by
  definition P@x's. It is shown in a quotation box labelled "P@x wrote", with
  characters that are not printable ASCII shown as escapes.
- **Destructive disk operations** add the typed phrase of
  [section 11.5](#115-the-installer) (`ERASE` and the end of the serial
  number) and show the disk as that screen does.
- **No default button**, the same input delay as the approval sheet, and no
  "remember this".
- **A repair session** is opened here:

  > **Open a repair session?**
  > P@x wrote: "Restart the printing service and repair its configuration."
  > For the next [15 ▾] minutes P@x may run administrator commands for this
  > purpose. Each one is shown as it runs and is recorded.
  > **Never covered:** disks, start-up, firmware, encryption, system updates
  > and security settings. Those are asked here, one at a time.
  > The session ends when the time is up, when the screen locks, when the task
  > finishes, or when you end it.
  > [Cancel]   [Open repair session]

**What makes it unmistakable, and hard to imitate.**

1. **It is not on the desktop.** Every TUR prompt that appears among the
   user's windows is, by definition, not the trusted screen. The setup flow
   teaches one sentence: "TUR asks for your password for P@x's requests only
   on the trusted screen, which shows your phrase and picture."
2. **A personal security phrase and picture.** The user chooses them *on the
   trusted screen itself*, during P@x's setup (9.12). They are stored where
   only the trusted screen's account can read them. No program in the user's
   session, P@x included, has ever seen them, so none can draw a convincing
   copy. A full-screen imitation would be missing the phrase.
3. **A look used nowhere else.** A deep-blue field (`#0A2A4A`) inside a
   brushed-silver frame, taken from the logo's lettering. `tur-kit` does not
   offer this composition to applications, and a test fails if a TUR
   application uses the trusted screen's colours together.
4. **An imitation gains nothing but a password.** Approval is recorded by the
   system component that verified the authentication on the trusted screen.
   Nothing a window in the user's session displays or collects can approve an
   operation. The phrase exists so that the password is not typed into an
   imitation.

"Impossible to imitate" in the strict sense needs either hardware (a light or
key that only the system controls) or a compositor that draws the prompt. The
design above makes an imitation detectable by an attentive user and worthless
for approving anything. That is the honest extent of the claim.

**The stop control.** `Meta+Esc`, at any time, pauses P@x and withdraws its
input. It is delivered two ways:

- *Today:* a global shortcut registered through the desktop file of
  `tur-pax-ui` [K7], handled by `tur-pax`. It works, but the binding is in the
  user's configuration, and a client can ask KWin to inhibit shortcuts while
  it has focus.
- *To close the gap without KWin:* a very small system service, the **physical
  input witness**, that reads keyboards below the compositor and reports two
  things only: that the stop combination was pressed, and that some physical
  key or button was used (with no indication of which). Emulated input never
  appears at that level, so the service cannot be fooled or suppressed by
  anything in the session. It is also what would let TUR notice that the user
  has started typing while P@x is acting (9.4). A service that reads keyboards
  is sensitive however little it reports, so it is proposed, not decided:
  [section 17](#17-open-questions-for-the-owner), question 2.

**Implementation, and what must be verified.** `tur-confirm` is a standalone
Qt Quick program using Qt Quick Controls' Basic style and nothing from Plasma,
started by `tur-pax-guard` in a session of class "greeter" for a dedicated
account, under an unmodified `kwin_wayland` (a separate GPL program, used as
shipped). Authentication is through PAM. All of this is an **assumption until
a prototype passes the trusted-path tests** (task D-16):

- that a second compositor session can be started on demand and switched to
  and from in about a second (to be measured, not promised), including in the
  QEMU test machine;
- that no process in the user's session can reach the trusted session's
  sockets, its display or its input: in particular that `/dev/uinput` is not
  accessible to the user (a udev rule shipped with some game-controller
  packages grants it, which would let a user process create a device that
  every compositor treats as physical);
- how the system's authentication requests for TUR's class 3 operations are
  routed to it. polkit allows one agent per session [K9]; routing needs either
  a TUR agent in place of `polkit-kde-agent-1` or a separate authentication
  path in `tur-pax-guard`. This is a security design of its own and needs an
  ADR before it is built;
- memory while the trusted session exists (it is started on first need and
  stopped after a period without use).

**When P@x is off**, class 3 operations requested by the user in Settings are
authenticated by the desktop's ordinary authentication dialog. The trusted
screen is part of P@x's safeguards; update, rollback and recovery never depend
on it (AGENTS.md, AI control layer rules).

**The path to an in-desktop prompt.** A prompt that appears over the desktop
without a screen switch needs KWin to accept physical input only for a surface
and to keep it out of capture for a given consumer. KWin already knows the
device behind every event; the work is a filter and a way to mark a surface,
proposed upstream. Until it exists the trusted screen is the trusted path.

### 9.9 Privacy controls

Settings ▸ P@x ▸ Privacy (`tur.pax.privacy`) and the tray item's menu.

```text
What P@x may see
 P@x sees   ◉ Only the window it is working in      ○ The whole screen when a task needs it
 Private mode                                             [ off ]   Meta+Shift+Esc
   While on, P@x cannot see the screen or any window. It can still talk with you
   and use tools that do not look at the screen.

Applications P@x never sees or touches                                    [Add…]
   KeePassXC · KWallet · Private browser windows · The lock screen         (built in)
   Signal                                                                  [Remove]

Applications where P@x always asks first                                   [Add…]
   Firefox · Konsole and other terminals                                   (built in)

After a task
 Keep screenshots P@x took                                 [ off ]
 Keep the record of what P@x did                           [ 90 days ▾ ]   [Open the record]

How much P@x does without asking
 ◉ Normal            Does things that can be undone; asks before the rest
 ○ Ask every time    Asks before every action, including looking
 ○ Only look         P@x can look and explain, and changes nothing
```

- **Private mode** is one action from anywhere: the tray menu, `Meta+Shift+Esc`
  or saying "private mode". The tray item changes at once. Leaving private
  mode is only ever done by the user.
- **Excluded applications** are never captured, never read through the
  accessibility tree and never acted in. The "Add…" picker lists installed
  applications by name and icon; the setting stores application identifiers.
  The built-in entries cannot be removed. (The policy engine's known limits
  note that default exclusions are not yet shipped; the list above is this
  document's proposal for them.)
- **Making P@x stricter** takes effect at once. **Making it less strict**
  (removing an exclusion, a per-application relaxation, spoken approval,
  raising a spending limit) is approved on the trusted screen, because the
  policy file is owned by `tur-pax-guard` and P@x must not be able to change
  it (AI control layer §17, item 5).

**How exclusion is enforced with KWin 6.8.**

- *One window by default.* P@x's capture is a stream of the single window it
  is working in, not of the screen. The portal and KWin both offer per-window
  streams [K5]. Other windows, notifications, the panel and TUR's own overlay
  are then simply not in the picture. Whether a pre-authorised portal session
  can choose a window without a dialog was **not verified**; if not, KWin's own
  screen-cast interface offers a stream by window identifier.
- *Whole-screen capture* (when a task needs it) includes everything KWin
  renders. Excluded applications' windows and TUR's own surfaces are blanked
  in each frame by the capture component before the frame leaves it, using
  window geometry and stacking order from KWin [K6]. KWin can also hide a
  window from all capture, but that would hide it from the user's own video
  calls too, so it is used only for TUR's protected windows [K4].
- KWin has no "hide from this consumer only" [K4]. That is the upstream
  feature that would replace the blanking.

### 9.10 The repair-session view

While a repair session is open, three things are true on screen: the pill
carries the orange segment with the time left and an "End" button; the tray
item has an orange dot; and the repair view is open in the conversation panel
or as its own window.

```text
┌ Repair session ──────────────────────────────────────────────────────────────┐
│ ▲ P@x may run administrator commands             12:41 left    [End now]     │
│ Purpose: "Restart the printing service and repair its configuration."        │
│ Opened by you on the trusted screen at 10:44                                 │
├──────────────────────────────────────────────────────────────────────────────┤
│ 10:44:09 ✓ Read    systemctl status cups.service                  read-only  │
│ 10:44:15 ✓ Restart service  cups.service                          typed      │
│ 10:44:31 ✓ Change file  /etc/cups/cupsd.conf   +1 −1  [Show] [Undo]  typed   │
│ 10:44:40 ● Running  cupsctl --share-printers                                 │
│                                                                              │
│ ┌ P@x is asking ───────────────────────────────────────────────────────────┐ │
│ │ Run this as administrator?                                               │ │
│ │   lpadmin -p Office -o printer-error-policy=retry-job                    │ │
│ │ TUR cannot check what this command will do.                              │ │
│ │                         [ Don't run ]      [ Run once ]                  │ │
│ └──────────────────────────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────────────────────────┤
│ Never covered by this session: disks, start-up, firmware, encryption,        │
│ system updates, security settings.                         [Open the record] │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **The list is live** and is fed by the privileged helper, not by P@x. Each
  line has the time, the outcome, the operation and its kind: a typed
  operation (which the helper can check), or a free-text command.
- **A free-text privileged command** is never run on the strength of the
  session alone. It appears as an approval card with the exact command and the
  sentence "TUR cannot check what this command will do." The card follows the
  rules of 9.7.
- **"End now"** ends the session at once. So do the timer, locking the screen
  and the end of the task. The pill's segment disappears the moment it ends.
- **Afterwards** the view becomes a summary: "What changed", each change with
  "Show" and, where the helper recorded how, "Undo"; "Undo everything this
  session changed"; and what could not be undone, said plainly. Past sessions
  are listed in Settings ▸ P@x ▸ Repair sessions.

Buildable today as a user interface. It depends on the helper and repair
sessions of milestone A5, and on the trusted screen for opening a session.

### 9.11 Memory, schedules and spending

**Memory** (Settings ▸ P@x ▸ Memory, `tur.pax.memory`).

```text
P@x remembers things                                              [ on ]
 Memory stays on this computer. When something P@x remembers matters to a task,
 it is sent to your AI provider with that task.

[ Search memory                    ]

You told P@x                                                             12
  "My printer is the one in the study."            3 Nov 2026   [Edit] [Forget]
  "Always ask before emailing my manager."         1 Nov 2026   [Edit] [Forget]
P@x learned from what it did                                             31
  LibreOffice is installed as a Linux application.  2 Nov 2026          [Forget]
Ways of doing things that worked                                          4
  "Scan to PDF and save in Documents" · 5 steps     28 Oct 2026  [Show] [Forget]
Read somewhere, waiting for you                                           1
  "The Wi-Fi password for the office is …"   read on a web page, 9 Nov 2026
  P@x does not keep passwords. This was not saved.               [Dismiss]
  "Marta's birthday is 4 March."             read in an email, 9 Nov 2026
                                                          [Keep]   [Discard]

[Forget everything…]
```

Every entry shows where it came from and when. Entries from content are kept
only if the user says so. "Forget everything…" asks once, names the number of
entries, and cannot be triggered by P@x.

**Schedules** (Settings ▸ P@x ▸ Schedules, `tur.pax.schedules`).

```text
Reminders and scheduled tasks                                        [New…]

 Weekly report to Marta           Fridays 16:00 · next: 14 Nov     [ on ]
   Last run: 7 Nov, done                          [Run now] [Edit] [Delete]
   May do without asking (approved by you on 1 Nov):
     Send email · to marta.kowalski@example.org · subject "Weekly report"
                                                               [Revoke]
 Remind me to stretch             Every day 11:00                  [ on ]
   A reminder. It does nothing but tell you.

While you were away                                                      2
  Fri 7 Nov 16:00  Weekly report: sent.
  Sat 8 Nov 09:00  Back up photos: waited for you. It needed to delete 3 files
                   and had no approval for that.    [Approve now] [Dismiss]
```

- P@x proposes; the user creates. A proposal arrives as a card in the
  conversation ("P@x suggests a schedule") whose button opens this page's
  "New…" sheet filled in.
- A standing approval names one tool with its exact parameters. It is granted
  on the trusted screen, because it is policy.
- "While you were away" is the first thing shown in the conversation panel
  when the user returns after an unattended task did or deferred something.

**Spending** (Settings ▸ P@x ▸ Spending, `tur.pax.spending`). P@x uses the
user's own key, so its cost is the user's.

- In the conversation header: "this task 0.04 USD". In the tray item's
  tooltip: "today 0.31 USD of 2.00 USD".
- In Settings: today and this month as numbers, the limit per task and per
  day, and the sentence "These are estimates from your provider's published
  prices. Your provider's invoice is what counts."
- When a limit is reached the task stops, and a system card says:

  > **P@x stopped: this task reached its limit of 1.00 USD**
  > What was done so far is listed above.
  > [Allow another 1.00 USD for this task]   [Stop here]

- Amounts are in the provider's billing currency. Token counts are in
  "Details". Raising a limit is approved on the trusted screen.
- A cost is shown only when TUR knows the price of the model in use. When it
  does not, the display shows the amount of use the provider reports, and
  says "TUR does not know this model's price, so it cannot show a cost."
- A limit takes effect between two answers from the provider, because
  providers report use at the end of an answer. Settings says so: "A task can
  pass its limit by the cost of one answer."

Charts are not needed for three numbers, and the module that would draw them
(Qt Graphs) is GPL-only.

### 9.12 Setting up P@x

Started from the welcome window, the tray item or Settings ▸ P@x. Seven
screens; the user can leave at any point and nothing is enabled until the last.

**1. What P@x is.**

> **Meet P@x**
> P@x is the assistant built into TUR. You can ask it, by typing or speaking,
> to do things on this computer, to fix problems, or to show you how to do
> something yourself.
> P@x works through an AI service that you choose and pay for directly. TUR
> does not run one and does not see what you ask.
> You stay in charge: things that can be undone, P@x does. Before sending,
> buying, deleting or installing, it asks. Disks, start-up and security it can
> never change without you approving on a screen it cannot reach.
> [Not now]   [Continue]

**2. Choose a provider.** Three cards in alphabetical order, with no
recommendation: Anthropic (Claude), Google (Gemini), OpenAI. Each has "Get a
key" and "How this provider handles your data".

**3. What will be sent.** The consent text, from §8 of the AI control layer,
with the chosen provider's name filled in:

> **What P@x sends to Anthropic**
> To do what you ask, P@x sends these to Anthropic over an encrypted
> connection:
> - what you ask, as text;
> - pictures of the window P@x is working in, or of the screen when a task
>   needs it;
> - the names and states of buttons and other controls in that window;
> - the results of the tools and commands P@x runs for you;
> - the things P@x remembers that matter to the task.
>
> **Never sent:**
> - the sound of your voice (speech is turned into text on this computer);
> - the trusted screen, sign-in prompts and password managers;
> - windows of applications you exclude;
> - passwords and keys stored by the system;
> - support bundles, unless you attach one yourself.
>
> **What Anthropic keeps**
> P@x asks every provider not to store your conversations where the provider
> offers that choice. What Anthropic keeps anyway, and for how long, is
> governed by Anthropic's terms, not by TUR. TUR cannot control it.
> Read Anthropic's data terms
>
> You can see everything P@x did in Settings ▸ P@x ▸ Activity, and stop P@x
> at any time with Meta+Esc.
> ☐ I understand what will be sent to Anthropic.
> [Back]   [Continue]

"Continue" is disabled until the box is ticked. The consent is recorded with
its date and the text's version; if the text changes in an update, P@x asks
again before its next task.

The paragraph "What Anthropic keeps" differs by provider, because the
providers differ: one keeps nothing between requests by design of its
interface, and two store a conversation for weeks unless asked not to, which
P@x always asks ([pax-providers.md](pax-providers.md), section 9). The
providers' terms have not yet been reviewed by the project; the wording of
this screen is final only after that review (ADR-0004).

**4. Your key.**

> **Paste your key from Anthropic**
> [ ••••••••••••••••••••••••••••            ]   [Show]
> TUR keeps the key in this computer's password store. It is given only to
> P@x, and it is never written to settings files, logs or support bundles.
>
> Model   [ _(a list)_                        ▾ ]   or type a model's name
> [Check the key]   This sends one very small request, which Anthropic bills.

"Check the key" reports the result in plain words, with the `TUR-AI-*` code on
failure ("Anthropic did not accept this key.").

The model must be chosen: the provider layer has no default and no model name
is built into any program, and it cannot yet ask a provider which models
exist ([pax-providers.md](pax-providers.md), sections 5 and 10). The list is
therefore a data file shipped and updated with TUR, holding for each provider
the models TUR has run its conformance tests against, with their prices; a
name typed by hand is accepted and marked "not tested with TUR". That file is
a requirement on `tur-pax` and does not exist.

**5. Limits.** "Stop a task when it has cost [1.00] USD. Stop for the day at
[5.00] USD." with the sentences about estimates from 9.11. For a model whose
price TUR does not know, the limits are set in the provider's own unit of
use and the screen says why.

**6. What P@x may see and control.**

> **Let P@x see and use the screen?**
> With this on, P@x can look at the window it is working in and use the
> keyboard and pointer there, which is how it operates Windows and Android
> applications. You will always see when it is looking or controlling.
> With this off, P@x can still talk with you, use TUR's own tools and run
> commands you approve.
> ◉ On   ○ Off
> [Back]   [Continue]

Turning it on writes the standing grant for the remote-desktop portal (the
`kde-authorized` permission, Plasma 6.3 and later [K2]), so that there is no
portal dialog on every session, as the AI control layer requires (§4).

**7. The trusted screen.** The setup switches to the trusted screen for real.
There the user chooses the security phrase and picture and confirms with their
password, and reads: "This is the trusted screen. When P@x asks for something
only you can approve, TUR brings you here. P@x cannot see or use it. If you
are ever asked for your password for P@x somewhere that does not show this
phrase and picture, do not type it." Then back to the desktop:

> **P@x is ready**
> Hold Meta+Space and speak, or tap it and type.
> Try: "Show me where to change the wallpaper."
> [Done]

Until the trusted screen exists (task D-16), step 7 is omitted and P@x is
limited to classes 0 and 1, as the AI control layer requires.

### 9.13 What can be built today, and what is blocked upstream

| Element | On KWin 6.8 as shipped | Blocked until |
|---|---|---|
| Summoning (shortcut, tray, launcher, file manager) | Yes | — |
| Conversation surface | Yes | — |
| Activity indicator | Yes, as a client surface with the lease rule | A compositor-enforced indicator: KWin work |
| P@x's pointer, as something the user sees | Yes | — |
| P@x's pointer and focus independent of the user's, when acting through the screen | **No.** One seat, one cursor. Interim: turn-taking | Multi-seat in KWin (open issue, no implementation [K3]) |
| Highlight in TUR's own applications | Yes | — |
| Highlight in other applications | Yes, approximate | A supported window-geometry interface would make it sturdier [K6] |
| Pointing at a row in KDE System Settings | Partly: by accessibility name ([section 10.4](#104-kdes-settings)) | Stable identifiers for controls, and deep links, in System Settings |
| Class 2 approval sheet | Yes, protected by TUR's own gating and a capture-exclusion rule | Input immunity: a per-surface "physical devices only" filter in KWin [K3] |
| Trusted confirmation (class 3) | Yes, **as a separate trusted screen**; to be proven by prototype | An in-desktop version needs the same KWin filter plus per-consumer capture exclusion |
| Stop control that cannot be suppressed | Partly (global shortcut) | The witness service (TUR's decision), or a non-rebindable physical-only shortcut in KWin |
| Excluding applications from what P@x sees | Yes: one-window capture, and blanking for whole-screen capture | Per-consumer capture exclusion in KWin [K4] |
| Repair view, memory, schedules, spending, setup | Yes (user interface); they depend on milestones A5 to A7 | — |

Proposals to make upstream, in order of value to TUR. They are contributions
to KWin under its licence, not TUR code:

1. A way to mark a surface as accepting physical input devices only.
2. Capture exclusion per consumer, and for layer-shell surfaces; completing the
   client protocol that is in draft [K4].
3. A separate seat (pointer and focus) for an emulated-input client.
4. An indicator, drawn by the compositor, while emulated input or capture is
   active.

## 10. Settings

### 10.1 A settings program of TUR's own, beside KDE's

**Decision.** TUR's settings are in a standalone program, `tur-settings`. KDE
System Settings stays as it is, for everything KDE configures. The two link to
each other.

The alternatives, and why not:

| Option | Verdict |
|---|---|
| TUR pages as modules inside KDE System Settings | A module is code loaded by a GPL program. Every TUR settings page, including those that govern P@x, would have to be GPL (AGENTS.md). Rejected |
| One TUR program that replaces System Settings entirely | Would mean rebuilding displays, network, sound, printers, input, users, accessibility and more, each of which KDE maintains well. Against the project's principle of integrating before building. Rejected |
| **A TUR program for TUR's pages; KDE's for KDE's; cross-linked** | Chosen |

The cost is real and is stated: there are two settings programs. It is reduced
in four ways.

1. **TUR Settings appears inside KDE System Settings.** A desktop file in
   System Settings' external-modules directory adds an entry, "TUR", to its
   side list. Choosing it starts `tur-settings`; System Settings shows "TUR is
   an external application and has been automatically launched" [S2]. This is
   configuration, not a module.
2. **KDE System Settings appears inside TUR Settings.** The side list of
   `tur-settings` ends with "Displays, sound, network and more", which opens
   System Settings.
3. **Search crosses both.** The search field of `tur-settings` and the
   launcher's search find a setting wherever it lives and open the right
   program at the right place (10.3, 10.4).
4. **The launcher has one favourite, "Settings"**, which is `tur-settings`'s
   front page: the state of the system at a glance and the way into both.

### 10.2 Pages

```text
┌ Settings ────────────────────────────────────────────────────────────────────┐
│ [ Search settings          ] │ P@x ▸ Privacy                                 │
│                              │                                               │
│ Overview                     │ What P@x may see                              │
│ Updates                   ●  │  P@x sees          ◉ Only the window it is    │
│ Recovery                     │                      working in               │
│ Applications                 │                    ○ The whole screen when a  │
│ Windows applications         │                      task needs it            │
│ Android                      │ ╔═══════════════════════════════════════════╗ │
│ Storage                      │ ║ Private mode                      [ off ] ║ │
│ Devices                   2  │ ╚═══════════════════════════════════════════╝ │
│ P@x                       ▾  │  While on, P@x cannot see the screen or any   │
│   General                    │  window.                                      │
│   Voice                      │                                               │
│   Privacy                 ◀  │ Applications P@x never sees or touches [Add…] │
│   Approvals                  │  KeePassXC · KWallet · …                      │
│   Memory                     │                                               │
│   Schedules                  │                                               │
│   Spending                   │                                               │
│   Activity                   │                                               │
│ Diagnostics                  │                                               │
│ Appearance                   │                                               │
│ About                        │                                               │
│ ──────────────────────────── │                                               │
│ Displays, sound, network     │                                               │
│ and more  →                  │                   tur.pax.privacy#private-mode│
└──────────────────────────────────────────────────────────────────────────────┘
```

(The double frame is the highlight of 9.5, drawn by the program itself. The
address at the bottom right is shown only in developer mode.)

| Page | Address | Holds | Changing it |
|---|---|---|---|
| Overview | `tur.overview` | Version and build, update state, health of the last start, anything needing attention | — |
| Updates | `tur.updates` | [Section 11.1](#111-updates-and-rollback): state, automatic download, metered connections, what is new | User; applying an update is class 3 for P@x |
| Recovery | `tur.recovery` | Versions on this computer, going back, the record of each start, safe graphics mode | Class 3 |
| Applications | `tur.apps` | The router's three preferences: "Never run applications in a virtual machine" (with what is lost), "Prefer open-source editions", and the list of applications pinned to a way of running, each linking to its page; which program opens `.exe`, `.apk` and package files | Class 1 |
| Windows applications | `tur.windows` | Compatibility layer: versions and per-application pins. Windows (virtual machine), from [windows-guest.md](windows-guest.md): state, Windows version, activation and end-of-support date as Windows reports them; memory and processors; "Keep Windows ready", "Free memory quickly" and the two timers, with the sentence that a resting Windows does nothing; shared folders, each read-only or read and write; clipboard, sound and printers; snapshots (take, name, restore: "Everything written inside Windows since then is lost. Files in shared folders are not restored."); the weekly maintenance time; "Show the Windows screen"; "Remove Windows…", which lists what will be deleted and its size | Class 1 for timers and sharing; class 2 for restore and installation; setting up and removing need the user's authentication |
| Android | `tur.android` | On or off; image version; Google Play (links to the application centre's flow); shared folders; clipboard sharing; keep running in the background | Class 2 to switch on; class 1 otherwise |
| macOS (experimental) | `tur.macos` | Shown only when the feature is enabled: on or off, the published list of what works | Class 2 |
| Storage | `tur.storage` | Volumes and their state, with the messages of [section 11.3](#113-storage-prompts); per-volume policy | Class 1; formatting and encryption are class 3 |
| Devices | `tur.devices` | [Section 11.4](#114-the-device-centre) | Read-only; hand-off is class 3 |
| P@x ▸ General | `tur.pax.general` | On or off; provider, model and key; what P@x may see and control | **Class 4 for P@x.** The user changes these; loosening is confirmed on the trusted screen |
| P@x ▸ Voice | `tur.pax.voice` | Push-to-talk key; wake word; voice and speed; spoken approval of class 2 | As above |
| P@x ▸ Privacy | `tur.pax.privacy` | [Section 9.9](#99-privacy-controls) | As above |
| P@x ▸ Approvals | `tur.pax.approvals` | Mode (normal, ask every time, only look); applications where P@x need not ask; standing approvals | As above |
| P@x ▸ Memory | `tur.pax.memory` | [Section 9.11](#911-memory-schedules-and-spending) | Forgetting is class 1; "forget everything" is the user's |
| P@x ▸ Schedules | `tur.pax.schedules` | Reminders, scheduled tasks, "while you were away" | P@x proposes (class 2); the user creates |
| P@x ▸ Spending | `tur.pax.spending` | Today, this month, the limits | Class 4 for P@x |
| P@x ▸ Activity | `tur.pax.activity` | The audit record as a readable list: every task, every action, its class and decision; repair sessions and what they changed | Read-only |
| Diagnostics | `tur.diagnostics` | Health of the last start, the first-start self-test's results, "Create a support bundle" | — |
| Appearance | `tur.appearance` | Dark, light, or light by day and dark at night; everything else links to KDE's appearance pages | Class 1 |
| About | `tur.about` | Version, build and base; licences and third-party notices; the statement about Fedora ([section 12.2](#122-what-the-user-is-told)) | — |

Every page shows the state first and the controls second, in the two-layer
manner: plain words on top, "Technical detail" for the rest.

### 10.3 The addressing scheme

The AI control layer requires that "every settings page and each row on it has
a stable identifier" (§16.6). The scheme serves more than P@x: the launcher's
search, links in notifications and error messages, the documentation, and
support ("open `tur.recovery`") all use it.

**Addresses.**

```text
address   = page [ "#" row ]
page      = namespace "." segment *( "." segment )
namespace = "tur" | "kde"
segment   = 1*( a-z | 0-9 | "-" | "_" )
row       = 1*( a-z | 0-9 | "-" )

tur.pax.privacy                    a page
tur.pax.privacy#private-mode       a row on it
kde.kcm_nightlight                 a page of KDE System Settings
kde.kcm_nightlight#switching-times a row on it
```

**Rules.**

1. An address, once released, keeps its meaning for ever and is never reused,
   exactly as an error code (§29.3). A row that moves or is renamed keeps its
   address; a row that is removed leaves an entry marked `removed`, with
   `replaced_by` where there is a successor.
2. Addresses are lower-case ASCII, are not translated, and are never shown to
   the user except in developer mode.
3. Every row has one. A page cannot be merged if it contains a control with no
   address.

**The registry.** Each program that has addressable content ships a registry
file, installed under `/usr/share/tur/settings/`, conforming to a new schema,
`schemas/settings-registry.schema.json`. Illustrative:

```json
{
  "schema_version": 1,
  "program": "org.tur.Settings",
  "pages": [
    {
      "page": "tur.updates",
      "title": "Updates",
      "path": ["Updates"],
      "rows": [
        {
          "id": "download-automatically",
          "title": "Download updates automatically",
          "keywords": ["auto update", "background download"],
          "kind": "switch",
          "change": { "class": 1, "tool": "update_set_automatic_download" }
        }
      ]
    },
    {
      "page": "tur.pax.privacy",
      "title": "Privacy",
      "path": ["P@x", "Privacy"],
      "rows": [
        {
          "id": "private-mode",
          "title": "Private mode",
          "keywords": ["stop watching", "hide my screen"],
          "kind": "switch",
          "change": { "class": 4 }
        }
      ]
    }
  ]
}
```

- `title` and `keywords` are source strings, translated with the program.
- `kind` is one of `switch`, `choice`, `number`, `text`, `list`, `action`,
  `information`.
- `change.class` is the action class of changing the setting, as the AI
  control layer defines it. Pointing at a setting is always class 0. `tool`
  names the typed tool that changes it, when one exists; P@x uses the tool,
  not the screen, whenever it can (AI control layer, principle 3). Class 4
  means P@x can open the page and point at the row, and nothing more: this is
  how "including the settings that govern P@x itself" is met.

**The interface.** Two D-Bus interfaces on the session bus, proposed, to be
frozen before work depends on them (§16).

```text
org.tur.Addressable1        implemented by every TUR program with addressable content
  Methods   Reveal(s address, a{sv} options) -> a{sv} result
            ClearHighlight()
            List() -> s                        this program's registry, as JSON
  Signals   RowChanged(s address)              the row's value or state changed

org.tur.Settings1           implemented by tur-settings; started on demand
  Methods   Open(s address)                    open the page; no highlight
            Reveal(s address, a{sv} options) -> a{sv} result
            Resolve(s address) -> s            registry entry, with the owning program
            Search(s text, u limit) -> a(ssd)  address, title, score
```

- `Reveal` opens the owning program if needed, shows the page, scrolls the row
  into view, moves keyboard focus to it, and draws the highlight. Options:
  `highlight` (default true), `duration` in seconds (0 keeps it until
  `ClearHighlight`), `dim` (dim the rest of the page). The result holds the
  row's rectangle relative to the program's window, the program's process and
  application identifiers, and the window's title, which is what
  `tur-pax-overlay` needs to bring P@x's pointer to the row
  ([9.5](#95-highlighting)).
- `org.tur.Settings1` is the single entry point: it resolves any address,
  `tur` or `kde`, from the merged registries and forwards to the owner.
- `RowChanged` is how guide mode knows that the user did the step.
- From the command line: `tur-settings tur.pax.privacy#private-mode`. As a
  link: `tur-settings:tur.pax.privacy#private-mode`, registered as a URI
  scheme in the program's desktop file.
- Errors are D-Bus errors whose message starts with a `TUR-DESKTOP-SETTINGS-*`
  code (the `DESKTOP` family already holds the catalogue's codes under their
  own topic): unknown address, removed address (with its successor), page not
  available on this system.

**In the code.** `tur-kit` provides `SettingsPage` and `SettingRow`. A row
without an `address` does not compile into a page (the component requires the
property), and a test compares every page's rows with its registry file in
both directions. A second test compares the registry with the one on the main
branch and fails if an address disappeared without a `removed` entry.

**Tools for P@x**, declared in the manifest of `tur-settings`: `settings_search`
(class 0), `settings_reveal` (class 1: it opens a window), `settings_clear`
(class 0). Changing a value is always the tool named in the row's `change`
entry, with that entry's class.

### 10.4 KDE's settings

KDE System Settings is not TUR's code, so its addresses are a mapping that TUR
maintains, and its precision has limits that must be stated.

**Pages: exact.** `kde.<module>` opens that module:
`systemsettings kcm_nightlight`, or the `systemsettings://` link; a running
System Settings switches to it [S1]. Module identifiers are KDE's own and are
listed by `systemsettings --list`. Where a module takes arguments that select
an item (a network connection, an application's notifications), the registry
records them [S3].

**Rows: by the accessibility tree, for a curated list.** Nothing in System
Settings addresses a control from outside [S4]. TUR ships
`kde-settings-map.json`, a registry in the same schema whose rows carry a
*locator* instead of belonging to a TUR page:

```json
{
  "page": "kde.kcm_nightlight",
  "title": "Night Light",
  "open": { "module": "kcm_nightlight" },
  "verified_with": "plasma-6.8.2",
  "rows": [
    {
      "id": "switching-times",
      "title": "Switching times",
      "locator": {
        "accessible_id": "…",
        "role": "combo box",
        "label_catalog": "kcm_nightlight",
        "label_msgid": "Switching times:"
      },
      "change": { "class": 1 }
    }
  ]
}
```

(The values are illustrative; none was read from the module.)

- The locator is tried in order: the control's accessibility identifier (Qt
  6.8 and later expose one, defaulting to the control's QML `id` [S5]); then
  its role together with its label, where the label is looked up in the
  module's own translation catalogue so that the match works in every
  language.
- The control's rectangle comes back relative to the window, and
  `tur-pax-overlay` adds the window's position ([9.5](#95-highlighting)).
- The list is curated, not complete: the settings people most often ask
  about (display scaling and resolution, Night Light, wallpaper, pointer
  speed, tap to click, keyboard layouts, wireless networks, Bluetooth, sound
  output and input, printers, sleep and screen locking, date and time,
  language, default applications, users, accessibility). It starts at about
  forty rows and grows by need.
- **It is tested, because it will break.** KDE does not promise that a
  control's identifier or label is stable. An end-to-end test opens every
  mapped page in the image's own System Settings, resolves every locator, and
  fails if one does not match exactly one control. The test runs on every
  image build, so a Plasma update that moves a control is caught before users
  see it.
- **When a locator fails on a user's machine**, P@x says so and degrades
  honestly: it opens the page, says "It is on this page, under 'Switching
  times'", and, if screen access is on, finds the control from the picture.

For applications to expose the accessibility tree, the session's accessibility
flag must be on [S5]. `tur-pax` turns it on while P@x is enabled. Its cost to
running applications was not measured.

**What would make this exact** is upstream work, to be proposed: stable
accessibility identifiers for the controls of settings modules, and an
argument to open a module at a control. There is an open KDE request about
jumping to a search match, on which a maintainer commented that it "would
require a ton of engineering" [S4]; TUR's mapping file and its test are the
kind of evidence that makes such a proposal concrete.

### 10.5 How P@x shows a setting

The exit criterion of milestone A4b ("P@x opens the right settings page and
points at the exact row in a VM test") is met by this sequence:

1. The user asks: "where do I turn off the screen dimming?"
2. P@x calls `settings_search`. The answer is an address,
   `kde.kcm_powerdevilprofilesconfig#dim-screen` (illustrative), with its
   title and path.
3. P@x calls `settings_reveal` with the address. `org.tur.Settings1` opens the
   owning program at the page and returns the row's rectangle, or the locator
   resolves it in System Settings.
4. `tur-pax-overlay` moves P@x's pointer to the row and draws the highlight
   (for a TUR page the program has already drawn its own).
5. P@x says where it is, in words that match what is on screen: "It's here, in
   System Settings under Power Management: 'Dim screen'." In "do it for me",
   it then changes the setting through the row's tool, or, where there is
   none, through the accessibility tree, subject to the row's class.

Nothing in steps 1 to 4 changes a setting. Each is class 0 or class 1.

## 11. System experiences

These are the moments at which TUR has to keep its two promises: that it tells
the user what happened, and that an update cannot break it. Each is specified
with the exact text. Text that belongs to an error code is owned by that
code's registry entry (§29.3); the user interface displays it and does not
rephrase it.

### 11.1 Updates and rollback

The update service (`org.tur.Update1`, in development) owns the behaviour. The
desktop shows its state in three places: a notification from `tur-notifier`, a
tray item that exists only while there is something to say, and Settings ▸
Updates (`tur.updates`).

Design rules (judgements, to be confirmed with the update service's design):

- An update never interrupts. It is downloaded and checked in the background
  and takes effect at a restart the user chooses.
- There is no "installing updates, do not turn off" screen. A bootc update is
  staged beside the running system and finalised during a normal shutdown.
- One notification per update, repeated at most once a day while a restart is
  pending. No countdown, no forced restart.
- After a rollback the system does not try the same update again by itself.

**An update is ready.**

> **TUR 0.4.0 is ready**
> It has been downloaded and checked. It will be used the next time you
> restart. The version you are using now stays on this computer in case the new
> one does not start.
> [Restart now]   [Later]   [What is new]

**Settings ▸ Updates**, one state at a time:

| State | Text |
|---|---|
| Up to date | TUR is up to date. Version 0.3.2. Last checked today at 09:14. [Check now] |
| Checking | Checking for updates… |
| Downloading | Downloading TUR 0.4.0 · 310 MB of 1.2 GB. You can keep working. [Pause] |
| Verifying | Checking the download. |
| Ready | TUR 0.4.0 is ready. Restart to use it. [Restart now] [What is new] |
| Not applied | The update was not applied. _(two-layer message with the `TUR-UPDATE-*` code: signature, checksum, space or network)_ [Try again] |
| Offline | TUR cannot check for updates without a network connection. Nothing is wrong. |

Below the state: "Versions on this computer", the same list as Settings ▸
Recovery, and the switches "Download updates automatically" (on) and "Download
on metered connections" (off).

**The update was applied.** After the first login on the new version, when the
health gate (§5.4) has marked it good, one low-priority notification:

> **You are now using TUR 0.4.0**
> The previous version, 0.3.2, is kept. If something is not right, you can go
> back in Settings ▸ Recovery.
> [What is new]

**The update was rolled back.** This is the moment the product exists for, so
it gets the most care. Three surfaces, in the order the user meets them:

1. *During start-up*, if the boot splash is showing when the health gate gives
   up: "The new version did not start properly. Returning to the previous
   version." This needs the boot-health service to send the message to the
   splash before it restarts the machine: an **assumption** to settle in that
   service's design.
2. *On the login screen:* no banner. The login screen is Plasma Login Manager
   and TUR does not add code to it. (The desktop notification below is the
   reliable place.)
3. *After login*, a notification that does not expire until dismissed, and the
   same content at the top of Settings ▸ Updates until the user dismisses it:

> **TUR went back to version 0.3.2**
> The update to 0.4.0 did not start properly, so TUR returned to the version
> that was working. Your files, applications and settings are as you left
> them.
>
> **What happened**
> The desktop did not start within the time allowed.
>
> **What you can do**
> Nothing is needed. TUR will not install this update again by itself. When a
> corrected update is published, it will be offered as usual.
> [See what happened]   [Create a support bundle]   [Try the update again]
>
> Technical detail ▸ `TUR-BOOT-120` · deployment `3f9c…` · 3 attempts · failed
> unit `plasmalogin.service` · evidence kept in the journal of boot `a41e…`

The "What happened" sentence comes from the registry entry of the stage that
failed (§27.1). "See what happened" opens the record of the failed start in
Settings ▸ Recovery: the stages reached, the stage that failed, the failed
units, with "Create a support bundle" beside it. The evidence is kept until the
user removes it (§27.5).

The wording replaces the shorter message of §27.5 ("The update did not pass
boot checks. Previous version restored.") with the same meaning; this is listed
in [section 18](#18-amendments-to-specification-revision-10).

**Going back by choice.** Settings ▸ Recovery (`tur.recovery`):

```text
Versions on this computer
 ● TUR 0.4.0   in use            installed 12 November 2026   build 20261112.g9068a9b
 ○ TUR 0.3.2   previous, works   last used 11 November 2026   [Restart into this version]

Start-up
 Safe graphics mode is available from the start-up menu.     [How to open the start-up menu]
 Last start: 13.0 s to the login screen · all checks passed  [Show the record]
```

"Restart into this version" is a class 3 operation whoever asks for it. The
prompt before authentication:

> **Go back to TUR 0.3.2?**
> The computer restarts into version 0.3.2. Your files and applications stay
> as they are. System settings changed since the update (network connections
> for all users, accounts, passwords) go back to what they were. Version 0.4.0
> is kept, and you can return to it here.
> [Cancel]   [Authenticate and restart]

### 11.2 Diagnostics and the support bundle

`tur-diagnostics` is a small standalone program with one job, so that it still
works when larger things do not. It drives `tur-diag` (today through its
command line; through `org.tur.Diagnostics1` when that exists). The same
component is embedded in the installer's drawer and in Settings ▸ Diagnostics.

Entry points: Settings ▸ Diagnostics; "Report a problem" in the launcher; the
"Create a support bundle" button on every error message; the installer; an icon
on the safe-graphics desktop (§27.3); P@x (`diagnostics_collect`, which
produces the file and never sends it: sending is class 2, section 16.4 of the
AI control layer).

**Screen 1: what this is.**

> **Create a support bundle**
> A support bundle is one file that describes the state of this computer, so
> that someone else can work out what went wrong.
>
> **Nothing is sent anywhere.** The file is written to this computer. You
> decide who gets it.
>
> **In the bundle:** the TUR version and build, system and start-up logs,
> hardware and driver information, storage layout, the state of TUR's
> services.
> **Never in the bundle:** your documents, passwords and keys, browser data,
> the contents of your applications, P@x conversations and screenshots.
> **Replaced before the file is written:** your user name, the computer's
> name, serial numbers, network addresses.
>
> Detail: ◉ Standard   ○ Technical (more logs, for a developer)
> ☐ Only since the computer was last started
> ☐ Include complete system logs (asks for your password)
> Save to: `~/Documents/tur-support-20261112-a41e.tar.zst`   [Change…]
> [Cancel]   [Create]

The lists are generated from the collector table and the "never collected" list
of [diagnostics.md](diagnostics.md), not written by hand, so that the screen
cannot drift from what the collector does. Without the last tick box the bundle
is collected as the user, and whatever was refused is listed on screen 3.

**Screen 2: progress.** "Collecting · 14 of 31 · storage layout". The count is
real: the collector list is known in advance. There is no percentage.
[Cancel] leaves no file behind.

**Screen 3: review.**

> **The support bundle is ready**
> `tur-support-20261112-a41e.tar.zst` · 3.8 MB · in Documents
>
> **Replaced:** 42 values (user name 12, computer name 9, serial numbers 6,
> network addresses 15)
> **Could not be collected:** 3 items ▸
> [Show in Files]   [Look inside]   [Copy the path]   [Done]
>
> To share it, attach the file to your report or send it to the person helping
> you. TUR does not upload it.

"Look inside" lists the files in the bundle and opens any of them read-only,
with the redaction report first. A person can see exactly what they are about
to hand over. Failures show the `TUR-DIAG-*` code as a two-layer message.

### 11.3 Storage prompts

Storage decisions are made by `tur-storage`; its registry holds the text. The
desktop's part is to deliver that text at the right moment, in the right
place, without offering anything unsafe.

**The hibernated Windows disk.** The user opens a Windows disk in the file
manager. TUR mounts it read-only. Two things appear.

A notification, at normal priority:

> **Windows disk opened read-only**
> "Windows (C:)" was left hibernated by Windows, so TUR will not write to it.
> You can open and copy files from it.
> [Why?]   [Open]

"Why?" opens the volume's page in Settings ▸ Storage, which shows the registry
message in full, in the two-layer form:

```text
┌ Windows (C:) · 476 GB · NTFS · Samsung SSD 980 PRO ──────────────────────────┐
│ Opened read-only                                                             │
│                                                                              │
│ The Windows volume was not mounted for writing.                              │
│                                                                              │
│ Reason                                                                       │
│ The volume appears to be hibernated or in an unsafe state.                   │
│                                                                              │
│ What to do                                                                   │
│ Start Windows, turn off Fast Startup and hibernation, shut down fully, then  │
│ try again. TUR mounted the volume read-only to protect your data.            │
│                                                                              │
│ [How to turn off Fast Startup]   [Try again]   [Open read-only]              │
│                                                                              │
│ Technical detail ▾                                    TUR-STORAGE-NTFS-004   │
│   Device: /dev/disk/by-uuid/01D9A7C2E4F6B801                                 │
│   Driver: ntfs3      Mount policy: read-only                                 │
│   Evidence: filesystem=ntfs; ntfs_state=hibernated         [Copy]            │
└──────────────────────────────────────────────────────────────────────────────┘
```

- There is no "open for writing anyway" button, here or anywhere (§6.3). "Try
  again" asks the service for a fresh decision.
- "How to turn off Fast Startup" is a help page shipped with TUR, readable
  offline.
- The file manager is Dolphin, which TUR does not modify. Its own read-only
  indication stays as it is.

Other storage prompts follow the same pattern, each with its registry text:

| Situation | Code | Notification title | Offered |
|---|---|---|---|
| NTFS volume not shut down cleanly | `TUR-STORAGE-NTFS-005` | Windows disk opened read-only | Why? · Open |
| BitLocker volume | `TUR-STORAGE-BITLOCKER-001` | This disk is encrypted by Windows | Why? |
| Locked encrypted volume | `TUR-STORAGE-LUKS-001` | This disk is locked | Unlock… (the passphrase is asked by the system's authentication prompt, never by this window) |
| Apple volume | `TUR-STORAGE-APFS-001` | Apple disk opened read-only | Why? · Open |
| Write-protected medium | `TUR-STORAGE-MEDIA-001` | This disk is write-protected | Open |
| Unknown filesystem | `TUR-STORAGE-FS-002` | TUR does not recognise what is on this disk | Details (never "format") |
| Removed while in use | to be assigned | "_Name_" was removed while it was in use | What may have been lost ▸ |

Formatting, partitioning and encryption are not offered from a prompt. They are
class 3 operations reached deliberately from Settings ▸ Storage, with the
destructive confirmation of [section 11.5](#115-the-installer).

### 11.4 The device centre

Settings ▸ Devices (`tur.devices`) shows what `tur-device` knows
([device-manager.md](device-manager.md)). It reads; it changes nothing.

```text
Devices                                             [ Search devices          ]
 ! 2 devices need attention
 ───────────────────────────────────────────────────────────────────────────────
 ! Wireless network   Intel Wi-Fi 6 AX201           Firmware file could not be loaded
 ! Fingerprint reader Unknown device 27c6:55a2      No driver for the device
 ───────────────────────────────────────────────────────────────────────────────
 Graphics        ✓ AMD Radeon 780M                  2 displays connected
 Sound           ✓ Family 17h HD Audio
 Network         ✓ Realtek RTL8125 2.5GbE           Connected
 Storage         ✓ Samsung SSD 980 PRO 1TB
 Input           ✓ 4 devices
 Bluetooth       ✓ Intel AX201 Bluetooth            On
 Cameras         ✓ Integrated Camera
 Thunderbolt     – None
 Firmware        ↑ 1 update available
 This computer   LENOVO 21JK · UEFI · Secure Boot on
```

Selecting a device opens its page: what it is, the driver ("Driven by `iwlwifi`,
part of Linux"), its state, and any finding as a two-layer message with the
registry's next step:

> **Firmware file could not be loaded** · `TUR-DEVICE-FIRMWARE-001`
> The wireless network device asked for a firmware file that is not on this
> computer, and it is not working.
> **What to do:** install updates and restart. If it still does not work,
> create a support bundle and report it. Do not copy firmware from other
> sources.
> [Open Updates]   [Create a support bundle]   Technical detail ▸

Rules:

- Status is an icon and a word, never a colour alone.
- Serial numbers and hardware addresses are hidden. "Show serial numbers"
  asks for authentication, as the service requires (`read-sensitive`).
- No finding suggests a driver made for another operating system (§10.6).
- Firmware updates and handing a device to the Windows guest are class 3 and
  are separate work. When hand-off exists, a device page shows its state in the
  words of the state machine: "Used by this computer", "Being handed to
  Windows…", "Used by Windows", "Being returned…", "Unavailable: Windows did
  not release it".
- Printers, displays, Bluetooth pairing, Thunderbolt authorisation and input
  settings stay in KDE System Settings; the device page links to the right
  module.

### 11.5 The installer

[ADR-0007](../adr/0007-app-centric-routing.md) defers TUR's own graphical
installer until the standard one has been outgrown, and the installer core
([installer.md](installer.md)) does not yet write to a disk. The screens are
designed now so that the core's data model and the user interface agree; the
program, `tur-installer`, is the last task in the plan.

**Layout.** Every screen has the same three regions. The diagnostics drawer is
part of the window from the first screen to the last (§28.4).

```text
┌──────────────────────────────────────────────────────────┬───────────────────┐
│ TUR installer                              Step 3 of 6   │ Diagnostics     ▸ │
│                                                          │                   │
│                                                          │ Now               │
│                    (the current screen)                  │  Finding disks    │
│                                                          │ Done              │
│                                                          │  ✓ Starting       │
│                                                          │  ✓ Checking this  │
│                                                          │    computer       │
│                                                          │ Warnings      1 ▸ │
│                                                          │ Last action       │
│                                                          │  lsblk --json     │
│                                                          │ [Detailed log]    │
│ [Back]                                       [Continue]  │ [Create a support │
│                                                          │  bundle]          │
└──────────────────────────────────────────────────────────┴───────────────────┘
```

The drawer shows the current phase, completed phases, warnings, the last
command or component, and the two buttons. It can be narrowed to a strip that
still shows the current phase and a warning count; it cannot be closed.
"Detailed log" opens the session log in a pane that follows the output, with
the phase records of the session directory, without leaving the installer. On
a screen narrower than 1100 px the drawer is a strip by default.

**Screens.**

| # | Screen | Content | Phases of installer.md |
|---|---|---|---|
| 1 | Welcome | Language, keyboard, and an "Accessibility" button (screen reader, larger text, high contrast). "This installer can erase a disk. It will show you exactly what it is going to do, and it will not change anything until you confirm on the last screen before installing." | `START` |
| 2 | This computer | The preflight checks as a list, each with pass, warning or blocking, its code, and a two-layer message: start-up mode (UEFI), Secure Boot state, processor, memory, graphics mode, network. A blocking check disables "Continue" and says what to do. | `ENVIRONMENT_CHECK`, `HARDWARE_INVENTORY` |
| 3 | Choose the disk | Every disk with model, size, how it is connected, and what is on it ("Windows 11 and 212 GB of files", "Empty", "This is the disk the installer is running from", which cannot be chosen). No device path is shown as the name of a disk. | `STORAGE_DISCOVERY`, `TARGET_SELECTION` |
| 4 | The plan | Before and after, side by side: existing partitions and filesystems; the new layout. Encryption choice with the key-recovery explanation. The start-up configuration that will be written. Works offline; a network is asked for only if the user chooses something that needs one. | `PARTITION_PLAN` |
| 5 | Confirm | The destructive confirmation below. | `USER_CONFIRMATION` |
| 6 | Installing | Phases grouped for a person, each expandable to the real phases and their logs. Then the result. | `PARTITIONING` to `COMPLETE` |

**Screen 5: the destructive confirmation** (§28.5). It is the only screen with
a red frame, and it has no default button.

```text
┌ Erase this disk and install TUR ─────────────────────────────────────────────┐
│                                                                              │
│ THE DISK                                                                     │
│   Samsung SSD 980 PRO 1TB · 931.5 GiB · NVMe                                 │
│   Serial number  S5GXNX0T7K2M4F2K   (identifies this disk; think before      │
│                                      sharing a picture of this screen)       │
│   Stable path   /dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T7K2M4F2K│
│                                                                              │
│ ON IT NOW                                 AFTER INSTALLING                   │
│   1  EFI system    100 MiB  FAT32           1  EFI system   1 GiB   FAT32    │
│   2  Microsoft reserved 16 MiB              2  Boot         1 GiB   ext4     │
│   3  Windows (C:)  930 GiB  NTFS            3  TUR          929 GiB btrfs,   │
│   4  Recovery      750 MiB  NTFS                             encrypted       │
│                                                                              │
│ WHAT WILL BE DESTROYED                                                       │
│   Everything on this disk: all four partitions, including                    │
│   ● Windows 11, which was detected on partition 3                            │
│   ● all files on "Windows (C:)"                                              │
│   No user data on this disk is kept.                                         │
│                                                                              │
│ ENCRYPTION                                                                   │
│   The disk will be encrypted with the passphrase you chose. If you forget    │
│   it, nobody can recover your files, including the TUR project. You will be  │
│   shown a recovery key to write down after installation.                     │
│                                                                              │
│ To confirm, type   ERASE 7K2M4F2K                                            │
│ [                                  ]                                         │
│                                                                              │
│ [Back]                                            [Erase disk and install]   │
└──────────────────────────────────────────────────────────────────────────────┘
```

- Every line of this screen is taken from the plan's confirmation text and
  consequences, which the installer core computes. The user interface does not
  compose it.
- The phrase is the one the core derives from the disk's identity, so it
  differs from disk to disk. The button is enabled only when the phrase matches
  exactly; pasting is disabled in the field; Enter does not activate the button.
- If the disk's contents or identity change while the screen is open, the
  screen is replaced by "This disk has changed since you were shown the plan."
  with the `TUR-INSTALL-IDENTITY-*` or `-CONFIRM-004` message, and the user
  returns to screen 3.
- The layout shown under "after installing" is illustrative; the partition
  scheme is an unverified assumption of the installer core.

**Screen 6: progress.**

```text
Installing TUR
 ✓ Preparing the disk             3 steps       12 s
 ● Copying TUR to the disk        Writing the system image · 2.1 GiB of 5.4 GiB
 ○ Making the computer start TUR
 ○ Checking the installation

[Cancel is not available while the disk is being written. Why?]
```

- A phase with a known total shows it. A phase without one shows an
  indeterminate indicator and its activity ("Creating the encrypted volume").
  There is no overall percentage.
- "Cancel" is offered while cancelling is safe, and replaced by the sentence
  above once a destructive phase has started.
- **Failure.** The failed phase turns into a two-layer message with its code
  and "what state the disk is in now", then the choices the phase's class
  allows: "Try this step again" (only for a phase that may be retried), "Create
  a support bundle", "Restart the computer", and guidance: "The disk has been
  partly written. It will not start. Your other disks were not touched."
- **Success.** "TUR is installed. Remove the installation medium and restart."
  [Restart now] [Create a support bundle] [Show the installation record].
- **An interrupted installation**, on the next start of the installer:
  "Previous installation did not finish." with what was reached and the option
  to examine the disk; it never continues by itself.

## 12. Replacing Fedora's marks

**No image may be published until this section is done** (AGENTS.md; issue
#11; [licence review](../dependencies/licence-review.md), finding F5). This is
not legal advice; the open points go to the review the specification requires
(§17.2).

### 12.1 The rule and the approach

Fedora's guidelines require that a system which mixes or modifies Fedora's
packages removes `fedora-logos`, `fedora-release` and `fedora-release-notes`,
which may be replaced "with packages of your own devising not containing the
Fedora Trademarks" [F1]. `fedora-logos` is also restricted by its copyright
licence to accepted Fedora spins [F2]. Beyond the three packages, anything
that *presents* the system as Fedora (name, logo, wallpaper, default theme,
start page) is replaced, and Fedora's infrastructure is not used in TUR's
name.

How other Fedora-based systems do it was checked [F5]. Ultramarine and Nobara
ship their own release and logos packages. The Universal Blue images rewrite
`os-release` and swap the logos package but keep Fedora's release package,
which the guideline text does not allow; Fedora contributors have noted that
the requirement "is frequently not observed" [F6]. TUR follows the text:

1. **Two small packages of TUR's own**, built in CI and installed in the image
   build, so that the package database stays consistent and every dependency
   on `system-release` and `system-logos` is met:
   - `tur-release`, started from Fedora's `generic-release` (MIT), which exists
     for this purpose [F3]. It provides `system-release`, carries TUR's
     `os-release`, and keeps the two things a Kinoite system needs from the
     packages it replaces: the systemd presets (by depending on Fedora's
     preset packages) and the polkit rule for rpm-ostree, which
     `generic-release` lacks [F3].
   - `tur-logos`, providing `system-logos`, with TUR's artwork under the file
     names other packages look for.
2. **A removal list** for packages whose only purpose is Fedora's
   presentation, and replacement of the defaults that point at them.
3. **A squashed or re-chunked final image**, so that the removed files are not
   still present in an earlier layer of what is published (an **assumption**
   about how layers are pushed, to be confirmed with the publishing workflow).
4. **A test** that fails the image build if a mark comes back (12.5).

`os-release` after the change (values for the names; versions are filled in by
the build):

```text
NAME="TUR"
PRETTY_NAME="TUR 0.4.0"
ID=tur
ID_LIKE=fedora
VERSION_ID=…                      (TUR's version)
IMAGE_ID=tur
LOGO=tur-logo-icon
DEFAULT_HOSTNAME=tur
HOME_URL, DOCUMENTATION_URL, SUPPORT_URL, BUG_REPORT_URL    the project's
CPE_NAME                           the project's
(no REDHAT_* keys, no VARIANT naming a Fedora edition)
```

Changing `ID` from `fedora` is what Fedora intends for a derived system [F6],
and it has known consequences that the task must handle: COPR's vendor file,
a GRUB helper script that derives the EFI directory from `ID`, the time
server's vendor zone, and anything in the repository that tests
`ID=fedora` [F3] [F5]. The current `Containerfile` deliberately leaves `ID`
alone "because image and update tooling still key on" it; that note is
resolved by this task, with the boot test as the judge.

### 12.2 What the user is told

Settings ▸ About, the project's website and the README carry a statement of
origin. Two forms are possible, and choosing between them is a question for
the owner and the lawyer ([section 17](#17-open-questions-for-the-owner)):

- **As a Fedora Remix.** The guidelines offer the "Fedora Remix" mark to
  exactly this kind of system, on conditions: TUR's own name more prominent, no
  implication of endorsement, and a prominent notice [F1]. The text would be:

  > TUR is a Fedora Remix. It contains software from the Fedora Project and
  > software that is not from Fedora. TUR is not provided or supported by the
  > Fedora Project. Official Fedora software is available from the Fedora
  > Project at https://fedoraproject.org/.

- **Without any Fedora mark as branding**, with a factual sentence in About
  and the third-party notices, relying on ordinary descriptive use. The
  published guidelines do not contain a sanctioned "based on Fedora" wording
  [F1], so this form needs the lawyer's view:

  > TUR is built from software packaged by the Fedora Project and many other
  > projects. TUR is not provided, endorsed or supported by the Fedora
  > Project or by Red Hat. Official Fedora software is available at
  > https://fedoraproject.org/.

Either way the README's badge "Base: Fedora Kinoite · bootc" and similar uses
are reviewed with the same question.

KDE's names and artwork are kept, unmodified, to refer to KDE's own software:
"KDE Plasma" in About, Plasma's and Breeze's icons. KDE's logo is not used as
TUR's, and "KDE" or "Plasma" does not appear in TUR's product name [F9]. The
launcher's icon is TUR's mark.

### 12.3 Network contact nobody asked for

The setup wizard says "TUR does not collect information about you"
([6.2](#62-first-run-setup)), and the specification forbids anything silent
(§0, rule 9). The Fedora base makes some contacts by default. Each needs a
decision before the sentence is true:

| Contact | In the base | Proposed for TUR |
|---|---|---|
| Counting of installations by the package tools (`countme`) | On in Fedora's atomic desktops; the user agent is built from `os-release` [F4] | **Off.** A count sent to Fedora's servers in TUR's name is neither Fedora's statistic nor something the user was told about |
| Connectivity check | `http://fedoraproject.org/static/hotspot.txt` [F4] | Replace with an address the project operates, or turn the check off until it has one. Not Fedora's server |
| Time servers | Fedora's vendor zone of the NTP Pool; the pool asks each vendor to use its own zone [F8] | The project's own vendor zone, or servers named by the network. Not Fedora's zone |
| Browser start page and bookmarks | Fedora's start page; "Firefox for Fedora" [F4] | Depends on the browser decision ([section 17](#17-open-questions-for-the-owner)) |
| Flatpak remotes | Fedora's Flatpak remotes are added by a unit at first start [F4] | Keep Flathub; the Fedora remotes are a decision for the application centre's source list (their name is a truthful description of the source) |
| Location for the time zone or Night Light | A network lookup, if enabled | Not used by setup; Night Light's automatic location stays off unless the user turns it on |
| KDE's optional usage feedback | Off by default | Stays off; KDE's welcome window, which offers it, is not shown |
| Update checks | — | TUR's update service contacts TUR's image registry. Said plainly in Settings ▸ Updates, with the switch to turn automatic checks off |

Whether each row is as described in the image as built is to be confirmed by
the task (the first column comes from package definitions, not from a running
system).

### 12.4 Checklist

**A. Must be replaced or removed before any image is published.**

| # | Item | Where | How | Checked by |
|---|---|---|---|---|
| A1 | `fedora-logos` | Package: pixmaps, `fedora-logo-icon`, `start-here` icons, boot-loader and installer art, the Plymouth watermark | Replace with `tur-logos` | Package absent; no file from it present |
| A2 | `fedora-release-common`, `-kinoite`, `-identity-kinoite`, `-ostree-desktop` | Packages | Replace with `tur-release`; keep the preset packages; ship the rpm-ostree polkit rule | Packages absent; `rpm -q --whatprovides system-release` is `tur-release`; presets and rule present |
| A3 | `os-release`: `NAME`, `PRETTY_NAME`, `ID`, `VARIANT`, `LOGO`, `CPE_NAME`, the URLs, `REDHAT_*` | `/usr/lib/os-release` | From `tur-release`, as in 12.1 | No "Fedora", "fedoraproject" or "Red Hat" in the file except `ID_LIKE=fedora` |
| A4 | `/etc/system-release` and its aliases | Release package | "TUR release _version_". The firmware's boot entry is labelled from the text before "release" [F7], so it becomes "TUR" | File content; boot report shows the entry's label |
| A5 | Boot menu titles | Generated from `PRETTY_NAME` [F7] | Follows A3 | Boot report |
| A6 | Plymouth watermark and theme | `spinner/watermark.png` from A1; default theme | TUR's theme ([6.1](#61-starting)) | Start-up screenshot |
| A7 | The copy of the logo and `os-release` inside the initramfs | `initramfs.img` | Regenerate the initramfs in the image build after A1 to A6 [P8] | `lsinitrd` shows no Fedora file; boot test passes |
| A8 | Launcher icon | `start-here` icons from A1 | TUR's icon theme ([7.1](#71-panel)) | Desktop screenshot |
| A9 | "About this system" | Logo path in `kcm-about-distrorc`; name and URL from `os-release` | TUR's `kcm-about-distrorc` | File content |
| A10 | "Powered by Fedora Linux" and Fedora's wallpaper in first-run setup | `plasma-setup` and Fedora's patch to it [P9] | `tur-setup` replaces it ([6.2](#62-first-run-setup)) | First-start screenshot |
| A11 | Default host name `fedora` | `DEFAULT_HOSTNAME` | Follows A3 | `hostnamectl` in the boot report |
| A12 | The published image's earlier layers | OCI layers | Squash or re-chunk | Scan of every layer of the published image |
| A13 | Installer and live-medium art, when one exists | Logos package | `tur-logos`; TUR's installer | With the installer |
| A14 | TUR's own name, website and domain | The project | Must not contain "Fedora", other than the intact phrase "Fedora Remix" if that form is chosen [F1] | Review |

**B. Should be replaced: presents the system as Fedora, or uses Fedora's
services in TUR's name.**

| # | Item | Where | How |
|---|---|---|---|
| B1 | The "Fedora" global themes | `plasma-lookandfeel-fedora`, which `plasma-workspace` requires [F3] | TUR's themes are the default; the Fedora theme directories are removed from the image (the package cannot be) |
| B2 | Default look-and-feel and other desktop defaults | `kde-settings` | TUR's defaults ([7.6](#76-configuration-and-additions)) |
| B3 | Fedora's welcome page and its icon | `plasma-welcome-fedora` | Remove the package; KDE's welcome window is suppressed |
| B4 | Wallpapers | `f45-backgrounds-kde`, `fedora-workstation-backgrounds`, `desktop-backgrounds-compat` | Remove; `tur-theme` provides the default background |
| B5 | Login-screen wallpaper path `/usr/share/wallpapers/Fedora` | `kde-settings-plasmalogin` | TUR's file ([6.3](#63-signing-in)) |
| B6 | Browser start page, pinned tab, "Firefox for Fedora", bookmarks | `firefox`, `fedora-bookmarks` | By the browser decision: remove the package in favour of a Flatpak, or override its defaults |
| B7 | Connectivity check, time servers, installation counting | 12.3 | 12.3 |
| B8 | Bug-report and support addresses | `os-release`, and any tool that reads them | Follows A3 |
| B9 | Image labels | The image configuration (the base carries no Fedora name [F4]) | TUR's title, source and a licence label that does not name the project licence alone ([licence review](../dependencies/licence-review.md), item 10) |

**C. May stay: technical or factual, not presentation.** Each is recorded so
that nobody removes it by reflex or is surprised to find it.

| # | Item | Why it stays |
|---|---|---|
| C1 | The directory `EFI/fedora` on the system partition | Fedora's signed GRUB is built to look there; renaming it needs a re-signed boot chain ([ADR-0013](../adr/0013-bootloader.md)) [F7] |
| C2 | Fedora's signed shim and GRUB, and the label "Fedora" inside shim's fallback file | Unmodified, and needed for Secure Boot. The firmware entry that the fallback path creates on a machine with no entry may therefore still read "Fedora" (seen in the [TUR-003 report](../reports/TUR-003.md)); the entry bootupd creates reads "TUR" |
| C3 | `fedora-repos`, `fedora-gpg-keys` | Fedora's own mark-free release package requires them [F3]; they say truthfully where packages come from |
| C4 | The `.fc45` tag in package and kernel versions; RPM vendor fields | Factual provenance (no statement from Fedora was found either way: an **assumption**) |
| C5 | Package, unit and policy *file names* containing "fedora" | Not shown to users. Fedora's own packager's note says file names are not the concern; that is a packager's remark, not a legal statement [F3] |
| C6 | `fedora-chromium-config`, `fedora-flathub-remote`, `fedora-third-party`, `fedora-workstation-repositories` | Names only. Removal is optional and is decided with the application centre's sources |
| C7 | `ID_LIKE=fedora` | Tells tools which family the system belongs to |
| C8 | KDE's and Plasma's names and artwork, unmodified, referring to KDE's software | Permitted for that purpose [F9] |

### 12.5 The check

`tests/image/smoke.sh` gains a section that fails the build when:

- any package of table A is installed, or `system-release` and `system-logos`
  are not provided by TUR's packages;
- a file in the image, outside an allow-list that is table C written down,
  contains "Fedora" in a place a user sees: `os-release`, the release files,
  desktop files' names and comments, the global themes' metadata, the default
  configuration, the Plymouth theme, the initramfs;
- any file from `fedora-logos` is present, by name or by checksum.

The end-to-end test adds what only a running system shows: text recognised in
the start-up, setup, login and desktop screenshots must not contain "Fedora".

## 13. Accessibility and internationalisation

These are requirements on every TUR program, each with the test that enforces
it. The target is WCAG 2.2 level AA, applied to desktop software in the manner
of EN 301 549 [A2]. None of it has been tested with people who rely on
assistive technology; that testing is a release requirement (AI control layer
§9; [section 16](#16-risks)).

### 13.1 Accessibility requirements

| # | Requirement | How Qt Quick meets it | Test |
|---|---|---|---|
| X1 | **Everything works from the keyboard.** Every control is reachable with Tab and arrow keys in reading order, and operable with Space or Enter. No trap, except the deliberate modality of a sheet | Qt Quick Controls take focus on Tab; `tur-kit`'s own controls set `activeFocusOnTab`; `KeyNavigation` and `FocusScope` for lists and groups | A Qt Quick Test walks every page with Tab only and asserts that every enabled control received focus once |
| X2 | **Focus is always visible** and not hidden behind other content (2.4.7, 2.4.11) | The focus ring of [5.4](#54-shape-space-and-icons); pages scroll the focused row into view | Screenshot of every page with focus on each control class; the ring's contrast is in the token test |
| X3 | **Every control has a name, a role and its state** for a screen reader | The `Accessible` attached property: `name`, `role`, `description`, `checked`, and `labelledBy` (Qt 6.10) [A1]. `tur-kit` controls set them; a control without a name fails to build into a page | A test walks the item tree of every page and fails on an interactive item without `Accessible.name` and `role`; the end-to-end test dumps the AT-SPI tree of each program and compares it with a recorded one |
| X4 | **Changes are announced.** Progress, results, errors and P@x's state changes are spoken | `Accessible.announce()` (Qt 6.8) [A1] for events; live values through `Accessible` properties | QML test asserts an announcement for each event class |
| X5 | **Contrast**: 4.5:1 for text, 3:1 for control boundaries and meaningful graphics (1.4.3, 1.4.11) | The palette of [5.2](#52-palette) | The contrast table as a unit test |
| X6 | **Nothing is said by colour alone** (1.4.1) | Status is icon and word; the application centre's ratings, the indicator's states and the device list are specified that way | Screenshot tests include a greyscale rendering of each status set |
| X7 | **High contrast.** In the session, TUR's programs follow whatever colour scheme the user chose, including Breeze High Contrast. TUR's system surfaces (setup, installer, trusted screen, overlay) have their own high-contrast palette, switched by the accessibility menu | Kirigami takes the scheme; `tur-kit` holds the fixed palette. Qt's `contrastPreference` hint exists since 6.10 but KDE's platform theme does not set it [A1], so the switch is TUR's own setting | Screenshot matrix includes high contrast |
| X8 | **Text can be enlarged to 200 %** without loss of content (1.4.4) | Sizes in points; layouts wrap and scroll, never clip | QML test at 200 % font size asserts no clipped text item |
| X9 | **Reduced motion** is honoured | [5.5](#55-motion): the desktop's animation setting, read by `tur-kit` because Qt 6.11 has no motion hint [A1] | QML test with the setting at zero asserts no running animation longer than 100 ms |
| X10 | **Targets** are at least 24 by 24 px (2.5.8); TUR's are 36 px and 44 px | `tur-kit` sizes | Token test |
| X11 | **Time limits.** Nothing times out into a decision. Notifications that need an answer stay. Input delays on approval surfaces are short and fixed | Design rules of [9.7](#97-approving-a-proposed-action-class-2) and [7.4](#74-notifications) | Review; QML test that no approval surface has an expiring timer |
| X12 | **The trusted screen and the setup wizard are usable without sight**: they run outside the user's session, so they start their own screen reader when the accessibility menu or a key combination asks for it | Orca in the setup and trusted sessions (an **assumption**: that it runs in a minimal KWin session) | End-to-end: AT-SPI tree of both; manual test with a screen reader before release |
| X13 | **P@x's overlay does not block assistive technology.** The pill is reachable from the keyboard (`Meta+Esc` to stop; a shortcut to move focus to it); pointer and highlight are also described in words in the conversation | Layer-shell keyboard interactivity on demand; every step has text | Compositor test |
| X14 | **P@x's voice is assistive technology** (AI control layer §9): every spoken thing is also text, and every action available by voice is available by keyboard | Conversation panel | Review |
| X15 | **Input methods** work in every text field of TUR's programs | Qt's text-input support with KWin's input method | End-to-end with one input method |

Two limits to state. P@x reads other applications through the same
accessibility tree a screen reader uses, so an application that is
inaccessible to a blind user is also harder for P@x: improving one improves
the other. And the state of screen reading on Plasma under Wayland in 2026
was not established by this research [S5]; it must be assessed on the image
before any accessibility claim is made.

### 13.2 Internationalisation requirements

| # | Requirement | How |
|---|---|---|
| L1 | **Every string is translatable**, and none is assembled from fragments | `qsTr` with full sentences and numbered placeholders; plural forms through `%n` [I1] |
| L2 | **One translation workflow for all TUR programs** | Qt's own format (`.ts`), extracted with `lupdate` in CI and hosted on a translation platform. Chosen over KDE's gettext-based library because it adds no dependency, is native to CMake, and switches language at run time, which the setup wizard needs [I1] |
| L3 | **Source strings are British English**, written as they appear in this document | Review; a spelling check in CI on the extracted strings |
| L4 | **Translator context for every string that could be ambiguous** | `//:` comments; the screenshot of each page is published beside the string |
| L5 | **Right-to-left layouts** | `LayoutMirroring` bound to the application's direction, inherited by children; directional icons mirrored; nothing positioned by `x` [I1]. P@x's panel docks on the left, and pointer captions flip |
| L6 | **Text expansion** | Layouts tested with a pseudo-locale that lengthens every string by 40 % [I1] |
| L7 | **Dates, times, numbers, sizes and money** in the user's locale | `Qt.locale()` formatting; sizes in binary units with the unit spelled as the locale does; money with the provider's currency code, never a guessed symbol |
| L8 | **Error codes, addresses, commands and file paths are never translated** | They are in the technical layer, in the fixed-width face |
| L9 | **Registry text is translatable**: error-code messages, settings titles and keywords, the application centre's fixed vocabulary | Each registry ships a translation catalogue; the code and the address are the keys |
| L10 | **Fonts for every script** | Inter with fallback to the Noto families ([5.3](#53-typography)); the image carries the CJK and emoji fonts |
| L11 | **P@x answers in the user's language**; system text about P@x is translated like any other | The request's language is passed to the provider by `tur-pax`; approval sheets and the trusted screen use TUR's own translated text and never the model's |
| L12 | **The trusted screen's exact-action text is never machine-translated.** Commands and parameters are shown as they are | [9.7](#97-approving-a-proposed-action-class-2), [9.8](#98-the-trusted-confirmation-prompt-class-3) |

The first release is English only. The requirements hold from the first
program, so that translation is a matter of adding catalogues and not of
changing code.

## 14. Component inventory

Every program and data package this design adds. "Side" is that of
[section 4](#4-the-licence-boundary-on-the-desktop): **A** a TUR program under
the project licence; **B** configuration or theme data; **C** code under an
upstream licence because it runs inside a GPL host. D-Bus names of TUR
services are the proposed ones (§16) and are frozen with each service.

### 14.1 Programs (side A)

Common to all Qt programs below, and not repeated in the table: Qt 6.11 Core,
Gui, Qml, Quick, Quick Controls, Quick Layouts, Svg and DBus, all
`LGPL-3.0-only` (or GPL, or commercial), linked dynamically from Fedora's
packages; and `tur-kit`. Each is a `QGuiApplication`.

| Name | What it is | Language | D-Bus and protocols used | Side | Further dependencies (licence) |
|---|---|---|---|---|---|
| `tur-kit` | Shared QML module and small library: tokens, controls, the two-layer message, settings page and row, the launch card of 8.9, accessibility and translation helpers, fixture loader | QML, C++ | Reads `org.freedesktop.portal.Settings` (appearance) | A | Optional: KColorScheme (`LGPL-2.0-or-later`) to read the system scheme |
| `tur-setup` | First-run setup | QML, C++ | `org.freedesktop.locale1`, `timedate1`, `hostname1`, `NetworkManager`, `org.freedesktop.Accounts` or a helper; runs under `kwin_wayland` as shipped | A | — |
| `tur-welcome` | The welcome window | QML, C++ | `org.tur.Storage1`, `org.tur.Device1`, `org.tur.Diagnostics1` (read) | A | Kirigami (`LGPL-2.0-or-later`); `qqc2-breeze-style` at run time (LGPL) |
| `tur-centre` | The application centre, with the setup flows for Windows, Android and optional stores | QML, C++ | The catalogue and router (the `tur-catalogue` command line with `--json` until `org.tur.Catalogue1` exists), `org.tur.Runtime1`, `org.tur.Guest1`, `org.tur.Update1` (read), `org.freedesktop.Notifications`; implements `org.tur.Addressable1` | A | Kirigami; `qqc2-breeze-style` |
| `tur-settings` | TUR's settings; resolver of setting addresses | QML, C++ | Implements `org.tur.Settings1` and `org.tur.Addressable1`; uses `org.tur.Update1`, `Storage1`, `Device1`, `Runtime1`, `Agent1`, the policy owner's interface, `org.freedesktop.secrets` (the key), the portal permission store (P@x's screen grant) | A | Kirigami; `qqc2-breeze-style` |
| `tur-diagnostics` | Support-bundle window; also a component embedded in the installer and in Settings | QML, C++ | `org.tur.Diagnostics1`, or the `tur-diag` command line until it exists | A | Kirigami; `qqc2-breeze-style` |
| `tur-notifier` | Session service: turns system events into notifications, and owns every tray item, P@x's included. No window. The only TUR user-interface process in an idle session | Rust | Listens to `org.tur.Update1`, `Storage1`, `Device1`, `Runtime1`, `Guest1`, `Agent1`; calls `org.freedesktop.Notifications`; exports StatusNotifierItem objects and their menus | A | `zbus` (MIT); no Qt |
| `tur-search` | Launcher search provider. No window | Rust | Implements `org.kde.krunner1`; calls `org.tur.Settings1`; searches the catalogue with the `tur-catalogue` library, used by path like any shared Rust code | A | `zbus` (MIT). The interface is implemented from its description; KDE's XML file (`LGPL-2.0-or-later`) is not copied into the program |
| `tur-pax-ui` | P@x's ask bar and conversation panel, its setup flow, the repair view | QML, C++ | `org.tur.Agent1`; layer-shell; default shortcut through its desktop file | A | LayerShellQt (`LGPL-2.1-only OR LGPL-3.0-only`) |
| `tur-pax-overlay` | P@x's indicator, pointer and highlights | QML, C++ | Exports `org.tur.PaxOverlay1`; layer-shell; **KDE-specific:** `org.kde.KWin` (`getWindowInfo`) and the Plasma window-management protocol, for window positions | A | LayerShellQt; a client library for the window-management protocol (KWayland, `LGPL-2.1-or-later`: an **assumption** that it still exposes what is needed) |
| `tur-pax-approve` | The class 2 approval sheet | QML, C++ | The policy owner's interface (`tur-pax-guard`, proposed) | A | — |
| `tur-confirm` | The trusted screen | QML, C++ | The policy owner's interface, on the system bus; runs in its own session under `kwin_wayland` as shipped | A | Basic style only; nothing from KDE Frameworks |
| `tur-installer` | The graphical installer | QML, C++ | `org.tur.Installer1`; `tur-diagnostics` component | A | Basic style only |

Named here but designed elsewhere: `tur-guest` and `tur-guest-display`
([windows-guest.md](windows-guest.md)), whose windows, launcher entries and
states this document presents; `tur-catalogue`
([catalogue-and-routing.md](catalogue-and-routing.md)); `tur-pax-guard` (the
system component that owns P@x's policy, standing approvals and
repair-session record; AI control layer §17, item 5); the proposed physical
input witness; and the boot-stage reporter.

Interfaces this design introduces, to be written as D-Bus XML and frozen:
`org.tur.Settings1`, `org.tur.Addressable1`, `org.tur.PaxOverlay1`.

```text
org.tur.PaxOverlay1                session bus, /org/tur/PaxOverlay1
  Methods   Acquire(s purpose) -> s lease          the indicator lease of 9.3
            Release(s lease)
            SetActivity(s lease, s state, a{sv} detail)
                 state: idle | listening | working | observing | acting | waiting | paused
                 detail: screens, window, repair-session expiry
            MovePointer(s lease, s screen, d x, d y, a{sv} options)
            PulsePointer(s lease, s kind)
            HidePointer(s lease)
            HighlightRect(s lease, s screen, (dddd) rect, a{sv} options) -> u id
            HighlightInWindow(s lease, u pid, s app_id, s title, (dddd) rect, a{sv} options) -> u id
            SetCaption(s lease, u id, s text, as controls)
            Clear(s lease, u id)
  Signals   StopRequested()  PauseRequested()  ResumeRequested()
            GuideControl(u id, s control)          back | repeat | do-it | stop
            PointerArrived(s lease)
            HighlightLost(u id, s reason)          window moved, covered or closed
  Property  Shown (b)                              true only while the pill is mapped on every screen concerned
```

What the user interface needs from `org.tur.Agent1` beyond the surface in the
AI control layer (§11), as requirements on that interface: the activity state
as a property with change notification; the steps of a task as a list with
plain-language text, tier, class and audit identifier; cost so far for the
task and the day; guide mode (show or do) as a per-task property; private
mode; "while you were away" as a query.

### 14.2 Data packages (side B)

| Name | Contents | Format |
|---|---|---|
| `tur-theme` | Global themes `org.tur.dark` and `org.tur.light` (metadata and defaults only); colour schemes; the icon theme with the lotus mark and TUR's own icons; the wallpaper | JSON, INI, SVG, PNG |
| `tur-desktop-defaults` | System defaults for Plasma and KWin; the panel for new accounts; window rules for TUR's windows; login and lock-screen configuration; the System Settings entry; the file-manager menu entry; the search provider's announcement; suppression of KDE's welcome window and setup wizard | INI, desktop files |
| `tur-plymouth-theme` | The boot splash | INI, PNG |
| `tur-logos` | TUR's artwork under the names other packages expect; provides `system-logos` | SVG, PNG; an RPM |
| `tur-release` | `os-release`, the release files, dependencies on the preset packages, the rpm-ostree polkit rule; provides `system-release` | Text; an RPM, started from `generic-release` (MIT), and staying MIT where it is Fedora's text |
| `kde-settings-map` | The mapping of [10.4](#104-kdes-settings) | JSON |

The brand artwork (the logo and marks derived from it) is the project's and
needs terms that protect the name and mark; a software licence, the project's
or any other, is the wrong instrument. That is a question for the legal review.

### 14.3 Upstream-licensed code (side C)

**None.** `desktop/upstream-licensed/` exists, with a README that states the
rule and is otherwise empty. The two reserves of
[section 4](#4-the-licence-boundary-on-the-desktop) would go there
(`plasma-layout/`, GPL-2.0-or-later) or upstream (KWin patches).

### 14.4 Upstream programs used as shipped

For completeness, the GPL programs this design *runs* and does not link or
extend: `kwin_wayland` (as the compositor of the setup and trusted sessions),
`plasmashell`, `systemsettings` (started with a module name), `krunner` and
Kickoff (which call `tur-search` over D-Bus), Plasma Login Manager, Plymouth,
Dolphin. Running a program, or being called by it over a documented D-Bus
interface, does not make a combined work
([licence review](../dependencies/licence-review.md), section 4.5).

## 15. Engineering plan

### 15.1 Repository layout

```text
desktop/
  CMakeLists.txt               one CMake project for every Qt program
  CMakePresets.json            presets: ci, ci-sanitize
  ALLOWED_IMPORTS              QML imports and libraries a TUR program may use
  README.md                    the rules of section 4, for implementers
  kit/                         tur-kit: QML module "Tur.Kit" and libturkit
    tokens/ controls/ message/ settings/ a11y/ fixtures/ gallery/ tests/
  apps/
    setup/ welcome/ centre/ settings/ diagnostics/
    pax-ui/ pax-overlay/ pax-approve/ confirm/
      each:  CMakeLists.txt  src/ (C++)  qml/  fixtures/  tests/  i18n/
             data/ (desktop file, icons, registry file, tool manifest)
  dbus/                        interface XML owned by desktop programs
  data/                        side B: files installed as they are; no code
    theme/                     look-and-feel, colour schemes, icons, wallpaper
    defaults/                  xdg/, skel/, kwinrulesrc, plasmalogin, System
                               Settings entry, service menu, search announcement
    plymouth/
    kde-settings-map.json
  upstream-licensed/           side C: empty but for a README
  packaging/                   spec files for tur-release and tur-logos
  ci/
    builder/Containerfile      the builder image
    builder.lock               its digest
  tests/
    e2e/                       scripts run in the boot-test guest
    compositor/                tests that need a running KWin
    screenshots/               reference images
services/
  tur-notifier/  tur-search/   Rust, each its own Cargo workspace (ADR-0005);
                               found by the existing rust workflow
installer/
  gui/                         tur-installer, built by the desktop CMake project
schemas/
  settings-registry.schema.json
```

`desktop/` holds QML **and C++**; the AGENTS.md layout table says QML only
and is amended. The two Rust session services live under `services/` with
the others, so the existing discovery, lint and lock-file handling apply to
them unchanged.

### 15.2 The build

**Recommendation: CMake, with the Qt programs written in QML and a thin layer
of C++, talking to the Rust services over D-Bus. No Rust inside a
user-interface process.**

The options, as they stand in November 2026 [Q4]:

| Option | State | Verdict |
|---|---|---|
| **C++ and QML with CMake**; Rust stays in the services | Qt's own toolchain. Everything needed is in Fedora 45's packages | **Chosen** |
| Rust in the user-interface process with cxx-qt | Version 0.10.0 (24 August 2026), still before 1.0 with breaking changes between minor versions; upstream tests Qt up to 6.10, not 6.11; dynamically loaded QML modules need CMake anyway | Not now |
| Qt Bridge for Rust (from the Qt Company) | Version 0.4.1 (29 October 2026); described by its authors as beta, a technology preview, with "compile times increased substantially" | Revisit when it is a supported release |
| Slint | Not Qt: no Kirigami, no Breeze, no Qt Quick Test | No |
| QML only, with a generic launcher | Cannot do D-Bus proxies with types, layer-shell set-up or models of any size | No |

Reasons, given that agents cannot compile locally and CI is the only compiler
(ADR-0002, ADR-0005):

1. **The process boundary already exists.** The services are Rust behind
   D-Bus. Putting Rust in the user-interface process as well adds a second
   toolchain to that process without removing the first: Qt's meta-object
   compiler, CMake and a C++ compiler are needed for any Qt program.
2. **Every round trip costs minutes.** A mistake must be legible from a CI
   log. A C++ shim fails in one compiler with a direct message. A bridged
   build fails in macro expansion, generated C++ or the link step across
   three toolchains. (A judgement; no measurements were found.)
3. **Most of the work needs no compiler at all.** With the logic in the
   services and the rest in QML, the fast checks (`qmllint`, the QML tests
   under the offscreen platform) catch most mistakes in a job that takes a
   few minutes.
4. **It matches the decision already made.** ADR-0005: "Qt 6 / QML, with C++
   only where QML needs it."

**What C++ is for, and nothing else:** typed D-Bus proxies generated from the
services' interface XML; list models; the fixture backend; layer-shell and
window set-up; reading the system's appearance settings. A C++ file that
decides anything about policy, routing, storage or approval is in the wrong
process and is refused in review.

**Rules of the build.**

- C++20, warnings as errors in CI, a sanitizer preset for the test job.
- One QML module per program, declared with `qt_add_qml_module`; `qmllint`
  runs on every module and a warning fails the job.
- The interface XML in a service's `dbus/` directory is the single source: the
  Rust service is tested against it and the C++ proxy is generated from it.
- Every program takes `--fixture DIR` and then answers from JSON files instead
  of D-Bus. Every test below the end-to-end level uses fixtures, so the
  desktop can be built and tested before the services exist, and the fixture
  files double as the working contract until an interface is frozen.
- Formatting (`clang-format`, `qmlformat`) is computed by CI and returned as a
  patch, in the manner of the Rust workflow's `fixes` artifact.
- Translation sources are extracted by CI (`lupdate`) and returned the same
  way.

**Getting the programs into the image.** The image's `Containerfile` gains a
build stage that starts from the builder image, builds `desktop/` and installs
it into a staging directory, and the final stage copies that directory in. A
list of the installed files is written for the SBOM, which closes item 3 of
the licence review's checklist for these files. Only `tur-release` and
`tur-logos` are RPMs, because other packages depend on what they provide.

### 15.3 Continuous integration

A new workflow, `desktop.yml`, triggered by changes under `desktop/`,
`installer/gui/`, the settings-registry schema and itself. All jobs run on
GitHub-hosted `ubuntu-24.04` runners. The repository is public, so a runner
has 4 processors and 16 GB of memory [Q8].

| Job | Runs in | Does | Budget (assumption) |
|---|---|---|---|
| `boundary` | Runner | SPDX header on every file; nothing under `desktop/` outside `upstream-licensed/` carries a GPL identifier and the reverse (ADR-0003's check, which does not exist yet); every QML import and linked library is in `ALLOWED_IMPORTS`; no `QApplication`; no GPL-only Qt module in any `find_package`; desktop files and registry files validate | 1 min |
| `build-test` | Builder container | Configure, build, `qmllint`, unit tests and QML tests with `QT_QPA_PLATFORM=offscreen`; uploads logs and the formatting and translation patch | 5 min |
| `screenshots` | Builder container | Renders every page of every program from fixtures across the matrix of 15.4; compares with the references; always uploads the rendered images and a contact sheet; on a difference, uploads the proposed new references | 5 min |
| `compositor` | Builder container | Starts `kwin_wayland --virtual` with software OpenGL in a private D-Bus session, as KDE's own test runner does [Q7], and runs the tests that need a compositor | 5 min |
| `builder` | Runner | Only when `desktop/ci/builder/` changes, and weekly: builds the builder image from Fedora 45 pinned by digest, pushes it to the project's registry, returns the new digest for `builder.lock` | 10 min |

The end-to-end tests are a new stage of the existing `boot` workflow, which
already installs the image to a disk and starts it under QEMU with KVM. Its
trigger paths gain `desktop/**`.

The existing `rust` workflow already runs for changes under
`docs/architecture/` and `catalogue/`, because several components test their
design note against their code. A change to this document therefore starts
it; nothing in it reads this document.

**The builder image.** Fedora 45 with, verified as package names in Fedora 45
[Q5]: `qt6-qtbase-devel`, `qt6-qtbase-private-devel`, `qt6-qtdeclarative-devel`,
`qt6-qtsvg-devel`, `qt6-qttools-devel`, `qt6-linguist`, `qt6-qtwayland-devel`,
`kf6-kirigami-devel`, `kf6-kcolorscheme-devel`, `layer-shell-qt-devel`,
`qqc2-breeze-style`, `extra-cmake-modules`, `cmake`, `ninja-build`, `ccache`,
`kwin`, `mesa-dri-drivers`, `breeze-icon-theme`, `rsms-inter-fonts`,
`jetbrains-mono-fonts`, `appstream`; and, for the tests, an AT-SPI client
library for Python (package name to be confirmed). (`qt6-qtquickcontrols2` no
longer exists; the controls are in `qt6-qtdeclarative` [Q5].) A prebuilt
image pinned by digest is used rather than installing packages in every job:
it is faster, and it makes the fonts and Qt version that the screenshot
references depend on exactly reproducible.

**What an agent gets back.** Because nobody can run the programs locally, the
artifacts are the development loop: compiler and test logs; the formatting
patch; and, from `screenshots`, the picture of every page the change touched.
An agent reads those images to see what it built, and the owner reviews the
same contact sheet in the pull request. `scripts/dev/ci.py` gains the
commands to fetch them and to apply proposed references.

### 15.4 Test strategy

| Level | What it proves | Tool | Where |
|---|---|---|---|
| **Static** | The licence boundary; the contrast table; registries valid; no setting address removed; desktop files valid | Scripts; JSON Schema | `boundary` |
| **Unit** | Models, parsers, fixture and D-Bus backends; notifier and search logic | Qt Test; Rust tests | `build-test`; `rust` workflow |
| **Component** | Each page behaves: every interaction, keyboard-only traversal (X1), accessible name and role on every control (X3), announcements (X4), 200 % text (X8), reduced motion (X9), right-to-left, lengthened strings (L5, L6), every state of every flow in sections 6 to 11 reachable from a fixture | Qt Quick Test, offscreen | `build-test` |
| **Screenshot** | Each page looks as approved | Qt Quick Test's `grabImage`, saved and compared outside Qt with a tolerance | `screenshots` |
| **Compositor** | What only a compositor can show: the overlay lets clicks through and takes them on the pill; one surface per screen; the approval sheet is above other windows and absent from a KWin screenshot; the default shortcut reaches `tur-pax-ui`; the overlay disappears when the session locks | Test clients under `kwin_wayland --virtual` | `compositor` |
| **End to end** | The image behaves: listed below | The boot test's guest agent, QMP from the host, AT-SPI in the guest | `boot` |
| **Not run** | Real graphics hardware, high-density and mixed-refresh displays, performance budgets (§30), screen readers used by people who depend on them, voice | — | Recorded as NOT RUN until hardware and testers exist (ADR-0002) |

**The screenshot matrix.** Variant (dark, light, high contrast) by direction
(left-to-right, right-to-left) by scale (1, 2). The offscreen platform selects
Qt Quick's software renderer [Q6], which is deterministic and draws no shader
effects; that is acceptable because nothing may depend on an effect
([5.5](#55-motion)). Fonts and Qt come from the pinned builder image.
Animations are off. A reference changes only through an explicit file change
in the pull request, so a visual change is always something a reviewer sees.
Whether `grabImage` works under the offscreen platform is an **assumption**
[Q6]; the first task proves it, and the fallback is the `compositor` job's
virtual KWin.

**End-to-end cases**, added to the boot test in this order:

1. A screenshot during start-up shows TUR's splash; recognised text contains
   no "Fedora".
2. First-run setup is driven to the end with the keyboard through QMP. The
   account it creates replaces the one the test fabricates today.
3. After login: the panel has the expected items, the wallpaper and colour
   scheme are TUR's, the welcome window is shown, and KDE's is not.
4. The welcome window does not appear at the second login.
5. `tur-centre --fixture` opens, a search returns the fixture's rows, and an
   application page shows its "How it will run" block.
6. `tur-settings tur.pax.privacy#private-mode` opens the page with the row
   focused and highlighted; `systemsettings` shows the "TUR" entry.
7. Every locator in `kde-settings-map.json` resolves to exactly one control in
   the image's System Settings.
8. A scripted P@x (no model, no key: the driver of milestone A2) moves the
   overlay's pointer to a row and the highlight is in the right place, in the
   screenshot, to within a few pixels. This is the exit criterion of A4b.
9. Trusted-path tests (AI control layer §12): emulated input sent to the
   user's session while the trusted screen is displayed reaches nothing on
   it; a capture taken in the user's session at that moment does not contain
   it; the approval is refused without authentication.
10. The AT-SPI tree of each TUR program matches its recorded tree.

The harness follows what KDE's own image tests do [Q7]: tests are injected
into the guest, run as units with their logs captured, and the host only
watches, sends keys and takes screenshots [Q8]. Automatic login for the test
account is a drop-in for Plasma Login Manager [Q8]. The virtual machine goes
from 2 to 4 processors for the desktop stage.

### 15.5 Implementation tasks

Each task is one pull request by one agent, with the tests named in it. "Needs"
lists tasks that must merge first; "Waits for" lists things outside this plan.

| # | Task | Delivers | Needs | Waits for |
|---|---|---|---|---|
| **D-01** | **Toolchain proof** | Builder image; `desktop.yml` with `boundary`, `build-test`, `screenshots`; the `desktop/` skeleton; a one-page program with one QML test and one reference image; `ci.py` commands for the artifacts. Settles the assumptions about `qqc2-breeze-style`, Kirigami's colours and `grabImage` | — | — |
| **D-02** | **`tur-kit`** | Tokens, controls, the two-layer message, the contrast test, the gallery program that the screenshot job renders, the accessibility walk and the direction and text-length harness | D-01 | — |
| **D-03** | **Fedora's marks** | `tur-release` and `tur-logos`; the removal list; `os-release`; the initramfs regenerated; the checks of 12.5; the boot test passes with `ID=tur`. Closes the branding part of issue #11 | — | The owner's answers to questions 3 and 5 for the final text and mark; a placeholder mark until then |
| **D-04** | **Theme and defaults** | Colour schemes generated from the tokens; both global themes; icon theme; wallpaper; Plymouth theme; login and lock configuration; system defaults; the panel for new accounts; KDE's welcome window and splash off | D-02, D-03 | — |
| **D-05** | **Desktop end-to-end harness** | The boot test's desktop stage: automatic login, keyboard and pointer through QMP, screenshots including start-up, AT-SPI dump, text recognition; cases 1, 3 | D-04 (can begin against the stock desktop) | — |
| **D-06** | **`tur-setup`** | The seven screens; the unit, account, session and polkit rule; `plasma-setup` out; case 2 | D-02, D-05 | — |
| **D-07** | **Settings shell and addressing** | The registry schema; `tur-settings` with Overview, Appearance and About; `org.tur.Settings1` and `org.tur.Addressable1`; command line and link; the System Settings entry; the registry tests; case 6 | D-02 | — |
| **D-08** | **`tur-diagnostics`** | The three screens over the `tur-diag` command line; the Diagnostics page | D-07 | — |
| **D-09** | **`tur-notifier`; Updates and Recovery** | The notifier and its tray items; the pages of 11.1 against fixtures | D-07 | `org.tur.Update1` frozen, for the live backend |
| **D-10** | **`tur-centre` 1: find** | Shell, search, application page, no-route page, the fixed vocabulary, reading the merged catalogue through `tur-catalogue --json`; fixtures for seed entries (Microsoft 365, Valorant, Krita, the Android banking class) kept in the repository and checked against the router's real output by a test in the Rust workflow; case 5 | D-02 | — (the catalogue and router are merged) |
| **D-11** | **`tur-centre` 2: install** | Install, update and remove flows; Sources; Installed and Updates | D-10 | Store adapters; `tur-runtime` as a service |
| **D-12** | **`tur-centre` 3: files** | "I have a file"; file-type handlers | D-10 | `tur-runtime` as a service; the hand-off from inspection to the catalogue |
| **D-13** | **`tur-search`** | The launcher search provider | D-07, D-10 | — |
| **D-14** | **`tur-welcome`** | The welcome window; case 4 | D-06 | — |
| **D-15** | **`tur-pax-overlay`** | Indicator, pointer, highlights; `org.tur.PaxOverlay1`; the compositor tests | D-02 | — |
| **D-16** | **Trusted screen prototype** | `tur-confirm`, its session, the switch there and back, the security phrase; an ADR on how authentication is routed; case 9. This task decides whether section 9.8 stands | D-02, D-05 | The design of `tur-pax-guard`; the owner's answers to questions 1 and 2 |
| **D-17** | **`tur-pax-ui`** | Ask bar, conversation panel, steps, the setup flow of 9.12, on a fixture of `org.tur.Agent1` | D-02 | — |
| **D-18** | **Approval sheet** | `tur-pax-approve`; window rules; the capture-exclusion test | D-15, D-17 | `tur-pax-guard` |
| **D-19** | **P@x's pages** | Privacy, Approvals, Memory, Schedules, Spending, Activity; the repair view | D-07, D-17 | Milestones A5 to A7 |
| **D-20** | **KDE's settings and guide mode** | `kde-settings-map.json` with its first rows; the locator; cases 7 and 8 | D-07, D-15, D-05 | Milestone A2's driver |
| **D-21** | **Storage and Devices** | The pages and prompts of 11.3 and 11.4 | D-07, D-09 | `org.tur.Storage1`, `org.tur.Device1` as services |
| **D-22** | **Adding a store** | The Google Play flow of 8.6 | D-11 | ADR-0011's verification; the Android milestones |
| **D-23** | **`tur-installer`** | The screens of 11.5 | D-02, D-08 | An installer core that executes a plan; ADR-0007's condition |
| **D-25** | **Windows on the desktop** | The setup flow of 8.9 in `tur-centre`; the launch card; the Windows tray item and notifications; the Windows page of Settings; the marking of launcher entries, checked in the end-to-end test with the guest design's stand-in guest | D-09, D-11 | Guest milestones W3 to W5 |
| **D-24** | **Upstream proposals** | Written proposals to KWin and System Settings for the items of 9.13 and 10.4. Work for a person with the owner, not for a coding agent | D-16, D-20 for evidence | — |

D-01 to D-05 are the foundation and go first, in that order except that D-03
is independent and can run beside D-01 and D-02. After them, three lines can
run in parallel without touching the same files: the system line (D-06 to
D-09, D-14, D-21), the application centre (D-10 to D-13, D-22, D-25) and P@x
(D-15 to D-20). D-10 can start as soon as D-02 has merged: what it reads is
already on the main branch.

## 16. Risks

| # | Risk | Severity | Mitigation |
|---|---|---|---|
| 1 | **Plasma 6.8 lets any unsandboxed process in the session inject input and capture the screen without a prompt** [K2]. P@x's safeguards on the desktop then rest on P@x never causing unsandboxed code to run: through a command, or through a file it writes that is later executed (an autostart entry, a shell profile, a user unit, a desktop file, KWin's rules or shortcut files) | Critical | A requirement on the command runner (no compositor socket, filtered session bus) and on the policy engine (writes to locations that are executed or that configure the compositor are class 2 at least). Stated in AI control layer amendments. The trusted screen does not depend on it |
| 2 | **The trusted screen may not be buildable as designed**: starting and switching to a second compositor session on demand; routing authentication to it; speed; behaviour in the test VM | High | A prototype with the trusted-path tests before anything depends on it (D-16). Until it passes, P@x is limited to classes 0 and 1, as the AI control layer already says |
| 3 | **`/dev/uinput` reachable by the user** (a rule shipped with game-controller support grants it) would let a session process create a device every compositor treats as physical, defeating the trusted screen | High | Verified in D-16; if the rule is needed for games, it is narrowed or the trusted session ignores virtual devices |
| 4 | **The class 2 approval sheet is protected by TUR's own code, not by the compositor**, and so falls short of section 6 of the AI control layer | High | The owner decides (question 1). The stronger alternative works today at a cost in smoothness |
| 5 | **One seat.** When P@x acts through the screen it takes the user's cursor and focus | Medium | Turn-taking with the person always winning ([9.4](#94-pxs-pointer)); tools and the accessibility tree preferred; upstream proposal |
| 6 | **Removing Fedora's marks breaks something** (`ID` change; release package swap), or the statement of origin is not acceptable | High for publication | The boot test judges D-03; the legal review answers question 3 |
| 7 | **The data-only panel layout is brittle** across Plasma versions | Medium | End-to-end case 3 on every image build; the reserve of section 4 |
| 8 | **`org.kde.breeze` for TUR's programs** is not in the image, or looks unlike the desktop style | Medium | Settled in D-01; fallback to the Basic style themed by `tur-kit` |
| 9 | **The mapping of KDE's settings breaks** when Plasma moves a control | Medium | End-to-end case 7 catches it before release; honest degradation at run time; upstream proposal |
| 10 | **Two settings programs** confuse people | Medium | Cross-links and one search ([10.1](#101-a-settings-program-of-turs-own-beside-kdes)). To be watched in the first tests with real users |
| 11 | **Agents cannot see what they build**, so the result drifts from this design | Medium | Screenshots of every page in every pull request; the owner reviews the contact sheet |
| 12 | **Screenshot tests are noisy** across font or Qt updates | Medium | Pinned builder image; tolerance; references regenerated by CI and reviewed |
| 13 | **Accessibility is designed but unproven.** Screen reading on Plasma under Wayland was not assessed; no person who depends on it has tried TUR | High before release | Automatic checks from the first program; an assessment on the image; testing with users as a release requirement |
| 14 | **LGPL obligations of TUR's programs**: notices, the user's ability to replace the libraries, and KDE frameworks that contain GPL files [Q1] [Q2] | Medium | `ALLOWED_IMPORTS`, the `boundary` job, dynamic linking to system packages; legal review |
| 15 | **The privacy sentence in setup is ahead of the image** | Medium | [12.3](#123-network-contact-nobody-asked-for) is part of D-03 and D-04; the sentence ships only when the table is done |
| 16 | **The boot splash changes a boot-critical file** | Medium | Boot test; Plymouth's text fallback |
| 17 | **The application centre's words and the router's values drift apart** | Low | [8.2](#82-words-the-application-centre-uses) maps one-to-one onto the merged catalogue; a test in `tur-centre` fails if the router has a route, rating, outlook or severity with no label |
| 19 | **The catalogue holds only other people's reports**, and few of them: twelve rated records in 32 entries. Most pages will say "No reports" | Medium | Said plainly on the Home page; TUR's own tests are what change it; "No reports" is designed to read as a fact, not an alarm |
| 20 | **Launcher entries from the Windows guest cannot be marked as the guest design requires** if Kickoff does not show the description line | Low | The corner mark in the icon does not depend on Kickoff; checked in D-25 |
| 21 | **Router text is English only**, so the application centre cannot be translated where it matters most | Medium | Requirement 1 of [8.10](#810-how-it-is-built) on the router |
| 18 | **Interfaces KDE treats as its own** (window-management protocol, KWin's D-Bus interface) change | Low | Confined to `tur-pax-overlay`; compositor tests |

## 17. Open questions for the owner

1. **Where is a class 2 action approved** (send, buy, delete, install)?
   - *On the desktop*, in the approval sheet of
     [9.7](#97-approving-a-proposed-action-class-2). Smooth. Protected by TUR's
     own code against a misled model, not by the compositor, and not against
     other software running as the user.
   - *On the trusted screen*, like class 3. Strong today. Costs a switch of
     screen for every approval.

   Recommendation: the desktop sheet for class 2; the trusted screen for
   class 3, for opening a repair session and for anything that loosens P@x's
   limits. This matches the README's two promises ("it asks first"; "a prompt
   that P@x cannot see or touch") and needs section 6 of the AI control layer
   and interpretation 29 of the policy engine amended to recognise two levels.
2. **May TUR run a small system service that watches physical keyboards** for
   one purpose: to see the stop combination, and that some key was pressed,
   and to report nothing else? It gives a stop control nothing in the session
   can suppress, without waiting for KWin. It is a program that reads
   keyboards, and needs its own ADR and review if the answer is yes.
3. **How does TUR describe its origin?** As "a Fedora Remix", which Fedora's
   guidelines provide for with a required notice, or with no Fedora mark and
   a factual sentence, which needs the lawyer ([12.2](#122-what-the-user-is-told)).
4. **Which web browser ships?** Fedora's Firefox package carries Fedora's
   start page and name. Either it is replaced by the browser's own Flatpak, or
   its defaults are overridden. The choice also decides the fourth pinned
   application.
5. **May the logo be redrawn as a flat mark?** The supplied logo is a
   rendered image with glows. The boot splash, the launcher icon at 16 px,
   P@x's flame and the tray item need a simplified vector version of the lotus
   and of the flame, in few colours. This design assumes yes.

### 17.1 The owner's answers (12 November 2026)

These answers govern where the sections above say otherwise.

1. **Class 2 is approved on the desktop**, in the approval sheet of 9.7. The
   trusted screen is for class 3, for opening a repair session and for
   anything that loosens P@x's limits. Section 6 of the AI control layer and
   interpretation 29 of the policy engine are to be amended to recognise the
   two levels.
2. **The physical input witness is allowed**, with an ADR and a review before
   it ships. The owner added a condition: **the stop combination and P@x's
   other keyboard commands are visible at all times** while P@x is switched
   on, not only while it acts. The P@x indicator (D-15) always carries the
   stop combination, and the full list of P@x's commands is one action away
   from it. The visible list and the bindings come from one source, so they
   cannot disagree.
3. **No Fedora mark.** TUR describes its origin with a factual credit line
   only ("TUR is built from software packaged by the Fedora Project and many
   other projects. TUR is not provided, endorsed or supported by the Fedora
   Project or by Red Hat."), which the lawyer is to review. "Fedora Remix" is
   not used. D-03 proceeds on that basis.
4. **Firefox is the default browser.** The owner also asked for **Chrome for
   people coming from Windows**. Google Chrome's terms do not allow TUR to
   ship it in the image, so it is offered, not preinstalled: the first-run
   welcome and the application centre offer it with one action, installed
   from Google's own source and updated by it, and the welcome says plainly
   that it comes from Google. (This reading of the answer "firefox and chrome
   for windows" is the designer's; the owner may correct it.)
5. **The logo may be redrawn as a flat mark** (lotus and flame) for small
   icons, the boot screen and P@x's pointer. The full logo stays for the
   README and About.

## 18. Amendments to specification revision 1.0

| Document and section | Amendment |
|---|---|
| Specification §9.1 | Replace "Discover or a customized software center" with the application centre, `tur-centre` (ADR-0011). Add: TUR's settings are in `tur-settings`, beside KDE System Settings. Add: every user-interface component TUR writes is a standalone program (ADR-0016). |
| §9.2 | Replace the three visual modes with one identity in a dark and a light variant ([section 5](#5-visual-direction); ADR-0007). |
| §9.3 | Add: the bridge writes an "About this application" action into each application's desktop file ([7.3](#73-windows-and-task-switching)). |
| §9.4 | The activity indicator and P@x's pointer are client surfaces on the compositor's overlay layer, with the lease rule of [9.3](#93-the-activity-indicator), until the compositor can draw them. |
| §9.5 | Add the requirements of [section 13](#13-accessibility-and-internationalisation) with their tests. |
| §6.3 | Add: the error code is visible on the first layer; the text comes from the registry and is not written in user-interface code ([5.6](#56-tone-of-voice)). |
| §6.6 | Add: the self-test's findings are shown in the welcome window and in Settings ▸ Diagnostics. |
| §16 | Add `org.tur.Settings1`, `org.tur.Addressable1`, `org.tur.PaxOverlay1`. Add the requirement that every settings row has a registered address ([10.3](#103-the-addressing-scheme)). |
| §18.1 | Add the layout of [15.1](#151-repository-layout). |
| §18.3 | Add the `desktop` workflow and the desktop stage of the boot test. |
| §20, Phase 3 | Replace "TUR branding and visual modes" with tasks D-01 to D-14; add D-15 to D-20 beside milestones A2 to A4b. |
| §24 | Add the risks of section 16, items 1 to 6 and 13. |
| §27.1 | Add the words shown for each stage ([6.1](#61-starting)). |
| §27.5 | Replace the user-visible response for a failed new deployment with the text of [11.1](#111-updates-and-rollback). |
| §28.4 | Add: the diagnostics drawer can be narrowed but not closed. |
| §29.3 | Add the family `TUR-SETUP-*`, and the topic `SETTINGS` in the `DESKTOP` family for addressing errors. |
| AI control layer §4 | Correct: on Plasma 6.8 an ordinary process that is not sandboxed *can* capture the screen and inject input without a prompt [K2]. "The compositor is the single point at which the assistant's access is granted, shown and revoked" does not hold; the single point is TUR's own capture and input component, and everything P@x runs must be sandboxed away from the compositor. Add: P@x captures one window by default ([9.9](#99-privacy-controls)). |
| AI control layer §6 | The five guarantees may be provided by a separate trusted session outside the user's compositor ([9.8](#98-the-trusted-confirmation-prompt-class-3)), not only by the compositor. If the owner accepts question 1: define two levels, the desktop approval sheet for class 2 and the trusted screen for class 3, repair sessions and loosening of policy. |
| AI control layer §8 | "Compositor-drawn indicator" becomes the overlay with the lease rule, plus Plasma's own screen-cast item and remote-control notification. |
| AI control layer §11 | Add to `org.tur.Agent1` what the user interface needs ([14.1](#141-programs-side-a)). |
| AI control layer §16.6 | P@x's pointer is drawn by a client. "The user's cursor is not taken away" holds when P@x acts through tools and the accessibility tree; when it acts through the screen, turn-taking applies until KWin has a second seat ([9.4](#94-pxs-pointer)). |
| agent-policy.md, section 10, item 29 | Follows the answer to question 1. Add to the known limits: file writes to locations that are executed or that configure the compositor. |
| android-runtime.md 7.1 | Add the "About this application" action and the marking of [7.2](#72-launcher) to launcher entries. |
| windows-guest.md 6.7 | "Every entry is marked as a Windows application in the launcher" is done as [7.2](#72-launcher) specifies: the description line and a corner mark drawn into the icon on the host. Add the "About this application" action. |
| windows-guest.md 7.2, 7.3, 7.4 | The setup flow's first page, the launch card and the words for the guest's states are those of [8.9](#89-setting-up-windows-and-android) and [7.5](#75-system-tray). |
| catalogue-and-routing.md | Add the three requirements of [8.10](#810-how-it-is-built): identifiers and parameters for reasons, caveats and prerequisites; detected host facts; an updatable, signed catalogue. Add the user-facing labels of [8.2](#82-words-the-application-centre-uses) by reference. Its statement that a change to catalogue data alone does not start CI is out of date. |
| `schemas/app-profile.schema.json` | `test_status` has four values (`untested`, `works`, `partial`, `broken`); the catalogue's `rating` has five (`good`, `usable`, `poor`, `broken`, `unknown`). One scale should serve both; this document uses the catalogue's. |
| pax-providers.md 10 | Add the data file of tested models and their prices that the setup flow needs ([9.12](#912-setting-up-px)). |
| AGENTS.md | The list of GPL-only Qt modules is incomplete: add Charts, Data Visualization, Lottie, HTTP Server, GRPC, MQTT, CoAP, Network Authorization, Quick 3D Physics, Canvas Painter [Q1]. Add: KDE frameworks that contain GPL files and must not be linked or imported ([3.5](#35-qt-and-kde-libraries-a-tur-program-may-link)). Add: a TUR program is a `QGuiApplication`. Layout table: `desktop/` is QML and C++. |
| tests/boot | The setup-done flag and the fabricated account change with D-06. |

## 19. Sources

Read on 12 November 2026. Dates in brackets are the date of the release,
merge or last change of the source. Source-code references are to the
`Plasma/6.8` branch or the `v6.8.2` tag (27 October 2026) of the KDE
repositories at `invent.kde.org`, and to the `6.11` branch of Qt, unless
stated. "MR" is a merge request in the named repository.

**KWin and Wayland**

- [K1] Layer shell. KWin `src/wayland/layershell_v1.cpp` (version 5),
  `src/layershellv1window.cpp`, `src/wayland_server.cpp` (not restricted),
  `src/scene/windowitem.cpp` (lock screen):
  https://invent.kde.org/plasma/kwin/-/tree/Plasma/6.8 . LayerShellQt, licence
  and QML interface: https://invent.kde.org/plasma/layer-shell-qt/-/tree/Plasma/6.8 .
  Qt's input region: https://github.com/qt/qtbase/blob/6.11/src/plugins/platforms/wayland/qwaylandwindow.cpp
- [K2] Emulated input. KWin MR 5496, libeis backend (merged 15 April 2024,
  Plasma 6.1): https://invent.kde.org/plasma/kwin/-/merge_requests/5496 .
  Portal: xdg-desktop-portal-kde MR 279 (15 April 2024), MR 326, the
  `kde-authorized` grant (9 January 2025, Plasma 6.3), MR 571, the notification
  (9 June 2026, Plasma 6.7): https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/merge_requests/326 .
  KWin MR 6057, restricted interfaces denied only to sandboxed clients (18
  June 2026, Plasma 6.8): https://invent.kde.org/plasma/kwin/-/merge_requests/6057 .
  `src/plugins/eis/eisbackend.cpp`, `eiscontext.cpp` (touch capability).
  Portal documentation: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html
- [K3] One seat; no input immunity. libei: https://libinput.pages.freedesktop.org/libei/ .
  KWin `src/input.cpp`, `src/input_event.h`. Multi-seat:
  https://invent.kde.org/plasma/kwin/-/issues/218 (opened 15 April 2024,
  open) and https://invent.kde.org/plasma/kwin/-/issues/331 (30 October 2026).
- [K4] Capture exclusion. KWin MR 8442 (19 November 2025, Plasma 6.6), MR
  8828, the window rule (19 February 2026, Plasma 6.7), MR 9171, screenshots
  (13 May 2026), MR 8446, window-management state, MR 8519, draft client
  protocol: https://invent.kde.org/plasma/kwin/-/merge_requests/8442 .
  `src/rulesettings.kcfg`, `src/plugins/screencast/filteredsceneview.cpp`.
  This Week in Plasma, 22 November 2025:
  https://blogs.kde.org/2025/11/22/this-week-in-plasma-ui-and-performance-improvements
- [K5] Screen cast. xdg-desktop-portal-kde `src/screencast.h`,
  `src/screencast.cpp` (version 6, source types, cursor modes, tray item); MR
  597 (26 August 2026). KWin `src/wayland/screencast_v1.cpp`.
  https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
- [K6] Window geometry. KWin `src/org.kde.KWin.xml`, `src/dbusinterface.cpp`
  (`getWindowInfo`), `src/wayland/plasmawindowmanagement.cpp`. A supported
  scripting interface over D-Bus is only a request:
  https://invent.kde.org/plasma/kwin/-/issues/243
- [K7] Global shortcuts. kglobalacceld `src/kserviceactioncomponent.cpp`,
  `src/globalshortcutsregistry.cpp`:
  https://invent.kde.org/plasma/kglobalacceld/-/tree/Plasma/6.8 .
  xdg-desktop-portal-kde `src/globalshortcuts.cpp`.
- [K8] Window rules. KWin `src/rulebooksettings.cpp`, `src/rulesettings.kcfg`,
  `src/rules.h`, `src/window.cpp`.
- [K9] Lock screen and authentication agent. kscreenlocker `ksldapp.cpp`,
  `greeter/greeterapp.cpp`: https://invent.kde.org/plasma/kscreenlocker/-/tree/Plasma/6.8 .
  polkit-kde-agent-1 `main.cpp`: https://invent.kde.org/plasma/polkit-kde-agent-1/-/tree/Plasma/6.8 .
  One agent per session: https://github.com/polkit-org/polkit/blob/main/src/polkitbackend/polkitbackendinteractiveauthority.c
- [K10] Protocols implemented. KWin `src/wayland/CMakeLists.txt`,
  `src/wayland_server.cpp`. Protocol catalogue: https://wayland.app/protocols/
- [K11] Effects. KWin `src/plugins/highlightwindow/highlightwindow.cpp`;
  overlay request: https://invent.kde.org/plasma/kwin/-/issues/276
- [K12] Command-line options. KWin `src/main_wayland.cpp`.

**Plasma configuration**

- [P1] Global theme package. plasma-workspace
  `shell/packageplugins/lookandfeel/lookandfeel.cpp` and `lookandfeel/`:
  https://invent.kde.org/plasma/plasma-workspace/-/tree/v6.8.2
- [P2] Defaults at login. plasma-workspace `startkde/startplasma.cpp`;
  `kcms/lookandfeel/tool/lnftool.cpp`.
- [P3] Layout. plasma-workspace `shell/shellcorona.cpp`; plasma-desktop
  layout templates and `applets/kickoff/main.xml`:
  https://invent.kde.org/plasma/plasma-desktop/-/tree/v6.8.2 ; plasma-setup
  `files/plasma-setup-tmpfiles.conf.in`; scripting documentation:
  https://develop.kde.org/docs/plasma/scripting/
- [P4] Defaults on an image. https://develop.kde.org/docs/administration/kiosk/introduction/ ;
  Fedora's kde-settings: https://forge.fedoraproject.org/kde/kde-settings (19
  August 2026); bootc on `/etc`:
  https://github.com/bootc-dev/bootc/blob/main/docs/src/bootc-filesystem.7.md
  (29 September 2026).
- [P5] Colour schemes. https://invent.kde.org/plasma/breeze/-/blob/v6.8.2/colors/BreezeLight.colors ;
  plasma-integration `khintssettings.cpp`; https://develop.kde.org/docs/plasma/theme/quickstart/
- [P6] Icons, cursors, wallpapers. https://specifications.freedesktop.org/icon-theme/latest/ ;
  https://invent.kde.org/plasma/breeze/-/blob/v6.8.2/cursors/svg-cursor-format.md ;
  breeze `wallpapers/Next`.
- [P7] Plasma Login Manager. https://invent.kde.org/plasma/plasma-login-manager
  (README, `plasmaloginsettingsbase.kcfg`, `src/frontend/startkde/startplasma.cpp`);
  https://fedoraproject.org/wiki/Changes/PlasmaLoginManager (29 September
  2026). kscreenlocker's configuration.
- [P8] Plymouth. https://gitlab.freedesktop.org/plymouth/plymouth/-/blob/main/src/plugins/splash/two-step/plugin.c ;
  https://src.fedoraproject.org/rpms/plymouth/blob/rawhide/f/plymouth.spec ;
  initramfs in a bootc build:
  https://gitlab.com/fedora/bootc/docs/-/blob/main/modules/ROOT/pages/initramfs.adoc
  (12 August 2025).
- [P9] plasma-setup. https://invent.kde.org/plasma/plasma-setup (v6.8.2:
  `files/`, `src/bootutil/`, `src/auth/authhelper.cpp`,
  `src/qml/LandingComponent.qml`, `docs/CUSTOM_MODULES.md`);
  https://fedoraproject.org/wiki/Changes/Unified_KDE_OOBE (29 November 2025);
  Fedora's package: https://src.fedoraproject.org/rpms/plasma-setup
- [P10] KDE's welcome window. https://invent.kde.org/plasma/plasma-welcome
  (README, `src/kded/daemon.cpp`).
- [P11] Licences of themes and interpreted files.
  https://community.kde.org/Policies/Licensing_Policy ;
  https://www.gnu.org/licenses/gpl-faq.html (13 January 2026), "IfInterpreterIsGPL"
  and "GPLPlugins".
- [P12] Tray, notifications, launcher search.
  https://invent.kde.org/frameworks/kstatusnotifieritem ;
  https://specifications.freedesktop.org/notification/latest/protocol.html
  (version 1.3, 18 August 2024); plasma-workspace `libnotificationmanager`;
  https://invent.kde.org/frameworks/krunner/-/blob/master/src/data/org.kde.krunner1.xml ;
  plasma-desktop `applets/kickoff/main.qml`.
- [P13] Light by day, dark by night.
  https://blogs.kde.org/2025/08/02/this-week-in-plasma-day/night-theme-switching/
  (2 August 2025); Plasma 6.5 announcement:
  https://kde.org/announcements/plasma/6/6.5.0/

**Settings and accessibility**

- [S1] Opening modules. systemsettings `app/main.cpp`:
  https://invent.kde.org/plasma/systemsettings/-/tree/v6.8.2 ; kcmutils
  `src/kcmshell/main.cpp`;
  https://invent.kde.org/plasma/kde-cli-tools/-/blob/v6.8.2/plasma-open-settings/main.cpp
- [S2] External entries in System Settings. systemsettings
  `app/kcmmetadatahelpers.h` (`plasma/systemsettings/externalmodules`),
  `app/ExternalAppModule.cpp`.
- [S3] Module arguments. plasma-nm `kcms/kcm_connections/kcm.cpp`;
  the notifications module.
- [S4] No address for a control. https://bugs.kde.org/show_bug.cgi?id=510894
  (last changed 2 August 2026);
  https://invent.kde.org/plasma/systemsettings/-/issues/28 ; kcmutils
  `src/qml/components` (setting highlighter).
- [S5] AT-SPI in Qt. qtbase `src/gui/accessible/linux/dbusconnection.cpp`,
  `atspiadaptor.cpp`; https://doc.qt.io/qt-6.11/qml-qtquick-accessible.html
- [A1] Qt's accessibility hints and properties.
  https://doc.qt.io/qt-6.11/qaccessibilityhints.html (contrast, 6.10; no motion
  hint); https://doc.qt.io/qt-6/qaccessibilityhints.html (motion, 6.12);
  plasma-integration at v6.8.2; xdg-desktop-portal-kde `src/settings.cpp`.
- [A2] https://www.w3.org/TR/WCAG22/ (W3C Recommendation, 12 December 2024).
  EN 301 549 V4.1.1 (published 2 September 2026): https://nda.ie/news/en301549-published
- [A3] KWin's keyboard monitor for assistive technology (since 6.4): KWin
  `src/a11ymanager.cpp`. Not used by this design.

**Fedora**

- [F1] Fedora trademark guidelines: https://docs.fedoraproject.org/en-US/legal/trademarks/
  (text last changed 5 December 2024). Remix:
  https://fedoraproject.org/wiki/Remix
- [F2] https://src.fedoraproject.org/rpms/fedora-logos/raw/f45/f/fedora-logos.spec
  and the package's COPYING.
- [F3] https://src.fedoraproject.org/rpms/generic-release/raw/f45/f/generic-release.spec ;
  https://src.fedoraproject.org/rpms/generic-logos/raw/rawhide/f/generic-logos.spec ;
  https://docs.fedoraproject.org/en-US/bootc/os-release-and-versions/ (10 July
  2024); plasma-workspace's package definition:
  https://src.fedoraproject.org/rpms/plasma-workspace/blob/rawhide/f/plasma-workspace.spec
- [F4] What Kinoite 45 contains: https://forge.fedoraproject.org/atomic-desktops/config
  (branch f45); fedora-release's `os-release`; Fedora's kde-settings;
  NetworkManager's connectivity configuration; dnf5's defaults.
- [F5] Other Fedora-based systems. https://github.com/ublue-os/aurora ,
  https://github.com/ublue-os/bluefin , https://github.com/ublue-os/bazzite ,
  https://github.com/Ultramarine-Linux/packages ,
  https://github.com/Nobara-Project/rpm-sources ; Asahi's exception:
  https://forge.fedoraproject.org/council/tickets/issues/432 (approved 6
  April 2023).
- [F6] Statements by Fedora contributors.
  https://forge.fedoraproject.org/atomic-desktops/tracker/issues/125 (29
  August 2026); https://discussion.fedoraproject.org/t/200797 (31 August to 16
  September 2026); https://discussion.fedoraproject.org/t/197924 (29 July
  2026); https://discussion.fedoraproject.org/t/132263 (29 September 2024).
- [F7] Boot entries. https://github.com/coreos/bootupd/blob/main/src/efi.rs ;
  ostree's deployment titles; shim's fallback file.
- [F8] https://www.ntppool.org/en/vendors.html
- [F9] https://kde.org/stuff/clipart/ . KDE's trademark licence text itself
  was not found.

**Qt, libraries, build and test**

- [Q1] https://doc.qt.io/qt-6.11/licensing.html and the per-module pages;
  https://www.qt.io/development/open-source-lgpl-obligations
- [Q2] Licence headers of KDE Frameworks, scanned per repository at
  https://invent.kde.org/frameworks/ and https://invent.kde.org/plasma/ ;
  https://community.kde.org/Policies/Licensing_Policy
- [Q3] Style selection. plasma-integration
  `qt6/src/platformtheme/kdeplatformtheme.cpp`:
  https://invent.kde.org/plasma/plasma-integration/-/tree/Plasma/6.8 ;
  Kirigami `src/platform/platformpluginfactory.cpp`;
  https://invent.kde.org/frameworks/qqc2-desktop-style/-/raw/master/README.md
- [Q4] Rust and QML. https://github.com/KDAB/cxx-qt/releases (0.10.0, 24
  August 2026) and https://kdab.github.io/cxx-qt/book/ ;
  https://www.qt.io/blog/qt-bridge-for-rust-0.3-cxx-qt-compatibility-and-soundness
  (23 September 2026); https://slint.dev/terms-and-conditions ;
  https://develop.kde.org/docs/getting-started/rust/
- [Q5] Fedora 45 packages: https://packages.fedoraproject.org/ (Qt 6.11.2, KDE
  Frameworks 6.31.0, Plasma 6.8.2).
- [Q6] Offscreen platform and software renderer. qtbase
  `src/plugins/platforms/offscreen/qoffscreenintegration.cpp`; qtdeclarative
  `src/quick/scenegraph/qsgcontextplugin.cpp`;
  https://doc.qt.io/qt-6.11/qtquick-visualcanvas-adaptations-software.html ;
  https://doc.qt.io/qt-6.11/qml-qttest-testcase.html
- [Q7] Testing under KWin. https://invent.kde.org/sdk/selenium-webdriver-at-spi
  (`run.rb`); KDE Linux's tests:
  https://blogs.kde.org/2026/07/13/openqa-testing-in-kde-linux/ (13 July 2026).
- [Q8] https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html ; Plasma
  Login Manager's `[Autologin]` configuration (repository above);
  https://docs.github.com/en/actions/reference/runners/github-hosted-runners ;
  https://github.blog/changelog/2024-04-02-github-actions-hardware-accelerated-android-virtualization-now-available/
- [T1] Fonts: https://packages.fedoraproject.org/ (`rsms-inter-fonts`,
  `jetbrains-mono-fonts`, the Noto packages); Plasma's default fonts:
  plasma-integration `qt6/src/platformtheme/kfontsettingsdata.cpp`.
- [I1] https://doc.qt.io/qt-6.11/i18n-source-translation.html ;
  https://doc.qt.io/qt-6.11/qtquick-positioning-righttoleft.html ;
  https://docs.weblate.org/en/latest/formats/qt.html ;
  https://docs.weblate.org/en/latest/admin/addons.html

**Not established by this research**, and marked as assumptions where the
design relies on them: whether `Meta+Space` and `Meta+Esc` are free in Plasma
6.8's defaults; whether KDE's shortcut portal reports the release of a held
combination; the configuration keys of Plasma's day and night switching;
whether a pre-authorised portal session can pick a window without a dialog;
the path and format of file-manager service menus in Plasma 6.8; whether
Plymouth's `two-step` module shows messages as drawn; how the `two-step`
passphrase prompt is worded; the state of screen reading on Plasma under
Wayland; whether Fedora's CJK fonts and `qqc2-breeze-style` are in the Kinoite
image; whether any wayland-protocols proposal for a secure surface exists (the
site could not be read); a Fedora-sanctioned "based on Fedora" wording.

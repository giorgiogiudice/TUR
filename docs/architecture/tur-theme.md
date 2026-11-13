<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# TUR's theme and the defaults of a new account

**Task:** D-04 of [desktop-experience.md](desktop-experience.md), section 15.5
("Theme and defaults"). **Report:** [desktop-5](../reports/desktop-5.md).
**Code and data:** `desktop/data/` ([README](../../desktop/data/README.md)),
`image/Containerfile` (one block), `tests/boot/` (the desktop check).

This document says what TUR installs to give KDE Plasma its look and a new
account its desktop, where each file goes and why, what was verified in
Plasma's sources, and where the result departs from the design. Everything
here is configuration and pictures: side B of the licence boundary
([desktop-experience.md](desktop-experience.md), section 4). No code runs
inside Plasma.

"Verified" below means read in the source of the version the image ships
(Plasma 6.8.2 tags of `plasma-workspace`, `plasma-desktop`, `plasma-welcome`,
`plasma-setup`, `plasma-login-manager` at invent.kde.org; KDE Frameworks
`kcolorscheme` 6.31.0; Fedora 45's `kde-settings` 44.0 and its patches to
`plasma-setup`), on 13 November 2026. Section 10 lists the files. Nothing
was run outside CI.

## 1. What is installed

| In the image | What | Made by |
|---|---|---|
| `/usr/share/color-schemes/TurDark.colors`, `TurLight.colors` | KDE colour schemes | generated from the tokens (`gen_theme.py`) |
| `/usr/share/plasma/look-and-feel/org.tur.dark/`, `org.tur.light/` | Global themes "TUR Dark" and "TUR Light": `metadata.json` and `contents/defaults` only | written |
| `/usr/share/tur/xdg/kdeglobals` | Default global theme (TUR Dark), the light/dark pair, double click | written |
| `/usr/share/tur/xdg/ksplashrc` | No splash while Plasma starts (locked) | written |
| `/usr/share/tur/xdg/plasma-welcomerc` | KDE's welcome window does not open by itself | written |
| `/usr/share/tur/xdg/kicker-extra-favoritesrc` | The launcher's favourites for a new account | written |
| `/etc/xdg/<each of the four>` | A relative link to the file above | written |
| `/etc/skel/.config/plasma-org.kde.plasma.desktop-appletsrc`, `plasmashellrc` | The panel of a new account | written |

`image/Containerfile` copies `desktop/data/rootfs/` as it is, in one
delimited block after the branding step, so that these global themes replace
the provisional `org.tur.dark` of TUR-008 (whose two files in
`image/rootfs/` are overwritten and can be deleted by the image's owner).
`desktop/data/MANIFEST.tsv` lists every file with its SHA-256 and every link
with its target; the boot test compares the installed image with it.

Not installed by this task, and why:

- **An icon theme.** Breeze and Breeze Dark stay the icon themes (see 4).
- **Fonts.** Inter and JetBrains Mono are not in the image (desktop-kit.md,
  5, item 8); adding them is a package install in the image build, which is
  the image's owner's. The global themes set no font until they are there:
  naming a font the system lacks would give whatever fontconfig substitutes.
- **A light wallpaper picture.** See 4.
- **The Plymouth theme, the login screen's wallpaper and the lock screen's
  wallpaper**: TUR-008 installed them (`image/`), and they are unchanged.

## 2. Colours

### 2.1 From the tokens to KDE's roles

`desktop/kit/tokens/tokens.json` is the one source of TUR's colours. KDE's
colour scheme has seven colour sets (View, Window, Button, Selection,
Tooltip, Complementary, Header, and an inactive Header), each with two
backgrounds, eight text roles and two decorations, plus the window frame's
colours (`[WM]`) and two "effects" (verified: `colorsapplicator.cpp`,
`kcolorscheme.cpp`). `gen_theme.py` maps each key to a token; the mapping is
a design judgement and is the table `ROLES` in the script. Its rules:

| KDE's role | TUR's token | Why |
|---|---|---|
| Window background | `surface.base` | The window's own ground |
| View background (lists, fields) | `surface.raised` | "Lists, cards, text fields" |
| Button background | `surface.overlay` | A button stands off the window |
| Header (title bars, tool bars) | `surface.raised` dark, `surface.sunken` light | One step from the window, as KDE's own schemes do; inactive: `surface.base` |
| Tooltip | `surface.overlay` | "Menus, dialogs, tooltips" |
| Complementary | the dark variant's values in both schemes | KDE uses it for the contrasting parts (on-screen displays, the lock screen); dark in a light scheme, as KDE's light scheme does |
| Alternate backgrounds | the neighbouring surface (`raised`, `base`, `sunken`) | Alternating rows, pressed buttons |
| Normal text | `text.primary` | |
| Inactive text | `text.secondary` | Supporting text, held to 4.5:1 |
| Active, link and visited text | `accent.text` | Blue is the system; TUR has no second link colour |
| Negative, neutral, positive | `negative`, `warning`, `positive` | Warning is orange, kept apart from P@x's gold |
| Focus decoration | `accent.text` | The design's focus ring |
| Hover decoration | `accent` | |
| Selection, and all text on it | `selection`, `selection.on` | 5.6 and 5.5 to 1 |
| Window frame, active / inactive | as the header / `surface.base`, with `text.primary` / `text.secondary` | |
| Disabled text | an effect: ordinary text fades towards the background by 0.56 (dark) or 0.57 (light) | KDE has no colour for disabled text, only an effect. The amount is computed so that `text.primary` lands on `text.disabled` |
| Inactive windows | no effect | A window looks the same with or without the focus (calm) |
| Frame shading (`[KDE] contrast`) | 4 (KDE's default 7) | Quieter frames; depth comes from the four surfaces |

The full table, with every value, is printed by `gen_theme.py --table` and
appears in the `desktop` workflow's summary.

### 2.2 Contrast

The check computes the WCAG 2.2 ratio of every text role on both backgrounds
of its set (4.5:1), of both decorations on both backgrounds (3:1), of the
selection against the window and the list it sits on (3:1), and of both
window titles. All pass. The lowest:

| Scheme | Lowest text pair | Ratio | Lowest boundary pair | Ratio |
|---|---|---|---|---|
| TUR Dark | Selection: text on selection | 5.58 | Selection against View | 3.12 |
| TUR Light | Button and Header: positive text on `surface.sunken` | 4.91 | Button and Header: hover on `surface.sunken` | 4.63 |

Disabled text is exempt (WCAG 1.4.3), as in the design's own table.

### 2.3 Gold

Gold is P@x's and nothing else's (design, 5.1). No KDE role is mapped to a
P@x token (the generator refuses), and the check fails on any of P@x's
colours, or on any colour of the gold family (hue 33 to 55 degrees with real
saturation), in any file of the installed tree.

## 3. The two global themes

A global theme here is `metadata.json` and `contents/defaults`, nothing else.
Verified in `libklookandfeel/klookandfeelmanager.cpp`: at each login
(`startkde/startplasma.cpp`, `setupPlasmaEnvironment`) Plasma writes the
theme's defaults into the account's `~/.config/kdedefaults/`, which comes
before every system directory and after the account's own settings. It
applies these keys and no others: colour scheme, widget style, icons, Plasma
style, cursor, window switcher, window placement, window decoration and its
border size, fonts. `[Wallpaper] Image` is read by the wallpaper itself
(`wallpapers/defaultwallpaper.cpp`) for a desktop that has no picture of its
own. `gen_theme.py` fails on any other key.

| | TUR Dark (`org.tur.dark`) | TUR Light (`org.tur.light`) |
|---|---|---|
| Colour scheme | `TurDark` | `TurLight` |
| Icons | Breeze Dark | Breeze |
| Cursor | Breeze (`breeze_cursors`) | Breeze |
| Widget style, window frame | Breeze | Breeze |
| Plasma style | `default` (Breeze, which follows the colour scheme) | the same |
| Wallpaper | `Tur` | `Tur` |
| Splash, layout script | none (both are code inside Plasma) | none |

The package's `Id` is its directory's name: `plasma-apply-lookandfeel`
refuses a package whose id differs from the name it is given
(`kcms/lookandfeel/tool/lnftool.cpp`).

KDE's other defaults already are what the design's section 7.3 asks for and
are not repeated: KWin's window switcher is the thumbnail grid
(`tabboxconfig.h`, `defaultLayoutName`), Breeze puts minimise, maximise and
close on the right, `Meta` opens the launcher, edges tile.

## 4. Icons, the launcher's mark and the wallpaper

**Icons: Breeze.** The design (7.6) proposed icon themes `tur` and
`tur-dark` inheriting Breeze, whose only purpose was to put TUR's mark on the
launcher under Breeze's name `start-here-kde-symbolic`. A theme that
inherits Breeze also appears as a choice in System Settings and changes the
name of the theme every program reports. **Decision:** keep Breeze and set
the launcher's icon in the panel of a new account
(`[Configuration][General] icon=tur-logo-icon`, Kickoff's own setting,
verified in `applets/kickoff/main.xml`). `tur-logo-icon` is TUR's mark on its
dark round tile, installed by TUR-008 in `hicolor` at every size. The price:
an account that removes the launcher and adds it again gets KDE's icon.

**Wallpaper: `Tur`, in both themes.** TUR-008's `image/branding/branding.sh`
installs one picture, dark, and makes `images_dark` a link to `images`. A
second, light picture needs that step changed (it is the image's), so TUR
Light shows the same dark picture for now: a light panel and light windows
on the dark wallpaper. Follow-up 2 in section 9.

## 5. Where the system defaults live

The design left this to D-04 (7.6): the place that reaches the user's
session, the first-run setup's session and the login screen. Verified:

| Session | Its configuration directories | Source |
|---|---|---|
| A person's Plasma session | `~/.config/kdedefaults`, `/etc/xdg`, `/usr/share/kde-settings/kde-profile/default/xdg` | `startplasma.cpp` prepends `kdedefaults`; Fedora's `kde-settings` sets the rest in `/etc/xdg/plasma-workspace/env/env.sh` |
| plasma-setup's session | the same (a full Plasma session) | the same |
| The login screen (Plasma Login Manager's greeter) | `/etc/xdg` only: its environment comes from PAM (`pam_env`), which sets no `XDG_CONFIG_DIRS` | `plasma-login-manager` 6.8.2: `data/pam/fedora/plasmalogin-greeter`, `src/daemon/Display.cpp` |

So `/etc/xdg` is the one directory all three read, and it comes before
Fedora's `kde-settings` directory, whose `kdeglobals` and
`kicker-extra-favoritesrc` it must override. **Decision:** the content lives
under `/usr/share/tur/xdg/`, and `/etc/xdg` holds a relative link to each
file. The image's content stays under `/usr` and follows every update; an
owner who wants a different system default replaces the link with a file of
their own, which an update then leaves alone (bootc's three-way merge of
`/etc`). A person's own settings are in their `~/.config`, which always
comes first: nothing here can overwrite them.

What each file does (all verified):

- **`kdeglobals`**, `[KDE]`: `LookAndFeelPackage=org.tur.dark` (the theme of
  an account that has not chosen one, and of the login screen);
  `DefaultDarkLookAndFeel` and `DefaultLightLookAndFeel`, the pair Plasma
  switches between when "light by day, dark at night" is on
  (`kcms/lookandfeel/lookandfeelsettings.kcfg`; the design, 3.2, had these
  keys as not verified); `SingleClick=false`.
- **`ksplashrc`**, `[KSplash][$i] Engine=none`, locked. Plasma starts its
  splash unless `Engine` is something other than `KSplashQML`
  (`startplasma.cpp`, `setupKSplash`, `startKSplashViaSystemd`). A global
  theme cannot say this: `KLookAndFeelManager::setSplashScreen` always writes
  `Engine=KSplashQML` (its comment: "TODO: a way to set none as splash in the
  l&f"), and it does so for TUR's themes too, although they hold no splash:
  a global theme without one falls back to Breeze's, so Plasma counts it as
  having one. It writes into the account's `kdedefaults`, which comes before
  `/etc/xdg`. The first boot test of this task showed exactly that: the
  account's `kdedefaults/ksplashrc` said `Engine=KSplashQML`,
  `Theme=org.tur.dark`, and `plasma-ksplash.service` ran (run 43320682140).
  Hence the lock (`[$i]`, KDE's mark for a setting that files read later,
  the account's included, cannot change). **The price: the splash cannot be
  chosen in System Settings.** TUR-008's provisional theme had
  `[ksplashrc] Engine=none` in its defaults, which Plasma never applies;
  that is why its boot test still showed Plasma's splash.
- **`plasma-welcomerc`**, `[General] LastSeenVersion=6.8.0`,
  `ShowUpdatePage=false`. Plasma's welcome daemon opens the window when
  `LastSeenVersion` is empty, and after a Plasma update when
  `ShowUpdatePage` is true (`plasma-welcome`, `src/kded/daemon.cpp`). It is
  what `plasma-setup` itself does for its own session. The program stays in
  the launcher ("Welcome Center").
- **`kicker-extra-favoritesrc`**, `Prepend=` Files, Firefox, Settings,
  `IgnoreDefaults=true`: read once, when the launcher first starts in an
  account (`applets/kicker/kastatsfavoritesmodel.cpp`, `portOldFavorites`).

## 6. The panel of a new account

Plasma reads the panel from the account's own
`~/.config/plasma-org.kde.plasma.desktop-appletsrc`, opened without the
system cascade (`shell/shellcorona.cpp`). A file with a panel in it stops
Plasma from running its default layout (`ShellCorona::load`); the desktop
itself is made by Plasma for the account's activity and screen, with the
global theme's wallpaper. `plasma-setup` relies on the same behaviour for
its own session (`files/plasma-setup-tmpfiles.conf.in`). So the file is
data in `/etc/skel/.config/`, copied when an account is made: by `useradd
-m`, which `plasma-setup` runs (`src/auth/authhelper.cpp`), and which the
boot test runs for its user. It is the one place KDE requires a file under
`/etc` for this.

| Position | Applet | Setting |
|---|---|---|
| 1 | `org.kde.plasma.kickoff`, the launcher | `icon=tur-logo-icon` |
| 2 | `org.kde.plasma.icontasks`, pinned and running applications | `launchers=` Files (`org.kde.dolphin.desktop`), Firefox (`org.mozilla.firefox.desktop`), Settings (`systemsettings.desktop`) |
| 3 | `org.kde.plasma.marginsseparator` | an invisible spacer, as in KDE's own panel |
| 4 | `org.kde.plasma.systemtray` | |
| 5 | `org.kde.plasma.digitalclock` | |

The panel is at the bottom (`location=4`), horizontal (`formfactor=2`),
floating (Plasma's default, `PanelView::defaultFloating`), 44 pixels high
(`plasmashellrc`, `[PlasmaViews][Panel 1][Defaults] thickness`). No pager and
no "show desktop" button (design, 7.1). The application centre is not pinned
yet: `tur-centre` is not in the image, and the task manager would show an
entry for a missing program (`launchertasksmodel_p.h`, `isValidLauncherUrl`
accepts any `applications:` address). Discover is not pinned (design, 7.1).

`plasma-setup` copies its own session's `kdeglobals`, `kwinrc` and
`kwinoutputconfig.json` into the new account after `useradd`
(`authhelper.cpp`). None of TUR's new-account files is among them, so they
are not overwritten.

## 7. plasma-setup's light/dark switch

The switch cannot be pointed at TUR's themes by configuration. Verified:
upstream `plasma-setup` 6.8.2 runs `plasma-apply-lookandfeel --apply` with
the names `org.kde.breezedark.desktop` and `org.kde.breeze.desktop` compiled
in (`modules/prepareutil/prepareutil.cpp`), and decides whether the switch
starts on "dark" by comparing the colour scheme's name with `BreezeDark`.
Fedora's patch `plasma-setup-select-fedora-lookandfeel.patch` replaces the
names with `org.fedoraproject.fedoradark.desktop` and
`org.fedoraproject.fedora.desktop`, which TUR-008 removed. By the source,
in the image the switch therefore starts on "light" while the screen is
dark, and moving it changes nothing (the tool answers "Unable to find the
theme named …"). This was read, not watched: the boot test does not operate
the switch.

What was not done, and why: global themes named after Fedora's, holding
TUR's, would make the switch work but put Fedora's names back into the image
and into every new account's settings, and fail the image's check "no
Fedora global theme" (`tests/image/smoke.sh`); a wrapper program in front of
`plasma-apply-lookandfeel` would be code standing in for a configuration.
The answer is task D-06 (`tur-setup` replaces `plasma-setup`, as the design
says), or, until then, a one-line downstream patch to `plasma-setup` if the
owner wants it. The boot test records the names the installed `plasma-setup`
holds, as an informational check.

## 8. How it is tested

| What | Where | Proves |
|---|---|---|
| `gen_theme.py --check` | `desktop` workflow, `boundary` job | the generated files are the tokens'; every scheme colour is a token's; no gold; WCAG AA in both schemes; the formats and keys of section 3, 5 and 6; no Fedora text; SPDX headers; the manifest |
| `gen_theme.py --self-test` | the same | each of eleven planted faults is caught |
| `render_schemes.qml` | `desktop` workflow, `screenshots` job | a reference picture of each scheme, compared pixel by pixel (`desktop/tests/screenshots/colour-scheme-*`), and every text in it at 4.5:1 against what is really behind it |
| `tests/boot/desktop_checks.py` | `boot` workflow, the extra boot of `splash.py` | on the booted image, for the boot test's user signing in for the first time: the files are the manifest's; the account got TUR Dark and its colours, Breeze icons, cursor and frame, double click; the panel as plasmashell holds it (`dumpCurrentLayoutJS`); the desktop has no picture of its own and the screen shows TUR's wallpaper (pixels against `branding/wallpaper`); no welcome window; no splash; TUR's two themes offered; TUR Light applies as System Settings applies it; the panel is dark, then light, in the two pictures |
| `screenshot-text.py` | `boot` workflow | no picture, the light one included, names Fedora |

## 9. Departures from the design, and follow-ups

Departures (each a decision the owner may reverse):

1. **No TUR icon theme**; the launcher's icon is set in the new account's
   panel (section 4).
2. **The system defaults are linked from `/etc/xdg`**, not placed in a
   directory under `/usr` added to `XDG_CONFIG_DIRS`: the login screen reads
   no such variable (section 5).
3. **TUR Light uses the dark wallpaper** until a light picture is installed.
4. **No fonts are set** until Inter and JetBrains Mono are in the image.
5. **Four pinned applications are three**: the application centre is pinned
   when it is in the image.
6. **The welcome window's favourite (`tur-welcome`) is not there**: D-14.

Follow-ups:

1. Image: install `rsms-inter-fonts` and `jetbrains-mono-fonts`; then the
   global themes name them (`[kdeglobals][General] font`, `fixed`; note that
   Plasma 6.8 looks for the font keys in the wrong group when deciding
   whether a theme has fonts, `packageContents`, so the check of that needs
   care).
2. Image: a light wallpaper picture in `Tur/contents/images` with the dark
   one in `images_dark` (a change to `branding.sh`, step 6, and a light
   rendering in `branding/render.py`).
3. `plasma-setup`'s switch (section 7): D-06, or a downstream patch.
4. The `boot` and `image` workflows do not start on a change under
   `desktop/data/` alone (their trigger paths are the image owner's): add
   `desktop/data/**` to both.
5. The image's own smoke test still checks the default global theme in
   Fedora's `kde-settings` directory; it could check `/etc/xdg/kdeglobals`
   and the manifest.
6. Previews for the two global themes (System Settings shows a placeholder):
   pictures of the desktop, from the boot test's screenshots.
7. When `tur-centre` and `tur-welcome` are in the image: pin the centre
   first on the panel, add both to the favourites.
8. The provisional `image/rootfs/usr/share/plasma/look-and-feel/org.tur.dark/`
   can be deleted (it is overwritten in the build).

## 10. Sources read

Plasma 6.8.2 (tags `v6.8.2` at invent.kde.org):

- `plasma-workspace`: `startkde/startplasma.cpp`;
  `libklookandfeel/klookandfeelmanager.cpp`;
  `kcms/lookandfeel/lookandfeelsettings.kcfg`;
  `kcms/lookandfeel/tool/lnftool.cpp`; `kcms/colors/colorsapplicator.cpp`;
  `wallpapers/defaultwallpaper.cpp`; `shell/shellcorona.cpp`;
  `shell/panelview.cpp`; `shell/dbus/org.kde.PlasmaShell.xml`;
  `applets/kicker/kastatsfavoritesmodel.cpp`;
  `applets/systemtray/systemtray.cpp`;
  `libtaskmanager/launchertasksmodel.cpp`, `launchertasksmodel_p.h`;
  `ksplash/ksplashqml/plasma-ksplash.service.in`;
  `lookandfeel/org.kde.breeze*/contents/defaults` (read for the format, not copied).
- `plasma-desktop`: `applets/kickoff/main.xml`; `applets/taskmanager/main.xml`;
  `containments/panel/main.xml`;
  `layout-templates/org.kde.plasma.desktop.defaultPanel/contents/layout.js`.
- `plasma-welcome`: `src/kded/daemon.cpp`, `src/config.kcfg`.
- `plasma-setup`: `modules/prepareutil/prepareutil.cpp`,
  `src/auth/authhelper.cpp`, `files/plasma-setup-tmpfiles.conf.in`.
- `plasma-login-manager`: `src/frontend/startkde/startplasma.cpp`,
  `src/daemon/Display.cpp`, `data/pam/fedora/plasmalogin-greeter`,
  `src/frontend/kcm/auth/plasmaloginauthhelper.cpp`.
- `kwin`: `src/tabbox/tabboxconfig.h`.

KDE Frameworks 6.31.0: `kcolorscheme`, `src/kcolorscheme.cpp`,
`src/kcolorschememodel.cpp`.

Fedora 45: `kde-settings` 44.0-1 (`/etc/xdg/plasma-workspace/env/env.sh`, the
files of `kde-profile/default/xdg`), `kde-settings-plasmalogin`,
`plasma-setup` 6.8.2's spec and its two patches (src.fedoraproject.org,
branch f45), `firefox.spec` (the desktop file `org.mozilla.firefox.desktop`).

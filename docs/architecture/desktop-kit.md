# tur-kit: the shared UI kit

How to build a TUR desktop program on `tur-kit`, how its tests and reference
images work, and what the first build task found. The design is
[desktop-experience.md](desktop-experience.md) (sections 4, 5, 13 and 15); the
decision is [ADR-0016](../adr/0016-desktop-and-user-interface.md). The rules
for code under `desktop/` are in [desktop/README.md](../../desktop/README.md).

Status: tasks D-01 and D-02 of the design's plan. Report:
[desktop-1.md](../reports/desktop-1.md).

## 1. What the kit is

| Module | Library | For | Contents |
|---|---|---|---|
| `Tur.Kit` | `libturkit` | Every TUR program | Tokens (`Theme`, `Motion`), controls, the two-layer `Message` |
| `Tur.Kit.Pax` | `libturkitpax` | P@x's surfaces only | Gold: `PaxTheme`, `PaxMark`, `PaxName`, `PaxPill` |
| `Tur.Kit.Testing` | `libturkittesting` | Tests only; not shipped | `Harness`: contrast walk, accessibility walk, direction and text-length harness |
| `Tur.Kit.Gallery` | `tur-kit-gallery` | People, and the screenshot job | Every component in every state |

The kit links Qt Core, Gui, Qml, Quick and Quick Controls 2 and nothing from
KDE, because the trusted screen and the installer use it with nothing from
KDE (design, 14.1).

## 2. Building a program on it

```cmake
qt_add_executable(tur-example src/main.cpp)
qt_add_qml_module(tur-example URI Tur.Example VERSION 1.0 NO_CACHEGEN QML_FILES qml/Main.qml)
target_link_libraries(tur-example PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick)
tur_link_qml_modules(tur-example turkit)
```

```cpp
QGuiApplication app(argc, argv);     // never a QApplication
TurKit::useStandaloneLook();         // only for a program outside the session
QQmlApplicationEngine engine;
engine.loadFromModule("Tur.Example", "Main");
```

```qml
import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

Window {
    width: 640; height: 480; visible: true
    title: qsTr("Updates and recovery")

    Kit.Page {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGroup

            Kit.Label { text: qsTr("Updates and recovery"); kind: Kit.Label.Title }
            Kit.Banner { Layout.fillWidth: true; severity: Kit.Banner.Positive; text: qsTr("TUR is up to date.") }
            Kit.Button { text: qsTr("Check now"); kind: Kit.Button.Primary }
            Item { Layout.fillHeight: true }
        }
    }
}
```

`desktop/kit/gallery/` is the worked example: `main.cpp`, `Main.qml` and a
page for each family of components.

Import the kit **with a qualifier** (`as Kit`): its `Button`, `Label` and
`TextField` have the names Qt Quick Controls uses.

**Which style.** A program inside the session does not choose a style: Plasma
gives a `QGuiApplication` `org.kde.breeze` (section 5 below), and stock
controls and Kirigami pages look like KDE's. A program outside the session
(setup, installer, trusted screen, overlay) calls
`TurKit::useStandaloneLook()`: the Basic style, Inter, and the dark variant
whatever the machine says. The kit's own controls are built on Qt Quick
Templates and draw themselves from the tokens, so they look the same under
either style. `Kit.Page` also sets the palette from the tokens, so that a
stock control under the Basic style takes TUR's colours.

### Tokens

`desktop/kit/tokens/tokens.json` is the one source: the values of the design's
section 5. `Theme.qml` and `pax/PaxTheme.qml` are generated from it and
committed; `gen_tokens.py --check` (in the `boundary` job) fails if they are
stale. After changing the JSON: `python3 desktop/kit/tokens/gen_tokens.py
--write`. The Plasma colour schemes of task D-04 are to be generated from the
same file.

| Singleton | Gives |
|---|---|
| `Kit.Theme` | `light` (dark unless set or the desktop says light); the 17 system colours (`surfaceBase` … `negative`); type sizes and weights; `spaceGrid` 4, `spaceRelated` 8, `spaceGroup` 16, `spaceMargin` 24; `radiusControl` 6, `radiusCard` 10, `radiusSheet` 14; `targetPointer` 36, `targetTouch` 44; `textScale` |
| `Kit.Motion` | `control` 120, `appear` 200, `panel` 320 ms and `fadeControl`, `fadeAppear`, `fadePanel`; `factor` follows the desktop's `AnimationDurationFactor`; at zero nothing moves, a fade is at most 100 ms and nothing loops (`loops`) |
| `Pax.PaxTheme` | `pax`, `fill`, `fillText`, and the overlay's fixed palette |

Rules, enforced by `desktop/ci/boundary.py`:

- **No colour literal in QML.** A colour is a token. A control has no colour
  property to set: `Button.kind`, `Label.tone`, `Banner.severity` choose it.
- **Gold is P@x.** `Kit.Theme` has no gold. Gold is `Tur.Kit.Pax`, and a file
  may import that only under `desktop/kit/pax`, the gallery, the tests and
  `desktop/apps/pax-*`. P@x's components may not use `Theme.accent` or
  `Theme.selection`: blue is the system.
- **Every animation takes its duration from `Kit.Motion`.**

### Components

| Component | Notes |
|---|---|
| `Page` | The ground of a view: background, right-to-left mirroring, palette |
| `Label` | `kind`: Body, Strong, Small, Caps, Heading, Title, Display, Mono. `tone`: Primary, Secondary, Accent, Positive, Warning, Negative, OnAccent, OnSelection. `paragraph` for running text (lines 1.4 apart). `tabular` for figures that change |
| `Button` | `kind`: Secondary, Primary, Quiet, Destructive |
| `TextField` | `label` (also its accessible name), `helperText`, `errorText` |
| `CheckBox`, `Switch` | |
| `ListRow` | `subtitle`, `selected`, `onOverlay` (inside a sheet or menu) |
| `Card`, `Separator` | |
| `Banner` | `severity`: Info, Positive, Warning, Negative; `text`; `actionText`, `triggered()` |
| `Sheet` | Modal dialog sheet: `title`, content, `buttons: [...]`; give the safe button `focus: true`; Escape leaves. It mirrors itself and shows the focus ring as soon as it opens |
| `ProgressBar` | `label`, `detail` (real figures only), `indeterminate` |
| `BusyIndicator` | `label` |
| `Message` | The two-layer message, below |
| `Glyph`, `FocusRing`, `StateLayer`, `Disclosure`, `CodeLabel` | Parts the others are made of |

**The two-layer message** (specification 6.3 and 29.3; design 5.6).

```qml
Kit.Message {
    severity: Kit.Message.Error          // Error, Warning, Notice, Done
    code: "TUR-STORAGE-NTFS-004"         // always visible; can be selected and copied; never translated
    title: entry.title                   // what happened
    reason: entry.reason
    whatToDo: entry.action
    technicalDetail: entry.detail        // closed until asked for; fixed-width; Copy
    actions: [qsTr("Try again")]         // the safe action first
    paxAvailable: pax.enabled            // "Ask P@x to look into it", only while P@x is on
    onActionTriggered: index => ...
    onSupportBundleRequested: ...        // offered whenever severity is Error
    onPaxRequested: ...
}
```

The words come from the code's registry entry; a program does not write them.

### What every page must do

- Every string through `qsTr`, as a full sentence. Codes, paths and commands
  are not translated.
- Nothing positioned by `x`; anchors and layouts mirror by themselves inside
  a `Kit.Page`.
- Every interactive control has an accessible name and role. The kit's have;
  a `TextField` needs its `label`.
- Text wraps. Nothing is elided.

## 3. Tests

Everything runs in CI, in the `desktop` workflow.

| What | Where | Proves |
|---|---|---|
| Licence boundary | `desktop/ci/boundary.py`, and `--self-test` | Section 4 of the design, on sources and on what the built programs link |
| Token table | `gen_tokens.py --check` | Generated files current; every declared colour pair meets WCAG 2.2 (4.5:1 text, 3:1 boundary) and equals the figure in the design's table |
| Unit | `kit/tests/unit/*.cpp` | Reading the desktop's animation setting; the pseudo-locale |
| `tst_tokens`, `tst_motion` | `kit/tests/` | The live tokens equal the JSON in both variants; reduced motion |
| `tst_controls`, `tst_message` | | Pointer, keyboard, accessible name and role of each control; the message's two layers |
| `tst_contrast` | | **The contrast walk.** For every gallery page, in both variants: each text and each control boundary on the page against the colour really painted behind it |
| `tst_a11y` | | **The accessibility walk.** For every page, both directions: Tab alone reaches every enabled control once, in reading order, and comes round; each exposes a name and a role to the accessibility layer |
| `tst_textscale` | | No text elided or cut off at 200 % text size, with strings 40 % longer in a right-to-left script, and both |
| `tst_environment` | | The tests really ran on the offscreen platform, the software renderer, the Basic style, Inter and JetBrains Mono |
| Screenshots | `kit/tests/shots/`, `desktop/tests/screenshots/` | Each page looks as approved. The comparison first proves on made-up pictures that it can fail |
| Sanitizers | preset `ci-sanitize` | All of the above again under AddressSanitizer and UndefinedBehaviorSanitizer |

A new program gets the walks by loading its pages the way `kit/tests/PageHost.qml`
loads the gallery's and calling `Testing.Harness`:

```qml
const findings = Testing.Harness.contrastFindings(page, [Kit.Theme.lineSubtle])
const stops = Testing.Harness.tabStops(page)
const seen = Testing.Harness.accessible(stops[0])     // valid, name, role, ...
Testing.Harness.rightToLeft = true
Testing.Harness.pseudo = Testing.Harness.RtlLong      // every qsTr string longer, in Hebrew letters
```

**The contrast walk** lists every visible item in painting order. For a text
item it takes the topmost opaque rectangle painted before it under the text's
centre; for a rectangle with a border, the one under the border's edge. It
then computes the WCAG ratio of the two colours. Unavailable (disabled) items
are exempt, as WCAG exempts them, and borders in `line.subtle` are decoration.
It fails on translucent text and on text with nothing opaque behind it,
because it cannot judge either.

Reference images: how they are made, compared and changed is in
[desktop/README.md](../../desktop/README.md), "Reference images".

## 4. Departures from the design

Each of these is a place where what was built is not what
[desktop-experience.md](desktop-experience.md) says, with the reason.

| Design | Built | Why |
|---|---|---|
| The builder image is pushed to the project's registry and pinned by digest in `builder.lock` (15.3) | The base image is pinned by digest; the built image is kept in the Actions cache under a key made from the Containerfile and `builder.lock`; the versions the pictures depend on are recorded in `builder.packages` and compared on every run | Pushing needs a registry and write permission that this task could not set up or verify, and the project has a rule against publishing images before Fedora's marks are replaced. The cache gives the same image to every run until it is evicted; that weakness is a known gap |
| A `builder` job that runs when the Containerfile changes, and weekly | A `builder` job in every run that does nothing when the image is cached (about 10 s), and a weekly run that keeps the cache alive | Follows from the row above |
| In-session programs use `org.kde.breeze` and Kirigami; the kit "adds TUR's tokens and the components KDE has no equivalent for" (5.7) | The kit's controls, including a button, a text field, a check box and a switch, are TUR's own, built on Qt Quick Templates and drawn from the tokens. They do not depend on the style and look the same under `org.kde.breeze` and Basic | The kit must also serve the programs outside the session, which may use nothing from KDE (14.1), and AGENTS.md says "use Qt Quick with the project's own controls". A program inside the session still runs under `org.kde.breeze` and may use stock controls and Kirigami beside the kit's. Whether a kit button beside a Breeze button looks like one family is for the first in-session program (D-07) to judge, on a picture |
| Icons are Breeze's, loaded from the system theme (5.4) | The kit draws its own nine line glyphs (four status signs, a check, two arrows, a cross) and P@x's mark | Outside a Plasma session nothing recolours a Breeze icon: Qt's own icon engine draws it in the theme's fixed dark grey, which cannot be read on a dark surface. The engine that recolours is in KIconThemes, which is on the forbidden list. Breeze's icons remain right for in-session programs, through Kirigami |
| Three variants: dark, light, high contrast (15.4) | Dark and light. The high-contrast palette, and the trusted screen's and the boot splash's, are in `tokens.json` and checked for contrast, but there is no high-contrast `Theme` | The design gives four colours for high contrast, not a value for each of the seventeen roles. Mapping them is a design decision, wanted before D-06 |
| Screenshot matrix: variant by direction by scale 1 and 2 (15.4) | Dark and light; left-to-right and right-to-left with strings 40 % longer, at 1x; left-to-right at 1.5x | The task asked for a fractional scale, which finds more than 2x does (rounding of 1 px lines). Right-to-left at 1.5x would add twelve pictures and test nothing the others do not |
| "Qt Quick Test's `grabImage`" makes the pictures (15.4) | `Harness.grab`, which calls the same `QQuickWindow::grabWindow` and crops by the real pixel ratio. The toolchain proof still uses `grabImage` | Section 5, item 5 |
| The kit "reads `org.freedesktop.portal.Settings`" (14.1) | Light or dark comes from Qt's `QStyleHints::colorScheme()`, which the platform theme feeds; the animation speed is read from `kdeglobals` | No D-Bus code that CI cannot exercise. Section 6 |
| `build-test` uploads a formatting and a translation patch (15.3) | Not done | Section 6 |
| `ci.py` gains commands for the artifacts (15.3, D-01) | `ci.py artifact` already fetches them; accepting references is `desktop/ci/update-references.py`, which calls it | `ci.py` is a shared file outside this task. Section 6 |
| Layout of 15.1 | Adds `kit/pax/` (gold, as its own module) and `ci/probe/` (the proof program); `apps/`, `dbus/`, `data/` and `packaging/` hold a README each and no program directories yet | A directory arrives with the task that fills it |

## 5. What D-01 found

Task D-01 was to settle the assumptions of the design's sections 5.7, 15.4 and
16 (risks 8 and 12). Evidence is from the runs named in
[desktop-1.md](../reports/desktop-1.md): the `probe` directory of the
`desktop-build-test` artifact holds each scenario's report and picture.

1. **`qqc2-breeze-style` is available, and is already in the image.** Fedora
   45 has `qqc2-breeze-style` 6.8.2. The package list of the OS image (the
   SBOM of the `image` workflow) has it too, so the Kinoite base ships it;
   nothing needs adding. *Holds.*
2. **A `QGuiApplication` is given `org.kde.breeze` by Plasma's platform
   theme.** With `plasma-integration` 6.8.2 loaded (`QT_QPA_PLATFORMTHEME=kde`)
   and no style asked for, `QQuickStyle::name()` is `org.kde.breeze` before
   the program does anything. Without the platform theme Qt's default is
   `Fusion`. *Holds*, with the caveat that this was the platform theme loaded
   in a container under the offscreen platform, not a Plasma session.
3. **Kirigami takes the system's colour scheme with `org.kde.breeze`.** With a
   `kdeglobals` in TUR's dark colours, `Kirigami.Theme` reported
   `#0b0f17` / `#eef1f6` / `#1267c4` for background, text and highlight, with
   the platform theme and without it; with no `kdeglobals` it reported Breeze
   Light's. It also took the animation setting (`longDuration` 1 ms with
   `AnimationDurationFactor=0`, 200 ms otherwise). *Holds.*
4. **Under the Basic style Kirigami does not take the scheme**: it reported
   `#efefef` and black with the same `kdeglobals`. So the design's fallback
   ("Basic everywhere, reading the scheme through KColorScheme") would need
   that reading written; it is not needed, because 1 to 3 hold.
5. **`grabImage` works under the offscreen platform with the software
   renderer**, at a scale of 1: the proof's test reads the accent colour back
   from the middle of its swatch. Every picture is rendered twice per run and
   the two are identical, pixel for pixel. *Holds, with one limit:* at
   `QT_SCALE_FACTOR=1.5` the window is drawn at 1.5x but `grabImage` cuts the
   item out as if the scale were 1, giving the top-left two thirds. The
   harness's `grab` crops by the real ratio instead. The fallback to a
   virtual KWin is not needed.
6. **Stock controls under `org.kde.breeze` do draw in the software
   renderer**, in the system's colours (`probe/session-theme.png`). The only
   QML warnings were sixteen from the style's own `ProgressBar.qml`
   ("Cannot read property 'top' of null"), which still drew.
7. **Qt Quick Shapes draws in the software renderer**, anti-aliased, which the
   kit's glyphs and spinner rely on.
8. **Fonts.** In the builder, `rsms-inter-fonts` gives the family "Inter" and
   `jetbrains-mono-fonts` "JetBrains Mono". Neither is in the OS image yet
   (task D-04 adds them, as the design says). The image does have Noto Sans
   CJK, Hebrew and Arabic, which answers the design's question about CJK.
   With Plasma's platform theme the default fonts are Noto Sans 10 pt and
   Hack, as the design says.
9. **What the RPMs say about licences.** Fedora's licence tag for
   `kf6-kirigami` includes `GPL-2.0-or-later`, and for `qqc2-breeze-style`
   `GPL-2.0-or-later` and `GPL-2.0-only OR GPL-3.0-only`, beside the LGPL
   ones. A tag covers the whole source package, tests and build scripts
   included, so this neither confirms nor contradicts the design's statement
   that their shipped code is LGPL throughout [Q2]. It is a point for the
   legal review. `tur-kit` links neither.

## 6. Known gaps

- **The builder image is not in a registry.** If the Actions cache is evicted
  the image is rebuilt with that day's Fedora packages, and a Qt or font
  update can then change the pictures. `versions.sh` warns and the procedure
  is documented, but the design's pinned digest is the real fix. It needs a
  decision on where the project keeps images.
- **No high-contrast variant** (X7), and so no high-contrast pictures.
- **The kit does not follow an arbitrary system colour scheme.** Inside the
  session it is TUR dark or TUR light, by the desktop's preference. A user who
  picks another scheme (Breeze High Contrast, say) gets it in KDE's
  applications and in stock controls, not in the kit's. Reading the scheme
  (KColorScheme, optional in the design) is not done.
- **The animation speed is read once**, at start; a change while a program
  runs is not followed (`SystemAppearance.reload()` exists). Whether the
  desktop's light or dark preference is followed while running depends on the
  platform theme reporting it; under the offscreen platform it cannot be
  tested, and the unit test for it is skipped there.
- **A stock control inside a `Kit.Page` under the Basic style** is given TUR's
  colours through the palette, but no test or picture shows one yet. The one
  stock control in a picture (on the proof page) is outside a `Kit.Page` and
  is Qt's default grey.
- **Announcements (X4) are not built.** Nothing calls `Accessible.announce`.
- **The greyscale rendering of each status set (X6) is not built.** Status is
  a sign and a word everywhere, by construction; there is no test of it.
- **Focus pictures (X2)**: one control per page is photographed with keyboard
  focus, not every class of control.
- **The contrast walk judges text and borders, not fills.** A filled control
  against its surroundings (the selected row, the progress bar) is covered by
  the token table only. A border that is exactly black and 1 px wide is taken
  for "no border", because that is what an unset border reports; no token is
  black.
- **The accessibility walk reads Qt's accessibility interfaces, not AT-SPI.**
  What a screen reader really receives is the end-to-end test's job (D-05).
  No person who relies on assistive technology has tried any of this.
- **The 200 % text test does not notice a control that outgrows its
  container**, only text that is elided or cut.
- **No formatting patch and no translation extraction** in `build-test`.
  `qmlformat` and `lupdate` are in the builder; `clang-format` is not.
- **Install rules** exist since TUR-011: the kit's two libraries (`libturkit`,
  `libturkitpax`) go into the image with the programs that use them
  (`tur_install_modules`; image/README.md, "The desktop programs"). The
  gallery and the test harness are not installed.
- **`Message` takes its words as properties.** The registry loader that turns
  a code into them (specification 29.3) does not exist yet.
- **Fixed-width text looks heavier on the dark variant** than on the light in
  the pictures. The font and weight asked for are the same in both. The cause
  was not established; the likely one is how the software renderer blends
  light text on a dark ground. Not checked on a real display.
- **The pseudo-locale writes right-to-left text in Hebrew letters**; Arabic
  shaping is not exercised.

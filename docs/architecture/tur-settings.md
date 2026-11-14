# tur-settings: TUR's settings, and the addressing of every setting

How `tur-settings` is put together, what a settings address is and what
happens to one, the two D-Bus interfaces as they were built, and how each
state of the design is reached. The design is
[desktop-experience.md](desktop-experience.md), sections 9.5 and 10; the
requirement behind the addressing is the AI control layer's
([ai-control-layer.md](ai-control-layer.md), 16.6): every settings page and
each row on it has a stable identifier, so that P@x can open exactly the page
and point at exactly the row it is talking about. The kit it is built on is in
[desktop-kit.md](desktop-kit.md).

Status: task D-07 of the design's plan, "Settings shell and addressing". The
shell, the search, the pages Overview, Appearance and About, the registry and
its schema, the command line and the link, and `org.tur.Settings1` and
`org.tur.Addressable1`. It changes no setting. Report:
[desktop-3.md](../reports/desktop-3.md).

## 1. Structure

Code: `desktop/apps/tur-settings/`. One QML module, `Tur.Settings` (library
`libtursettings`), and the program `tur-settings`, which is a window around it.

| Part | Files | What it is |
|---|---|---|
| Addresses | `src/address.*` | The grammar of an address, and nothing else |
| The registry | `src/registrydata.*`, `src/registry.*`, `registry/*.json` | The loader, with every rule of the format; the search; `Registry`, the one registry of the program, for the views |
| What the system is | `src/systemfacts.*`, `fixtures/` | Reads the two files the image build writes |
| Texts | `src/documents.*` | The licence, the required notice and the third-party notices, compiled in from the top of the repository |
| An address becomes something on screen | `src/settingshost.*`, `qml/Shell.qml` | Section 3 |
| The bus | `src/busadaptors.*`, `src/bustypes.h`, `dbus/*.xml` | Section 4 |
| Codes | `src/codes.*` | `TUR-DESKTOP-SETTINGS-NNN` |
| A page, a group, a row | `qml/SettingsPage.qml`, `SettingsGroup.qml`, `SettingRow.qml`, `RowHighlight.qml`, `LinkRow.qml`, `ChoiceOption.qml`, `DocumentText.qml` | Section 5 |
| Pages | `qml/OverviewPage.qml`, `AppearancePage.qml`, `AboutPage.qml`, `SearchResults.qml` | Section 5 |
| Data that is installed | `data/` | The launcher entry and link scheme, the entry in KDE System Settings, the D-Bus service file |

The C++ does what the design's 15.2 allows it and nothing else: it reads
files, holds a registry, answers on the bus and starts one program. It decides
nothing about any setting, and nothing in this program changes one.

## 2. The registry

**An address** is a page, or a row on a page (design, 10.3):

```text
address   = page [ "#" row ]
page      = namespace "." segment *( "." segment )
namespace = "tur" | "kde"
segment   = 1*( a-z | 0-9 | "-" | "_" )
row       = 1*( a-z | 0-9 | "-" )
```

A page address is at most 120 characters and a row identifier at most 64.
Nothing is trimmed, lower-cased or guessed: text that does not follow the
grammar exactly is not an address.

**A registry** lists the addresses of one program. The format is
[`schemas/settings-registry.schema.json`](../../schemas/settings-registry.schema.json);
a file is named after its program (`org.tur.Settings.json`) and is to be
installed under `/usr/share/tur/settings/`.

```json
{
  "licence": "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
  "schema_version": 1,
  "program": "org.tur.Settings",
  "pages": [
    {
      "page": "tur.appearance",
      "title": "Appearance",
      "path": ["Appearance"],
      "keywords": ["look", "theme", "colours", "colors", "style"],
      "sections": [{ "id": "variant", "title": "Light or dark" }],
      "rows": [
        {
          "id": "variant",
          "title": "Dark, light, or light by day and dark at night",
          "keywords": ["dark mode", "light mode", "night mode"],
          "kind": "choice",
          "section": "variant",
          "source": { "type": "kde-config", "available": false,
                      "waits_for": "TUR's two global themes (task D-04)",
                      "file": "kdeglobals", "group": "KDE", "key": "LookAndFeelPackage" },
          "change": { "class": 1 }
        }
      ]
    },
    { "page": "tur.pax.privacy", "title": "Privacy", "path": ["P@x", "Privacy"], "status": "planned" }
  ]
}
```

What the design's illustration has, with these additions:

| Field | Meaning |
|---|---|
| `program` | The program's application identifier, which is also the D-Bus name on which it offers `org.tur.Addressable1` |
| `status` of a page | `available` (the default); `planned`: the address is reserved and the page is not built, so it cannot be opened; `removed` |
| `status` of a row | `available`, or `removed`: only the address, the last title and `replaced_by` are kept |
| `listed` | `false` for a page that is in the list only while a feature is on (`tur.macos`) |
| `sections`, and `section` of a row | The titled groups of a page. Sections are not addressable, so a row that moves to another keeps its address |
| `source` | **What backs the row**: a key of a file, a D-Bus service, a command, a key of a KDE configuration file, a hint of the desktop, something the program knows by itself, or another address that the row opens. `available` says whether that exists in the system today, and `waits_for` what is missing |
| `open`, `verified_with`, `locator` | For KDE System Settings (design, 10.4): the module that opens a page, the Plasma version it was checked against, and how a control is found. The locator is task D-20; nothing resolves one yet |

**The rules a schema cannot express** are applied by the program's loader
(`registrydata.cpp`) and by `desktop/ci/registry.py`: a page address occurs
once in all registries, a row identifier once on its page; a row names a
section its page declares; a `kde` page says how it is opened and a `tur` page
does not; a page that is planned or removed has no row that is still there;
`replaced_by`, and the address a row opens, exist and are not removed. A file
that breaks a rule is not loaded at all.

**An address is never deleted.** `registry.py --against REF`, in the
`boundary` job, fails when an address that the main branch has is gone
without a `removed` entry, or when a removed one has come back.

**What the program loads.** Its own registry and
`registry/org.kde.systemsettings.json` are compiled in. On top of them it reads
every `*.json` in `--registry DIR` (default `/usr/share/tur/settings`, where
that exists), skipping a program it already carries.

`org.kde.systemsettings.json` holds the four pages of KDE System Settings that
the Appearance page leads to. It is a stand-in for the design's
`kde-settings-map.json` (task D-20) and is to be replaced by it. Its
`verified_with` means that the module's metadata file exists under that name
in Plasma's source at tag v6.8.2; no module was opened.

**One string.** The title of a row on screen is the registry's: `SettingRow`
takes an address and reads its title from the entry. So what the search finds,
what is said over the bus and what a person reads cannot differ.

**Search** (`RegistryData::search`). Every word of the query must match the
title, a keyword, the section or the page of an entry; a whole word counts for
more than the beginning of one, the title for more than a keyword. Pages that
are planned or removed and rows that are removed are not found. Among equal
scores TUR's own entries come before KDE's.

## 3. What happens to an address

One path, whoever asks: the command line, a link, a match of the search, a
button of a page, `org.tur.Settings1`.

```text
address ─▶ SettingsHost.perform()
             │ not an address                      TUR-DESKTOP-SETTINGS-004
             │ no registry has it                  …-001
             │ removed                             …-002, naming its successor
             │ a page that is reserved             …-003
             ├─ a page of KDE System Settings ──▶ systemsettings <module>      (…-005 if it cannot be started)
             ├─ a page of another TUR program ──▶ its org.tur.Addressable1     (…-005 if it does not answer)
             └─ a page of this program ─────────▶ Shell.show(page, row, options)
```

`Shell.show`:

1. shows the page; another page begins at its top;
2. finds the row by its address among the items of the page;
3. opens the group the row is in, if it is closed;
4. **settles the layout** (`SettingsHost::settle`): every layout under the
   window does at once what it would do before the next frame, so that the
   row's position can be read whether or not the window has been drawn since
   the page changed;
5. scrolls the row into sight, with 16 px to spare;
6. if asked, marks the row: `RowHighlight` is a child of the row, so the line
   stays on it while the page scrolls or the window changes size;
7. gives the keyboard to the row's control, or to the row itself when it has
   none that can be used; what already has the keyboard keeps it (a card is a
   focus scope: a row that let go for a moment would hand the keyboard to its
   card, and the page would scroll to show the whole card);
8. returns the row's rectangle in the window.

**The highlight** (design, 9.5): a rounded line 3 px wide, 4 px clear of the
row, with a 6 px corner. It is gone when `ClearHighlight` is called, when
another address is shown, when the person chooses another page or searches,
on Escape, and after `duration` seconds if one was given. With `dim`, the rest
of the window is darkened by a fifth, in four pieces that leave the row and
its line clear and are placed again before every frame.

**Codes.** Every refusal has one; over the bus it is the beginning of the
error's message, and in the window it is the code of the kit's two-layer
message.

| Code | D-Bus error | When |
|---|---|---|
| `TUR-DESKTOP-SETTINGS-001` | `org.tur.Settings1.Error.UnknownAddress` | Well formed, and no registry has it |
| `-002` | `…RemovedAddress` | It was removed; the message names its successor |
| `-003` | `…PageNotAvailable` | Reserved for a page that is not built |
| `-004` | `…NotAnAddress` | Does not follow the grammar |
| `-005` | `…OwnerUnreachable` | KDE System Settings could not be started, or the program that owns the address did not answer |
| `-006` | `…RowNotShown` | Registered, and the page does not show it: a fault of this program |
| `-007` | — | A registry file breaks the format (printed at start) |
| `-010` | — | `/usr/lib/tur/release` cannot be read (shown on Overview and About) |

They are this program's own and, like the application centre's, are not yet
registry entries in the sense of specification 29.3; their sentences are in
`src/codes.cpp` and `src/settingshost.cpp` until a registry holds them.

## 4. The interfaces, as built

The XML is `desktop/apps/tur-settings/dbus/org.tur.Settings1.xml` and
`org.tur.Addressable1.xml`. Both are **proposed, not frozen**. The adaptors are
written by hand (`src/busadaptors.h`); no code is generated from the XML, and
a test holds what the program offers on the bus to the two files.

```text
org.tur.Settings1        session bus; name org.tur.Settings1; path /org/tur/Settings1
  Methods    Open(s address)                       an empty address brings the window forward
             Reveal(s address, a{sv} options) -> a{sv} result
             ClearHighlight()
             Resolve(s address) -> s               JSON: address, program, page, row
             Search(s text, u limit) -> a(ssd)     address, title, score; limit 0 is 20; at most 100
  Property   Address (s, read)                     where the window is

org.tur.Addressable1     session bus; name org.tur.Settings (the registry's "program");
                         path /org/tur/Addressable1
  Methods    Reveal(s address, a{sv} options) -> a{sv} result
             ClearHighlight()
             List() -> s                           this program's registry, as JSON
  Signal     RowChanged(s address)
```

**Options of `Reveal`:** `highlight` (b, default true), `duration` (d,
seconds; 0, the default, keeps the highlight), `dim` (b, default false), `pax`
(b, default false: the highlight is the system's blue; true: the caller is the
assistant, and the highlight is P@x's gold).

**The result of `Reveal`:**

| Key | Type | Meaning |
|---|---|---|
| `address`, `page`, `row` | s | The address and its two parts |
| `program` | s | The application identifier of the owner |
| `located` | b | Whether the rectangle is known |
| `rect` | (dddd) | x, y, width and height of the row relative to the top left corner of the window's content, in the window's own (logical) pixels: what `org.tur.PaxOverlay1.HighlightInWindow` takes. For a page, the area the page is shown in |
| `scale` | d | Device pixels to one logical pixel |
| `highlighted`, `focused` | b | Whether the highlight was drawn and the keyboard is on the row |
| `pid`, `app_id`, `window_title` | u, s, s | What the overlay needs to find the window |

For a page of KDE System Settings the result has `located` false and no
`rect`: System Settings is started at the module, and a row of it cannot be
pointed at until the locator of task D-20 exists. Nothing is made up.

`org.tur.Addressable1` on `org.tur.Settings` speaks for this program's own
pages only; an address of another owner is refused there and nothing is
opened. `org.tur.Settings1` is the single entry point: it passes an address
of another TUR program on to that program's `org.tur.Addressable1.Reveal`
(for `Open`, with `highlight` false) and returns its answer; an error of the
`org.tur.` family is passed back as it is.

**The command line.**

```text
tur-settings [address] [--fixture DIR] [--registry DIR] [--system-settings-program PROGRAM]
             [--standalone] [--variant dark|light] [--rtl] [--developer] [--no-dbus] [--check]
tur-settings --search WORDS | --resolve ADDRESS | --list
```

- `tur-settings tur.appearance#variant` opens the page with the row in sight,
  focused and highlighted. A page address opens the page.
- **The link** is the same address with the scheme in front:
  `tur-settings:tur.appearance#variant` (`%23` for `#` is accepted after the
  scheme). `data/org.tur.Settings.desktop` registers the scheme
  (`MimeType=x-scheme-handler/tur-settings;`, `Exec=tur-settings %u`).
- **One window.** A second start hands its address to the window that is open
  (`Reveal` for a row, `Open` for a page or for nothing) and ends: status 0,
  or 1 with the error on standard error.
- **Started on demand.** `data/org.tur.Settings1.service` lets the bus start
  the program when `org.tur.Settings1` is called and no window is open.
- `--search`, `--resolve` and `--list` answer from the registries as JSON and
  open no window; they need no display. Status 0; 6 when nothing matches; 2
  for an address that is unknown or is not one. This is what a command
  adapter of P@x can run in its sandbox (section 8).
- `--developer` shows the address of what is on screen at the foot of the
  window. Addresses are shown nowhere else (design, 10.3, rule 2).

**The entry in KDE System Settings** (design, 10.1) is
`data/systemsettings/org.tur.Settings.desktop`, to be installed in
`/usr/share/plasma/systemsettings/externalmodules/`.

**Where things are to be installed.** Nothing is installed yet (there are no
install rules under `desktop/`; [desktop-kit.md](desktop-kit.md), section 6).

| File | Destination |
|---|---|
| `tur-settings` | `/usr/bin/` |
| `registry/org.tur.Settings.json` | `/usr/share/tur/settings/` |
| `data/org.tur.Settings.desktop` | `/usr/share/applications/` |
| `data/systemsettings/org.tur.Settings.desktop` | `/usr/share/plasma/systemsettings/externalmodules/` |
| `data/org.tur.Settings1.service` | `/usr/share/dbus-1/services/` |

## 5. The shell and the pages

The window is `Shell`: the search field and the list of pages on one side, one
page on the other, and under the list the way into KDE System Settings.

**The list** is the registry's: every page of this program that is listed,
under its registered title, in the registry's order, which is the design's
(10.2). The pages this task does not build are in the list and cannot be
opened. P@x's eight pages stand under a heading that is closed until asked for
or until one of them is gone to. The list scrolls by itself in a low window.

| State (design) | Reached by | What shows it |
|---|---|---|
| The front page (10.1) | The window opens there; `tur.overview` | `OverviewPage` |
| A page (10.2) | The list; an address | `SettingsPage`, with its `SettingsGroup`s and `SettingRow`s |
| "Displays, sound, network and more" (10.1) | The last row of the side | Starts `systemsettings` |
| Search (10.1, 10.3) | Typing in the field; Ctrl+F goes to it | `SearchResults`: each match with where it is; choosing one goes there and highlights it |
| Search: nothing found | Words no registry has | Says where the rest of the settings are, with "Open System Settings". It does not say that there is no such setting: most of KDE's are in no registry yet |
| A row asked for by its address (9.5, 10.3) | The command line, a link, a match, the bus | In sight, focused, highlighted |
| … by the assistant | `Reveal` with `pax` | The line is gold |
| … with the rest dimmed | `Reveal` with `dim` | `Shell`'s shade |
| An address that cannot be opened | Any refusal of section 3 | The kit's message above the page, with the code |
| Developer mode (10.2) | `--developer` | The address at the foot of the window |
| The two layers of a page (10.2) | "Technical detail" on About | A group that is closed until asked for; a row in it that is asked for by its address opens it |

**What each page reads, and what it does not have.**

| Row | Source | Today |
|---|---|---|
| Overview: Version, Build | `/usr/lib/tur/release` (`TUR_VERSION`, `TUR_BUILD_ID`), written by `image/Containerfile` | Read |
| Overview: Updates | `org.tur.Update1` | **Not available.** The row says that TUR cannot say here whether an update is waiting, and shows no value |
| Overview: The last start | `tur-boot-health status --json` | **Not available**: "TUR does not show here yet how the last start went: this page cannot read the record of each start." The program is in the image since ADR-0014 was implemented; this page does not run it yet (section 7) |
| Overview: Needs attention | The services above, storage and devices | **Not available.** "That nothing is listed does not mean that nothing is wrong." |
| Appearance: The desktop is set to | The desktop's colour-scheme preference, as Qt reports it | Read. If the window was started in a variant of its own (`--variant`), the row says that it does not follow the desktop |
| Appearance: Dark, light, or light by day and dark at night | Plasma's global theme (`kdeglobals`, `[KDE] LookAndFeelPackage`) | **Not available**: TUR's two global themes are task D-04. The three options are drawn, cannot be chosen, and none is marked, because which applies cannot be read either |
| Appearance: Global theme, Colours, Fonts, Night Light | `kde.kcm_lookandfeel`, `kde.kcm_colors`, `kde.kcm_fonts`, `kde.kcm_nightlight` | Each opens System Settings at that module |
| About: Version, Build, Base | `/usr/lib/tur/release` | Read |
| About: TUR and the Fedora Project | The statement of origin (design, 12.2) | The second of the design's two forms, until the owner and the lawyer decide (section 6) |
| About: TUR's licence; Software from other projects | `LICENSE.md`, `NOTICE`, `THIRD_PARTY_NOTICES.md`, compiled in | Shown word for word, closed until asked for |
| About, Technical detail: Source revision, Base image, What the system calls itself, This program | `/usr/lib/tur/release`; `os-release` (`PRETTY_NAME`, `ID`); the program | Read |

A value that a file does not hold is "Not recorded". Where
`/usr/lib/tur/release` cannot be read at all, Overview and About say so with
`TUR-DESKTOP-SETTINGS-010`, the file's name and what the system does call
itself. A row whose source is not available says so in words that meet the
contrast requirement, not in the colour of unavailable controls: why it is
missing is what the person came to read.

**Fixtures** (`fixtures/README.md`). `--fixture DIR` reads `DIR/tur-release`
and `DIR/os-release` instead of the system's files. Three sets: a TUR image as
it is built today, one after Fedora's marks are replaced, and a system that
is not TUR. They were written by hand after `image/Containerfile`, not copied
from a built image. There is no fixture for a service that does not exist.

## 6. Departures from the design

Each of these is a place where what was built is not what
[desktop-experience.md](desktop-experience.md) says or draws, with the reason.

| Design | Built | Why |
|---|---|---|
| The directory is `apps/settings/` (15.1) | `apps/tur-settings/` | The task named it so, as for the application centre |
| `tur-kit` provides `SettingsPage` and `SettingRow` (10.3; `desktop/kit/settings/`) | They are in the program: `qml/SettingsPage.qml`, `SettingsGroup.qml`, `SettingRow.qml` | The task allowed additions to the kit only where needed, and one program uses them. They move to the kit with the second program that shows a settings page (D-08) |
| Interface XML is in `desktop/dbus/` (15.1) | `desktop/apps/tur-settings/dbus/` | That directory was outside this task's paths. To move when the interfaces are frozen |
| The System Settings entry is in `desktop/data/defaults/` (15.1) | `desktop/apps/tur-settings/data/systemsettings/` | The same |
| The highlight is a 3 px gold line with a 1 px dark line outside it (9.5) | Gold only when the caller says it is the assistant (`pax`); otherwise the system's blue. No dark line | Gold means P@x and nothing else (5.1), and a row opened from the command line, a link or the search was not asked for by P@x. The dark line is for a ground the overlay does not control; inside the program the ground is the card, and both lines meet 3:1 on it |
| Options of `Reveal`: `highlight`, `duration`, `dim` (10.3) | And `pax` | The row above |
| `org.tur.Settings1`: `Open`, `Reveal`, `Resolve`, `Search` (10.3) | And `ClearHighlight`, and the property `Address` | The tool `settings_clear` needs a method on the entry point; `Address` lets a caller, and a test, see where the window is |
| `Open(s address)` | An empty address brings the window forward where it is | What a second start with no address needs |
| "A row without an `address` does not compile into a page" (10.3) | The property is required: a page with such a row does not load, and the tests of every page fail | QML is not compiled ahead of time (desktop/README.md). Whether `qmllint` reports a missing required property was not tried |
| A removed address: "unknown address, removed address (with its successor), page not available" (10.3) | Those three, and three more codes (section 3) | Something that is not an address, an owner that does not answer, and a row that is registered and not shown are different faults |
| `kde.<module>` for every module; rows by a locator (10.4) | Four pages of KDE's, no rows; a row of a KDE page is opened at its page and reported as not located | D-20 |
| The side list ends with "Displays, sound, network and more →" (10.2) | The same words, with "Opens System Settings" under them and no arrow | The kit's row has a second line and no trailing sign; the words say what happens |
| P@x's pages are a group with an arrow (10.2) | A heading that opens and closes, closed at first | The drawing shows it open because a P@x page is selected |
| Overview holds "update state, health of the last start, anything needing attention" (10.2) | Three rows that say they cannot tell | Nothing reports them to a desktop program yet |
| Appearance: "Dark, light, or light by day and dark at night" (10.2) | Drawn, and cannot be used | D-04 |
| In-session programs use Kirigami and stock controls under `org.kde.breeze` (5.7) | `Tur.Kit`'s controls only; the one stock control is a scroll bar. One control the kit lacks, the radio button, is drawn in the program (`ChoiceOption.qml`) | As for the application centre |
| Error text comes from the code's registry entry (5.6) | Sentences in the program | No registry of error codes exists |
| The statement of origin is one of two forms, to be decided (12.2, question 3) | The second form, word for word | It claims no mark. It is in one place, `qml/Words.qml` |

**Words that are not the design's.** The titles of the sections ("This
system", "State of the system", "Light or dark", "Everything else", "Where TUR
comes from", "Licences"); the titles of rows the design does not name ("The
desktop is set to", "TUR and the Fedora Project", "TUR's licence", "Software
from other projects", "Source revision", "Base image", "What the system calls
itself", "This program"); every sentence that says why something is not
available; "Not recorded" and "Not available yet"; "Open in System Settings";
"Read the licence" and "Read the third-party notices"; the sentences of the
search; and the sentences of the eight codes. They are in the registry,
`qml/Words.qml`, the pages and `src/`, and are for the design's owner to
confirm or replace.

## 7. Gaps

**Not built**, each with what it waits for:

- **Changing anything.** No row of this task changes the system.
- **The last start.** `tur-boot-health` was merged while this task was under
  way. The row still says "not available": reading `tur-boot-health status
  --json` belongs with the Diagnostics page (D-08) and the Recovery page
  (D-09), which need the same answers.
- **KDE's settings**: the mapping file, the locator and the pointing at a row
  of System Settings (D-20). Only four pages are registered.
- **The launcher's search** (`tur-search`, D-13), which is to call
  `org.tur.Settings1.Search`.
- **P@x's tools.** Section 8.
- (Built since, TUR-011: install rules; `tur-settings`, its launcher entry,
  its bus activation file and its entry in System Settings are in the
  image.)
- **Translations.** Every sentence goes through `qsTr`; registry titles go
  through `qsTranslate` with the context `SettingsRegistry`. No catalogue is
  extracted or loaded (design, L9).
- **Announcements** (X4), **high contrast** (X7): as for the kit.
- **Raising the window under Wayland.** `Reveal` asks the window to show
  itself, raise and activate. A Wayland compositor activates a window only
  for a caller that passes an activation token; none is passed.

**Known weaknesses.**

- **The sender of `pax` is not checked.** Any program in the session can ask
  for a gold highlight. Gold is a convention of presentation, not a
  protection, and Plasma 6.8 lets any program in the session draw anything
  (design, risk 1); but if gold is to mean "P@x asked" strictly, the option
  must be honoured only for the bus name of `tur-pax-overlay` (D-15).
- **A button does not wrap.** "Open in System Settings", at 200 % text and
  40 % longer, is wider than its card. The text-size test looks at text that
  is elided or cut, not at a control that outgrows its container
  ([desktop-kit.md](desktop-kit.md), section 6).
- **The registry is read once**, at start. A registry file that another
  program installs later is seen at the next start.
- **`RowChanged` is sent for the page on screen only.**
- **The fixtures were written by hand** and are not held to a built image.
- **Dimming can hardly be seen in the dark variant**: a fifth of the darkest
  tone over a dark window.
- **A match pressed before the first frame** after the matches appear reaches
  the last match: until a layout has placed them, they lie on top of one
  another. A person cannot press that fast; a test did.

## 8. Tools for P@x

The design (10.3) names three tools, "declared in the manifest of
`tur-settings`". **No manifest is shipped by this task**, for three reasons
that are each somebody's to settle:

1. A tool manifest may hold no key but its three
   (`schemas/agent-tool-manifest.schema.json`; P@x's loader refuses an unknown
   one), and every file under `desktop/` must carry an SPDX identifier in its
   first lines (`desktop/ci/boundary.py`). A JSON file can do both only with a
   `licence` key. One of the two rules has to give way for manifests.
2. P@x performs a tool today by running a command in its read-only sandbox
   (`schemas/pax-tool-adapter.schema.json`). `settings_search` can be
   performed that way, and `tur-settings --search` exists for it.
   `settings_reveal` cannot: it needs the session bus and opens a window. It
   waits for an adapter that calls D-Bus.
3. A component's own test loads its two files with P@x's loader
   (`tests/agent_tools.rs`). `tur-settings` is not a Rust component, and the
   desktop builder has no Rust.

The manifest, as it would be:

```json
{
  "schema_version": 1,
  "service": "org.tur.Settings1",
  "tools": [
    {
      "name": "settings_search",
      "description": "Find a setting by what a person would call it. Returns the address, the title and a score for each match. Nothing is opened and nothing is changed.",
      "class": 0,
      "method": "Search",
      "input_schema": {
        "type": "object",
        "properties": { "text": { "type": "string", "minLength": 1, "maxLength": 200 } },
        "required": ["text"],
        "additionalProperties": false
      }
    },
    {
      "name": "settings_reveal",
      "description": "Open the settings at an address from settings_search, bring the row into view and highlight it. Returns where the row is in the window. Opens a window; changes no setting.",
      "class": 1,
      "method": "Reveal",
      "input_schema": {
        "type": "object",
        "properties": { "address": { "type": "string", "minLength": 1, "maxLength": 185 } },
        "required": ["address"],
        "additionalProperties": false
      }
    },
    {
      "name": "settings_clear",
      "description": "Remove the highlight in the settings window.",
      "class": 0,
      "method": "ClearHighlight",
      "input_schema": { "type": "object", "properties": {}, "additionalProperties": false }
    }
  ]
}
```

and the one adapter that can exist today:

```json
{
  "schema_version": 1,
  "service": "org.tur.Settings1",
  "adapters": [
    {
      "tool": "settings_search",
      "kind": "command",
      "program": "/usr/bin/tur-settings",
      "arguments": [{ "option": "--search", "input": "text" }],
      "success_exit_codes": [0, 6]
    }
  ]
}
```

Neither was loaded by P@x's loader; both are written from the two schemas and
are a proposal.

## 9. Tests

All in CI, in the `desktop` workflow.

| What | Where | Proves |
|---|---|---|
| The registries | `desktop/ci/registry.py`, and `--self-test`, in the `boundary` job | Every registry follows the schema and the rules of section 2; no address of the main branch was dropped. The self-test plants one mistake of each kind and requires that each is reported |
| `tst_settings_registry` | `tests/unit/` | The grammar of an address; the program's own registries load in the program's loader; every rule of the format, on a registry that breaks it; the search |
| `tst_settings_facts` | | The syntax of os-release; each fixture set; a system that is not TUR; the texts are the repository's; the codes |
| `tst_settings_bus` | | **The addressing over D-Bus**, on a session bus of the test's own (section 9.1) |
| `tst_shell`, `tst_pages` | `tests/` | The list, the search, the way into System Settings, developer mode; each page, with the exact words on screen and what it says about what it cannot read |
| `tst_registry_pages` | | **The registry and the pages, in both directions**: every row a page shows is registered under that page, and every registered row is shown; its title, kind, section and availability are the registry's |
| `tst_addressing` | | **Every registered row** is revealed in a window too low to show it, and is then where the program said, in sight, focused and marked; asked for again, it does not move; the line's pixels, in blue and in gold, in both variants; dimming; duration; a closed group; pages; the link; every refusal with its code |
| `tst_walks` | | The kit's contrast walk (both variants), accessibility walk (both directions) and text-size test (200 %, right-to-left with longer strings, both), over each of the ten states the pictures are taken of |
| Screenshots | `tests/shots/render_settings.qml`, `desktop/tests/screenshots/settings-*.png` | Each state looks as approved, in the kit's matrix |
| `settings-starts*`, `settings-refuses*`, `settings-answers*`, `settings-desktop-files` | `CMakeLists.txt` | The program itself starts on a page, a row and a link with no QML warning; refuses what it cannot open, with the code; answers `--search`, `--resolve` and `--list` with no display at all; its desktop files are valid |

### 9.1 The bus test

`tst_settings_bus` starts a `dbus-daemon` that nothing else is on and stops it
at the end. Three steps.

1. **The window in the test's own process**, on the bus, called from a second
   connection. Here the test can see the window. For each of four rows (one in
   sight, one with controls that cannot be used, one below the window's edge
   in a closed group, one with a control that takes the keyboard):
   - `Reveal` is called over the bus with no highlight, and its `rect` is
     compared with the row's own geometry once the window has drawn;
   - **the pixels that belong to the row** are found by making the row
     invisible and comparing two pictures of the window: every pixel that
     changed lies inside `rect`, and they begin at its left and top edges;
   - `Reveal` is called again with the highlight: the same rectangle, and the
     line's colour is read from the picture 6 px outside each edge, with no
     line in the 4 px between; then with `pax`, where the line is gold; then
     `ClearHighlight`.

   Also there: what the bus offers against the two XML files; `Search`,
   `Resolve`, `List`, `Open`, the property `Address`; every error with its
   name and code; `RowChanged`; a page of KDE's, with a stand-in for System
   Settings that records what it was asked to open.
2. **The program itself**, started by hand. It reports the rectangles that the
   window of step 1 reported, so what was seen in the picture there holds for
   the program. A second start hands over its address and ends, for a row, a
   page, both as links, nothing, and three addresses that are refused. A
   caller that is not Qt (`busctl`) gets the same rectangle. An address of
   another program is passed on to a stand-in that owns `org.tur.Example`, and
   refused with `-005` when that program is not there.
3. **The program started by the bus** from its service file, because a call
   arrived and no window was open: the first call it ever receives is a
   `Reveal`, and it answers with the same rectangle.

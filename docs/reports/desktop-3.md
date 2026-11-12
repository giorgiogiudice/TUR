# desktop-3 — tur-settings: the shell and the addressing (D-07)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-3 (task D-07 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop (desktop/apps/tur-settings), desktop CI, the
                  settings-registry schema
Build ID:         not applicable: this task builds no image
Source revision:  515b665 (feat/tur-settings, pull request 47), which is
                  the commit the results below come from. The commit that
                  adds this report follows it and changes no code.

SUMMARY
- What changed: TUR has its own settings program. tur-settings is a
  standalone Qt Quick program on Tur.Kit with the shell of the design's
  section 10, a search over the settings registries, and the pages
  Overview, Appearance and About. Every page and every row has an address
  that is registered in a file with a schema. The program is opened at an
  address from the command line, from a link and over D-Bus
  (org.tur.Settings1, org.tur.Addressable1); it shows the page, brings the
  row into sight, gives it the keyboard, highlights it, and reports the
  row's rectangle in its window.
- Why it changed: the assistant is to "help the user find the settings,
  even pull up the settings and point the exact lines with its pointer"
  (AI control layer, 16.6). This is the part of that which is the settings
  program's: a stable address for every setting, and an answer to "where
  on screen is it".
- What it does not do: it changes no setting. Three rows of Overview and
  one of Appearance have nothing behind them yet; each is shown, says that
  it cannot tell or cannot be used and why, and shows no value. It does
  not point: the pointer is the overlay's (D-15). It does not locate a row
  inside KDE System Settings (D-20).

FILES
- Added: desktop/apps/tur-settings/ (CMakeLists.txt; src/: address,
  codes, registrydata, registry, systemfacts, documents, bustypes,
  busadaptors, settingshost, main; qml/: 15 files; registry/: 2 files;
  dbus/: 2 interface files; data/: 2 desktop files and a D-Bus service
  file; fixtures/: 3 sets and a README; tests/: 5 QML test files,
  SettingsStage.qml, tst_main.cpp, shots/render_settings.qml, unit/: 3 Qt
  Test programs and 2 stand-in programs, data/);
  schemas/settings-registry.schema.json; desktop/ci/registry.py;
  desktop/tests/screenshots/settings-*.png (60);
  docs/architecture/tur-settings.md; docs/reports/desktop-3.md
- Modified: desktop/CMakeLists.txt (one add_subdirectory);
  desktop/ALLOWED_IMPORTS (five lines, below); desktop/ci/screenshots.sh
  (renders the settings too); desktop/ci/in-builder.sh (the runner's user
  has a name in the container); desktop/ci/builder/Containerfile (two
  packages); .github/workflows/desktop.yml (the registry check, in the
  boundary job)
- Removed: none

BUILD
- Command: desktop/ci/build.sh ci, in the builder image, in workflow
  "desktop". C++20, -Wall -Wextra -Werror. Again with preset ci-sanitize.
- Result: success, run 43236651205. 224 build steps.
- Warnings: none from the compiler; none from qmllint. CMake prints the
  warnings about Kirigami plugins of tur-probe that were there before.

TESTS
- Static (job "boundary"): the licence boundary over the files of
  desktop/, and its self-test; the token check; the registry check:
  2 registry files, 24 pages and 21 rows follow the schema and the rules
  of tur-settings.md, section 2; its self-test plants 27 mistakes and
  requires each to be reported. Pass. The comparison with the main branch
  found no registry there yet, so it had nothing to lose; that it can
  fail was seen on made-up revisions in the self-test only.
- Linked libraries: every shared library the built programs need is on
  the allow list. Pass.
- Unit tests (Qt Test), 3 new programs:
  tst_settings_registry, 115 functions and data rows: the grammar of an
  address (9 read, 24 refused); the program's registries in the program's
  loader; 52 registries that each break one rule, each refused whole with
  the right words; the search.
  tst_settings_facts, 27: the syntax of os-release; each fixture set; a
  system that is not TUR; the texts; the codes.
  tst_settings_bus, 38: the addressing over D-Bus on a private
  session bus (below).
  Pass.
- QML tests, 5 new files run by one program, headless, 204 functions
  and data rows: the shell (20), the pages (19), the registry against the
  pages in both directions (7), the addressing (86), and the kit's
  walks over the ten states the pictures are taken of (72: contrast in
  both variants, keyboard and accessible names in both directions, text
  at 200 %, right-to-left with strings 40 % longer, and both). Pass.
- Program starts: tur-settings starts on a fixture, on a page, on a row,
  on a link and where there is no TUR, each with no QML warning; refuses
  an unknown address and a reserved page with the code; answers --search,
  --resolve and --list with no display at all. 10 tests. Its two desktop
  files pass desktop-file-validate. Pass.
- The whole of desktop/: 775 test functions and data rows passed, 1
  skipped (the kit's, as before), in 30 tests, in 70 s. The same under
  AddressSanitizer and UndefinedBehaviorSanitizer, in 103 s. Pass.
- Screenshots (job "screenshots"): 163 pictures, of which 60 are new,
  each rendered twice; the two renderings are identical; each matches its
  reference with no pixel beyond 8/255 in a channel. The 103 pictures of
  the kit and the application centre did not change, though the builder
  image was rebuilt. Pass, run 43236651205.
- In a Plasma session, under org.kde.breeze, beside KDE's applications:
  NOT RUN. The tests and the pictures use the Basic style.
- Under a Wayland compositor: NOT RUN. Everything ran on Qt's offscreen
  platform. Whether the window comes forward when Reveal is called, and
  what application identifier KWin sees, were not observed.
- With KDE System Settings: NOT RUN. The builder has none. A stand-in
  recorded which module tur-settings asked for. That System Settings shows
  the entry "TUR", and that the four module identifiers open what they
  name, were checked in Plasma's source at v6.8.2 and nowhere else.
- The link, through a desktop: NOT RUN. The program was given the link as
  its argument; no desktop handed it over from xdg-open.
- D-Bus activation by systemd's user bus (dbus-broker): NOT RUN. The
  private bus is dbus-daemon, which started the program from the shipped
  service file with the program's path changed.
- End-to-end case 6 of the design (the image starts tur-settings at an
  address; System Settings shows "TUR"): NOT RUN. Nothing installs the
  program in the image, and the desktop stage of the boot test is D-05.
- With a pointer drawn at the rectangle (case 8): NOT RUN. D-15.
- AT-SPI as a screen reader receives it; with people who rely on
  assistive technology: NOT RUN.
- Boot tests, hardware tests, performance tests: NOT RUN.

DIAGNOSTICS
- Log paths: GitHub Actions run 43236651205 (desktop). Artifacts:
  desktop-build-test (build, test and qmllint logs) and
  desktop-screenshots (actual/, reference/, contact sheets, report.md).
- Diagnostic bundle path: not applicable
- Relevant error IDs: TUR-DESKTOP-SETTINGS-001 to -007 and -010 are
  introduced by this program (docs/architecture/tur-settings.md, section
  3). They follow the code grammar and are not registry entries; no
  registry exists.

RECOVERY
- Rollback procedure: revert the pull request. Nothing is installed in
  the image; no other component reads these files. The change to
  desktop/ci/in-builder.sh and the two packages in the builder image
  affect every desktop job: reverting them returns the builder to its
  previous cache key.
- Recovery tested: not applicable; nothing is deployed.

KNOWN LIMITATIONS
- See docs/architecture/tur-settings.md, sections 6 (departures) and 7
  (gaps). The ones that matter most:
- It changes nothing, and four of its rows say they cannot do their job
  yet.
- Any program in the session can ask for a gold highlight (the option
  "pax"): the sender is not checked.
- No tool manifest for P@x is shipped; why, and what it would be, is in
  tur-settings.md, section 8.
- The interfaces are proposed, not frozen.
- Nothing was run in a session, under Wayland, or with KDE System
  Settings.

NEXT SAFE ACTION
- D-08 (Diagnostics), D-09 (Updates and Recovery), D-13 (the launcher's
  search) and D-15 (the overlay, which consumes the rectangle) can start
  on this.
- For the owner: the decisions listed under "Decisions the owner may want
  to reverse".
- For whoever freezes the interfaces: move the two XML files to
  desktop/dbus/ and decide the points under "For the design".
```

## Runs

| Run (desktop) | Commit | Result | What it showed |
|---|---|---|---|
| 43229804508 | 91d626d | fails | First build, with the builder image rebuilt (two packages added). Everything compiled at the first attempt except one test program, which did not link: moc had found no class in `tst_registry.cpp`. The cause was not established; the file used raw string literals, stored lambdas and `Q_DECLARE_METATYPE`, which the two test programs that did link do not, and it was rewritten without them |
| 43231524844 | 7bb630e | fails | Everything built; `qmllint` clean at the first attempt. 179 of 183 QML tests passed. The bus test could not connect: `dbus-daemon` refuses a client whose user it cannot look up, and the runner's user has no name in the container. Two test mistakes (a page out of sight in a low window was pressed; the walks' window was too low for the licence at 200 %). The 60 pictures rendered, and the first were looked at: About showed the runner's path of `os-release` |
| 43233999783 | dd9dc0f | fails | The container is given a passwd file. The bus test ran: 36 of 38. A pressed match led to the wrong setting: the test pressed before the first frame, when the matches still lay on top of one another. The picture test of the bus found the row elsewhere than reported: the test itself had moved it, by taking the keyboard from the row with nowhere to go |
| 43235348406 | db1f4f2 | fails | QML tests all pass. The bus test found a real fault: **a row asked for a second time moved**. The row gave up the keyboard for a moment to take it again "from the keyboard"; the card it stands on took it, and the page scrolled to show the whole card. The reported rectangle was right; the row had gone |
| 43236651205 | 515b665 | pass | That corrected, with a test over every row. Every job green; 163 of 163 pictures match |

Five pushes of code, and one of documents after them. The settings' QML
tests take 33 s (52 s under the sanitizers), the bus test 5 s (9 s), and the
pictures of the settings 21 s for each of the two renderings. The builder
image was rebuilt once, in the first run; the versions of Qt, the text stack
and the fonts in it are those of `desktop/ci/builder.packages`, unchanged.

## What was built, and what proves it

| Built | Proven by |
|---|---|
| **The registry schema**, `schemas/settings-registry.schema.json`: pages, sections, rows, what backs each row and whether that exists, what changing it is for P@x, and the entries for KDE's pages | `desktop/ci/registry.py` applies it to the two registries in the `boundary` job, and its self-test shows that 27 kinds of mistake are caught |
| **The registries**: `org.tur.Settings.json` (the 20 pages of the design's table, 3 built and 17 reserved; 21 rows) and `org.kde.systemsettings.json` (4 pages) | The same check; `tst_settings_registry` loads them with the program's own loader and holds the list of pages to the design's |
| **The program's loader**, with every rule of the format | `tst_settings_registry`: 52 registries that each break one rule are each refused whole |
| **Addresses never disappear** | `registry.py --against`, in the `boundary` job. Its comparison was exercised on made-up revisions only: the main branch has no registry yet |
| **The pages show exactly their registered rows** | `tst_registry_pages`: both directions, for each of the three pages; title, kind, section and availability are the registry's |
| **The shell**: the list, one page, the way into System Settings, developer mode | `tst_shell` (20) |
| **Search** over the registries, TUR's and KDE's | `tst_settings_registry` (ranking), `tst_shell` (the view), `tst_settings_bus` (`Search`) |
| **Overview, Appearance, About**, reading the files the image build writes | `tst_pages` (19), with the exact words; `tst_settings_facts` |
| **Nothing is faked**: a row with nothing behind it says so and shows no value | `tst_pages` (three rows of Overview, the choice of variant); `tst_registry_pages` (a row is available exactly when the registry says its source is) |
| **The command line and the link** | `tst_addressing` (5 forms); the `settings-starts-*` tests on the program; `tst_settings_bus` (a second start, 8 cases) |
| **`org.tur.Settings1` and `org.tur.Addressable1`** are what the two XML files state | `tst_settings_bus`: the introspection of the running program against the files |
| **A row is revealed where the program says it is** | `tst_addressing`: for each of the 21 rows, in a window 480 px high, the reported rectangle is the row's once the window has drawn, the row is in sight, has the keyboard and is marked; asked for again, it does not move. `tst_settings_bus`: over the bus, for four rows, the rectangle is compared with **the pixels that change when the row is made invisible**: all lie inside it and begin at its left and top edges |
| **The highlight**: 3 px, 4 px clear, blue or gold | `tst_addressing` reads the line's pixels on all four sides, and the gap, in both variants and both colours; `tst_settings_bus` reads them over the bus; the contrast walk judges the line |
| **Dimming, duration, clearing, a closed group** | `tst_addressing` |
| **Every refusal has its code** | `tst_addressing` (14 things that are not addresses; 8 that are not there); `tst_settings_bus` (the D-Bus error names) |
| **The program itself on the bus**: takes the name; reports the same rectangles as the window that was looked at; one window | `tst_settings_bus`, step 2 |
| **A client that is not Qt** gets the same answer | `tst_settings_bus`: `busctl` |
| **An address of another program is passed on** | `tst_settings_bus`: a stand-in that owns `org.tur.Example` |
| **Started on demand** | `tst_settings_bus`, step 3: the bus starts the program from the shipped service file; the first call it receives is a `Reveal` |
| **`RowChanged`** | `tst_settings_bus` |
| **A page of KDE System Settings is opened at its module; a row of one is not located, and the answer says so** | `tst_pages`, `tst_shell`, `tst_settings_bus`, against a stand-in that records what it was asked |
| **Contrast, keyboard, text size, both directions** | `tst_walks` (72) |

## What the pictures show

Twenty-one of the 60 were opened and looked at before they were committed:
each of the ten states at least once, in both variants, left-to-right and
right-to-left, and four at 1.5x. The other thirty-nine were not opened; they
are the same states in the other combinations. They are
`desktop/tests/screenshots/settings-<state>-<variant>-<direction>-<scale>.png`.
Each is the whole shell, 1080 px wide, at the height at which nothing scrolls
(at least 760 px).

- **overview.** The search field with its focus ring and "Search settings";
  the list with Overview selected, Appearance and About available, the other
  pages in the colour of unavailable things, and "P@x" as a closed heading
  with an arrow; under a hairline, "Displays, sound, network and more / Opens
  System Settings". "Overview"; "This system" with Version `0.1.0-dev` and
  Build `20261112.g387ac4f62f6c` in the fixed-width face; "State of the
  system" with three rows that each read "Not available yet" and a sentence
  why.
- **overview-not-tur.** Above the groups, the kit's message with the warning
  sign: "TUR could not read what this system is.", the reason, the closed
  "Technical detail" and `TUR-DESKTOP-SETTINGS-010`. Version and Build read
  "Not recorded".
- **appearance.** "The desktop is set to / Dark"; the choice, with its
  sentence and three rings that are unavailable and empty; "Everything else"
  with four rows, each with one sentence and "Open in System Settings".
- **appearance-row-revealed.** The same, with a blue line round the row of
  the choice and `tur.appearance#variant` at the foot. In the light variant
  the first row also says "This window was started in the light variant and
  does not follow the desktop."
- **appearance-system-settings-missing.** Above the page, the kit's message:
  "The program that holds this setting could not be opened.", "KDE System
  Settings could not be started.", a filled "Close", `TUR-DESKTOP-SETTINGS-005`.
- **about.** Version, Build and Base; "TUR and the Fedora Project" with the
  statement; "TUR's licence / TUR Noncommercial License 1.0.0", the required
  notice, "Read the licence"; "Software from other projects", two sentences,
  "Read the third-party notices"; "Technical detail", closed.
- **about-row-revealed-by-pax.** "Technical detail" open; a gold line round
  "Source revision"; the rest of the window dimmed; the address at the foot.
- **about-licence.** The licence open on the sunken surface, word for word,
  with its Markdown marks: 3051 px high.
- **search-results** ("dark"). "Settings that match “dark”" and a card of two
  rows, each with where it is: "Appearance › Light or dark". No page is
  selected in the list.
- **search-nothing** ("printer"). "None of TUR's own settings matches.
  Displays, sound, network, printers and most other settings are in System
  Settings." and "Open System Settings".
- **Right-to-left with lengthened strings.** Every state mirrors: the list at
  the right, values at the left, the address at the left of the foot. Titles
  from the registry are lengthened like the rest. Nothing is cut.

Things that look wrong, or may be taken for wrong:

- **In the dark variant the dimming can hardly be seen.** A fifth of the
  darkest tone over a dark window changes little. In the light variant it is
  plain. The design's "dimmed by 20 %" may need a different rule for dark.
- **The licence is shown with its Markdown marks** (`#`, `**`, `[…](#…)`).
  It is the file as shipped; rendering it would be rewriting a licence.
- **Seventeen of twenty entries of the list cannot be opened.**
- **Overview is mostly sentences that say what it cannot tell.**
- **"Open in System Settings" four times** on Appearance.
- **In a right-to-left layout typed Latin text starts at the left of the
  search field**, as in the application centre.
- **The identifiers of the fixture `rebranded` are made up** (a revision of
  `0123…`, a digest of zeros), and are in the pictures of
  about-row-revealed-by-pax.
- Nothing was found clipped, overlapping, missing a glyph or unreadable
  against its background, in either variant, in the pictures that were
  opened.

## Decisions the owner may want to reverse

1. **Gold only for P@x.** The design says the highlight is gold. Built: blue
   unless the caller passes `pax`, because a row opened by a person is not
   P@x's doing and gold is to mean P@x and nothing else. This put one file of
   `tur-settings` on the list of those that may import the gold tokens.
2. **The sender of `pax` is not checked.**
3. **The statement of origin** is the second of the design's two forms (no
   Fedora mark). The owner and the lawyer were to choose.
4. **Pages that are not built are listed**, unavailable, as the application
   centre lists its views. The alternative is to list only what exists.
5. **P@x's pages are under a closed heading.**
6. **`SettingsPage` and `SettingRow` are in the program, not in the kit**,
   because the task said to add to the kit only where needed.
7. **The interface XML is in the program's directory**, not `desktop/dbus/`,
   and the System Settings entry is not in `desktop/data/`: both were outside
   this task's paths.
8. **Additions to the interfaces**: `ClearHighlight` and the property
   `Address` on `org.tur.Settings1`; the option `pax`; the result keys
   `located`, `scale`, `highlighted`, `focused`, `page`, `row`, `program`;
   `Open("")`.
9. **Eight codes**, where the design names three kinds of error.
10. **The registry has more than the design's illustration**: `status`
    (`planned`), `listed`, `sections`, and `source` with `available` and
    `waits_for`.
11. **A row of a reserved page is refused as "page not available"**, not as
    "unknown address".
12. **`org.tur.Addressable1` refuses an address that is not its program's**;
    only `org.tur.Settings1` passes one on or opens System Settings.
13. **"The last start" still says it cannot tell**, though `tur-boot-health`
    is now in the image: this page does not run it.
14. **The builder image has two more packages** (`dbus-daemon`,
    `desktop-file-utils`) and **the container is given a passwd and a group
    file** (`desktop/ci/in-builder.sh`). Both affect every desktop job.
15. **No tool manifest is shipped.**
16. **About does not say "KDE Plasma"** or its version (design, 12.2): nothing
    read here knows it.

## Additions to ALLOWED_IMPORTS, and to the kit

`desktop/ALLOWED_IMPORTS`, five lines:

| Line | Why |
|---|---|
| `allow qml Tur.Settings desktop/apps/tur-settings/**` | The program's own QML module |
| `allow cmake tursettings desktop/apps/tur-settings/**` | Its library |
| `allow soname libtursettings.so* *` | The same, as the built programs need it |
| `allow qml Tur.Kit.Pax desktop/apps/tur-settings/qml/RowHighlight.qml,desktop/apps/tur-settings/tests/**` | The highlight is gold when P@x asked. One file of the program, and its tests, read the gold token |
| `allow cmake turkitpax desktop/apps/tur-settings/**` | The program links the module that holds it |

One dependency is used for the first time, and was already on the list: **Qt
D-Bus** (`Qt6::DBus`, `libQt6DBus.so.6`, LGPL-3.0-only).

**Nothing in the kit was changed.** Found and not changed:

- The kit has no radio button; the program draws one (`ChoiceOption.qml`).
- A `Card` is a focus scope (it is a `Pane`). An item on a card that gives up
  the keyboard hands it to the card. That is what moved the row in run
  43235348406, and `kit/tests/PageHost.qml` uses the same "take it away, give
  it back" step to show a focus ring.
- A `Button` does not wrap, and nothing notices one wider than its card.

## Dependencies

| Dependency | Fedora 45 package | Licence | Used for | Boundary |
|---|---|---|---|---|
| Qt Core, Gui, Qml, Quick, Quick Controls 2 (Templates, Layouts; one stock scroll bar) | `qt6-qtbase`, `qt6-qtdeclarative` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | Everything | Linked dynamically by side A; allowed |
| Qt D-Bus | `qt6-qtbase` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | `org.tur.Settings1`, `org.tur.Addressable1` | The same. It loads libdbus (`AFL-2.1 OR GPL-2.0-or-later`), which TUR's code does not link |
| `Tur.Kit`, `Tur.Kit.Pax` | this repository | Project licence | Tokens, controls, the message; the gold token | Side A |
| Qt Test, Qt Quick Test, `Tur.Kit.Testing` | as before | LGPL-3.0-only; project licence | Test programs only | Not shipped |
| KDE System Settings | `systemsettings` (not in the builder) | GPL-2.0-or-later | Started as a separate program, with a module's name | The command line; nothing linked |
| `dbus-daemon` 1.16.2, `busctl` (systemd 262.1), `desktop-file-validate` 0.28 | `dbus-daemon`, `systemd`, `desktop-file-utils` | GPL-2.0-or-later and others | Run by tests, in the builder | Tools; not linked, not shipped |
| `python3-jsonschema` | Ubuntu's, on the runner | MIT | The registry check, in the `boundary` job | A tool |

## Shared files

Edited: `desktop/CMakeLists.txt` (one line; outside the task's list of paths,
and there is no other way to build the program), `desktop/ALLOWED_IMPORTS`,
`desktop/ci/screenshots.sh`, `desktop/ci/in-builder.sh`,
`desktop/ci/builder/Containerfile`, `.github/workflows/desktop.yml`.

Not edited, and now out of date: `desktop/README.md` (layout, the jobs, the
fixtures), `desktop/apps/README.md` (the table says `settings/`),
`desktop/dbus/README.md` and `desktop/kit/settings/README.md` (both say
"empty until D-07").

## For the design

Text proposed for `docs/architecture/desktop-experience.md`, which this task
did not edit.

**10.3, "The registry".** After the illustration, add:

> A page has a `status`: `available`, `planned` (the address is reserved and
> the page cannot be opened yet) or `removed`. A page may declare `sections`;
> a row names its section, which is not part of its address. A row has a
> `source`: what backs it, with `available`, false where that is not part of
> the system yet, and `waits_for`. A program shows a row whose source is not
> available as unavailable, with no value.

**10.3, "The interface".** Replace the two listings with those of
[tur-settings.md](../architecture/tur-settings.md), section 4, and add:

> Each program offers `org.tur.Addressable1` at `/org/tur/Addressable1` under
> the name its registry gives as `program`; it answers for its own pages
> only. The result's `rect` is `(dddd)`: x, y, width and height in the
> window's logical pixels, relative to the top left corner of the window's
> content. `located` is false, and there is no `rect`, where the row cannot
> be found; a caller must not assume one.
>
> Options: `highlight`, `duration`, `dim`, and `pax` (the caller is the
> assistant: the highlight is gold. Otherwise it is the system's blue).

and, under the errors: "`TUR-DESKTOP-SETTINGS-001` unknown address, `-002`
removed (with its successor), `-003` page not available, `-004` not an
address, `-005` the owner could not be reached, `-006` the page does not show
the row."

**9.5, "How it looks".** "3 px gold line" is P@x's highlight. Add: "A row that
a person opened by its address, from the command line, a link or a search, is
marked the same way in the system's accent." And state what dimming is in the
dark variant, where 20 % of dark over dark is hardly seen.

**10.3, "In the code".** `SettingsPage` and `SettingRow` are in `tur-settings`
until a second program shows a settings page. "Does not compile" should read
"does not load": QML is not compiled ahead of time.

**15.1.** `apps/tur-settings/`; say where a program's interface XML lives
before it is frozen.

**15.3.** The builder image needs `dbus-daemon` for a session bus in a
container (Fedora's default, `dbus-broker`, cannot be started without
systemd), and the container needs the runner's user in `/etc/passwd`.

Text proposed for `docs/architecture/ai-control-layer.md`, section 16.6, after
"every setting is addressable":

> The settings program resolves an address and answers with the row's
> rectangle in its window (`org.tur.Settings1.Reveal`; desktop experience,
> 10.3). Pointing at a setting is three steps, none of which changes
> anything: `settings_search` (class 0), `settings_reveal` (class 1: it opens
> a window), and the overlay's pointer at the rectangle. `settings_reveal`
> needs the session bus, which the read-only sandbox does not have: it waits
> for an adapter that calls D-Bus.

## Follow-ups worth an issue

1. **Freeze `org.tur.Settings1` and `org.tur.Addressable1`**, move the XML to
   `desktop/dbus/`, and decide `pax` and its sender (with D-15).
2. **P@x's tools for settings**: where a desktop program's manifest lives
   given the SPDX rule; a D-Bus adapter in `tur-pax`; a test of the manifest
   with P@x's loader (tur-settings.md, section 8).
3. **Install rules** for desktop programs, and the image build stage: without
   them none of this reaches the image, and case 6 cannot run.
4. **Read `tur-boot-health status --json`** for "The last start" (with D-08 or
   D-09).
5. **Move `SettingsPage`, `SettingsGroup`, `SettingRow` and a radio button
   into the kit** with the second settings page.
6. **The kit: a card takes the keyboard** when an item on it lets go; a
   button does not wrap. Add to issue 38.
7. **Dimming in the dark variant.**
8. **Update the four READMEs under `desktop/`.**
9. **A Wayland run**: activation of the window on `Reveal`, and the
   application identifier, under `kwin_wayland --virtual` (the `compositor`
   job of the design, which does not exist yet).
10. **moc found no class in a test file** that used raw string literals,
    stored lambdas and `Q_DECLARE_METATYPE`; the cause is not known.

## Not done

- The System Settings entry is a file; it is installed nowhere and was seen
  in no System Settings.
- No install rule; nothing in the image; case 6 not run.
- No tool manifest.
- No change of any setting.
- KDE's settings beyond four pages; the locator.
- Translations; announcements; high contrast.
- `desktop/README.md` and the other READMEs.

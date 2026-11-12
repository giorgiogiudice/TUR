# desktop-1 — Toolchain proof (D-01) and tur-kit, the shared UI kit (D-02)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-1 (tasks D-01 and D-02 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop (desktop/), desktop CI
Build ID:         not applicable: this task builds no image
Source revision:  a4d95854e535 (feat/desktop-kit, pull request 35), which is the
                  commit the results below come from. The commit that adds
                  this report follows it and changes no code: it adds the
                  two documents and a few sentences to desktop/README.md.

SUMMARY
- What changed: the desktop has a toolchain and a foundation. A builder image
  and a workflow that compile, test and photograph Qt programs in CI; an
  automated licence boundary check; the desktop/ skeleton; a one-page proof
  program. On top of it tur-kit: the design's tokens from one source, TUR's
  own controls, the two-layer message, gold kept apart for P@x, a gallery of
  every component in every state, 37 reference pictures of it, and a test
  harness that walks every page for contrast, keyboard reach, accessible
  names and clipped text.
- Why it changed: nothing the user sees had been built, and nothing under
  desktop/ could be built or seen at all, because no machine an agent works
  on may compile or show a window. Every later desktop task builds on this.

FILES
- Added: .github/workflows/desktop.yml;
  desktop/{CMakeLists.txt,CMakePresets.json,ALLOWED_IMPORTS,README.md};
  desktop/ci/ (builder/Containerfile, builder.lock, builder.packages,
  boundary.py, compare_screenshots.py, update-references.py, and the scripts
  build-builder, load-builder, in-builder, versions, build, test, lint,
  probe, screenshots); desktop/ci/probe/ (tur-probe, two pages, one test);
  desktop/kit/ (tokens/, controls/, message/, pax/, a11y/, gallery/, tests/,
  and a README in settings/ and fixtures/);
  desktop/tests/screenshots/ (37 PNG) and desktop/tests/README.md;
  a README in desktop/{apps,data,dbus,packaging,upstream-licensed};
  docs/architecture/desktop-kit.md; docs/reports/desktop-1.md
- Modified: none
- Removed: none

BUILD
- Command: desktop/ci/build.sh ci (cmake --preset ci, cmake --build --preset
  ci), in the builder image, in workflow "desktop". C++20, -Wall -Wextra
  -Werror. Again with preset ci-sanitize (AddressSanitizer and
  UndefinedBehaviorSanitizer).
- Result: success, run 43191713274. 106 build steps; 13 programs and libraries.
- Warnings: none from the compiler; none from qmllint, which is first shown a
  planted mistake and must report it.

TESTS
- Static (job "boundary"): the licence boundary over the sources of desktop/
  (SPDX identifier on every file, every QML import, CMake package and target
  and C++ include against desktop/ALLOWED_IMPORTS, no QApplication, no colour
  literal outside the token files, gold only where allowed); a self-test
  that plants 23 files and requires each violation to be reported; the token
  check, 79 colour pairs against their WCAG thresholds and against the
  figures in the design's table. Pass.
- Linked libraries: the 13 built programs and libraries need 167 shared
  libraries in all; every one is on the allow list. Pass.
- Unit tests: 2 Qt Test programs, 17 functions (reading the desktop's
  animation setting; the pseudo-locale). Pass; 1 skipped (see limitations).
- QML tests: 8 files run by one program, headless (offscreen platform, Qt
  Quick's software renderer): tokens, motion, controls, the message, the
  contrast walk (12 page and variant pairs), the accessibility walk (12 page
  and direction pairs), text size (18 cases), the environment. Pass.
  With the unit tests: 123 test functions and data rows passed, 1 skipped.
- The same tests under AddressSanitizer and UndefinedBehaviorSanitizer (leak
  detection off). Pass.
- Program starts: tur-kit-gallery and tur-probe each start, draw and report
  no QML warning. Pass.
- Screenshots (job "screenshots"): 37 pictures, each rendered twice; the two
  renderings are identical; each matches its reference with no pixel beyond
  8/255 in a channel. Before comparing, the job makes eight pictures of its
  own and requires the comparison to fail the ones that differ (a missing
  hairline, one wrong pixel, another size) and pass the ones that do not.
  Pass, run 43191713274.
- Compositor tests: NOT RUN. No compositor job yet (D-15).
- End-to-end on the image, AT-SPI as a screen reader receives it: NOT RUN
  (D-05).
- With people who rely on assistive technology: NOT RUN.
- Boot tests: NOT RUN. Nothing here is in the image.
- Hardware tests: NOT RUN. No GPU; no real display; nothing was seen on a
  screen by a person.
- Performance tests: NOT RUN.
- In a Plasma session: NOT RUN. The session's style and colour scheme were
  probed in a container with Plasma's platform theme loaded, not in a session.

DIAGNOSTICS
- Log paths: GitHub Actions run 43191713274. Artifacts: desktop-build-test (build,
  test and qmllint logs, versions.log, the builder's package list, and
  probe/: the report and a picture for each scenario of the toolchain probe);
  desktop-screenshots (actual/, reference/, diff/, proposed/, a contact sheet
  for each combination, report.md); desktop-builder (the package list, from
  the run that built the image).
- Diagnostic bundle path: not applicable
- Relevant error IDs: none. The codes in the gallery (TUR-STORAGE-NTFS-004
  and others) are samples, not registry entries.

RECOVERY
- Rollback procedure: revert the pull request. Nothing outside desktop/,
  the one workflow and two documents is touched; nothing is installed in the
  image; no other workflow reads these files.
- Recovery tested: not applicable; nothing is deployed.

KNOWN LIMITATIONS
- See docs/architecture/desktop-kit.md, sections 4 (departures from the
  design) and 6 (known gaps). The ones that matter most:
- The builder image is in the Actions cache, not in a registry pinned by
  digest. If the cache is evicted the image is rebuilt from that day's Fedora
  packages and a Qt or font update can change the pictures.
- There is no high-contrast variant, and the kit does not follow a colour
  scheme other than TUR's dark and light.
- Accessibility is tested through Qt's interfaces by a program. No screen
  reader and no person has used any of it. Announcements are not built.
- The kit's own controls stand beside Breeze's inside the session; nobody has
  yet seen the two together.
- One unit test is skipped on the offscreen platform: following the desktop's
  light or dark preference while a program runs. The rule itself is tested.
- No formatting or translation patch; no install rules.

NEXT SAFE ACTION
- D-03 (Fedora's marks) and D-04 (theme and defaults: the colour schemes
  generated from desktop/kit/tokens/tokens.json, Inter and JetBrains Mono
  added to the image). D-07 or D-10 can start on the kit now.
- A decision for the owner: where the project keeps a builder image, so that
  it can be pinned by digest as the design intends.
```

## Runs

| Run | Commit | Result | What it showed |
|---|---|---|---|
| 43187894364 | 7f22392 | fail | First build. Compiled and linked at the first attempt; qmllint clean; 118 of 123 test functions passed. Failures: a sheet was not mirrored right-to-left (found by the accessibility walk); two mistakes in tests; clipped progress captions; no sanitizer runtime in the image; pictures at 1.5x came out 800 px wide |
| 43189478078 | e2daee0 | fail | All of those corrected but one mistake in a test. Pictures at 1.5x correct. The builder image rebuilt with the sanitizer runtimes |
| 43190375559 | 42c062a | fail (no references) | `build-test` green. The 37 pictures rendered and looked at; 31 accepted as references |
| 43190892085 | 52a689e | fail (6 new pictures) | 31 pictures matched their references, the first real comparison. Placeholder text moved onto the text line; the six pictures of the fields page rendered again and accepted |
| 43191297269 | d8ffe42 | pass | All four jobs green; 37 of 37 pictures match |
| 43191713274 | a4d9585 | pass | The same, with the comparison first proving on made-up pictures that it can fail |

Times of the passing run, on GitHub-hosted `ubuntu-24.04` runners:

| Job | Time | Of which |
|---|---|---|
| `boundary` | 9 s | |
| `builder` | 7 s | The image was in the cache |
| `build-test` | 1 min 51 s | Fetch and load the image 36 s; configure and build 24 s; tests 6 s; probe 5 s; sanitizer build 25 s; sanitizer tests 6 s |
| `screenshots` | 1 min 22 s | Fetch and load the image 33 s; configure and build 28 s; render twice and compare 14 s |

The jobs run side by side after `builder`; the workflow takes about two and a
half minutes.

The design's budget is 1 minute for `boundary` and 5 minutes each for
`build-test` and `screenshots`. Building the builder image, when it is not in
the cache, took 79 to 85 seconds; it is 1.98 GB unpacked, 633 MB in the cache,
738 packages.

## What the assumptions turned out to be

The design left these to D-01 (sections 5.7, 15.4, 16). The evidence is the
`probe/` directory of the `desktop-build-test` artifact and the package list of
the OS image; the detail is in
[desktop-kit.md](../architecture/desktop-kit.md), section 5.

| Assumption | Answer | Evidence |
|---|---|---|
| `qqc2-breeze-style` is in, or can be added to, the image | **Holds.** It is already in the Kinoite base | `qqc2-breeze-style` 6.8.2-1.fc45 in the SBOM of the OS image (run 43146811082) and in Fedora 45 |
| A `QGuiApplication` gets `org.kde.breeze` under Plasma | **Holds**, in a container with Plasma's platform theme loaded; not tried in a session | `styleAtStart: org.kde.breeze` with `QT_QPA_PLATFORMTHEME=kde`; `Fusion` without |
| Kirigami takes the system's colour scheme with it | **Holds** | With a `kdeglobals` in TUR's dark colours `Kirigami.Theme` reports `#0b0f17`, `#eef1f6`, `#1267c4`; without it, Breeze Light's. Under the Basic style it does not (`#efefef`), so the fallback would need KColorScheme |
| `org.kde.breeze` "looks unlike the desktop style" (risk 8) | **Not settled.** It draws under the software renderer in the system's colours; what it looks like beside KDE's applications needs a session | `probe/session-theme.png` |
| `grabImage` works under the offscreen platform | **Holds at a scale of 1.** At 1.5x it cuts the wrong rectangle; the harness crops the window's picture itself. The virtual-KWin fallback is not needed | The proof's test reads the accent colour from its swatch; 37 pictures, twice, identical |
| Screenshot tests are noisy (risk 12) | **Not seen.** Two renderings in a run are identical, and the 25 pictures whose code did not change between two runs were identical byte for byte. Not yet tried across a Qt or font update | Runs 43189478078 and 43190375559 |
| Whether the Kinoite image has the CJK fonts (5.3) | **It has**: Noto Sans CJK, Hebrew and Arabic. It does not have Inter or JetBrains Mono | SBOM of the OS image |

## What the pictures show

All 37 references were opened and looked at, in both variants, before they
were committed. They are in `desktop/tests/screenshots/`.

- **Foundations.** The seventeen system swatches with their token names and
  values, P@x's three beside P@x's name and a gold pill, and the type scale.
  The values printed under the swatches are the design's. The flame mark is
  the teardrop of the pointer sketch, small; it reads as a leaf as much as a
  flame. It is a placeholder.
- **Buttons.** Four kinds in four states. Primary is filled blue with dark
  text in the dark variant and white text in the light, as the design's
  contrast note requires. Destructive is outlined in red with red text.
  Pressed is a visible wash. The focused button has a 2 px ring clear of its
  edge.
- **Fields and choices.** Text fields with a label above; a helper line that
  wraps; a wrong entry with a red 2 px border, a sign and the reason in words;
  unavailable fields in the disabled colour. Check boxes and switches, on, off
  and unavailable. A switch that is on is filled, with its handle at the end.
- **Cards, lists, banners and progress.** A card; five list rows, one with a
  focus ring inside it and one selected (white on blue); four banners, each
  with its own sign and border colour; a progress bar at 39 % with real
  figures; an indeterminate bar, which with motion frozen is three still
  blocks; a spinner, still.
- **The two-layer message.** An error closed, with "Try again", "Create a
  support bundle" and "Ask P@x to look into it", the disclosure with a focus
  ring, and the code at the end of the row; the same error open, with five
  lines of technical detail in the fixed-width face on the sunken surface and
  a Copy button; a warning; a notable event.
- **Dialog sheet.** A sheet over a dimmed page, with a title, a paragraph, a
  device in the fixed-width face, a check box and two buttons; the safe
  button has the focus ring.
- **Right-to-left with lengthened strings.** Every page mirrors: titles and
  text at the right, buttons running right to left, check boxes and switches
  at the right of their labels, the progress bar filling from the right, the
  code at the left end of its row. Text is in Hebrew letters and about 40 %
  longer; it wraps and nothing is cut. Codes, paths and typed text stay in
  Latin letters, left to right.
- **1.5x.** The same pages, sharp, 1200 px wide. Hairlines are one or two
  device pixels and even.
- **The toolchain proof.** A title, a paragraph, an accent swatch, an outlined
  swatch, an arc and a check sign, a line in the fixed-width face, two Hebrew
  words drawn by a fallback font, and a stock Qt Quick button.

Things that look wrong or may be taken for wrong:

- **Fixed-width text looks heavier in the dark variant than in the light.**
  The same font and weight are asked for. The cause was not established.
- **The stock button on the proof page is light grey on a dark page.** That
  is Qt's Basic style with no palette, on purpose: the page proves a stock
  control draws and takes a click. Inside a `Kit.Page` a stock control takes
  TUR's colours.
- **In a right-to-left layout the technical detail starts beside the Copy
  button, not at the right edge.** It is left-to-right text and is aligned
  as such; whether it should sit at the other end is a design question.
- **In the light variant the white hairline of P@x's pill cannot be seen**,
  as intended: it is there for dark content. The dark outline carries it.
- Nothing was found clipped, missing a glyph, or unreadable against its
  background, in either variant.

## Dependencies

Everything the programs built here link or import, and which side of the
licence boundary it is on (design, section 4). "Side A" is TUR's own programs
under the project licence; they may link LGPL libraries dynamically.

| Dependency | Fedora 45 package | Licence | Used by | Boundary |
|---|---|---|---|---|
| Qt Core, Gui | `qt6-qtbase` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | Everything | Linked dynamically by side A; allowed |
| Qt Qml, Quick, Quick Controls 2 (with Templates, Layouts, Shapes) | `qt6-qtdeclarative` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | Everything | Linked or imported by side A; allowed |
| Qt Test, Qt Quick Test | `qt6-qtbase`, `qt6-qtdeclarative` | LGPL-3.0-only (or GPL, or commercial) | Test programs only | Not shipped |
| Kirigami | `kf6-kirigami` 6.31.0 | LGPL-2.0-or-later and others; the RPM's tag also lists GPL-2.0-or-later | `tur-probe`'s second page only, to read its colours | Imported by a CI tool; allowed by the design; not used by the kit |
| `qqc2-breeze-style`, `plasma-integration` | 6.8.2 | The RPM tags list LGPL and GPL identifiers | Loaded by Qt at run time in the probe's scenarios | Not linked or imported by TUR code |
| Inter | `rsms-inter-fonts` 4.1 | OFL-1.1 | Rendering | Data |
| JetBrains Mono | `jetbrains-mono-fonts` 2.304 | OFL-1.1 | Rendering | Data |
| Noto Sans, Noto Sans Hebrew | `google-noto-sans-vf-fonts`, `google-noto-sans-hebrew-vf-fonts` | OFL-1.1 | Fallback fonts in the pictures | Data |
| libstdc++, libgcc | `libstdc++`, `libgcc` 16.2.1 | GPL-3.0-or-later with the GCC runtime exception | Everything | The exception permits it |
| glibc | `glibc` 2.44 | LGPL-2.1-or-later and others | Everything | Allowed |
| libasan, libubsan | `libasan`, `libubsan` | GPL-3.0-or-later with the GCC runtime exception | The sanitizer build only | Not shipped |
| CMake, Ninja, GCC, qmllint, moc, rcc | `cmake` 4.3.0, `ninja-build` 1.13.2, `gcc-c++` 16.2.1, `qt6-*-devel` 6.11.2 | BSD-3-Clause and others, Apache-2.0, GPL-3.0-or-later, GPL-3.0-only with the Qt exception | Build and test tools | Run, not linked |
| Python 3, Pillow | `python3` 3.15.2, `python3-pillow` 12.3.0 | Python-2.0.1, MIT | CI scripts; picture comparison | Run, not linked |

The builder image also contains GPL-only packages that nothing here links:
`kwin`, `kf6-kio`, `kf6-kiconthemes`, `qt6-qtquicktimeline`, `qt6-qtquick3d`
and others arrive as dependencies of `kwin` and `plasma-integration`. The
check of what the built programs link is what keeps them out.

No new dependency was added to any Rust component, to the image, or to any
other workflow.

## Shared files that need a change

None was edited. Three are worth a change by whoever owns them:

1. **AGENTS.md**, layout table: `desktop/` holds "QML, C++, data" already;
   the build-environment section could name the desktop loop beside the Rust
   one (`desktop/ci/update-references.py RUN_ID`).
2. **`scripts/dev/ci.py`**: the design says it "gains the commands to fetch
   [the artifacts] and to apply proposed references". `ci.py artifact` already
   fetches; applying is `desktop/ci/update-references.py`, which calls it. A
   `ci.py references RUN_ID` that forwards to it would match the design's
   wording. Also useful and missing: job and step durations in `ci.py wait`.
3. **The design**, `desktop-experience.md`: the places where what was built
   differs are listed in [desktop-kit.md](../architecture/desktop-kit.md),
   section 4. Two are corrections of fact rather than choices: `grabImage` is
   wrong at a fractional scale under the software renderer (15.4), and
   Breeze's icons cannot be recoloured without KIconThemes, which section 4
   forbids (5.4).

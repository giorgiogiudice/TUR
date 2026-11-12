# desktop-2 — tur-centre part 1, "find" (D-10)

```text
TUR DEVELOPMENT REPORT

Task ID:          desktop-2 (task D-10 of the desktop design,
                  docs/architecture/desktop-experience.md, section 15.5)
Subsystem:        Desktop (desktop/apps/tur-centre), desktop CI; one test in
                  services/tur-catalogue
Build ID:         not applicable: this task builds no image
Source revision:  01f8ad7 (feat/tur-centre-find, pull request 39), which is
                  the commit the results below come from. The commit that
                  adds this report follows it and changes no code: it adds
                  this report, completes docs/architecture/tur-centre.md and
                  adds a section to desktop/README.md.

SUMMARY
- What changed: TUR has the first part of its application centre. tur-centre
  is a standalone Qt Quick program on Tur.Kit with the shell of the design's
  section 8, a search over the catalogue, an application's page ("How it
  will run", the other ways, each with its rating and who reported it, what
  is needed first, the caveats) and the "no route" page. It reads the
  catalogue and the router by running tur-catalogue --json as a separate
  program, or from recorded answers of it, behind one interface. A test in
  the Rust workflow regenerates those recorded answers from the router and
  fails on any difference.
- Why it changed: the application centre is priority 4 of AGENTS.md and the
  place where "what will run, how, and according to whom" reaches a person.
  The catalogue and the router were merged with nothing to show them.
- What it does not do: it installs nothing. The primary button says what
  would happen next and is unavailable. Five of the seven views of the side
  list are listed and unavailable.

FILES
- Added: desktop/apps/tur-centre/ (CMakeLists.txt; src/: cataloguesource,
  processsource, fixturesource, answers, hitsmodel, catalogue, main; qml/:
  18 files; fixtures/: 18 JSON files in default/, assumed-host/ and
  vocabulary.json, and update.py; tests/: 6 QML test files, CentreHost.qml,
  tst_main.cpp, shots/render_centre.qml, unit/: 3 Qt Test programs and a
  stand-in program);
  desktop/tests/screenshots/centre-*.png (66);
  services/tur-catalogue/tests/desktop_fixtures.rs;
  docs/architecture/tur-centre.md; docs/reports/desktop-2.md
- Modified: desktop/CMakeLists.txt (one add_subdirectory);
  desktop/ALLOWED_IMPORTS (three lines, below); desktop/ci/screenshots.sh
  (renders the centre's states too); desktop/README.md and
  desktop/apps/README.md; .github/workflows/rust.yml (one path, so that the
  fixtures' test runs when the fixtures change);
  desktop/kit/message/Message.qml and desktop/kit/controls/TextField.qml
  (two kit changes, below) with a test for each in desktop/kit/tests/
- Removed: none

BUILD
- Command: desktop/ci/build.sh ci, in the builder image, in workflow
  "desktop". C++20, -Wall -Wextra -Werror. Again with preset ci-sanitize.
- Result: success, run 43203830424. 160 build steps; 21 programs and
  libraries.
- Warnings: none from the compiler; none from qmllint. CMake prints 15
  warnings about Kirigami plugins of tur-probe, which were there before.

TESTS
- Rust workflow, run 43203830197, services/tur-catalogue: the new file
  tests/desktop_fixtures.rs, 4 tests. The fixtures are what tur-catalogue
  prints for the seed catalogue (18 files); a fixture that differs, is
  missing or is stale is reported (on made-up files); a fixture keeps the
  router's text and carries the licence identifier; the Home view's standing
  sentence is still true of the catalogue. Pass. That the first of these can
  fail was seen: in run 43195432519 it failed on 18 missing files, and that
  is how the files were made.
- Static (job "boundary"): the licence boundary over 264 files of desktop/;
  its self-test; the token check. Pass.
- Linked libraries: 21 built programs and libraries need 269 shared
  libraries; every one is on the allow list. Pass.
- Unit tests (Qt Test), 3 new programs, 83 functions and data rows:
  reading the three documents and every way reading can fail (43); the two
  sources: a missing program, a time limit, too much output, a crash,
  standard error, no input, the limit on runs at once, cancelling, no
  shell, and a fixture that answers what was recorded and nothing else (21);
  the search and the page over recorded answers, over a failing program and
  over answers that arrive out of order (19). Pass.
- QML tests, 6 new files run by one program, headless, 183 functions and
  data rows (12 of them the set-up and tear-down of the six test cases):
  the shell (25), search (12), an application's page (9), the "no route"
  page (10), the vocabulary (38), and the kit's walks over the eleven
  states the pictures are taken of: contrast in both variants (22),
  keyboard and accessible names in both directions (22), text at 200 %,
  right-to-left with strings 40 % longer, and both (33). Pass.
- Program starts: tur-centre starts on a fixture; on a search, an
  application and a "no route" address; and with no catalogue program at
  all; each time with no QML warning. 5 tests. Pass.
- The whole of desktop/: 391 test functions and data rows passed, 1
  skipped (the kit's, as before). The same under AddressSanitizer and
  UndefinedBehaviorSanitizer. Pass.
- Screenshots (job "screenshots"): 103 pictures, of which 66 are new, each
  rendered twice; the two renderings are identical; each matches its
  reference with no pixel beyond 8/255 in a channel. The 37 pictures of the
  kit did not change. Pass, run 43203830424.
- tur-centre against the real tur-catalogue program: NOT RUN. The builder
  image has no Rust toolchain and no tur-catalogue. The program was run
  against a stand-in (tests/unit/helper.cpp) for every failure, and against
  recorded answers for every answer. The chain that stands in for the real
  run: the Rust test gives tur-catalogue exactly the arguments it records
  in the fixtures, and a unit test holds ProcessSource to those arguments.
- Against a real machine's host facts: NOT RUN. Nothing detects them. One
  fixture set states a machine; the other lets the router assume one.
- End-to-end case 5 of the design (the image starts tur-centre --fixture,
  a search returns rows, a page shows "How it will run"): NOT RUN. The
  desktop stage of the boot test is task D-05.
- In a Plasma session, under org.kde.breeze, beside KDE's applications:
  NOT RUN. The tests and the pictures use the Basic style.
- AT-SPI as a screen reader receives it; with people who rely on assistive
  technology: NOT RUN.
- Boot tests, hardware tests: NOT RUN. Nothing here is in the image.
- Performance tests: NOT RUN. Not measured: how long a search of twenty
  hits takes with forty runs of the catalogue program behind it.

DIAGNOSTICS
- Log paths: GitHub Actions runs 43203830424 (desktop) and 43203830197
  (rust). Artifacts of the desktop run: desktop-build-test (build, test and
  qmllint logs) and desktop-screenshots (actual/, reference/, contact
  sheets, report.md).
- Diagnostic bundle path: not applicable
- Relevant error IDs: TUR-DESKTOP-CENTRE-001 to -007 are introduced by this
  program (docs/architecture/tur-centre.md, section 2). They follow the
  code grammar and are not registry entries; no registry exists. The
  catalogue's TUR-DESKTOP-CATALOGUE-009 and -011 are shown where the
  catalogue reports them.

RECOVERY
- Rollback procedure: revert the pull request. Nothing is installed in the
  image; no other component reads these files. The two kit changes keep the
  kit's behaviour for every existing use (its pictures did not change).
- Recovery tested: not applicable; nothing is deployed.

KNOWN LIMITATIONS
- See docs/architecture/tur-centre.md, sections 6 (departures) and 7
  (gaps). The ones that matter most:
- It installs nothing, and says so on every page that has a route.
- Nothing the router says can be translated: its reasons, caveats,
  prerequisites and alternatives are English text. The right-to-left
  pictures show its sentences in English beside translated ones.
- By default the router is told nothing about the machine, and every page
  then says that how the application will run is an assumption.
- No installed location is defined for the catalogue; --catalogue must be
  given.
- A search costs one run of the catalogue program and then two for each
  application found.
- Several sentences on screen are neither the design's nor the router's;
  they are listed in tur-centre.md, section 6, for the design's owner.

NEXT SAFE ACTION
- D-11 (installing, Sources, Installed, Updates) and D-13 (the launcher's
  search provider) can start on this.
- For the owner of tur-catalogue: the proposals of tur-centre.md, section
  7; the first is an identifier and parameters for every sentence the
  router returns.
- For the design's owner: the departures of section 6, and the words that
  need a decision.
```

## Runs

| Runs (desktop, rust) | Commit | Result | What it showed |
|---|---|---|---|
| 43195432724, 43195432519 | 829fc9d | rust fails, as intended | The Rust test alone. It compiled and passed Clippy at the first attempt and failed on 18 missing fixtures, printing each; `update.py` wrote them from the job's log. `rustfmt` wanted seven lines changed; CI's patch was applied |
| 43201038132, 43201038100 | 7b89ee5 | desktop fails | First build of the program. It compiled and linked at the first attempt; `qmllint` clean; all but eight test functions passed. Failed: the text-size test in the eight states with an empty search field (below, kit); and `centre-starts-on-a-search`, on a QML warning: a search row read the router's option before the model had handed it over. Found by reading the walk's counts, not by a failure: Tab did not reach the side list. The 66 pictures rendered, and the first were looked at |
| 43202384272, 43202384220 | 7ef8464 | `build-test` green; `screenshots` fails for want of references | Those corrected. 391 of 391, also under the sanitizers. Sixty of the pictures looked at here |
| 43203086869, 43203086903 | 52e1b58 | the same | One change of wording on a blocked way. The pictures were taken from this run: 60 identical, byte for byte, to the run before; the six that changed looked at |
| 43203830424, 43203830197 | 01f8ad7 | pass | Every job green; 103 of 103 pictures match |

Five pushes of code, and one of documents after them. Times were not
recorded; the QML tests of the application centre take 13 s (20 s under the
sanitizers) and its pictures 17 s for each of the two renderings.

## What was built

- **The shell** (design, 8.1). A header with the search field, which has the
  keyboard when the window opens; a side list of the seven views; one content
  area. An application's page takes the whole width, with the way back in
  the header. Every view has an address (`tur-centre:home`,
  `tur-centre:search?q=…`, `tur-centre:app/<id>`); the program takes one on
  its command line. What has the keyboard is scrolled into sight.
- **Search** (8.3). Typing asks the catalogue once the keys pause (250 ms);
  Enter asks at once. One row for each application, in the catalogue's
  order, with the route the router recommends for this machine, that route's
  rating, and what is needed first. Rows appear as soon as the search
  answers and are filled in as the router answers for each. An application
  that cannot run is listed and says so. States: nothing asked, under way,
  results, nothing found, failed.
- **An application's page** (8.4). "How it will run" is the option the
  router marks `recommended`, in the fixed order: route, explanation, rating
  with its source, what is needed first, caveats with blockers first.
  "Other ways to run it" lists every other option in the router's order.
  "All reports" opens when the catalogue holds more than one record for a
  way. "Details" holds where each fact was checked. Unknown is "No reports",
  with its own sign.
- **The "no route" page** (8.8). "Cannot run on TUR"; why, in the sentence the
  design fixes for the case; what the person can do instead; the ways that
  were considered, each blocked, with its rating and source; the references;
  the code; and the router's own words, which open in place.
- **The backend**, behind one interface: `ProcessSource` runs the catalogue
  program (a vector of arguments, the subject after `--`, at most four runs
  at once, ten seconds and 4 MiB each, nothing blocking); `FixtureSource`
  answers from recorded answers and never guesses. `answers.cpp` checks the
  shape and the vocabulary of every answer and turns every failure into a
  code, a title, a reason and a technical layer for the kit's two-layer
  message.
- **The fixed vocabulary** (8.2), in one file, held against the router's own
  list of values by a test.
- **The fixtures and their test**, and the script that writes them from CI.

## What the pictures show

All 66 were opened and looked at, in both variants, before they were
committed. They are `desktop/tests/screenshots/centre-<state>-<variant>-<direction>-<scale>.png`.
Each is the whole shell, 1080 px wide, at the height at which nothing
scrolls (at least 480 px), with the search field focused as it is when the
window opens.

- **home.** The title "Application centre", the search field with its focus
  ring and the placeholder "Search", the side list with Home selected (white
  on blue) and Search available; Categories, Installed, Updates, Sources and
  Open a file in the colour of unavailable things. In the content, "Home" and
  one bordered line with the information sign: "TUR has not tested
  applications itself yet. Every rating here is what the named source
  reports." **Under it the content area is empty**: part 1 has nothing else
  to put on Home.
- **search-results** ("office"). "2 applications in TUR's catalogue for
  "office"", then a card of two rows. Microsoft 365: a tile with "M", the
  name, "Microsoft · Office suite: Word, Excel, PowerPoint and others.",
  "Windows application, in Windows", the question mark in a dashed circle with
  "No reports", "Needed first" and the router's two sentences, and a
  "Details" button at the end. LibreOffice: "Linux application", "No
  reports".
- **search-nothing-found** ("photothing"). "Nothing found for
  "photothing"." and one button, "Tell the project you need it". Nothing
  about files.
- **search-failed** (no catalogue program). The kit's message: a cross in a
  circle, "The application catalogue could not be read.", "Reason: The
  program that reads it could not be started.", a filled "Try again", the
  closed "Technical detail" and `TUR-DESKTOP-CENTRE-001`. No support-bundle
  button.
- **application-microsoft-365.** "‹ Home" and the search field in the header.
  The tile, the name, "Microsoft · Productivity", and an unavailable "Set
  up…"; under them "The application centre cannot install applications yet."
  The box "How it will run": "Windows application, in Windows" with
  "Recommended" at the end of the line, the explanation, the router's reason
  in the supporting colour, "No reports" and the nobody-sentence, "Needed
  first" with two points, "Before you install" with one note about memory.
  "Other ways to run it": the compatibility layer first, with "May not work"
  in orange, the cross in a circle and "Does not work", "Reported by WineHQ
  AppDB as "Garbage" · version 365 Business · checked 12 November 2026 · Not
  tested by TUR", and the router's warning with a triangle; then "Web
  application". "About this application" and the one-line summary. "Details"
  open: "Where each fact was checked", two references, each with "Checked 12
  November 2026" and its address in blue.
- **application-krita.** "Install", unavailable. "Linux application", "No
  reports", "Before you install: Nothing known stands in the way." The other
  way is also "Linux application", with the router's note that the package
  is maintained by its packagers.
- **application-elden-ring.** "Open in Steam", unavailable. "Windows game,
  through Proton"; a tick with an exclamation mark and "Works, with minor
  problems", "Reported by ProtonDB as "gold" · checked 12 November 2026 · Not
  tested by TUR". "All reports (2)" open: that report again, and a tick in a
  circle with "Works well", reported by Valve Steam Deck compatibility as
  "Verified"; then "A source's rating is evidence from its contributors'
  machines, not a promise."
- **application-netflix-assumed-host.** Under the header line an orange-
  bordered warning: "TUR has not checked what this computer has. How this
  application will run here is an assumption, and may turn out differently."
  "Web application", recommended. The other way, "Android application", "May
  not work", two things needed first, and a triangle with "Probably will not
  work fully" over the router's sentence about copy protection.
- **application-unknown.** The kit's message alone: "No application has this
  ID", the catalogue's own message as the reason, the closed "Technical
  detail" and `TUR-DESKTOP-CATALOGUE-009`. No "Try again".
- **no-route-valorant.** No button in the header line. A card: the cross in a
  circle and "Cannot run on TUR"; the design's two sentences about anti-cheat;
  "What you can do" with two points; "Ways that were considered": "Windows
  game, through Proton" set back, "Cannot be used on this computer" in red,
  "Does not work", "Reported by AreWeAntiCheatYet as "Denied" · checked 12
  November 2026 · Not tested by TUR", the blocker with a cross and the
  router's warning with a triangle. A hairline, then "Where each fact was
  checked" (open: three references), `TUR-DESKTOP-CATALOGUE-011`, and
  "Detail" (open: the router's reason and alternative in the fixed-width
  face on the sunken surface).
- **no-route-banking.** The same pattern for the class "Banking and payment
  applications for Android": the design's two sentences about the certified
  phone, one thing to do, "Android application" blocked, "No reports" without
  "It may still work", and the two sections closed.
- **Right-to-left with lengthened strings.** Every state mirrors: the side
  list at the right, the back arrow pointing right, tiles and signs at the
  right of their text, "Recommended" and the buttons at the left end. The
  fixed vocabulary is in Hebrew letters and longer; it wraps, and the title
  takes two lines. Nothing is cut.
- **1.5x.** The same states, sharp, 1620 px wide.

Things that look wrong, or may be taken for wrong:

- **In the right-to-left pictures every sentence of the router is English**,
  set at the right edge, between translated lines. That is what the program
  would show a person who reads a right-to-left language today.
- **In the right-to-left pictures a date reads "November 2026 12"** inside a
  translated sentence. The pseudo-locale puts Hebrew letters round a date
  that is still formatted for English; a real translation would bring its
  own date. Not a defect that was fixed.
- **In a right-to-left layout the cursor of the empty search field stands at
  the left** while its placeholder is at the right, and typed Latin text
  starts at the left. Qt places the cursor by the direction of the input, not
  of the layout.
- **Home is mostly empty.**
- **Fixed-width text looks heavier in the dark variant** (the codes, and the
  router's own words on the "no route" page), as in the kit's pictures.
- **Two ways can carry the same label.** Krita's Flatpak and its Fedora
  package are both "Linux application"; only the router's reason tells them
  apart.
- **The same fact twice.** Where a way is rated badly, the rating's line and
  the router's own caveat say it in two forms, the second with the date as
  2026-11-12.
- **"Needed first" under "Cannot be used on this computer"** on the banking
  class: the router lists the prerequisite of a blocked way, and it is shown.
- **The sign of an obstacle sits about three pixels below the first line** of
  its text ("Probably will not work fully").
- Nothing was found clipped, overlapping, missing a glyph or unreadable
  against its background, in either variant.

## Where the design was departed from

In full, with reasons, in
[tur-centre.md](../architecture/tur-centre.md), section 6. In short:

1. What is needed first is the router's sentences, not the short forms of
   the drawings: 8.2 says the application centre writes none.
2. The router's reason is shown under every way.
3. A blocked way is set back in the supporting colour, not greyed to the
   colour of unavailable controls, and does not say "It may still work".
4. The primary button is unavailable, with a sentence the design does not
   have. The design states no placeholder for installing.
5. When the router assumed the machine, a warning the design does not have.
6. Kirigami is not used; the directory is `apps/tur-centre/`; a row of a
   search is on several lines; the web route has the fixed explanation.
7. Failure text is in the program, because no registry exists.
8. The fixture loader is in `tur-centre`, not in the kit as 14.1 lists it:
   with one program there is nothing yet to share.

## What the router's output lacks for these screens

In full in [tur-centre.md](../architecture/tur-centre.md), section 7. Nothing
in the router or the catalogue was changed.

1. **Text without identifiers.** Caveats and prerequisites have a `code`;
   reasons have none; no sentence carries its parameters. Nothing the router
   says can be translated (the design's requirement 1 of 8.10).
2. **Errors outside the JSON.** With `--json`, a catalogue that cannot be
   loaded is reported on standard error as text.
3. **A hit is thin.** `search --json` gives name, publisher and category; the
   summary and the recommended route cost two more runs for each hit.
4. **No code for "no route"** in the JSON; only exit status 3.
5. **No host facts**, and no place for them or for preferences.
6. **No installed location** for the catalogue.
7. **Messages for a terminal** in the catalogue's errors.
8. **A caveat that repeats the rating**, and **prerequisites on a blocked
   way**.
9. **No icons, descriptions or pictures; an identifier where a name is
   wanted** for another Linux system; **no way to list** the catalogue or a
   category; **no count of TUR's own tests**.

## Additions to ALLOWED_IMPORTS, and to the kit

`desktop/ALLOWED_IMPORTS`, three lines, all TUR's own code under the project
licence:

| Line | Why |
|---|---|
| `allow qml Tur.Centre desktop/apps/tur-centre/**` | The program's own QML module |
| `allow cmake turcentre desktop/apps/tur-centre/**` | Its library, linked by the program and its tests |
| `allow soname libturcentre.so* *` | The same, as the built programs need it |

No library was added to the boundary. The catalogue is reached by running a
program (`QProcess`, in Qt Core), which is the command-line side of the
boundary that AGENTS.md names.

The kit, two changes, each with a test, neither changing a picture:

| Change | Why |
|---|---|
| `Message.supportBundleAvailable` (default true) | `Message` always offered "Create a support bundle" for an error. Nothing makes a bundle yet (D-08), so the button would have done nothing. A program that cannot make one now says so, and the button is not shown |
| `TextField` is as tall as its placeholder needs | A placeholder in a script the interface font lacks is drawn in a fallback font with taller lines. At 200 % text the Hebrew placeholder of the search field was reported as cut off |

Found and not changed in the kit: a `ListRow` does not take the keyboard by
itself (Qt's item delegate has no focus policy), so Tab passes a list of
them by. The application centre's side list sets the policy; the kit's own
gallery list is not reachable by Tab.

## Dependencies

Nothing new. What `tur-centre` links or imports, and which side of the
licence boundary it is on:

| Dependency | Fedora 45 package | Licence | Used for | Boundary |
|---|---|---|---|---|
| Qt Core, Gui, Qml, Quick | `qt6-qtbase`, `qt6-qtdeclarative` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | Everything; `QProcess` and JSON are in Qt Core | Linked dynamically by side A; allowed |
| Qt Quick Controls 2 (Templates, Layouts, Shapes; one stock scroll bar) | `qt6-qtdeclarative` 6.11.2 | LGPL-3.0-only (or GPL, or commercial) | The views | Imported by side A; allowed |
| `Tur.Kit` | this repository | Project licence | Tokens, controls, the two-layer message | Side A |
| Qt Test, Qt Quick Test, `Tur.Kit.Testing` | `qt6-qtbase`, `qt6-qtdeclarative`; this repository | LGPL-3.0-only; project licence | Test programs only | Not shipped |
| `tur-catalogue` | this repository (`services/tur-catalogue`) | Project licence | Run as a separate program; its recorded output is the fixtures | The command line; nothing linked |
| libstdc++, libgcc, glibc | as for the kit | GPL-3.0-or-later with the GCC runtime exception; LGPL-2.1-or-later | Everything | As for the kit |

Not used, though the design's inventory lists them for `tur-centre`:
Kirigami and `qqc2-breeze-style`.

## Not done

- Installing, updating, removing; Sources; Installed; Updates; Open a file;
  Categories; stores in the search; "offline".
- Choosing another way and pinning it.
- The filter row of the search.
- In "Details": editions, licence, what the application can reach, where its
  data is.
- The buttons of the "no route" page.
- `org.tur.Addressable1`, the desktop file, the file-type handlers, install
  rules.
- Announcements (X4), a high-contrast variant (X7), the greyscale rendering
  of the rating icons (X6).
- A translation catalogue; plural forms through `%n`.
- A run against the real `tur-catalogue`, in a session, or on the image.

## Shared files

Edited, as the task allowed: `.github/workflows/rust.yml` (the path
`desktop/apps/tur-centre/fixtures/**`, in both lists), `desktop/CMakeLists.txt`,
`desktop/ALLOWED_IMPORTS`, `desktop/ci/screenshots.sh`, `desktop/README.md`,
`desktop/apps/README.md`.

Worth a change by whoever owns them, and not edited:

1. **`docs/architecture/desktop-experience.md`**: section 8's drawings and
   its rule about the router's text disagree on what is needed first; the
   design states no placeholder for a primary button that cannot act yet,
   and no words for a machine that was assumed; "greyed" for a blocked way
   conflicts with the contrast requirement; 15.1 names the directory
   `apps/centre/`; 14.1 puts the fixture loader in the kit and Kirigami in
   `tur-centre`.
2. **`docs/architecture/catalogue-and-routing.md`**, "Not built": "The
   application centre. No user interface" is no longer so; and the JSON of
   `route`, `show` and `search` is now a contract that a test holds, which
   that document could say.
3. **`services/tur-catalogue`**: the proposals above.
4. **`desktop/kit/fixtures/README.md`** says the fixture loader arrives with
   the first program that talks to a service. It arrived in
   `desktop/apps/tur-centre/src/fixturesource.*`; whether a shared loader is
   wanted is for the second such program to show.
5. **`desktop/kit`**: `ListRow` and the keyboard, above.
6. **The `desktop` workflow** does not start when only the catalogue or the
   router changes. It does not need to: the fixtures are what the pictures
   are made from, and a change of the router that changes them fails the
   `rust` workflow until the fixtures, and then the pictures, are accepted.

# tur-centre: the application centre

How `tur-centre` is put together, how it asks the catalogue and the router,
what its fixtures are, and how each state of the design is reached. The design
is [desktop-experience.md](desktop-experience.md), section 8; the decisions are
[ADR-0007](../adr/0007-app-centric-routing.md) and
[ADR-0011](../adr/0011-application-centre-and-stores.md). What it reads is in
[catalogue-and-routing.md](catalogue-and-routing.md). The kit it is built on is
in [desktop-kit.md](desktop-kit.md).

Status: task D-10 of the design's plan, "find": the shell, search, an
application's page and the "no route" page. It reads; it installs nothing.
Report: [desktop-2.md](../reports/desktop-2.md).

## 1. Structure

Code: `desktop/apps/tur-centre/`. One QML module, `Tur.Centre` (library
`libturcentre`), and the program `tur-centre`, which is a window around it.

| Part | Files | What it is |
|---|---|---|
| Asking | `src/cataloguesource.h`, `processsource.*`, `fixturesource.*` | One interface, two sources of answers (section 2) |
| Reading | `src/answers.*` | Checks the shape and the vocabulary of an answer; turns every failure into the content of the kit's two-layer message |
| What the views read | `src/catalogue.*`, `src/hitsmodel.*` | `Catalogue`, with its `search` and its `application`; the rows of a search |
| Words | `qml/Words.qml` | The fixed vocabulary of the design's 8.2, and nothing else |
| Views | `qml/Shell.qml`, `HomeView`, `SearchView`, `ResultRow`, `ApplicationView`, `ApplicationRoutes`, `NoRoute`, `WayBlock`, `Facts` | Section 3 |
| Small parts | `RatingLine`, `RatingGlyph`, `CaveatRow`, `AppTile`, `BackButton`, `LinkButton`, `FailureMessage` | |

The C++ does what the design's 15.2 allows it and nothing else: it runs a
program, reads JSON, and holds a list model. It decides nothing about routing.
Which option is recommended, in which order the options stand, what each
caveat says: all of that is the router's, passed through.

**What is the router's and what is the design's.** Two kinds of text reach the
screen and they are kept apart in the code.

- *The fixed vocabulary* (`Words.qml`): the label and one-line explanation of
  each route, the five ratings, "according to whom", the outlook, the names of
  the categories, the primary button, and the fixed sentences of the "no
  route" page. Each stands for one value of the router or the catalogue. They
  go through `qsTr` and can be translated.
- *The router's text*: every reason, caveat, prerequisite and alternative, and
  the catalogue's error titles and messages. Shown as it is. No view writes
  one, shortens one or assembles one.

`tests/tst_vocabulary.qml` holds the vocabulary against the router's own list
of values (`fixtures/vocabulary.json`), and `src/answers.cpp` refuses an
answer that uses a route, a rating, an outlook, a severity or a kind of source
this program has no words for: it is never shown raw and never shown as
something else. That is the design's risk 17.

## 2. The backend interface

```cpp
struct CatalogueRequest { Command command; QString subject; };   // Search, Show, Route
struct CatalogueReply   { Failure failure; int exitStatus; QByteArray output, errorOutput; ... };

class CatalogueSource : public QObject {
    virtual quint64 submit(const CatalogueRequest &) = 0;   // returns at once
    virtual void cancel(quint64 id) = 0;
Q_SIGNALS:
    void finished(quint64 id, const CatalogueReply &);      // later, never from inside submit()
};
```

| Source | Used | Answers by |
|---|---|---|
| `ProcessSource` | The program, by default | Running `tur-catalogue [--catalogue DIR] <search\|show\|route> --json [--host FILE] [--prefs FILE] -- <subject>` |
| `FixtureSource` | Tests, the screenshot job, `tur-centre --fixture DIR` | Looking the question up in a directory of recorded answers (section 4) |

Everything above the interface is the same for both, which is what makes a
picture taken from fixtures a picture of the program.

`ProcessSource`:

- **Never a shell.** The program and its arguments are a vector. The subject
  (what was typed, or an identifier from an address) comes after `--`, so it
  is a value whatever it begins with.
- **Never blocking** (specification §30.1). Every run is asynchronous; at most
  four run at once and the rest wait their turn. A search that is replaced by
  a newer one is cancelled.
- **Bounded.** A run that prints more than 4 MiB, or lasts longer than ten
  seconds, is stopped and reported. Standard error is kept up to 64 KiB.
  Nothing is written to the program's input.
- A subject that is empty, longer than 200 characters or contains control
  characters is not passed on at all.

A search is one `search`; then, for each application it finds, one `show` and
one `route`, which fill that application's row in when they arrive. The rows
are on screen as soon as the search answers. An application's entry and route
are remembered while the program runs, so the page of a search result opens
without asking again.

**Failures.** `answers.cpp` turns whatever goes wrong into a code, a title, a
reason and a technical layer (the command, how it ended, the beginning of
standard error), shown by the kit's `Message`.

| What happened | Code |
|---|---|
| The catalogue reports an error in its JSON (`{"error": {...}}`) | The catalogue's own code, title and message: for example `TUR-DESKTOP-CATALOGUE-009`, "No application has this ID" |
| The catalogue reports on standard error and prints no JSON (a catalogue it cannot load) | The catalogue's code, from `error[CODE]: message` |
| The program could not be started | `TUR-DESKTOP-CENTRE-001` |
| It did not answer in time and was stopped | `TUR-DESKTOP-CENTRE-002` |
| It printed more than the limit and was stopped | `TUR-DESKTOP-CENTRE-003` |
| Its answer is not JSON, has the wrong shape, or uses a value with no words | `TUR-DESKTOP-CENTRE-004` |
| It ended abnormally | `TUR-DESKTOP-CENTRE-005` |
| A fixture directory holds no answer for the question | `TUR-DESKTOP-CENTRE-006` |
| The subject cannot be passed to the program | `TUR-DESKTOP-CENTRE-007` |

Two things are answers and not failures: a search that finds nothing (exit
status 6, with its document), and "no route" (exit status 3, with its
document). The `TUR-DESKTOP-CENTRE-*` codes are this program's own; like the
catalogue's they are not yet registry entries in the sense of specification
§29.3, and their sentences are in `answers.cpp` until a registry holds them.

**Command line.**

```text
tur-centre [address] [--fixture DIR]
           [--catalogue-program PROGRAM] [--catalogue DIR] [--host-facts FILE] [--preferences FILE]
           [--standalone] [--variant dark|light] [--rtl] [--check]
```

`address` is `tur-centre:home`, `tur-centre:search?q=WORDS` or
`tur-centre:app/<id>` (design, 8.1). `Shell.address` says where the shell is,
in the same form, and `Shell.open()` goes to one; anything else is refused.

## 3. The views, and how each state is reached

The window is `Shell`: a header with the search field, the side list of the
design's seven views, and one content area. An application's page takes the
whole width, with the way back in the header.

| State (design) | Reached by | What shows it |
|---|---|---|
| Home, with its standing sentence (8.1) | `tur-centre:home`; the window opens there | `HomeView` |
| The five views part 1 does not build (8.1) | Listed in the side list, unavailable | `Shell.sections` |
| Search: nothing asked (8.3) | `tur-centre:search`; an empty field | `SearchView`, title only |
| Search: under way | A query that has not been answered | The spinner; or the rows of the last answer, under the words they answer |
| Search: results, one row for each application, in the catalogue's order | A query the catalogue matches | `ResultRow`: the route the router recommends, its rating, what is needed first |
| Search: an application that cannot run is still listed | A hit whose recommended option is `no-route` | The row says "Cannot run on TUR" |
| Search: a class of applications (8.3, "Classes") | "banking app" | The class's row |
| Search: nothing found | Exit status 6 with no hits | "Nothing found for ...", "Tell the project you need it" |
| Search: the catalogue could not be asked | Any failure of section 2 | `FailureMessage`, with "Try again" where trying again can help |
| Application: being fetched (8.4) | `tur-centre:app/<id>`, before the answers | The spinner |
| Application: could not be fetched | An unknown identifier; any failure | `FailureMessage`, with the catalogue's code |
| Application: "How it will run" | A recommended option that is a route | `ApplicationRoutes`, `WayBlock` with `leading` |
| ... rating with its source | The option's `rating` and `rating_source` | `RatingLine`, `Words.sourceSentence` |
| ... "All reports (n)" (8.2) | More than one record for the route in the entry | A section that opens in place |
| ... "Needed first" | The option's `prerequisites` | The router's sentences |
| ... "Before you install", or "Nothing known stands in the way." | The option's `caveats`, blockers first; none | `CaveatRow`; notes under "More" when there are more than four |
| ... outlook `doubtful`: "May not work" | The option's `outlook` | `WayBlock` |
| "Other ways to run it" | Every other option, in the router's order | `WayBlock` for each |
| The primary button (8.4) | Prerequisites: "Set up…". A store: "Open in Steam". A publisher's page: "Open the publisher's page". A doubtful route: "Install anyway". Otherwise "Install". No route: no button | `Words.primaryAction`; not available in part 1 |
| The machine was assumed, not stated | The route's `host_assumed` | A banner on the page |
| "Where each fact was checked" (8.4) | The entry's references | `Facts`, under "Details" |
| No route (8.8) | A recommended option that is `no-route` | `NoRoute`: the statement, why, what to do instead, the ways that were considered, the code, the router's own words |

The "why" and "what you can do" of the "no route" page take the form the
design's table fixes, for the cases it names. Which case it is comes from the
router: the requirement of the blocker it repeats in its statement
(`kernel-anti-cheat`, `play-integrity`, `arm-only-native-code`) and the kind of
its alternative (`mac`). For any other case, and for kernel anti-cheat in
something that is not a game, the router's own reason and alternative are
shown.

## 4. The fixture rule

`desktop/apps/tur-centre/fixtures/` holds recorded answers of `tur-catalogue`
for the seed catalogue.

| Path | Content |
|---|---|
| `default/` | Eight searches, and `show` and `route` of every application they find and of Microsoft 365, Valorant, Krita, the Android banking class and Elden Ring; and of an identifier no application has. The machine is stated: the one the design's drawings assume, with the compatibility layers and enough memory, and neither Windows nor Android set up |
| `assumed-host/` | Netflix, with no host facts given: the router assumes a machine and says so |
| `vocabulary.json` | Every value of the router's types that reaches the screen, and the catalogue's codes |
| `update.py` | Writes the files from the log of the job that checks them |

A file is a JSON object with a list `invocations`; each records `command`,
`subject`, the `arguments` the program was given, its `exit_status`, its
`stderr`, and its `stdout`: the router's JSON as it printed it, field for
field and in its order.

**The rule: nobody writes a fixture by hand.** The test
`services/tur-catalogue/tests/desktop_fixtures.rs`, in the `rust` workflow,
runs `tur-catalogue` for every recorded question and fails when a file differs
from what the program prints, when one is missing, and when one is there that
nothing produces. The `rust` workflow starts when the fixtures change. So a
picture of the application centre cannot show a rating, a caveat or a sentence
the router does not give.

Nobody can run the router on a development machine, so CI makes the files:

1. Change the catalogue, the router, or the list of questions at the top of
   `desktop_fixtures.rs`, and push. The test fails and prints the files as they
   should be.
2. `desktop/apps/tur-centre/fixtures/update.py JOB_ID`, with the ID of the job
   `services/tur-catalogue`, writes them.
3. Look at `git diff`, commit and push. The pictures change with the fixtures;
   accept them as [desktop/README.md](../../desktop/README.md) describes.

`FixtureSource` does not guess: a question that was not recorded is answered
with `TUR-DESKTOP-CENTRE-006`, not with the nearest answer and not with
"nothing found". `tst_sources` holds `ProcessSource`'s arguments against the
arguments recorded in the fixtures, so the program is tested against answers
to the questions it really asks.

## 5. Tests

All in CI, in the `desktop` workflow, except the first row.

| What | Where | Proves |
|---|---|---|
| Fixtures | `services/tur-catalogue/tests/desktop_fixtures.rs` (`rust` workflow) | The fixtures are the router's real output; the Home view's standing sentence is still true of the catalogue |
| `tst_centre_answers` | `tests/unit/` | The three documents are read; malformed JSON, a wrong shape, an unknown value, the catalogue's errors and every failure of the asking each give their code |
| `tst_centre_sources` | | A missing program, a timeout, too much output, a crash, standard error, no input, the limit on runs at once, cancelling, no shell; a fixture answers what was recorded and nothing else |
| `tst_centre_catalogue` | | The search fills its rows in as answers arrive; only the last question asked is shown; an application is asked about once; a failure is not remembered |
| `tst_shell`, `tst_search`, `tst_application`, `tst_noroute` | `tests/` | Each state of section 3, with the exact words on screen |
| `tst_vocabulary` | | Every value of the router has its words, and the words are the design's |
| `tst_walks` | | The kit's contrast walk (both variants), accessibility walk (both directions) and text-size test (200 %, right-to-left with longer strings, both), over each of the eleven states the pictures are taken of |
| Screenshots | `tests/shots/render_centre.qml`, `desktop/tests/screenshots/centre-*.png` | Each state looks as approved, in the kit's matrix |
| `centre-starts*` | `CMakeLists.txt` | The program itself starts on a fixture, on an address, and without a catalogue program, with no QML warning |

## 6. Departures from the design

Each of these is a place where what was built is not what
[desktop-experience.md](desktop-experience.md), section 8, says or draws, with
the reason.

| Design | Built | Why |
|---|---|---|
| The directory is `apps/centre/` (15.1) | `apps/tur-centre/` | The task named it so; the program's name is the directory's |
| In-session programs use Kirigami and stock controls under `org.kde.breeze` (5.7, 14.1) | `Tur.Kit`'s controls only; the one stock control is a scroll bar | Nothing here needed Kirigami, so no dependency was added. What the program looks like under `org.kde.breeze`, beside KDE's applications, has not been seen |
| The drawings shorten what is needed first: "Windows set up on this computer", "your own Windows licence" (8.3, 8.4) | The router's sentences: "The Windows guest must be set up.", "You must supply a Windows licence: TUR does not include one." | 8.2 says every prerequisite is the router's text and the application centre writes none. The drawings and the rule disagree; the rule was followed. The router's sentences name the engine ("Windows guest", "Wine"), which the labels avoid |
| The router's reason is shown for a doubtful way and when there is no route (8.2) | Under every way, in the supporting colour | It is the router's text, and it is what tells two ways with the same label apart: Krita's Flatpak and its Fedora package are both "Linux application" |
| A row of a search shows route and rating on one line (8.3) | On two lines, with what is needed first under them | So that each wraps by itself at 200 % text |
| A blocked way is "greyed" (8.2) | Its label is in the supporting text colour, not the colour of unavailable controls | The reason it cannot be used is what the person came to read, and must meet the contrast requirement (13.1, X5) |
| "Nobody has reported on this way of running it. It may still work." for every rating nobody gave (8.2) | Left out on a way that is blocked; "No reports" stands alone | "It may still work" is untrue of a way that cannot be used |
| "Web application ... Opens office.com in its own window." (8.4) | "Opens the service's website in its own window." | The one-line explanation of 8.2, which is fixed; one explanation for each route |
| The primary button acts (8.4) | It says what will happen next and is unavailable, with a line the design does not have: "The application centre cannot install applications yet." | Installing is D-11. The design states no placeholder for it; this is the nearest honest thing |
| Host facts are detected (8.10, requirement 2) | When the router was told nothing, a warning the design does not have: "TUR has not checked what this computer has. How this application will run here is an assumption, and may turn out differently." | Nothing detects them yet, and "How it will run" must not read as a fact about this computer when it is not one |
| Error text comes from the code's registry entry (5.6) | The catalogue's own title and message for its codes; for this program's seven codes, sentences in `answers.cpp` | No registry exists yet |
| "Tell the project you need it" opens the project's issue page (8.3) | Hands `https://github.com/giorgiogiudice/TUR/issues` to the browser | The design does not give the address |
| Icons are Breeze's; TUR draws the five rating icons and neutral glyphs for kinds of application (5.4) | The kit's glyphs; the two rating icons the kit lacks are drawn in `RatingGlyph.qml`; a tile with the first letter of the name stands for an application | As for the kit: nothing recolours a Breeze icon without KIconThemes. The catalogue has no icons |
| The header has a menu sign and a magnifier in the field (8.3) | Neither | The side list does not collapse; the magnifier is decoration |

**Words that are neither the design's nor the router's.** Beside the two
sentences and the failure sentences above: the spinner's names for a screen
reader ("Searching the catalogue", "Asking the catalogue about this
application"); "Back to the results" and "Back to Home"; "Details of _name_";
"Checked _date_" under a reference; "Linux application, from another Linux
system" when the catalogue does not name the system; the names of nine of the
ten categories (the design gives "Productivity"); "Try again"; and the words a
screen reader hears before a caveat ("Warning:", "Note:"; the other two are the
design's). They are in `Words.qml`, `answers.cpp` and the views, and are for
the design's owner to confirm or replace.

## 7. Gaps

**What the router's output lacks for these screens.** None of these was
changed; each is a proposal for the owner of `tur-catalogue`.

| Gap | Effect on screen | Proposal |
|---|---|---|
| Reasons, caveats, prerequisites and alternatives are English text. Caveats and prerequisites carry a `code`; reasons carry none, and no text carries its parameters | Nothing the router says can be translated. In the right-to-left pictures its sentences stay English beside translated ones | The design's requirement 1 of 8.10: a stable identifier and the parameters for every sentence, the reason included |
| With `--json`, a catalogue that cannot be loaded is reported on standard error as text, and standard output is empty | The code is read out of standard error | Print `{"error": ...}` for those too |
| `search --json` returns the name, publisher and category of a hit only | Two more runs for each hit to fill its row in | Include the summary and the recommended option in a hit, or offer one command that returns both |
| The JSON of `route` does not carry the code `TUR-DESKTOP-CATALOGUE-011` when there is no route; only the exit status says so | The code is written in `Words.qml` and checked against the catalogue's list | Carry the code in the "no route" option |
| Nothing detects host facts, and nothing says where they or the preferences are kept | By default the router assumes a machine, and every page says "TUR has not checked what this computer has" | The design's requirement 2 of 8.10 |
| No installed location for the catalogue is defined | `--catalogue` must be given, or the program's default (`catalogue` in the current directory) applies | Decide the path with requirement 3 of 8.10 |
| The catalogue's error messages are written for a terminal: "no application in the catalogue has the ID ...; use `tur-catalogue search` to find it", in lower case and without a full stop | Shown as it is, under "Reason" | A message for a person beside the one for a terminal, or the identifier and parameters of requirement 1 |
| A way with a rating of `poor` or `broken` carries a caveat that repeats the rating, its source and the date, in another form ("Reported broken by WineHQ AppDB ("Garbage"), checked 2026-11-12.") | The same fact twice on the page, the second time with the date as the catalogue writes it | Leave the caveat out of the JSON, or mark it so that a caller that shows the rating can leave it out |
| A way that is blocked still lists what is needed first (the Android runtime for the banking class) | "Needed first" under "Cannot be used on this computer" | Leave prerequisites out of a blocked option |
| The catalogue has no icon, no long description and no screenshots | A tile with the first letter of the name; "About this application" is the one-line summary | Later; not needed to find an application |
| The name of another Linux system is an identifier (`debian`) | The label would read "Linux application, from debian" | A display name in the entry |
| No command lists the catalogue or a category | The Categories view cannot be built on the command line | With `org.tur.Catalogue1` |
| `route --json` does not say whether any record is TUR's own test | Home's standing sentence is shown unconditionally; a Rust test fails when it stops being true | A count in the output, or the service |

**Not built in part 1**, each with what it waits for:

- **Installing.** The primary button says what will happen next and is not
  available; one line says "The application centre cannot install applications
  yet." (D-11)
- **Choosing another way** ("Change…", "Always run it this way"). Nothing owns
  the preferences file the router reads.
- **The filter row** of the search wireframe ("Show: All, Linux, ..."). The
  design does not say whether it selects by the editions an application has or
  by the route it would run as, and the kit has no control for it.
- **Categories, Installed, Updates, Sources, Open a file**: listed in the side
  list, unavailable. (D-11, D-12; Categories needs a way to list the
  catalogue.)
- **Stores**: "Also found in ...", "Still searching: ...", a store that fails,
  "offline". (D-11)
- **Nothing found**: the sentence about looking at a file and "Open a file…"
  are left out until TUR can look at a file (D-12).
- **Details**: editions, the licence, what the application can reach, where
  its data is.
- **"See the report"** for TUR's own tests: the report's path is shown; it is
  a path in the repository, which nothing here can open.
- **The buttons of the "no route" page** ("How to start Windows", "Search
  similar games", "Switch on the experimental macOS layer"): their sentences
  are shown; what they would open does not exist yet.
- **`org.tur.Addressable1`**, the desktop file and the file-type handlers.
- **Announcements** (X4), **high contrast** (X7) and the **AT-SPI** comparison:
  as for the kit ([desktop-kit.md](desktop-kit.md), section 6).
- **Translations.** Every string of the fixed vocabulary goes through `qsTr`;
  no catalogue is extracted or loaded. The count of results uses two strings
  ("1 application", "%1 applications") instead of `%n`, because without a
  translation loaded Qt would print "2 application(s)".

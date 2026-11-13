# tur-centre: the application centre

How `tur-centre` is put together, how it asks the catalogue and the router,
what its fixtures are, and how each state of the design is reached. The design
is [desktop-experience.md](desktop-experience.md), section 8; the decisions are
[ADR-0007](../adr/0007-app-centric-routing.md) and
[ADR-0011](../adr/0011-application-centre-and-stores.md). What it reads is in
[catalogue-and-routing.md](catalogue-and-routing.md). The kit it is built on is
in [desktop-kit.md](desktop-kit.md).

Status: tasks D-10, "find" (the shell, search, an application's page and
the "no route" page; report [desktop-2.md](../reports/desktop-2.md)), and
D-11, "install" (installing with its plan, progress and cancel; Open;
Installed; Updates; Remove keeping the person's data; Sources; "I have the
installer file"; Chrome for people coming from Windows; report
[desktop-7.md](../reports/desktop-7.md)). It installs through `tur-apps`
([app-install-backend.md](app-install-backend.md)), which it runs as a
program of the person's session (section 2a).

## 1. Structure

Code: `desktop/apps/tur-centre/`. One QML module, `Tur.Centre` (library
`libturcentre`), and the program `tur-centre`, which is a window around it.

| Part | Files | What it is |
|---|---|---|
| Asking | `src/cataloguesource.h`, `processsource.*`, `fixturesource.*` | One interface, two sources of answers (section 2) |
| Reading | `src/answers.*` | Checks the shape and the vocabulary of an answer; turns every failure into the content of the kit's two-layer message |
| What the views read | `src/catalogue.*`, `src/hitsmodel.*` | `Catalogue`, with its `search` and its `application`; the rows of a search |
| Installing | `src/appssource.h`, `appsprocess.*`, `appsfixture.*` | One interface to `tur-apps`, two sources: the program, and recorded runs of it (sections 2a, 4) |
| What the install views read | `src/apps.*` | `Apps` (the list, the sources, one operation per application) and `AppsOperation` (plan, progress, cancel, result or failure) |
| Words | `qml/Words.qml`, `qml/RouterWords.qml` | The fixed vocabulary of the design's 8.2 and of installing; the router's sentences written from their identifiers (issue #41) |
| Views | `qml/Shell.qml`, `HomeView`, `SearchView`, `ResultRow`, `ApplicationView`, `ApplicationRoutes`, `NoRoute`, `WayBlock`, `Facts`, `InstalledView`, `UpdatesView`, `SourcesView` | Section 3 |
| Installing, on a page | `InstallSheet`, `OperationBlock`, `OnThisComputer`, `RemoveSheet`, `InstallerDialog` | The plan; progress, cancel and the end; what is installed; the removal's question; the file chooser |
| Small parts | `RatingLine`, `RatingGlyph`, `CaveatRow`, `AppTile`, `BackButton`, `LinkButton`, `FailureMessage`, `ChoiceRow`, `InstalledRow` | |

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
  the catalogue's error titles and messages. No view writes one, shortens one
  or assembles one. Since D-11 the reasons, the prerequisites and the caveats
  without parameters are written from the router's stable identifiers
  (`reason_parts`, `code`, `text_id` and `params`; issue #41) by
  `RouterWords.qml`, through `qsTr`, so that they can be translated; in
  English each is the router's own sentence, word for word, and
  `tests/tst_routerwords.qml` holds every option of the recorded answers to
  that. A sentence whose identifier or parameter this version does not know
  is shown in the router's words, as before. Alternatives and the caveats
  with parameters are still the router's English.
- *tur-apps' text*: the plan's sentences (how a source is verified, what a
  download is, what a permission allows, what removal does), its activity
  lines and its failure messages. Shown as they are, under the centre's own
  headings; a failure's title is the centre's, by the code.

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

`address` is `tur-centre:home`, `tur-centre:search?q=WORDS`,
`tur-centre:app/<id>`, `tur-centre:installed`, `tur-centre:updates` or
`tur-centre:sources` (design, 8.1). `Shell.address` says where the shell is,
in the same form, and `Shell.open()` goes to one; anything else is refused.
D-11 adds `--apps-program PROGRAM` (default `tur-apps`, on the `PATH`) and
`--apps-fixture DIR` (recorded runs of `tur-apps`; nothing is installed). The
catalogue directory, host facts and preferences given to the router are
given to `tur-apps` too.

The Home view's standing sentence asks `tur-catalogue validate --json`
(added for it) and stands only while `rated_by_tur_tests` is 0; while the
count is not known, nothing is said about it.

## 2a. Installing: how the centre runs tur-apps

**Decision: a child process of the person's session, not the proposed
`org.tur.Apps1` service.** `tur-apps` installs per user (`flatpak --user`,
`tur-runtime` in the home directory), so it needs no privilege the
application centre does not have, and nothing is gained by putting a bus
between two programs of the same user. The command line already has
everything the screens need: `plan --json`, `install --events` (JSON lines),
`list --json`, `sources --json`, `--keep-data`/`--delete-data`; the design
note says the centre can run it "as it runs `tur-catalogue` today"
(app-install-backend.md, "How D-11 will use it"). A service would add an
activation file, a bus policy, packaging and an image change for no new
ability. It becomes worth it when something other than the centre must
watch or cancel a job (P@x's class-2 tools, a notification after the window
is closed); the interface is proposed and its methods map one to one onto
the subcommands, so the move is mechanical. What is lost meanwhile:
closing the window during an installation asks it to cancel and waits up to
30 seconds for it to take back what it did (an update or removal is let
finish), instead of letting it carry on.

| Centre | `tur-apps` (arguments after the global `--catalogue`, `--host`, `--prefs`) |
|---|---|
| Install: the plan | `plan --json [--installer FILE] -- ID` |
| Install: carried out | `install --yes --events [--installer FILE] -- ID`; standard input stays open |
| Cancel | the line `cancel` on that input |
| Update | `update --events -- ID` |
| Remove | `remove --keep-data --events -- ID` or `remove --delete-data --events -- ID`: always one of the two, from the person's answer |
| Installed | `list --json` |
| Updates | `list --check-updates --json` |
| Sources | `sources --json`; `sources enable --json -- NAME`, `sources disable --json -- NAME` |
| Open | `launch -- ID`, started detached, so that the application outlives the window |

`AppsProcess` never uses a shell; the subject comes after `--`; a run that
asks or lists is stopped after two minutes (it may ask Flathub); an
installation, update or removal has no time limit and reports all along;
output is bounded at 16 MiB.

**Cancelling** needed one change in `tur-apps` (`src/cancel.rs`): with
`install --events`, the line `cancel` on standard input asks to cancel, and
`tur-apps` honours it only where everything can be taken back: before
anything is changed; while a pinned installer downloads (curl is stopped,
the partial file deleted); after the download, before `tur-runtime` starts;
and while `flatpak install` runs (flatpak and everything it started are
stopped as a process group, SIGTERM then SIGKILL, and what it had added is
uninstalled, as after a failure, then checked). It then ends with
`TUR-DESKTOP-APPS-013`, "Cancelled. Nothing was installed." and what was
taken back. The centre offers Cancel only in those steps
(`AppsOperation.cancellable`) and says "Cancelled. Nothing was installed."
only when that is the answer; if the installation finished first, it says
it is installed.

**Errors** are the kit's two-layer message with the code: `tur-apps`' own
(`TUR-DESKTOP-APPS-001` to `-017`, its message as the reason, the centre's
translatable title for the code), or the centre's for the running of it:
`TUR-DESKTOP-CENTRE-001` (could not be started), `-002` (timed out), `-003`
(too much output), `-004` (an answer that cannot be read, or a run that
ended without a result or an error), `-005` (crashed), `-006` (not recorded,
fixtures only), `-007` (a subject that cannot be passed), `-008` (an
installed application could not be started). A failure leaves the page as
it was before the attempt, with "Try again" where that can help, "Switch on
Flathub and try again" for `TUR-DESKTOP-APPS-008`, and "Choose the
installer…" for `TUR-DESKTOP-APPS-005`.

## 3. The views, and how each state is reached

The window is `Shell`: a header with the search field, the side list of the
design's seven views, and one content area. An application's page takes the
whole width, with the way back in the header.

| State (design) | Reached by | What shows it |
|---|---|---|
| Home, with its standing sentence (8.1) | `tur-centre:home`; the window opens there; the sentence while `validate --json` counts no rating of TUR's | `HomeView` |
| The two views not built yet, Categories and Open a file (8.1) | Listed in the side list, unavailable | `Shell.sections` |
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
| The primary button (8.4) | Prerequisites: "Set up…". A store: "Open in Steam". A publisher's page: "Open the publisher's page" (opens it), unless the catalogue pins a free installer, which TUR downloads: "Install". A doubtful route: "Install anyway". Otherwise "Install". No route: no button. Installed: "Open" | `Words.primaryAction`; "Install" acts for the routes `tur-apps` installs by (Flatpak, Wine); otherwise it is unavailable, with one line saying why |
| Installing: the plan is being made (D-11) | "Install" | `OperationBlock`, a spinner in place of the button |
| ... the plan (source and how it is verified, what is downloaded and its size, what it may reach, how it is removed, licence and who packages it) | The plan answers | `InstallSheet`; nothing is done until its "Install" |
| ... progress | The sheet's "Install" | `OperationBlock`: the step, a bar with numbers only when `tur-apps` gives a total, the last activity line, Cancel while it is safe |
| ... done | The result event | "_name_ is installed."; the button becomes "Open"; "On this computer" appears |
| ... failed | The error event, or a failure of the running | The two-layer message with the code; the page as before the attempt |
| ... cancelled | Cancel, answered with `TUR-DESKTOP-APPS-013` | "Cancelled. Nothing was installed." |
| ... a source switched off | The plan refused with `TUR-DESKTOP-APPS-008` | The message, and "Switch on Flathub and try again" |
| "I have the installer file" | A Windows application (route `wine`) | The file chooser; then the same flow, with `--installer FILE`, from `tur-apps`' hash check |
| On this computer (8.4) | An installed application | `OnThisComputer`: how it runs, version, from, space, where its data is, its update state; Update; Remove… (only for what `tur-apps` installed) |
| Remove's question (8.5) | Remove… | `RemoveSheet`: "Keep its settings and documents" chosen; "Remove its settings and data too" |
| Installed (8.1) | `tur-centre:installed` | `InstalledView`: every application of `tur-apps list`, across routes, with route, source, version, size and update state; Open, Details |
| Updates (8.5) | `tur-centre:updates` | `UpdatesView`: asks the sources; grouped by where each came from; Update, Update all; "TUR itself is updated in Settings, not here."; "Everything is up to date." |
| Sources (8.6) | `tur-centre:sources` | `SourcesView`: the person's Flatpak sources, a switch for Flathub and Fedora's; the design's default stated |
| Chrome for people coming from Windows (17.1, answer 4) | Home; `tur-centre:app/com.google.chrome` | A card on Home with "Install Google Chrome…" (the page and its plan, one action) and where Chrome comes from; the same sentence on its page and in its plan |
| "Works with these settings" | An option with `settings` (a `tur-autopilot` record) | `WayBlock`: the remedies, who found them, "not a rating"; such a record in "All reports" says the same |
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
| `default/` | Eight searches, and `show` and `route` of every application they find and of Microsoft 365, Valorant, Krita, the Android banking class, Elden Ring, Chromium B.S.U., Google Chrome and Notepad++; and of an identifier no application has; and `validate --json`. The machine is stated: the one the design's drawings assume, with the compatibility layers and enough memory, and neither Windows nor Android set up |
| `assumed-host/` | Netflix, with no host facts given: the router assumes a machine and says so |
| `vocabulary.json` | Every value of the router's types that reaches the screen, and the catalogue's codes; since D-11 every reporter, `tur-autopilot` included |
| `update.py` | Writes the files from the log of the job that checks them |
| `apps/<scenario>/runs.json` | Recorded runs of the real `tur-apps` (below) |
| `apps/update.py` | Copies them out of the artifact of the job that records them |

**Recorded runs of `tur-apps`** (D-11). `desktop/ci/centre-e2e/record.py`
runs the real `tur-apps`, in the desktop workflow's job `centre-e2e`, on a
fresh user with Fedora 45's flatpak and Flathub, with exactly the argument
vectors of section 2a, and writes what it printed line by line and how it
ended. One file per scenario: `sources` (Flathub not added, the plan
refused with `TUR-DESKTOP-APPS-008`, Flathub switched on, the plan made),
`install` (Chromium B.S.U.: the list, the plan, the installation's events,
the list after, the update check), `update` (an earlier commit put in place
with flatpak, then `tur-apps update`), `remove` (keeping its data),
`cancel` (an installation cancelled once flatpak reports its first
operation; the file says after which line "cancel" was sent), `chrome`
(Google Chrome's plan, installation and removal), `failure` (the one
failure Flathub does not give on demand, flatpak failing half-way: the real
`tur-apps` against its own stand-in for flatpak, and the file says so) and
`installer` (a file that is not Notepad++'s pinned installer). `AppsFixture`
answers a request with the first recorded run of the same arguments that
has not answered yet, and then the last of them; plays a streaming run back
line by line; and stops a cancelled run where it was cancelled until the
centre cancels too. `tst_centre_apps` holds every recorded vector against
`AppsRequest::arguments`. The same rule holds: nobody writes these by hand;
`apps/update.py RUN_ID` copies a new recording.

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

All in CI, in the `desktop` workflow, except the first two rows.

| What | Where | Proves |
|---|---|---|
| Fixtures | `services/tur-catalogue/tests/desktop_fixtures.rs` (`rust` workflow) | The fixtures are the router's real output; `validate --json` gives Home the catalogue's count of TUR's own ratings |
| Cancelling in `tur-apps` | `services/tur-apps/tests/cli.rs` `a_cancelled_flatpak_installation_is_stopped_and_takes_back_what_it_added`; `exec::tests::a_program_is_stopped_with_what_it_started_when_asked` (`rust` workflow) | "cancel" stops flatpak and what it started, takes back the runtime it had added, records nothing, and ends with `TUR-DESKTOP-APPS-013`; without it the same installation succeeds |
| `tst_centre_apps` | `tests/unit/` | `tur-apps`' argument vectors, held against every recorded run; streaming lines, "cancel" written to an installation, no input otherwise, a missing program, a timeout, too much output, a crash, a subject that cannot be passed; failures carry codes and the centre's words; recorded runs play back in order and hold where cancelled |
| `tst_install` | `tests/` | On the recorded runs: the plan's sheet, progress with Cancel, success and Open; a failure with its code and the page as before; cancel; Remove keeping the data; Update and Update all; Sources; "Switch on Flathub and try again"; "I have the installer file"; Chrome from Home |
| `tst_routerwords` | `tests/` | Every reason, prerequisite and caveat written from its identifier is the router's English, for every recorded option |
| End to end | `tests/e2e/e2e_*.qml`, `desktop/ci/centre-e2e/` (job `centre-e2e`) | The real centre, the real `tur-apps` and `tur-catalogue`, Fedora 45's flatpak, Flathub: Flathub switched on in Sources; Install from the page; the plan; progress; Open (the game's window found and photographed); Installed; Update; Remove keeping the data; Install again and Cancel; checked from outside: data kept, nothing installed |
| `tst_centre_answers` | `tests/unit/` | The three documents are read; malformed JSON, a wrong shape, an unknown value, the catalogue's errors and every failure of the asking each give their code |
| `tst_centre_sources` | | A missing program, a timeout, too much output, a crash, standard error, no input, the limit on runs at once, cancelling, no shell; a fixture answers what was recorded and nothing else |
| `tst_centre_catalogue` | | The search fills its rows in as answers arrive; only the last question asked is shown; an application is asked about once; a failure is not remembered |
| `tst_shell`, `tst_search`, `tst_application`, `tst_noroute` | `tests/` | Each state of section 3, with the exact words on screen |
| `tst_vocabulary` | | Every value of the router has its words, and the words are the design's |
| `tst_walks` | | The kit's contrast walk (both variants), accessibility walk (both directions) and text-size test (200 %, right-to-left with longer strings, both), over each of the twenty-two states the pictures are taken of; a state with a sheet is walked on the sheet, which must hold the keyboard |
| Screenshots | `tests/shots/render_centre.qml`, `desktop/tests/screenshots/centre-*.png` | Each state looks as approved, in the kit's matrix |
| `centre-starts*` | `CMakeLists.txt` | The program itself starts on a fixture, on an address, on Installed, Updates and Sources from recorded runs, and without a catalogue program or `tur-apps`, with no QML warning |

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
| The primary button acts (8.4) | It acts for "Install" by the two routes `tur-apps` installs by, for "Open the publisher's page" and for "Open". "Set up…", "Open in _Steam_" and "Install" by any other route are unavailable, each with a line the design does not have ("The application centre cannot set up what is needed first yet.", "… cannot open stores yet.", "… cannot yet install applications that run this way.") | Setting up Windows and Android (D-25, the Android milestones), the store adapters and the other routes are not built; the line says so instead of a button that fails |
| "Pressing Install starts at once when nothing else is needed" (8.5) | Install first shows the plan in a sheet; nothing is done until its "Install" | The task asks for the plan first (source, size, access, removal, licence); the design's sheets of 8.5 are for when something must happen first, and the plan is such a sheet |
| Once installed, "Remove" and "Repair" are in a menu (8.4) | "Update" and "Remove…" are buttons in "On this computer"; there is no Repair | The kit has no menu; `tur-apps` has no repair |
| "Remove its settings and data too (1.4 GB)" (8.5) | Without the size | `tur-apps list` gives the space the application takes, not its data's |
| Updates: "TUR 0.4.0 is ready. Restart to use it. [Open Updates in Settings]" (8.5) | "TUR itself is updated in Settings, not here." | Nothing reads `org.tur.Update1` yet; the sentence is true without it |
| Sources: every source and way of running applications (8.6) | The person's Flatpak sources only, with Flathub and Fedora's switchable; one line says stores and setting up Windows and Android are not here yet | That is what `tur-apps sources` manages; stores and the guest flows are later tasks |
| Progress: "Downloading · 310 MB of 1.1 GB" (8.5) | For a Flatpak, "Downloading and installing" with an indeterminate bar and flatpak's own line ("installing runtime/…") | flatpak's command line gives no machine-readable progress (app-install-backend.md); a bar with numbers appears for a pinned download, which has a total |
| Chrome "installed from Google's own source and updated by it" (17.1, answer 4) | Installed per user from Flathub; Flatpak downloads Google's package from dl.google.com at installation (the "extra data" of Flathub's manifest); the package around it is maintained by Flathub's volunteers, not by Google, and the page and the plan say so | Google publishes no Flatpak; its own Linux packages are an RPM repository, which on an atomic system means layering a system package for every user with administrator rights and a restart, and `tur-apps` installs per user. The browser itself does come from Google's server |
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

D-11 adds, for the same review: the plan's headings ("Where it comes from",
"What is downloaded", "What it may reach on this computer", "How it is
removed", "Licence"), "Install _name_?", "With Remove, in this application
centre. Its settings and documents are kept unless you choose otherwise.",
the licence and packaging sentences ("Open source.", "Proprietary: the
publisher's terms apply.", "Packaged by volunteers, not by its publisher."),
the step labels ("Downloading and installing", "Setting it up in the Windows
compatibility layer", "Checking the download", "Finishing", "Keeping its
settings and documents", "Removing what nothing else uses"), "Making the
plan: where it comes from, how large it is, what it may reach", "_name_ is
installed.", "_name_ is updated." / "was already up to date.", "_name_ is
removed. Its settings and documents were kept." / "…, with its settings and
data.", "Cancelling…", "Switch on Flathub and try again", "I have the
installer file", "Choose the installer", the labels of "On this computer"
and the Installed, Updates and Sources views, the access kinds ("Files",
"Devices", "Network", "Display", "Sound", "Services of the session"),
"Installed outside the application centre…", the sources sentence ("TUR's
choice is that Flathub is on for everyone…"), Chrome's offer and origin
sentences, "Tested by TUR on _date_ · TUR has not rated how well it works"
and the autopilot's sentences, and the titles of `tur-apps`' codes (its own,
made translatable).

## 7. Gaps

**What the router's output lacks for these screens.** None of these was
changed; each is a proposal for the owner of `tur-catalogue`.

| Gap | Effect on screen | Proposal |
|---|---|---|
| Reasons, caveats, prerequisites and alternatives are English text, with identifiers since apps-1 (`reason_parts`, `text_id`, `params`) | Reasons, prerequisites and the caveats without parameters are written from their identifiers (section 1); alternatives and the requirement caveats not yet in `RouterWords.qml` stay English in the right-to-left pictures | Add the remaining caveats and the alternatives to `RouterWords.qml`; the test holds each to the router's English |
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
| `route --json` does not say whether any record is TUR's own test | Solved in D-11 with `tur-catalogue validate --json`, asked once for Home | — |

**What `tur-apps`' output lacks for these screens** (proposals for its owner):

| Gap | Effect on screen | Proposal |
|---|---|---|
| The plan's sentences (verification, downloads, access, removal) and the activity lines are English, with no identifier | They stay English under translated headings | Identifiers and parameters, as the router now has |
| flatpak gives no progress figures | An indeterminate bar for every Flatpak installation | Use `libflatpak`'s transaction progress in a service, or flatpak's `--json` progress if it gains one |
| `list` gives the space an application takes, not the size of its data | Remove's question cannot say how much "Remove its settings and data too" frees | The size of the data places in `list` |
| A cancelled installation ends with `TUR-DESKTOP-APPS-013`, the code for "nothing was done without your confirmation" | The centre tells a cancel from a refusal by whether it asked to cancel | A code of its own (it needs the design note's table changed) |
| `launch` waits for the application | The centre starts it detached and cannot report its failure after it started | A `launch --detach` that reports whether it started |

**Not built**, each with what it waits for:

- **Choosing another way** ("Change…", "Always run it this way"). Nothing owns
  the preferences file the router reads.
- **The filter row** of the search wireframe ("Show: All, Linux, ..."). The
  design does not say whether it selects by the editions an application has or
  by the route it would run as, and the kit has no control for it.
- **Categories and Open a file**: listed in the side list, unavailable (D-12;
  Categories needs a way to list the catalogue).
- **Stores**: "Also found in ...", "Still searching: ...", a store that fails,
  "offline"; Steam and the others in Sources (store adapters).
- **Setting up Windows and Android** ("Set up…"; 8.9): D-25 and the Android
  milestones. Installing by any route but Flatpak and Wine: `tur-apps`.
- **Updating a Windows application**: `tur-runtime` has no update; `tur-apps`
  refuses it with `TUR-DESKTOP-APPS-004`, and the centre shows that message.
- **The compatibility layer's update offer** (8.5, "A newer compatibility
  layer is available…"): nothing pins a runtime per application yet.
- **The operating system's card in Updates**: `org.tur.Update1`.
- **Flathub on by default**: the image or the first-run setup must switch it
  on for each person (app-install-backend.md); until then Sources and the
  plan's failure offer the switch.
- **System-wide Flatpaks** are not in Installed (`tur-apps list` is per user).
- **Nothing found**: the sentence about looking at a file and "Open a file…"
  are left out until TUR can look at a file (D-12).
- **Details** of an application that is not installed: editions, the
  licence, what it can reach (the plan says the last two before installing;
  "On this computer" says where its data is once it is installed).
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

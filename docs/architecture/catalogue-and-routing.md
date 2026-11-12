# Application catalogue and router

Design note for the first slice of app-centric routing
([ADR-0007](../adr/0007-app-centric-routing.md),
[ADR-0011](../adr/0011-application-centre-and-stores.md); specification §2.3,
§7.3, §7.4, §19.2). Code: `services/tur-catalogue/`. Data: `catalogue/`.
Entry schema: `schemas/app-catalogue-entry.schema.json`.

## Scope

TUR is app-centric: the user asks for an application and the system chooses
the best route to run it. This slice provides the two things that decision
needs.

- **The catalogue.** One entry per application: the editions it exists in,
  what each edition requires, and what is known about how well each route
  runs it.
- **The router.** A pure function from a catalogue entry, facts about the
  machine and the user's preferences to an ordered list of routes, following
  the ladder of ADR-0007.

It also provides search over the catalogue and a command-line tool.

Nothing here installs, launches or fetches anything. The router is a function
of its three inputs and returns values. See [Not built](#not-built).
Installing by the router's answer is `tur-apps`
([app-install-backend.md](app-install-backend.md)), which links this crate.

A route is a way of running an application. It is never a statement that the
application works: that needs a recorded test (§2.3). The catalogue keeps the
two apart, and so does every report the tool prints.

## Entry format

An entry is one TOML file, `catalogue/<id>.toml`. The format is version 1 and
is described by `schemas/app-catalogue-entry.schema.json` (JSON Schema
2020-12), which describes the same data model. TOML was chosen because entries
are written and reviewed by people and need comments. Unknown fields are
refused at every level.

| Field | Required | Content |
|---|---|---|
| `schema_version` | yes | `1` |
| `id` | yes | Stable application ID: lower case, dot-separated, at least two segments. The file is named after it |
| `entry_kind` | no | `application` (default) or `class`: a class of applications that share a routing answer, whose editions may leave identifiers out |
| `name` | yes | The name shown to the user |
| `aliases` | no | Other names people search for |
| `publisher` | yes | Who publishes it |
| `category` | yes | `game`, `productivity`, `creative`, `communication`, `development`, `utility`, `media`, `internet`, `finance`, `education`. Games are routed differently |
| `summary`, `homepage` | no | One line; the publisher's page |
| `editions` | yes | The ways the application exists. At least one |

Each edition has a `kind` and the identifiers of that kind:

| `kind` | Identifiers | Routes that can run it |
|---|---|---|
| `flatpak` | `flatpak_id`, `remote` (default `flathub`) | `flatpak` |
| `linux-native` | `obtain`: `fedora-package` with `package`; `other-distribution-package` with `distribution` and `package`; `steam` with `store_id`; `publisher-page` with `download_page` | `native`; `distro-container` for another distribution's package |
| `windows` | `obtain`: `steam`, `epic`, `gog` or `microsoft-store` with `store_id`; `publisher-page` with `download_page` | `proton`, `wine`, `windows-guest` |
| `android` | `package`; `stores`: `f-droid`, `google-play`, `publisher-page` (with `download_page`) | `android-runtime` |
| `web` | `url`; `primary` when the web application is the publisher's primary form | `web` |
| `macos` | `obtain`: `mac-app-store` or `steam` with `store_id`; `publisher-page` with `download_page` | `darling` |

A Windows edition says where it is obtained: a store identifier, or the
publisher's download page. `download_page` is a page for a person to read. A
URL that ends in an installer or archive name (`.exe`, `.msi`, `.dmg`, `.apk`,
`.zip` and others) is refused, because TUR never fetches a binary the user did
not ask for. The Microsoft Store and Apple's App Store can be recorded as
facts; TUR cannot integrate them (ADR-0011, rule 5), and the router says so.

**The pinned installer** ([ADR-0019](../adr/0019-app-install-backend.md)). A
Windows edition obtained from the publisher's page and marked `open-source`
may carry `[editions.installer]`, the one place an entry names a file to
download: `tur-apps` fetches it when the user asks for the application, and
nothing in it runs before its size and digest match.

| Field | Content |
|---|---|
| `url` | The publisher's own `https` location of the file; the path ends in the file's name, `.exe`, `.msi` or `.zip`, with no query or fragment |
| `sha256`, `size` | 64 lower-case hexadecimal digits; bytes, at most 4 GiB |
| `version` | The version the file installs |
| `licence`, `licence_url` | The licence as the publisher states it, and where it was read |
| `hash_source` | Where the digest was taken from: the publisher's checksum file or the digest its release page publishes |
| `checked` | The day all of it was checked |
| `portable` | The file is a single program, copied into place, never run to install |

The validator refuses an installer on any other kind of edition, on an
edition obtained elsewhere than the publisher's page, and on an edition that
is not marked `open-source` (`TUR-DESKTOP-CATALOGUE-003`): an entry for a
proprietary application only ever takes the user's own file. Pins are filled
in for the four applications measured in `tests/windows-runtime/apps.json`,
from those pins: 7-Zip, Notepad++, PuTTY and VLC.

Every edition may also carry `licence` (`open-source` or `proprietary`),
`packaging` (`publisher` or `community`, for Linux packages), `notes`, and
three lists.

**`requirements`**, the known facts that decide routing:

| Requirement | Meaning |
|---|---|
| `kernel-anti-cheat` | Anti-cheat that works only on real Windows: it loads a Windows kernel driver, or its publisher has refused to enable it for Proton |
| `hardware-attested-drm` | Copy protection that needs hardware attestation, such as Widevine level 1 |
| `play-integrity` | Google's Play Integrity device verdict |
| `arm-only-native-code` | Native code built for ARM processors only |
| `kernel-driver` | A kernel driver of the edition's own operating system |
| `hardware-dongle` | A hardware licence dongle |
| `heavy-gpu` | Heavy use of the graphics processor |
| `online-account` | An online account with the publisher |

**`records`**, the compatibility records. Each has a `route`, a `rating`, who
reported it, the `date`, and optionally `version_tested` and `notes`. A route
without a record is unknown.

**`references`**, the public pages the edition's facts were checked against,
each with a title, a URL and the date it was checked.

### Rating scale

| Rating | Meaning |
|---|---|
| `good` | Runs as intended, with no workaround worth mentioning |
| `usable` | Runs and is usable; needs tweaks or has minor problems |
| `poor` | Runs, with problems that get in the way of normal use |
| `broken` | Does not start, or cannot be used for its purpose |
| `unknown` | Not known. The only rating that needs no source |

How each external source's own scale maps onto this one is in
[`catalogue/README.md`](../../catalogue/README.md). The source's own word is
always kept beside the mapped rating.

## Sourcing and the honesty rule

TUR has rated nothing yet. The catalogue therefore holds only what a public
source reports, attributed to that source, and `unknown` everywhere else.
TUR's own automated tests of four Windows applications are recorded, with
their report, as `unknown` (see [Not built](#not-built)).

A record states who it comes from in `reported_by`:

| `reported_by` | Allowed rating | Must carry | Must not carry |
|---|---|---|---|
| `nobody` | `unknown` only | | Any source field |
| `external` | Any | `source_name`, `source_url`; and `source_rating`, the source's own words, unless the rating is `unknown` | `test_report`; a source name that is TUR itself |
| `tur-test` | Any | `test_report`, a path under `docs/reports/` or `tests/`; `version_tested` | Any external source field |

The rules are enforced, not only written down:

- A rating other than `unknown` without a source is refused
  (`TUR-DESKTOP-CATALOGUE-006`).
- A record that claims TUR's own test without a test-report reference is
  refused with the same code. Unit test:
  `a_tur_test_claim_without_a_report_reference_is_refused`; through the
  command line: `validate_refuses_a_tur_test_claim_without_a_report`.
- When the reference is present, the report file must exist in the
  repository. `tur-catalogue validate --repository <root>` checks it, and so
  does the seed test
  `no_seed_record_claims_tur_s_own_test_without_a_test_report`.
- An external record never reads as TUR's. The router's output carries the
  source's name, URL, own rating and date; the text report prints "reported by
  ... ; not tested by TUR".

An entry with every route unknown is acceptable. An invented rating is not.

The same care applies to the other facts. In the seed, every edition carries a
reference or a note that says what could not be confirmed, and no requirement
is entered on an edition without a reference
(`every_seed_edition_and_requirement_is_backed_by_a_reference`). A test cannot
check that a reference really supports a fact; a reviewer has to.

The seed catalogue holds 35 entries: the 32 of the first slice, and
Notepad++, PuTTY and Klavaro, added with `tur-apps`. Twelve records carry a
rating, each from one of four sources; every other route is unknown. Four
further records are TUR's own tests, all `unknown` (see [Not
built](#not-built)). The sources, all checked on 12 November 2026:

| Source | Used for |
|---|---|
| ProtonDB, `https://www.protondb.com/`: the summary tier of a Steam application | Rating of the `proton` route for Elden Ring, Cyberpunk 2077, Baldur's Gate 3 and Apex Legends |
| Valve Steam Deck compatibility, as shown on the Steam store; category definitions at `https://www.steamdeck.com/en/verified` | Rating of the `proton` route for Elden Ring, Cyberpunk 2077 and Apex Legends |
| WineHQ Application Database, `https://appdb.winehq.org/`; rating definitions at `https://gitlab.winehq.org/winehq/appdb/-/wikis/Rating-Definitions` | Rating of the `wine` route for Microsoft 365, Adobe Photoshop and Visual Studio |
| AreWeAntiCheatYet, `https://areweanticheatyet.com/` | Anti-cheat status; rating of the `proton` route for Valorant and Apex Legends, whose status is Denied |
| Steam store listings and the store's public application API | Steam application IDs, publishers, platforms |
| Flathub listings and verification status, `https://flathub.org/` | Flatpak IDs, licences, whether the publisher maintains the package |
| Fedora Packages, `https://packages.fedoraproject.org/` | Fedora package names |
| Google Play listings; F-Droid, `https://f-droid.org/` | Android package names and stores |
| Publishers' own pages, named in each entry's references | Editions, download pages, requirements |

Limits of the seed data, stated plainly:

- Adobe's own pages refused automated requests; the Photoshop entry carries no
  fact from them except the address of the product page, and says so.
- ProtonDB's tier definitions could not be read automatically; its tiers are
  mapped by their order.
- A requirement is entered only where a reference supports it. Many entries
  therefore list fewer requirements than the application really has.
- A Steam Deck rating is Valve's review on the Steam Deck. It is entered for
  Windows-only games and says nothing about other hardware.
- Editions were entered where a reference shows them. An entry is not a
  complete list of an application's editions.

## Validation

`Catalogue::load` reads every `.toml` file of a directory and validates the
result. `README.md` and dot-files are ignored; any other file or a
subdirectory is an error. Every problem is reported, with its file and code.

| Rule | Code |
|---|---|
| The file is TOML, has no unknown field, and every value has the right type and is in its enumeration | `-002` |
| `schema_version` is 1; the ID is well formed; the file is named `<id>.toml` | `-003` |
| Text is not empty, has no control character and no surrounding space | `-003` |
| Every URL is `https`, with a lower-case DNS host, no user name or password, printable ASCII only | `-003` |
| `download_page` is not a direct download | `-003` |
| Every date is a real day written `YYYY-MM-DD`, not before 2000 and not after today | `-003` |
| An edition carries the identifiers of its kind and no field of another kind | `-003` |
| An edition, a requirement or a store is not listed twice | `-003` |
| Application IDs are unique | `-004` |
| No name or alias reads the same as another, in the entry or across entries, ignoring case, punctuation and spacing | `-005` |
| The honesty rule (above) | `-006` |
| A record names a route that can run its edition | `-007` |
| Sizes: 256 KiB per file, 16 editions, 32 aliases, 32 records and 32 references per edition, text lengths | `-008` |

The test `the_seed_catalogue_loads_and_validates` loads the real `catalogue/`
directory, so a bad entry breaks CI. The schema file is kept in step with the
Rust types by two tests that compare field names, required fields and every
enumeration (`the_schema_and_the_rust_types_name_the_same_fields`,
`the_schema_and_the_rust_types_list_the_same_values`). The entries are not run
through a JSON Schema validator in CI; the Rust validator is the authority.

## Search

`search(entries, query)` works on the loaded catalogue only. Nothing is
fetched.

Names are compared through a key: letters and digits, in lower case, with
everything else removed. `Counter-Strike 2`, `counter strike 2` and
`counterstrike2` share a key. Each name and alias of an entry is scored
against the query, and the entry takes its best score:

| Match | Score |
|---|---|
| The keys are equal | 1000 |
| The name's key starts with the query's key | 800 to 899; a shorter name scores higher |
| Every word of the query starts a word of the name | 700 |
| The query's key appears inside the name's key (three characters or more) | 600 |
| The keys differ by a few typing mistakes | 400 for one, 300 for two |
| The words match, some with typing mistakes | 350, less 50 for each further mistake |

A name scores 5 more than an alias, so a name wins a tie. A typing mistake is
one inserted, deleted or replaced character, or two neighbours swapped. None
is forgiven in a word of up to three characters, one up to six, two beyond.
Equal scores are ordered by name, then by ID, so the same catalogue and query
always give the same list. At most 20 hits are returned.

## Routing

```
route(request, host_facts, preferences) -> Vec<RouteOption>
```

**Inputs.** The request is the catalogue entry and whether the user asks for
the Windows guest to be offered. Host facts are stated by the caller, not
detected: architecture (`x86_64`, `aarch64`, `other`); whether Flatpak, a
container tool for other distributions, Wine, Proton through `umu-launcher`
and the Android runtime are present; whether the user has supplied ARM
translation and opted into Google Play; whether a Windows guest is set up and
a Windows licence is present; whether Darling is enabled; the GPU class
(`none`, `integrated`, `discrete`, `unknown`); and memory. Preferences are:
never use a virtual machine; prefer open source; and a pinned route per
application.

**Output.** A list that is never empty. Each option carries the route; the
edition and where it is obtained; the rating and its source; an outlook; a
plain-language reason; the caveats the user must see before choosing; and
what must be installed or enabled first. Exactly one option is marked
`recommended`.

### Reason codes, for translation

Issue #41 asked for stable identifiers beside the English, so that the
application centre can show the router's words in another language
(desktop-experience.md, 8.10 and 13.2). The additions are additive: every
English field is still there, unchanged.

| Field | Content |
|---|---|
| `reason_code` (option) | What the first sentence of `reason` says: why the option is listed |
| `reason_parts` (option) | `reason` sentence by sentence: `{code, params}` each. A demoted Linux edition adds `listed-after-better-route`; the no-route statement adds `route-blocked` per blocked route and `macos-no-darling`, `no-edition` |
| `text_id`, `params` (caveat) | The identifier of the caveat's sentence, and the values it was written with |
| `text_id` (alternative) | The identifier of the alternative's sentence |

Reason codes: `linux-flatpak`, `linux-native`, `linux-other-distribution`,
`windows-game-proton`, `windows-wine`, `guest-layer-rated` (`rating`),
`guest-known-obstacle`, `guest-asked`, `android`, `web-primary`, `web-pinned`,
`web-nothing-better`, `macos-darling`, `listed-after-better-route` (`route`,
`rating`, `better_rating`), `no-route` (`name`), `route-blocked` (`route`,
`caveat`: the blocking caveat's `text_id`, whose own `params` are on that
caveat in the statement's list), `macos-no-darling`, `no-edition`.

Caveat sentence identifiers are listed in `CAVEAT_TEXT_IDS`
(`src/route.rs`): one per requirement, route group and machine fact for the
requirement caveats (`requirement.kernel-anti-cheat.proton`,
`requirement.heavy-gpu.integrated`, …, none with parameters), and
`architecture.not-x86-64` (`arch`), `closed-store.windows-guest` and
`closed-store.not-integrated` (`store`), `community-package`,
`experimental.darling`, `guest-memory.too-little` (`needed_gb`,
`available_gb`), `guest-memory.enough` and `guest-memory.unknown`
(`needed_gb`), `reported-inadequate.external` (`rating`, `source`,
`source_rating`, `date`), `reported-inadequate.external-unquoted`,
`reported-inadequate.tur-test` (`rating`, `report`, `date`),
`reported-inadequate.unsourced` (`rating`), `guest-unsuitable-for-games`,
`guest-not-offered.no-virtual-machines`, `pin-unavailable` (`route`,
`because`: `no-virtual-machines`, `blocked` or `not-offered`). Alternative
identifiers: `ALTERNATIVE_TEXT_IDS`.

A parameter's value is one of the catalogue's or router's own words (`wine`,
`poor`, `microsoft-store`), a number, a date or a name: never English prose.
The rule that makes the identifiers worth having is enforced over the whole
input space of `tests/router_space.rs`: **one identifier with one set of
parameters is always the same English sentence**
(`sentences_carry_stable_identifiers`, applied to every case), every
identifier is a listed one, and the unit test
`every_requirement_sentence_has_one_listed_identifier` walks every
requirement, route and machine fact. The application centre's fixtures are
regenerated with the new fields; the centre ignores fields it does not read.

The **outlook** of an option is `clear` (nothing known stands in the way),
`doubtful` (a poor or broken rating, or a known obstacle that no rating
contradicts) or `blocked` (the route cannot work on this machine). `clear` is
not a promise that the application works; the rating and its source say what
is known.

### The ladder

Options are listed in this order.

| Rung | Route | Listed when |
|---|---|---|
| 1 | Flatpak, then native Linux | A Linux edition exists |
| 2 | Web application, primary | The web application is the publisher's primary form |
| 3 | Container of another distribution | The Linux edition is another distribution's package |
| 4 | Proton for a game; Wine for any other Windows application | A Windows edition exists |
| 4 | Windows guest, directly after its compatibility layer | See the decision table below |
| 5 | Android runtime | An Android edition exists |
| 6 | Web application, not primary | No route other than a guest or Darling has a clear outlook; or the user pinned it |
| 7 | Darling | A macOS edition exists and the user has enabled Darling |
| last | Any route that is blocked | Always listed, with the caveat that says why |

Then:

- A Linux edition whose record says `poor` or `broken` is moved to just after
  the first later route, other than Darling, whose record says `good` or
  `usable`. This is "a native Linux edition wins unless its record says it is
  clearly worse than another route".
- With "prefer open source", open-source editions come first within a rung.
  The preference never moves an option to another rung.
- If no listed route is `clear` or `doubtful`, the list starts with **no
  route**: the reason, which repeats each blocker, and the least-bad
  alternative.

**Recommendation.** The pinned route, when it is listed and not blocked;
otherwise the first `clear` option; otherwise the first `doubtful` one;
otherwise the no-route statement. A pin changes which option is recommended.
It never changes the order.

### Decision table

| Case | Result |
|---|---|
| A Linux edition exists | Listed first. Recommended unless its record is adverse and another route's record is good |
| Windows edition, not a game, Wine rated `good` or `usable`, or unrated with no known obstacle | Wine only |
| Windows edition, not a game, Wine rated `poor` or `broken` | Wine, then the Windows guest. The guest is recommended |
| Windows edition, not a game, Wine unrated, with a kernel driver, a dongle, hardware-attested copy protection or a closed store | Wine, marked doubtful, then the Windows guest |
| Windows edition, not a game, the user asks for the guest or pins it | Wine, then the Windows guest |
| Any of the three rows above, and the user never uses a virtual machine | Wine only, with a caveat that says a guest would have been offered and what is lost |
| Windows edition, game | Proton only. Never the guest: it has no usable GPU acceleration |
| Windows edition with kernel anti-cheat | **No route.** Alternative: dual boot. Proton (or Wine) is listed after it, blocked |
| Android edition | Android runtime |
| Android edition needing Play Integrity | **No route** unless another edition has one. The Android runtime is listed, blocked: "needs a phone certified by Google: it will refuse to run" |
| Android edition with ARM-only code | Blocked on a machine that is not ARM, unless the user supplied ARM translation: then a warning |
| macOS edition and another edition | The other edition's routes. Darling last, and only if enabled |
| macOS edition only, Darling not enabled | **No route.** Alternative: a Mac |
| macOS edition only, Darling enabled | Darling, doubtful, labelled experimental |
| Web edition, primary | Listed on rung 2, alongside the others |
| Web edition, not primary | Listed only when nothing better is clear, or when pinned |
| A route for x86-64 software (Proton, Wine, the guest, Darling) on another architecture | Blocked |
| Nothing listed can work, or the entry has no edition | **No route**, with the reason and the least-bad alternative. Never an empty list |

### Requirements on each route

`requirement_severity` gives the weight of a requirement on a route. A
**blocker** means the route cannot work. An **obstacle** means the route does
not meet the requirement and the application probably will not work fully; a
record that says `good` or `usable` outweighs an obstacle, except where the
obstacle comes from the machine. Every option carries a caveat for every
requirement of its edition.

| Requirement | Linux routes | Proton, Wine | Windows guest | Android runtime | Darling | Web |
|---|---|---|---|---|---|---|
| `kernel-anti-cheat` | warning | **blocker** | **blocker** | **blocker** | **blocker** | note |
| `hardware-attested-drm` | warning | obstacle | obstacle | obstacle | obstacle | warning |
| `play-integrity` | note | note | note | **blocker** | note | note |
| `arm-only-native-code`, on a machine that is not ARM | **blocker** | **blocker** | **blocker** | **blocker**; warning with ARM translation | **blocker** | note |
| `kernel-driver` | warning | obstacle | note | **blocker** | **blocker** | note |
| `hardware-dongle` | note | obstacle | warning | obstacle | obstacle | note |
| `heavy-gpu` | by GPU class | by GPU class | obstacle | by GPU class | by GPU class | by GPU class |
| `online-account` | note | note | note | note | note | note |

"By GPU class": an obstacle with no GPU, a warning with integrated graphics, a
note otherwise. On an ARM machine `arm-only-native-code` is a note.

Whatever blocks the Windows guest also blocks both compatibility layers. A
guest can therefore never be usable while its layer is blocked, which is what
keeps the layer in front of it.

Other caveats: the memory the guest needs ("about 16 GB", ADR-0007's
indicative figure, compared with the machine's memory when it is known); the
experimental status of Darling; a closed store; a package maintained by
someone other than the publisher; a record that says `poor` or `broken`, with
its source; and a pin that could not be honoured.

Prerequisites name what is missing: Flatpak, a container tool, Wine,
`umu-launcher` with a Proton build (which the user's machine fetches: TUR
does not ship Proton), the Android runtime, Google Play when the catalogue
lists the package there only, the Windows guest, a Windows licence.

### Invariants and their tests

The invariants are asserted over an enumerated input space in
`services/tur-catalogue/tests/router_space.rs`. Five tests walk the space:

| Test | Space | Cases |
|---|---|---|
| `windows_edition_space` | Game or not; all 256 requirement sets; six ratings of the layer; 24 hosts; 12 preference sets | 442 368 |
| `android_edition_space` | 256 requirement sets; two stores; three ratings; 64 hosts; three preference sets | 294 912 |
| `macos_edition_space` | 256 requirement sets; two sources; three ratings; 12 hosts; pinned or not | 36 864 |
| `linux_and_web_edition_space` | Five edition templates; 256 requirement sets; three ratings; three hosts; three preference sets | 34 560 |
| `mixed_edition_space` | All 127 combinations of seven editions; web primary or not; game or not; no requirement or one of the eight on every edition; four rating patterns; three hosts; eight preference sets | 438 912 |

Every case is checked against every invariant:

| Invariant | Check | Shown to fail on a bad result by |
|---|---|---|
| The result is never empty; "no route" is stated exactly when nothing listed can work, first, with a reason that repeats each blocker and with an alternative | `never_empty_and_no_route_is_explicit` | `an_empty_result_is_detected`, `a_silent_best_guess_is_detected` |
| A guest never precedes a compatibility layer for the same edition, and is never listed without it | `guest_never_precedes_its_layer` | `a_guest_before_its_layer_is_detected` |
| A forbidden route never appears: no guest when virtual machines are forbidden or for a game; no Darling unless enabled; Proton only for games, Wine only for the rest | `forbidden_routes_never_appear` | `a_forbidden_route_is_detected` |
| Every option carries a caveat for every requirement of its edition, with the severity of the table; an option is blocked exactly when it carries a blocker | `requirements_carry_their_caveats` | `a_missing_blocking_caveat_is_detected` |
| Exactly one option is recommended; never a blocked one; the pinned one when it is usable; an unused pin is explained | `one_sound_recommendation` | |
| Blocked options come last; no option is listed twice | `blocked_last_and_no_duplicates` | |
| A Linux edition leads unless its record is adverse; nothing overtakes one without an adverse record against it and a supporting record for the other | `linux_leads_unless_adverse` | |
| The guest is offered only for a stated reason; what is lost is stated only when a guest is withheld by preference | `guest_only_for_a_reason` | |

The blocking cells of the requirement table are written out a second time, by
hand, in `the_blocking_cells_of_the_requirement_table`.

Each routing rule also has a named unit test in
`services/tur-catalogue/src/route.rs`:

| Rule | Test |
|---|---|
| A native Linux edition wins | `a_native_linux_edition_wins_when_one_exists` |
| ... unless its record says it is clearly worse | `a_native_linux_edition_loses_when_its_record_says_it_is_clearly_worse`, `a_native_linux_edition_still_wins_when_it_is_not_clearly_worse` |
| The compatibility layer comes before the guest | `the_guest_is_offered_after_wine_when_wine_is_rated_poor_or_broken`, `a_demoted_linux_edition_does_not_split_a_layer_from_its_guest` |
| The guest is offered only when the layer is poor, unrated with a known obstacle, or asked for | `a_windows_application_gets_wine_and_no_guest_when_nothing_is_known_against_wine`, `the_guest_is_offered_when_wine_is_unrated_and_has_a_known_obstacle`, `the_guest_is_offered_when_the_user_asks` |
| Never when virtual machines are forbidden; what is lost is said | `the_guest_is_never_offered_when_virtual_machines_are_forbidden_and_the_loss_is_stated` |
| Games: Proton first, never the guest | `a_game_gets_proton_first_and_never_the_guest` |
| Kernel anti-cheat: no route, dual boot stated | `a_game_with_kernel_anti_cheat_has_no_route_and_dual_boot_is_stated` |
| Android: the runtime, with the Play Integrity and ARM-only caveats | `an_android_application_runs_in_the_android_runtime_with_its_caveats` |
| macOS only: other editions first; Darling only if enabled, experimental | `a_macos_application_prefers_its_windows_linux_or_web_edition`, `a_macos_only_application_has_no_route_unless_darling_is_enabled` |
| Web when nothing better exists, or alongside when primary | `a_web_edition_is_offered_when_nothing_better_exists`, `a_primary_web_edition_is_offered_alongside` |
| Nothing workable: an explicit no route | `nothing_workable_gives_an_explicit_no_route_never_an_empty_list` |
| The router never launches or installs anything | `the_router_source_has_no_process_file_or_network_access` |

### Where ADR-0007 is silent, and what was chosen

| Question | Choice |
|---|---|
| "The user can override" the first route, and "a compatibility layer always comes before a guest": which wins when the user pins the guest? | Both hold. The order never changes; the pinned guest is the recommended option |
| Is the layer still listed when it is rated `broken`? | Yes, first, marked doubtful, with the report. The guest after it is recommended |
| "Flatpak or native": which first? | Flatpak, because it leaves the immutable base untouched |
| Where does a web edition rank? The ladder does not mention the web | Primary: after native Linux, before everything else. Not primary: only when nothing better is clear, after the Android runtime |
| Is the Windows guest "better" than a web edition? | Not for deciding whether to list the web edition: the guest costs a Windows licence and memory. Both are listed |
| What does "prefer open source" change? | The order within a rung only |
| A game pinned to Wine, or another application pinned to Proton | Not honoured; the option list says so. Proton is for games and Wine for the rest |
| "Unknown with a known blocker" | The layer is unrated and carries an obstacle the guest would get round: a kernel driver, a dongle, hardware-attested copy protection, a closed store. Heavy GPU use is not one: the guest has no usable GPU acceleration either |
| Kernel anti-cheat in something that is not a game | Same answer: no route, dual boot |
| Hardware-attested copy protection | An obstacle, not a blocker: such software often still runs at reduced quality |
| A Linux edition rated poor against Darling rated good | Darling stays last. It is experimental |
| The 16 GB figure for the guest | Used as a caveat. Below about 15 GB of installed memory the guest is marked doubtful. ADR-0007 calls the figure indicative, to be measured |
| Architectures other than x86-64 | Proton, Wine, the guest and Darling are blocked. Linux, Android and web editions are not checked for the architecture |
| The Android "then" column is empty | Where an Android edition is blocked, the alternative is a certified phone or tablet |

## Relation to `tur-runtime`

| | `tur-catalogue` | `tur-runtime` |
|---|---|---|
| The user has | The name of an application | A file |
| Input | A catalogue entry | The file's bytes |
| Knows | Editions, requirements, reported ratings | Format and architecture |
| Answers | Which routes, in which order, with which caveats | Which runtime can host this file |
| Says "no" as | A no-route option with an alternative | `unsupported`, with a reason code |

ADR-0007 makes the catalogue the first source and inspection the fallback for
software the catalogue does not know. The two do not call each other yet.
`tur-catalogue` does **not** depend on `tur-runtime`: none of its types fits
without loss (its `Runtime` has no Proton, Flatpak, web or container route,
and its `HostFacts` has no GPU, memory, licence or Flatpak fact), and a
dependency would tie the two lockfiles together for no shared code.

The route names line up with the application profile's `runtime` values
(`schemas/app-profile.schema.json`), so that a later change can draft a
profile from a chosen route:

| Route | Profile `runtime` |
|---|---|
| `flatpak`, `native` | `native` |
| `proton` | `proton` |
| `wine` | `wine` |
| `windows-guest` | `windows-vm` |
| `android-runtime` | `android` |
| `darling` | `darling` |
| `distro-container`, `web` | None yet |

`tur-runtime` never recommends Proton, because that needs the application
database. This crate is that database.

## Command line

```
tur-catalogue [--catalogue DIR] validate [dir] [--repository DIR]
tur-catalogue [--catalogue DIR] search <query> [--json]
tur-catalogue [--catalogue DIR] show <id> [--json]
tur-catalogue [--catalogue DIR] route <id> [--host <facts.json>] [--prefs <prefs.json>] [--offer-guest] [--json]
```

`--catalogue` defaults to `catalogue` in the current directory. Without
`--host`, `route` assumes an x86-64 machine with Flatpak and nothing
optional, and says so. A field left out of the host facts takes the same
default.

```json
{ "arch": "x86_64", "flatpak": true, "distro_containers": false, "wine": true,
  "proton": true, "android_runtime": false, "arm_translation": false,
  "google_play": false, "windows_guest": false, "windows_licence": false,
  "darling_enabled": false, "gpu": "discrete", "memory_mib": 32768 }
```

```json
{ "never_use_virtual_machine": true, "prefer_open_source": false,
  "pinned_routes": { "com.adobe.photoshop": "wine" } }
```

| Exit status | Meaning |
|---|---|
| 0 | The catalogue is valid; the search found something; a route exists and needs nothing first |
| 1 | The catalogue, or a host-facts or preferences file, could not be read |
| 2 | The command line is wrong |
| 3 | `route`: no route runs the application on this machine |
| 4 | `route`: the recommended route needs something installed or enabled first |
| 5 | The catalogue is not valid |
| 6 | No application has that ID, or nothing matches the search |

Errors are written to standard error as `error[CODE]: message`. With `--json`
an error is a JSON object on standard output.

## Error codes

Every code uses `DESKTOP` with a `CATALOGUE` topic. `tur-core` now has a
`CATALOGUE` family, registered for these codes, and they have not moved to it:
`tur-catalogue show` and `route` print a code in their JSON output, the
application centre displays `TUR-DESKTOP-CATALOGUE-011`, and its recorded
fixtures, which `tests/desktop_fixtures.rs` compares with this program's
output byte for byte, contain the strings. They move in one change that covers
this table, the fixtures and the application centre together. The codes are
one table in `services/tur-catalogue/src/codes.rs`, with tests that they are
valid and unique and that none has moved.

| Code | Condition |
|---|---|
| `TUR-DESKTOP-CATALOGUE-001` | The catalogue directory, or a file in it, could not be read |
| `TUR-DESKTOP-CATALOGUE-002` | An entry is not valid TOML, has an unknown field or a value of the wrong type |
| `TUR-DESKTOP-CATALOGUE-003` | A field holds a value that is not allowed |
| `TUR-DESKTOP-CATALOGUE-004` | Two entries share an application ID |
| `TUR-DESKTOP-CATALOGUE-005` | A name or alias is used more than once |
| `TUR-DESKTOP-CATALOGUE-006` | A rating has no admissible source |
| `TUR-DESKTOP-CATALOGUE-007` | A record names a route its edition cannot use |
| `TUR-DESKTOP-CATALOGUE-008` | A size limit is exceeded |
| `TUR-DESKTOP-CATALOGUE-009` | No application has this ID |
| `TUR-DESKTOP-CATALOGUE-010` | Nothing matches the search |
| `TUR-DESKTOP-CATALOGUE-011` | No route runs the application on this machine |
| `TUR-DESKTOP-CATALOGUE-012` | A host-facts or preferences file is not valid |

These are not yet registry entries in the sense of §29.3.

## Not built

- **Store adapters.** This crate talks to no store. `tur-apps` installs from
  Flatpak remotes and from pinned publisher downloads
  ([app-install-backend.md](app-install-backend.md)); Steam, Google Play
  and the rest are not integrated.
- **Installation and launching** in this crate. `tur-apps` acts on the
  Flatpak and Wine routes; no container, guest or Android image is created
  by anything.
- **The application centre's service.** No D-Bus service. The library is
  linked by this command-line tool and by `tur-apps`.
- **Live updates of the catalogue.** The catalogue is the directory in this
  repository. There is no update channel, signature or merge of local
  additions.
- **A rating from TUR's own tests.** Four records are TUR's own
  (`reported_by = "tur-test"`, report `docs/reports/windows-runtime-2.md`):
  the Wine route of 7-Zip, Notepad++, PuTTY and VLC. All four are `unknown`:
  VLC because playback was not tried; the other three because the
  application centre's Home view says, unconditionally, that TUR has tested
  nothing and every rating is a named source's
  (`the_home_view_s_standing_sentence_is_still_true` guards it). A rating
  from TUR waits for that sentence to become conditional (a change to
  `tur-centre`). Every rating in the catalogue is still an external report.
- **Host detection.** Host facts are stated by the caller. The detection in
  `tur-runtime` is not used.
- **Fallback to inspection.** An application that is not in the catalogue is
  reported as unknown; the hand-off to `tur-runtime` is not written.
- **Per-architecture editions.** An entry does not say which processor
  architectures a Linux, Android or web edition supports, beyond
  `arm-only-native-code`.
- **Security policy** (§7.4) is not an input.
- **JSON Schema validation in CI.** The schema is kept in step with the Rust
  types by tests; entries are not run through a schema validator.
- **Translations of the router's sentences.** The identifiers and parameters
  are there (see [Reason codes](#reason-codes-for-translation)); the
  translated sentences belong to the application centre.

The Rust workflow lists `catalogue/**` in its path filters, so a change to
the entries alone is validated by CI. (An earlier version of this note said
it did not; that was out of date.)

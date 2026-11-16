# The TUR application catalogue

One file per application. Each file says which editions of the application
exist, what each edition requires, and what is known about how well each route
runs it. The router (`services/tur-catalogue`) reads these files to order the
routes for an application; see
[catalogue and routing](../docs/architecture/catalogue-and-routing.md).

The entries are facts about software, not code. They are checked by
`tur-catalogue validate` and by a test that loads this directory, so an entry
that breaks a rule breaks CI.

## The honesty rule

**TUR's own ratings are those of one experiment.** Forty records rate the
`rehost` route `starts`: the Windows and macOS packages of 20 open-source
Electron applications that TUR's re-hosting experiment
(`docs/reports/exp-rehost-1.md`, section 3.1) measured reaching their own
first window on the Linux build of Electron; nothing beyond that window
and typed text was tested, so none says `usable`. Each names its report and the
run that holds the evidence. TUR's first automated tests of Windows
applications (`docs/reports/windows-runtime-2.md`) are recorded as
`reported_by = "tur-test"` with the rating `unknown` (see
`docs/architecture/catalogue-and-routing.md`, "Not built"). Every other
rating is a named source's. A rating may be entered only

- as `unknown`, or by leaving the record out, which means the same; or
- attributed to a **named public source that the author opened and read**,
  with its URL, the date it was checked and the source's own rating in its own
  words. Such a record is `reported_by = "external"`. It says what the source
  reports. It never says that TUR tested anything; or
- from **TUR's own test**, `reported_by = "tur-test"`, with its report.

A record with `reported_by = "tur-test"` must give `test_report`, the path of
the test report in this repository, and the report must exist. Without it the
entry is refused (`TUR-DESKTOP-CATALOGUE-006`).

### Records from TUR's compatibility autopilot

`tur-runtime app autopilot` tries known remedies for a Windows application
that fails, in throwaway copies of its prefix, and writes what made it pass
as a record in this format
([compat-autopilot.md](../docs/architecture/compat-autopilot.md)):

```toml
[[editions.records]]
route = "wine"
rating = "unknown"
reported_by = "tur-autopilot"
version_tested = "1.0"
date = "2026-11-13"
remedies = ["dll-builtin:d3dx9_43", "windows-version-win11"]
runtime_version = "wine-9.0"
evidence_sha256 = "<64 hexadecimal digits>"
notes = "..."
```

Such a record states settings, never a rating: its rating must be `unknown`,
its route `wine` or `proton`, and it must name 1 to 16 `remedies` (as
`tur-runtime`'s remedy catalogue names them, with a DLL after a colon where
the remedy takes one), the `runtime_version` it was measured with and the
`evidence_sha256` of its result. `test_report` is optional: a record a person
shares from their own machine has none; one TUR measured in CI names its
report. The router never rates a route from it; it shows the settings beside
the route (`settings` in `tur-catalogue route --json`). The three fields are
refused on any other kind of record (`TUR-DESKTOP-CATALOGUE-003`), and a
rating on a `tur-autopilot` record is refused as unsourced
(`TUR-DESKTOP-CATALOGUE-006`).

### Records of a re-host

The `rehost` route runs the publisher's Windows or macOS package on the
official Linux build of the runtime it is made with (Electron, NW.js, .NET,
the JVM, Godot, LÖVE, Ren'Py, frozen Python), through TUR's `tur-rehost`.
Its record carries what was measured:

```toml
[[editions.records]]
route = "rehost"
rating = "starts"
reported_by = "tur-test"
test_report = "docs/reports/exp-rehost-1.md"
version_tested = "1.143.17232"
date = "2026-11-16"
notes = "..."

[editions.records.rehost]
package = "VSCodium-win32-x64-1.143.17232.zip"
package_sha256 = "<64 hexadecimal digits>"
package_size = 231406156
runtime = "electron"
runtime_version = "43.7.7"
evidence = "https://github.com/giorgiogiudice/TUR/actions/runs/43778332673"
native_builds = ["@parcel/watcher@2.5.6"]
unresolved = ["microsoft-authentication@0.0.1"]
```

`package` is the file's name, without a directory; `runtime` is one of
`electron`, `nwjs`, `dotnet`, `jvm`, `godot`, `love`, `renpy`,
`pyinstaller`; `evidence` is the `https` address of the run.
`native_builds` lists the native modules that had to be built from source,
`unresolved` those with no Linux build that the measured version did not
load when it started; each item is `name@version` (the name may be scoped,
`@scope/name`), and an unresolved module whose package cannot be identified
is written `file:<file name>`, such as `file:node.napi.node`. At most 64 in
each list. The table is required on a `rehost` record and refused on any
other (`TUR-DESKTOP-CATALOGUE-003`). Only TUR's own test can rate the route:
an external source cannot have measured TUR's re-host
(`TUR-DESKTOP-CATALOGUE-006`). The router offers the route only when the
record says `good`, `usable` or `starts`.

An entry with every route unknown is a good entry. An invented rating is not.
The same holds for requirements and editions: enter what a reference shows,
and say in `notes` what could not be checked.

## Writing an entry

- The file is named `<id>.toml`. The ID is lower case, reverse-DNS style, and
  never changes. A class of applications that share an answer (for example
  banking applications for Android) uses `entry_kind = "class"` and an ID
  under `tur.class.`.
- The format is `schemas/app-catalogue-entry.schema.json`. Unknown fields are
  refused. Dates are quoted strings, `"YYYY-MM-DD"`.
- A Windows edition of **free software** obtained from the publisher's page
  may pin the publisher's installer under `[editions.installer]`: its `url`
  (the publisher's own location, ending in the file's name), `sha256`,
  `size`, `version`, `licence`, `licence_url`, `hash_source` (the
  publisher's checksum file or release digest the SHA-256 was taken from) and
  `checked`, and `portable = true` for a single program. Take every value from
  the publisher, never compute a digest of a file you downloaded yourself and
  call it the publisher's. The edition must say `licence = "open-source"`:
  TUR never downloads proprietary software, and the validator refuses a pin
  on any other edition. `tur-apps` downloads a pinned file when the user asks
  for the application, and runs nothing in it before size and digest match
  ([ADR-0019](../docs/adr/0019-app-install-backend.md)).
- Every URL is `https`. `download_page` is a page for a person to read, never
  a direct link to an installer or archive: TUR does not fetch a binary the
  user did not ask for.
- `aliases` are other names people type. No name or alias may read the same
  as one of another application, ignoring case, punctuation and spacing.
- List an edition only if a reference shows that it exists. Give the reference
  under `[[editions.references]]` with the date it was checked.
- `requirements` are the known facts that decide routing: `kernel-anti-cheat`,
  `hardware-attested-drm`, `play-integrity`, `arm-only-native-code`,
  `kernel-driver`, `hardware-dongle`, `heavy-gpu`, `online-account`. Enter one
  only when a reference supports it.
- A compatibility record names a route that can run its edition: `flatpak` for
  a Flatpak; `native` or `distro-container` for a native Linux edition;
  `proton`, `wine`, `windows-guest` or `rehost` for a Windows edition;
  `android-runtime`; `web`; `darling` or `rehost` for a macOS edition.

## The rating scale and how sources map onto it

| Catalogue rating | Meaning | ProtonDB tier | WineHQ AppDB rating | Valve Steam Deck category | AreWeAntiCheatYet status |
|---|---|---|---|---|---|
| `good` | Runs as intended, with no workaround worth mentioning | platinum | Platinum | Verified | |
| `usable` | Runs and is usable; needs tweaks or has minor problems | gold, silver | Gold, Silver | Playable | |
| `starts` | Starts and shows its first window, measured by TUR; nothing beyond that was tested | (none) | (none) | (none) | (none) |
| `poor` | Runs, with problems that get in the way of normal use | bronze | Bronze | | |
| `broken` | Does not start, or cannot be used for its purpose | borked | Garbage | Unsupported | Denied, Broken |
| `unknown` | Not known | no tier | no test result | Unknown | |

The mapping is TUR's reading of each source's own definitions, which were read
for WineHQ and for Valve. ProtonDB's page of tier definitions is drawn by a
script and could not be read on the day the seed was written; its tiers are
mapped by their order (platinum, gold, silver, bronze, borked). The source's
own word is always kept in `source_rating`, so the mapping can be checked and
changed. AreWeAntiCheatYet's statuses "Supported" and "Running" describe the
anti-cheat, not the whole game, and are used as references only.

`starts` is TUR's own word for what TUR itself saw: the application started
and showed its first window, and nothing beyond that was tested. No
external source's scale maps onto it, not even a source's "it starts": a
record that says `starts` must be `reported_by = "tur-test"`, with its
report (`TUR-DESKTOP-CATALOGUE-006`).

A rating from one of these sources describes what that source's contributors
saw on their machines, with their versions. It is evidence, not a promise.

## Sources used for the seed entries

The 20 re-hosted applications (Bruno, draw.io, balenaEtcher, Freelens,
Heroic Games Launcher, Jitsi Meet, Joplin, Logseq, LosslessCut, MarkText,
Mattermost, Min, MQTTX, Podman Desktop, Rocket.Chat, Standard Notes, Tabby,
Trilium Notes, VSCodium, Zulip) were entered on 16 November 2026 from
TUR's re-hosting experiment and each application's release page on GitHub.
Their Linux edition, the publisher's own, has no record: nothing is claimed
about it.

The other entries were all checked on 12 November 2026.

| Source | What it was used for |
|---|---|
| ProtonDB (`https://www.protondb.com/`), summary tier per Steam application | Ratings of the `proton` route |
| Valve Steam Deck compatibility, as shown on the Steam store (`https://store.steampowered.com/`); category definitions at `https://www.steamdeck.com/en/verified` | Ratings of the `proton` route for Windows-only games |
| WineHQ Application Database (`https://appdb.winehq.org/`); rating definitions at `https://gitlab.winehq.org/winehq/appdb/-/wikis/Rating-Definitions` | Ratings of the `wine` route |
| AreWeAntiCheatYet (`https://areweanticheatyet.com/`) | Anti-cheat status; ratings of the `proton` route for games whose status is Denied |
| Steam store listings and the store's public application API | Steam application IDs, publishers, platforms |
| Flathub (`https://flathub.org/`) listings and its verification status | Flatpak application IDs, licences, whether the publisher maintains the package |
| Fedora Packages (`https://packages.fedoraproject.org/`) | Fedora package names |
| Google Play listings and F-Droid (`https://f-droid.org/`) | Android package names and stores |
| Each publisher's own pages, named in the entry | Editions, download pages, requirements |

Adobe's own pages refused automated requests on that day; the Photoshop entry
says so and carries no fact that depends on them except the address of the
product page.

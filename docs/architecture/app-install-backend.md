# The application install backend (`tur-apps`)

Design note, as built. Code: `services/tur-apps/`. Decision:
[ADR-0019](../adr/0019-app-install-backend.md). Builds on
[ADR-0007](../adr/0007-app-centric-routing.md) (the router),
[ADR-0011](../adr/0011-application-centre-and-stores.md) (one application
centre), [catalogue-and-routing.md](catalogue-and-routing.md) and
[runtime-manager.md](runtime-manager.md). The application centre's design is
[desktop-experience.md](desktop-experience.md), chapter 8; this component is
what task D-11 ("`tur-centre` 2: install") waits for.

## Scope

`tur-apps` is one standalone program that takes a catalogue identifier and
installs, updates, lists and removes the application by the route the
router chose:

- **Flatpak**: a Linux application from a Flatpak remote (Flathub, or
  Fedora's own registry), installed **per user** through the `flatpak`
  command line;
- **Windows** (the `wine` route): the publisher's installer, downloaded only
  where the catalogue pins its address, size and SHA-256 for freely
  redistributable software, or a file the user supplies; checked before
  anything in it runs; handed to `tur-runtime app install`, which is used as
  it is.

Every other route is refused before anything is done, with a stable code and
a plain sentence: the Windows guest, the Android runtime, Darling, games
through Proton, distribution packages, containers of other distributions
and web applications.

Not built here: the D-Bus service (the interface is proposed, below), the
application centre's screens, and installing the launcher entry of a Windows
application (still staged by `tur-runtime`).

## The flow

```
tur-apps plan|install <id>
  │
  ├─ catalogue + router  (tur_catalogue::route, linked as a library)
  │     no route ──────────────────────────► TUR-DESKTOP-APPS-003, the router's own reason
  │     route not built ───────────────────► TUR-DESKTOP-APPS-004, and the built routes it also offers
  │     a prerequisite missing (router) ───► TUR-DESKTOP-APPS-009, the router's own words
  │
  ├─ flatpak route: is the remote switched on for this user? ── no ─► TUR-DESKTOP-APPS-008
  │     remote-info: metadata (permissions), commit; Flathub's API: sizes
  │
  ├─ wine route: a file given? ── yes ─► hash it; must equal the pin when there is one ─► else 006
  │                              no ──► a pin on an open-source edition? ── no ─► TUR-DESKTOP-APPS-005
  │
  ├─ the plan, shown (text, --json, or the first event)
  ├─ confirmation (--yes, or a question on a terminal) ── none ─► TUR-DESKTOP-APPS-013
  │
  ├─ flatpak: flatpak install --user --noninteractive <remote> <ref>
  │     what it added is recorded; a failure takes it back
  ├─ wine: curl → file.part → size + SHA-256 → rename → tur-runtime app install
  │     a mismatch deletes the file; tur-runtime never runs ─► TUR-DESKTOP-APPS-006
  └─ the record: $XDG_DATA_HOME/tur/apps/installed/<id>.json
```

The router is linked as a library (`tur-catalogue` is a path dependency), so
the plan carries the router's own option, not a copy of its rules: the plan's
`router` field is exactly an element of `tur-catalogue route --json`'s
`options`, and a refusal for "no route" repeats the router's reason word for
word. A test compares the two (`no_route_is_refused_with_the_router_s_own_reason`),
and the apps workflow compares `tur-apps` with the `tur-catalogue` binary.

### Host facts

Without `--host`, `tur-apps` looks at three facts and leaves the rest at the
router's defaults: the architecture, whether `flatpak` is installed, and
whether Wine is (`/usr/bin/wine`). The plan says which (`host.how`:
`detected` or `stated`). Host detection proper is still the router's open
item (issue #28).

## The plan

`schemas/app-install-plan.schema.json`. What it says, before anything is
done:

| Field | Content |
|---|---|
| `application`, `route`, `chosen_by` | The application; `flatpak` or `wine`; `router` or `you` (with `--route`) |
| `router` | The router's option: reason, `reason_code`, `reason_parts`, rating and its source, caveats with `text_id` and `params`, prerequisites |
| `host` | The facts the router was given, and how |
| `source` | Where it comes from, and **how it is verified**: Flatpak's check against the remote's signing key; the SHA-256 the catalogue pins (with where the digest was taken from and when); or, for a user's file with no pin, the sentence that TUR cannot check it and records its digest |
| `downloads` | Each download: what, from where, its size and installed size in bytes when known (and from where the number came), the SHA-256 or the Flatpak commit |
| `installs` | Where it goes |
| `access` | What it may reach, in the centre's words (files, devices, network, display, sound, services); for a Flatpak read from its metadata, for a Windows application from `tur-runtime`'s sandbox |
| `removal` | The command, what removal does, where the user's data is, and `data_kept_by_default: true` |
| `steps` | `download`, `verify`, `install`, `record`, each with the command it runs |
| `notes` | Anything else: an offline plan, extensions whose size is not known in advance |

`--offline` asks no remote: no size, no commit, no permissions
(`access_known: false`). The assistant's tool uses it.

## The Flatpak route

Everything is per user (`--user`): nothing needs administrator rights and
nothing is installed for other users. Facts this relies on were read from
flatpak's source at 1.18.4 (Fedora 45 ships `1.19.0^really1.18.4`) and
1.14.6 (Ubuntu 24.04, where CI runs):

- Without a terminal, `list`, `remotes` and `remote-ls` print tab-separated
  columns with no header, and nothing for no rows
  (`app/flatpak-table-printer.c`). `--json` exists only from 1.17 and is not
  used.
- `--noninteractive` asks nothing and prints one line per operation
  (`Installing <ref>`, `Updating <ref>`, `Uninstalling <ref>`) and failures
  as `Error: Failed to …` on standard error
  (`app/flatpak-quiet-transaction.c`). **There is no machine-readable
  progress in the command line**: the events carry these lines as
  `activity`, and the centre shows the activity, not a bar
  (desktop-experience.md 8.5: "no invented percentages").
- Sizes in `list` and `remote-info` are written for people, by the locale;
  only `info --show-size` prints bytes. Every run gets `LC_ALL=C.UTF-8`.
  Sizes for the plan come from Flathub's API (`/api/v2/summary/<id>`, bytes),
  fetched through the same audited `curl` module.
- Exit status 1 on every failure.

| Operation | Command |
|---|---|
| Plan | `flatpak remotes --user --show-disabled --columns=name,url,options`; `flatpak remote-info --user --show-metadata <remote> <ref>` (permissions); `remote-info --show-commit`; `flatpak list --user --columns=ref` (is the runtime there) |
| Install | `flatpak install --user --noninteractive <remote> app/<id>/<arch>/stable`; the references before and after are compared, and what was added is recorded |
| Update | `flatpak update --user --noninteractive <ref>` |
| Update check | `flatpak remote-ls --user --updates --app --columns=application,version,origin` |
| List | `flatpak list --user --app --columns=application,version,branch,origin,runtime`; `flatpak info --user --show-size <ref>` |
| Remove | `flatpak uninstall --user --noninteractive [--delete-data] <ref>`, then the runtimes and extensions this installation added that no remaining application uses |
| Start | `flatpak run --user <id>` |

A failed installation uninstalls whatever it had added (a runtime pulled in
before the application failed). Removal checks afterwards that none of the
added references is still installed (unless another application uses it),
that the deployment and the exported launcher entry are gone, and with
`--delete-data` that `~/.var/app/<id>` is gone.

### Sources

`tur-apps sources` lists the user's remotes and the two TUR knows;
`tur-apps sources enable flathub|fedora` adds or re-enables one for this
user, `disable` switches it off (what was installed stays).

| Source | Added with | Verified upstream |
|---|---|---|
| Flathub | `flatpak remote-add --user --if-not-exists --title=Flathub flathub https://dl.flathub.org/repo/flathub.flatpakrepo` | The command on `https://flathub.org/setup`; the `.flatpakrepo` carries Flathub's signing key |
| Fedora Flatpaks | `… --title=Fedora Flatpaks fedora oci+https://registry.fedoraproject.org` | The location Fedora's own `flatpak-add-fedora-repos.service` adds |

**What the Fedora 45 Kinoite base configures** (read from Fedora's package
sources and the atomic-desktops configuration, branch f45; see
[the report](../reports/apps-1.md#verified-upstream)): the `fedora` remote
(enabled) and `fedora-testing` (disabled), both **system-wide**, added at
first boot by `flatpak-add-fedora-repos.service`. **Flathub is not
configured** until the user opts into third-party repositories
(`fedora-third-party`, offered on Kinoite by Plasma Welcome); it is then a
system remote, unfiltered in effect.

**What TUR's default should be.** The design's Sources view shows Flathub
"On" (desktop-experience.md 8.6), and ADR-0011 makes only *proprietary*
stores opt-in. A per-user installation needs the remote in the user's own
installation; a system remote does not serve `--user`. So: Flathub should be
switched on for each user at first login, by the image or the first-run
setup, as `tur-apps sources enable flathub` does, and shown in Sources with
a switch. This slice does not do that (it does not touch `image/`); until it
is done, installing a Flathub application is refused with
TUR-DESKTOP-APPS-008 and the command that switches Flathub on.

## The Windows route

### Where the installer comes from

The catalogue gains one optional field on a Windows edition,
`[editions.installer]` (schema `app-catalogue-entry.schema.json`, Rust
`tur_catalogue::Installer`): the publisher's own download location, its
SHA-256, size, version, licence and where the licence was read, where the
digest was taken from, and the day it was checked. The validator refuses it
unless the edition is obtained from the publisher's page **and** marked
`open-source`: TUR never downloads proprietary software, and an entry for a
paid or proprietary application only ever takes the user's file.

Filled in this change for the four applications measured in
`tests/windows-runtime/apps.json`, reusing those pins: 7-Zip 26.06,
Notepad++ 8.9.9.1, PuTTY 0.85 and VLC 3.0.24.

| Case | What happens |
|---|---|
| A pin, no file given | Downloaded by `curl` to `<name>.part` in a new directory `/var/tmp/tur-apps-<id>-<process>`, mode 0700, never an existing one; size and SHA-256 checked; only then renamed; the directory is deleted after installing, and on any failure |
| A pin, and the user's file | The file is hashed in the plan; it must be the pinned file, or TUR-DESKTOP-APPS-006; hashed again just before it is used |
| No pin, the user's file | "The same flow from the hash check on": the digest is shown in the plan and recorded; the plan says plainly that it cannot be checked against the publisher |
| No pin, no file | TUR-DESKTOP-APPS-005, with the publisher's page or the store and the command to give a file |

### The one place that downloads

`src/download.rs` is the only module that uses the network, and it does so
through the system's `curl`, never a linked HTTP client (ADR-0019 says why).
Every run is:

```
curl -q --proto =https --proto-redir =https --location --max-redirs 10 --fail
     --no-progress-meter --connect-timeout 30 --retry 2 --max-time 3600
     --max-filesize <pinned size> --output <dir>/.<name>.part
     --write-out '%{url_effective}\n%{http_code}\n' -- <pinned URL>
```

Why `/var/tmp`: `tur-runtime` refuses to take a program from a hidden folder
of the home directory (`TUR-SECURITY-SANDBOX-006`: "it is in a hidden part of
the home directory, where credentials and application profiles are kept"),
which rules out `~/.cache`, as the first CI run showed; it protects the
user's runtime directory; and `/tmp` is held in memory on Fedora.
`--download-dir` names another root.

`-q` first, so no `~/.curlrc` is read; `https` on the first request and on
every redirect; the URL after `--`. Only an `Installer` from the catalogue
can be passed. A test asserts that no other source file builds a download or
uses `std::net`.

### Installing

`tur-runtime app install <file> --app-id <catalogue id> --name <catalogue
name> --json [--portable]`. The catalogue's identifier is used as
`tur-runtime`'s, so one identifier names the application everywhere (both
use the same grammar). `tur-runtime` rolls a failed installation back
itself; `tur-apps` records nothing then.

### Removing, and the user's documents

`tur-runtime app uninstall` deletes the application's private home with the
user's documents in it, without asking (issue #32). `tur-apps remove` never
calls it before the question about the data is answered:

- **keep** (the default, with `--keep-data`, on a terminal the answer to
  "Keep its settings and documents? [Y/n]", and without a terminal when no
  flag is given): the private home and the prefix's Windows profiles
  (`drive_c/users`: `AppData` and the documents folders) are **copied** to
  `$XDG_DATA_HOME/tur/apps/kept/<id>/<UTC time>/home` and
  `…/windows-profiles` first. Symbolic links are not followed (a prefix
  links folders to places outside it) and are listed as skipped. If the
  copy fails, nothing is removed. The prefix's backups are kept, as
  `tur-runtime` keeps them.
- **delete** (`--delete-data`): nothing is copied, and the backups are
  deleted too (`--remove-backups`).

Then `tur-runtime app uninstall`, which checks its own paths, and
`tur-apps` checks again that the application's directory and its launcher
directory are gone. Anything left is TUR-DESKTOP-APPS-015 with the list.

Updating a Windows application is not built: `tur-runtime` has no update
(TUR-DESKTOP-APPS-004 says so and how to move to a new version). The update
*check* is: an application installed from a pinned download has an update
when the catalogue now pins another digest.

## One list

`tur-apps list [--check-updates] [--json]` lists, for this user, every
application `tur-apps` installed, and also the per-user Flatpaks and the
`tur-runtime` applications installed without it (`installed_by`: `tur-apps`,
`flatpak`, `tur-runtime`). For each: identifier (the catalogue's when it
knows the application), name, route, where it came from, version, size in
bytes, where its data is, `installed` or `missing`, and the update state
(`checked`, `available`, `version`, from where). System-wide Flatpaks are not
listed in this slice.

## Events

With `--events`, `install`, `update` and `remove` write one JSON object per
line (`schemas/app-install-event.schema.json`): `plan`, `step-started`,
`step-finished`, `activity`, `progress` (`done`, and `total` only when
known), and last exactly one `result` or `error`. Without it, the same as
text. The download reports progress about twice a second with the pinned
total; Flatpak reports activity lines only.

## The service

`services/tur-apps/dbus/org.tur.Apps1.xml`, proposed, not frozen and **not
implemented**. Session bus (everything is per user); one method per
subcommand, returning the same JSON documents, with jobs reporting through
an `Event` signal; `Cancel` belongs to the service alone. `Remove` requires
`data` = `keep` or `delete`: over D-Bus there is no default, because the
centre must ask (desktop-experience.md 8.5). The table that ties methods to
subcommands is `DBUS_METHODS` in `src/cli.rs`; `tests/interfaces.rs` holds
the XML, that table and the command line together.

The design document names `org.tur.Catalogue1` for the catalogue service and
puts store adapters behind it (8.10). Installing is a different authority
(it changes the computer, class 2 for the assistant) from searching (class
0), so this note proposes a separate `org.tur.Apps1`; the catalogue service
can remain read-only.

## How D-11 will use it

| Centre (desktop-experience.md) | `tur-apps` |
|---|---|
| "How it will run", the primary button (8.4) | `plan --json`: `route`, `router`, and a refusal's code decide "Install", "Set up…" (009 with the router's prerequisites), "Open the publisher's page" / "Choose the installer…" (005), nothing (003) |
| "What the application can reach" (8.4) | `access` |
| Progress replaces the button (8.5) | `install --events`: `progress` with `total` for a bar with numbers; `activity` otherwise |
| "You get this application from its publisher" (8.5) | 005's message; then `install --installer FILE` |
| Remove, with the one question (8.5) | `remove --keep-data` or `--delete-data`, from the user's answer; never without |
| Installed, Updates (8.1) | `list --check-updates --json` |
| Sources (8.6) | `sources --json`, `sources enable|disable` |
| Translation (13.2) | Codes (below), and the router's `reason_code`, `reason_parts`, `text_id`, `params` |

Until the service exists the centre can run the command line with `--json`
and `--events`, as it runs `tur-catalogue` today.

## The assistant

`agent-tools.json` and `agent-tools.adapter.json`, at the component's top level:
`apps_list_installed` (`list --json`) and `apps_plan` (`plan --offline
--json`), both class 0. Installing, updating, removing and switching sources
are class 2 (ai-control-layer.md) and are **not** tools in this slice.
`tests/agent_tools.rs` loads both files with P@x's loader, holds each command
line against the built program and runs the tools.

They were first under `pax/`, while P@x's end-to-end test listed the
components that ship tools; since pax-5 the tests discover tool sets and the
files are at the top of the component. The image installs `/usr/bin/tur-apps`
and the tools in `/usr/share/tur/pax/tools/tur-apps/` (report TUR-010).

## Command line

```
tur-apps [--catalogue DIR] [--host FILE] [--prefs FILE] [--download-dir DIR]
         [--flatpak-program P] [--curl-program P] [--tur-runtime-program P] [--wine-program P]
tur-apps plan <id> [--route flatpak|wine] [--installer FILE] [--offline] [--json]
tur-apps install <id> [--route R] [--installer FILE] [--yes] [--events]
tur-apps list [--check-updates] [--json]
tur-apps update <id> [--events]
tur-apps remove <id> [--keep-data | --delete-data] [--events]
tur-apps launch <id>
tur-apps sources [enable NAME | disable NAME] [--json]
```

`--catalogue` defaults to `/usr/share/tur/catalogue`. Programs are looked for
in `/usr/bin` (and `tur-runtime` in `/usr/local/bin`), not in `PATH`.

| Exit status | Meaning |
|---|---|
| 0 | Success. `launch`: the application's own status |
| 1 | An input could not be read or is not valid |
| 2 | The command line is wrong |
| 3 | Refused before anything was changed |
| 5 | A file is not the one expected; nothing in it ran |
| 6 | No such application, or not installed by `tur-apps` |
| 7 | A step failed while the operation was carried out |
| 8 | A removal left something behind |

## Error codes

`TUR-DESKTOP-APPS-*`, in the desktop family like the catalogue's
(`TUR-DESKTOP-CATALOGUE-*`). One table, `src/codes.rs`, tested to be valid
and unique.

| Code | Condition | Exit |
|---|---|---|
| `TUR-DESKTOP-APPS-001` | Something this needs could not be read (catalogue, file, home) | 1 |
| `TUR-DESKTOP-APPS-002` | No application has this identifier | 6 |
| `TUR-DESKTOP-APPS-003` | No route runs it on this computer (the router's reason) | 3 |
| `TUR-DESKTOP-APPS-004` | Installing by this route is not built yet | 3 |
| `TUR-DESKTOP-APPS-005` | TUR does not download it: supply the installer | 3 |
| `TUR-DESKTOP-APPS-006` | A file is not the one expected (size or SHA-256); nothing in it ran | 5 |
| `TUR-DESKTOP-APPS-007` | A download did not complete | 7 |
| `TUR-DESKTOP-APPS-008` | The source is not switched on | 3 |
| `TUR-DESKTOP-APPS-009` | A program or prerequisite is missing | 3 |
| `TUR-DESKTOP-APPS-010` | A step failed | 7 |
| `TUR-DESKTOP-APPS-011` | Not installed by `tur-apps` | 6 |
| `TUR-DESKTOP-APPS-012` | Already installed | 3 |
| `TUR-DESKTOP-APPS-013` | Nothing was done without confirmation | 3 |
| `TUR-DESKTOP-APPS-014` | The route asked for is not offered here | 3 |
| `TUR-DESKTOP-APPS-015` | Something of the application is still there after removal | 8 |
| `TUR-DESKTOP-APPS-016` | A record is not valid | 1 |
| `TUR-DESKTOP-APPS-017` | An input is not valid | 1 |

These are not yet registry entries in the sense of §29.3.

## Tests

| What | Test |
|---|---|
| Every flow against stand-ins for `flatpak`, `curl` and `tur-runtime`: plan, install, list, update, remove keeping and deleting data; what is run, in which order, what is left | `tests/cli.rs` |
| A no-route refusal repeats the router's reason | `no_route_is_refused_with_the_router_s_own_reason` |
| A corrupted or short download is refused and `tur-runtime` never runs | `a_corrupted_download_is_refused_before_anything_runs`; `download::tests::a_download_that_is_not_the_pinned_file_is_deleted` |
| Proprietary software is never downloaded | `proprietary_software_is_never_downloaded_and_takes_your_file`; the validator's `a_proprietary_application_never_carries_an_installer_to_download` |
| A user's file is checked from the hash on | `a_file_you_give_is_checked_from_the_hash_on` |
| Removal keeps documents by default and leaves nothing | `a_windows_application_is_downloaded_checked_installed_and_removed_keeping_documents`, `a_windows_application_removed_with_its_data_leaves_nothing`, `a_flatpak_is_planned_installed_listed_and_removed_keeping_its_data` |
| A failed Flatpak installation takes back what it added | `a_failed_flatpak_installation_takes_back_what_it_added` |
| The D-Bus XML, the command line, the schemas, this note and the codes agree; one module downloads | `tests/interfaces.rs` |
| The assistant's tools | `tests/agent_tools.rs` |
| The real programs | the `apps` workflow, `services/tur-apps/ci/e2e.py`; results in [apps-1](../reports/apps-1.md) |

## Not built

- The D-Bus service; cancelling a running job.
- Installing a Windows application's launcher entry (staged by
  `tur-runtime`); updating a Windows application.
- Flathub switched on by default for each user (needs `image/` or the
  first-run setup).
- System-wide Flatpaks in the list; Flatpak bundles and `.flatpakref` files
  ("I have a file", D-12).
- Every other route (guest, Android, Proton, Darling, packages, containers,
  web).
- A check of the remote's signing key beyond what `flatpak` does.

<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Self-healing

Design note for bet 5 of [breakthroughs.md](../breakthroughs.md), "a system
that heals itself", first slice. Decision: [ADR-0027](../adr/0027-self-healing.md)
(Proposed). Builds on [diagnostics.md](diagnostics.md) and
[problem-reports.md](problem-reports.md) (ADR-0024), [publishing.md](publishing.md)
(ADR-0023), [boot-health-and-rollback.md](boot-health-and-rollback.md)
(ADR-0014), [agent-policy.md](agent-policy.md) and
[ai-control-layer.md](ai-control-layer.md) §5, and follows the pattern of the
[compatibility autopilot](compat-autopilot.md): a sourced, declarative
catalogue of remedies, tried safely.

- Code: `services/tur-diagnostics/src/heal/` (library `tur_diag::heal`),
  `src/heal_cli.rs` (`tur-diag heal ...`).
- Catalogue: `services/tur-diagnostics/heal/catalogue.json`, compiled in;
  schema `schemas/heal-catalogue.schema.json`.
- Units: `tur-heal.service`, `tur-heal.timer`, `tur-heal-request.socket`,
  `tur-heal-request@.service`; in the session `tur-heal-notify.path` and
  `.service`; `tmpfiles.d/tur-heal.conf` (all in `image/rootfs/`).
- The image's list of fixed codes: `/usr/share/tur/heal/fixes.json`, schema
  `schemas/heal-fixes.schema.json`.
- P@x: four tools in `services/tur-diagnostics/agent-tools.json`; the
  `request` adapter kind in `services/tur-pax/src/adapter.rs`.
- Proof: `.github/workflows/heal.yml` (booted guest, fault injection),
  `tests/pax/scenarios/20-heal-request.json` (P@x end to end), the unit tests
  in `src/heal/tests.rs`; report [heal-1](../reports/heal-1.md).

## 1. The loop

```text
 tur-heal.timer ─▶ tur-heal.service (root, no P@x)
                     tur-diag heal scan --repair
                       for each catalogue entry:
                         check ─ present? ── no ─▶ was it repaired before?  ─▶ "held"
                           │                       did an update name it?  ─▶ "fixed by the update"
                           │                       otherwise               ─▶ "cleared"
                           yes
                           ├─ automatic remedy (class ≤1, reversible, ≤N a boot) ─▶ apply ─▶ check again
                           │        passes ─▶ "repaired" (and "held" at the next scan)
                           │        fails  ─▶ "not repaired" ─▶ report offered if the entry says so
                           ├─ remedies for a person ─▶ notice: what was found, the command or P@x
                           └─ no remedy, cause in TUR ─▶ report offered
                     state: /var/log/tur/heal/state.json (readable by anyone, nothing personal)

 person / P@x ── tur-heal-request.socket ─▶ tur-diag heal handle (root, one per connection)
                     apply or undo a class 0/1 remedy of the catalogue, only while its fault is present
 person (sudo) ── tur-diag heal apply CODE REMEDY --yes ─▶ class 2 remedies, after confirmation
 person ── tur-diag heal report CODE ─▶ prepared report, shown in full ─▶ tur-diag report send (ADR-0024)
 update ── /usr/share/tur/heal/fixes.json names CODE ─▶ next scan checks again ─▶ "fixed" or "happened again"
```

Detection, the known remedy, verification and the offer of a report work
without P@x. P@x adds explanation in conversation and can ask for the same
remedies through the same socket, under the policy engine (section 5).

## 2. Where it runs

In `tur-diag`, which is already in the image (`image/programs.tsv`), as a
subcommand: no new program, no new crate. The service is a oneshot unit
started by a timer five minutes after boot and every thirty minutes, after
boot health has judged the boot. Nothing in the boot waits for it, boot
health does not count it, and it never touches boot, the bootloader,
firmware, disks or encryption: no catalogue entry can express that (section
3), and remedies of class 3 are refused by the validator.

Why a timer and not a resident service: a scan is a few short commands
(`systemctl`, `timedatectl`, `df`, `coredumpctl`, a link read); a resident
process would hold memory for nothing. Faults that need a faster answer
(audio after resume, a crash at login) are a follow-up: a path or event unit
can start the same scan.

## 3. The fault catalogue

Data, compiled into the program, in the format of
`schemas/heal-catalogue.schema.json`. Each entry has:

| Field | What it is |
|---|---|
| `code` | A stable TUR error code in the family the fault belongs to (`TUR-NETWORK-…`, `TUR-STORAGE-…`); never `TUR-DIAG-HEAL-*`, which are tur-diag's own errors |
| `title`, `explanation` | What is wrong and what it does, in plain words |
| `component` | The problem-report component |
| `check` | One of a fixed set of check kinds, with parameters. The repair is verified by running the same check again, once a second for at most `verify_seconds` |
| `remedies` | Each one of a fixed set of remedy kinds, with `class` (0–2), `automatic`, `max_per_boot`, `reversible`, and `changes` and `undo` in plain words |
| `report` | `never` (the cause is not TUR's), `when-unrepaired`, or `always` (the cause is in TUR and nothing is safe to try), with `report_kind` |
| `notice` | The texts the person is shown: `found`, `repaired`, `unrepaired`, `report` |
| `sources` | Where each check and remedy was verified upstream, with the date it was read |

**Kinds.** A check or a remedy is a kind implemented in Rust, not a command
line: no entry can name a program, a script, a URL or a download.

| Check kind | What it does |
|---|---|
| `unit-failed` | `systemctl is-failed` on a fixed list of units |
| `network-time-off` | `timedatectl show --property=NTP --property=CanNTP`: `NTP=no` while `CanNTP=yes` |
| `resolver-not-managed` | the resolver service is active and `/etc/resolv.conf` is neither a link to one of its files nor a file naming only its stub |
| `free-space-low` | `df --output=avail,size -B1` under a byte and a percent limit |
| `tur-program-crashed` | `coredumpctl list --json=short --since=@<boot>`, programs under `/usr/bin/tur-` or `/usr/libexec/tur/` |

| Remedy kind | What it does | Undo |
|---|---|---|
| `restart-units` | `systemctl reset-failed` and `start` for each failed unit the check found | nothing to undo: no setting changes |
| `set-network-time` | `timedatectl set-ntp true` | `timedatectl set-ntp false` |
| `restore-resolver-link` | keeps a copy of `/etc/resolv.conf` (root only, `/var/lib/tur/heal/backups/`), then points it at `../run/systemd/resolve/stub-resolv.conf` in one rename, and restores its SELinux label | puts the copy back, content and mode |
| `vacuum-journal` | `journalctl --rotate --vacuum-size=…` | none: class 2 |

**Rules** (`Catalogue::validate`, held over the shipped catalogue by a test):
codes parse and are unique; a remedy answers its check's kind; class 3 is
never a catalogue remedy; a class 0 or 1 remedy is reversible and an
irreversible one is class 2; only a reversible class 0/1 remedy may be
automatic, at most one per fault; a fault without a remedy must offer a
report; every entry has a source.

**The first slice's entries.**

| Code | Fault | Remedy | Class | Automatic | Report |
|---|---|---|---|---|---|
| `TUR-DESKTOP-SERVICE-001` | a watched service failed (chronyd, NetworkManager, systemd-resolved, bluetooth, cups, avahi-daemon, upower, power-profiles-daemon, udisks2, ModemManager) | `start-failed-service` | 1 | yes, once a boot | when unrepaired |
| `TUR-NETWORK-TIME-001` | network time switched off | `enable-network-time` | 1 | no: the person may have switched it off | never |
| `TUR-NETWORK-RESOLVER-001` | `/etc/resolv.conf` replaced (a VPN client, a script) | `restore-resolver-link` | 1 | no: it may be deliberate | never |
| `TUR-STORAGE-SPACE-001` | under 1 GiB or 2 % free for `/var` | `vacuum-journal` | 2 | no | never |
| `TUR-DIAG-CRASH-001` | a TUR program crashed in this boot | none | — | — | always (`crash`) |

"Automatic" is a property of the entry, not a global switch: only a repair
that changes no setting the person may have chosen is applied without
asking. Restarting a failed service changes no setting; switching network
time back on or replacing the resolver file would override what the person
may have done on purpose, so they wait for the person.

## 4. How the person is told

Every change of a fault's state is an event in the state file. The person's
notices are the events of kinds `detected`, `repaired`, `not-repaired`,
`report-offered`, `fixed-by-update`, `recurred-after-update` and `undone`,
with the catalogue's text and the commands that act on them:

| Event | Text (example) | Actions offered |
|---|---|---|
| detected | "Setting the clock from the network is switched off. If you did not mean this, TUR can switch it back on." | `tur-diag heal apply TUR-NETWORK-TIME-001 enable-network-time` |
| repaired | "chronyd.service had stopped working. TUR started it again and checked that it is running." | `tur-diag heal undo …` for a remedy a person applied |
| not-repaired | "The system disk is still almost full. Old logs were not enough: remove files or applications you no longer need." | |
| report-offered | "tur-diag crashed. This is a problem in TUR itself. You can send a report to TUR's developers; you see everything in it first, and nothing is sent unless you choose to." | `tur-diag heal report TUR-DIAG-CRASH-001` |
| fixed-by-update | "The problem you reported is fixed: "A TUR program crashed" has not happened again since TUR was updated to 0.2.0, which addresses it." | |
| recurred-after-update | "TUR 0.2.0 was meant to fix "A TUR program crashed", but it happened again. You can send a new report so that TUR's developers know." | `tur-diag heal report …` |

`tur-diag heal notices [--json] [--mark-shown]` lists the ones this person
has not seen (kept in `$XDG_STATE_HOME/tur/heal/shown.json`). In a desktop
session `tur-heal-notify.path` watches the state file and
`tur-heal-notify.service` runs `tur-diag heal notify`, which shows each as a
desktop notification through `gdbus` and the standard notification service.
Settings and P@x's window are the later home of these notices; the commands
are the interface they will call.

## 5. The socket, and P@x

`tur-heal-request.socket` listens on `/run/tur/heal.socket` (mode 0666).
For each connection systemd starts `tur-heal-request@.service`, which runs
`tur-diag heal handle --connection %i` as root with the connection as its
standard input and output. The instance name ends with the peer's UID
(systemd's `instance_from_socket`, read 2026-11-13), which is recorded with
what was done.

A request is one line of JSON:
`{"schema_version":1,"tool":"heal_apply_remedy"|"heal_undo_remedy","arguments":{"code":…,"remedy":…}}`,
nothing more (unknown fields are refused). The answer is one JSON object,
`{"ok":true,"result":{…}}` or `{"ok":false,"error":{"code","message","detail"}}`.

**What the socket can do is no more than the service does by itself**:

- only remedies of class 0 or 1, which are reversible; class 2 is refused
  with `TUR-DIAG-HEAL-003` before anything is checked;
- only while the fault's check finds the fault at that moment
  (`TUR-DIAG-HEAL-002` otherwise, nothing changed), so a request cannot
  change a working system;
- at most three times a boot for one remedy (`TUR-DIAG-HEAL-005`);
- undo only of something applied and not yet undone.

So any account on the computer may connect: it can cause only what the
service would do on its own at its next scan, or undo it.

**P@x.** Four tools, in `tur-diagnostics`' manifest:

| Tool | Class | Performed by |
|---|---|---|
| `heal_list_faults` | 0 | `tur-diag heal status --json`, in the read-only sandbox (it reads `/var/log/tur/heal/state.json`, which the sandbox shows) |
| `heal_explain` | 0 | `tur-diag heal explain --json -- CODE`, in the sandbox |
| `heal_apply_remedy` | 1 | a `request` adapter: the request above, to `/run/tur/heal.socket` |
| `heal_undo_remedy` | 1 | the same |

The `request` adapter kind is new in `tur-pax` (ADR-0027). The read-only
sandbox cannot change anything, and that stays true: a request is not a
program and is not run in it. After the policy engine has allowed the call
by the class its manifest declares, `tur-pax` itself connects to the socket
the adapter names (which must be under `/run/tur/`), sends the tool's name
and its validated input and nothing else, and returns the service's answer,
bounded, filtered and framed like a program's output. The call succeeded when
the answer says `"ok": true`. The service decides and performs. P@x performs
nothing above class 1: the class 2 remedy is refused by the service, and no
tool can send a report (ADR-0024).

## 6. State and files

| Path | What | Mode |
|---|---|---|
| `/var/log/tur/heal/state.json` | every fault seen (state, first and last seen, build and version, subjects, evidence, every remedy applied with who, the account, the outcome and its undo, whether a report was offered, how it was resolved), and the last 200 events | 0644 |
| `/var/lib/tur/heal/backups/a<N>.json` | the copy of a file a remedy replaced | 0600, directory 0700 |
| `/var/lib/tur/heal/lock` | taken by every scan and request (`flock`) | 0600 |
| `$XDG_STATE_HOME/tur/heal/shown.json` | which notices this person has seen | the person's |

The state is a record of what happened and holds nothing personal: evidence
says "a file of its own naming 1 name server", never which; no file content,
no address. It lies under `/var`, which every deployment shares, so it
survives an update and a rollback.

## 7. The report, and the fix that comes back

**The report** is ADR-0024's, unchanged. `tur-diag heal report CODE`
prepares one for a fault TUR offers a report for: kind from the catalogue
(`crash`, `repair-failed`), the fault's code as the error code, the component
(for a crash, the crashed program's), a description with the explanation,
what the check saw and the repairs tried. It is shown in full and saved; it
is sent only by `tur-diag report send --to URL`, confirmed by the person,
every time. There is no standing consent: ADR-0024's "send automatically"
switch is still not built, and this design adds none.

**The link back.** Each image carries `/usr/share/tur/heal/fixes.json`, the
codes a change in it addresses. A pull request that fixes a fault the
service finds adds the code there (empty today). The receiver's
`mark-fixed --issue N --version V` already tells the person's
`tur-diag report status` that a report is fixed; the healing service closes
the loop on the computer itself:

- At each scan, a fault that is not resolved, was recorded under another
  build, and whose code the running image's list names, is checked again.
- Gone: resolved as `fixed-by-update`, and the person is told "The problem
  you reported is fixed …" when they sent a report with that code (their
  sent records, `tur-diag report list`), or "… has not happened again since
  TUR was updated to V" otherwise.
- Present: `recurred-after-update`, "TUR V was meant to fix …, but it
  happened again", and the report is offered again.

A crash is checked within the current boot, so after the update's restart
"not seen since" is what the check can say; the notice says no more than
that.

An update that makes things worse is not this design's to catch: boot
health's automatic rollback (ADR-0014) does that, before the healing service
ever runs.

## 8. Error codes

`TUR-DIAG-HEAL-001` unknown fault or remedy · `-002` the fault is not
present, nothing changed · `-003` class 2 remedy needs the person, as
administrator, not a request · `-004` not confirmed · `-005` tried too often
in this boot · `-006` the check could not be made · `-007` nothing to undo ·
`-008` no report offered · `-009` state or data directory unusable · `-010`
request not understood · `-011` the healing service did not answer. Full
entries in `src/error.rs`.

## 9. How it is proven

- **Unit tests** (`src/heal/tests.rs`, fake machine): every check and remedy
  kind and its undo; the validator's rules; a service restarted once a boot
  and a report offered when it fails again; a request refused for class 2,
  for a fault not present, for malformed input; the kept copy restored byte
  for byte and never in the shared state; the fix link both ways; the
  notices' texts.
- **P@x end to end** (`pax` workflow, scenario `heal-request`): the installed
  tools, real `tur-pax`, scripted provider; the service is `tur-diag heal
  handle` started for each connection as systemd would, on a resolver file
  in a scratch root.
- **Booted guest** (`heal` workflow, `tests/boot/heal.py`): the product image,
  its own units; real faults injected; see heal-1 for what each check proves.

## 10. Not done in this slice

- Desktop delivery of notices is written (`heal notify`) and not proven: the
  test guest has no graphical session.
- User-scope faults (a failed user service such as PipeWire, a broken user
  Flatpak remote) need a scan in the session; only system faults are checked.
- `tur-update` does not yet show, before an update, which of this computer's
  faults it addresses.
- The receiver does not know the codes it was told are fixed; a maintainer
  writes `fixes.json` by hand.
- A Settings page and P@x's window for notices; the "send automatically"
  switch of ADR-0024 (not wanted without its own ADR).
- More faults: a full home, a broken Flatpak remote, a stale lock, audio
  after resume, Wi-Fi power saving. Each needs a check kind, a remedy kind,
  a source and a fault-injection test.

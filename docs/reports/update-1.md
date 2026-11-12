# update-1 — The update manager, first slice: model, policy and plans

```text
TUR DEVELOPMENT REPORT

Task ID:          update-1
Subsystem:        Update manager (specification 15.1; org.tur.Update1)
Build ID:         not applicable: this task builds no image
Source revision:  c59b45d824db (feat/tur-update, pull request 44), which is
                  the commit the measurements below come from. The commit
                  that adds this report follows it and changes no code: one
                  comment line in src/lib.rs that points here.

SUMMARY
- What changed: a new component, services/tur-update, with the binary
  tur-update. It reads the state of the system from bootc status, decides
  what an update manager should do next, and prints the plan: the exact
  commands and their effects. It runs one command, bootc status
  --format=json, and only when asked about the machine it runs on. It
  stages, releases, rolls back and pins nothing.
  1. A typed model of bootc status --format=json for bootc 1.17.1, the
     version in the base image, read from its source and held against
     real output.
  2. The policy, as pure functions: may a check run; may the known update
     be staged, for the update manager by itself and for a person; may the
     system go back; may a version be kept or released. Every decision has
     a TUR-UPDATE-* code; 38 codes are registered.
  3. Plans: for a decision to proceed, the commands in order. Staging is
     three steps: download held, read back and compare, release.
  4. The command line: status, check, plan stage|rollback|pin|unpin,
     commands, codes; text and JSON; schemas/update-report.schema.json.
  5. Two read-only tools for P@x, tested with P@x's loader.
  6. A proposed org.tur.Update1 (introspection XML, not implemented), the
     design, and ADR-0017 (proposed).
  7. A workflow, update, that runs the base image as a container and holds
     the program against the image's real bootc and ostree.
- Why it changed: ADR-0014 decided how a new version is tried and undone,
  and left to "the updater" what is fetched and when. The desktop design
  (11.1) needs something to show.
- What it does not show: NOTHING HERE HAS UPDATED A SYSTEM. No image was
  downloaded, staged, released or rolled back by this component, in CI or
  anywhere. The plans are text. That they work when run is not shown.

FILES
- Added: services/tur-update/ (Cargo.toml, Cargo.lock; src: lib, main,
  bootc, state, health, conditions, policy, plan, probe, codes, cli;
  tests: policy, fixtures, cli, agent_tools, interfaces; tests/fixtures:
  24 JSON fixtures, provenance.json, bootc-upstream/LICENSE-MIT;
  pax/agent-tools.json, pax/agent-tools.adapter.json;
  dbus/org.tur.Update1.xml; ci/bootc_capture.py, ci/check_fixtures.py);
  schemas/update-report.schema.json; .github/workflows/update.yml;
  docs/architecture/update-manager.md; docs/adr/0017-update-manager.md;
  docs/reports/update-1.md
- Modified: docs/adr/README.md (one index line)
- Removed: none
- .github/workflows/rust.yml is not modified: it finds components by
  their Cargo.toml, and found this one.

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test --all-targets --all-features; cargo test --doc (workflow "rust",
  job services/tur-update); cargo build --locked (workflow "update")
- Result: see RUNS below
- Warnings: none from the compiler or clippy

TESTS
- Unit tests: 44, in the library. Pass.
- Integration tests: 65 in five files. Pass.
    tests/policy.rs      22  each rule of the policy table alone, for both
                             requesters; then every combination: 13
                             machines x 7 200 sets of conditions x 2
                             requesters = 187 200 staging decisions with
                             their plans, the check, the next step and the
                             rollback decision for each, held against the
                             promises of the policy. 1.2 to 2.4 s.
    tests/fixtures.rs    16  the model against the 24 fixtures
    tests/cli.rs         15  the built program on every recorded machine;
                             240 JSON documents validated against the
                             schema; text output; exit statuses
    tests/agent_tools.rs  5  the tool manifest and adapters, with P@x's
                             loader, against the built program
    tests/interfaces.rs   7  the D-Bus XML, the design note, the schema,
                             and the sources: one place starts a program
- Workflow "update", on a hosted Ubuntu 24.04 runner, with the pinned base
  image quay.io/fedora/fedora-kinoite@sha256:f857f6c5... run by podman:
  37 checks, and the 24 fixtures against their recorded sources. Pass.
  Detail below.
- Boot tests: NOT RUN. Nothing here is in the image or can affect boot.
- The plans, carried out: NOT RUN. No staging, release, rollback or pin.
- tur-update status --live on a booted TUR system: NOT RUN. It ran in the
  base image as a container, where bootc reports no booted deployment.
- tur-update status --live as an ordinary user on a booted system: NOT
  RUN. Tested with a stand-in for bootc.
- In P@x's sandbox, and the "pax" workflow: NOT RUN. The tool set is not
  where tur-pax looks for it (see "What is not done").
- Against a run of boot health on main: NOT RUN. Pull request 36 was
  merged (2876154) while this was in review. The boot-health fixtures are
  its output at commit c1667d98; the code that writes that output is the
  same in the merge, but no fixture was taken again from a run on main.
- Hardware tests, performance tests: NOT RUN.

DIAGNOSTICS
- Log paths: GitHub Actions runs below; artifact "tur-update-bootc" of the
  update workflow (everything the image's bootc and ostree printed, and
  what tur-update printed inside the image)
- Diagnostic bundle path: none
- Relevant error IDs: new: TUR-UPDATE-STATE-001 to -007, -CHECK-001,
  -NETWORK-001 and -002, -POWER-001 and -002, -SPACE-001, -HEALTH-001 to
  -006, -SETTINGS-001, -SESSION-001, -STAGE-001 to -007, -ROLLBACK-001 to
  -005, -PIN-001 to -005. The UPDATE family already existed in tur-core.

RECOVERY
- Rollback procedure: revert the merge. No image, no boot path and no
  stored data is touched; nothing is installed anywhere.
- Recovery tested: not applicable.

KNOWN LIMITATIONS
- See "What is not done" below, and sections 12 and 13 of
  docs/architecture/update-manager.md.

NEXT SAFE ACTION
- Review ADR-0017 and the interface of section 8 of the design. Then,
  on top of boot health as merged: put tur-update in the image, add
  tur-update status --live to the boot test, and add one rollback
  scenario that stages with bootc upgrade --download-only and
  --from-downloaded. That scenario is the proof ADR-0017 asks for before
  anything executes a plan.
```

## Runs

Every workflow on the measured commit is green. Since pull request 36 was
merged by then, the `rust` workflow ran on this branch merged with a main that
contains `services/tur-boot-health`.

| Commit | Workflow | Run | Result |
|---|---|---|---|
| `c59b45d` | rust (13 components; `services/tur-update` among them) | 43229877652 | pass |
| `c59b45d` | update | 43229877993 | pass |

Earlier runs on this branch, for the record of how it got here:

| Commit | Workflow | Run | Result | What it showed |
|---|---|---|---|---|
| `20acc2e` | rust | 43224766492 | fail | The library compiled, clippy was clean and its 43 unit tests passed at the first attempt; the job failed only because CI had to compute `Cargo.lock` and rustfmt's formatting, which it returned |
| `20acc2e` | update | 43224766510 | fail, as intended | The first capture of `bootc status` in the base image. It failed because the fixture it compares with did not exist yet; the fixture was then taken from this run's artifact |
| `86ac665` | rust | 43227534645 | fail | One test: the design note it reads was not written yet. The policy tests did not run in this job |
| `86ac665` | update | 43227534509 | pass | The live read in the image, and every command named, for the first time |
| `68be782` | rust | 43228350701 | pass | The policy tests ran for the first time, and passed |
| `68be782` | update | 43228350855 | pass | |
| `07c1184` | rust, update | 43229227148, 43229227168 | fail | Neither run started a job: both ended within a minute with no jobs at all, on a commit that changed no workflow file. The cause was not established; pull request 36 was merged into main about a minute later. The next commit, `c59b45d`, which adds only comments and documentation to that tree, is the measured one |

The `pax`, `boot`, `image`, `desktop` and `windows-runtime` workflows did not
run: no file they watch changed.

## What the update workflow measured

On a hosted runner, with the base image of `image/base.lock` pulled by digest
and run as a container (`--network=none`):

| Measurement | Value |
|---|---|
| bootc | 1.17.1 (`bootc-1.17.1-1.fc45.x86_64`) |
| OSTree | 2026.4 (`ostree-2026.4-1.fc45.x86_64`) |
| rpm-ostree, bootupd | 2026.3-2, 0.3.2-1 |
| Base image, unpacked | 7 433 626 776 bytes |
| `bootc status --format=json` in a container | Exit 0. `spec.image`, `status.booted`, `.staged`, `.rollback`, `.type`, `.usrOverlay` are `null`; `rollbackQueued` and `readOnly` are `false`. 255 bytes, members in sorted order |
| The same as uid 1000 | Exit 0, the same document |
| `--json`, `--format-version=1`, `--booted` | The same document |
| `--format-version=2` | Refused: "Unsupported format version: 2" |
| `--format=humanreadable` | "System is not deployed via bootc." |
| `bootc-fetch-apply-updates.timer` | `disabled` |
| `tur-update status --live` inside the image | Exit 1, `TUR-UPDATE-STATE-002`, nothing on standard output: the program ran the image's `/usr/bin/bootc status --format=json` and read its answer |
| `tur-update status --from` a recorded machine, inside the image | Exit 0, state `ready` |

The 37 checks: the bootc version is the one the fixtures were verified
against; the document is the committed fixture, byte for byte as JSON; the
seven commands `tur-update` names each have their program in the image, each
of their options is described by that program's `--help`, and `booted` and
`rollback` are named by the help of `ostree admin pin`; the four runs of
`tur-update` inside the image. Before them, `check_fixtures.py` confirmed that
all 24 fixtures are listed, that the ten captures have the recorded SHA-256,
and that the fourteen samples from bootc's source tree are what the
conversion gives for the file with the recorded git blob identifier at tag
`v1.17.1`.

## Where the fixtures come from

Not one was written by hand. `services/tur-update/tests/fixtures/provenance.json`
has the detail for each.

| Fixtures | Source |
|---|---|
| `bootc/tur-booted.json` | `boot` workflow, run 43186059647, main at `6a13a68b`: `bootc status --json` on the TUR test image after installation |
| `bootc/tur-staged.json`, `tur-after-fallback.json`, `tur-after-update.json` | `rollback` workflow of pull request 36, run 43207801019 at `c1667d98`: `bootc status --format=json` with a staged deployment, after three failed attempts and the fallback, and after a good update |
| `boot-health/*.json` (five) | The same run: `tur-boot-health status --json` at the same moments, one after a failed finalisation, and the state file `rejected.json` |
| `bootc/container-base-image.json` | `update` workflow, run 43224766510: the base image as a container |
| `bootc-upstream/*.json` (fourteen) | bootc at tag `v1.17.1`, `crates/lib/src/fixtures/`, converted from YAML by `ci/check_fixtures.py`. MIT OR Apache-2.0; `LICENSE-MIT` is beside them |

The rollback run is marked failed in GitHub. Its failures are in other
scenarios of pull request 36; the scenarios these files come from passed
(`check-fails`, `good`, `finalize-fails`), and the files are what bootc and
`tur-boot-health` printed in them.

## What is verified, and what is assumed

Section 3 of the design has the table, fact by fact, with the file read or
the run that showed it. In short:

- **Read from source and seen in real output:** the shape of the status
  document; that it is sorted JSON with `null` for absent entries; the options
  of `bootc upgrade` and `bootc rollback`; `ostree admin pin` and its
  operands.
- **Read from source only:** that `bootc status` needs root on a booted
  system; that a second `bootc rollback` swaps the order back; that `/etc`
  does not carry over on a rollback; that OSTree refuses to pin a staged
  deployment; that `cachedUpdate` is written by a check.
- **Assumed:** that the previous version is removed when a new one is
  applied; that an unknown amount of free space is safe to proceed on; that
  held staging followed by release behaves on TUR as bootc documents it.

## What is not done

1. **Nothing is executed.** The service, its state, scheduling, and the
   states of an operation in progress are the next slice.
2. **The tools are not offered to P@x.** `services/` is read as a directory
   of tool sets by `services/tur-pax/tests/tools.rs` and by `tests/pax/e2e.py`,
   and both assert the list of components that ship tools. Adding
   `services/tur-update/agent-tools.json` would have failed the `rust`
   workflow's `services/tur-pax` job on this pull request, and those files
   are not this task's to change. The two files are therefore in
   `services/tur-update/pax/`, complete and tested with P@x's loader, and one
   `git mv` from their place. Section 14 of the design lists the three edits
   that go with the move.
3. **Even then the tools can say little**: in P@x's sandbox `bootc status`
   cannot run as root, and the tools answer `TUR-UPDATE-STATE-004`. They
   become useful when `org.tur.Update1` publishes the state.
4. **Conditions are not measured.** Power, network, free space and the repair
   session are options of the command line.
5. **Signatures are not checked and no channel exists.** `status` reports how
   the image followed is verified; for the test image that is "none", and
   every decision about it carries a note saying so.
6. **A kept version that is neither in use nor the previous one** cannot be
   released or returned to.
7. **`tur-core` is unchanged.** The UPDATE family was there.

## What was found in existing designs

1. **Going back does not keep system settings.** The desktop design (11.1)
   says "Your files, applications and settings stay as they are" in the
   prompt for going back and in the fallback notice. bootc's manual says
   changes under `/etc` do not carry over to the version gone back to. A
   password changed, a user added or a system-wide network connection saved
   since the update would be gone. Boot health's own notice says it
   correctly. Text is proposed in section 14 of the design.
2. **Nothing removes an entry from boot health's rejected list**, though
   section 5.10 of that design says a person's request does.
3. **`status.otherDeployments` is left out when empty**, and bootc 1.17.1 has
   `status.readOnly` and `status.usrOverlay`, which section 3.3 of the
   boot-health design does not list.
4. **bootc's rollback message has no `MESSAGE_ID`** in the journal, though
   its manual names one. Boot health measured the same and records it
   (addendum C, 17.1). A later slice that wants to detect a rollback made
   behind its back cannot rely on it.
5. **`bootc status` needs root**, so neither Settings nor P@x can read the
   versions without the service. The desktop design assumes the service; it
   is worth saying that there is no fallback.

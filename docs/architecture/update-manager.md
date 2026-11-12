# Update manager

- **Component:** `services/tur-update` (library `tur_update`, binary `tur-update`)
- **Specification sections:** §5.3, §15.1, §15.4, §16 (`org.tur.Update1`), §27.5,
  §29.3; [boot health and rollback](boot-health-and-rollback.md) (addendum C,
  sections 5.2, 5.5, 5.9 to 5.11); [desktop experience](desktop-experience.md),
  sections 5.6 and 11.1; AI control layer §5 and §11;
  [ADR-0014](../adr/0014-boot-health-and-rollback.md),
  [ADR-0017](../adr/0017-update-manager.md) (Proposed)
- **Status:** first slice. The model, the policy and the plans. **Nothing in
  this component changes the system**: it runs one command, `bootc status
  --format=json`, and only when asked about the machine it runs on. No D-Bus
  service exists yet.

Everything described here is tested in CI. The model is tested against real
output of bootc 1.17.1: of the TUR test image booted in a virtual machine, of
the base image run as a container, and of bootc's own samples. No update has
been downloaded, staged, released or rolled back by this component, anywhere.
Section 12 lists what has not been done and what is recorded as NOT RUN.

## 1. What it is

TUR is updated by replacing the whole system image. bootc downloads a new
image, stages it as a second deployment beside the running one, and the next
restart starts it. [Boot health](boot-health-and-rollback.md) counts the
attempts of the new version and returns to the previous one if it does not
start. The update manager is what decides **when** an image is downloaded and
staged, **which** image is not staged, and what "go back" and "keep this
version" do. It is the owner of `org.tur.Update1` (§16).

This slice answers three questions with plain data, and acts on none of them:

1. What is on this computer? (`status`)
2. What should the update manager do next, and why? (`check`)
3. What exactly would an operation run, and what would it do? (`plan`)

```text
bootc status --format=json          tur-boot-health status --json      options
        │ bootc::parse_status                │ health::parse_boot_health   │
        ▼                                    ▼                             ▼
   SystemState ───────────────────────► Conditions (power, network, space, …)
        │                                    │
        └──────────────┬─────────────────────┘
                       ▼
        policy: decide_check, decide_stage, decide_rollback, decide_pin, …
                       │  Decision: proceed | nothing | wait | refuse, with a code
                       ▼
        plan: the commands, in order, and their effects. Printed, never run.
```

| Module | What it holds |
|---|---|
| `bootc` | The document `bootc status --format=json` prints, as bootc 1.17.1 defines it |
| `state` | The typed model: deployments, the image followed, what the last check found |
| `health` | What `tur-boot-health status --json` says, as far as a decision needs it |
| `conditions` | Power, network, free space, a repair session, the two settings, the thresholds |
| `policy` | The decisions, as pure functions |
| `plan` | The commands a decision leads to, and their effects; the table of every command named |
| `probe` | The one place a program is started: `bootc status --format=json` |
| `codes` | The `TUR-UPDATE-*` registry (§29.3) |
| `cli` | The command line |

Dependencies: `tur-core` (error codes), `serde`, `serde_json`, `clap`: the same
three third-party crates `tur-storage` uses, all MIT or Apache-2.0. `tur-pax`
is a development dependency of one test.

## 2. The model

### 2.1 What is read

`bootc status --format=json` prints one document, `apiVersion:
org.containers.bootc/v1`, `kind: BootcHost`. The types in `bootc.rs` mirror
bootc's own definitions (`crates/lib/src/spec.rs` at tag `v1.17.1`) member for
member, with the same rules for what may be absent, and nothing more lenient:
a document that lacks a member bootc always writes is not interpreted.

| Member | Type in bootc | Used for |
|---|---|---|
| `apiVersion`, `kind` | text | Refusing any other format (`TUR-UPDATE-STATE-003`). `v1alpha1` is accepted because bootc's own samples carry it; real output is `v1` |
| `spec.image` | reference, transport, optional `signature` | The image the system follows |
| `spec.bootOrder` | `default` or `rollback` | A queued rollback |
| `status.booted`, `.staged`, `.rollback` | boot entry or `null` | The version in use, the one waiting, the previous one |
| `status.otherDeployments` | list, left out when empty | Further deployments: kept ones |
| `status.rollbackQueued` | boolean | A queued rollback |
| `status.type` | `bootcHost` or `null` | Read, not used: `booted.image` says the same |
| `status.usrOverlay` | access mode and persistence, or `null` | Shown by `status` |
| `status.readOnly` | boolean | A system on read-only media (`TUR-UPDATE-STATE-006`) |
| entry `.image` | reference, `imageDigest`, `version`, `timestamp`, `architecture`; or `null` | The version. The digest is its identity |
| entry `.cachedUpdate` | an image, or `null` | What the last check found |
| entry `.incompatible` | boolean | Layered packages: bootc cannot update it |
| entry `.pinned` | boolean | A kept version |
| entry `.downloadOnly` | boolean | A staged version that is held |
| entry `.softRebootCapable` | boolean | Reported. No decision uses it: TUR does not use soft reboots for updates (boot health, section 5.5) |
| entry `.store`, `.ostree`, `.composefs` | backend state | Reported. TUR is on the OSTree backend |

A member this version does not know is passed over, as bootc does.

### 2.2 What it is made into

`SystemState` holds the image followed, the booted deployment (required: with
`status.booted` null there is no state, `TUR-UPDATE-STATE-002`), the staged
and rollback deployments, the others, and three facts: a rollback is queued,
the medium is read-only, an overlay is on `/usr`.

**What is known about an update** comes from `cachedUpdate` of the booted
deployment, which `bootc upgrade --check` writes:

| `cachedUpdate` | Meaning |
|---|---|
| `null` | No check on record: `not-checked` |
| The digest of the booted image | The last check found what is running: `up-to-date` |
| Another digest | An update is `available` |

**The one state the Updates page shows** (desktop design, 11.1), as far as the
deployments tell it:

| State | When | Sentence |
|---|---|---|
| `ready` | A deployment is staged and not held | "TUR 0.4.0 is ready. Restart to use it." |
| `held` | A deployment is staged in download-only mode | "TUR 0.4.0 has been downloaded and is waiting to be released." |
| `available` | An update is known and is not what is staged | "TUR 0.4.0 is available." |
| `up-to-date` | The last check found what is running | "TUR is up to date. Version 0.3.2." |
| `not-checked` | No check on record | "TUR has not checked for updates yet. Version 0.3.2." |

"Checking", "Downloading", "Verifying" and "Not applied" of the desktop's
table are states of an operation in progress or just ended. They belong to
the service that runs the operation, which does not exist yet. "Last checked
today at 09:14" needs the time of the last check, which the document does not
carry; the service will record it.

Text taken from the document (a version label, a reference) is made safe
before it is printed: control characters are replaced and it is cut at 200
characters.

## 3. What was verified about bootc and OSTree, and how

The versions are the ones in the pinned base image (`image/base.lock`,
`quay.io/fedora/fedora-kinoite` at digest `sha256:f857f6c5…`), read in CI by
the `update` workflow: **bootc 1.17.1** (`bootc-1.17.1-1.fc45`) and **OSTree
2026.4** (`ostree-2026.4-1.fc45`).

"Source" means the upstream file was fetched at that tag and read. "Image"
means the program in the base image printed it in CI. "Captured" means real
output of the TUR test image booted in a virtual machine. Nothing below was
observed while this component changed a system: it never has.

| Fact | How it is known |
|---|---|
| The shape of the status document (section 2.1) | Source: bootc `crates/lib/src/spec.rs`, `docs/src/host-v1.schema.json`. Captured: four documents (one deployment; staged beside booted; after an automatic fallback; after a good update). Image: the document in a container. bootc's own fourteen samples |
| JSON output has its members in sorted order and prints `null` for an absent entry | Captured, image |
| In a container `bootc status` succeeds for any user and reports `booted: null` | Image (root and uid 1000) |
| On a booted system `bootc status` must run as root | Source only: `status.rs` (`get_host`) calls `prepare_for_write` when the system is OSTree-booted, which calls `require_root` and re-executes under `unshare -m`. Not observed: no test here runs it on a booted system as an ordinary user |
| `--format=json` is the format to parse; `--json` prints the same; `--format-version` accepts 0 and 1 and refuses 2 | Source: `man/bootc-status.8.md`. Image |
| `cachedUpdate` is what a check found, and may equal the booted image | Source: `status.rs` (`imagestatus`, `render_cached_update`), sample `spec-booted-update-same-digest.yaml` |
| `bootc upgrade --check` fetches only the manifest and configuration | Source: `man/bootc-upgrade.8.md`, `cli.rs`. Image: the help text |
| `bootc upgrade --download-only` stages a deployment that is not applied at shutdown and is discarded by a restart; `bootc upgrade --from-downloaded` releases it without fetching; `--quiet` hides progress | Source: `bootc-upgrades.7.md`, `cli.rs`. Image: the four options are in `bootc upgrade --help` |
| A staged deployment that is not held is applied at shutdown by `ostree-finalize-staged.service` | Source: `man/bootc-upgrade.8.md`. Captured by the rollback workflow of pull request 36 |
| `bootc rollback` makes the rollback deployment first; a staged deployment is discarded; run with a rollback already queued it swaps the order back | Source: `man/bootc-rollback.8.md`, `deploy.rs` (`rollback`). Image: the help text |
| `bootc rollback` fails without a rollback deployment and for one that is not a container image | Source: `deploy.rs` ("No rollback available", "Rollback is not container image based") |
| Changes to `/etc` made in the newer version do not carry over to the version gone back to | Source: `man/bootc-rollback.8.md` |
| bootc has no command to pin a deployment; `ostree admin pin` takes an index or `booted`, `pending`, `rollback`, and `--unpin` | Source: bootc `cli.rs`; OSTree `man/ostree-admin-pin.xml`, `src/ostree/ot-admin-builtin-pin.c` at `v2026.4`. Image: `ostree admin pin --help` |
| OSTree refuses to pin a staged deployment | Source: `src/libostree/ostree-sysroot.c` at `v2026.4` |
| `bootc-fetch-apply-updates.timer` is not enabled in the base image | Image: `systemctl --root=/ is-enabled` prints `disabled`. Boot health masks it as well |
| The base image is 7.43 GB unpacked | Image: `podman image inspect`, 7 433 626 776 bytes |

**Assumptions, not verified:**

- That the previous version is removed when a new one is applied, unless it
  is pinned. The captures show two deployments after an update that began
  with one; none began with two.
- That an unknown amount of free space is safe to proceed on. OSTree has its
  own free-space check when it writes; this component does not rely on having
  seen it work.
- That `bootc upgrade --download-only` followed by `bootc upgrade
  --from-downloaded` behaves on TUR as documented. The rollback workflow
  stages with `bootc switch`, not with these two.

## 4. The policy

### 4.1 Principles

1. **An update TUR could not undo is not started.** Without boot health, or
   where attempts cannot be counted, nothing is staged.
2. **The only version known to work is never put at risk.** While the running
   version is on trial, nothing is staged and the start-up order is not
   changed.
3. **A version that failed here does not come back by itself.** A person can
   ask for it; a timer cannot.
4. **An update never interrupts and never surprises.** It waits for power, for
   an unmetered connection, for a repair session to end. No decision restarts
   the computer and no plan contains a restart.
5. **Unknown is said, not guessed.** Each unknown input has a stated meaning,
   and a decision that proceeded on one says so in its notes.
6. **A person is never held back where the update manager is not.**

A decision has one of four outcomes: **proceed**; **nothing** (nothing to do,
and that is fine); **wait** (not now, and no repair is needed); **refuse** (not
until something is put right). Every decision carries a code. When several
rules hold an operation back, the decision is the first, refusals before
waits, and the others are listed in `also`.

### 4.2 Checking for updates

| # | Condition | Outcome | Code |
|---|---|---|---|
| 1 | The running deployment is not a plain image | refuse | `TUR-UPDATE-STATE-005` |
| 2 | The system runs from read-only media | refuse | `TUR-UPDATE-STATE-006` |
| 3 | Offline | wait | `TUR-UPDATE-NETWORK-001` |
| 4 | Otherwise | proceed | `TUR-UPDATE-CHECK-001` |

A check may run on battery, on a metered connection, in a repair session and
with boot health unknown: it fetches a few kilobytes and changes no
deployment.

### 4.3 Downloading and staging

"Automatic" is the update manager on its timer. "Person" is someone who asked,
in Settings or at a terminal.

| # | Condition | Automatic | Person | Code |
|---|---|---|---|---|
| 1 | The running deployment is not a plain image; read-only media | refuse | refuse | `TUR-UPDATE-STATE-005`, `-006` |
| 2 | No check on record, nothing staged | nothing | nothing | `TUR-UPDATE-STAGE-003` |
| 3 | Up to date, nothing staged | nothing | nothing | `TUR-UPDATE-STAGE-002` |
| 4 | The update is staged, not held | nothing | nothing | `TUR-UPDATE-STAGE-004` |
| 5 | Boot-health state not known | refuse | refuse | `TUR-UPDATE-HEALTH-004` |
| 6 | Attempts cannot be counted | refuse | refuse | `TUR-UPDATE-HEALTH-003` |
| 7 | The image is in boot health's rejected list | refuse | proceed, with a warning | `TUR-UPDATE-HEALTH-001`; `TUR-UPDATE-HEALTH-002` |
| 8 | Free space below what is needed | refuse | refuse | `TUR-UPDATE-SPACE-001` |
| 9 | The running version is on trial, or not judged yet | wait | wait | `TUR-UPDATE-HEALTH-005` |
| 10 | This start failed its checks | wait | proceed | `TUR-UPDATE-HEALTH-006` |
| 11 | "Download updates automatically" is off | wait | proceed | `TUR-UPDATE-SETTINGS-001` |
| 12 | A repair session is open | wait | proceed | `TUR-UPDATE-SESSION-001` |
| 13 | Offline | wait | wait | `TUR-UPDATE-NETWORK-001` |
| 14 | Metered, and "Download on metered connections" is off | wait | proceed, with a note | `TUR-UPDATE-NETWORK-002` |
| 15 | On battery below 50 %, or at an unknown charge | wait | — | `TUR-UPDATE-POWER-001` |
| 16 | On battery below 20 % | (rule 15) | wait | `TUR-UPDATE-POWER-002` |
| 17 | Otherwise | proceed | proceed | `TUR-UPDATE-STAGE-001` |

Variants of rule 17: when the update replaces one that is staged, the code is
`TUR-UPDATE-STAGE-006`. When the update is already staged and held, only its
release remains (`TUR-UPDATE-STAGE-005`): nothing is downloaded, so rules 8,
11 and 13 to 16 do not apply; rules 5 to 7, 9, 10 and 12 do.

**What an unknown means.**

| Input | Unknown is treated as | Why |
|---|---|---|
| Boot health | A refusal (rule 5) | Principle 1 |
| Power | Mains | A desktop computer reports no battery |
| Battery charge, on battery | Too low for an automatic download; enough for a person | The cautious reading for a timer; a person can see the battery |
| Network | Online | The fetch reports its own failure |
| Metered | Not metered | As desktop software treats NetworkManager's "unknown" |
| Free space | Enough | Assumption (section 3); the note says it was not checked |
| Size of the update | 8 GiB | The base image is 7.43 GB unpacked; an update of unknown size is taken to share nothing with what is installed |

**The numbers** are in one place, `conditions::Limits`: 50 % and 20 % battery,
8 GiB assumed, 2 GiB kept free. With the size unknown an update therefore needs
10 GiB free. They are judgements (section 13).

### 4.4 The next automatic step

`next_step` is what the update manager would do now, by itself: **stage** when
staging may proceed; otherwise **check**, when the reason is one a check could
change (nothing known, up to date, or the known update is one that failed
here, which a corrected update would replace); otherwise what the staging
decision says: **nothing** with an update staged, **wait**, or **refuse**. How
often it does this is scheduling, which is not in this slice.

### 4.5 Going back

Going back is always a person's request, and it is class 3 for the assistant
(AI control layer, section 5): it is confirmed on the trusted path. That
confirmation is not this component's business; whether going back is sound is.

| # | Condition | Outcome | Code |
|---|---|---|---|
| 1 | Read-only media | refuse | `TUR-UPDATE-STATE-006` |
| 2 | No previous version | refuse | `TUR-UPDATE-ROLLBACK-002` |
| 3 | Already queued for the next restart | nothing | `TUR-UPDATE-ROLLBACK-005` |
| 4 | The previous version is not a plain image | refuse | `TUR-UPDATE-ROLLBACK-004` |
| 5 | The previous version is in the rejected list | refuse | `TUR-UPDATE-ROLLBACK-003` |
| 6 | The running version is on trial, or not judged yet | wait | `TUR-UPDATE-HEALTH-005` |
| 7 | Otherwise | proceed | `TUR-UPDATE-ROLLBACK-001` |

Power, the network, free space and the settings play no part. With boot health
unknown going back proceeds, and the note says rule 5 could not be checked.

### 4.6 Keeping and releasing

| Condition | Keep | Release |
|---|---|---|
| The version named does not exist, is staged, or is neither in use nor the previous one | refuse, `TUR-UPDATE-PIN-003` | refuse, `TUR-UPDATE-PIN-003` |
| It is kept | nothing, `TUR-UPDATE-PIN-002` | proceed, `TUR-UPDATE-PIN-004` |
| It is not kept | proceed, `TUR-UPDATE-PIN-001` | nothing, `TUR-UPDATE-PIN-005` |
| Read-only media, and something would change | refuse, `TUR-UPDATE-STATE-006` | refuse, `TUR-UPDATE-STATE-006` |

### 4.7 How the policy is tested

`tests/policy.rs`. Each row of 4.3 is a test that changes one condition from
favourable ones and expects its code, for both requesters (26 rows). Then every
combination is walked: 12 machines, 6 states of power, 5 of the network, 3 of
free space, 10 of boot health, a repair session or none, and the two settings:
7 200 sets of conditions, 172 800 staging decisions, with their plans. For each
the test asserts what the principles promise, independently of how the rules
are written: nothing is staged without boot health, on trial, or where attempts
cannot be counted; by itself the update manager never stages a rejected image,
never acts in a repair session, never downloads against a setting or on a low
battery; a person is never held back where the update manager is not; each
code is given with one outcome only; `also` lists refusals before waits and no
code twice; a plan has steps exactly when its decision proceeds. All 21 codes a
staging decision can carry are met in the walk.

## 5. What "roll back" and "keep this version" mean

**Roll back** (Settings ▸ Recovery, "Restart into this version"): `bootc
rollback`, then a restart the user chooses.

- The previous version becomes the one the computer starts. The version in use
  becomes the previous one, and stays on the computer.
- Nothing is downloaded and the running system is not changed until the
  restart.
- A staged update is discarded.
- **Files in home folders and installed applications are not changed. System
  settings are.** Each version has its own `/etc`; going back does not merge
  it. What was changed under `/etc` since the previous version last ran (a
  network connection saved for all users, a user account, a password) goes
  back to what it was then. The plan says so; the desktop text of 11.1 does
  not (section 14).
- The previous version is started **without counted attempts**. Boot health
  arms a trial only for a staged deployment. That is why going back to a
  version that failed here is refused: "Try the update again" is the way to
  retry it, because it stages the image and so gives it three attempts.
- Only the previous version can be gone back to. A kept version that is
  neither in use nor the previous one cannot be started by `bootc rollback`
  (section 12).

**Keep this version**: `ostree admin pin booted` or `ostree admin pin
rollback`.

- A kept version is never removed when updates are installed. It uses disk
  space until it is released, and it is never removed to make room for an
  update.
- **Keeping a version does not stop updates.** The switch for that is
  "Download updates automatically".
- A staged version cannot be kept before it has been started.
- **Release** (`ostree admin pin --unpin`) undoes it: the version is treated
  like any other again.

## 6. Plans

A plan is made for a decision to proceed, and has no steps otherwise. It names
commands by full path, in order, with what each changes and what the user
would notice. Every command a plan can name is in one table, `plan::COMMANDS`,
which `tur-update commands` prints:

| Name | Command | Changes | Run by `tur-update` |
|---|---|---|---|
| `read-status` | `/usr/bin/bootc status --format=json` | nothing | with `--live` only |
| `check` | `/usr/bin/bootc upgrade --check` | the cached result of a check | never |
| `stage-held` | `/usr/bin/bootc upgrade --download-only --quiet` | the system | never |
| `release` | `/usr/bin/bootc upgrade --from-downloaded` | the system | never |
| `rollback` | `/usr/bin/bootc rollback` | the system | never |
| `pin` | `/usr/bin/ostree admin pin <booted or rollback>` | the system | never |
| `unpin` | `/usr/bin/ostree admin pin --unpin <booted or rollback>` | the system | never |

No command restarts the computer: `--apply` and `--soft-reboot` are named
nowhere.

**Staging is three steps**, and the middle one is the reason
([ADR-0017](../adr/0017-update-manager.md)):

1. `bootc upgrade --download-only --quiet`: the image is downloaded and
   staged, **held**. A restart at this point starts the running version again
   and discards the staged one.
2. `bootc status --format=json`: what is staged is read back. Its digest must
   be the one the decision was made for, and must not be in the rejected list.
   A tag can move between the check and the download; the decision was about a
   digest. If it is anything else, the decision is made again for what is
   actually staged, and the staged version stays held meanwhile.
3. `bootc upgrade --from-downloaded`: the staged version is released, fetching
   nothing. From here it is "ready": finalised at the next shutdown, started at
   the next boot, on trial for three attempts.

For an update that is already staged and held, the plan is steps 2 and 3.

Effects are plain sentences, for example for staging: the new version is
downloaded beside the one in use, which keeps running; at the next restart the
computer starts it, with three attempts, and returns by itself if it does not
start; the version in use stays as the previous one; the previous version now
is removed, unless it is kept.

## 7. How it fits boot health

Boot health is designed in [addendum C](boot-health-and-rollback.md) and
decided in ADR-0014. Its implementation is pull request 36
(`services/tur-boot-health`), not merged when this was written. This component
depends on none of its code. It reads one document, the output of
`tur-boot-health status --json` (`boot-health-status.schema.json` in that
pull request), and the fixtures are that program's real output from the
rollback workflow.

| What boot health records | Member read | What the update manager does with it |
|---|---|---|
| Images that failed their trial | `rejected.images[].digest` | Never staged automatically (4.3 rule 7); never gone back to (4.5 rule 5); marked in `status` |
| Whether attempts can be counted | `counting.usable` | Nothing is staged where they cannot (rule 6). Boot health would refuse to apply the update at shutdown (`TUR-BOOT-TRIAL-002`); the update manager does not download it |
| A trial in progress | `trial` not null, or `counting.boot_counter` not null | Nothing is staged or reordered (rule 9; 4.5 rule 6) |
| The gate's verdict for this boot | `verdict.result`; `null` while it runs | `null`: as a trial. `failed`: no automatic staging (rule 10) |
| What the user is to be told | `notice` (kind, text, code), until acknowledged | Passed through in `status`. "The update was not applied" (`TUR-BOOT-TRIAL-001`) and the fallback notice are boot health's text and are not rephrased |

What the update manager must respect, from addendum C section 5.10, and where
it does:

| Requirement | Where |
|---|---|
| "The updater must not stage a rejected digest unless a person asks for it explicitly" | Rule 7; and step 2 of staging re-checks the digest that was actually staged |
| "Any future TUR updater is the agent in active control and reads the rejected list" | It reads it through `status --json`, which also carries the three other facts. `bootc-fetch-apply-updates.timer` stays masked: two agents would undo each other |
| Arming happens at shutdown, for any staged deployment that is not held | The update manager arms nothing. Step 3 only makes the deployment eligible; boot health does the rest. A deployment that is still held is not armed, as designed |
| Soft reboots are not used for updates | No plan names `--soft-reboot` or `--apply` |
| State that must survive a rollback is not kept in `/etc` | This slice keeps no state. A later slice keeps its own under `/var/lib/tur/update` |

Three places where the two do not yet meet (section 14 proposes text):

1. **Nothing removes an entry from the rejected list.** Section 5.10 says a
   person's request "removes the entry". `tur-boot-health` has no command for
   it and the update manager must not write another component's state. Here a
   person's request stages the image for a new trial and the entry stays, so
   that the image is still not staged automatically. If the retried version is
   marked good, the entry is stale and harmless.
2. **A rollback by a person arms no trial**, by design (5.10). The update
   manager therefore refuses the one case where that is unsafe (4.5 rule 5).
3. **`status --json` of boot health is root's view.** A session program cannot
   run it, as it cannot run `bootc status` (section 3). Both reach the session
   through `org.tur.Update1`.

## 8. D-Bus contract (proposed, not implemented)

`services/tur-update/dbus/org.tur.Update1.xml`. System bus, name
`org.tur.Update1`, object `/org/tur/Update1`. The interface is **not frozen**
and no service exports it. The desktop design lists who will use it:
`tur-settings` (Updates and Recovery), `tur-notifier`, `tur-centre` (read
only).

The service will run as root, because `bootc status` needs root (section 3).
It will gather the conditions (UPower, NetworkManager, the filesystem,
`tur-boot-health`), run the policy of this crate, carry out the plans of this
crate, and publish the state. It will be the only component that runs `bootc
upgrade` or `bootc rollback`.

JSON results are documents of `schemas/update-report.schema.json`, as with
`org.tur.Storage1`: one versioned schema for the bus, the command line and
the diagnostics bundle (§16).

| Method | Authorisation (polkit action) | Returns | Timeout | Cancellation |
|---|---|---|---|---|
| `GetStatus` | `org.tur.update.read`: any local session | `s`: status document | 5 s | Not cancellable; reads |
| `PreviewCheck` | `org.tur.update.read` | `s`: check document | 5 s | Not cancellable; reads |
| `Plan` | `org.tur.update.read` | `s`: plan document | 5 s | Not cancellable; reads |
| `Check` | `org.tur.update.check`: active local session | `s`: the decision for the check | 5 s to decide; the check itself 120 s | `Cancel` stops the fetch; nothing was changed |
| `Download` | `org.tur.update.download`: active local session | `s`: plan document for what is being carried out | 5 s to decide; no limit on the download, which reports `Progress` | `Cancel` stops it before step 3. A staged, held version is left held and is discarded by the next restart |
| `Rollback` | `org.tur.update.rollback`: administrator authentication, every time | `s`: plan document for what was carried out | 60 s | Not cancellable once `bootc rollback` has started. Before the restart, calling it again is refused (`TUR-UPDATE-ROLLBACK-005`) |
| `Keep`, `Release` | `org.tur.update.keep`: administrator authentication | `s`: plan document | 30 s | Not cancellable |
| `Cancel` | The caller that started the operation | nothing | 5 s | — |

Rules common to all methods:

- **Errors.** `org.tur.Update1.Error.NotAuthorized`, `.InvalidArgument`,
  `.Refused`, `.Waiting`, `.NothingToDo`, `.Busy`, `.Cancelled`, `.TimedOut`,
  `.Failed`. The message starts with the `TUR-UPDATE-*` code, then the user
  message. A method that changes the system returns `.Refused`, `.Waiting` or
  `.NothingToDo` when the decision is not to proceed.
- **The decision is made again at call time.** A preview or a plan is not a
  promise.
- **One operation at a time.** A second `Check`, `Download` or `Rollback`
  while one runs returns `.Busy`.
- **For the assistant every method that changes the system is class 3.**
  polkit decides for a person; the policy engine decides for P@x, and for P@x
  the answer is the trusted path, whatever polkit would say.
- **No method restarts the computer.** "Restart now" is the session's.
- **No secret is an argument.** Registry credentials, when there are any, are
  not passed over this interface.

Signals: `StateChanged` (a status document), `Progress` (phase, bytes done,
bytes in all). Properties: `SchemaVersion`, `State`. `State` adds the
operation states to those of section 2.2: `checking`, `downloading`,
`verifying`, `not-applied`, `offline`.

## 9. Assistant tools, command line and schema

### 9.1 Tools

`services/tur-update/pax/agent-tools.json` declares two tools, both class 0,
and `pax/agent-tools.adapter.json` performs them with this component's
program:

| Tool | Class | Method | Performed by |
|---|---|---|---|
| `update_status` | 0 | `GetStatus` | `tur-update status --live --json` |
| `update_preview_check` | 0 | `PreviewCheck` | `tur-update check --live --json` |

Neither takes an argument. Staging, going back, keeping and releasing are
class 3 and are not declared as tools at all in this slice: a tool nobody can
perform, for an operation a person must confirm on a path that does not exist
yet, would be a name without a meaning.

`tests/agent_tools.rs` loads the two files with P@x's own loader, checks the
classes, holds each command line against the built program, and runs the
tools on recorded machines.

**Two things stand between these tools and P@x.**

1. **Where the files are.** Other components keep the two files in their own
   directory, `services/<component>/`. `services/` is read as a directory of
   tool sets by `services/tur-pax/tests/tools.rs` and by the `pax` workflow
   (`tests/pax/e2e.py`), and both list the components that ship tools: five.
   A sixth breaks both until they are changed, and the `pax` workflow has to
   build and install `/usr/bin/tur-update`. That is a change to the
   assistant's own tests, outside what this slice may touch. So the files are
   one directory down, in `pax/`, where `tur-pax` does not look, and a test
   says so. Moving them up is one `git mv` and the three edits of section 14.
2. **What the tools can see.** P@x runs tool programs in its read-only
   sandbox, as the user. There `bootc status` fails (section 3), and the tool
   returns `TUR-UPDATE-STATE-004`: reading the versions needs administrator
   rights. Until `org.tur.Update1` exists the tools can therefore say only
   that. They are complete as an interface and useful once the service
   publishes the state.

### 9.2 Command line

```text
tur-update status  (--from FILE | --live) [--boot-health FILE] [--json]
tur-update check   (--from FILE | --live) [--boot-health FILE] [conditions] [--json]
tur-update plan stage    (--from FILE | --live) [--boot-health FILE] [conditions] [--requested] [--json]
tur-update plan rollback (--from FILE | --live) [--boot-health FILE] [conditions] [--json]
tur-update plan pin      (--from FILE | --live) [--deployment booted|rollback] [--json]
tur-update plan unpin    (--from FILE | --live) [--deployment booted|rollback] [--json]
tur-update commands [--json]
tur-update codes [--json]

conditions: --power unknown|mains|battery  --battery-percent N
            --network unknown|online|offline  --metered unknown|yes|no
            --free-bytes N  --update-bytes N  --repair-session
            --no-automatic-download  --download-on-metered
```

`--from` names a file holding what `bootc status --format=json` printed; the
tool then runs no program. `--live` runs `/usr/bin/bootc status --format=json`
and nothing else. The conditions are whatever the options say; what is not
given is unknown. The tool takes nothing from the process environment.

Exit status: 0 when the answer was printed, whatever the answer is; 1 when
there is no answer (the state could not be read), with the code and the
registry's text on standard error; 2 for a usage error. This is what
`tur-storage` does.

### 9.3 Schema

`schemas/update-report.schema.json`: five kinds of document, `status`,
`check`, `plan`, `commands`, `codes`, each with `schema_version: 1`. A plan's
`executed` member is the constant `false`. `tests/cli.rs` runs the built
program on every recorded machine (16 machines, 15 documents each) and
validates every document.

## 10. Error codes

Family `TUR-UPDATE-*` (§29.3). Each has a registry entry with title, severity,
detection, user message, technical explanation and recovery action
(`codes.rs`; `tur-update codes`). User messages follow the desktop's tone
rules: a code that stopped an update begins "The update was not applied."

| Code | Severity | User message |
|---|---|---|
| `TUR-UPDATE-STATE-001` | error | TUR could not read which versions are on this computer. |
| `TUR-UPDATE-STATE-002` | error | This system does not receive TUR updates. |
| `TUR-UPDATE-STATE-003` | error | TUR could not read which versions are on this computer. |
| `TUR-UPDATE-STATE-004` | notice | Reading the versions on this computer needs administrator rights. |
| `TUR-UPDATE-STATE-005` | error | This installation was changed in a way that TUR updates cannot manage. |
| `TUR-UPDATE-STATE-006` | notice | This system runs from read-only media and cannot be updated. |
| `TUR-UPDATE-STATE-007` | error | TUR could not read the record of earlier start-up checks. |
| `TUR-UPDATE-CHECK-001` | info | TUR can check for updates. |
| `TUR-UPDATE-NETWORK-001` | notice | TUR cannot check for updates without a network connection. Nothing is wrong. |
| `TUR-UPDATE-NETWORK-002` | notice | The update is waiting for a connection that is not metered. |
| `TUR-UPDATE-POWER-001` | notice | The update is waiting until the computer is plugged in. |
| `TUR-UPDATE-POWER-002` | warning | The battery is too low to download the update now. |
| `TUR-UPDATE-SPACE-001` | error | The update was not applied. There is not enough free space on this computer. |
| `TUR-UPDATE-HEALTH-001` | notice | TUR will not install this update again by itself. |
| `TUR-UPDATE-HEALTH-002` | warning | This update did not start properly on this computer before. TUR will try it once more because you asked. |
| `TUR-UPDATE-HEALTH-003` | error | The update was not applied. This installation cannot count start-up attempts, so a failed update could not be undone automatically. |
| `TUR-UPDATE-HEALTH-004` | error | The update was not applied. TUR could not confirm that a failed update would be undone automatically. |
| `TUR-UPDATE-HEALTH-005` | notice | TUR is still checking that this version started properly. |
| `TUR-UPDATE-HEALTH-006` | warning | TUR did not pass its start-up checks, so updates are not installed automatically. |
| `TUR-UPDATE-SETTINGS-001` | info | An update is available. It is not downloaded because automatic downloads are switched off. |
| `TUR-UPDATE-SESSION-001` | notice | The update is waiting until the repair session has ended. |
| `TUR-UPDATE-STAGE-001` | info | The update can be downloaded and prepared. |
| `TUR-UPDATE-STAGE-002` | info | TUR is up to date. |
| `TUR-UPDATE-STAGE-003` | info | TUR has not checked for updates yet. |
| `TUR-UPDATE-STAGE-004` | info | The update is ready. Restart to use it. |
| `TUR-UPDATE-STAGE-005` | info | The update has been downloaded and is waiting to be released. |
| `TUR-UPDATE-STAGE-006` | info | A newer update replaces the one that was waiting. |
| `TUR-UPDATE-ROLLBACK-001` | info | TUR can go back to the previous version. |
| `TUR-UPDATE-ROLLBACK-002` | notice | There is no previous version on this computer to go back to. |
| `TUR-UPDATE-ROLLBACK-003` | warning | TUR will not go back to that version: it did not start properly on this computer. |
| `TUR-UPDATE-ROLLBACK-004` | error | The previous version cannot be used by TUR updates. |
| `TUR-UPDATE-ROLLBACK-005` | info | TUR will go back to the previous version at the next restart. |
| `TUR-UPDATE-PIN-001` | info | TUR can keep this version. |
| `TUR-UPDATE-PIN-002` | info | TUR is already keeping this version. |
| `TUR-UPDATE-PIN-003` | notice | There is no such version on this computer. |
| `TUR-UPDATE-PIN-004` | info | TUR can stop keeping this version. |
| `TUR-UPDATE-PIN-005` | info | TUR is not keeping this version. |

`STATE-001` to `-004` and `-007` are failures of the command line: there is no
answer. The others are the code of a decision. `STATE-001` and `-003` share a
sentence because the user can do the same about both; the technical layer
tells them apart.

The four codes the desktop's "Not applied" state names (signature, checksum,
space, network) are one here, space. The other three are what a download
reports when it fails, and belong to the slice that runs one.

## 11. What later slices add

| Slice | What | Needs |
|---|---|---|
| **Executing** | The service `org.tur.Update1`: gathers the conditions, runs the three steps of staging, `bootc rollback`, pin and unpin; its own state under `/var/lib/tur/update` (the time of the last check, the digest a person went back from, the operation in progress); the states "checking", "downloading", "verifying", "not applied"; codes for a download that failed (network, checksum) | The interface of section 8 frozen; boot health merged; a boot test that stages with `--download-only` and `--from-downloaded` and reaches the new version; fault injection: power cut between the three steps |
| **Scheduling** | When the automatic step runs: after boot is marked good, daily, and when the network returns | The executing slice |
| **Signatures** | The image followed must be verified when fetched: `spec.image.signature` is `containerPolicy` and the policy names TUR's key; a refusal code for an unverified source and for a failed verification (§5.3, §15.1). Today the test image has no verification configured, and `status` and the decisions say so in a note | A signing key and a published image, which need the release work (phase 7) and the Fedora marks replaced |
| **Channels** | Which image reference a machine follows (stable, testing), and moving between them with `bootc switch`; "What is new" for a version | A registry, release process (§20, phase 7) |
| **Kept versions** | Releasing, or going back to, a kept version that is neither in use nor the previous one | The OSTree index of a deployment, which `bootc status` does not give |
| **Android images** | `tur-update` as the manager of a second kind of payload: Android system and vendor images under `/var/lib/waydroid/tur-images/<version>/`, verified and activated with the session stopped, the previous set retained, the user's Android data archived or snapshotted before a major upgrade, and an OS update that would break the Waydroid and image pair reported before it is activated ([android-runtime.md](android-runtime.md) section 8.3, threat "Outdated Android system" in 10.4, milestone B4 in section 14; ADR-0006) | The Android runtime (priority 6); a model of a payload that is not a bootc deployment. Nothing in this slice anticipates it beyond keeping "version" and "deployment" apart |
| **Runtime payloads** | Wine, Proton and the like have version manifests and per-application pinning (§15.2) | The runtime manager's design; not an OS update |

## 12. Deliberately not done, and what was not run

- **No command that changes a system is run**, by the program or by any test.
  Staging, releasing, going back, pinning: NOT RUN.
- **The live read on a booted TUR system: NOT RUN.** `tur-update status
  --live` ran in CI inside the base image as a container, with the image's
  bootc 1.17.1, where bootc reports no booted deployment and the program
  answers `TUR-UPDATE-STATE-002`. On a booted system bootc re-executes itself
  under `unshare`; that path has not been exercised by this program. It needs
  `tur-update` in the image and a line in the boot test.
- **The live read as an ordinary user on a booted system: NOT RUN.** The
  `TUR-UPDATE-STATE-004` path is tested with a stand-in for bootc.
- **In P@x's sandbox: NOT RUN** (section 9.1).
- **On real hardware: NOT RUN.** There is no hardware runner (ADR-0002).
- **Against boot health as merged: NOT RUN.** The boot-health fixtures are
  output of pull request 36 at commit `c1667d98`. If its `status --json`
  changes before it merges, `health.rs` and four fixtures change with it.
- **The conditions are not measured.** Power, network, free space and the
  repair session are what the command line is told.
- **No D-Bus service, no polkit actions, no systemd units, nothing in the
  image.**
- **No signature is checked** and no channel exists.
- **A kept version that is neither in use nor the previous one** can be seen
  and cannot be named.
- **The schema is checked by a validator written for the test**, for the
  keywords the schema uses, as in `tur-storage`.

## 13. Decisions the owner may want to reverse, and open questions

1. **Two-step staging** (ADR-0017). It costs one more command and a second
   read of the status. The alternative is `bootc upgrade` alone, which stages
   whatever the tag points to at that moment.
2. **Boot health unknown refuses staging**, even for a person. The alternative
   is to proceed with a warning. On a machine where boot health is absent or
   broken that means no updates at all until it is repaired, including the
   update that would repair it. A recovery path for that case (for instance,
   staging from the rescue environment) is not designed.
3. **A person may retry a rejected image**, and the entry stays in the list.
4. **A person is not held back by the settings, a repair session, a metered
   connection or a start that failed its checks.** The desktop should say so
   before the download (the note is in the decision).
5. **The thresholds**: 50 % and 20 % battery; 8 GiB assumed and 2 GiB kept
   free, so 10 GiB when the size is not known. On a small disk that may never
   be met. The size becomes known when a check records it (`bootc upgrade
   --check` prints the difference between the manifests).
6. **"Keep this version" does not stop updates.** A user may expect it to.
7. **Going back to a rejected version is refused outright.** An alternative is
   to arm a trial for a manual rollback, which is a change to boot health.
8. **Unknown power is mains; unknown network is online; unknown metered is not
   metered.**
9. **How many versions may be kept** is not limited. Each costs disk space.
10. **Should a version a person went back from be offered again
    automatically?** bootc warns that an agent doing automatic upgrades may
    undo a rollback. Here that needs the update manager's own state (the
    digest gone back from), which the executing slice adds; the policy has no
    rule for it yet.
11. **The interface of section 8 is a proposal.** In particular: whether
    `Download` needs authentication; whether `Plan` should exist or the
    desktop should compose its own text.

## 14. Text proposed for other documents

Not applied by this change: each file belongs to someone else.

| Document | Proposal |
|---|---|
| `specification.md` §15.1 | Add: the update manager stages in three steps (held download, verification of the staged digest, release); it stages nothing where boot health is unknown, cannot count attempts, or has the running version on trial; a rejected image is staged again only at a person's request; the size and thresholds of section 4.3 |
| `specification.md` §16 | `org.tur.Update1`: replace "Stage, deploy, assess, rollback" with "Check, stage, go back, keep; the state of updates. Assessment is boot health's (`tur-boot-health`)" |
| `specification.md` §27.5 | Row "Update download/signature failure": the message is the registry's for the code, beginning "The update was not applied." |
| `specification.md` §29.3 | Record the `TUR-UPDATE-*` topics in use: STATE, CHECK, NETWORK, POWER, SPACE, HEALTH, SETTINGS, SESSION, STAGE, ROLLBACK, PIN |
| `boot-health-and-rollback.md` §5.10 | "unless a person asks for it explicitly, which removes the entry": either add a command that removes an entry (`tur-boot-health forget <digest>`, run by the update service) or change the sentence to "the entry stays until that version is marked good, when `mark-good` removes it". The second needs no new interface |
| `boot-health-and-rollback.md` §5.9 or §5.10 | State that a rollback by a person is not a counted trial, and that the update manager therefore refuses to go back to a rejected version |
| `boot-health-and-rollback.md` §3.3 | `status.otherDeployments` is left out when empty; `status.readOnly` and `status.usrOverlay` exist in 1.17.1; `bootc-fetch-apply-updates.timer` is `disabled` in the base image (measured) |
| `desktop-experience.md` §11.1 | "Your files, applications and settings stay as they are" (the prompt for going back, and the fallback notice) is not true of system settings: say "Your files and applications stay as they are. System settings changed since then (network connections for all users, accounts, passwords) go back to what they were." Boot health's own notice already says this |
| `desktop-experience.md` §11.1 | "Not applied … signature, checksum, space or network": add the reasons that exist before any download: boot health unknown, attempts cannot be counted. Add the states `held` and `not-checked`. Add "Versions on this computer" marks: kept, failed to start here |
| `desktop-experience.md` §11.1 | "Try the update again" on the fallback notice is the person's request of 4.3 rule 7 and gives `TUR-UPDATE-HEALTH-002`, a warning to show before it starts |
| `ai-control-layer.md` §11 | The update tools: `update_status` and `update_preview_check`, class 0. No class 3 update tool is declared until the trusted path exists |
| `services/tur-pax/tests/tools.rs`, `tests/pax/e2e.py`, `.github/workflows/pax.yml` | When the tool set moves to `services/tur-update/`: add `tur-update` to the three lists of components (six, not five), and build and install `/usr/bin/tur-update` in the `pax` workflow. A scenario for the tools is only useful once the service exists |
| `THIRD_PARTY_NOTICES.md` | The sentence "No such code exists yet" about upstream-licensed files: `services/tur-update/tests/fixtures/bootc-upstream/` holds fourteen test samples converted from bootc's source tree, under bootc's licence (MIT OR Apache-2.0), with `LICENSE-MIT` beside them. They are test data and are not in the image |

## 15. Sources

All fetched and read on 12 November 2026.

| Ref | Source | Used for |
|---|---|---|
| U1 | bootc at tag `v1.17.1` (https://github.com/bootc-dev/bootc): `crates/lib/src/spec.rs`, `status.rs`, `cli.rs`, `deploy.rs`, `store/mod.rs` | The status document; root; `cachedUpdate`; upgrade, download-only, rollback |
| U2 | bootc documentation at the same tag: `docs/src/man/bootc-status.8.md`, `bootc-upgrade.8.md`, `bootc-rollback.8.md`, `bootc-switch.8.md`, `docs/src/bootc-upgrades.7.md`, `docs/src/host-v1.schema.json` | Options and their meaning; `/etc` on rollback |
| U3 | bootc's samples at the same tag, `crates/lib/src/fixtures/spec-*.yaml` and `spec-v1-null.json` (fourteen files; `tests/fixtures/provenance.json` has the git blob identifier of each) | Fixtures |
| U4 | OSTree at tag `v2026.4` (https://github.com/ostreedev/ostree): `man/ostree-admin-pin.xml`, `src/ostree/ot-admin-builtin-pin.c`, `src/libostree/ostree-sysroot.c` | Pinning |
| C1 | `boot` workflow, run 43186059647 (main, commit `6a13a68b`), artifact `tur-boot-test` | `bootc status` of the installed test image |
| C2 | `rollback` workflow, run 43207801019 (pull request 36, commit `c1667d98`), artifacts `tur-rollback-failing-gate` and `tur-rollback-kept-or-not-applied` | `bootc status` with a staged deployment, after a fallback, after a good update; `tur-boot-health status --json` at the same moments |
| C3 | `update` workflow of this change (runs in [the report](../reports/update-1.md)), artifact `tur-update-bootc` | bootc and OSTree versions in the base image; `bootc status` in a container, as root and as a user; the help of every command named; the image size |

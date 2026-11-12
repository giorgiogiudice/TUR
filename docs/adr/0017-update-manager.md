# ADR-0017: The update manager — TUR drives bootc in three steps, and stages nothing boot health cannot undo

- **Status:** Accepted, with the amendment at the end
- **Date:** 2026-11-12
- **Specification sections:** §5.3, §15.1, §15.4, §16 (`org.tur.Update1`),
  §27.5; [ADR-0014](0014-boot-health-and-rollback.md). Design:
  [update manager](../architecture/update-manager.md)

## Context

[ADR-0014](0014-boot-health-and-rollback.md) decided how a new version is
tried and how TUR returns from one that does not start. It left to "the
updater" what is fetched and when, and bound it in three ways: a rejected
image is not staged again automatically; bootc's own update timer is masked;
an update is refused where boot attempts cannot be counted.

The first slice of that updater, `tur-update`, changes nothing: it reads
`bootc status`, decides, and prints plans. But a plan is a statement of what
TUR will do, the desktop is being built against it, and the choices below
shape the interface. They are recorded now, as proposed, before anything
executes them.

What is known, from the source and documentation of bootc 1.17.1 and OSTree
2026.4, the versions in the base image, and from real output (the design,
section 3):

- `bootc upgrade` stages whatever the followed reference points to **at that
  moment**. A check made earlier (`bootc upgrade --check`) was about a digest;
  a tag can move between the two.
- `bootc upgrade --download-only` stages a deployment that is **held**: it is
  not applied at shutdown and a restart discards it. `bootc upgrade
  --from-downloaded` releases it without fetching.
- `tur-boot-health` arms a trial at shutdown for a staged deployment that is
  not held, and only for a staged deployment. `bootc rollback` reorders two
  deployments and arms nothing.
- `bootc status` must run as root on a booted system.
- bootc cannot pin a deployment; OSTree can, by index or as `booted` and
  `rollback`.

## Options considered

**How an update is staged.**

1. **`bootc upgrade`, one step.** Simplest. The image staged may not be the
   one that was checked against the rejected list and, later, against a
   signature policy and a channel. A restart during or right after the
   download applies whatever was staged.
2. **`bootc switch` to the checked digest.** Stages exactly that digest. It
   also changes the reference the system follows to a digest, after which
   `bootc upgrade` finds nothing: the update manager would have to keep the
   real reference itself and switch back.
3. **Three steps: download held, read back, release.** The image is staged
   held; the staged digest is read from `bootc status` and held against the
   decision; then it is released. The reference followed is untouched. Between
   the steps a restart or a power cut leaves the running version and nothing
   staged.

**What happens when boot health is unknown or cannot count.**

1. Proceed with a warning. Updates keep flowing on a machine whose safety net
   is missing.
2. **Refuse to stage.** No update can leave such a machine unable to start,
   and it gets no updates until the net is repaired.

**What "keep this version" means.**

1. Stop updating. One control, two meanings; a kept version would also mean no
   security updates.
2. **Pin the deployment**, and nothing else. Updates continue; the switch for
   stopping them is "Download updates automatically".

## Decision

- **TUR's update manager is the only agent that runs `bootc upgrade` and
  `bootc rollback`.** bootc's timer stays masked (ADR-0014).
- **Staging is three steps** (option 3): `bootc upgrade --download-only`,
  then `bootc status` and a comparison of the staged digest with the one the
  decision was made for and with the rejected list, then `bootc upgrade
  --from-downloaded`. If the staged image is not the expected one, the
  decision is made again for what is staged, and it stays held meanwhile.
- **Nothing is staged that boot health could not undo**: not when its state
  is unknown, not where attempts cannot be counted, and not while the running
  version is itself on trial, when staging would remove the only version
  known to work. This holds for a person's request as for a timer.
- **Nothing is staged while going back is queued.** bootc warns that an
  agent doing automatic upgrades may revert a rollback; this agent does not.
- **A rejected image is staged again only at a person's request**, as a new
  counted trial. Going back to a rejected version with `bootc rollback` is
  refused, because that start would not be counted.
- **The update manager never restarts the computer.** No plan names `--apply`
  or `--soft-reboot`; the restart is the user's, through the session.
- **"Go back" is `bootc rollback` to the previous version**, and its plan
  says that system settings under `/etc` go back with it. **"Keep this
  version" pins a deployment** with `ostree admin pin` and does not stop
  updates.
- **Automatic work waits; it does not fail**: for mains power or half a
  battery, for a connection that is not metered, for a repair session to end.
  A person who asks is held back only by what protects the machine: boot
  health, space, no network, an almost empty battery.
- **The state reaches the session through `org.tur.Update1`**, a root
  service, because `bootc status` needs root. The command line is the
  service's logic without the service.

The policy table, the thresholds and the proposed interface are in the
design. The thresholds are judgements and can change without a new record.

## Consequences

- One more command and one more read of the status for every update. In
  return, what is released is what was decided on. Signature and channel
  checks, when they exist, have a place to stand: step 2.
- **A machine whose boot health is absent or broken receives no updates**,
  including one that would repair boot health. That case needs a recovery
  path, which is not designed.
- The desktop text for going back ("Your files, applications and settings
  stay as they are") is wrong about system settings and has to change
  (design, section 14).
- Boot health's sentence that a person's request "removes the entry" from the
  rejected list has no mechanism. Here the entry stays; the design proposes
  two ways to close the gap.
- A kept version that is neither in use nor the previous one cannot be
  released or returned to through `bootc status` alone. That needs the OSTree
  index and is left to a later slice.
- `tur-update` holds no state in this slice. The executing slice keeps its own
  under `/var/lib/tur/update`, never in `/etc` (ADR-0014).
- The interface `org.tur.Update1` is proposed here and frozen separately,
  before the desktop's live backend depends on it (`AGENTS.md`).

## Verification

Shown by this change, in CI:

- The model reads real output of bootc 1.17.1: the TUR test image booted with
  one deployment, with a staged one, after an automatic fallback and after a
  good update; the base image as a container; bootc's fourteen samples.
- Every command a plan can name exists in the base image, and every option is
  one its `--help` describes (`update` workflow).
- The policy keeps its promises over every combination of the conditions it
  knows (187 200 decisions).

Not shown, and required before the decision is accepted as built:

- **The three steps on a booted TUR image**: stage held, read back, release,
  restart, and reach the new version under a counted trial. Then with the tag
  moved between check and download, and with a power cut between each pair of
  steps. This belongs with the rollback scenarios of ADR-0014.
- That `bootc upgrade --from-downloaded` leaves a deployment `tur-boot-arm`
  treats as any other staged deployment.
- That a held deployment survives until released and is discarded by a
  restart, as documented.
- Re-check at each rebase of the base image, with ADR-0014's list: the shape
  of `bootc status --format=json`, and the options named. The `update`
  workflow fails when bootc's version is no longer the one the fixtures were
  verified against.

## Acceptance and amendment (12 November 2026)

Accepted as the basis for the executing slice, with one change to the third
point of the decision.

"Nothing is staged that boot health could not undo" holds without exception
for automatic work. For a person it holds while the running version is on
trial. But where boot health's state is **unknown**, or attempts **cannot be
counted**, refusing a person's request too would leave a machine with broken
boot health unable to receive the update that repairs it. In those two cases a
person may stage the update after a warning that says plainly that this
update will not be undone automatically if it fails to start, and how to
return to the previous version by hand from the boot menu. It is a class 3
operation, never available to P@x on its own authority and never to a timer.
The first slice refuses in these cases; the executing slice implements the
warning and the override, with a scenario in the `rollback` workflow.

Before anything executes, the proof named under Verification is required: a
scenario that stages with `--download-only` and then `--from-downloaded`,
including a moved tag and power cuts between the steps.


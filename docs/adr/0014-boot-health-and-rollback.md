# ADR-0014: Boot health and rollback — a TUR attempt counter in GRUB's environment block

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §3.5, §3.6, §5.3 to §5.6, §15.1, §19.5,
  §22 (TUR-005), §27.1, §27.4, §27.5

## Context

TUR promises that an update cannot leave the machine unable to start: a new
version is deployed beside the running one, and a version that does not boot
properly is replaced by the previous one automatically, with the evidence
kept.

The specification built this on systemd-boot boot counting (§5.3).
[ADR-0013](0013-bootloader.md) established, from a booted image, that the
stack is GRUB 2.12 through Fedora's signed shim, installed by bootupd, with
an OSTree deployment managed by bootc 1.17.1, and that this loader does no
boot counting. It left the mechanism to this decision and named greenboot as
the thing to evaluate first.

The evaluation is in
[boot-health-and-rollback.md](../architecture/boot-health-and-rollback.md),
section 3, with sources. What decides the matter:

- **greenboot, as Fedora 45 ships it, counts only what its own health check
  has seen fail.** The maintained implementation is the Rust rewrite
  (greenboot-rs, 0.16.4 in Fedora 45, 0.16.5 in testing). In its source the
  boot counter is written in one place: after a health check has failed. A
  new deployment that panics, cannot mount its root, lands in emergency mode
  or hangs is never counted. The older shell implementation did arm the
  counter when an update was staged; ADR-0013 and TUR-003 describe that
  behaviour.
- greenboot-rs runs its checks without any time limit, reports through a
  message of the day, and has open defects in the rollback path itself:
  the unit that records the rollback trigger does not run at shutdown, and a
  failed finalisation makes a healthy machine roll back. These are user
  reports upstream, with a fix for the first under review. TUR has not
  reproduced them.
- bootc has no boot-health or automatic-rollback feature and announces
  none. Its documentation says its composefs backend "does not configure
  boot entry counting". OSTree can write Boot Loader Specification counters,
  but Fedora's GRUB does not implement them, and bootc supports systemd-boot
  only with the composefs backend.
- Fedora's GRUB does have what a counter needs, already inside the signed
  binary: an environment block it can write, `increment` and `decrement`,
  and expansion of GRUB variables in the kernel arguments of a loader entry.
- systemd's `boot-complete.target` is usable with any loader. Its design
  note asks loaders that keep counters elsewhere to supply their own
  replacement for `systemd-bless-boot` and keep using the target.

Constraints: the stock signed boot chain must stay untouched (ADR-0001);
TUR's own code must be separate programs (ADR-0003); nothing in boot may
depend on networking or on a person; nothing is built or booted outside CI
(ADR-0002).

## Options considered

1. **Adopt greenboot as packaged.** Least TUR code. Covers health-check
   failures only, not the early failures a user cannot repair from a
   desktop. No timeouts. Rollback currently depends on an unfixed defect.
2. **Adopt greenboot and extend it.** TUR would arm the counter itself,
   write and clear greenboot's private GRUB variables, wrap every check for
   timeouts and replace the reporting. Those variables were renamed within
   the 0.16 series, which broke rollback across versions upstream. Two state
   machines, one of them not TUR's, in the most critical path.
3. **A small TUR mechanism on GRUB's environment block.** One GRUB fragment
   of about 20 lines, delivered through bootupd the way greenboot's is, and
   one TUR program hooked to `boot-complete.target`. TUR owns the logic and
   its proof. It reuses the variable name and meaning Fedora's GRUB tooling
   and greenboot use, `boot_counter`, so the part written into `grub.cfg`
   at installation stays compatible with them.
4. **Move to a boot path with native counting.** systemd-boot with bootc's
   composefs backend and unified kernel images, or a GRUB patched as
   openSUSE's is. Needs TUR to sign boot components, which ADR-0001 and
   ADR-0013 rule out for now, and bootc does not configure counting on that
   path yet either.

## Decision

TUR implements option 3.

- **Counting.** A GRUB fragment, installed by bootupd into `grub.cfg`,
  decrements `boot_counter` in GRUB's environment block before it starts a
  kernel, whenever the counter is set. At zero it starts the second loader
  entry, which is the previous deployment. It passes the kind of boot
  (`tur.boot=trial`, `tur.boot=fallback`) and the attempts left to the
  kernel command line through GRUB variable expansion.
- **Arming.** The running, known-good version sets the counter to three at
  shutdown, immediately before OSTree finalises a staged deployment.
- **Gate.** `tur-boot-health` runs a fixed list of required checks with time
  limits, ordered before `boot-complete.target`. A unit after that target
  removes the counter. That is the moment a boot is marked good. It does not
  wait for a person to log in.
- **Failure.** During a trial, a failed gate, emergency mode, an initramfs
  failure, a kernel panic and a stopped PID 1 each lead to a restart, by
  plain systemd units, dracut's own settings, the kernel's panic timeout and
  a hardware watchdog. None of these paths runs TUR code. Outside a trial
  nothing restarts the machine.
- **Rollback.** GRUB selects the previous deployment. That deployment, once
  running, makes the choice permanent with `bootc rollback`, records the
  failed image as rejected, and tells the user. The version under trial is
  never trusted to repair anything.
- **State** that must survive a rollback lives in the environment block and
  under `/var/lib/tur/boot-health`, never in `/etc`.
- greenboot is not installed. systemd-boot is not adopted.

The full design, the health checks, the stage mapping, the evidence kept and
the test plan are in the addendum.

## Consequences

- Specification §5.3's systemd-boot paragraph is replaced. The amendments
  are listed in section 14 of the addendum. Open question 1 of §23 is closed
  by ADR-0013 and this record.
- This deviates from "integrate upstream projects rather than forking them"
  and from ADR-0013's expectation that greenboot would be wrapped. TUR does
  not fork greenboot; it declines to depend on it yet. The addendum states
  what would have to be true upstream to revisit this (section 4.3), and the
  question is re-asked at every Fedora rebase.
- TUR owns boot-critical logic that no other distribution exercises. The
  proof in CI is therefore part of the decision, not a follow-up.
- **The GRUB fragment is frozen at installation.** bootupd writes
  `grub.cfg` once. The fragment, the environment variables and the kernel
  arguments are "contract version 1"; changing them needs a new ADR and a
  migration for installed machines.
- **Licence boundary.** `tur-boot-health` and `tur-boot-notify` are separate
  programs under the project licence. The GRUB fragment is interpreted
  inside GRUB and follows greenboot's; it is kept in a marked directory
  under BSD-3-Clause with its own licence file.
- **The installer** (TUR-006) must put `/boot/grub2` on a filesystem GRUB
  can write: ext4 on a plain partition until another layout is tested. TUR
  refuses to apply an update where it cannot count attempts.
- **Every TUR service** must leave shared state under `/var` readable by the
  previous version until the boot is marked good.
- **The updater** (§15.1) must not stage a rejected image automatically, and
  bootc's own update timer is masked.
- Bootloader updates through bootupd are outside this guarantee. They are
  not transactional, and they do not run during a trial.
- **Not recoverable automatically**, and stated as such: a hang before any
  watchdog is armed, or on a machine without a usable one (the user forces a
  restart; forced restarts are counted); a failure of GRUB, shim or the
  firmware's boot entry; loss of the counter together with a version that
  cannot reach userspace; both deployments broken; any failure that appears
  after the boot was marked good, including a user's session.

## Verification

Before anything is built on it, in this order (steps of the addendum's
implementation brief):

- **Step 2 is a go or no-go.** On the real image under UEFI: GRUB can save
  the counter on this layout; `increment` works; a kernel argument written
  as a GRUB variable in `/usr/lib/bootc/kargs.d` survives bootc and OSTree
  and is expanded by `blscfg`. If the expansion cannot be used, the
  contingency in section 5.4 of the addendum applies and this ADR is
  amended before acceptance.
- **Step 4** shows the GRUB half alone: three counted attempts of a
  deployment that panics, then the previous deployment, with no TUR program
  involved.
- **Steps 6 to 11** are the proof required by TUR-005 and §19.5: a good
  update is kept and marked good; a failed health check, a failed required
  unit, a crash-looping display manager, an unmountable root, an early
  panic, a kernel GRUB cannot start, emergency mode, a stopped PID 1 and a
  stalled boot each end on the previous version within three attempts; a
  killed virtual machine during staging, finalisation, marking and fallback
  always leaves one whole version.
- **Secure Boot.** The same counting with Secure Boot enforced, before this
  record's claim about the unmodified signed chain is relied on.
- **Not shown by any of the above**, and recorded as not run until a
  hardware runner exists: real firmware, real hardware watchdogs, a real
  power cut with a drive's write cache.
- Confirm in step 1 what the image enables by default: bootc's update
  timer, bootupd's update service, `systemd-pstore`, GRUB's boot-success
  timer; and whether the initramfs carries a watchdog driver.
- Re-check at each rebase: that `blscfg` still expands variables, that
  bootupd still installs `configs.d` fragments, the shape of
  `bootc status --format=json`, and what greenboot and bootc then offer.

## Acceptance (12 November 2026)

Accepted as the approach to build and test. It stands or falls on step 2 of the
implementation brief: whether a kernel argument written as a GRUB variable
survives bootc and OSTree and is expanded at boot. If that step fails, this
record is revisited before anything further is built on it.


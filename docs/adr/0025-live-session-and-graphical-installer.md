# ADR-0025: The installer medium's live session and graphical installer

- **Status:** Proposed
- **Date:** 2026-11-13
- **Specification sections:** §4.3, §5.2, §6.1 to §6.3, §14.4, §28.3 to §28.5;
  tasks D-23 and TUR-009; amends [ADR-0018](0018-first-installer-medium.md)
- **Amended:** 2026-11-13 by [ADR-0029](0029-install-beside-windows.md): "Use
  the free space beside Windows" is available, on a disk where it is possible

## Context

The first installer medium ([ADR-0018](0018-first-installer-medium.md),
[TUR-007](../reports/TUR-007.md)) starts a text program on the first console
that erases one whole disk and runs `bootc install to-disk`. It is proven in
CI, including beside a Windows-like disk and under Secure Boot. The owner
judged it "pretty primitive" and asked for a live version with a wizard: a
TUR desktop a person can try, from which a graphical installer installs.

The design of that installer exists ([desktop-experience.md](../architecture/desktop-experience.md),
§11.5, task D-23): six screens, a diagnostics drawer, the destructive
confirmation of §28.5, a progress screen of real stages. The account is
created at the first start, not by the installer (§6.2).

Constraints, beyond ADR-0018's:

- AGENTS.md: never a destructive storage operation without explicit
  confirmation showing the exact target and its stable identity. The
  graphical installer must keep every safety rule of the text installer, and
  the two must not drift apart.
- AGENTS.md: a TUR desktop program is a standalone Qt Quick program on the
  project's own controls (`Tur.Kit`), a `QGuiApplication`, importing only what
  `desktop/ALLOWED_IMPORTS` allows.
- A graphical program must not run as root as a whole.
- The installed disk must stay the one ADR-0013 and ADR-0014 were proven on.
- The medium's Secure Boot chain stays Fedora's unmodified shim and GRUB.
- Issue 81 asks whether TUR's root filesystem can carry ext4's `casefold`
  feature, so that Wine prefixes can be made case-insensitive.

### State of upstream, 13 November 2026

Fetched for this decision, not recalled.

| Subject | What upstream says | Source |
|---|---|---|
| First-run setup in a live environment | `plasma-setup.service` carries `ConditionKernelCommandLine=!rd.live.image`: it does not run on a live medium. | [S1] |
| Automatic sign-in with Plasma Login Manager | KDE neon's live images write `[Autologin] User=<live user>` and `Session=plasma.desktop` to `/etc/plasmalogin.conf.d/`. TUR's boot test proved the same keys for one throw-away boot (`tests/boot/splash.py`). Fedora 44 moved every KDE variant to Plasma Login Manager. | [S2], [S3] |
| Fedora's live scripts | `livesys-scripts` configures Fedora's live media at start; 0.9.3 "Refresh[es] Plasma Login Manager to fix configuration for branding and autologin". Its repository on pagure.io is a read-only snapshot since pagure was retired in 2026. | [S4], [S5] |
| KWin and the keyboard layout | KWin reloads the layout when the `Layout` group of `kxkbrc` changes, through `KConfigWatcher`. KConfig announces a change with the D-Bus signal `org.kde.kconfig.notify.ConfigChanged` on `/<file name>`, argument `a{saay}` (group to keys). | [S6], [S7] |
| `bootc install to-disk` progress | bootc 1.17.1 has no progress option for `install to-disk` (only `install reset` has one). It prints its steps as lines: partitioning, `Initializing ostree layout`, `layers needed: N (size)`, `Deploying container image...done`, `Installing bootloader via bootupd`, `Trimming root`, `Finalizing filesystem root`, `Installation complete!`. It makes the root with `mkfs.ext4 ... -O verity`; there is no option for further features. | [S8], TUR-007's logs |
| GRUB and ext4 `casefold` | Fedora 45's GRUB (`rhboot/grub2`, branch `fedora-45`, `grub-core/fs/ext2.c`) supports the incompatible features filetype, extents, flex_bg, meta_bg, 64bit and encrypt, and ignores recover, mmp, csum_seed and largedir. `casefold` (0x20000) is neither: GRUB refuses to open the filesystem ("filesystem has unsupported incompatible features"). A GRUB maintainer: GRUB does not implement casefold, so it cannot read such a filesystem. | [S9], [S10] |
| `tune2fs` and `casefold` | e2fsprogs supports the feature since 1.45; `tune2fs` can switch it off again only after checking no directory uses it (1.47.0). `tune2fs -O casefold -E encoding=utf8` on an unmounted root was measured in CI: accepted (TUR-009). | [S11], TUR-009 |

## Options considered

**The live session.**

1. **Fedora's `livesys-scripts`.** The upstream way, used by Fedora's own
   live media. It does more than TUR needs (it adapts a whole spin), its
   behaviour is spread over scripts whose repository is now a read-only
   snapshot, and it is GPL-3.0 shell code that would run at every start of the
   medium. Not chosen; what it does that TUR needs is four files.
2. **A live user and Plasma Login Manager's own autologin, made when the
   installer environment is built.** `useradd` in the environment's
   Containerfile, one `plasmalogin.conf.d` file, and the live user's settings
   (no screen lock, no wallet, no suspend, the installer opened at sign-in).
   All of it exists only in the installer environment. **Chosen.**

**Where the privileged work happens.**

1. **The whole wizard as root.** Rejected by the task and by good sense: a
   Qt program with a file dialog and a desktop session as root.
2. **A privileged D-Bus service** the wizard calls. A long-running root
   daemon with an interface to freeze, a bus policy and activation files, for
   five calls.
3. **pkexec of one small program, once per step.** The wizard runs as the
   live user. Each step (check, disks, install, sticks, save-log) runs
   `pkexec /usr/libexec/tur-installer/tur-install-engine STEP`, which writes
   JSON lines. A polkit action whose `exec.path` is exactly that program, and a
   rule that allows it to the live user only, on the medium only. **Chosen.**
   The engine trusts nothing it is told: it looks at the disk again (stable
   path, serial number, WWN, size, content fingerprint, not the medium, not in
   use) and derives the confirmation phrase itself from the disk it finds. A
   wizard that misbehaved could not erase a disk it was not shown and
   confirmed for.

**One installation path for both installers.**

1. **Two programs**, the text installer and a new engine, each with its own
   checks. They would drift.
2. **`tur-installer-core` gains an execute mode.** The right end state
   (installer.md), but it is a Rust library whose design has phases, a session
   directory and resume rules that do not exist yet; and the text installer's
   proof would not carry over.
3. **One shell library, `engine.sh`, sourced by both.** The text installer's
   proven functions moved, unchanged in what they do, into
   `/usr/lib/tur-installer/engine.sh`: verifying the payload, the checks, the
   disk scan and its plain words, the phrase, the revalidation, the one
   `bootc install to-disk` command, the check of the result, keeping the log.
   **Chosen.** The text installer's tests run unchanged against it.

**Casefold on the root filesystem (issue 81).**

1. **Set it at installation** (`tune2fs -O casefold -E encoding=utf8` on
   the unmounted root after `bootc install`; bootc 1.17.1 has no option for
   further mkfs features, it passes only `-O verity`). The root holds
   `/boot`, and GRUB reads it at every start. **Measured** (TUR-009, iso run
   43369283112): `tune2fs` accepts it on the unmounted root (e2fsprogs
   1.47.0 on the runner; Fedora 45 has 1.47.4), `dumpe2fs` then lists
   `casefold`, and the disk no longer starts: GRUB 2.12 prints
   `grub_search_fs_uuid: no such device: <the root's UUID>` and stops at its
   prompt. Rejected.
2. **A separate `/boot`, or a separate filesystem for `/var/home`, made
   casefold-capable.** Wine prefixes live under `/var/home`, so either works
   for issue 81. Both are a different disk layout from the one ADR-0013 and
   ADR-0014 were proven on (GRUB's environment block on the ext4 root), and
   `bootc install to-disk` would have to be replaced by `to-filesystem` or
   given a block setup that makes them. Needs its own ADR and the boot and
   rollback proofs again. Not in this change; recommended as the way to
   issue 81.
3. **Case-insensitive storage for Wine prefixes in a filesystem of its own**,
   for example an ext4 image file under `/var` made with casefold and mounted
   by the Windows runtime. Needs nothing from the installer. Recommended to
   the Windows-runtime work (`services/tur-runtime`), not done here.

**Storing the image once (issue 76).** The payload could be a read-only
container store inside the environment's SquashFS, where its files are the
same as the environment's and SquashFS stores them once. That changes the
source `bootc install` reads from (`containers-storage:` instead of `oci:`),
and the overlay storage driver on top of the medium's overlay root needs
proving. Not in this change, which keeps the proven `oci:` source; the medium
stays at about 6.7 GB.

## Decision

The medium's boot menu has two entries. The default, **"Try or install
TUR"**, starts `graphical.target` with `tur.live=1`: Plasma Login Manager
signs `liveuser` in to Plasma, and the session opens TUR's graphical installer
(`desktop/apps/tur-installer`, program `tur-installer-wizard`), which also has
an icon on the desktop. The second, **"Install TUR (text mode)"** (hotkey
`t`), starts `tur-installer.target` with the text installer of ADR-0018.

The wizard is a standalone `QGuiApplication` on `Tur.Kit`, with the screens of
§11.5: welcome (language, keyboard layout, what will happen), this computer,
where to install, your account (made at the first start, §6.2), the
confirmation, installing, and the result; with the diagnostics drawer of
§28.4 on every screen. Its only way to a disk is the installation engine,
started through pkexec for each step. "Use the free space beside Windows"
(issue 64, step 1) is shown and is not available.

Both installers run the same `bootc install to-disk` command, from the same
library. TUR's root filesystem does not get `casefold`.

## Consequences

- A person can try TUR before installing, with the network if they want it;
  the installation itself still uses none.
- The installer environment differs from the payload in more than
  `dracut-live`: a user, its home, `/etc` files of the live session, polkit's
  action and rule, and the wizard's program, two libraries and QML modules
  under `/usr/lib/tur-installer`. No package is added or changed, and
  `build-medium.sh` still hashes every file `bootc install` reads and stops
  if one differs from the payload's.
- The wizard is compiled in the installer environment's build, in a stage
  made from the payload, against the payload's own Qt; its window is loaded
  once in the build, offscreen, on recorded answers, and the build fails on a
  QML warning or a missing library.
- The engine logs every JSON line it sends. The installer's log (the
  engine's log, bootc's output, the wizard's state lines) is copied to the
  installed disk, `/var/log/tur-installer/<session>/`, with the SELinux label
  of `/var/log` read from the disk (§28.3). Before, it was lost at power-off.
- The live user has no password and may run the engine without one. It is
  the person at the computer, on a medium that offers no remote login (sshd
  stays masked). The rule exists only on the medium.
- The installed system is unchanged. The first start still shows the base
  image's first-run setup until `tur-setup` (D-06) exists.
- New error codes, medium only (not yet in the registry crate):

  | Code | Meaning |
  |---|---|
  | `TUR-INSTALL-MEDIUM-004` | The installation engine was refused or started without the rights it needs |
  | `TUR-INSTALL-MEDIUM-005` | The installation engine stopped without a result |

- **Licences.** The wizard and the engine are TUR programs under the TUR
  licence. The boot menu stays BSD-3-Clause. Nothing GPL-only is linked or
  imported: the wizard links Qt Core, Gui, Qml, Quick, Quick Controls and
  D-Bus (LGPL-3.0) and the kit.

## Verification

Shown by the `iso` workflow on every change (results in
[TUR-009](../reports/TUR-009.md)): the medium starts into the live session
with no network device; the wizard is driven through a whole installation
with the keyboard alone (QMP `send-key`), following its state lines on the
serial port; the medium is listed and cannot be chosen; a disk that holds data
(the Windows-like disk) shows the Windows and BitLocker warning, and the erase
button stays unavailable for a wrong phrase and for the right phrase in lower
case; Enter in the confirmation field erases nothing; the Windows-like disk is
byte-identical after the installation and after TUR's first start; the
installed system starts and passes the first-start checks (product medium)
and the boot test's checks and boot-health scenarios (proof medium, installed
with the wizard); the log is on the installed disk; the text installer's
scenarios run as before, through the boot menu's second entry. An
informational run gives an installed root `casefold` and starts it.

The wizard's screens are tested in the `desktop` workflow on recorded
answers of the engine: the kit's contrast walk in both variants, the
keyboard and accessible-name walk in both directions, text at 200 % and
lengthened right-to-left strings, the confirmation's rules, cancelling, and
failures; and photographed in both variants.

**Not shown:** real hardware, real USB sticks, real Windows and BitLocker;
the wizard with a screen reader; saving the log to a real USB stick (the
engine's `sticks` and `save-log` steps have not run in CI); a failed
`bootc install` in a VM; a keyboard layout other than US.

## Sources

| | |
|---|---|
| S1 | https://invent.kde.org/plasma/plasma-setup/-/raw/master/files/plasma-setup.service.in |
| S2 | https://mail.kde.org/pipermail/neon-commits/2026-May/073680.html (KDE neon imager, `0090-plasmalogin`) |
| S3 | https://fedoraproject.org/wiki/Changes/PlasmaLoginManager |
| S4 | https://bodhi.fedoraproject.org/updates/FEDORA-2026-5066c533e8 (livesys-scripts 0.9.3) |
| S5 | https://pagure.io/livesys-scripts (read-only snapshot) |
| S6 | https://invent.kde.org/plasma/kwin/-/raw/master/src/keyboard_layout.cpp |
| S7 | https://invent.kde.org/frameworks/kconfig/-/raw/master/src/core/kconfig.cpp |
| S8 | `crates/lib/src/install.rs` at tag v1.17.1 of https://github.com/bootc-dev/bootc |
| S9 | `grub-core/fs/ext2.c`, branch `fedora-45` of https://github.com/rhboot/grub2 |
| S10 | https://lists.gnu.org/archive/html/grub-devel/2021-12/msg00114.html |
| S11 | https://e2fsprogs.sourceforge.net/e2fsprogs-release.html |

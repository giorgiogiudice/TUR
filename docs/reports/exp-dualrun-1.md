<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-dualrun-1 — the person's own Windows, started inside TUR from its real partitions (experiment)

> **Status: registered and built; no measurement has finished.** Pull request
> 131. The hypotheses were committed first (`01d0df6`), before any run. At
> hand-back (14 November 2026, 03:55 UTC) the runners had about 120 runs
> queued and none of this branch's had started in 50 minutes: the synthetic
> job (workflow `exp-dualrun`, job `lint`, on the pull request's head) and
> the licence-terms stage (run 43549695840, dispatched at 03:04 UTC) were
> queued, and the experiment stage had not been dispatched, because its
> precondition (reading the evaluation's licence terms) had not been met. **Every number in this report
> that would come from CI is marked NOT RUN.** What is in this report today is
> the design, the prototype, the experiment as written, and what was
> established from sources. Section 5 says exactly which run fills which
> table.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-dualrun-1 (time-boxed research spike `dualrun`)
Subsystem:        Windows applications (the last route: the person's own
                  Windows as a guest); storage safety; no TUR service changed
Build ID:         not applicable: this task builds no image
Source revision:  exp/dualrun (pull request 131)

SUMMARY
- What changed: an experiment, not a feature. A prototype launcher
  (tests/experiments/dualrun/tur-dualrun) that inspects a disk holding
  Windows without mounting it, refuses in plain words when starting it would
  be unsafe, gives the guest Windows' partitions and nothing else through a
  device-mapper view, runs sessions on a copy-on-write layer that can be
  thrown away, kept or undone, and writes the guest's firmware boot entry.
  A CI experiment that installs Microsoft's evaluation of Windows 11
  unattended in a virtual machine standing in for a PC, starts the same disk
  as TUR's guest and back, and injects faults.
- Why: the owner asked for the person's existing Windows to run inside TUR
  without a restart, as the route that makes every Windows program work, and
  then (14 November 2026) made it the centre of TUR's direction.

FILES
- Added: tests/experiments/dualrun/** (README with the registered
  hypotheses, tur-dualrun, dualrunlib/, guest/, ci/),
  .github/workflows/exp-dualrun.yml, docs/reports/exp-dualrun-1.md
- Modified: none
- Removed: none

BUILD
- Command: none. Python and shell only; syntax-checked, never run on a
  development machine.
- Result: not applicable
- Warnings: none

TESTS
- Unit tests: NOT RUN (the synthetic job, 30 cases, was queued: workflow
  exp-dualrun, job lint, on the pull request's head commit)
- Integration tests: NOT RUN (stage `experiment` not dispatched: see the
  status above and section 2)
- Boot tests: NOT RUN (nothing here changes TUR's boot)
- Hardware tests: NOT RUN (no hardware runner; the owner's PC procedure is
  section 10)
- Performance tests: NOT RUN

DIAGNOSTICS
- Log paths: CI artifacts exp-dualrun-synthetic, exp-dualrun-terms,
  exp-dualrun-experiment (results.json, results.md, per-start logs)
- Diagnostic bundle path: not applicable
- Relevant error IDs: TUR-DUALRUN-001 to -072 (experiment-local, section 3.4)

RECOVERY
- Rollback procedure: delete the branch; nothing outside
  tests/experiments/dualrun, the workflow and this report is touched
- Recovery tested: NOT RUN (the experiment's own fault injection is
  section 5.3)

KNOWN LIMITATIONS
- Section 13.

NEXT SAFE ACTION
- Let the queued synthetic job and the `terms` stage (run 43549695840)
  finish; fix what the synthetic job finds; read the terms the other
  prints (its job summary and artifact exp-dualrun-terms); if they plainly
  allow evaluation and testing use, dispatch workflow exp-dualrun on
  exp/dualrun with stage=experiment and terms_read=yes; then fill
  section 5. Nothing on the owner's PC before that, and then only steps 1
  to 3 of section 10, which cannot write to his disk.
```

## The answer

Not known yet. The experiment that would answer H1 (round trips without
damage) and H2 (reliable refusals), which everything else stands on, is
written and waiting for a runner. What can be said today, from sources and
from the design work:

- **The idea is buildable from parts Fedora ships**, with no kernel patch and
  no change to Windows: QEMU/KVM with an emulated NVMe or AHCI disk, an Intel
  network card, xHCI, a 16550 serial port and Hyper-V's bus, all of which
  Windows has drivers for out of the box. No virtio device is needed to
  start, and none may be boot-critical afterwards.
- **The dangerous parts have a design that does not depend on Windows
  behaving**: the guest never gets the disk, only a view in which TUR's own
  partitions have no mapping at all; the kernel itself refuses to mount a
  partition the guest holds; and a session on a layer never writes to the
  real partitions until the person keeps it.
- **Two findings change the plan of ADR-0015 for this route.** (1) The
  guest's emulated NVMe controller could not be saved and restored in QEMU as
  of the versions read (a patch series was at v10 in June 2026), so "resume
  from a saved state" may need AHCI or a newer QEMU; this is measured, not
  assumed. (2) An unmodified Windows has no virtio drivers, so the agent
  channel, memory return and shared folder of ADR-0015 (virtio-serial,
  virtio balloon, virtiofs) are not available at first start; the experiment
  uses a serial port, Hyper-V's dynamic-memory device and Windows' own file
  sharing instead.
- **Windows 10 cannot be tested in CI lawfully**: Microsoft no longer offers
  an evaluation of it. The owner's PC is the Windows 10 test.

## The experience

The owner's words (14 November 2026): TUR merges with Windows; with a single
boot the person still has their Windows, now supercharged; C: stays C:; the
desktop looks like theirs; "TUR just gave Windows some superpowers"; people
can still boot only their Windows; one identity shared, with Windows as its
home when Windows is present. This section designs for that. Each part says
what is measured by this spike, what is design, and what is an honest limit.

### One boot

The person switches the computer on and arrives at their desktop. No menu,
no second sign-in, nothing that says "virtual machine".

| Step | What happens | Budget (target, not a result) | Measured here |
|---|---|---|---|
| 1 | Firmware starts TUR (the boot order of pull request 112: the person chose TUR as default) | firmware | no |
| 2 | TUR starts to its sign-in screen | TUR's boot budget (ADR-0014's tests) | no |
| 3 | The person signs in once, to TUR | — | no |
| 4 | While the desktop appears, `tur-guest` runs the check-list (under 2 s: a few dozen reads from the disk) and starts or resumes the Windows side in the background | 45 s cold, 10 s from a saved state | cold start in CI (nested, so slower); whether a saved state can be resumed at all |
| 5 | Windows applications appear in the launcher at once (from the list cached at the last session) and open when the guest is ready; one already open at the last close can be shown as "resuming" | — | no |

Why the Windows side should not block step 3: TUR's own desktop is usable
from second one, and most sessions begin in a browser. The guest is a
background service of the desktop, like the Android runtime.

**Near-instant start.** Three ways, in order of risk:

1. *Keep the guest's memory in a file* (QEMU's own save, encrypted with a key
   in the person's keyring, bound to the disk's state token of section 6.2):
   resume in seconds. The hard rule: **a saved state is valid only while no
   one else has touched the Windows partitions.** If Windows was started by
   itself in between, the saved state must be thrown away, never resumed
   (resuming old memory over a changed disk destroys the filesystem). The
   launcher's token detects that. H7 measures whether QEMU can save a guest
   with the controller in use.
2. *Windows' own hibernation inside the guest.* Rejected: a hibernation image
   made on virtual hardware would be found by the next native start. The
   guest's firmware therefore declares no S3 and no S4, which also makes
   Fast Startup impossible inside the guest without changing Windows'
   setting (section 3.5).
3. *Start at sign-in, never stop while TUR runs.* Costs memory; with memory
   return (section 5.5) an idle Windows can be squeezed.

**Sign-in, honestly.** One password for both sides is possible only in part.

| The person's Windows account | Inside TUR | What TUR does |
|---|---|---|
| Local account with a password, or Microsoft account with a password | Works | The person enters the Windows password once; TUR keeps it in the person's keyring (encrypted under the TUR sign-in, never in the clear, never in a profile or log) and presents it to Windows: as Remote Desktop credentials where the edition has it, or typed at Windows' sign-in screen through the virtual keyboard. Offer to make the TUR password the same; never require it |
| PIN, face or fingerprint (Windows Hello) | **Does not work in the guest.** Hello's keys live in the computer's security chip; the guest has a virtual one. Natively they keep working | Windows asks for the password instead. Said before set-up |
| Microsoft account with no password at all | **Cannot sign in inside TUR** until the person adds a password or a local sign-in in Windows | Detected at the first session; one page explains it and how to add a password |
| A saved state (way 1 above) | No sign-in at all: the session was never closed | The saved memory holds an unlocked session, so it is encrypted at rest and bound to the TUR sign-in |

NOT RUN: all of this table (CI uses a local account with a password and
automatic sign-in set by the test rig). Whether handing the real security
chip to the guest would let Hello work there is an open question with a
serious cost (TUR then shares its own chip with a less trusted system) and
is not recommended without a security review.

### Volumes keep their identity

**Inside Windows nothing changes.** The view (section 3.2) presents the same
disk identity, the same partition table and the same partitions at the same
offsets, so Windows' own records of which volume is C: and which is D: (they
key on the disk and partition GUIDs) still match. H8 measures it: every
letter and label in the guest equal to the native ones.

**TUR's own partitions never get a letter.** What decides that is the
partition type GUID, not the filesystem:

| Partition as TUR makes it | In Explorer | In Disk Management | Verdict |
|---|---|---|---|
| Type "Linux root (x86-64)" `4f68bce3-…`, ext4 (pull request 112 today) | Nothing (expected; H8 measures it natively and in the guest) | A partition with no letter and no filesystem name | Keep |
| Type "Linux filesystem" `0fc63daf-…` | The same | The same | Acceptable |
| Type "basic data" `ebd0a0a2-…` holding a Linux filesystem | **A letter, and "You need to format the disk"** | RAW | Never. The launcher also refuses to hand such a partition to the guest (it reads the content, not just the type) |
| A FAT, exFAT or NTFS volume made by TUR | A letter | A volume | Only as a deliberate shared volume with a plain label ("Shared with TUR"); never for TUR's system |

What pull requests 112 and 116 must change to honour this (they create the
partitions): (1) nothing about the type, it is already right; (2) set the GPT
"required partition" attribute (bit 0) on TUR's partition, so that Windows'
Disk Management does not offer to delete it (to be confirmed on real
Windows; uninstalling then uses `diskpart`'s `override`); (3) keep the
partition name "TUR" (Disk Management does not show GPT names, `diskpart`
and third-party tools do); (4) never create a second EFI system partition or
any FAT/NTFS volume for TUR's own use; (5) record the Windows volumes'
letters at install time only as a cache: the source of truth is Windows'
own `MountedDevices` record, read fresh (the `inwin` spike publishes the
format).

**On TUR's side the person sees the same names.** "C:" and "D:" with their
Windows labels, in the file manager and in file dialogs, never
`/run/tur/volumes/fs-01d9…`. The labels come from the volume itself (the
launcher's reader returns them without mounting); the letters from Windows'
record. A path pasted as `C:\Users\Ann\Documents\x.docx` is accepted where a
person would paste one (file dialogs, the launcher, P@x) and resolved
through the same mapping the Wine tier already uses for drive letters
(storage design, section 6). TUR's own system volumes are not listed in
everyday views at all.

### The desktop looks like theirs

Three ways were asked for. The recommendation is **(c): TUR's desktop
carrying their choices by default, their real Windows desktop one key away,
and the real desktop as the main face only on hardware that can give Windows
a graphics card.**

**(a) Their real Windows desktop is the face.** The guest's desktop full
screen; TUR's Linux, Android and re-hosted applications appear inside it as
ordinary Windows windows.

- *Is that possible on every edition?* The piece it needs on the Windows
  side is the Remote Desktop **client** showing remote application windows.
  That client (`mstsc.exe`) is in every edition, Home included; only the
  Remote Desktop *host* is missing from Home. Microsoft's own WSLg works
  this way on Windows 10 and 11: a Weston compositor with an RDP backend
  that Microsoft extended to remote single applications, built on FreeRDP's
  server library, shown by `mstsc.exe` started "in silent mode" (WSLg's
  README, read 14 November 2026). So TUR's side can serve its application
  windows to the guest the way WSLg serves Linux applications. **Costs:**
  the application-remoting shell is in Microsoft's fork of Weston (MIT), not
  in upstream Weston, so TUR would carry it or use xrdp's implementation;
  every TUR window is then encoded, sent and drawn by Windows; and TUR's
  applications lose their place on KWin (no KWin effects, P@x's overlay
  would have to be a remote window too). NOT RUN.
- *The weakness, plainly:* **everything the person sees is then drawn inside
  a virtual machine.** Without a graphics card of its own, Windows draws
  with the processor (the "Basic Display Adapter"), which is slower than
  their Windows was, for everything, all day.

Display paths that exist for a Windows guest today:

| Path | Needs in Windows | Quality | Status |
|---|---|---|---|
| Standard VGA, shown by QEMU | nothing | Unaccelerated, one screen, fixed modes; fine for forms, poor for video | What this experiment uses |
| Remote Desktop (whole desktop or RemoteApp) | Pro, Enterprise or Education | Windows' own remote display driver; good for office work; 3D on the processor | ADR-0015's default; H5(a) |
| QXL or virtio-gpu 2D | a driver from virtio-win | 2D only; no 3D driver for Windows guests (ADR-0015: unmerged since 2023) | Changes Windows |
| A graphics card handed to the guest | the card's normal driver | Native speed | See below |

Handing a graphics card over: with **two cards** (a desktop with an
integrated and a discrete one, or two discrete) TUR draws with one and
Windows gets the other; the picture comes back through a second cable or
through shared memory (Looking Glass, which needs a guest program and
optionally a host kernel module; ADR-0015 keeps it an expert option). With
**one card**, giving it to Windows means TUR has nothing to draw with while
Windows runs: that is no longer one desktop. **Laptops** with an integrated
card driving the panel and a discrete one without outputs can hand the
discrete card over in principle, but the picture must then come back through
shared memory, the card's firmware often refuses to start in a guest, and
battery life suffers. Splitting one card between host and guest exists only
as vendor features (Intel's SR-IOV on recent integrated cards, NVIDIA's
licensed vGPU) whose state on Fedora 45 was not verified. So (a) at native
speed is a desktop-with-two-cards feature today, not the default.

**(b) TUR's desktop carries their desktop over.** At install and at every
start, TUR reads from the Windows installation the wallpaper, the desktop
icons and their positions, taskbar and Start pins, the user folders as the
very same files, default applications and file associations, accent colour
and light or dark, and arranges TUR's desktop the same way. Windows
applications appear as seamless windows (H5); Windows itself starts only
when one is needed. Faster and lighter. Where it would feel wrong: it is
TUR's shell wearing their choices, so everything that is Windows' *own*
interface is not there (the Start menu's layout and search, Settings,
Explorer's ribbon and context menus, the notification centre, Windows key
shortcuts that open Windows panels). A person who goes looking for "This PC"
or right-clicks the Start button meets TUR. **TUR may reuse the person's own
files and settings; it must not copy Microsoft's interface artwork (icons,
sounds, the Start logo, default wallpapers that ship with Windows are
Microsoft's works even though they sit on the person's disk: show them from
the person's volume in place, do not copy them into TUR or ship look-alikes)
and must not imitate Windows' trade dress.** What can be read, and in which
formats, is the `inwin` spike's subject; its findings are to be used here,
not duplicated.

**(c) The combination, recommended.** (b) by default; their real Windows
desktop, whole, one key away (a full-screen view of the guest's own shell,
for everything (b) cannot wear); and on a computer with a second graphics
card, an option to make the real desktop the main face. This keeps the
default light and honest, and keeps the real thing within reach.

### It must feel like a gain, never a loss

| What could make their Windows feel worse | Prevented, mitigated, or an honest limit |
|---|---|
| Slower graphics inside TUR | **Limit**, shown before installing: "Windows inside TUR draws without the graphics card. Office work, yes; video editing, 3D and games, use Restart into Windows." Mitigated with a second card |
| Less memory | **Mitigated**: memory is given to the guest on demand and taken back when idle (H4, Hyper-V's dynamic memory, no driver added); refused with a plain message when there is not enough (TUR-DUALRUN-040). Below 8 GB of memory the set-up says the Windows side will be tight |
| Games and anti-cheat | **Limit**: many anti-cheat systems refuse a virtual machine. Games route to Proton or to Restart into Windows (as ADR-0015 already says) |
| Activation prompts | **Mitigated** by carrying the computer's identity (H3). If Windows still treats the guest as another computer, that is shown before set-up, with what Microsoft's terms say (section 11). Nothing is done to silence activation |
| BitLocker recovery prompt at every start | **Limit, said before set-up**: a BitLocker volume asks for its 48-digit key inside TUR each time; TUR can keep the key in the keyring if the person enters it, and types it. Never bypassed. Many Windows 11 PCs have device encryption on without the owner knowing: the installer must detect it (the launcher's reader does) and say so |
| Windows Hello stops working in the guest | **Limit**: password inside TUR, Hello natively (above) |
| A peripheral that needs its Windows driver | **Mitigated**: the device is handed to the guest (USB) and Windows' own driver runs there; devices on other buses stay with Restart into Windows |
| Windows "found new hardware" every time | **Measured** (H1, H3): device re-detection per start, first and later. The two hardware sets are both remembered after the first round trip, if H1 holds |
| Windows starting slower by itself afterwards | **Must not happen**: H10 measures it; nothing boot-critical is added |
| Fast Startup must be off, or TUR refuses | **Mitigated**: the refusal says exactly what to do (Restart, not Shut down). The set-up asks to turn Fast Startup off in Windows, the one setting TUR asks the person to change |
| Fear of losing data | **Prevented by default** if the owner accepts the proposal that the first sessions are protected ones (section 6), and by the backup advice of section 10 |

### Windows must always remain startable by itself

Design rules, with what is measured:

1. **Windows' boot files and firmware entry are never written by TUR.** The
   guest has its own firmware variables; the launcher adds a boot entry
   there, never in the computer's firmware. H10 compares every file on the
   EFI system partition and the stand-in PC's firmware entries before the
   first and after the last guest session.
2. **Nothing boot-critical is added to Windows.** No driver is installed at
   all in this experiment. If drivers are added later (virtio for speed),
   they must be optional and demand-start, and Windows must still start on
   the plain controller both natively and in the guest. H10 compares the
   list of boot-start drivers.
3. **TUR's software inside Windows is idle when Windows runs by itself.** The
   test agent exits at once when it finds no host port. The product agent
   must do the same on a positive sign from the host (a handshake), never on
   the mere presence of a port. It listens on no network port.
4. **The person may make Windows the default and rarely start TUR**
   (`tur-dualboot default windows`, pull request 112). Then nothing of TUR
   runs at all. The finding of pull request 112 stands: a later shim update
   overwrites Windows' fallback loader on the shared partition again;
   `tur-dualboot` should put it back after such an update, and the guest
   does not depend on that file (section 4.3).
5. **Removing TUR leaves Windows exactly as it was.** An uninstall path from
   both sides, to be built with the installer, not here: from TUR, "Remove
   TUR" sets Windows first in the boot order, deletes TUR's firmware entry
   and its files on the EFI system partition (by the checksums recorded at
   install), puts Windows' fallback loader back, and after the restart a
   small Windows program (or three documented `diskpart` lines) deletes
   TUR's partition and extends C:. From Windows, the same program does all
   of it. Layers, undo records and saved states live on TUR's partition and
   go with it; **an open layer must be kept or thrown away first**, which
   the uninstaller checks. NOT RUN.

### One identity, shared, with Windows as its home

The person is one person on both sides. Because a natively started Windows
cannot read TUR's filesystem, the shared identity lives on the Windows side
whenever Windows is present, and TUR reads it fresh at every start.

| Kind of thing | Source of truth | Direction | Conflict |
|---|---|---|---|
| Files: Desktop, Documents, Downloads, Pictures, Music, Videos | The Windows volume (the person's profile folders, wherever Windows' own record says they are, including a redirected or OneDrive folder) | Both ways, the very same files: TUR's user folders *are* those folders | None: one copy |
| Name and picture | Windows' account | Windows → TUR, at each start | Windows wins |
| Wallpaper, accent colour, light or dark | Windows' settings | Windows → TUR at each start; TUR → Windows only by asking the agent to set them while the guest runs | Last change wins; a change made in TUR while the guest is down is applied at its next start |
| Desktop icons, pins, shortcuts | Windows' records (formats: `inwin`) | Windows → TUR; TUR's own pins (Linux, Android applications) are kept beside them in TUR and are not written into Windows | Windows' set wins for Windows items |
| Installed Windows applications | Windows (the agent's list, cached) | Windows → TUR | — |
| Default applications | Each side has its own; a file type whose Windows default is a Windows program opens in the guest | — | The person's last explicit choice in TUR |
| Passwords and keys | Each side its own store. The Windows sign-in secret only is kept in TUR's keyring | Never synchronised | — |
| P@x's memory, policy store and audit trail | **TUR's side only** | Never written to the Windows volume, never writable from the guest | — |

When Windows is started by itself the person sees nothing of P@x's history:
that is by design, and the set-up says so. Without a Windows installation
TUR's identity has the same shape (the same folders, on TUR's own volume),
so adding or removing Windows later moves the folders' backing store and
nothing else.

**The hard part: the files live on a volume two systems must never mount at
once.** The mechanism proposed:

- **One stable path on TUR's side**, whichever way it is served:
  `~/Documents` and the others are mount points owned by a small TUR service
  (a FUSE filesystem, or an autofs point).
- **While the Windows side is down**, the service serves them from the
  volume itself, mounted by the kernel's `ntfs3` driver, read/write only
  when the launcher's reader says the volume is clean and not hibernated
  (the same check-list; storage design section 8 proposed exactly this
  check and had no detector until now).
- **While the Windows side is up**, TUR must not mount the volume (the view
  holds it, and the kernel refuses). The service then serves the same paths
  **through the guest**: Windows' own file sharing (present in every
  edition, Home included) over the private virtual link, or TUR's agent
  where sharing is unavailable.
- **The switch**: the guest starts only after the service has flushed and
  released the volume; files open on TUR's side at that moment are the
  problem. Proposed rule: the service keeps a handle table and re-opens each
  handle by path on the other backing once it is up (reads and whole-file
  saves survive; a program holding a byte-range lock or a memory map across
  the switch gets an error). Starting the guest waits, with a visible
  reason, for programs that hold such things.

What breaks or changes, to be stated in the product: file locking differs
between the two backings; through the guest every access is a network round
trip (large trees, many small files and file watching are slower, and
`inotify` becomes polling or the share's change notification); names differ
only in case collide (`ntfs3` with `windows_names` on one side, Windows'
rules on the other); Linux permissions and symbolic links are not
representable either way (the folders behave like the storage design's NTFS
row); paths longer than 260 characters and names Windows reserves are
refused; and with BitLocker the first backing does not exist at all: the
files are reachable only while the Windows side runs, unless the person
unlocks the volume on TUR's side with the recovery key (possible with
cryptsetup's BitLocker support; its write safety was not reviewed).

H9 tests the cheapest telling case: a file written by TUR while the guest is
down is seen by the guest; one written through the guest is on the volume
afterwards; the volume is clean both times, by the launcher's reader and by
Windows' own check. The switching service is design only.

**How Linux, Android and re-hosted applications sit beside the Windows
windows.** On the recommended desktop (c) they are all KWin windows: Linux
applications natively; Android applications through the Android runtime
(spike `exp/android`, ADR-0006); applications re-hosted from their Windows
or macOS packages onto a Linux runtime (spike `exp/rehost`) as native
windows that never needed Windows at all; Windows applications that Wine
runs as Wine windows (spikes `exp/wincli`, `exp/windiff`); and only the rest
as guest windows from this route. One launcher, one task switcher, and the
application's properties always name the route (specification §3.4).

## 1. What was registered, and what ran

The registration is `tests/experiments/dualrun/README.md` at commit
`01d0df6`, before any measurement. Hypotheses H5 to H10 were added to it in
the same first commit after the owner's additions of 14 November arrived
during the first hours; nothing had run.

| | Hypothesis | Kill criterion | Result |
|---|---|---|---|
| H1 | A natively installed Windows starts unmodified as a guest from the same disk and natively again, repeatedly | Repair needed in either direction | **NOT RUN** |
| H2 | Reliable refusals; the guest gets only Windows' partitions; a kill leaves Windows no worse than a power cut | A state that must be refused is accepted; a byte outside the given partitions changes; damage Windows cannot repair itself | Synthetic part **NOT RUN (queued)**; real-Windows part **NOT RUN** |
| H3 | Identity can be carried; with it Windows reports fewer changes | none (decides only the explanation) | **NOT RUN** |
| H4 | Memory return, USB hand-over and clean shutdown with built-in drivers | None of them possible without added drivers | **NOT RUN** |
| H5 | Each Windows application as its own host window: (a) RemoteApp, (b) TUR's own helper | (a) no separate windows; (b) hidden content cannot be obtained without Remote Desktop | **NOT RUN** |
| H6 | Protected sessions: discard leaves every byte; commit is seen natively; interrupted commit completes; undo; repair from outside | A discarded session changes a byte, or a kept one needs repair | Synthetic part **NOT RUN (queued)**; real-Windows part **NOT RUN** |
| H7 | The guest's start fits a budget; a saved guest can be resumed | No controller Windows starts from can be saved | **NOT RUN** |
| H8 | Letters and labels unchanged in the guest; TUR's partition has no letter | Any differs | **NOT RUN** |
| H9 | The same files both ways, the volume clean both times | A write missing, or the volume dirty or damaged | **NOT RUN** |
| H10 | After guest sessions Windows by itself is unchanged: speed, boot drivers, boot files, firmware entry | A boot-critical change | **NOT RUN** |

## 2. A real Windows in CI, lawfully

Written before use, in the registration; repeated here with what was found
on 14 November 2026.

| Question | Finding | Source |
|---|---|---|
| Does Microsoft publish an evaluation of Windows 11? | Yes: Windows 11 Enterprise, version 26H2, 90 days; also Enterprise LTSC 2024. "Register, then download and install the full-featured software for a 90-day evaluation." No product key. "Designed for IT professionals" | [S1], [S2] |
| Can CI fetch it from Microsoft's own server by a stable link? | Yes. The download page lists `go.microsoft.com/fwlink` links per language; the English x64 one redirects (302) to `software-static.download.prss.microsoft.com/dbazure/26300.9457.260913-1737.26h2_ge_release_svc_refresh_CLIENTENTERPRISEEVAL_OEMRET_x64FRE_en-us.iso`. The registration form is on another page and is not enforced by the link | [S1]; a HEAD request, 14 November |
| Size | 8,225,329,152 bytes (7.66 GiB), range requests accepted, last modified 17 September 2026 | the same request |
| Its terms for automated testing | **Not established.** The page does not publish the evaluation's licence terms; they are inside the medium and shown by Setup. Microsoft's use-terms site lists retail and OEM terms; no evaluation entry was found in the part read. Stage `terms` extracts the text from the medium and prints the paragraphs that govern use, installing nothing. **It had not run at hand-back.** | [S1], [S3] |
| Windows 10 | **No evaluation is offered any more.** The Windows 10 Enterprise evaluation page answers 301 to a 2024 blog post about the end of Windows 10 support. No other lawful automated source was found; old direct links, if any still answer, would be media Microsoft has withdrawn from evaluation and were not tried | [S4] |
| GitHub's Windows runners | Allowed by ADR-0015's amendment for the guest-side helper. They cannot stand in here: the experiment needs one installed disk started under two machine configurations | ADR-0015 |

How the medium is handled is in the registration (never committed, cached or
uploaded; `ci/artifact-guard.py` removes from the artifact directory any
file with the signature or extension of an executable, hive, event log,
image, archive or RTF document, and anything over 8 MiB, and lists what it
removed). The run is dispatched by hand with `terms_read=yes`; the answer
file's `AcceptEula` accepts the evaluation's terms on behalf of whoever
dispatches. The task allowed this use in CI; what it could not settle is
what the terms themselves say, because they could not be read in advance.
If stage `terms` shows terms that do not plainly allow evaluation and
testing, or that are unclear about an automated run, the judgement is the
owner's (section 14, decision 1), H1 to H10's Windows parts stay NOT RUN in
CI, and the owner's PC procedure (section 10) is the experiment.

No product key is entered, no activation or evaluation timer is reset or
extended, Setup's hardware checks are met (a software TPM 2.0, firmware with
Secure Boot support) and not bypassed, and the guest has no route to the
Internet.

## 3. The safety design

### 3.1 The check-list

Run before every start, by `tur-dualrun check` (`dualrunlib/checklist.py`).
Everything is read from the disk opened read-only, with reads that bypass
the page cache: the guest writes through device-mapper with direct I/O,
which does not invalidate what a buffered reader cached earlier, and a
safety check must not decide on stale blocks. Nothing is mounted.

| # | What is read | From | Refusal |
|---|---|---|---|
| 1 | The partition table: both checksums, the backup at the end of the disk, no overlap | LBA 1 and the entries | 003 |
| 2 | Exactly one EFI system partition, holding `EFI/Microsoft/Boot/bootmgfw.efi` | a FAT reader | 002 |
| 3 | A Windows system volume: NTFS with a `Windows` directory, or a BitLocker volume | boot sectors, the NTFS root directory | 001 |
| 4 | **Hibernation in the volume**: the first bytes of `\hiberfil.sys` (`hibr`/`HIBR` = a valid image: hibernation, or the kernel image Fast Startup leaves; `wake`, zeros, no file = none; anything else = unknown) | the root directory index, the file's record and its first cluster | 010; unknown: 013 |
| 5 | **Hibernation in the boot configuration**: the boot manager's "attempt resume" element (`26000005`) in the BCD store, which is on the unencrypted EFI system partition and so readable even with BitLocker | a registry-hive reader | 011 |
| 6 | BitLocker: the `-FVE-FS-` signature | boot sector | 030 unless the person says they hold the key; then 5 alone decides hibernation, and unknown is 013 |
| 7 | The journal: the restart pages of `$LogFile` (clean = the "volume is clean" flag or no client in use, ntfs-3g's rule) and the volume's dirty flag | file records 2 and 3 | **Not a refusal** (see below); a note |
| 8 | Every partition to be given: not mounted, not swap, no holder, and the kernel's own answer to an exclusive open | `/proc/self/mountinfo`, `/proc/swaps`, sysfs, `open(O_EXCL)` | 020 |
| 9 | A Linux filesystem inside a partition typed as Windows data (older Linux installers did this) | ext, Btrfs, XFS, LUKS, swap signatures | not given to the guest; a note |
| 10 | Free memory: the guest's share plus what TUR keeps | `/proc/meminfo` | 040 |
| 11 | Hardware virtualisation | `/dev/kvm` | 041 |
| 12 | No session and no unfinished layer on this disk | the view's name, the state directory | 050, 070 |

Two deliberate choices:

- **A volume that was not shut down properly is not refused.** After a power
  cut, a native start replays the journal; so does a start inside TUR. If
  TUR refused, a crash of TUR itself would force the person to restart into
  Windows to repair what the guest could repair. Only hibernation is refused,
  because only there does starting destroy something (the saved session) or
  wake a memory image on hardware it was not made on. The same reader gives
  the storage service what it lacked (storage design, section 8): for
  *mounting* on TUR's side, unclean and dirty remain read-only.
- **There is no force switch.** One experiment-only switch exists, valid in
  protected mode only, to measure what Windows does when a Fast Startup
  volume is started anyway on a layer that is then thrown away.

### 3.2 What the guest is given, exactly

The guest never gets the disk. It gets one device-mapper device
(`dualrunlib/session.py`) with the disk's size and layout, built from the
partition table:

| Range of the disk | Mapped to | Why |
|---|---|---|
| First sectors up to the first partition (protective MBR, partition table) and the backup table at the end | A private copy in a file ("the shadow"), byte-identical at the start | Windows sees the same disk GUID, partition GUIDs and offsets as natively. Nothing the guest writes to the table reaches the real one; a change is detected at the end and reported |
| The EFI system partition, the Microsoft reserved partition, every Windows data partition whose content is NTFS, BitLocker, FAT, exFAT or ReFS, Windows recovery partitions | `linear` onto the real partition | These are Windows' |
| Every other partition (TUR's root, other systems) and every unallocated range | Reads as zeros, writes fail (`flakey … error_writes` over a `zero` device); `zero` or `error` where the target is missing | **No mapping to those blocks exists.** A fault in Windows, or an escape from QEMU as the user, cannot reach them. Zeros, not errors, on read, so that a probing read does not fill Windows' event log with disk errors |

The partition table is shown unchanged, TUR's entries included, so that the
guest's view of the disk is the native one. If Windows changes the table
inside a session (its updates sometimes resize the recovery partition), the
change lands in the shadow, is reported at the end, and is **not** applied
by this prototype. Whether to apply a change that touches only Windows'
entries and ranges, with the person's confirmation, is decision 6.

### 3.3 Locking

Device-mapper opens each mapped partition exclusively. While the view exists
the kernel refuses to mount those partitions (and any other exclusive open);
creating the view fails if one is mounted. The check of row 8 is therefore a
courtesy that produces a readable refusal; the kernel is the lock. The
view's name is derived from the disk's GUID, so a second session on the same
Windows cannot be created. TUR's storage service must treat "held by the
view" as a reason of its own (`TUR-STORAGE-…`: "in use by Windows inside
TUR"), and `tur-dualboot restart-windows` must refuse while a view, an open
layer or a saved state exists.

The shared EFI system partition (pull request 112) is given to the guest
whole and writable, as natively: Windows keeps its boot configuration there.
TUR's boot loader lives on it too. That is the same exposure as dual boot,
not a new one; but bootupd must not update the partition while the guest
runs (it will fail on the kernel's lock; it should wait).

### 3.4 What the person is told

Each refusal has a code, a title, a reason and an action, in the form of the
storage design. The texts are in `dualrunlib/checklist.py`; a test asserts
that none contains the word "force". The two that matter most:

```text
TUR-DUALRUN-010
Windows is asleep on its disk, not shut down.

Reason:
Windows was hibernated, or was shut down with Fast Startup, which saves part
of its memory to the disk. Starting it inside TUR now would throw that away
or wake it on a computer it does not recognise, and could damage files.

Action:
Start Windows by itself, then choose Restart (not Shut down), and come back
to TUR. To stop this happening again, turn off Fast Startup in Windows'
power options.

Technical detail:
partition 3 (Windows): hiberfil.sys begins with 'HIBR' (… bytes); journal
clean: False
```

```text
TUR-DUALRUN-030
This Windows is locked with BitLocker.

Reason:
The key that unlocks it is sealed inside this computer's security chip and
is released only when Windows starts by itself. Started inside TUR, Windows
will ask for its 48-digit recovery key every time. TUR does not have that
key and does not try to get around the lock.

Action:
Find the recovery key first (for a Microsoft account:
account.microsoft.com/devices/recoverykey), then start again and say that
you have it. Or use Restart into Windows.
```

The others: 001 no Windows found; 002 Windows' start-up files not found;
003 partition table unusable; 011 set to wake from hibernation at next
start; 013 could not tell whether Windows was shut down completely; 020 a
Windows partition is in use by TUR (names the mount point); 040 not enough
free memory; 041 no hardware virtualisation; 050 already running or not
closed properly; 070 a protected session is waiting to be kept or thrown
away; 072 Windows has changed since this was saved.

A note for the product: "Restart, not Shut down" is exact. With Fast Startup
on (Windows' default), *Shut down* leaves the hibernated kernel; *Restart*
never does. So "I restarted from Windows into TUR" always passes, and "I shut
Windows down yesterday" does not. The set-up should ask the person to turn
Fast Startup off, the one Windows setting TUR asks for.

### 3.5 A crash or power cut while the guest runs

- **The guest's firmware offers no sleep states** (`ICH9-LPC.disable_s3`,
  `disable_s4`): Windows inside TUR cannot hibernate and cannot use Fast
  Startup, whatever its setting says, so a guest session can never leave a
  hibernation image made on virtual hardware for the next native start to
  find. Windows' setting itself is not changed. Whether a session without
  S4 makes Windows drop its hibernation file or setting is measured (the
  power facts before and after).
- **Direct session**: QEMU opens the view with `cache=none`; a completed
  write is on the disk. Killing QEMU is a power cut for Windows: an unclean
  journal, replayed at the next start, natively or inside TUR. The fault
  injection of section 5.3 kills at four moments and checks with Windows'
  own tools.
- **Protected session**: nothing reaches the disk until commit. A crash
  leaves the disk exactly as before the session and a layer that holds a
  crash-consistent state, which can be thrown away or (not built) offered
  for recovery.
- **The launcher dies, QEMU lives**: `tur-dualrun recover` finds the guest by
  its record, asks it to shut down through QEMU's control socket, waits,
  and removes the view. With libvirt this case does not exist (the guest
  does not depend on the manager's process); the prototype shows the
  minimum.

## 4. The launcher

### 4.1 QEMU directly, or libvirt?

The prototype drives QEMU directly (`dualrunlib/qemu.py`) so that every
device and every byte of state is visible to the experiment. **TUR should
use libvirt's session instance, as ADR-0015 already decides**, for the same
reasons (SELinux confinement of QEMU as the user, the TPM and firmware
handling, a guest that survives its manager), with one addition this route
needs: **a small root helper** behind polkit that does only three things:
run the check-list, create the view and hand its device node to the user,
and remove it. QEMU itself never runs as root and never opens the disk.
Everything else (layers are files, the firmware variables are a file) is
the user's. The domain definition is generated per session from the view's
record; the items that differ from ADR-0015's profile are in section 4.2.

### 4.2 The machine an unmodified Windows starts on

| Item | For this route | ADR-0015's profile (a guest installed for the purpose) |
|---|---|---|
| Disk | The view, on emulated **NVMe or AHCI, matching the controller class Windows was installed on** (H1x measures the mismatch). Raw, `cache=none`, discard passed through | A qcow2 image on virtio |
| Network | Intel e1000e (driver built in) on a private user-mode link | virtio on passt |
| Agent channel | A 16550 serial port (driver built in), or TCP on the private link | virtio-serial |
| Memory return | Hyper-V's dynamic-memory device (`hv-balloon`, in QEMU since 8.2) on Hyper-V's bus; Windows' driver for it is built in | virtio balloon |
| Shared folder | Windows' own file sharing, or Remote Desktop's redirected folder | virtiofs |
| Display | Standard VGA, or Remote Desktop where the edition has it | Remote Desktop |
| TPM | A virtual TPM kept per guest (Windows 11 expects one; BitLocker and Hello keys of the real chip are not in it) | The same |
| Firmware | OVMF with its own variables per guest, **a boot entry written by TUR** (4.3), no S3, no S4 | OVMF |
| Identity | The computer's own SMBIOS strings, disk serial and network address, read by the helper (4.4) | Generated once |

Virtio devices can be added later, after the person approves installing
drivers in their Windows, as optional devices; never as the boot disk.

### 4.3 The guest's firmware entry

A guest with fresh firmware variables has no "Windows Boot Manager" entry,
so OVMF falls back to `EFI/BOOT/BOOTX64.EFI`. On a disk shared with TUR that
file can be TUR's shim (bootupd writes it; pull request 112 puts Windows'
copy back; a later shim update overwrites it again), and the guest would
then start TUR's boot loader inside the guest. So the launcher writes the
entry itself, before the guest starts (`dualrunlib/ovmfvars.py`): the EFI
system partition by its partition GUID and
`\EFI\Microsoft\Boot\bootmgfw.efi`, first in `BootOrder`, appended to the
variable store in edk2's own format. No boot index is passed to QEMU, so
OVMF leaves the order alone (the finding of `tests/iso/isovm.py`). The
experiment starts the guest once with Secure Boot enforced and Microsoft's
keys enrolled (on a layer): Windows 11 26H2's boot manager may be signed
with Microsoft's 2023 certificate, which older key sets lack. NOT RUN.

### 4.4 Identity

`tur-dualrun identity` reads the computer's SMBIOS strings (system, board,
chassis, firmware) from `/sys/class/dmi/id`; the launcher passes them with
`-smbios`, and the disk's serial number and the network address likewise.
The embedded Windows key table (ACPI `MSDM`) can be passed with
`-acpitable`; the prototype has the mechanism (`acpi_tables`) and no command
that uses it, because whether to do so is a licence question for the owner's
lawyer (section 11), and CI has no such table and fabricates none.

## 5. Measurements

**Everything in this section is NOT RUN at hand-back.** The tables below are
the forms the run fills; `ci/summary.py` writes them into the run's summary
and `results.md`. Both configurations are QEMU/KVM virtual machines on a
runner (nested virtualisation): times are comparable with each other, not
with a PC.

| Table | Scenario (`ci/experiment.py`) | What it records |
|---|---|---|
| 5.1 Round trips | `roundtrips` | Five native→guest→native trips: seconds from QEMU's start to the agent's first line in each; how the guest was closed (Windows' own shutdown, or the ACPI power button from the host) and how long that took; per start what Windows reports: model, UUID, licence status and remaining grace, devices present and known, devices with problems, device-installation events, licensing events; whether TUR's stand-in partition is byte-identical; whether the guest wrote to its copy of the partition table. Trips 1 and 2 without the PC's identity, 3 to 5 with it, 5 without a TPM |
| 5.2 Shutdown states | `states` | After a full shutdown, a Fast Startup shutdown and a hibernation of real Windows 11: the first bytes of `hiberfil.sys`, the journal's restart pages, the dirty flag, the boot configuration's resume element, and what the check-list said. Then: what Windows does when a Fast Startup volume is started as a guest anyway (protected, thrown away); the boot type Windows records at each following start |
| 5.3 Faults | `faults` | QEMU killed during start, idle, heavy writing (file data, small files, registry) and during shutdown; the launcher killed while the guest runs, then `recover`. After each: the volume's state by the launcher's reader, the check-list's answer, Windows' own scan in the following start, and at the end a native start with `chkdsk /scan` |
| 5.4 Layers | `layers` | A destructive session thrown away (the disk's allocated extents hashed before and after; the documents' hashes natively); a kept session committed (seconds, bytes) and seen natively; the restore point refused after that native start; a commit killed part-way and run again; an immediate undo and the native view after it; the boot configuration destroyed and restored from TUR's copy |
| 5.5 Integration | `integration` | QEMU's resident memory idle, after ballooning 6 GiB to 3 GiB, and after giving it back; what Windows reports; a USB storage device handed to the running guest (seconds until Windows shows it, and after removal); seconds from the power button to power-off |
| 5.6 Seamless | `seamless`, `remoteapp` | Section 7 |
| 5.7 Other controller, Secure Boot | `controller` | Installed on NVMe, started on AHCI (on a layer); started with Secure Boot enforced |
| 5.8 Saved state | `savedstate` | QEMU's save to a file with NVMe and with AHCI: refused or completed, seconds, bytes; resume and seconds until the agent answers |
| 5.9 Files | `files` | H9's two writes, the driver used on TUR's side, the volume's state after each, Windows' scan |
| 5.10 Windows alone at the end | `final` | Start time against the first native start; boot-start drivers against the baseline; every file on the EFI system partition against the manifest taken before the first guest session; the stand-in PC's firmware entries; Windows' own boot-time records; `chkdsk /scan` |

The synthetic job (`ci/synthetic.py`, 30 cases, no Microsoft software, no
virtual machine) covers on volumes made by `mkfs.ntfs`: every hibernation
variant and its refusal, the dirty flag, clean, unclean and torn journal
pages, an unreadable root directory, a BitLocker signature, a mounted
partition, a missing boot manager, a Linux filesystem under a Windows type,
too little memory, the refusal texts; the view (size, table, mapping, zeros
for TUR's partition, a refused write, the kernel's lock, the shadow); a
protected session thrown away with the whole disk hashed; a commit killed
and run again, undo, and undo refused after the disk changed; the firmware
entry; the copy of the boot files. NOT RUN (queued). A volume made by
`mkfs.ntfs` is not one left by Windows: the same reader is run against real
Windows in 5.2 and 5.3.

## 6. Protected, layered, controlled by P@x

### 6.1 Protected: what TUR can do for a Windows that runs on top of it

| Superpower | Mechanism | Built here | Measured |
|---|---|---|---|
| "Try this installer; undo it if it goes wrong" | A protected session: the view read-only, writes to a qcow2 layer on TUR's side; discard deletes the layer | Yes | NOT RUN (5.4) |
| Ransomware-style damage contained | The same: nothing reaches the partitions until the person keeps the session. What is contained is the *disk*; files reached through a shared folder are not | Yes | NOT RUN (5.4) |
| A restore point before a Windows update, and a rollback of Windows itself | At commit, the blocks about to be overwritten are saved first (an undo record); `undo` writes them back | Yes | NOT RUN (5.4) |
| Windows will not start: explain and fix from outside | TUR keeps a copy and checksums of the person's own boot files (their files, on their computer: not redistribution); it can compare, restore one, rewrite the guest's firmware entry, read the boot configuration, and start Windows' own recovery environment inside the guest on a layer, keeping the repair only if Windows then starts | Copy, compare and restore: yes. The recovery environment on a layer: no | NOT RUN (5.4, the boot configuration case) |
| The disk checked from outside | The launcher's reader says clean, unclean, dirty or hibernated without starting anything. *Repair* of NTFS stays Windows' job (in the guest, on a layer if wanted); TUR ships no NTFS repair tool it trusts | Reader: yes | NOT RUN |

### 6.2 The hard constraint, exactly

**Whatever is not committed to the real partitions does not exist for a
native start of Windows.** So:

1. **One layer at most, and it never outlives the session's decision.** A
   protected session ends in *Keep* (commit) or *Throw away*. A new session
   is refused while a layer waits (070). There are no stacked or long-lived
   layers over a disk that also starts natively: a second, older layer would
   be invalid the moment the first is committed.
2. **A layer is bound to the disk's state.** At attach the launcher computes
   a token: a hash of the partition table, the first 8 KiB of every Windows
   partition, each NTFS journal's restart areas (they change at every mount
   by Windows) and the boot configuration. Commit is refused if the token no
   longer matches (072): Windows was started by itself, or the disk was
   changed, after the layer was opened. TUR's own writes to the volume do
   not always change the journal, so the storage service must invalidate the
   token itself whenever it mounts a Windows volume read/write.
3. **Restart into Windows with an open layer.** Proposed: `tur-dualboot
   restart-windows` refuses and offers Keep or Throw away. If the person
   starts Windows from the firmware's own menu instead, Windows starts from
   the disk as it was before the session; the layer is then stale and can
   only be thrown away, and TUR says so at its next start: "Windows was
   started by itself while changes were waiting; those changes cannot be
   applied any more." This is the honest cost of protection, and the reason
   protected sessions should end with an explicit choice, not linger.
4. **Commit is the one dangerous moment.** It copies the layer into the
   partitions; half-way through, the disk is in a state that never existed.
   The design makes it restartable (the undo record is complete and on disk
   before the first block is copied; copying the whole layer again gives the
   same result; a marker forbids any other use of the disk until it is
   finished), and the experiment kills it part-way and runs it again. What
   it cannot do is stop the person starting Windows from the firmware's menu
   during that window. Mitigations: commit at shutdown of the session while
   the person is told to wait (seconds to a minute for a typical session;
   measured in 5.4), keep the window short, and at the next start of TUR
   finish the commit before anything else. If Windows was started natively
   on a half-committed disk, TUR must not finish the commit (072) and must
   say plainly that Windows' own disk check is now the repair. Decision 3
   asks whether that residual risk is acceptable for "kept" sessions, or
   whether kept sessions should always be direct ones.
5. **A restore point is valid only until Windows next runs outside that
   chain.** Undo puts back old blocks; if Windows changed other blocks since
   (a native start, or a later session), the mix would be damage. The token
   enforces this: the experiment checks that undo is refused after a native
   start.

The alternative for restore points without the commit window, not built: an
undo journal *while running* (device-mapper's snapshot of an origin: writes
go straight to the disk, old blocks are copied aside first; rollback merges
them back). Every write then costs a copy, and merging has the same
interruption window, but only when the person asks for a rollback.

### 6.3 Layered: one vocabulary

Proposed for this route and for the `layers` spike (layered Wine prefixes),
so that the person meets one idea:

| Word | Here | In a Wine prefix (to be aligned with `layers`) |
|---|---|---|
| **Base** | Their Windows as it is on its partitions: the only thing a native start sees | The prefix template |
| **TUR's additions** | What TUR put *inside* the base with the person's approval: the agent, optional drivers. They are ordinary installed software in the base, listed by TUR, removable, and inert when Windows runs by itself. They are not a separate block layer, because nothing that must survive a native start can live outside the base | The runtime's own files |
| **Session layer** | The writes of one protected session, until Keep or Throw away | An application's or a session's changes |
| **Restore point** | The undo record of one commit; valid until the base next changes outside it | A snapshot of the prefix |
| **Keep / Throw away / Undo** | commit / discard / undo | The same words |

The difference to state in both places: a Wine prefix has one owner (TUR),
so its layers can be long-lived and stacked; a dual-run Windows has a second
owner (Windows itself, natively), so its layers cannot.

### 6.4 Controlled by P@x, safely

The policy engine and P@x live on the host, underneath Windows. A small TUR
agent inside the guest gives P@x the windows and controls (the helper of the
`winsee` spike; on real Windows it can use UI Automation in full, which the
`inwin` spike is measuring) and gives the seamless desktop its window list
(H5). The rule stands: **nothing P@x does bypasses the policy engine**, and
the agent holds no authority of its own: it answers only the host, over a
private channel, and every request passes the policy engine on the host
before it is sent. Nothing inside Windows is a trusted confirmation path
(ADR-0015).

What P@x could then do for the person's Windows from outside that no
assistant inside Windows can:

- **Explain and fix a Windows that will not start**: it reads the disk's
  state and the boot files' checksums without Windows running, compares them
  with the copy, and proposes one named repair.
- **Undo a bad change by rolling back a layer**, including a change that
  broke Windows' own recovery tools.
- **Operate applications under limits enforced below Windows**: a session it
  drives can be a protected one, with no network, with only a granted
  folder, and thrown away at the end, whatever the application or Windows
  does.
- **See what a session changed** before it is kept (the layer is a list of
  changed blocks; mapping them to files needs a reader of the layered
  volume, not built).

Action classes (ADR-0012's model; the numbers are a proposal for the policy
table, to be fixed there):

| Action on the guest | Class | Confirmation |
|---|---|---|
| Read the window list, read the disk's state, read the layer's size | 0 (observe) | none |
| Start an application the person has used; focus, move, minimise a window | 1 (reversible, in the session) | by grant |
| Type into or click in a Windows application; close a window with unsaved work; hand a USB device to the guest | 2 (acts for the person) | per grant or per action |
| Start a session; throw a layer away | 2 | per action: throwing away loses the session's work |
| Keep a layer (commit); undo; restore a boot file; shut Windows down while applications are open | 3 (changes the disk or Windows' boot) | a confirmation P@x cannot operate |
| Anything that sets a refusal aside | not available | — |

## 7. One seamless desktop (H5)

**All NOT RUN.** What the experiment does, and what is already known.

### 7.1 Where the edition allows it: RemoteApp through FreeRDP

Scenario `remoteapp` switches Remote Desktop and RemoteApp on with Windows'
documented settings (in a protected session that is thrown away, so the
evaluation Windows is left untouched), then starts two applications with
FreeRDP on a host desktop (Xvfb with openbox, **not KWin**) and records: one
host window per application window, their titles and whether each has an
icon; seconds to the first window; whether a resize from the host reaches
the guest's window and after how long (coarse: the agent's answer over a
serial line is in the number); clipboard both ways; a folder redirected by
the client both ways; and Windows' own file sharing reached from the host.
**Not measured here:** move and resize latency properly, multi-monitor,
HiDPI, drag and drop, audio, taskbar grouping, and all of it on KWin
(ADR-0015's acceptance tests). Windows 11 **Pro** is not available as an
evaluation; the result is for Enterprise.

Two things this route needs from the person's Windows that a purpose-built
guest did not: Remote Desktop must be switched on **in their Windows** (it
then also listens when Windows runs by itself: the firewall rule must be
limited to the private virtual link), and their account needs a password
Remote Desktop accepts (a PIN does not work; an account without a password
cannot connect). Both need the person's approval, once.

### 7.2 Where it does not (Home): TUR's own helper, nothing asked of Windows' licensing

Windows' Remote Desktop service is not patched, wrapped or replaced.
Scenario `seamless` runs TUR's helper in the guest (in this spike:
PowerShell and a small C# class compiled by Windows' own compiler; in the
product: the C helper of `winsee`) and shows, in pictures:

1. `1-guest-screen.png`: the guest's one screen with two overlapping
   application windows.
2. `2-cut-from-screen-*.png`: each window cut out of that screen by its
   rectangle: **what VirtualBox's seamless mode had to work with.** The
   lower window's picture contains the upper one; the hidden part is marked
   and its share recorded.
3. `3-own-content-*.png`: each window's own content, asked from Windows
   (`PrintWindow` with full-content rendering), hidden parts included, with
   the time each capture takes.
4. `4-host-desktop-two-windows.png`: two host windows showing them.
5. A click on the host's picture of the lower window routed to the guest
   through the virtual tablet: the window comes to the front; a menu opened
   and whether it is a top-level window of its own; one window closed and
   one minimised from outside; what a capture of a minimised window gives.

What is already clear without the run, to be confirmed or corrected by it:

| Problem | Why | Way out |
|---|---|---|
| Hidden content | The guest's one screen holds only what is visible | Per-window capture inside the guest: `PrintWindow` (polling, tens of milliseconds per window, some windows refuse), or the Windows Graphics Capture API (frames pushed per window, on the graphics card when there is one), or DWM thumbnails. H5(b)'s kill criterion is whether any of these gives the hidden part |
| Latency and animation | Capture, copy across the boundary, draw | Shared memory between guest and host instead of a serial line or a socket (an `ivshmem` device needs a driver in Windows; a virtual display's dma-buf does not exist without a virtio or passed-through card). **This is the open engineering question of the route**: without added drivers, frames travel over the network link |
| Menus, tooltips, drop-downs | They are separate top-level windows, often without a title, owned by another | Show each as a host popup positioned relative to its owner; the helper reports the owner |
| The guest's taskbar, desktop icons and wallpaper | They are windows too | Not shown; the helper hides the taskbar or reports its rectangle to be skipped |
| Stacking and focus | Two window managers | The host is the authority; the helper applies the host's order in the guest, and reports a guest-side change (a dialog raising itself) |
| Input | Pointer and keys must arrive at the right window | Absolute pointer through the virtual tablet after bringing the target to the front; the cost is a round trip before each first click |
| Secure desktop (UAC prompts, sign-in) | No capture API sees it | Shown from the guest's whole screen in a host window of its own, and said to be Windows' own prompt |

### 7.3 Around both routes

Windows applications in TUR's launcher (the Start menu's entries and icons,
read by the helper while the guest runs and cached; `inwin`'s formats when
it is down); "open with" (the file reaches the guest by its Windows path on
the same volume, no copy); notifications (the helper forwards Windows'
toasts to the host's notification service, class 0); and what P@x needs
(the same helper feeds it, section 6.4). All design; NOT RUN.

## 8. Where this sits in TUR

### 8.1 In the router

The last route for Windows applications and devices, after the catalogue's
answer, a re-hosted native runtime, Wine, and before "Restart into Windows":

| Situation | Route |
|---|---|
| The catalogue says Wine works, or the application re-hosts | As today |
| Wine is inadequate, and **the person has Windows installed on this computer** | Their own Windows, inside TUR (this route). The application is already installed there, with its licence and settings |
| The same, and the check-list refuses (hibernated, BitLocker without the key, no memory) | The refusal, with Restart into Windows as the other way |
| A device that needs its Windows driver | Handed to their Windows inside TUR (USB); other buses: Restart into Windows |
| No Windows on this computer | The purpose-built guest of ADR-0015, which needs the person's own licence and media |
| Games with anti-cheat, 3D, video editing | Proton, or Restart into Windows; never this route by default |

### 8.2 Relation to the planned guest for people without Windows

One manager (`tur-guest`), two kinds of guest. They share the display path,
the helper, the lifecycle and P@x's tools. They differ in the disk (the view
over real partitions and the root helper, against a qcow2 image), in the
devices (built-in drivers only, against virtio from the start), in identity
(the computer's own, against generated), and in what may be stored outside
Windows (nothing that must survive a native start, against anything). The
purpose-built guest can have long-lived snapshots; this one cannot (6.2).

### 8.3 What needs the person's approval

Once, at set-up: using their Windows inside TUR at all (after a page that
states the limits of "The experience"); turning Fast Startup off in Windows;
TUR keeping a copy of their boot files and, if they choose, their Windows
sign-in secret and BitLocker recovery key in the keyring; installing TUR's
helper in their Windows; later and separately, switching Remote Desktop on,
and installing optional drivers. Each session: nothing, unless the
check-list refuses. Each Keep, Undo and repair: a confirmation P@x cannot
operate.

## 9. Proposed decision text (for an ADR; the lead assigns the number)

> **Dual run: the person's installed Windows as a guest of TUR.** Where a
> computer has Windows installed beside TUR, `tur-guest` can start that
> installation as a KVM guest from its own partitions. (1) The guest is
> given a device-mapper view that maps Windows' partitions and nothing
> else; the partition table is a private copy; every other range has no
> mapping. The view is made by a root helper that does nothing else; QEMU
> runs as the user under libvirt's session instance. (2) Before every start
> a check-list reads the disk without mounting it and refuses, in plain
> words and with no override, when Windows is hibernated or was left by
> Fast Startup, when that cannot be established, when a Windows partition
> is in use, when the volume is locked by BitLocker and the person does not
> hold its recovery key, and when memory is short. (3) The guest uses only
> devices Windows has drivers for; nothing boot-critical is ever added to
> the person's Windows; its firmware offers no sleep states; TUR writes the
> guest's firmware boot entry and never the computer's. (4) A session is
> direct, or protected: on a layer that ends in Keep or Throw away. At most
> one layer exists; it and every restore point are bound to the disk's
> state and are refused once Windows has run outside them. (5) Windows'
> licensing and edition limits are not circumvented: no activation
> mechanism is touched, Remote Desktop is used only where the edition has
> it and is never patched, BitLocker is never bypassed. (6) Not released to
> users before the legal review of section 11 and before H1 and H2 have
> passed on real hardware.

Status to propose: **not yet**. The text is ready; the evidence (H1, H2) is
not.

## 10. Trial on the owner's own PC

**Do not start before the CI experiment has passed H1 and H2.** Then the
steps are ordered so that the first three cannot write to the disk.

Before anything: (a) a full image backup of the Windows disk to an external
drive with a tool the owner trusts, and a check that it can be restored;
(b) in Windows, create a recovery drive; (c) note whether "Device
encryption" or BitLocker is on (Settings, Privacy and security) and, if so,
save the recovery key; (d) in Windows, turn Fast Startup off (Control Panel,
Power options, "Choose what the power buttons do") and choose **Restart**.

TUR's image does not yet contain QEMU, OVMF or swtpm (ADR-0015 lists them as
to be added; this spike may not edit `image/`). Until it does, QEMU runs
from a Fedora 45 toolbox (`toolbox create`; inside: `dnf install
qemu-system-x86-core edk2-ovmf swtpm`); the root steps run on the host,
which has Python and `dmsetup`.

| Step | Command (from a checkout of this branch, in `tests/experiments/dualrun`) | Writes to the disk? | Stop if |
|---|---|---|---|
| 1 | `sudo ./tur-dualrun inspect /dev/nvme0n1 > inspect.json` (the device that holds Windows; check with `lsblk`) | No | It fails; send the file |
| 2 | `sudo ./tur-dualrun check /dev/nvme0n1 --mode protected` | No | Any refusal: do what it says. TUR's partition must be under `hide`, Windows' under `give` |
| 3 | A protected session: `sudo ./tur-dualrun attach /dev/nvme0n1 --state /var/lib/tur-dualrun --mode protected --overlay $HOME/dualrun/layer.qcow2 --owner $(id -u)`; then as the user `./tur-dualrun vars --session /var/lib/tur-dualrun/<name>/session.json --out $HOME/dualrun/vars.fd` and `./tur-dualrun identity > identity.json`; write `config.json` (the keys of `dualrunlib/qemu.py`: the firmware code file, `vars.fd`, 6 GiB, the layer as a qcow2 disk on NVMe or AHCI as the PC has, `display: gtk`, `no_sleep_states: true`, the identity) and `./tur-dualrun run --config config.json --run $HOME/dualrun/run` from the toolbox. Windows 10 appears in a window. Shut it down from its Start menu. Then `sudo ./tur-dualrun discard /dev/nvme0n1 --state /var/lib/tur-dualrun` | **No**: the view is read-only | Windows does not start: note what the screen says; discard; nothing was changed |
| 4 | Restart into Windows itself. It must start as always | — | It does not: this would contradict step 3's guarantee; restore the backup and report |
| 5 | Only with the backup of (a): a direct session (`--mode direct`, the disk in `config.json` is the view's device, format raw). Shut Windows down inside. `sudo ./tur-dualrun detach …` | **Yes** | The detach reports a partition-table change |
| 6 | Restart into Windows itself; run `chkdsk C: /scan`; look at Settings, Activation | — | Any error |

What to send back: `inspect.json` (it contains volume labels and partition
GUIDs, no file names or contents), what Windows showed about activation and
devices in steps 3 and 5, and the times. This would be the first Windows 10
result and the first on real firmware, a real TPM and, if present, real
BitLocker and an OEM key. The wrapper that turns step 3 into one command is
not written; the lead may prefer to wait for it.

## 11. Licence questions for the owner's lawyer

Nothing was built whose purpose is to defeat licensing. The questions:

1. **One instance, physical and virtual.** Microsoft's terms for Windows 11
   allow one instance per licence on one device, "whether that device is
   physical or virtual", and say that using the software on more than one
   virtual device needs a licence for each (ADR-0015's sources). Here the
   same installed instance runs alternately on the physical computer and in
   a virtual machine *on that same computer*, never both at once. Is that
   one device or two?
2. **OEM licences.** An OEM licence is tied to the computer it came with.
   Does running that installation in a virtual machine on that computer keep
   within it? Does carrying the computer's own SMBIOS identity, and its own
   embedded key table (`MSDM`), into that virtual machine change the answer?
   The launcher does not pass the key table until this is answered.
3. **Remote use.** The clauses ADR-0015 already flags (a device "accessed
   solely for remote use", remote access by the licensed user) for the
   Remote Desktop display path, here on the person's own installation.
4. **Windows 10** out of support: any term that restricts use in a virtual
   machine after the end of support.
5. **The evaluation in CI** (section 2): does "evaluation" cover an
   automated compatibility experiment run by a project, and who accepts the
   terms when a workflow does?
6. **The person's own boot files kept by TUR** on their own computer for
   repair: confirmed as not redistribution?

## 12. Effort

One session of design and writing; no CI iteration finished. About 6,100
lines: the launcher and its library (about 2,400), the synthetic tests
(about 640), the experiment with its guest scripts, the seamless-desktop
part and the summary (about 2,400), the workflow and CI helpers (about
500), the guest side (about 250), and the registration.

## 13. Limits, and what was not run

- **Nothing has run.** Every claim about Windows' behaviour in this report
  is either from a source or a design expectation, and is marked. The code
  was syntax-checked only; a first CI run of code this size will find
  errors in the harness before it finds anything about Windows.
- **Not real hardware.** Real firmware, a real TPM, real BitLocker sealing,
  OEM activation, a real graphics card, real USB devices: NOT RUN until the
  owner's PC.
- **Windows 10: NOT RUN** and not runnable in CI lawfully. **Windows 11 Home
  and Pro: NOT RUN** (the evaluation is Enterprise); edition differences
  (Remote Desktop host, BitLocker management) are from Microsoft's
  documentation as cited in ADR-0015.
- **BitLocker: NOT RUN** beyond detecting its signature on a synthetic
  volume. A BitLocker scenario on virtual TPMs (enable it in the stand-in
  PC, start as guest, enter the recovery key, go back) is designed and not
  written.
- **Online activation: NOT RUN** (the guest has no Internet). The evaluation
  reports its own time-limited licence; how a retail or OEM digital licence
  reacts is not something an evaluation can show.
- **KWin and Wayland: NOT RUN.** The host desktop in H5 is Xvfb with
  openbox.
- **The partition-table shadow** reports a change and does not apply it.
  A Windows update that resizes its recovery partition inside a session
  would leave the real table behind the filesystem; this needs decision 6
  before unattended use.
- **Other disks.** A Windows with volumes on a second disk gets only the
  system disk in this prototype.
- **4K-native disks, dynamic disks, Storage Spaces, Intel RST/VMD modes**:
  not handled; the last is the likeliest real-world failure of H1 (Windows
  installed with the firmware in RAID/VMD mode has another boot driver) and
  the launcher does not yet detect it.
- **The token** does not see writes TUR itself makes through `ntfs3` without
  touching the journal; the storage service must invalidate it (6.2).
- **QEMU 8.2 on Ubuntu 24.04** is what CI runs, not Fedora 45's 11.1.

## 14. Decisions for the owner

1. **Microsoft's evaluation of Windows 11 Enterprise in CI.** The task
   allowed downloading it in CI and using it there; its licence terms could
   not be read in advance (they are inside the medium). Stage `terms` prints
   them. If they plainly allow installing the software for evaluation and
   testing, the experiment is dispatched and the terms are quoted here. If
   they do not, or are unclear about an automated run, **the owner
   decides**, and until then H1 to H10 stay NOT RUN in CI.
2. **Is the default session protected or direct?** Proposal: protected for
   the first sessions and for anything P@x drives; direct once the person
   has seen it work; always the person's choice.
3. **May a protected session be kept** (committed), given the window during
   which a native start would find a half-written disk (6.2, point 4)? The
   alternative: protected sessions are only ever thrown away, and work to be
   kept is done in direct sessions.
4. **Which desktop is the face**: (c) as recommended (TUR's desktop carrying
   their choices, the real Windows desktop one key away, the real desktop as
   the face only with a second graphics card)?
5. **Sign-in**: may TUR keep the Windows sign-in secret (and, if the person
   enters it, the BitLocker recovery key) in the person's keyring?
6. **A partition-table change made by Windows inside a session**: apply it
   to the real table after confirmation when it touches only Windows' own
   entries, or always refuse and send the person to a native start?
7. **The legal review** of section 11, before anything reaches a user.
8. **The image**: QEMU, OVMF, swtpm and (for H5) FreeRDP's X11 client are
   needed in TUR's image for the owner's trial without a toolbox (ADR-0015
   already lists them); this spike could not edit `image/`.

## Sources

Read on 14 November 2026 unless noted.

| | |
|---|---|
| S1 | Microsoft Evaluation Center, Windows 11 Enterprise download: https://www.microsoft.com/en-us/evalcenter/download-windows-11-enterprise |
| S2 | Microsoft Evaluation Center, Windows 11 Enterprise: https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise |
| S3 | Microsoft licence terms index: https://www.microsoft.com/en-us/useterms |
| S4 | https://www.microsoft.com/en-us/evalcenter/download-windows-10-enterprise (301 to https://blogs.windows.com/windowsexperience/2024/10/31/how-to-prepare-for-windows-10-end-of-support-by-moving-to-windows-11-today/) |
| S5 | WSLg, README (architecture: Weston's RDP backend extended to remote applications, FreeRDP, `mstsc.exe`): https://github.com/microsoft/wslg |
| S6 | QEMU, "hw/nvme: add basic live migration support", v10, June 2026 (the emulated NVMe controller was not migratable before it; whether it is merged in QEMU 11.1 was not established): https://patchew.org/QEMU/20260611180842.6390-1-alexander@mihalicyn.com/ |
| S7 | QEMU, Hyper-V Dynamic Memory Protocol driver (`hv-balloon`), cover letter and source: https://patchwork.kernel.org/project/qemu-devel/cover/cover.1672878904.git.maciej.szmigiero@oracle.com |
| S8 | virt-firmware (`virt-fw-vars`), considered for the firmware entry and not used (the launcher writes edk2's variable format itself, as `tests/iso/efivars.py` reads it): https://gitlab.com/kraxel/virt-firmware |
| S9 | In this repository: ADR-0015 and `docs/architecture/windows-guest.md`; ADR-0007; `docs/architecture/storage.md` section 8 (what `ntfs3` does and does not check; ntfs-3g's hibernation and journal rules); pull request 112 (ADR-0029 as proposed there, `engine.sh`: TUR's partition is type `4f68bce3-…`, name "TUR"); `tests/iso/isovm.py` (OVMF and boot order) |
| S11 | Linux kernel, dm-flakey: `error_writes`: "All write I/O is failed with an error signalled. Read I/O is handled correctly." (used for the ranges the guest must not have): https://docs.kernel.org/admin-guide/device-mapper/dm-flakey.html |
| S12 | Microsoft, `shutdown`: `/hybrid` "Shuts down the device and prepares it for fast startup. This option must be used with the /s option"; `/h` hibernation (how the experiment produces each state): https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/shutdown |
| S10 | Microsoft, BCD WMI provider, `BcdBootMgrElementTypes`: `BcdBootMgrBoolean_AttemptResume` = 0x26000005, "Indicates that a resume operation should be attempted during a system restart"; `BcdBootMgrObject_ResumeObject` = 0x23000006: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/bcd/bcdbootmgrelementtypes . That Windows sets the element when it hibernates or uses Fast Startup is the expectation the experiment measures on a real store |

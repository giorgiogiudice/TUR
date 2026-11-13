<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
<!--
  Maintained list of what works and what does not in TUR's installer medium and
  the system it installs. Its body (everything below the title) becomes the
  section "What works and what does not" of every release's notes
  (docs/releases/TEMPLATE.md). Keep it short, plain and true for the build being
  released: update it in the same pull request as the change it describes, and
  read it again before every release. Link the issue for each item.
-->
# Known issues

**Everything here was tested in virtual machines only.** No release has yet
been tried by the project on a real computer, a real USB stick or a real disk.
If you are testing on real hardware, you may be the first.

### What works (in a virtual machine, on every change)

- The medium starts under UEFI, from a USB stick or an optical drive, with no
  network.
- It installs TUR onto one whole, blank disk or onto a disk that holds data
  (after a confirmation that names the disk), and refuses a disk that is too
  small or a confirmation that does not match.
- Beside Windows **on a second disk**: TUR is installed on the other disk and
  the Windows disk is left byte for byte unchanged.
- The installed system starts to the graphical login with SELinux enforcing,
  and a bad update is undone by itself at the next starts (boot health).
- With Secure Boot enforced and Microsoft's keys enrolled (the keys PCs ship
  with), the medium started and the installed system started (informational;
  in a virtual machine only).

### Not tested yet

- **Real hardware of any kind:** firmware, NVMe and SATA disks, USB sticks,
  Wi-Fi-only computers, NVIDIA graphics, laptops' special keys, touch screens,
  vendor boot menus, Secure Boot on real firmware
  ([#76](https://github.com/giorgiogiudice/TUR/issues/76)).
- Real Windows and BitLocker beside TUR
  ([#76](https://github.com/giorgiogiudice/TUR/issues/76)).
- Some of the installer's refusals: a disk without a serial number, a
  read-only disk, a disk in use, a disk changed after confirmation, and the
  screen after a failed installation
  ([#76](https://github.com/giorgiogiudice/TUR/issues/76)).

### Known problems and limits

- **Whole disk only.** The installer cannot keep data or put TUR on the same
  disk as Windows or another system. Installing beside Windows on one disk is
  planned ([#64](https://github.com/giorgiogiudice/TUR/issues/64)); until
  then, use a second disk.
- **TUR becomes the first boot entry.** Beside Windows on another disk, TUR's
  first start puts itself first in the firmware's boot order (the entry is
  named "Fedora"). Windows stays in the list unchanged; choose it from the
  firmware's boot menu ([#76](https://github.com/giorgiogiudice/TUR/issues/76)).
- **The medium is large:** the system is on it twice
  ([#76](https://github.com/giorgiogiudice/TUR/issues/76)). The stick size it
  needs is under "System requirements".
- English only, US keyboard layout only, UEFI only; no encryption, no manual
  partitioning.
- The installer keeps its log only in memory: it is lost at power-off
  ([#76](https://github.com/giorgiogiudice/TUR/issues/76)).
- **There are no updates yet.** The installed system has nothing to update
  from; a newer build is installed by installing again.
- Boot health: start-up takes longer than it should (a 15-second hold), a
  bad update undone by boot health is not announced on screen yet, and
  `mcelog.service` fails on AMD processors so such starts are reported as
  "degraded" ([#46](https://github.com/giorgiogiudice/TUR/issues/46)).
- TUR's own desktop programs (the application centre and settings) are not in
  the system yet; the desktop is KDE Plasma as Fedora Kinoite ships it, with
  TUR's look ([#59](https://github.com/giorgiogiudice/TUR/issues/59)).
- P@x, the assistant, cannot use TUR's local AI yet, and local AI is not in
  the system ([#74](https://github.com/giorgiogiudice/TUR/issues/74)).
- Some Fedora names remain visible: the firmware boot entry and the EFI
  folder are named "Fedora", the network firewall zone is
  `FedoraWorkstation`, and Firefox says it is "managed by your organisation"
  ([#77](https://github.com/giorgiogiudice/TUR/issues/77)).

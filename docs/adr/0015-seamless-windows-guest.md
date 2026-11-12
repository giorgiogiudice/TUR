# ADR-0015: Seamless Windows guest on QEMU/KVM, libvirt and FreeRDP RemoteApp

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §3.4, §7.4, §10.4, §11.4, §12.4, §13, §16, §17.2,
  §20, §23 (question 9); [ADR-0007](0007-app-centric-routing.md). See the
  [seamless Windows guest design](../architecture/windows-guest.md)

## Context

ADR-0007 makes a seamless Windows guest a first-class tier: one real Windows
installation under QEMU/KVM, started on demand, suspended when its last window
closes, its applications shown as individual windows on the Plasma desktop. It
is used only where a compatibility layer is known to be inadequate, which in
practice means Microsoft Office and Adobe's applications. ADR-0007 left open
whether to adopt WinBoat, adapt it, or build directly on FreeRDP, and required
the licence terms for running Windows in a guest to be confirmed.

Constraints already decided:

- Microsoft binaries are never shipped; WinApps is never adapted or
  redistributed; FreeRDP and WinBoat are usable (AGENTS.md, licence review).
- TUR's own code is a set of separate programs
  ([ADR-0003](0003-licence.md)).
- No compatibility runtime runs as root (§12.1). The base is Fedora 45 Kinoite
  with SELinux enforcing ([ADR-0001](0001-base-distribution.md)).
- A guest's applications are exposed to P@x by a small agent that is an MCP
  tool server ([ADR-0008](0008-assistant-tool-protocol.md)).

What was found on 12 November 2026, with sources in the design document:

- **Individual windows from a Windows guest are done with RDP RemoteApp.**
  FreeRDP 3.33.1 is in the TUR image. Its X11 client has the mature
  implementation and runs under XWayland. Its SDL client gained RemoteApp for
  Wayland on 23 September 2026, labelled beta. The older Wayland client has
  none.
- **Fedora builds FreeRDP without FFmpeg, and the image carries the
  placeholder for OpenH264.** The H.264 graphics modes are not available on
  the image as built.
- **Microsoft allows Remote Desktop connections to Windows 11 Pro and
  Enterprise, not to Home, and names only Windows Server as supported for
  RemoteApp.** RemoteApp on Pro works in practice and is switched on by a
  setting Microsoft documents for unattended setup.
- **Microsoft's licence terms** for Windows 11 allow one instance per licence
  on one device, "whether that device is physical or virtual". They also say a
  device does not include a system "accessed solely for remote use over a
  network", and restrict remote access to a licensed user "who physically
  uses the licensed device". How these apply to a guest displayed by RDP on
  the same machine could not be established.
- **WinBoat** (MIT, beta, v0.9.2 of 25 August 2026) is an Electron application
  that runs dockur/windows in a container and shows windows through FreeRDP.
  dockur/windows obtains the installation media by scripting Microsoft's
  download service.
- **No graphics acceleration exists for an x86-64 Windows guest without
  passthrough.** The virtio-win 3D driver has been an unmerged pull request
  since 2023; Triton needs an ARM64 guest and forked host components.
- **The redistribution terms of the signed virtio-win driver image were not
  found.**
- Fedora 45 packages QEMU 11.1, libvirt 12.6, OVMF, swtpm and virtiofsd. None
  is in the image yet.

## Options considered

1. **Adopt WinBoat as the tier.** Least work at first. Brings an Electron
   application, a compose workflow and a third-party container image whose
   default behaviour (scripted download from Microsoft, generic product keys)
   TUR may not ship; runs QEMU in a container with added network privilege;
   gives TUR no control over the display client, suspend path or agent; adds a
   second launcher beside the application centre.
2. **Build a TUR manager on QEMU/KVM and FreeRDP RemoteApp, with QEMU driven
   through libvirt's session instance.** TUR writes a manager and a guest
   agent. Every engine is a Fedora package driven as a separate program.
   QEMU runs as the user under libvirt's SELinux confinement.
3. **The same, with QEMU driven directly through QMP.** Fewer packages. TUR
   would own the guest's confinement policy, TPM and firmware handling and
   save and restore.
4. **A different display path:** SPICE or Looking Glass (one desktop window,
   not application windows; Looking Glass needs a passed-through GPU and a
   kernel module), or a guest-side Wayland proxy as ChromeOS has for Linux
   guests (nothing of the kind exists for Windows).

## Decision

Adopt option 2.

1. **`tur-guest`** is a per-user manager with a D-Bus interface
   (`org.tur.Guest1`, session bus) and a command line. It drives QEMU/KVM
   through libvirt's session instance by `virsh` and guest definitions,
   linking nothing.
2. **Windows are RDP RemoteApp windows through FreeRDP.** `xfreerdp` under
   XWayland is the baseline; `sdl-freerdp` on native Wayland is the target.
   Both sit behind `tur-guest-display`, which confines the client. Which is
   the default is decided by acceptance tests on KWin, and switching needs no
   new ADR.
3. **`tur-guest-agent`** runs unelevated inside the guest and speaks to
   `tur-guest` only, over a private virtio-serial channel. It lists and starts
   applications and exposes Windows UI Automation to P@x as MCP tools whose
   classes come from a manifest on the host.
4. **The user supplies Windows.** The user downloads the media from Microsoft
   in a browser, holds the licence, enters the key and accepts Microsoft's
   terms. TUR ships no Microsoft binary and no product key, scripts no
   Microsoft download, and bypasses no check of Windows Setup or activation.
   Baseline: Windows 11 Pro, current release. Home is refused.
5. **Guest drivers are fetched from their publisher**, by the user by default,
   and verified against a pinned digest. They are not shipped while their
   redistribution terms are unverified.
6. **The guest is untrusted.** It cannot reach services on the host's loopback
   interface; nothing is shared with it except folders the user grants; USB
   devices reach it only by the explicit hand-off of §10.4; nothing inside
   Windows is a trusted confirmation path.
7. **Lifecycle:** start on demand; pause, then save to disk, after the last
   window closes; snapshots that include the disk, firmware variables and TPM
   state. Budgets are targets until measured.
8. **GPU passthrough and Looking Glass remain an expert option**, to be
   decided by a later ADR.
9. **WinBoat is a reference, not a component.** WinApps is not used.

**Condition.** The tier is built and tested, but not released to users, until
the legal review has answered how Microsoft's remote-use clauses and the
status of RemoteApp on Windows 11 Pro apply to this design.

## Consequences

- Specification §13's default display for a Windows guest changes from GPU
  passthrough with Looking Glass to RDP RemoteApp. §23 question 9 is answered
  for the default path.
- TUR takes on two pieces of its own code: a virtual-machine manager, bounded
  by leaning on libvirt, and a Windows program, kept small and portable so
  that most of it is tested on Linux.
- The image gains QEMU's system emulator, libvirt's QEMU driver, OVMF, swtpm
  and virtiofsd, inactive until the user sets the tier up. Their versions join
  the build manifest.
- A Windows guest has no graphics acceleration by default. The tier is for
  office and business applications. Video editing, 3D and games are stated as
  not working, before setup and in the compatibility matrix.
- The tier needs a Windows 11 Pro licence of the user's own, and by ADR-0007's
  estimate a host with 16 GB of memory.
- The tier depends on behaviour Microsoft does not document as supported for
  client editions. A Windows update can break it. Snapshots before feature
  updates and a degraded single-window mode are the mitigations.
- Almost all of TUR's side can be proven in CI against a Linux stand-in guest
  with no Microsoft software. Installation, RemoteApp against Windows itself,
  activation and the application matrix need a dedicated machine and the
  owner's licence, and are recorded as not run until that exists.
- Addendum A's coverage table changes: applications in the Windows guest gain
  a semantic tier.
- The licence review's question 10 is partly answered and its open points are
  stated precisely.

## Verification

- Legal review of the five open points in the design document, section 13.3,
  before any release of the tier.
- The redistribution terms of the virtio-win driver image, before any
  assisted download.
- In CI, before Windows work begins: pause, save and restore of a guest with
  a TPM, Secure Boot firmware and a virtiofs share attached; the guest
  process's SELinux label; that the guest cannot reach the host's loopback;
  two remote-application windows correctly identified on KWin with each
  FreeRDP client.
- On a real guest: that Windows 11 Pro of the current release accepts
  RemoteApp sessions as configured; that the attestation-signed drivers load
  under Secure Boot; that activation survives save, restore and snapshot;
  the start-up, memory and idle budgets.
- At each Fedora rebase: FreeRDP's RemoteApp behaviour against the display
  acceptance tests, and the state of 3D drivers for Windows guests.
- Check the current Model Context Protocol specification before fixing the
  agent's protocol (ADR-0008).

## Sources

Read on 12 November 2026. The full list, with what each was used for, is in
[section 19 of the design](../architecture/windows-guest.md#19-sources).

- FreeRDP releases: https://github.com/FreeRDP/FreeRDP/releases
- Microsoft, Remote Desktop supported configurations:
  https://learn.microsoft.com/en-us/windows-server/remote/remote-desktop-services/clients/remote-desktop-supported-config
- Microsoft Software License Terms, Windows Operating System (April 2024):
  https://microsoft.com/content/dam/microsoft/usetm/documents/windows/11/oem-(pre-installed)/UseTerms_OEM_Windows_11_English.pdf
- WinBoat: https://github.com/winboat-org/winboat
- dockur/windows: https://github.com/dockur/windows
- virtio-win packaging: https://github.com/virtio-win/virtio-win-pkg-scripts/blob/master/README.md
- virtio-win 3D driver pull request:
  https://github.com/virtio-win/kvm-guest-drivers-windows/pull/943
- libvirt QEMU driver: https://libvirt.org/drvqemu.html

## Acceptance (12 November 2026)

Accepted as the technical approach, under the owner's decision in ADR-0007 that a
Windows guest is used only where a compatibility layer is not enough. Two
conditions stand: nothing in this tier is released to users before the legal
review of Microsoft's licence terms described above, and every budget in the
design is a target until it has been measured.


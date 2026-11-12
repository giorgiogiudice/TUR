# ADR-0007: App-centric routing with a seamless real-OS tier

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §1, §2, §3.4, §7, §13, §20, §32

## Context

The project's aim is that applications from every ecosystem work, on one desktop,
without the user having to know how. Revision 1.0 of the specification makes the
compatibility layers (Wine, Darling) the product and treats a real Windows guest
as a heavy, optional last resort reached through a "Run in full Windows" setting,
displayed through GPU passthrough and Looking Glass.

That design has a ceiling. A compatibility layer reimplements another operating
system's interfaces and is never complete. Wine with Proton is excellent for games
and uneven for productivity software such as Microsoft Office and Adobe's
applications. Darling does not run mainstream macOS GUI applications. So the
applications people most often need from another platform are the ones the primary
path handles worst.

Three further facts, checked on 12 November 2026:

- Projects such as WinApps and WinBoat already present applications from a real
  Windows guest as individual windows on a Linux desktop, using RDP RemoteApp
  through FreeRDP. WinBoat is still in beta.
- `umu-launcher` runs Proton outside Steam and is what Lutris now uses by default.
  It answers specification question 5.
- GPU acceleration inside a Windows guest without passthrough is experimental
  (the Triton DirectX driver, v0.4, is described by its author as unstable). A
  Windows guest is therefore suitable for productivity software, not for games.

An immutable Fedora base also cannot install another distribution's packages
directly, so "Linux applications" is not one case either.

## Options considered

1. **Keep revision 1.0:** compatibility layers first, a VM as an expert option.
   Lightest, but the ceiling above stands.
2. **VM only:** run every foreign application in a real guest OS. Highest
   compatibility for productivity software; heavy; poor for games; no legal route
   for macOS on PC hardware.
3. **App-centric routing across both.** The user asks for an application; TUR
   chooses the best route to it from a ladder that includes both compatibility
   layers and a seamless real-OS guest, and presents every route as ordinary
   windows.

## Decision

Adopt option 3.

1. **The unit is the application, not the executable.** An application catalogue
   records, for each known application, which routes work and how well. Inspection
   of an executable (the `tur-runtime` work) remains the fallback for unknown
   software.

2. **Routing ladder.** The first route that is known to work is used; the user can
   override it. A compatibility layer always comes before a guest: it is faster and
   lighter, and the owner's stated preference is to avoid virtualisation wherever
   an application runs well without it. The guest is used only where the layer is
   known to be inadequate, and making Windows applications run smoothly is the
   measure of success for both routes.

   | Wanted | First choice | Then |
   |---|---|---|
   | Linux application | Flatpak or native | A container of another distribution, for packages Fedora does not have |
   | Windows game | Proton through `umu-launcher` | Dual boot, where kernel anti-cheat requires it |
   | Windows productivity application | Wine, where the catalogue says it works | **Seamless Windows guest** |
   | Android application | Android runtime (addendum B) | — |
   | macOS application | The same application's Windows or Linux edition | Darling, experimental |

3. **A seamless guest is a first-class tier.** One Windows guest under QEMU/KVM,
   started on demand and suspended when its last window closes. Its applications
   appear as individual windows on the Plasma desktop, in the launcher and the task
   switcher, with shared folders, clipboard and audio. A small TUR agent inside the
   guest handles integration and gives the AI layer a semantic view of guest
   applications. GPU passthrough and Looking Glass become an expert option, not
   the default display path.

4. **macOS is an optional, experimental feature, not a pillar.** Darling is
   shipped as a feature the user can enable. It is off by default, labelled
   experimental, and comes with no promise that a given application works; a
   published list says what does. It is not a main line of effort. A macOS guest
   is not an option: Apple's licence allows macOS virtual machines only on Apple
   hardware.

5. **TUR builds the router and the experience, not the engines.** The engines are
   upstream: Proton and `umu-launcher`, Wine, Waydroid, Flatpak, a container tool
   for other distributions, QEMU/KVM and FreeRDP. TUR writes the catalogue, the
   routing, the application centre, the guest integration and the diagnostics.

## Consequences

- "It runs" stops meaning "Wine can start it". The compatibility matrix gains a
  route column, and the answer for a given application can be "in the Windows
  guest".
- The guest tier needs a licensed Windows installation, supplied by the user, and
  costs memory while it runs. TUR without the guest remains a complete system.
  Indicative memory: 8 GB without the guest tier, 16 GB with it. To be measured.
- The guest tier is a security improvement for untrusted Windows software: a guest
  is a stronger boundary than a Wine prefix.
- Specification §3.4 still holds: a compatibility layer and a guest are different
  things, and diagnostics and application properties must say which one an
  application is using.
- Some things no route delivers, and TUR must say so plainly: games with kernel
  anti-cheat (dual boot only); Android applications that require Google's device
  attestation; macOS and iOS applications in general.
- Roadmap changes: the "full Windows backend" (Phase 6) moves forward to sit
  beside the Windows runtime (Phase 4) and is rescoped around seamless windows;
  the macOS runtime (Phase 5) is reduced to an optional experimental feature; an application
  catalogue and application centre are added to Phase 3–4.
- Deferred or dropped from revision 1.0: GPU passthrough with Looking Glass as the
  default guest display; two of the three visual modes; most of
  `tur-device-manager` beyond a device inventory and guest hand-off; APFS; the
  custom graphical installer until the standard one has been outgrown.

## Verification

- Evaluate WinBoat and WinApps on the TUR image before choosing whether to adopt
  one, adapt one, or build the guest integration directly on FreeRDP: licence
  compatibility, how each provisions Windows, and behaviour on Wayland.
- Confirm `umu-launcher` and the Proton builds it manages can be shipped or
  fetched under their licences.
- Measure idle memory and start-up time of the guest tier, and the cost of
  suspend and resume, before publishing any number.
- Confirm the licence terms under which a user may run their Windows licence in a
  guest.

## Sources

Checked 12 November 2026.

- The Register, "Contain your Windows apps inside Linux Windows":
  https://www.theregister.com/2026/02/14/winapps_and_winboat/
- WinBoat: https://github.com/winboat-org/winboat
- Lutris 0.5.20 release coverage (Proton-GE through umu by default):
  https://www.gamingonlinux.com/2026/02/game-manager-lutris-v0-5-20-released-with-proton-upgrades-store-updates-and-much-more/page=1/
- umu-launcher package: https://archlinux.org/packages/multilib/x86_64/umu-launcher/
- Triton v0.4: https://www.linuxcompatible.org/story/triton-v04-updated-directx-11-gpu-driver-for-qemu-released/
- Mesa Venus: https://docs.mesa3d.org/drivers/venus

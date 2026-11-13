# Architecture Decision Records

An ADR records one decision: the context, the options considered, what was decided
and what follows from it. ADRs are required for any change that affects an
interface, boot, storage, security or update behaviour, and for any deviation from
the [specification](../architecture/specification.md).

## Process

1. Copy [`0000-template.md`](0000-template.md) to the next free number.
2. Open it as a pull request with status **Proposed**.
3. On merge the status becomes **Accepted**.
4. An ADR is never rewritten after acceptance. To change a decision, write a new
   ADR and mark the old one **Superseded by ADR-NNNN**.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-base-distribution.md) | Base distribution: Fedora Kinoite on bootc | Accepted |
| [0002](0002-ci-and-build-infrastructure.md) | CI and build infrastructure: GitHub Actions first | Accepted |
| [0003](0003-licence.md) | Licence: PolyForm Noncommercial 1.0.0 | Accepted; licence text superseded by 0010 |
| [0004](0004-ai-provider-model.md) | AI layer: optional, provider-agnostic, user-supplied key | Accepted |
| [0005](0005-languages-and-workspace.md) | Implementation languages and workspace layout | Accepted |
| [0006](0006-android-runtime.md) | Android runtime: Waydroid as an opt-in OS container | Accepted |
| [0007](0007-app-centric-routing.md) | App-centric routing with a seamless real-OS tier | Accepted |
| [0008](0008-assistant-tool-protocol.md) | Assistant tools use the Model Context Protocol | Accepted |
| [0009](0009-project-name.md) | The project is TUR, The Unified Realm; the assistant is P@x | Accepted |
| [0010](0010-licence-excludes-government.md) | Licence: noncommercial, and not for government use | Accepted |
| [0011](0011-application-centre-and-stores.md) | One application centre, integrating every store | Accepted |
| [0012](0012-assistant-policy-model.md) | The assistant's authority is enforced by a pure decision function outside the model | Accepted |
| [0013](0013-bootloader.md) | Bootloader: GRUB 2 through shim, installed by bootupd | Accepted |
| [0014](0014-boot-health-and-rollback.md) | Boot health and rollback: a TUR attempt counter in GRUB's environment block | Accepted |
| [0015](0015-seamless-windows-guest.md) | Seamless Windows guest on QEMU/KVM, libvirt and FreeRDP RemoteApp | Accepted |
| [0016](0016-desktop-and-user-interface.md) | Desktop and user interface: standalone programs, Plasma by configuration, one identity | Accepted |
| [0017](0017-update-manager.md) | The update manager: TUR drives bootc in three steps, and stages nothing boot health cannot undo | Accepted, amended |
| [0018](0018-first-installer-medium.md) | The first installer medium: a small live environment that runs the image's own installer | Proposed |
| [0019](0019-app-install-backend.md) | Installing applications: one backend, two routes, pinned downloads through curl | Accepted |
| [0020](0020-overlay-window-positions.md) | P@x's overlay finds other programs' windows through the Plasma window-management protocol, read with KWayland | Proposed |
| [0021](0021-local-ai.md) | Local AI: models that run on the computer, for P@x and for every application | Accepted |
| [0023](0023-publishing-signed-updates.md) | Publishing updates: signed images in a public registry, on channels | Accepted |
| [0024](0024-problem-reports.md) | Problem reports: prepared by P@x, sent only with consent, to a private receiver | Accepted |
| [0025](0025-live-session-and-graphical-installer.md) | The installer medium's live session and graphical installer | Proposed |
| [0026](0026-windows-device-drivers-in-user-space.md) | Windows device drivers: hand-off and HID first, a user-space driver host only as an experiment | Proposed |

## Open questions from the specification

[Section 23](../architecture/specification.md#23-open-questions-to-resolve-through-adrs)
lists the questions still to be settled. ADR-0001 answers questions 2, 3 and 4.
ADR-0013 answers question 1 (bootloader), with evidence from the boot test.

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
| [0013](0013-bootloader.md) | Bootloader: GRUB 2 through shim, installed by bootupd | Accepted |
| [0014](0014-boot-health-and-rollback.md) | Boot health and rollback: a TUR attempt counter in GRUB's environment block | Accepted |
| [0015](0015-seamless-windows-guest.md) | Seamless Windows guest on QEMU/KVM, libvirt and FreeRDP RemoteApp | Accepted |
| [0016](0016-desktop-and-user-interface.md) | Desktop and user interface: standalone programs, Plasma by configuration, one identity | Accepted |

## Open questions from the specification

[Section 23](../architecture/specification.md#23-open-questions-to-resolve-through-adrs)
lists the questions still to be settled. ADR-0001 answers questions 2, 3 and 4.
ADR-0013 answers question 1 (bootloader), with evidence from the boot test.

Number 0012 is reserved for the assistant policy model, under review.

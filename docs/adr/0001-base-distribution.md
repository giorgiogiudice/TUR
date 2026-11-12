# ADR-0001: Base distribution — Fedora Kinoite on bootc

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §5.1, §12.1, §23 (questions 2, 3 and 4)

## Context

The specification requires an image-based, transactional OS with atomic updates
and rollback, a KDE Plasma Wayland desktop, a current kernel and Mesa, and a
maintained security baseline. It left the base distribution, the mandatory
access-control framework and the image build model open.

Building these foundations from scratch would consume the project's early phases
without producing anything that distinguishes UOS. An existing distribution already
ships an image-based KDE desktop on the tooling the specification names.

## Options considered

1. **Fedora Kinoite (Fedora Atomic KDE), consumed as a bootc container image.**
   First-party bootc and OSTree integration, a current kernel and Mesa, KDE Plasma
   on Wayland, SELinux enforcing, signed boot components. Fedora releases roughly
   every six months, so UOS must rebase regularly.
2. **A Universal Blue image (Aurora or Bazzite) as the base.** More is
   pre-integrated, including gaming components, but it adds a second upstream
   between UOS and Fedora whose choices UOS would inherit.
3. **openSUSE Kalpa.** An image-like KDE desktop, but built on transactional
   updates and snapshots rather than bootc, so it does not match the specified
   update model.
4. **A Debian, Ubuntu or Arch base assembled into an image by UOS.** Full control,
   but UOS would own the image tooling, update model and security baseline itself.

## Decision

UOS derives from the Fedora Kinoite bootc image. The OS is defined as a
`Containerfile` on that base and delivered as a bootc image. SELinux is the
mandatory access-control framework.

## Consequences

- Specification questions 2 (base distribution), 3 (SELinux or AppArmor) and
  4 (bootc container image or traditional image builder) are settled.
- UOS uses the Fedora kernel unmodified. This keeps Secure Boot working through
  Fedora's signed boot chain. Any out-of-tree kernel module reopens the signing
  problem and needs its own ADR.
- Phases 0 to 3 of the roadmap become integration and verification work on an
  existing base rather than construction.
- Packages come from Fedora where they exist. Components Fedora does not package
  (Darling in particular) must be built and pinned by UOS.
- UOS takes on a rebase to each new Fedora release, gated by the boot, rollback and
  regression tests.
- Fedora's trademarks may not be used to brand UOS. Branding is replaced in the
  image.

## Verification

To be confirmed against current upstream state during the first image build, and
recorded in that task's report before anything is pinned:

- The exact image reference and digest of the Kinoite bootc base.
- **Bootloader and boot health (specification question 1).** Which bootloader the
  chosen bootc release supports on this base, and whether health-checked rollback
  is provided by an existing mechanism that UOS should wrap rather than replace.
  The specification's assumption of systemd-boot boot counting must be tested, not
  assumed.
- That `ntsync` is enabled in the Fedora kernel in use.
- The KDE Plasma and Mesa versions shipped.

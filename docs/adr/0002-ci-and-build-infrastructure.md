# ADR-0002: CI and build infrastructure — GitHub Actions first

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §18.3, §19, §31

## Context

TUR needs to build OS images and boot them under UEFI in a virtual machine on every
change. It also needs, eventually, tests that only real hardware can run: GPUs,
Bluetooth, suspend and resume, firmware updates, device passthrough and voice
input.

No dedicated build machine or test hardware is available at the start of the
project.

## Options considered

1. **GitHub-hosted runners only.** No infrastructure to maintain. Linux runners
   provide hardware virtualisation, so images can be boot-tested under QEMU/KVM.
   Limited disk, memory and job time; no GPU; no real devices.
2. **A dedicated build machine as a self-hosted runner from day one.** Removes the
   limits, but adds cost and an attack surface: a self-hosted runner on a public
   repository can be made to run code from a pull request.
3. **Both, phased.** Start hosted, add dedicated machines when a concrete need
   appears.

## Decision

All builds and tests run on GitHub-hosted runners for now. A dedicated build
machine and a physical test PC will be added later, when one of the triggers below
is reached.

## Consequences

- Phases 0 and 1 (image build, VM boot, diagnostics, rollback, installer tests on
  disposable disk images) fit within hosted runners.
- Graphical tests in CI use software rendering. They can show that the desktop
  starts; they say nothing about GPU drivers or performance.
- Every test in the hardware matrix (§31) and every performance benchmark (§19.6)
  is recorded as **not run** until hardware exists. A VM result is never reported
  as a bare-metal result.
- Image builds and boot tests do not run on developer machines or on servers used
  for anything else.

## Triggers for adding dedicated machines

Raise the need with the project owner as soon as any of these is reached:

- **Build machine:** a build no longer fits a hosted runner's disk, memory or time
  limit. Building Darling from source is the likely first case (Phase 5), and
  heavier image builds may reach it sooner.
- **Test PC:** the start of Phase 2 (hardware and storage), or the first task that
  needs a real GPU, real USB or Bluetooth devices, suspend and resume, a
  performance baseline, or microphone input for the voice interface.

## Verification

- Confirm in the first CI workflow that the hosted runner exposes `/dev/kvm` and
  that a UEFI guest boots with acceleration.
- When a self-hosted runner is added, restrict it so that pull requests from forks
  cannot run on it without approval.

# ADR-0005: Implementation languages and workspace layout

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §16, §18.1

## Context

UOS's own components are system services, command-line tools and desktop UI. The
specification names Qt 6 and QML for UI and leaves the service language open.

Development is done largely by several agents working in parallel, on machines
that must not compile or build anything (ADR-0002). Whatever is chosen has to work
with CI as the only compiler, and has to let parallel changes merge without
constant conflicts.

## Options considered

1. **Rust for services and tools; Qt 6 / QML for UI.** Memory safety for
   privileged, long-running services that parse untrusted input (executables,
   partition tables, device descriptors). Mature D-Bus support. Strict compiler
   and linter, which suits CI-only development.
2. **C++ throughout.** One language with the desktop, but no memory safety and a
   weaker fit for unattended agent development.
3. **Python or Go for services.** Faster to write; Python is a poor fit for
   boot-critical and privileged services, and Go binaries integrate less well
   with a desktop built on Qt and D-Bus.

## Decision

- **Rust** for UOS services, libraries and command-line tools.
- **Qt 6 / QML**, with C++ only where QML needs it, for graphical UI.
- **POSIX shell and Python** only for build, test and CI scripts, never in a
  shipped service.
- **Each Rust component is its own Cargo workspace** with its own `Cargo.lock`,
  under `crates/` (shared libraries), `services/` or `installer/`. Shared code is
  used through path dependencies.

## Consequences

- No Rust toolchain is needed, or permitted to run, on development machines. CI
  compiles, tests, and computes what a developer normally would locally: the
  lockfile and `rustfmt` output are returned as an artifact
  (`scripts/dev/ci.py fixes`).
- Per-component workspaces mean two components can add dependencies at the same
  time without a lockfile conflict. The cost is duplicated compilation and
  possible version drift between components. Revisit when the set of components
  is stable; a single workspace is the likely end state.
- The toolchain is pinned in `rust-toolchain.toml` and updated deliberately.
- `unsafe` is forbidden by default. A component that needs it says why in its
  own documentation.
- Third-party crates are a supply-chain surface. Each new dependency is stated in
  the pull request with its licence and the reason for it; prefer the standard
  library and a small set of widely used crates.

## Verification

- CI discovers every component and runs format, lint, tests and doc tests on it.
- A licence and advisory check over each lockfile is to be added before the first
  release.

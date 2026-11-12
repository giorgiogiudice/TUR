# AGENTS.md

Binding rules for automated development agents working on UOS. Human contributors
follow the same rules. The full baseline is the
[specification](docs/architecture/specification.md); where this file and the
specification disagree, the specification wins and the disagreement is a bug to
report.

## Read first

1. This file.
2. The subsystem document for the area you are changing, under `docs/architecture/`.
3. Any ADR in `docs/adr/` that the subsystem document references.

Do not load the whole specification by default. Read the sections your task names.

## Priority order

Never trade a higher item for a lower one.

1. UOS boots reliably and recovers.
2. Installation is safe, observable and exportable for diagnosis.
3. Linux hardware, storage, networking, audio and display work well.
4. The desktop is responsive and polished.
5. Windows compatibility, validated with real applications.
6. macOS compatibility, with an honest published matrix.
7. Desktop bridge and runtime manager.
8. Full Windows VM mode.
9. APFS and advanced device work, only with data-protection tests.

## Hard rules

- **Never** perform a destructive storage operation without explicit confirmation
  that shows the exact target device and its stable identity.
- **Never** run destructive installer tests on a developer or production host. Use
  disposable VM disk images.
- **Never** enable experimental APFS write support by default.
- **Never** load Windows `.sys` or macOS `.kext` drivers into the Linux kernel.
- **Never** collect or upload diagnostics silently.
- **Never** claim complete Windows or macOS compatibility. Report measured results.
- **Never** bundle Microsoft or Apple binaries.
- **Never** commit secrets, tokens, keys or personal data. Never log them.
- **Never** track an unpinned upstream branch in an image.
- Keep the Linux kernel the single authority for hardware. Use the distribution
  kernel; do not carry kernel patches without an ADR.
- Integrate upstream projects rather than forking them. Patches live in a small
  queue, each with a stated reason.
- Verify current upstream documentation and project health before pinning a
  dependency. Do not rely on what the specification says an upstream supports.

## Licence boundary rules

From the [licence review](docs/dependencies/licence-review.md) and
[ADR-0003](docs/adr/0003-licence.md). These decide where code may live.

- The project's noncommercial licence covers only the project's own separate
  programs. Build a new component as a **standalone program** that talks to
  others over D-Bus, a socket or the command line.
- Code that runs inside a GPL program cannot carry the project licence. That
  includes Plasma applets, look-and-feel packages, System Settings pages, KWin
  scripts and effects, Calamares modules and kernel modules. If such code is
  unavoidable it goes in a clearly marked directory under the upstream licence,
  with an ADR.
- Project-licensed code must not link GPL-only libraries. Known cases: the Qt
  modules that are GPL-only (Virtual Keyboard, Wayland Compositor, Quick 3D,
  Quick Timeline, Graphs) and `libbluetooth`. Check a library's licence before
  depending on it.
- **Never ship or redistribute:** Microsoft or Apple binaries; Google Mobile
  Services or an Android image that contains them; ARM translation libraries
  (`libhoudini`, `libndk_translation`); Proton builds (they bundle a proprietary
  Steam component: the user's machine fetches them); codecs from outside Fedora.
- Do not adapt or copy code from WinApps (its licence status is unclear). FreeRDP
  and WinBoat are usable.
- **No image may be published** until Fedora's logos and release packages have
  been replaced: Fedora's terms forbid distributing them in a derived system.

## AI control layer rules

- The AI layer is optional and off by default. Nothing in boot, install, update or
  recovery may depend on it.
- The assistant never holds authority that a person has not granted. Operations on
  disks, boot, firmware and encryption require confirmation through a path the
  assistant cannot operate.
- Provider-specific code stays behind the provider interface. No service other than
  `uos-agent` talks to a model provider.
- API keys live in the system keyring. They never appear in profiles, logs or
  diagnostic bundles.

## How to work

- One work item per change. Keep changes small and reviewable.
- State the problem, the component, the plan, the tests and the rollback behaviour.
- Write an ADR when a change affects an interface, boot, storage, security or
  update behaviour, or deviates from the specification.
- Freeze an interface (D-Bus XML or schema) before parallel work depends on it.
- A change is not done because it compiles. It is done when CI passes the relevant
  test, including a boot test for anything that can affect boot.
- Boot-critical and storage-critical work needs fault-injection and recovery tests.

## Completion report

End every task with the report template in
[section 21 of the specification](docs/architecture/specification.md#21-mandatory-operational-feedback-for-development-agents).
Do not write "working" without stating what was built and what was tested. Report
failures and skipped steps plainly.

## Build environment

**Nothing is compiled, built or booted on a development machine.** The machines
agents run on are shared and small. CI is the only compiler and the only place an
image is built or a VM is started
([ADR-0002](docs/adr/0002-ci-and-build-infrastructure.md),
[ADR-0005](docs/adr/0005-languages-and-workspace.md)).

- Do not install a Rust toolchain, and do not run `cargo`, `podman build`,
  `docker build` or `qemu` locally.
- Work on a branch, push, and let CI judge:

  ```sh
  git push -u origin HEAD
  scripts/dev/ci.py wait            # waits for every run on HEAD; prints failures
  scripts/dev/ci.py fixes RUN DIR   # applies CI's Cargo.lock and rustfmt patch
  scripts/dev/ci.py pr --title "..." --body-file body.md
  ```

- A pull request is ready when every workflow on its head commit is green.
- Do not merge your own pull request. Do not push to `main`.
- Tests that need real hardware are recorded as **not run** until a hardware
  runner exists. Do not substitute a VM result for a bare-metal result.

## Layout

| Path | Content | Language |
|---|---|---|
| `image/` | OS image definition | Containerfile, shell |
| `crates/` | Shared Rust libraries | Rust |
| `services/` | UOS services and their command-line tools | Rust |
| `installer/` | Installer | Rust, QML |
| `desktop/` | Desktop UI | QML |
| `schemas/` | Versioned JSON schemas for every interchange format | JSON Schema |
| `tests/` | Image, boot, installer and integration tests | shell, Python |
| `scripts/` | Build and CI scripts; `scripts/dev/` for developer helpers | shell, Python |
| `docs/` | Specification, subsystem designs, ADRs | Markdown |

Each Rust component is its own Cargo workspace (its `Cargo.toml` has a
`[workspace]` table) and depends on shared code by path. Every source file starts
with an SPDX licence identifier.

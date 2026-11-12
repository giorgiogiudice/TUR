# TUR image

The TUR operating system is one bootc container image, defined by
[`Containerfile`](Containerfile) on a base pinned in [`base.lock`](base.lock)
([ADR-0001](../docs/adr/0001-base-distribution.md)).

| Path | Purpose |
|---|---|
| `Containerfile` | The image definition |
| `Containerfile.containerignore` | What is sent to the build: this directory, and the sources of the programs compiled into the image |
| `base.lock` | The pinned base image: repository, tag, digest, version |
| `rootfs/` | Files copied into the image. Everything lives under `/usr` |
| `third-party-licensed/` | Files that are **not** under the TUR licence, each directory with its own licence and README |

## Building

Images are built in CI (the `image` workflow), or on a dedicated build machine:

```sh
scripts/build-image.sh localhost/tur:dev
tests/image/smoke.sh localhost/tur:dev "$(. <(podman run --rm localhost/tur:dev cat /usr/lib/tur/release); echo "$TUR_BUILD_ID")"
```

Do not build on a shared or production server: the base image alone is several
gigabytes.

A build is deterministic for a given commit. The layer timestamp is the commit
time and the build ID is `<commit date>.g<commit hash>`. CI builds the image twice
and fails if the two image IDs differ.

## Programs built into the image

TUR's own programs are compiled inside the image build, in a build stage of the
`Containerfile` (`FROM … AS programs`), and copied into the image with
`COPY --from=programs`. `tur-boot-health` is the first.

- **The toolchain is the official Rust image, pinned by digest** in the
  `Containerfile` (`RUST_IMAGE`, `RUST_TAG`, `RUST_DIGEST`). The stage fails if
  its `rustc` is not the version in [`rust-toolchain.toml`](../rust-toolchain.toml),
  so the two cannot drift apart. To move to a new Rust version, change both in
  one pull request.
- **The compile step has no network** (`RUN --network=none`,
  `cargo build --release --locked --offline`). A program therefore builds only
  from what is in this repository and its committed `Cargo.lock`. A component
  with third-party crates has to vendor them, or the step has to be given
  network access for a `cargo fetch --locked` first; that is a decision for the
  pull request that adds such a component.
- **The build context is the repository root**, filtered by
  `Containerfile.containerignore`. A component that joins the image adds its
  directory there, a `COPY` of its sources to the stage, and its build and
  `install` commands.
- **Reproducibility** is checked the same way as before: the `image` workflow
  builds twice, the second time without any cache, and compares image IDs. The
  release profile of a program built here should fix `codegen-units = 1` and
  switch off incremental compilation and debug information, as
  `services/tur-boot-health/Cargo.toml` does.
- The binary is linked against the C library of the toolchain image (Debian),
  which is older than the base image's. The build runs the program once inside
  the final image (`--version`) so that an incompatibility stops the build, not
  a boot.

## Boot health and rollback

What the image carries for [ADR-0014](../docs/adr/0014-boot-health-and-rollback.md);
the design is
[boot-health-and-rollback.md](../docs/architecture/boot-health-and-rollback.md),
section 10.

| In the image | Source | Purpose |
|---|---|---|
| `/usr/lib/bootupd/grub2-static/configs.d/08_tur_boot_count.cfg` | `third-party-licensed/grub/` (BSD-3-Clause) | The GRUB fragment that counts boot attempts. bootupd writes it into `grub.cfg` at installation; it is frozen from then on (contract version 1) |
| `/usr/lib/bootc/kargs.d/10-tur-boot.toml` | `rootfs/` | One kernel argument, `$tur_boot_args`, a GRUB variable that the fragment sets |
| `/usr/libexec/tur/tur-boot-health`, `/usr/bin/tur-boot-health` | `services/tur-boot-health/`, compiled in the build | Classify the boot, the health gate, mark good, make a fallback permanent |
| `/usr/lib/systemd/system/tur-boot-*.service`, `tur-boot-failed.target`, links in `multi-user.target.wants` and `emergency.target.wants` | `rootfs/` | The units of design section 6.1 |
| `/usr/lib/systemd/system/boot-complete.target.d/10-tur.conf` | `rootfs/` | The gate must pass before `boot-complete.target` |
| `/usr/lib/systemd/system/bootloader-update.service.d/10-tur.conf` | `rootfs/` | No bootloader update during a trial |
| `/usr/lib/systemd/journald.conf.d/10-tur.conf` | `rootfs/` | `Storage=persistent` |
| `/usr/lib/tmpfiles.d/tur-boot-health.conf` | `rootfs/` | `/var/lib/tur/boot-health` and its `evidence` directory |
| `/usr/lib/tur/boot-health/gate.toml`, `required-units.d/`, `checks.d/` | `rootfs/` | The gate's configuration. Read-only; there is no `/etc` override |
| `bootc-fetch-apply-updates.timer`, user unit `grub-boot-success.timer` | `Containerfile` | Masked: links to `/dev/null` |

No package is added. Nothing is written under `/etc`. The initramfs, the kernel
and the signed boot chain are untouched.

One point differs from the design, on measured evidence: no unit requires the
mount behind `/boot`. On this image `/boot` is an automount with a two-minute
idle timeout, and systemd stops a unit that requires the mount when it unmounts
it. The units are ordered after the mount instead
([TUR-005](../docs/reports/TUR-005.md)).

## What CI produces

| Artifact | Content |
|---|---|
| `build-manifest.json` | What the image was built from and contains ([schema](../schemas/build-manifest.schema.json)) |
| `sbom.spdx.json` | SPDX software bill of materials |
| `licences.txt` | Every licence expression in the image, with a count |

`scripts/sbom-licences.py sbom.spdx.json` turns the SBOM into the package and
licence list kept in
[`docs/dependencies/image-licences.csv`](../docs/dependencies/image-licences.csv).

## Identity

The image keeps Fedora's distribution `ID` so that image and update tooling keep
working, and identifies itself as TUR through `IMAGE_ID`, `IMAGE_VERSION` and
`BUILD_ID` in `/usr/lib/os-release` and through `/usr/lib/tur/release`. Full
rebranding is a Phase 3 task.

## Updating the base pin

The base is pinned by digest. The upstream registry publishes only a rolling tag,
so an old digest can eventually be removed upstream; until TUR mirrors its base,
the pin must be refreshed regularly.

Change `base.lock` in a pull request of its own, so that CI proves the new base
builds and passes the image tests before anything else depends on it.

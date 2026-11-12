# TUR image

The TUR operating system is one bootc container image, defined by
[`Containerfile`](Containerfile) on a base pinned in [`base.lock`](base.lock)
([ADR-0001](../docs/adr/0001-base-distribution.md)).

| Path | Purpose |
|---|---|
| `Containerfile` | The image definition |
| `Containerfile.containerignore` | What is sent to the build: this directory, TUR's artwork (`branding/`), and the sources of the programs compiled into the image |
| `programs.tsv` | The TUR programs compiled into the image: component, program, where it is installed |
| `programs.sh` | What the build stage runs: fetch the crates, compile the programs, gather what goes with them |
| `base.lock` | The pinned base image: repository, tag, digest, version |
| `branding/` | What replaces Fedora's marks: the script the build runs, the pinned replacement packages, Firefox's policy (section "Fedora's marks") |
| `signing/` | The script that adds TUR's rule to the container policy at build time (section "Signed updates") |
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
`COPY --from=programs`. [`programs.tsv`](programs.tsv) lists them;
[`programs.sh`](programs.sh) is what the stage runs.

| Program | Component | In the image | Assistant tools |
|---|---|---|---|
| `tur-boot-health` | `services/tur-boot-health` | `/usr/libexec/tur/`, linked from `/usr/bin/` | – |
| `tur-diag` | `services/tur-diagnostics` | `/usr/bin/` | `tur-diagnostics` |
| `tur-device` | `services/tur-device-manager` | `/usr/bin/` | `tur-device-manager` |
| `tur-storage` | `services/tur-storage` | `/usr/bin/` | `tur-storage` |
| `tur-catalogue` | `services/tur-catalogue` | `/usr/bin/` | `tur-catalogue` |
| `tur-runtime` | `services/tur-runtime` | `/usr/bin/` | `tur-runtime` |
| `tur-update` | `services/tur-update` | `/usr/bin/` | not installed: see below |
| `tur-pax` | `services/tur-pax` | `/usr/bin/` | – |
| `tur-pax-memory` | `services/tur-pax-memory` | `/usr/bin/` | – |
| `tur-pax-providers` | `services/tur-pax-providers` | `/usr/libexec/tur/` | – |
| `tur-installer` | `installer/tur-installer-core` | `/usr/libexec/tur/` | – |

`/usr/bin` holds what a person types and what an assistant tool adapter
starts by path; `/usr/libexec/tur` holds what units and other programs start.
No unit starts any of the ten new programs: nothing runs that did not run
before. The desktop program `tur-centre` is not in the image
([TUR-006](../docs/reports/TUR-006.md) says what stands in the way).

What goes with the programs:

| In the image | From | Purpose |
|---|---|---|
| `/usr/lib/tur/programs.tsv` | written by the build | What was installed: program, path, component, tools directory. The smoke test, the build manifest and the boot test read it |
| `/usr/share/tur/pax/tools/<component>/agent-tools.json`, `agent-tools.adapter.json` | each component that has the two files at its top level | The tools P@x may offer a model, and how each is performed ([pax-orchestrator.md](../docs/architecture/pax-orchestrator.md)). `tur-pax` reads this directory when it is not told another |
| `/usr/share/tur/catalogue/*.toml` | `catalogue/` | The application catalogue. `tur-catalogue` reads it when it is not told another, and its assistant tools name it |
| `/usr/share/licenses/tur/LICENSE.md`, `NOTICE` | the repository root | The licence of TUR's own programs and its required notice |
| `/usr/share/licenses/tur-third-party/` | the crates, as fetched | `crates.tsv`: which program holds which crate, with version, declared licence and checksum; `<crate>-<version>/`: the licence files that crate ships; `rust-std-<version>/`: those of the Rust standard library |

`tur-update` keeps its assistant tools in `services/tur-update/pax/`, not at
the component's top level, so they are not installed; moving them is a change
in `services/tur-pax`'s tests and is left to that work.

How the stage works, and why:

- **The toolchain is the official Rust image, pinned by digest** in the
  `Containerfile` (`RUST_IMAGE`, `RUST_TAG`, `RUST_DIGEST`). The stage fails if
  its `rustc` is not the version in [`rust-toolchain.toml`](../rust-toolchain.toml),
  so the two cannot drift apart. To move to a new Rust version, change both in
  one pull request.
- **Third-party crates are fetched, not vendored.** One step,
  `programs.sh fetch`, has network access and runs `cargo fetch --locked` for
  every component. It compiles nothing. Each component's committed
  `Cargo.lock` names every crate by version and by the SHA-256 of its crate
  file; Cargo checks each download against that and `--locked` refuses a
  lockfile that would have to change. What is fetched is therefore pinned as
  firmly as a vendored copy would be. The alternative, `cargo vendor`, would
  put the sources of about a hundred crates into the repository (the
  Windows-only ones included, since vendoring is not per platform), to be
  regenerated by CI on every dependency change because no development machine
  runs Cargo (AGENTS.md). What vendoring would add is independence from
  crates.io at build time. A crate file cannot be changed or removed there, a
  yanked version stays downloadable, and the build already needs the network
  for the two pinned images; if TUR ever mirrors its base it can mirror the
  crates with it.
- **The compile step has no network** (`RUN --network=none`,
  `cargo build --release --locked --offline`). A program therefore builds only
  from this repository, the crates fetched above and the pinned toolchain.
- **The release profile is set in the stage**, by `CARGO_PROFILE_RELEASE_*`
  variables in the `Containerfile`, for every program alike: one code
  generation unit, nothing incremental, no debug information, symbols
  stripped. A component's `Cargo.toml` need not repeat it. All components
  share one target directory, so a crate that several use in the same version
  is compiled once.
- **Reproducibility** is checked as before: the `image` workflow builds twice,
  the second time without any cache (so the crates are fetched again), and
  compares image IDs. When they differ it lists which of TUR's files differ.
- **The programs are linked against the C library of the toolchain image**
  (Debian 13), which is older than the base image's. The build runs every
  program once inside the final image (`--version`), so that one that cannot
  start stops the build and not a boot. The C code some crates carry (zstd in
  `tur-diag`; the cryptography of `ring` in `tur-pax` and
  `tur-pax-providers`) is compiled in the stage and linked statically; no
  program needs a library the base image does not have.
- **Licences.** `programs.sh` asks Cargo which crates each program links
  (`cargo tree --edges normal`), copies their licence files and writes
  `crates.tsv`. `scripts/sbom-programs.py` adds the programs and the crates to
  the SBOM and fails the `image` workflow if a crate's licence is not on its
  allow-list of permissive licences: TUR's programs carry a noncommercial
  licence and link their crates statically, so a GPL or LGPL crate may not be
  among them ([licence review](../docs/dependencies/licence-review.md),
  section 5).
- **The build context is the repository root**, filtered by
  `Containerfile.containerignore`.

To add a program: a line in `programs.tsv`, the component's directory in
`Containerfile.containerignore`, a `COPY` of it in the `Containerfile`, and
its paths in the triggers of the `image` and `boot` workflows. If it ships
`agent-tools.json` and `agent-tools.adapter.json`, they are installed without
further ado. The smoke test then expects one program more.

What the programs start is in the base image already: `bwrap` (bubblewrap)
and `prlimit` (util-linux) for P@x's sandbox, `lsblk`, `udevadm`,
`journalctl`, `fwupdmgr`, `bootc`, and the commands `tur-diag` collects with.
No package is added. The smoke test checks each.

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

## Signed updates

What the image carries for [ADR-0023](../docs/adr/0023-publishing-signed-updates.md);
the design as built, and how publishing is switched on, is
[publishing.md](../docs/architecture/publishing.md).

| In the image | Source | Purpose |
|---|---|---|
| `/usr/share/containers/policy.json` | the base's vendor policy, plus one rule added by [`signing/policy.py`](signing/policy.py) in a `RUN` of the `Containerfile` | An image named `ghcr.io/giorgiogiudice/tur` is accepted only with a sigstore signature by TUR's release key that is entered in Sigstore's public Rekor log. Every other source, the `oci:` and `containers-storage:` transports and the default keep the base's policy |
| `/usr/share/containers/registries.d/tur.yaml` | `rootfs/` | Signatures of that repository are sigstore attachments beside the image |
| `/usr/share/tur/signing/rekor.pub`, `fulcio.crt` | `rootfs/`, from Sigstore's trust root (`scripts/release/trust-roots.py`) | The Rekor log's key; Fulcio's chain for the keyless signature |
| `/usr/share/tur/signing/release.pub` | the owner, when publishing is switched on | TUR's release key. Absent until then: every image of the repository is refused |

Nothing is written under `/etc`: the base ships its policy as a vendor file
in `/usr/share/containers` and no `/etc/containers/policy.json`, which an
administrator may still create. The build fails if the base already has a
rule for the repository or if anything else in its policy would change; the
smoke test checks the rule, the files, and that the policy parses.

## What CI produces

| Artifact | Content |
|---|---|
| `build-manifest.json` | What the image was built from and contains ([schema](../schemas/build-manifest.schema.json)): among the rest, each TUR program with its version, size and SHA-256 as found in the image |
| `sbom.spdx.json` | SPDX software bill of materials: the RPM packages, and TUR's programs with the crates each links |
| `licences.txt` | Every licence expression of the RPM packages, with a count |
| `image-crates.csv`, `crates.tsv` | The crates compiled into TUR's programs, as kept in [`docs/dependencies/image-crates.csv`](../docs/dependencies/image-crates.csv), and the image's own list |
| `identifiers.md`, `identifiers.json` | The names the policy engine has built in, held against the desktop files, programs and units of the image (informational) |
| `tur-image-oci` (artifact of its own, kept five days) | The tested image as an OCI archive, with its image ID, build ID and source revision: what the release workflow publishes ([publishing.md](../docs/architecture/publishing.md), section 3) |

`scripts/sbom-licences.py sbom.spdx.json` turns the SBOM into the package and
licence list kept in
[`docs/dependencies/image-licences.csv`](../docs/dependencies/image-licences.csv).

## Identity

`/usr/lib/os-release` is TUR's: `NAME="TUR"`, `ID=tur`, `ID_LIKE=fedora`,
`VERSION_ID` and `IMAGE_VERSION` the TUR version, `BUILD_ID` the build,
`LOGO=tur-logo-icon`, `DEFAULT_HOSTNAME=tur-pc` (not `tur`: see TUR-008), and the project's addresses on
GitHub. `ID_LIKE=fedora` tells tools that look for the family (fwupd, for
example) where they are. `/etc/system-release` reads `TUR release <version>`;
the firmware boot entry that bootupd creates is named from it.
`/usr/lib/tur/release` holds the build identity and what the base called
itself (`TUR_BASE_OS_ID`, `TUR_BASE_OS_VERSION_ID`, `TUR_BASE_OS_VARIANT_ID`,
read before the swap); the build manifest takes the base's identity from there.

## Fedora's marks

Fedora's trademark guidelines require a system that mixes or modifies Fedora's
packages to remove `fedora-logos`, `fedora-release` and `fedora-release-notes`
(AGENTS.md: no image may be published before that). The final `RUN` of the
`Containerfile` runs [`branding/branding.sh`](branding/branding.sh), which
does it and replaces everything else that presents the system as Fedora
([desktop-experience.md](../docs/architecture/desktop-experience.md),
section 12; the full inventory, before and after, is in
[TUR-008](../docs/reports/TUR-008.md)):

| What | How |
|---|---|
| `fedora-logos`, `fedora-release-common`, `-kinoite`, `-identity-kinoite`, `-ostree-desktop` | Removed; Fedora's mark-free `generic-release`, `generic-release-common` and `generic-logos` installed instead, downloaded from Fedora's release tree and checked against the SHA-256 in [`branding/packages.lock`](branding/packages.lock). `system-release` and `system-logos` are theirs. The rpm-ostree polkit rule is kept; the systemd presets live in other packages and stay. The build fails if the swap leaves any dependency unmet |
| `os-release`, `/etc/system-release`, `system-release-cpe` | TUR's (section "Identity") |
| Logo | TUR's flat mark ([`branding/`](../branding/)) as `tur-logo-icon` in `hicolor`, as `start-here`, and as `/usr/share/pixmaps/system-logo-white.png`, which KDE's "About this system" shows |
| Boot splash | Plymouth theme `tur` ([`rootfs/usr/share/plymouth/themes/tur/`](rootfs/usr/share/plymouth/themes/tur/)), the default in `plymouthd.defaults` |
| initramfs | Regenerated with the arguments the base used (`--reproducible --add ostree --no-hostonly`), at the time of the build ID's date, so that it carries TUR's `os-release`, release file and splash |
| Desktop | Fedora's global themes removed; `org.tur.dark` (provisional, defaults only: Breeze's dark colours, TUR's wallpaper, no splash) is the default in kde-settings' `kdeglobals`; wallpaper `Tur`, also behind Fedora's names `Default` and `Fedora`; login screen (`rootfs/usr/lib/plasmalogin/plasmalogin.conf.d/10-tur.conf` and Fedora's `defaults.conf`) and lock screen (kde-settings' `kscreenlockerrc`) show it |
| Removed: `fedora-bookmarks`, `plasma-welcome-fedora`, `fedora-workstation-backgrounds`, `fedora-appstream-metadata`, `NetworkManager-config-connectivity-fedora` | Nothing requires them |
| Files deleted, record kept: `plasma-lookandfeel-fedora`, `f45-backgrounds-kde`, `f45-backgrounds-base` | Other packages require them; their licence files stay |
| Firefox | `tur-default-prefs.js` replaces Fedora's start page and pinned tab; `distribution.ini` no longer names the build "for Fedora"; if Fedora's bookmarks are compiled into the browser, the policy `NoDefaultBookmarks` keeps them out of new profiles |
| Contact nobody asked for | Installation counting (`rpm-ostree-countme.timer`) masked; the connectivity check against Fedora's server is gone with its package |

What stays, and why, is
[`tests/image/marks-allowlist.tsv`](../tests/image/marks-allowlist.tsv):
Fedora's repositories and signing keys, the `.fc45` tag, file names, comments,
and other projects' programs that mention Fedora. The `image` workflow fails
when anything else carries Fedora's name or a file of `fedora-logos` comes
back (`tests/image/marks.py`), and prints an inventory for people
(`tests/image/marks-inventory.sh`). The `boot` workflow boots the installed
disk once more with the splash shown and a user signed in, and fails if the
text on any screenshot names Fedora (`tests/boot/splash.py`,
`tests/boot/screenshot-text.py`).

The image CI builds still has the base's layers, Fedora's logos among them,
below TUR's; the image that is published is re-chunked so that no layer holds
them, and every layer of it is checked (`tests/image/marks.py --layers`;
[publishing.md](../docs/architecture/publishing.md), section 4). Not done
here: the time servers are still Fedora's NTP pool zone; the
splash is shown only with the `rhgb` or `splash` kernel argument, which the
image does not set. See TUR-008.

## Updating the base pin

The base is pinned by digest. The upstream registry publishes only a rolling tag,
so an old digest can eventually be removed upstream; until TUR mirrors its base,
the pin must be refreshed regularly.

Change `base.lock` in a pull request of its own, so that CI proves the new base
builds and passes the image tests before anything else depends on it.

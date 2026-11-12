# Third-party notices

## TUR's own code

The code and documentation written for TUR in this repository are licensed under
the [TUR Noncommercial License 1.0.0](LICENSE.md)
(`LicenseRef-TUR-Noncommercial-1.0.0`). The required copyright notice is in
[`NOTICE`](NOTICE).

Code in this repository that modifies an upstream project, or that is loaded
into one whose licence requires it, is not under the TUR licence. It carries the
upstream project's licence, lives in a marked directory with its own licence
file, and is identified by the SPDX header of each file
([ADR-0003](docs/adr/0003-licence.md)). Such files today:

- `image/third-party-licensed/grub/`: the GRUB configuration fragment that
  counts boot attempts (BSD-3-Clause). It is in the image.
- `services/tur-update/tests/fixtures/bootc-upstream/`: fourteen test samples
  converted from bootc's source tree (MIT OR Apache-2.0). Test data only; not
  in the image.

## Third-party software in the TUR image

TUR is built from software packaged by the Fedora Project and many other
projects. TUR is not provided, endorsed or supported by the Fedora Project or
by Red Hat. (This statement of origin is in the image as
`/usr/share/tur/statement-of-origin.txt`; its wording awaits legal review.)

The TUR operating-system image is built on Fedora Linux and contains
third-party software: 1,750 packages in build `20261112.gdf32bacdf174`, the
build this notice was written against. Each package is distributed under its
own licence. Most are under the GNU GPL or LGPL; others are under permissive
licences, font and documentation licences, and, for hardware firmware, vendor
licences that permit redistribution but not modification.

**TUR does not relicense any of these components.** The TUR Noncommercial
licence applies only to TUR's own code. It does not restrict, and cannot
restrict, your rights in the third-party components, which you may use, copy,
modify and redistribute under the terms of their own licences, including
commercially where those licences allow it.

| What | Where |
|---|---|
| Every package in the image, with its version and licence | [`docs/dependencies/image-licences.csv`](docs/dependencies/image-licences.csv) |
| The upstream projects TUR integrates, their licences and sources | [`docs/dependencies/upstream-projects.md`](docs/dependencies/upstream-projects.md) |
| Licence review, obligations and open questions | [`docs/dependencies/licence-review.md`](docs/dependencies/licence-review.md) |
| The exact contents of a given build | The SPDX file `sbom.spdx.json` produced with that build |
| Licence texts and copyright notices of each package | Installed by each package under `/usr/share/licenses/` in the image |

### Fedora's marks

The image does not contain Fedora's logo package `fedora-logos` or its
release packages `fedora-release-*`, as Fedora's trademark guidelines require
of a system that mixes or modifies Fedora's packages. In their place are
Fedora's own mark-free replacements, installed unmodified from Fedora's
release tree ([`image/branding/packages.lock`](image/branding/packages.lock)):

| Package | Licence | What TUR changes |
|---|---|---|
| `generic-release`, `generic-release-common` 45-0.4 | MIT | The identity files (`/usr/lib/os-release`, `/usr/lib/fedora-release`, `/usr/lib/system-release-cpe`) are rewritten with TUR's name |
| `generic-logos` 18.0.0-29.fc45 | GPL-2.0-only | Two icons named `start-here` and `/usr/share/pixmaps/system-logo-white.png` are replaced with TUR's mark; the rest is as Fedora ships it |

The polkit rule for rpm-ostree, `org.projectatomic.rpmostree1.rules`, is
Fedora's file from `fedora-release-ostree-desktop`, kept unchanged under its
MIT licence, which is in `/usr/share/licenses/tur-fedora-release-files/` in
the image. Packages that only presented the system as Fedora were removed, or,
where another package depends on them, their files were deleted and their
licence files kept ([TUR-008](docs/reports/TUR-008.md)).

TUR's mark, boot splash, wallpaper and the files under `branding/` are TUR's
own work under the TUR licence (a first draft; whether a software licence is
the right instrument for a brand is a question for the legal review,
[desktop-experience.md](docs/architecture/desktop-experience.md), 14.2).

## Third-party code inside TUR's own programs

TUR's programs are written in Rust and are compiled into the image
([`image/README.md`](image/README.md), "Programs built into the image"). Each
is one file that contains, besides TUR's own code, the Rust standard library
and the third-party crates the program depends on, linked statically. Those
parts keep their own licences: 69 crates and the standard library when this
section was written ([TUR-006](docs/reports/TUR-006.md)). All of
them are permissive: MIT, Apache-2.0, BSD-3-Clause, ISC, Unicode-3.0 and
CDLA-Permissive-2.0, most of them as a choice of MIT or Apache-2.0. Their
condition is that the licence text and the copyright notice accompany the
program, and the image carries them:

| What | Where |
|---|---|
| TUR's licence and required notice | `/usr/share/licenses/tur/` in the image |
| Which program contains which crate, with version, declared licence and checksum | `/usr/share/licenses/tur-third-party/crates.tsv` in the image; [`docs/dependencies/image-crates.csv`](docs/dependencies/image-crates.csv) |
| The licence files of each crate, as the crate ships them | `/usr/share/licenses/tur-third-party/<crate>-<version>/` in the image |
| The notices of the Rust standard library and of the crates it is built from | `/usr/share/licenses/tur-third-party/rust-std-<version>/COPYRIGHT-library.html` in the image, as the toolchain ships it |
| The same, machine-readable | The SBOM: each program is a package with a `STATIC_LINK` relationship to each crate in it |

Two crates carry C code that is compiled into a program: `zstd-sys` holds
Zstandard, which its authors offer under BSD-3-Clause or GPL-2.0-only and
which is taken here under BSD-3-Clause, in `tur-diag`; `ring` holds
cryptographic code derived from BoringSSL and OpenSSL (its `LICENSE` files
state the terms), in `tur-pax` and `tur-pax-providers`. The list also names
the procedural-macro crates a program was built with (`syn`, `quote`,
`serde_derive` and the like), which are not part of the program: one notice
too many rather than one too few.

A crate under the GPL or the LGPL may not be linked into a TUR program this
way. The `image` workflow fails if a crate's licence is not on the allow-list
in [`scripts/sbom-programs.py`](scripts/sbom-programs.py).

TUR does not yet publish an image. When it does, the corresponding source code
of the copyleft components must be available with each published image, as
their licences require. What has to be in place first is tracked in the licence
review.

## Trademarks

Fedora and the Fedora logo are trademarks of Red Hat, Inc. TUR is not provided,
endorsed or supported by the Fedora Project or Red Hat. Official Fedora software
is available from <https://fedoraproject.org/>. Other names are the trademarks
of their owners and are used only to identify the software concerned.

## Software TUR does not include

TUR does not include Microsoft or Apple software or Google Mobile Services.
Where TUR can run such software, you supply it yourself and are responsible for
holding the right to use it.

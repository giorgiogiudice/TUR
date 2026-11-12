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

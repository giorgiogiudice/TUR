<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Fixtures of tur-settings

`tur-settings --fixture DIR` reads what the system says it is from
`DIR/tur-release` and `DIR/os-release` instead of from `/usr/lib/tur/release`
and `/etc/os-release` (or `/usr/lib/os-release`). The tests and the pictures
use these sets.

| Set | What it stands for |
|---|---|
| `default/` | A TUR image as it is built today: `tur-release` as `image/Containerfile` writes it, and the base system's `os-release` with the `PRETTY_NAME`, `IMAGE_ID`, `IMAGE_VERSION` and `BUILD_ID` the same file stamps on it |
| `rebranded/` | A later image, once Fedora's marks are replaced (task D-03): `os-release` as the design's section 12.1 gives it |
| `not-tur/` | A system with no `tur-release`: the pages say that TUR could not read what the system is, with the code `TUR-DESKTOP-SETTINGS-010` |

**These files were written by hand**, after the lines of
`image/Containerfile` that write the real ones. They were not copied from a
built image: the builder that runs the tests has no TUR image in it. The
values in `default/` are those of the repository on 12 November 2026
(`VERSION`, `image/base.lock`). The base system's `os-release` holds more keys
than are here; only the ones this program reads, and the ones the stamp
writes, were kept. Nothing checks these files against a real image; the
end-to-end test of the design's case 6 is where that belongs.

Nothing here stands for a service that does not exist. The rows of Overview
that wait for the update service and for the record of each start have no
fixture: they say that they cannot tell, with or without one.

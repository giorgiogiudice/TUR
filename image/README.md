# UOS image

The UOS operating system is one bootc container image, defined by
[`Containerfile`](Containerfile) on a base pinned in [`base.lock`](base.lock)
([ADR-0001](../docs/adr/0001-base-distribution.md)).

| Path | Purpose |
|---|---|
| `Containerfile` | The image definition |
| `base.lock` | The pinned base image: repository, tag, digest, version |
| `rootfs/` | Files copied into the image. Everything lives under `/usr` |

## Building

Images are built in CI (the `image` workflow), or on a dedicated build machine:

```sh
scripts/build-image.sh localhost/uos:dev
tests/image/smoke.sh localhost/uos:dev "$(. <(podman run --rm localhost/uos:dev cat /usr/lib/uos/release); echo "$UOS_BUILD_ID")"
```

Do not build on a shared or production server: the base image alone is several
gigabytes.

A build is deterministic for a given commit. The layer timestamp is the commit
time and the build ID is `<commit date>.g<commit hash>`. CI builds the image twice
and fails if the two image IDs differ.

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
working, and identifies itself as UOS through `IMAGE_ID`, `IMAGE_VERSION` and
`BUILD_ID` in `/usr/lib/os-release` and through `/usr/lib/uos/release`. Full
rebranding is a Phase 3 task.

## Updating the base pin

The base is pinned by digest. The upstream registry publishes only a rolling tag,
so an old digest can eventually be removed upstream; until UOS mirrors its base,
the pin must be refreshed regularly.

Change `base.lock` in a pull request of its own, so that CI proves the new base
builds and passes the image tests before anything else depends on it.

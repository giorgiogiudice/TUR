<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# base-mirror-2 — Every pinned image mirrored; the desktop builder kept by digest

```text
TUR DEVELOPMENT REPORT

Task ID:          base-mirror-2 (issue 109; issue 38, first item)
Subsystem:        CI (ADR-0002): .github/actions/use-base-mirror,
                  base-mirror.yml and the workflows that pull pinned images;
                  the desktop builder image (desktop/ci, desktop-kit.md 15.3)
Build ID:         none (no OS image is produced by this change; the image,
                  boot and rollback workflows built and booted it as usual)
Source revision:  c7af116fe14e and 0fc8f7925cd8 (pull request 111), the
                  commits the measurements below come from. The first has no
                  cached and no pinned builder; the second pins the digest
                  the first one's run pushed and retries a failed copy to
                  ghcr.io. The commit after them makes the desktop jobs take
                  the image from the registry when the cache has lost it
                  (found on 0fc8f79, below); its runs are listed in the pull
                  request.

SUMMARY
- What changed:
  1. Pins are found, not listed. .github/actions/use-base-mirror/pins.py
     reads every tracked file under .github/, tests/ and scripts/, every
     *.lock but Cargo.lock and every Containerfile, and takes each
     sha256: digest there as a whole reference
     (REGISTRY/REPO[:TAG]@sha256:HEX) or as a P_IMAGE/P_DIGEST group
     (base.lock, builder.lock, the RUST_* ARGs). A digest it cannot place,
     or a pin whose repository has no mirror, fails base-mirror's lint.
  2. mirrors.tsv maps each upstream repository to a ghcr.io package:
     tur-base (kinoite, as before), tur-mirror-fedora (quay.io/fedora/fedora
     and registry.fedoraproject.org/fedora, same digests), tur-mirror-rust,
     -debian, -registry, -moto.
  3. base-mirror.yml copies every pin (skopeo copy --all
     --preserve-digests, tag sha256-<hex>, plus the version where the pin
     has one) and checks each copy by digest, tag and platform. It now runs
     when a workflow, lock file, Containerfile or the action changes, and
     daily. through-mirror runs only when the base pin or the mechanism
     changes, and also checks every quay.io pin and a docker pull of the
     builder's base with quay.io unreachable. pin-age covers every pin with
     a tag, in one issue ("Pinned images: N to re-pin, ...").
  4. The action writes one registries.conf table per mirror (podman,
     skopeo), and, for docker (`images:`), gives NAME_PULL (the mirror's
     reference when the mirror has the digest) and TUR_BUILD_CONTEXTS
     (docker build --build-context flags). Consumers: local-ai (fedora,
     compare), windows-drivers (three jobs), release (registry:3.1.1),
     release-installer (moto), reports (the receiver's rust and debian),
     desktop and pax (the builder's base). image, boot, rollback, update and
     iso get the new tables with no change (the rust stage of
     image/Containerfile now also tries its mirror).
  5. desktop/ci/builder.lock: base re-pinned to quay.io/fedora/fedora:45
     sha256:e15959f3... (the old sha256:fa773d10... is gone from quay.io).
     local-ai.yml carries the same re-pin as pull request 107, in identical
     lines, so the two merge cleanly in either order.
  6. Issue 38: the builder image lives in TUR's registry,
     ghcr.io/giorgiogiudice/tur-desktop-builder, pinned by BUILDER_DIGEST in
     builder.lock. It is used while its label org.tur.builder.inputs equals
     desktop/ci/builder-inputs.sh (Containerfile + base pin +
     BUILDER_REVISION). Otherwise the builder job builds it (base through
     the mirror), pushes it and prints the line to pin; a new check "The
     builder image is pinned" fails until that line is committed. The
     Actions cache only saves the other jobs a pull: a job that finds the
     cache empty takes the image from the registry (load-builder.sh), the
     pinned one or the one a run pushed for the same inputs
     (tag inputs-<hash>).
     workflow_dispatch input `rebuild` builds from scratch, under a cache
     key of its own, and keeps nothing.
  7. Because a kept image in a registry is distributed, the builder replaces
     fedora-logos and the fedora-release packages with Fedora's
     generic-logos and generic-release (AGENTS.md, licence rules), as the OS
     image does.
- Why it changed: quay.io removes a Fedora digest about a day after the tag
  moves. Only the OS base was mirrored (base-mirror-1); local-ai's pin and
  builder.lock's had already vanished, and the desktop workflow passed only
  because its builder came from the Actions cache.

PINNED DIGESTS FOUND (pins.py list; also git grep -nE 'sha256[:-][0-9a-f]{12,}')
- image/base.lock:6               quay.io/fedora/fedora-kinoite:45@sha256:cbc9be8a...  -> tur-base
- image/Containerfile:21          docker.io/library/rust:1.99.1-slim-trixie@sha256:f6c4a35d... -> tur-mirror-rust
- server/report-receiver/Containerfile:16  the same rust digest      -> tur-mirror-rust
- server/report-receiver/Containerfile:26  docker.io/library/debian:trixie-slim@sha256:a72b4433... -> tur-mirror-debian
- desktop/ci/builder.lock         quay.io/fedora/fedora:45@sha256:e15959f3... (was fa773d10...) -> tur-mirror-fedora
- .github/workflows/local-ai.yml  quay.io/fedora/fedora:45@sha256:e15959f3... (was fa773d10...) -> tur-mirror-fedora
- .github/workflows/windows-drivers.yml  registry.fedoraproject.org/fedora:45@sha256:fa773d10... -> tur-mirror-fedora
                                  (registry.fedoraproject.org still had it; kept, mirrored)
- .github/workflows/release.yml   docker.io/library/registry:3.1.1@sha256:325b4b29... -> tur-mirror-registry
- .github/workflows/release-installer.yml  docker.io/motoserver/moto:5.2.5@sha256:59a1965d... -> tur-mirror-moto
- desktop/ci/builder.lock         ghcr.io/giorgiogiudice/tur-desktop-builder@sha256:e319e96c... (TUR's own; no mirror)
- Not pins: base-mirror.yml NEVER_MIRRORED (marked "pins.py: ignore", must
  stay unmirrored); test fixtures under services/ and desktop/apps/ (data,
  never pulled); placeholder digests in Rust tests.

FILES
- Added: .github/actions/use-base-mirror/pins.py, mirrors.tsv;
  desktop/ci/builder-inputs.sh, fetch-builder.sh, push-builder.sh; this report.
- Modified: .github/actions/use-base-mirror/action.yml, setup.sh, check.sh,
  upstream.py; .github/workflows/base-mirror.yml, desktop.yml, pax.yml,
  local-ai.yml, windows-drivers.yml, release.yml, release-installer.yml,
  reports.yml; desktop/ci/builder.lock, build-builder.sh, load-builder.sh,
  builder/Containerfile; desktop/README.md; image/README.md ("The base image
  and its mirror"); docs/architecture/desktop-kit.md (the builder rows and
  the known gap).
- Removed: none.

BUILD
- Command: none locally (AGENTS.md). CI built the builder image
  (desktop run 43498648591, job "Builder image") and the OS image (image
  run 43498648466).
- Result: builder image built from the mirror and pushed:
  ghcr.io/giorgiogiudice/tur-desktop-builder:inputs-24d0d103c665045c =
  sha256:e319e96c9f19538229dc1f2bc11230b103fe53bdb95b0e7293b7c756e75682af.
- Warnings: every mirror package and the builder package are public (below).

TESTS (GitHub Actions, pull request 111, commit c7af116)
- Cache-cold rebuild of the builder: the cache key changed (new base, new
  Containerfile), so desktop run 43498648591 had no cached image and no pin.
  BuildKit took the base from the mirror: "[context quay.io/fedora/fedora@
  sha256:e15959f3...] load metadata for ghcr.io/giorgiogiudice/
  tur-mirror-fedora@sha256:e15959f3...". generic-release 45-0.4,
  generic-release-common 45-0.4 and generic-logos 18.0.0-29.fc45 replaced
  Fedora's; the system-release/system-logos checks passed.
- Screenshots against the references, with that image: "329 pictures
  rendered, 329 references, 329 match, 0 do not" (tolerance 8/255, 0 pixels
  beyond; every picture rendered twice, identical). No pixel shifted. The
  versions of every package a picture depends on (builder.packages: Qt
  6.11.2, freetype, harfbuzz, fontconfig, the fonts) equal the reference
  image's; builder.packages is unchanged.
- Build, lint, unit and QML tests, sanitizers, under KWin, application
  centre end to end, P@x's window: all passed (run 43498648591).
- "The builder image is pinned": failed as designed (nothing pinned yet) and
  printed BUILDER_DIGEST=sha256:e319e96c...; that line is in builder.lock in
  the next commit, whose runs take the image from the registry.
- docker through the mirror: local-ai 43498648792, windows-drivers
  43498648538, release-installer 43498648567, reports 43498648610 (named
  build contexts for the receiver), pax 43498648404: passed.
- podman through the drop-in with all seven tables: image 43498648466 and
  boot 43498648507 passed; rollback 43498648655: five of six jobs passed,
  "early-failures" failed in podman's storage before any pull ("checking if
  cached image exists from a previous build: getting top layer info: layer
  not known"); re-run by the next commit.
- base-mirror 43498648629: lint, pins.py check (10 pins found, all mirrored
  or TUR's own) and self-test passed; kinoite, fedora e15959f3, debian,
  registry and moto copied and checked per platform. Two copies failed with
  ghcr.io "500 Internal Server Error" on a blob upload (fedora fa773d10,
  rust); skopeo does not retry those, so pins.py now retries a copy up to
  four times. pin-age (dry run) passed. through-mirror was skipped because
  it needs the mirror job, which failed.

TESTS (commit 0fc8f79: builder pinned, copy retried)
- base-mirror 43516829594, all three jobs passed. mirror: the two missing
  copies made (fedora fa773d10 from registry.fedoraproject.org, rust), the
  rest "already mirrored"; every pin checked by digest, tag and platform.
  through-mirror: a never-mirrored digest failed in check.sh, pins.py
  resolve and podman pull; with quay.io unreachable ("Connection refused"),
  each quay.io pin (kinoite cbc9be8a, fedora e15959f3) resolved "from the
  mirror" as root and as the runner user; docker pulled the builder's base
  as ghcr.io/giorgiogiudice/tur-mirror-fedora@sha256:e15959f3...; the OS
  image built in 552 s, labels and stored base name quay.io and the pinned
  digest; smoke tests passed. pin-age (dry run): "Pinned images: 0 to
  re-pin"; windows-drivers' fa773d10 "tag moved; within 7 days".
- image 43516829971, boot 43516830109, rollback 43516829512 (all six jobs;
  the storage error of the run before did not recur): passed.
- local-ai 43516830061, windows-drivers 43516829967, release-installer
  43516829645, reports 43516829525, pax 43516829945: passed.
- desktop 43516829979: the builder job found no cache, took the pinned
  image from the registry ("pinned: ghcr.io/giorgiogiudice/
  tur-desktop-builder@sha256:e319e96c...", "builder image: pinned", pulled
  in 35 s) and saved it to the cache. The other jobs then waited hours for
  a runner, and the first to start, "P@x's window against the real
  service", FAILED: "Failed to restore cache entry ... fail-on-cache-miss".
  The entry had been evicted in between (the repository's caches stood at
  9.2 GB of the 10 GB limit, 57 entries). This is the weakness issue 38
  describes, inside one run. Fixed in the next commit: a job that finds the
  cache empty takes the image from the registry, so the cache is no longer
  needed for a run to pass.
- iso 43516829550 and release 43516829798 were still running, rust
  43516830072 still queued, when the next commit was pushed.
- NOT RUN: a workflow_dispatch run with `rebuild` (c7af116's run already
  built with nothing cached and nothing pinned); the release job's use of
  the registry image through the mirror, unless the release rehearsal of
  the last commit reaches it (see the pull request).
- Hardware tests: not applicable.

DIAGNOSTICS
- Log paths: base-mirror's job summary has a table of every pin, its
  mirror, the result and whether it is readable without credentials; every
  job that uses the action prints the drop-in, podman's reading of it as
  root and rootless, and for each docker image "pulled as ...". The
  builder job prints where its image came from (cache, pinned, built).

RECOVERY
- Rollback procedure: revert the merge. Workflows then pull from upstream
  only (as before for all but the base). The packages stay; deleting them is
  a manual step for the owner.
- Recovery tested: a digest never mirrored still fails clearly (through-
  mirror, unchanged check, plus pins.py resolve).

KNOWN LIMITATIONS
- Package visibility: ghcr.io/giorgiogiudice/tur-base is PUBLIC (as
  base-mirror-1 found). The packages this change created are public too:
  tur-mirror-fedora, -debian, -registry, -moto, and
  tur-desktop-builder (an anonymous registry token reads each; tur-mirror-
  rust did not exist yet when checked). Visibility was not changed: it is
  the owner's setting.
- tur-desktop-builder is a public image built from Fedora packages. Its
  Fedora marks are replaced, but it carries GPL and LGPL binaries (Qt, KDE
  Frameworks, KWin, GCC) whose sources are Fedora's; whether a public copy
  needs TUR to offer those sources itself is for the owner's legal review.
- docker reads no registries.conf: docker consumers use the mirror's
  reference (same digest, other name), so their logs name ghcr.io.
- A pin added outside the scanned paths (a new place) is caught only if a
  digest appears in a scanned file; pins.py scans .github/, tests/,
  scripts/, lock files and Containerfiles.
- The Actions cache is nearly full (9.2 GB of 10 GB on 14 November), so
  entries are evicted within hours. The desktop workflow no longer depends
  on it; other workflows' caches (cargo) only cost time when evicted.
- base-mirror now runs on every change to a workflow, lock file or
  Containerfile (about two minutes when the image build is skipped); a
  ghcr.io or Docker Hub outage then fails it.
- Scheduled paths (daily mirror, weekly issue writing) run only on main:
  NOT RUN here.

NEXT SAFE ACTION
- Owner: decide whether the mirror and builder packages may stay public
  (package settings), and the source-offer question above.
- Pull request 107 merged first; its local-ai comment ("this container
  image has no mirror") is corrected here. Main is merged in, with the
  event-driven release of pull request 115; docs/architecture/publishing.md,
  section 3, describes it.
```

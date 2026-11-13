<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# base-mirror-1 — A mirror that keeps the pinned base image pullable

```text
TUR DEVELOPMENT REPORT

Task ID:          base-mirror-1
Subsystem:        Image build (image/base.lock, ADR-0001) and CI (ADR-0002)
Build ID:         20261113.g05519ffdc5e4 (the image the through-mirror job
                  built from pull request 89 merged into main)
Source revision:  134628a35151 (ci/base-mirror, pull request 89), the commit
                  the measurements below come from. The commit that adds
                  this report follows it and changes no code.

SUMMARY
- What changed:
  1. .github/workflows/base-mirror.yml. Job "mirror" copies the digest in
     image/base.lock from quay.io/fedora/fedora-kinoite to
     ghcr.io/giorgiogiudice/tur-base with skopeo copy --all
     --preserve-digests, tagged <BASE_VERSION> and sha256-<hex>, and checks
     that the copy has the same digest by digest, by both tags and for
     every platform in the index. Runs on pull requests from this
     repository, pushes to main and by hand, when base.lock, the workflow
     or the action changes. Only this job has packages: write.
     Job "through-mirror" makes quay.io unreachable and builds the image
     from the mirror alone. Job "pin-age" runs weekly and opens or updates
     an issue "Base image pin is N days old" (issues: write, only there);
     on a pull request it only reports.
  2. .github/actions/use-base-mirror: logs podman/skopeo in to ghcr.io
     with the job's token, as the runner user and as root, writes a
     registries.conf.d drop-in that makes the package a
     mirror-by-digest-only mirror of quay.io/fedora/fedora-kinoite, and
     says, for the pin, whether the mirror and quay.io have it. It fails
     with an explanation when neither has it.
  3. image, boot, rollback and update use the action: one step after
     checkout and packages: read in the job that pulls the base.
  4. gate lists base-mirror. image/README.md: "The base image and its
     mirror".
- Why it changed: on 13 November 2026 quay.io removed the pinned
  sha256:f857f6c5... ("manifest unknown") about a day after Fedora moved
  :45, and image, boot, rollback and iso failed everywhere.
- The image reference the build uses is unchanged: the labels, the build
  manifest and the reproducibility check still name quay.io and the digest.

FILES
- Added: .github/workflows/base-mirror.yml; .github/actions/use-base-mirror/
  (action.yml, setup.sh, check.sh, upstream.py); this report.
- Modified: .github/workflows/image.yml, boot.yml, rollback.yml,
  update.yml (one step and a job permissions block each), gate.yml (one
  name), image/README.md (one section).
- Removed: none.

TESTS (all in GitHub Actions, pull request 89; nothing was built or pulled
on a development machine)
- Mirror copy of the current pin, sha256:cbc9be8a... (45.20261113.0), with
  its two platform manifests (aarch64 sha256:3100f7de..., x86_64
  sha256:97c16235...): copied in 134 s on the first run, 4-6 s per tag
  afterwards; every check "ok" (same digest by digest, by tag
  45.20261113.0, by tag sha256-cbc9be8a..., and per platform).
- Build with quay.io unreachable (job through-mirror, 134628a):
  /etc/hosts maps quay.io and its CDN names to 0.0.0.0; curl and a direct
  registry request to quay.io fail with "Connection refused". The pin then
  resolves from the mirror as root and as the runner user; skopeo --debug
  shows "Trying to access ghcr.io/giorgiogiudice/tur-base@sha256:cbc9be8a...".
  scripts/build-image.sh pulled quay.io/fedora/fedora-kinoite@sha256:cbc9be8a...
  and built in 481 s (image ID 863a39866bd3...). The image's labels
  org.tur.base.image = quay.io/fedora/fedora-kinoite and
  org.tur.base.digest = sha256:cbc9be8a...; the base is stored under its
  quay.io name and digest. tests/image/smoke.sh passed.
- A digest never mirrored: check.sh and sudo podman pull of
  quay.io/fedora/fedora-kinoite@sha256:f857f6c5... fail. podman says
  "(Mirrors also failed: [ghcr.io/giorgiogiudice/tur-base@...: manifest
  unknown]): quay.io/fedora/fedora-kinoite@...: manifest unknown"; the
  action's error says neither has it and to re-pin.
- Weekly reminder: on the pull request it read quay.io's :45 as
  sha256:cbc9be8a... (45.20261113.0), found the pin 0 days old and up to
  date (before the re-pin was merged into the branch: sha256:f857f6c5...,
  1 day old, within 7 days), and printed the issue it would write. The scheduled path that writes an issue is NOT RUN: a
  schedule runs only on main.
- Boot tests: the boot and rollback workflows ran with the action on this
  pull request (see the pull request's runs).

DIAGNOSTICS
- Log paths: the base-mirror run's job logs; each job that uses the action
  prints the drop-in, podman's reading of it for root and rootless, and
  the mirror/upstream/resolves lines.

RECOVERY
- Rollback procedure: revert the merge. The jobs then pull from quay.io
  only, as before. The package stays; deleting it is a manual step.

KNOWN LIMITATIONS
- The package is PUBLIC. It was meant to be private (the documented default
  for a new package). GitHub created it public: the docs say a package
  published by a workflow is linked to its repository, and this public
  repository's package came out readable without credentials (the
  workflow's anonymous read succeeds). Its visibility was not changed here.
- Only digests copied while quay.io still had them are kept; the old
  sha256:f857f6c5... is lost. A re-pin must be merged, or at least its
  pull request run, within about a day.
- skopeo 1.13 and podman 4.9 on ubuntu-24.04 read registries.conf.d even
  when CONTAINERS_REGISTRIES_CONF names another file (newer man pages say
  they do not), so check.sh asks quay.io through its registry API directly
  (upstream.py) to tell the two apart.
- On the hosted runners sudo keeps the runner user's XDG_RUNTIME_DIR, so
  root uses the runner user's auth file. The action logs the runner user
  in first; a root login first created a root-owned
  /run/user/1001/containers and broke rootless podman (first run).
- iso.yml does not use the action yet (owned elsewhere), so the iso
  workflow still depends on quay.io having the pin.
- Container storage on ghcr.io is free today (GitHub's billing page); each
  pin adds about two platform images.

NEXT SAFE ACTION
- Add the action to iso.yml (product and proof jobs). Decide whether the
  package may stay public; if not, change it in the package settings.
```

## Sources

- containers-registries.conf(5), current:
  https://github.com/containers/container-libs/blob/main/image/docs/containers-registries.conf.5.md
  ("mirrors will only be used during pulling if the image reference includes
  a digest"; mirrors "are attempted in the specified order", the primary
  location "is tried last"; drop-ins in /etc/containers/registries.conf.d and
  $HOME/.config/containers/registries.conf.d; tables with the same prefix are
  overwritten).
- GitHub, Working with the Container registry: "When you first publish a
  package, the default visibility is private"; publishing with GITHUB_TOKEN
  links the repository automatically, and publishing repositories get admin.
- GitHub, Billing for GitHub Packages: "Container image storage and bandwidth
  for the Container registry is currently free."

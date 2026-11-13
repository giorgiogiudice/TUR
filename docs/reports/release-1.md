<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# release-1 — Publishing the tested image as signed updates (ADR-0023)

```text
TUR DEVELOPMENT REPORT

Task ID:          release-1 (ADR-0023; pull request 78)
Subsystem:        Release workflow (.github/workflows/release.yml,
                  scripts/release/), image (container policy, trust roots),
                  image workflow (export), boot tests (signed-update test),
                  image tests (marks.py --layers)
Build ID:         20261113.g0d77e57346de (pull request 78 at abc709e as
                  GitHub merges it for testing), re-chunked as
                  sha256:bf20fd4b3aff7c7106d5661b976317cdf86168694842286fcc84208a680ab293
Source revision:  abc709e (feat/publish-signed); the final runs are in
                  "CI runs"

SUMMARY
- What changed: a release workflow that publishes the image the image
  workflow built and tested, never rebuilt: it waits for every workflow on
  the commit, takes the exported image (checked by image ID), re-chunks it
  so that no layer holds Fedora's removed logos, checks every layer and
  every file, signs it (release key with a Rekor entry; keyless with the
  workflow's identity; build manifest and SBOM attestations), proves on a
  booted TUR guest that the computer accepts and starts exactly that digest
  and refuses unsigned, wrongly signed and unlogged images before anything
  is staged, rehearses publishing and promotion, and publishes to
  ghcr.io/giorgiogiudice/tur only when the owner sets TUR_PUBLISH=1. The
  image carries the container policy and the trust roots that make the
  computer check.
- Why it changed: ADR-0023. Decided differently, on measured evidence: the
  computer checks TUR's release key and the public log, not the workflow's
  keyless identity, which its container libraries cannot match (amendment
  at the end of ADR-0023).

FILES
- Added: .github/workflows/release.yml; scripts/release/ (await.py,
  tested.py, rechunk.sh, compare-trees.py, variant.py, sign.sh, publish.sh,
  trust-roots.py); image/signing/policy.py;
  image/rootfs/usr/share/containers/registries.d/tur.yaml;
  image/rootfs/usr/share/tur/signing/ (rekor.pub, fulcio.crt, README);
  tests/boot/signed-update.py;
  tests/boot/guest/rootfs/usr/libexec/tur-boot-test/signed-update-agent;
  docs/architecture/publishing.md; docs/reports/release-1.md
- Modified: image/Containerfile (one RUN), image/README.md,
  .github/workflows/image.yml (export of the tested image),
  .github/workflows/gate.yml (release in the list),
  tests/image/marks.py (--layers, --self-test), tests/image/smoke.sh,
  tests/boot/guest/Containerfile and guest-agent (the new agent),
  docs/adr/0023-publishing-signed-updates.md (amendment note only)
- Removed: none.

BUILD
- Command: scripts/build-image.sh (CI); rpm-ostree compose
  build-chunked-oci (release workflow)
- Result: image built and tested (image run 43413472372); re-chunked in
  494 s into 65 layers.
- Warnings: cosign 3.1.4 prints "deprecated" for --new-bundle-format,
  --use-signing-config, --tlog-upload, --sign-container-identity and
  `copy`; each still works and each result is verified.

TESTS
- Image tests: pass, on the tested image (image workflow) and on the
  re-chunked one (release workflow: smoke tests; marks: 0 findings in the
  merged image and in all 65 layers).
- Same files: 473,952 entries of the tested image against 474,249 of the
  re-chunked one; the only differences are 1607 objects of OSTree's object
  store under /sysroot/ostree/repo/objects.
- Signing: both signatures and the build-manifest attestation verify with
  cosign; both name ghcr.io/giorgiogiudice/tur and the digest, and have a
  Rekor entry. The SBOM attestation was accepted by Rekor.
- Boot tests: the signed-update test in a booted guest (see "What the
  in-guest test proves"); the boot and rollback workflows pass with the
  new policy in the image, so bootc install and the oci: scenarios work.
- Hardware tests: NOT RUN.
- Performance tests: NOT RUN.

DIAGNOSTICS
- Artifacts: tur-release (tested.json, rechunk.txt, trees.json, marks/,
  signing/, prepare/, signed-update/ with the guest's reports, serial log
  and bootc's output for every case); tur-image-oci (the tested image).
- Relevant error IDs: none.

RECOVERY
- Rollback procedure: revert the pull request. The image change is one rule
  in the container policy, for one repository; nothing under /etc.
- Recovery tested: the new version booted in the guest was marked good by
  boot health; the rollback scenarios ran on an image with the change.

KNOWN LIMITATIONS
- The computer checks a long-lived release key (with the public log), not
  the workflow identity: ADR-0023 amendment.
- Nothing is published until the owner switches it on (publishing.md,
  section 8).
- Builds of one commit have the same image ID only on some runners.
- Key rotation, a forward-only check for testing, and release notes are
  not built.

NEXT SAFE ACTION
- Owner: the switch-on checklist below.
- Update manager (executing slice): expect spec.image.signature "insecure"
  for TUR's repository; the policy rule does the checking.
```

## Measurements before the design (release workflow, runs 43318262695 and 43318809905)

A temporary job of pull request 78 (commits d949ab8 and ff5c89c, removed
afterwards) looked at the pinned base
(`quay.io/fedora/fedora-kinoite@sha256:f857f6c5…`, 45.20261112.0) and at
cosign v3.1.4, and tried signatures against the base's own skopeo:

| What | Measured |
|---|---|
| Versions in the base | containers-common 0.69.2, skopeo 1.24.1, podman 6.1.3, bootc 1.17.1, rpm-ostree 2026.3, ostree 2026.4, python3 3.15.2, jq 1.8.2; no cosign |
| Where the policy is | `/usr/share/containers/policy.json` (containers-common, vendor file); **no** `/etc/containers/policy.json`. `containers-policy.json(5)` as shipped: `$XDG_CONFIG_HOME`, then `/etc/containers/policy.json`, otherwise `/usr/share/containers/policy.json` |
| The base's policy | default `insecureAcceptAnything`; `docker` scopes `registry.access.redhat.com` and `registry.redhat.io` (sigstoreSigned, Red Hat's key and Rekor key); `docker-daemon` accept |
| registries.d | vendor files in `/usr/share/containers/registries.d/` (`default.yaml`, Red Hat's two with `use-sigstore-attachments`); `/etc/containers/registries.d/` empty, for administrators (`containers-registries.d(5)`) |
| Re-chunking | `rpm-ostree compose build-chunked-oci` present (`--bootc`, `--format-version 1|2`, `--max-layers` default 64, `--previous-build`, `--label`, `--output oci:…`); no `bootc-base-imagectl` in Kinoite |
| `bootc switch --enforce-container-sigpolicy` | "enforces that containers-policy.json … includes a default policy which requires signatures" |
| cosign 3.1.4 | signs in the bundle format by default (an OCI referrer, tag `sha256-<digest>` without `.sig`); `--new-bundle-format=false --use-signing-config=false` give the attachment format (`sha256-<digest>.sig`, simple-signing payload, annotations `dev.cosignproject.cosign/signature`, `dev.sigstore.cosign/bundle`); these flags, `--tlog-upload` and `--sign-container-identity` print "deprecated" but work |
| Keyless identity in this repository's CI | `https://github.com/giorgiogiudice/TUR/.github/workflows/release.yml@refs/pull/78/merge`, issuer `https://token.actions.githubusercontent.com`; Rekor v1 (`rekor.sigstore.dev`) entry with a signed entry timestamp |

The base's skopeo against a policy with one scope,
`ghcr.io/giorgiogiudice/tur`, fetched through a `registries.conf.d`
redirect from a registry in the job:

| Image at the scope | Policy: `keyPath` + `rekorPublicKeyPath` | Policy: `fulcio` (GitHub issuer) + `rekorPublicKeyPath` |
|---|---|---|
| signed with the key, in Rekor | **accepted** | refused: missing `dev.sigstore.cosign/certificate` |
| unsigned | refused: "A signature was required, but no signature exists" | refused: same |
| signed with another key, in Rekor | refused: "No public key verified against the RekorSET" | refused: missing certificate |
| signed with the key, not in Rekor | refused: "missing dev.sigstore.cosign/bundle annotation" | refused: same |
| signed keyless by the workflow | refused: no key verified | **refused: `Required email "…" not found (got [])`** |

Also measured: without a `registries.d` entry for the registry the image is
actually fetched from, a signed image is refused ("no signature exists"):
containers/image looks attachments up at the physical location. The same
image pulled by the registry's own name is accepted unsigned: the scope is
the name asked for. A copy between two OCI layouts is unaffected.

## CI runs

Pull request 78. Every push to the pull request runs `image`, `boot`,
`rollback` and `iso` (it changes image/), then `release`.

| Commit | release run | What happened | Fixed by |
|---|---|---|---|
| 1822302 | 43321940766 | lint: three shellcheck notes | af622bf |
| af622bf | 43324878104 | Re-chunk: rpm-ostree opens the output first and failed on an OCI layout that did not exist (`OpenImageOptional: open index.json`) | 5d5ca9c: output to containers-storage, then the layout |
| 5d5ca9c | 43336236365 | Same files: 1568 differences, all OSTree objects under `/sysroot/ostree/repo/objects`; marks: the base was not in the store | 56c727e |
| 56c727e, ddbec4d | none | not run: the pull request conflicted with main | merges 772e154, abc709e |
| 88fd265 | 43356225989 | marks --layers: 64 findings, all `/sysroot/ostree/repo/config` repeated in every layer | ddbec4d |
| abc709e | 43413472528 | everything passed up to the guest; in the guest every case behaved as required, but the check looked for a message ("Source image rejected") that bootc does not print | FINAL |
| FINAL | FINAL | FINAL | |

Timing of a full run (abc709e; push at 11:00 UTC, free runners): `image`
21 minutes, `boot` 33, `rollback` 59, `iso` 76; the release workflow's wait
ended at 12:17 (77 minutes), and its release job took 36 minutes: about 11
minutes of re-chunking, 4 comparing the files, 1 for the marks, 9 installing
the disk, 3 preparing it, 3 to 4 for the test in the guest (the switch to the
signed image itself, over the guest's user-mode network: 113 s).
From push to the end of the release job: 1 hour 54 minutes. Under load, on
af622bf, the release job started two hours after the push.

## What the in-guest test proves

From release run 43413472528 (the refusal messages from its `switch-*.txt`;
FINAL for the run in which every check passes). The guest is the tested
image with the boot test's agent, installed to a disk, prepared by the boot
test's first two boots; publishing was off, so a throwaway key stood in for
the release key, mounted over `/usr/share/tur/signing/release.pub`; the
policy file was the image's own (check S1).

| Case | `bootc switch ghcr.io/giorgiogiudice/tur:<tag>` | Staged, deployments, image refs, repository objects |
|---|---|---|
| unsigned | exit 1: `failed to invoke method OpenImage: A signature was required, but no signature exists` | nothing; 1 deployment; 84 → 84; 134449 → 134449 |
| other-key | exit 1: `… OpenImage: No public key verified against the RekorSET: publicKey in Rekor SET does not match` | unchanged, as above |
| no-log | exit 1: `… OpenImage: missing dev.sigstore.cosign/bundle annotation` | unchanged, as above |
| the image under release | exit 0 in 113 s; staged `sha256:bf20fd4b…` | then restarted: the new version runs `sha256:bf20fd4b…`, build 20261113.g0d77e57346de, has no test agent, and boot health marked it good |

`bootc switch --enforce-container-sigpolicy` (informational): exit 1,
`containers-policy.json specifies a default of insecureAcceptAnything;
refusing usage`.

So: with plain `bootc switch`, the policy the image ships refuses each wrong
image when bootc's image proxy opens it, before a layer is fetched, and
accepts the signed one; the digest the guest staged and then ran is the
digest that was published to the job's registry and copied in the rehearsal.

Not proven here: the real release key (it does not exist; with publishing
on, the same test runs with the image's own `release.pub`); fetching from
ghcr.io (the guest fetched from the job's registry under ghcr.io's name);
the `promote` job and the push to ghcr.io (never run; the same script is
rehearsed against a second repository of the job's registry, where the copy
by digest, the immutable build ID, the signature tags and the promotion to
`stable` were checked).

## Findings

1. **Builds of one commit do not always have the same image ID.** In
   release run 43413472528, the image workflow's image was
   `sha256:26860e7b…`; three rollback groups booted that image, the boot
   workflow and one rollback group booted `98dba462…`, two rollback groups
   `a78a590d…`. On af622bf: five of six rollback groups the same, boot
   another. Earlier, for d447343 on main, the image and boot workflows
   differed in the layer of `08_tur_boot_count.cfg`'s `COPY` and in the
   branding layer, on runner images 20261101 and 20261111. The image
   workflow's reproducibility check builds twice on one runner, which does
   not show this. Worth an issue: the release therefore boots the published
   image itself instead of relying on the other workflows' image IDs.
2. **The computer's container libraries cannot check a keyless workflow
   identity** (measurements above; container-libs pull request 625).
3. **bootc passes the policy's reason on** as `failed to invoke method
   OpenImage: <reason>`, without containers/image's "Source image rejected"
   prefix that skopeo prints.
4. **bootc's `--enforce-container-sigpolicy` is unusable** while the default
   policy accepts anything; plain `bootc switch` records
   `ostree-unverified-registry` and the policy is applied all the same.
   update-manager.md, section 11, expects `containerPolicy`.
5. **The base pin vanishes from quay.io** about a day after Fedora moves
   `:45` (pull request 88, the base mirror of pull request 89); the release
   job uses the mirror for the base it reads fedora-logos' checksums from.

## Decisions the owner may want to reverse

- A long-lived release key, checked with the public log, instead of the
  keyless identity (ADR-0023 amendment); alternatives: a cloud KMS key
  reached with the workflow's OIDC token, or a Google service account
  through workload identity federation.
- The policy in the vendor file in `/usr`, not `/etc`.
- Re-chunking with format version 2 and rpm-ostree's default layer limit.
- `boot` must pass on the commit, but its image ID need not match the
  released one (finding 1).
- On pull requests, the release job runs with `packages: write` (it never
  pushes there; for pull requests from this repository it is no boundary,
  since such a pull request can change the workflow).
- Every run enters test signatures (throwaway keys, the workflow's pull
  request identity) in Sigstore's public log.

## Switching publishing on (the owner)

publishing.md, section 8:

1. On your own computer: `cosign generate-key-pair --output-key-prefix
   tur-release` (cosign 3.1, checksum checked), with a long password.
2. Repository secrets `TUR_RELEASE_SIGNING_KEY` (all of `tur-release.key`)
   and `TUR_RELEASE_SIGNING_PASSWORD`; keep the key offline or delete it,
   never commit it.
3. A pull request adding `tur-release.pub` as
   `image/rootfs/usr/share/tur/signing/release.pub`.
4. Repository variable `TUR_PUBLISH` = `1`.
5. After the first publication: package `tur` → Package settings → Change
   visibility → Public; Manage Actions access: `giorgiogiudice/TUR`, Write.
6. On the test PC, on an image built after step 3: `sudo bootc switch
   ghcr.io/giorgiogiudice/tur:testing`.

## Follow-ups worth an issue

1. Reproducible image IDs across runners (finding 1).
2. When container-libs matches a workflow identity (pull request 625) and it
   is in the base: the policy checks the keyless signature; retire the key.
3. Key rotation with `keyPaths`.
4. `testing` moves forward only.
5. Release notes from the merged pull requests (ADR-0023, consequences).
6. update-manager.md: `signature: insecure` for TUR's repository; the
   policy rule does the checking (finding 4).
7. cosign is moving to its bundle format, which containers/image does not
   read (container-libs issue 388): do not move past the pinned cosign
   until it does.

# ADR-0023: Publishing updates — signed images in a public registry, on channels

- **Status:** Accepted
- **Date:** 2026-11-12
- **Related:** [ADR-0014](0014-boot-health-and-rollback.md) (boot health),
  [ADR-0017](0017-update-manager.md) (update manager), issue #11 (no image may be
  published while it carries Fedora's marks)

## Context

The owner asked whether TUR can push updates to computers. CI builds the image
reproducibly on every change, boot health undoes an update that does not start,
and the update manager can decide what to do. Nothing publishes an image,
nothing proves an image came from TUR, and nothing applies an update.

## Decision

1. **Images are published to GitHub's container registry** (`ghcr.io`, public,
   free for a public project) by a release workflow in CI, never from a
   person's machine. The image published is the image CI built and tested: the
   same digest, promoted, not rebuilt.
2. **Every published image is signed in CI with keyless signing** (Sigstore:
   the workflow's own short-lived identity, recorded in a public transparency
   log), so no long-lived signing key exists to be stolen. The exact tooling
   (cosign, or the signature format containers/image verifies natively) is
   chosen by the implementing change from current upstream documentation,
   because what matters is that **the computer verifies it**.
3. **The TUR image carries its own verification policy**: the container policy
   for TUR's registry path accepts only images signed by TUR's release
   workflow on TUR's repository. An unsigned or wrongly signed image is refused
   by the computer before anything is staged.
4. **Channels**: `testing` and `stable` as tags of the registry path, plus an
   immutable tag per build. A build is published to `testing` when every
   workflow on `main` is green, including the rollback scenarios. It is
   promoted to `stable` by a person's decision after it has run on the test
   PC. A computer follows one channel; the owner's test PC follows `testing`.
5. **Nothing is published until issue #11 is closed** (Fedora's marks
   replaced). The release workflow is built and tested before that against a
   private or throwaway registry path, and switched on afterwards.
6. **Applying an update stays the update manager's job**, in its executing
   slice, under ADR-0017 (three steps, nothing staged that boot health could
   not undo), proven in the rollback harness before it ships.

## Consequences

- The full loop becomes possible: a problem is reported, fixed in a pull
  request, tested by CI, published to `testing`, installed and checked by boot
  health on the test PC, then promoted to `stable`.
- The registry and the transparency log are public: anyone can see which
  images TUR released and verify them.
- Registry outage means no updates, never a broken computer.
- Release notes are generated from the merged pull requests and shipped with
  the image, so the update manager can show what changed.

## Amendment: what the computer checks (13 November 2026)

Decided differently by the implementing change, on measured evidence
([publishing.md](../architecture/publishing.md), section 5;
[release-1](../reports/release-1.md)). For the owner to confirm or reverse.

1. **The computer checks TUR's release key, and the public log.** The
   container libraries bootc uses (containers/image, in Fedora 45's skopeo
   1.24.1, and on its main branch) match a Fulcio certificate's signer only
   by e-mail address. A GitHub Actions certificate names the workflow as a
   URI and has no e-mail, so a policy cannot say "signed by TUR's release
   workflow on TUR's repository" (measured: `Required email … not found (got
   [])`; upstream proposal container-libs pull request 625, open). The
   policy for `ghcr.io/giorgiogiudice/tur` therefore requires a sigstore
   signature by **TUR's release key** that is **entered in Sigstore's public
   Rekor log** (checked offline). That key is long-lived, contrary to
   decision 2: it is a GitHub Actions secret that only the release job reads,
   and because the computer also requires the log entry, any signature made
   with it, stolen or not, is public.
2. **The keyless signature of decision 2 is made as well**, for the
   workflow's identity, in the same format, and anyone can verify it with
   cosign. When TUR's base carries container libraries that match a workflow
   identity, the policy switches to it and the key is retired.
3. **The signature format** is the one containers/image reads: sigstore
   attachments (`sha256-<digest>.sig`) with a simple-signing payload, as
   cosign writes them with `--new-bundle-format=false`. cosign's newer bundle
   format is not readable by the computer yet.
4. **The policy is a vendor file**: Fedora 45 ships
   `/usr/share/containers/policy.json` and no `/etc/containers/policy.json`;
   TUR adds its rule to the vendor file in `/usr`, so every update brings it,
   and leaves `/etc` to the computer's owner.
5. **"The same digest, promoted, not rebuilt"** is kept for the image the
   `image` workflow tested, by image ID; the published image is that image
   re-chunked (so that no layer still holds Fedora's logos), and its own
   digest is the one booted by the release workflow and published. The
   `boot` and `rollback` workflows boot their own builds of the same commit,
   which do not yet have the same image ID across runners.

Alternatives that keep decision 2 whole, each needing the owner's accounts:
a signing key in a cloud KMS reached with the workflow's OIDC token, or a
Google service account's identity through workload identity federation (a
Fulcio certificate with an e-mail address, which the policy can match).

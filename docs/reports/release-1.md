<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# release-1 — Publishing the tested image as signed updates (ADR-0023)

DRAFT

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

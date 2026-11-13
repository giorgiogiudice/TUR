<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Publishing TUR's image as signed updates

- **Status:** built and tested in CI; **publishing is off** until the owner
  switches it on (section 8). Nothing has been published.
- **Decision:** [ADR-0023](../adr/0023-publishing-signed-updates.md), with the
  amendment at its end (the computer checks a release key, not the workflow's
  identity).
- **Related:** [update-manager.md](update-manager.md) (applying an update),
  [boot-health-and-rollback.md](boot-health-and-rollback.md) (what undoes a
  bad one), [image/README.md](../../image/README.md), report
  [release-1](../reports/release-1.md).

## 1. In one paragraph

The `release` workflow publishes the image that the `image` workflow built and
tested, without building it again. It waits until every workflow on the commit
has passed, takes the tested image as that workflow exported it (checked by its
image ID), re-chunks it so that no layer holds what a later layer removed,
checks every layer for Fedora's marks, signs it, and proves on a booted TUR
guest that the computer's own container libraries accept that signed image and
refuse an unsigned one, one signed with another key and one whose signature is
not in the public transparency log, before anything is staged. Then it copies
that exact digest, with its signatures, to `ghcr.io/giorgiogiudice/tur` under
an immutable build ID and the `testing` tag. A person promotes a build to
`stable`. Every TUR image carries a container policy that accepts images of
that repository only with TUR's signature.

## 2. What is published, and where

| | |
|---|---|
| Repository | `ghcr.io/giorgiogiudice/tur` (GitHub's container registry; public once the owner makes the package public) |
| Immutable tag | the build ID, `<commit date>.g<12 hex digits of the commit>`, e.g. `20261113.g1822302ab3cd`. Written once; a second release of the same build must name the same digest or stops |
| `testing` | the newest build released from `main`. The owner's test PC follows it |
| `stable` | a build a person promoted after it ran on the test PC (section 9) |
| Signatures | beside the image, tag `sha256-<digest>.sig` (section 5) |
| Attestations | beside the image, tag `sha256-<digest>.att`: the build manifest and the SBOM |
| Labels | those of the tested image: `org.tur.build-id`, `org.opencontainers.image.revision`, `.version`, `.source`, `org.tur.base.*` |

A computer follows one channel: `bootc switch ghcr.io/giorgiogiudice/tur:testing`
(or `:stable`). Applying updates stays the update manager's job under
ADR-0017, in its executing slice.

## 3. When a build is released, and which image

`.github/workflows/release.yml` runs on every push to `main`, on manual start,
and on pull requests that change the release itself (then as a rehearsal,
section 7).

**Waiting** (job `await`, `scripts/release/await.py`). Workflows run only when
their paths change, and `workflow_run` would fire once per finished workflow,
so the job instead lists the commit's runs through the API every minute until
none is running. For each workflow the latest run counts (push and manual runs
on `main`; the pull request's runs on a pull request). `gate`, `pax-live` (a
test against a live provider, excluded from the gate too) and the release
workflow itself do not count. The image is released only if:

- the `image` workflow ran and passed on the commit (it built and tested the
  image and exported it), and the `boot` workflow ran and passed on it;
- every other workflow that ran on the commit passed, the rollback scenarios
  among them when the commit changed what they test.

A failure stops the release with the failing workflows named. A commit on
which `image` did not run has nothing to release. A newer push to `main`
cancels the release of an older commit that is still waiting.

**The image** (`scripts/release/tested.py`). The `image` workflow, after all
its checks pass (smoke tests, the reproducibility build, Fedora's marks, SBOM,
build manifest), exports the image it tested as an OCI archive with its image
ID, build ID and source revision (artifact `tur-image-oci`, kept five days).
The release workflow downloads that archive, checks that the configuration it
holds has exactly that image ID, and loads it. The image ID is the digest of
the image's configuration, which names every layer by the digest of its
uncompressed content: an archive that passes is the tested image, whatever
compression it travelled in. The build ID and revision labels are checked
too.

The `boot` and `rollback` workflows build the same commit again and record
the image ID they booted. Those IDs are compared and recorded in
`tested.json`, not required to match: as measured (release-1, finding 1),
builds of one commit have the same image ID on some runners and not on
others. The
image that is published is booted by the release workflow itself (section 7).

**The digest that is published is the digest that was tested.** The
re-chunked image's manifest digest is computed once, from the OCI layout the
re-chunking wrote. That digest is pushed to the job's registry (read back and
compared), signed by digest, switched to and started by the booted guest
(which reports the digest it runs), and copied with
`skopeo copy --preserve-digests` (which refuses any copy that would change a
digest) to the published repository, with its signature and attestation
tags (`cosign copy --only=sig,att`); the build ID tag, the channel tag, the
digest and each signature tag are read back there and compared
(`scripts/release/publish.sh`).

## 4. Re-chunking

The image CI builds has the base's layers, with Fedora's logos in them, and
TUR's layers on top that delete them again; whoever pulls the image downloads
both ([desktop-experience.md](desktop-experience.md), 12.1, item 3; issue
#77). `scripts/release/rechunk.sh` runs `rpm-ostree compose
build-chunked-oci --bootc --format-version 2` (rpm-ostree 2026.3, from the
tested image itself; upstream `docs/build-chunked-oci.md`) on the tested
image. It writes the image's merged file system again as a bootc image whose
layers are cut by package (65 layers in release-1, with rpm-ostree's default
limit of 64 chunks), each
file in exactly one layer, except OSTree's repository configuration, which
the format repeats, identical, in every layer. Format version 2 puts parent directories in every layer,
so their metadata does not depend on unpacking order (the upstream
recommendation for reproducible output); there is no earlier published image
that version 1 would keep compatible. Once a build is published, the next one
is cut with `--previous-build docker://ghcr.io/giorgiogiudice/tur:testing`, so
the layer plan stays and an update changes the layers of the packages it
changes: a computer downloads those. The tool runs in a privileged container
with a container store of its own; the labels of the tested image are carried
over.

Three checks follow, each required:

- **Same files** (`scripts/release/compare-trees.py`): both images are mounted
  and every entry compared: type, permissions, owner, group, size, SHA-256,
  link target. Modification times are not compared (OSTree sets one value for
  all). The only differences allowed are under
  `/sysroot/ostree/repo/objects/`, OSTree's object store of the commit each
  image is made from (the base's commit in the tested image, the re-chunked
  image's own in the published one; 1607 objects in release-1, nothing else
  differed among about 474,000 entries). A changed, added or missing file
  anywhere else fails; `--self-test` proves it on small trees.
- **Fedora's marks in every layer** (`tests/image/marks.py --layers`): the
  merged image is checked as the `image` workflow checks it (text, file names,
  fedora-logos checksums, the initramfs, packages), and every layer is read as
  the tar stream it is: no layer may delete anything (a whiteout), no file may
  be in two layers (save `/sysroot/ostree/repo/config`, accepted only when
  every copy is the same), and no layer may hold a file of `fedora-logos` or a name
  the allow-list does not cover. Without whiteouts and duplicates the layers
  hold exactly the merged image's files, so the merged checks hold for every
  layer.
- **Smoke tests** (`tests/image/smoke.sh`) on the re-chunked image.

## 5. Signing

**What the computer can verify.** bootc fetches images through
containers/image (`skopeo experimental-image-proxy`), which applies
`containers-policy.json` when it opens an image, before any layer is fetched
(container-libs `common/pkg/json-proxy/handler.go`, `openImageImpl`). Its
`sigstoreSigned` requirement verifies sigstore signatures stored as
"attachments" (`use-sigstore-attachments` in `containers-registries.d(5)`):
the tag `sha256-<digest>.sig` holding a simple-signing payload, in the format
cosign writes with `--new-bundle-format=false`. It does not read cosign's
newer bundle format. Its trust anchors are a public key (`keyPath`), a Fulcio
CA (`fulcio`), or an X.509 CA (`pki`); with `rekorPublicKeyPath` it also
requires the signature's entry in a Rekor log, checked offline from the
signed entry timestamp.

**What it cannot verify yet.** The `fulcio` requirement matches the signer
only by e-mail address (`oidcIssuer` and `subjectEmail` are both mandatory;
container-libs `image/signature/fulcio_cert.go` matches
`untrustedCertificate.EmailAddresses` and has a FIXME for URIs and GitHub
workflow identities). A certificate GitHub Actions obtains from Fulcio names
the workflow as a URI (`https://github.com/giorgiogiudice/TUR/.github/workflows/release.yml@refs/heads/main`)
and carries no e-mail. Measured with Fedora 45's own skopeo 1.24.1 against a
keyless signature made in this repository's CI: `Required email "…" not found
(got [])`. Upstream proposal: container-libs pull request 625 (a
`buildSignerURI` match), open since February 2026. So **"the computer accepts
only images signed by TUR's release workflow's keyless identity" cannot be
expressed** in the policy Fedora 45's container libraries read.

**What TUR does** (`scripts/release/sign.sh`, cosign v3.1.4 pinned by
checksum):

1. **The release key, entered in Rekor.** A sigstore signature with TUR's
   release key (ECDSA P-256, made by `cosign generate-key-pair`), uploaded to
   Sigstore's public Rekor log at `https://rekor.sigstore.dev`. This is what
   the computer checks. The private key is a GitHub Actions secret
   (`TUR_RELEASE_SIGNING_KEY`, password `TUR_RELEASE_SIGNING_PASSWORD`); only
   the release job reads it, only when publishing. Because the policy also
   requires the Rekor entry, every signature a TUR computer accepts is in the
   public log: a stolen key cannot sign an update without that showing.
2. **Keyless, with the workflow's identity.** A short-lived Fulcio certificate
   for `https://github.com/giorgiogiudice/TUR/.github/workflows/release.yml@refs/heads/main`
   (issuer `https://token.actions.githubusercontent.com`), entered in Rekor.
   Anyone can check it with cosign (below). It is what ADR-0023 asked for, and
   what the computer will check once its container libraries can match a
   workflow identity; then the release key can be retired.
3. **Attestations**, keyless: the build manifest
   (`--type https://github.com/giorgiogiudice/TUR/blob/main/schemas/build-manifest.schema.json`)
   and the SBOM (`spdxjson`, 6 to 7 MB; Rekor accepted it in release run
   43413472528). If the log ever refuses the SBOM, the run says so and goes
   on: the SBOM is a workflow artifact in any case.

Every signature names `ghcr.io/giorgiogiudice/tur` as the signed repository
(cosign `--sign-container-identity`), wherever the image is when it is signed,
because that is the name a computer pulls by. The signatures are made once, in
the job's own registry, verified there with cosign, tested on the guest, and
copied unchanged with the image.

cosign marks `--new-bundle-format=false`, `--use-signing-config=false`,
`--tlog-upload` and `--sign-container-identity` as deprecated; it is moving
to bundles stored as OCI referrers, which containers/image cannot read. The
workflow pins cosign; moving to a cosign release without these flags needs
containers/image to read the new format first (container-libs issue 388).

**Verifying by hand** (cosign 3.1):

```sh
cosign verify --key image/rootfs/usr/share/tur/signing/release.pub \
    ghcr.io/giorgiogiudice/tur:testing
cosign verify \
    --certificate-identity https://github.com/giorgiogiudice/TUR/.github/workflows/release.yml@refs/heads/main \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    ghcr.io/giorgiogiudice/tur:testing
```

## 6. Verification on the computer

Fedora 45's containers-common 0.69.2 ships the policy as a vendor file,
`/usr/share/containers/policy.json`, and no `/etc/containers/policy.json`; an
`/etc/containers/policy.json`, if an administrator creates one, replaces the
vendor file whole (`containers-policy.json(5)`, as shipped). The same holds
for `registries.d`: vendor files in `/usr/share/containers/registries.d/`,
administrator files in `/etc/containers/registries.d/` (measured in the base,
release-1). TUR is the vendor, so its files go to `/usr`, which every update
replaces whole; `/etc` is left to the person who owns the computer.

| In the image | From | Purpose |
|---|---|---|
| `/usr/share/containers/policy.json` | the base's file, plus one rule written at build time by [`image/signing/policy.py`](../../image/signing/policy.py) | The rule below; everything else as the base ships it |
| `/usr/share/containers/registries.d/tur.yaml` | `image/rootfs/` | `use-sigstore-attachments: true` for `ghcr.io/giorgiogiudice/tur` |
| `/usr/share/tur/signing/rekor.pub` | Sigstore's trust root | The key of the Rekor log at `rekor.sigstore.dev` |
| `/usr/share/tur/signing/fulcio.crt` | Sigstore's trust root | Fulcio's chain (intermediate, root), for the keyless signature; not used by the policy yet |
| `/usr/share/tur/signing/release.pub` | the owner (section 8) | TUR's release key. **Absent until publishing is switched on**: until then every image of the repository is refused |
| `/usr/share/tur/signing/README` | `image/rootfs/` | What these files are |

The rule, as written into the policy:

```json
"ghcr.io/giorgiogiudice/tur": [
    {
        "type": "sigstoreSigned",
        "keyPath": "/usr/share/tur/signing/release.pub",
        "rekorPublicKeyPath": "/usr/share/tur/signing/rekor.pub",
        "signedIdentity": {"type": "matchRepository"}
    }
]
```

`matchRepository` because cosign's payload names a repository, not a tag
(`containers-policy.json(5)`). The build fails if the base already has a rule
for the scope, if anything else in the policy would change, or if an
`/etc/containers/policy.json` exists; `tests/image/smoke.sh` checks the rule,
the files, and that the policy still parses and `oci:` images are still
accepted. The release workflow checks that `rekor.pub` and `fulcio.crt` are
byte for byte what Sigstore's TUF-signed trust root says
(`scripts/release/trust-roots.py --check`).

**What is not affected.** The rule's scope is the repository name. Every
other registry, `oci:` and `containers-storage:` (what `bootc install`, the
boot test and the rollback scenarios use) and the default
(`insecureAcceptAnything`) stay as the base ships them. The key files are read
when an image of the scope is checked, not when the policy is loaded, so a
missing `release.pub` refuses TUR's repository and nothing else.

**bootc.** A plain `bootc switch ghcr.io/giorgiogiudice/tur:testing` records
the image as `ostree-unverified-registry:` (`spec.image.signature: insecure`
in `bootc status`), but the image proxy applies the policy all the same: the
guest test refuses with plain `bootc switch`. `bootc switch
--enforce-container-sigpolicy` (`ostree-image-signed:`, `containerPolicy`)
additionally refuses any policy whose default accepts anything, which the
base's does; TUR does not change the default, so that flag is not usable
without breaking every other image source. The update manager should
therefore expect `signature: insecure` for the TUR repository and rely on the
policy rule; the row "Signatures" of update-manager.md, section 11, expects
`containerPolicy` and is for that work to update.

**Mirrors.** containers/image looks signatures up at the registry it actually
fetches from, so a mirror of the repository needs its own `registries.d` entry
with `use-sigstore-attachments`; the policy still applies to the name asked
for.

## 7. What CI proves

On every release run, publishing on or off:

- the tested image is the `image` workflow's, by image ID (section 3);
- the re-chunked image has the tested image's files; no layer carries Fedora's
  marks or a whiteout or a shadowed file; it passes the smoke tests (section 4);
- the trust roots in the image are Sigstore's;
- both signatures verify with cosign; the attestation of the build manifest
  verifies;
- **in a booted guest** (`tests/boot/signed-update.py`, guest side
  `tests/boot/guest/rootfs/usr/libexec/tur-boot-test/signed-update-agent`):
  the tested image, installed to a disk with the boot test's agent and
  prepared with the boot test's first two boots, is asked with plain `bootc
  switch` and the policy the image ships to follow
  `ghcr.io/giorgiogiudice/tur:<tag>`, which the job's registry serves (a
  `registries.conf.d` drop-in in the guest's `/etc` points the name there):
  - `unsigned` (no signature), `other-key` (signed with another key, entered
    in Rekor), `no-log` (signed with the release key, not entered in Rekor):
    each is refused when bootc's image proxy opens the image, before any
    layer is fetched, with the policy's reason for that case ("A signature
    was required, but no signature exists"; "No public key verified against
    the RekorSET"; "missing dev.sigstore.cosign/bundle annotation"),
    and nothing is staged: no staged deployment, no new deployment, no image
    reference and no object added to OSTree's repository;
  - the image under release: staged, by exactly the digest under release;
    the guest restarts into it, the new version (the product image, with no
    test agent) reports the digest it runs, and boot health marks the boot
    good;
- the publishing script, run against a second repository of the job's
  registry: copies by digest with the signature and attestation tags, keeps
  the build ID immutable (run twice), promotes to `stable`, and cosign
  verifies both signatures there.

The fixtures are the image under release with one label added
(`scripts/release/variant.py`): same layers, another digest each, because a
signature belongs to a digest.

**Rehearsing without waiting.** A full run waits for every workflow on the
commit. Two shortcuts run only the release job, against an image CI already
built and tested, and never publish: a manual run of `release` with the
input `image_run` (the ID of a successful run of the `image` workflow; the
boot and rollback runs of the same commit are used alongside), and, on a
pull request, a commit that changes only the release itself (the image
workflow does not run on it, and the branch's latest successful image run
is used). The workflow file must be on `main` for the manual run to be
offered.

**While publishing is off** the release key does not exist. The job makes a
throwaway key pair, signs with it, and the guest gets its public half on a
tmpfs over `/usr/share/tur/signing` (the image's own files plus that key):
the policy file and its rule are the image's own. **With publishing on**, the
job signs with the real key and the guest uses the image's own
`release.pub`, untouched.

## 8. Switching publishing on (the owner)

Nothing below can be done by CI or an agent; each step is the owner's.

1. **Make the release key**, on your own computer (not on a server), with
   cosign 3.1 (`https://github.com/sigstore/cosign/releases`, check the
   checksum):

   ```sh
   cosign generate-key-pair --output-key-prefix tur-release
   ```

   Choose a long password. This writes `tur-release.key` (private, encrypted
   with the password) and `tur-release.pub`.
2. **Store the private half as secrets** of the repository, on GitHub:
   Settings → Secrets and variables → Actions → New repository secret:
   - `TUR_RELEASE_SIGNING_KEY`: the whole content of `tur-release.key`;
   - `TUR_RELEASE_SIGNING_PASSWORD`: its password.

   Then keep `tur-release.key` and its password offline (a password manager
   and an encrypted backup), or delete the file: the secret is the copy CI
   uses. Never commit it.
3. **Put the public half in the image**: a pull request that adds
   `tur-release.pub` as `image/rootfs/usr/share/tur/signing/release.pub`.
   Once merged, images built from then on accept TUR's signed updates.
4. **Switch publishing on**: Settings → Secrets and variables → Actions →
   Variables → New repository variable `TUR_PUBLISH` = `1`. From the next
   release run on `main`, the image goes to `ghcr.io/giorgiogiudice/tur`.
   The job refuses to publish if the secret is missing, if `release.pub` is
   missing, or if the secret's public half is not `release.pub`.
5. **Make the package public** after the first publication: on GitHub,
   your profile → Packages → `tur` → Package settings → Danger Zone → Change
   visibility → Public; and, on the same page, "Manage Actions access": the
   repository `giorgiogiudice/TUR` with the Write role (the first push from
   the workflow normally links it already). Until the package is public,
   computers cannot pull it without a login.
6. **Point the test PC at `testing`**: `sudo bootc switch
   ghcr.io/giorgiogiudice/tur:testing` on a TUR installation built after
   step 3. An installation from before step 3 has no `release.pub` and
   refuses the switch, as designed; reinstall it from a newer image.

To switch publishing off again, delete the variable or set it to anything
but `1`; to stop a key, remove the secret and ship a new `release.pub`
(rotation: section 10).

## 9. Promoting a build to `stable`

After a build has run on the test PC: Actions → `release` → Run workflow →
branch `main`, field "A published build ID to promote to stable" = the build
ID (as shown by `bootc status` or the release run's summary). The `promote`
job checks the input is a build ID, that the build is in the repository, and
that its digest carries both signatures (release key; keyless identity of the
release workflow on `main`); then it points `stable` at that digest and reads
it back. Nothing is rebuilt or re-signed: signatures belong to the digest.

## 10. Limits and open points

- The computer checks the release key and the public log, not the workflow
  identity (section 5). The key is long-lived; ADR-0023's "no long-lived
  signing key" holds only once containers/image can match a workflow
  identity (container-libs pull request 625) and that version is in TUR's
  base. Alternatives the owner may prefer: a key in a cloud KMS reached with
  the workflow's OIDC token (no exportable key), or a Google service
  account's identity through workload identity federation (a Fulcio
  certificate with an e-mail, which the policy can match today).
- Key rotation: ship the new `release.pub` beside the old one with
  `keyPaths`, release with the new key, and drop the old one once every
  computer has updated. Not built yet.
- `testing` moves to the newest commit released; a release run cancelled by
  a newer push leaves `testing` where it was. There is no check that
  `testing` only moves forward in history.
- The rollback scenarios and the boot test boot their own builds of the
  commit, not the exported image, and those builds have the same image ID
  only on some runners (section 3; release-1, finding 1).
- A full run on `main` waits for the slowest workflow on the commit (the
  `iso` workflow, 76 minutes in release-1) and then takes about 36 minutes:
  about two hours from merge to publication with free runners.
- Release notes from the merged pull requests (ADR-0023, consequences) are
  not built yet.

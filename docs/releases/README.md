<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Releasing a preview of TUR's installer medium

- **Status:** built; tested in CI against stand-ins for object storage and for
  GitHub. **No preview has been released.** Releasing is switched off until the
  owner completes section 3 and section 4.
- **Workflow:** [`.github/workflows/release-installer.yml`](../../.github/workflows/release-installer.yml),
  scripts in [`scripts/release-installer/`](../../scripts/release-installer/).
- **Related:** [ADR-0018](../adr/0018-first-installer-medium.md) (the medium),
  [ADR-0023](../adr/0023-publishing-signed-updates.md) and the image publishing
  (signed images, the release key), [the installation
  guide](../guides/install-on-a-test-pc.md).
- **Files here:** [`TEMPLATE.md`](TEMPLATE.md) (the release notes, filled by the
  workflow) and [`known-issues.md`](known-issues.md) (what works and what does
  not; maintained by hand, copied into every release's notes).

## 1. What a release is

| Part | Where | What |
|---|---|---|
| The medium | S3-compatible object storage (Linode Object Storage), `releases/<version>/tur-<version>-x86_64.iso` | The bootable USB image, exactly as a green `iso` run on `main` built and tested it |
| Checksums, manifest, signatures | the same folder of the bucket, **and** attached to the GitHub release | `.SHA256SUMS` (medium and manifest), `.manifest.json` (everything on the medium by SHA-256), `*.keyless.sigstore.json` and, when the release key exists, `*.release-key.sigstore.json` and `.release-key.pub` |
| The GitHub pre-release | tag `v<version>` on the commit the medium was built from | Marked **pre-release**, never "latest". The notes from `TEMPLATE.md`: version, build ID, image ID, what works and what does not, requirements, how to verify, how to write the stick, the guide, the licence summary and the credit line |

The medium does not go on the GitHub release itself because GitHub does not
accept it: "Each file included in a release must be under 2 GiB" (GitHub Docs,
[About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases),
read 13 November 2026; the same page: up to 1000 assets per release, no limit
on the total size or bandwidth). The medium is about 6.7 GB today.

Versions are pre-releases in the semantic-versioning sense, `X.Y.Z-<label>`,
for example `0.1.0-preview.1`; `X.Y.Z` must be the `VERSION` the medium was
built with (`0.1.0` while `VERSION` says `0.1.0-dev`). A version is released
once: a tag or release that exists is never moved or replaced.

## 2. How a release runs

Started by hand: **Actions → release-installer → Run workflow**, branch `main`,
with three inputs.

| Input | Meaning |
|---|---|
| `version` | e.g. `0.1.0-preview.1`; the tag will be `v0.1.0-preview.1` |
| `source_run` | the id (the number in the run's address) of a run of the `iso` workflow on `main` that passed and was started with `upload_medium` |
| `publish` | off by default. Even when ticked, nothing is published unless every guard of section 2.1 holds |

| Job | Permissions | What it does |
|---|---|---|
| `lint` | `contents: read` | shellcheck, Python syntax, unit tests of the guard, the verification and the notes |
| `test` | `contents: read`, `id-token: write` | The dry-run test (section 5) |
| `prepare` | `contents: read`, `actions: read`, `id-token: write` | 1. **The source run** (`source.py`): a run of `.github/workflows/iso.yml` in this repository, on `main`, from a push or a manual start, completed with "success", whose commit is on `main`, with the medium artifact (`tur-installer-medium-DO-NOT-REDISTRIBUTE`) and its test record (`tur-installer-medium-test`) still there. A run without the medium stops with "start the iso workflow on main by hand with upload_medium ticked". 2. **Downloads** both artifacts. 3. **Verifies** (`verify.py`): the medium's SHA-256 is the one in its `SHA256SUMS`, which is byte for byte the one in the test record; the manifest names the file, its size and SHA-256; the manifest's revision is the run's commit and its build ID the clean build of it; the version matches. 4. **Guard**: publish or dry run (section 2.1). 5. **Signs** (`sign.sh`) the medium and the checksum file with cosign: keyless, with this workflow's identity, entered in Sigstore's public log; and with TUR's release key when the secret `TUR_RELEASE_SIGNING_KEY` exists (the image publishing's key; if the repository already carries its public half, the secret must match it). Every signature is verified back. 6. **Writes the notes** (`notes.py`) from `TEMPLATE.md` and `known-issues.md`, shown in the run's summary. 7. Uploads the notes, checksums, manifest and signatures as the artifact **`release-installer-review`** (30 days) |
| `publish` | `contents: write`, `actions: read` | Only if every guard holds (section 2.1). 1. Checks every guard again and stops if one fails. 2. Downloads the medium from the source run again and checks it against the signed checksum file (`sha256sum -c`, `cosign verify-blob`). 3. **Uploads** (`upload.py`) the medium, checksums, manifest and signatures with rclone, multipart (64 MiB parts), never replacing an object with different content. 4. **Reads every object back** before going on: the storage reports the right size, and the whole object, downloaded from its public address, has the right size, SHA-256 and Content-Type. 5. **Creates the pre-release** (`github_release.py`): a draft on the source commit, the small files attached, then published as pre-release with "latest" off; then reads it back (published, pre-release, not latest, these assets, these notes, the tag on the source commit). A failure before publishing deletes the draft |

### 2.1 The guards

`publish` runs only if **all** of these hold. The job's `if:` checks the
first four itself; `scripts/release-installer/guard.py` checks all of them in
`prepare` and again at the start of `publish`, comparing each with an exact
value (anything else counts as "no"). Otherwise the run is a **dry run**:
everything up to the signatures and the notes runs, and the result is the
review artifact.

| Guard | Holds when |
|---|---|
| Started by hand | the event is `workflow_dispatch` (pull requests run `lint` and `test` only) |
| From `main` | the run was started on `refs/heads/main` |
| `publish` ticked | the input is `true` (default `false`) |
| Releases enabled | the repository variable `TUR_RELEASES_ENABLED` is exactly `1` |
| Storage configured | the four secrets `TUR_DOWNLOADS_ENDPOINT`, `TUR_DOWNLOADS_BUCKET`, `TUR_DOWNLOADS_ACCESS_KEY`, `TUR_DOWNLOADS_SECRET_KEY` exist |
| Public address | the repository variable `TUR_DOWNLOADS_PUBLIC_URL` is an `https://` address |
| Redistributable | the medium's manifest does not say "Not for redistribution". **Today it does** (`installer/iso/medium-manifest.py`, issue #77), so no medium built today can be published |
| Version free | neither the tag `v<version>` nor a release of it exists |

The workflow passes only whether each secret is set (`secrets.X != ''`),
never its value, to the guard. The storage credentials are given only to the
upload step, the release key only to the signing step; neither is printed.

## 3. Before the first public release

Not done by the workflow, and each one blocks a release:

1. **Legal review**: the licence ([ADR-0003](../adr/0003-licence.md), its
   verification items); the branding and trademark questions of issue #77
   (the credit line, the remaining descriptive uses of "Fedora", brand terms,
   Mozilla's terms for Firefox); the privacy statement for problem reports
   ([problem-reports.md](../architecture/problem-reports.md), section 9).
2. **The owner's test on real hardware**: the medium written to a real USB
   stick, started and installed on the test PC, the installed system started;
   the results in [known-issues.md](known-issues.md) and the
   [guide](../guides/install-on-a-test-pc.md).
3. **The medium's own texts**: the manifest's `distribution` field and the
   `iso` workflow's `DO-NOT-REDISTRIBUTE.txt` and artifact name still say the
   medium is not for redistribution (issue #77). The guard refuses until the
   manifest no longer says so; that change belongs to the medium
   (`installer/iso/`), with the legal review's answer.
4. **The installation guide for the public**: it is written for the
   project's own test PC today ("do not pass the medium on", "get the medium
   from an iso run"); its section 2 should point to the release instead.
5. **`known-issues.md` read again** against the build being released.

## 4. The owner's one-time setup

Everything here is done by the owner, in Cloud Manager and on GitHub. Nothing
in it can be done by CI or an agent.

1. **Create the bucket.** Cloud Manager → Object Storage → Create Bucket.
   Label e.g. `tur-downloads`; a region in a core data center near most
   testers (not Jakarta or São Paulo: their transfer is pooled separately and
   costs more, section 4.1). Note the bucket's **hostname** shown in its
   details, e.g. `tur-downloads.us-ord-1.linodeobjects.com`, and the region's
   **endpoint**, e.g. `us-ord-1.linodeobjects.com`.
2. **Create an access key limited to that bucket.** Object Storage → Access
   Keys → Create Access Key; label e.g. `tur-release-installer`; only the
   bucket's region; switch on **Limited Access**; set the bucket to
   **Read/Write** and everything else to none; Create. Copy the access key
   and the secret key now: the secret is shown once. (Akamai: "Manage access
   keys". A limited key cannot change bucket policies or bucket ACLs, which is
   what we want.)
3. **Let anyone download objects, but not list the bucket.** Creating the
   limited key wrote a bucket policy; the public-read rule must be **added to
   it**, not replace it, or the key loses its access (Akamai: "Define access
   and permissions using bucket policies"). Bucket policies can only be
   changed with an unlimited key and an S3 tool, so, on your own computer:
   1. create a temporary **unlimited** access key for the region;
   2. with the AWS CLI (or `s3cmd info` / `s3cmd setpolicy`, the tool Akamai
      documents), read the current policy:
      `aws s3api get-bucket-policy --bucket tur-downloads --endpoint-url https://us-ord-1.linodeobjects.com --query Policy --output text > policy.json`;
   3. add this statement to the `Statement` list in `policy.json`, keeping
      the existing statements:

      ```json
      {
        "Sid": "PublicReadObjects",
        "Effect": "Allow",
        "Principal": "*",
        "Action": ["s3:GetObject"],
        "Resource": ["arn:aws:s3:::tur-downloads/*"]
      }
      ```

      (objects only: no `s3:ListBucket`, and not the bare bucket ARN, which
      Akamai warns can lock Cloud Manager out of the bucket);
   4. `aws s3api put-bucket-policy --bucket tur-downloads --endpoint-url https://us-ord-1.linodeobjects.com --policy file://policy.json`;
   5. **delete the temporary unlimited key.**

   The first release proves it: its read-back downloads every object from
   its public address and stops if that fails.
4. **CORS and content types.** Nothing to set. The files are downloaded
   through plain links, which CORS does not affect; the workflow sets each
   object's Content-Type (`application/x-iso9660-image` for the medium,
   `text/plain` for checksums and keys, `application/json` for the manifest
   and signatures) and the read-back checks it. A CORS rule (GET from the
   project's site) would be needed only if a web page later fetches the
   files with JavaScript.
5. **Add the four secrets** on GitHub: the repository → Settings → Secrets
   and variables → Actions → Secrets → New repository secret:
   - `TUR_DOWNLOADS_ENDPOINT`: `https://us-ord-1.linodeobjects.com` (your
     region's endpoint, with `https://`);
   - `TUR_DOWNLOADS_BUCKET`: `tur-downloads`;
   - `TUR_DOWNLOADS_ACCESS_KEY`: the limited key's access key;
   - `TUR_DOWNLOADS_SECRET_KEY`: its secret key.
6. **Add the variable** `TUR_DOWNLOADS_PUBLIC_URL` (same page → Variables →
   New repository variable): `https://tur-downloads.us-ord-1.linodeobjects.com`
   (the bucket's hostname with `https://`, no trailing `/`). Releases go
   under `releases/<version>/` in it.
7. **The release key (optional).** If the image publishing's release key
   exists (secrets `TUR_RELEASE_SIGNING_KEY` and
   `TUR_RELEASE_SIGNING_PASSWORD`: `docs/architecture/publishing.md`,
   section 8, in pull request #78 until it merges),
   the medium is signed with it too; nothing more to do. Without it the
   medium is signed keyless only.
8. **Rehearse**: run `iso` on `main` with `upload_medium`, then
   `release-installer` with that run's id and `publish` **off**. Read the
   notes in the run's summary and the `release-installer-review` artifact.
9. **Only when section 3 is complete**, enable a release: add the variable
   `TUR_RELEASES_ENABLED` = `1`, run `release-installer` with `publish`
   ticked, and **delete the variable again** when the release is done, so
   that the next run is a dry run whatever is ticked.

The medium artifact of an `iso` run is kept three days: release within three
days of that run, or start `iso` again.

To withdraw a release: delete the GitHub release and its tag on GitHub, and
the folder `releases/<version>/` in the bucket (Cloud Manager, or `rclone
purge` with a key that may delete). A withdrawn version is not reused.

### 4.1 What it costs

From Akamai's pricing pages, read 13 November 2026
([Object Storage pricing](https://www.akamai.com/cloud/pricing/object-storage);
[Object Storage pricing, TechDocs](https://techdocs.akamai.com/cloud-computing/docs/object-storage-pricing);
[Network transfer usage and costs](https://techdocs.akamai.com/cloud-computing/docs/network-transfer-usage-and-costs);
[Object Storage limits](https://techdocs.akamai.com/cloud-computing/docs/object-storage-product-limits)):

| | |
|---|---|
| Base | $5 a month, prorated, "and includes 250 GB of storage"; charged until Object Storage is cancelled |
| Transfer | "1 TB of transfer allowance is added to your account's monthly global network transfer pool"; inbound transfer is free; outbound overage $0.005/GB ($0.015 in Jakarta, $0.007 in São Paulo, whose pools are separate) |
| Storage over 250 GB | $0.02 per GB |
| Requests | "no charges for API requests"; per-request charges for E3 endpoints are being evaluated and "will not appear on monthly invoices any earlier than October 1, 2027" |
| Limits that matter | objects up to 5 TiB; multipart parts of 5 MiB to 5 GiB, at most 10,000 parts (the workflow uses 64 MiB parts: about 100 for today's medium) |

**Expected:** **$5 a month.** A release is about 7 GB stored, so even ten
kept releases stay inside the 250 GB. Each download of today's 6.7 GB medium
uses 6.7 GB of transfer: the 1 TB that Object Storage adds covers about 150
downloads a month (more if the account's other services leave allowance in
the pool), and each download beyond costs about $0.034 (6.7 GB × $0.005), so
100 extra downloads cost about $3.40. Each release also downloads every file
once for the read-back (about 7 GB).

## 5. What CI proves, and what it does not

On every pull request that changes the workflow, these scripts or these
documents, and before every manual run, the `test` job runs
`scripts/release-installer/test/run.sh`: every script of the release, in the
workflow's order, against

- **moto 5.2.5** in server mode (Apache-2.0, a widely used stand-in for S3;
  `docker.io/motoserver/moto:5.2.5` pinned by digest) in place of Linode
  Object Storage. MinIO was not used: its community edition stopped
  publishing images in October 2025 and its repository was archived in 2026;
- **a stand-in for GitHub's REST API** (`test/fake_github.py`) for the source
  run, artifacts, tags and releases, behaving as GitHub documents (a draft
  makes no tag; publishing makes it on `target_commitish`; a pre-release is
  never "latest");
- **a 24 MiB fake medium** with the files and manifest the `iso` workflow
  writes, so the upload is split into five parts.

It shows that: each kind of wrong source run is refused (another branch,
failed, no medium, expired, commit not on `main`, another workflow, a fork,
an id that is not a number); the medium is verified and laid out; it is
signed with a throwaway key (and keyless on pull requests) and the
signatures verify, while a changed file does not; the notes render with the
download address and the verification commands; the upload is multipart
(ETag `-5`) and every object is read back through its public address; the
same upload again is accepted and different content under the same name is
refused; the read-back detects an object of the right size with the wrong
content, and a wrong Content-Type; the pre-release is created on the source
commit, pre-release, not latest, with exactly the small files and the notes;
a second release of the tag is refused; a release that fails half-way leaves
no draft and no tag. The unit tests check each guard alone stops
publishing, and the verification's refusals (corrupt medium, differing
record, wrong commit, `.dirty` build, version format and mismatch).

**Not shown**, because they need the real services: an upload to Linode
Object Storage (its multipart and checksum behaviour with rclone, its bucket
policy), a real GitHub release and tag (and whether a tag ruleset, if one is
added, lets the workflow's token create `v*` tags), and the keyless
signature with the release workflow's identity on `main` (tested with the
pull request's identity). The first real dry run on `main` shows the source
run, download, verification and signing on a real medium.

## 6. Tools

| Tool | Version | Licence | Pinned |
|---|---|---|---|
| rclone | v1.75.2 (9 October 2026; v1.76.0 of 6 November 2026 exists and was not taken a week after release) | MIT | SHA-256 of `rclone-v1.75.2-linux-amd64.zip`, as published in the release's `SHA256SUMS` on GitHub and on downloads.rclone.org |
| cosign | v3.1.4 | Apache-2.0 | SHA-256 of `cosign-linux-amd64` (the same pin as the image publishing) |
| moto (test only) | 5.2.5 | Apache-2.0 | image digest |

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Identify the image CI built and tested, from the artifacts of its runs.

    scripts/release/tested.py IMAGE_DIR OUT_JSON [--boot DIR] [--rollback DIR]

IMAGE_DIR is the `tur-image-oci` artifact of the `image` workflow: the OCI
archive of the image every step of that workflow tested, its image ID, build
ID and source revision. The script reads the archive's index and manifest
(not its layers) and fails unless the digest of the configuration it holds is
the image ID the workflow recorded. The image ID names the configuration,
which names every layer by the digest of its uncompressed content: an archive
that passes is that image, byte for byte, however its layers are compressed.

--boot and --rollback are where the `boot` and `rollback` workflows' artifacts
of the same commit were downloaded. Each of those workflows builds the image
again from the same commit and records the ID of what it booted
(host/disk-info.env, tur_image_id). They are compared with the ID above and
the result is recorded. A difference is reported, not fatal: the builds are
reproducible on one runner, not yet across runners (docs/reports/release-1.md),
and the published image is booted by the release workflow itself.

Writes OUT_JSON and prints a summary.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tarfile


def env(path: pathlib.Path) -> dict[str, str]:
    values = {}
    for line in path.read_text().splitlines():
        key, sep, value = line.partition("=")
        if sep:
            values[key.strip()] = value.strip()
    return values


def archive_config(path: pathlib.Path) -> tuple[str, str, dict]:
    """The manifest digest, the config digest and the config of an OCI archive."""
    with tarfile.open(path, "r:") as archive:
        def read(name: str) -> bytes:
            member = archive.getmember(name)
            return archive.extractfile(member).read()

        index = json.loads(read("index.json"))
        if len(index["manifests"]) != 1:
            sys.exit(f"{path}: expected one image in the archive, found {len(index['manifests'])}")
        manifest_digest = index["manifests"][0]["digest"]
        manifest_bytes = read("blobs/" + manifest_digest.replace(":", "/"))
        if "sha256:" + hashlib.sha256(manifest_bytes).hexdigest() != manifest_digest:
            sys.exit(f"{path}: the manifest does not match its digest")
        manifest = json.loads(manifest_bytes)
        config_digest = manifest["config"]["digest"]
        config_bytes = read("blobs/" + config_digest.replace(":", "/"))
        if "sha256:" + hashlib.sha256(config_bytes).hexdigest() != config_digest:
            sys.exit(f"{path}: the configuration does not match its digest")
        return manifest_digest, config_digest, json.loads(config_bytes)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("image_dir")
    ap.add_argument("out")
    ap.add_argument("--boot")
    ap.add_argument("--rollback")
    args = ap.parse_args()
    image_dir = pathlib.Path(args.image_dir)
    image_id = (image_dir / "image-id").read_text().strip()
    build_id = (image_dir / "build-id").read_text().strip()
    revision = (image_dir / "source-revision").read_text().strip()
    manifest_digest, config_digest, config = archive_config(image_dir / "tur-image.oci.tar")
    labels = (config.get("config") or {}).get("Labels") or {}
    problems = []
    if config_digest != "sha256:" + image_id:
        problems.append(f"the archive holds image {config_digest}, the image workflow tested sha256:{image_id}")
    if labels.get("org.tur.build-id") != build_id:
        problems.append(f"the archive's build ID label is {labels.get('org.tur.build-id')!r}, expected {build_id!r}")
    if labels.get("org.opencontainers.image.revision") != revision:
        problems.append(f"the archive's revision label is {labels.get('org.opencontainers.image.revision')!r}, "
                        f"expected {revision!r}")

    siblings = []
    for kind, root in (("boot", args.boot), ("rollback", args.rollback)):
        if not root:
            continue
        for found in sorted(pathlib.Path(root).rglob("disk-info.env")):
            if found.parent.name != "host":
                continue
            values = env(found)
            siblings.append({"workflow": kind, "file": str(found.relative_to(root)),
                             "image_id": values.get("tur_image_id", ""),
                             "same_as_released": values.get("tur_image_id") == image_id})
    record = {
        "image_id": "sha256:" + image_id,
        "archive_manifest": manifest_digest,
        "build_id": build_id,
        "source_revision": revision,
        "labels": labels,
        "booted_by_other_workflows": siblings,
        "problems": problems,
    }
    pathlib.Path(args.out).write_text(json.dumps(record, indent=2) + "\n")
    print(f"image tested by the image workflow: sha256:{image_id} (build {build_id}, revision {revision})")
    print(f"archive: manifest {manifest_digest}, configuration {config_digest}")
    for sibling in siblings:
        verdict = "the same image" if sibling["same_as_released"] else "ANOTHER BUILD of the same commit"
        print(f"{sibling['workflow']:9} {sibling['file']}: {sibling['image_id']}: {verdict}")
    for problem in problems:
        print(f"PROBLEM: {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

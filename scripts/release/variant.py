#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Add test variants of an image to its OCI layout: same layers, new digest.

    scripts/release/variant.py LAYOUT SOURCE_REF NAME [NAME ...]

For each NAME, the image SOURCE_REF is copied under the reference NAME with
one label added (org.tur.release-test=NAME). The layers are the same blobs;
only the configuration, and so the manifest digest, differ. The release
workflow pushes the variants to the same repository as the image under
test and signs them wrongly on purpose (unsigned, signed with another key,
signed without a transparency-log entry): a signature belongs to a digest,
so each refusal case needs a digest of its own (publishing.md, section 7).
Prints NAME=sha256:… for each. Test fixtures only; never published.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys


def main() -> int:
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    root, source, names = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3:]
    blobs = root / "blobs" / "sha256"

    def read(digest: str) -> bytes:
        return (blobs / digest.split(":", 1)[1]).read_bytes()

    def write(data: bytes) -> dict:
        digest = hashlib.sha256(data).hexdigest()
        (blobs / digest).write_bytes(data)
        return {"digest": "sha256:" + digest, "size": len(data)}

    index_path = root / "index.json"
    index = json.loads(index_path.read_text())
    ref = "org.opencontainers.image.ref.name"
    found = [m for m in index["manifests"] if m.get("annotations", {}).get(ref) == source]
    if len(found) != 1:
        sys.exit(f"{root}: no single image named {source}")
    manifest = json.loads(read(found[0]["digest"]))
    config = json.loads(read(manifest["config"]["digest"]))
    for name in names:
        index["manifests"] = [m for m in index["manifests"] if m.get("annotations", {}).get(ref) != name]
        variant = json.loads(json.dumps(config))
        variant.setdefault("config", {}).setdefault("Labels", {})["org.tur.release-test"] = name
        new_config = dict(manifest["config"], **write(json.dumps(variant, separators=(",", ":")).encode()))
        new_manifest = dict(manifest, config=new_config)
        descriptor = write(json.dumps(new_manifest, separators=(",", ":")).encode())
        index["manifests"].append({"mediaType": found[0].get("mediaType", manifest.get("mediaType")),
                                   **descriptor, "annotations": {ref: name}})
        print(f"{name}={descriptor['digest']}")
    index_path.write_text(json.dumps(index, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Verify a downloaded installer medium against what its `iso` run recorded,
and lay it out under the release's file names.

    verify.py --medium-dir DIR --record-dir DIR --revision SHA --version VERSION --out DIR

--medium-dir   the medium artifact: <name>.iso, <name>.SHA256SUMS, <name>.manifest.json
--record-dir   the run's test record (uploaded on every run), which holds its
               own copy of <name>.SHA256SUMS and <name>.manifest.json
--revision     the commit the `iso` run built
--version      the version to release, e.g. 0.1.0-preview.1

Every one of these must hold, or nothing is written:

- the SHA256SUMS file names exactly one .iso and one .manifest.json, both
  present, and their SHA-256 match;
- the manifest describes this file: name, size and SHA-256;
- the test record's SHA256SUMS and manifest are byte for byte the medium's;
- the manifest's revision is the run's commit, and its build ID is the
  clean build of that commit (<date>.g<12 hex>, no ".dirty");
- the version is a pre-release (X.Y.Z-something) whose X.Y.Z is the
  manifest's TUR version.

Then the medium is moved (not copied) to OUT/tur-<version>-x86_64.iso, the
manifest copied beside it, and a new tur-<version>-x86_64.SHA256SUMS written
for both. OUT/release.json and OUT/release.env hold the facts the notes and
the later steps use. Whether the manifest still says the medium is not for
redistribution is recorded (`redistributable`); the guard refuses to publish
such a medium.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import sys

VERSION_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-([0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)$")
BUILD_ID_RE = re.compile(r"^[0-9]{8}\.g([0-9a-f]{12})$")


class Refused(Exception):
    pass


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(8 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def parse_sums(path: pathlib.Path) -> dict[str, str]:
    sums = {}
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"([0-9a-f]{64}) [ *](\S+)", line)
        if not match:
            raise Refused(f"{path.name}: not a sha256sum line: {line!r}")
        name = match.group(2)
        if "/" in name or name in sums:
            raise Refused(f"{path.name}: unexpected entry {name!r}")
        sums[name] = match.group(1)
    return sums


def one(paths: list[pathlib.Path], what: str) -> pathlib.Path:
    if len(paths) != 1:
        raise Refused(f"expected exactly one {what}, found {len(paths)}: {[p.name for p in paths]}")
    return paths[0]


def check_version(version: str) -> re.Match:
    match = VERSION_RE.match(version)
    if not match:
        raise Refused(f"version {version!r} is not a pre-release version like 0.1.0-preview.1")
    return match


def verify(medium_dir: pathlib.Path, record_dir: pathlib.Path, revision: str, version: str) -> dict:
    match = check_version(version)
    sums_path = one(sorted(medium_dir.glob("*.SHA256SUMS")), "SHA256SUMS file in the medium artifact")
    stem = sums_path.name[: -len(".SHA256SUMS")]
    sums = parse_sums(sums_path)
    iso_name = one([n for n in sums if n.endswith(".iso")], ".iso entry in " + sums_path.name)
    manifest_name = one([n for n in sums if n.endswith(".manifest.json")], ".manifest.json entry in " + sums_path.name)
    if len(sums) != 2:
        raise Refused(f"{sums_path.name} lists {len(sums)} files, expected the medium and its manifest")
    if iso_name != f"{stem}.iso" or manifest_name != f"{stem}.manifest.json":
        raise Refused(f"{sums_path.name} names {iso_name} and {manifest_name}, expected {stem}.iso and {stem}.manifest.json")
    iso = medium_dir / iso_name
    manifest_path = medium_dir / manifest_name
    for path in (iso, manifest_path):
        if not path.is_file():
            raise Refused(f"{path.name} is listed in {sums_path.name} but not in the artifact")

    iso_sha = sha256(iso)
    iso_bytes = iso.stat().st_size
    if iso_sha != sums[iso_name]:
        raise Refused(f"{iso_name}: SHA-256 {iso_sha} is not {sums[iso_name]} ({sums_path.name})")
    manifest_sha = sha256(manifest_path)
    if manifest_sha != sums[manifest_name]:
        raise Refused(f"{manifest_name}: SHA-256 {manifest_sha} is not {sums[manifest_name]} ({sums_path.name})")

    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema") != "tur-installer-medium-manifest":
        raise Refused(f"{manifest_name} is not a medium manifest (schema {manifest.get('schema')!r})")
    medium = manifest.get("medium") or {}
    if medium.get("file") != iso_name or medium.get("bytes") != iso_bytes or medium.get("sha256") != iso_sha:
        raise Refused(
            f"the manifest describes {medium.get('file')} ({medium.get('bytes')} bytes, {medium.get('sha256')}), "
            f"not {iso_name} ({iso_bytes} bytes, {iso_sha})")

    record_sums = one(sorted(record_dir.rglob(sums_path.name)), f"{sums_path.name} in the test record")
    record_manifest = one(sorted(record_dir.rglob(manifest_name)), f"{manifest_name} in the test record")
    if record_sums.read_bytes() != sums_path.read_bytes():
        raise Refused(f"the test record's {sums_path.name} differs from the medium artifact's")
    if record_manifest.read_bytes() != manifest_path.read_bytes():
        raise Refused(f"the test record's {manifest_name} differs from the medium artifact's")

    source = manifest.get("source") or {}
    if source.get("revision") != revision:
        raise Refused(f"the medium was built from {source.get('revision')}, not from the run's commit {revision}")
    build_id = source.get("build_id") or ""
    build = BUILD_ID_RE.match(build_id)
    if not build or not revision.startswith(build.group(1)):
        raise Refused(f"build ID {build_id!r} is not a clean build of {revision}")
    tur_version = source.get("tur_version") or ""
    base = ".".join(match.group(1, 2, 3))
    if tur_version.split("-")[0] != base:
        raise Refused(f"version {version} does not match the medium's TUR version {tur_version!r} (expected {base}-…)")

    payload = manifest.get("payload") or {}
    layout = payload.get("oci_layout") or {}
    environment = manifest.get("installer_environment") or {}
    distribution = manifest.get("distribution") or ""
    return {
        "version": version,
        "tag": f"v{version}",
        "source_name": stem,
        "source_iso": iso_name,
        "iso_bytes": iso_bytes,
        "iso_sha256": iso_sha,
        "manifest_sha256": manifest_sha,
        "revision": revision,
        "build_id": build_id,
        "tur_version": tur_version,
        "image_id": payload.get("image_id"),
        "image_config_digest": layout.get("config_digest"),
        "payload_index_sha256": layout.get("index_sha256"),
        "base_image": payload.get("base_image"),
        "base_digest": payload.get("base_digest"),
        "base_version": payload.get("base_version"),
        "kernel": environment.get("kernel"),
        "smallest_disk_bytes": payload.get("smallest_disk_accepted_bytes"),
        "distribution": distribution,
        "redistributable": "no" if distribution.strip().lower().startswith("not for redistribution") else "yes",
    }


def lay_out(facts: dict, medium_dir: pathlib.Path, out: pathlib.Path) -> dict:
    base = f"tur-{facts['version']}-x86_64"
    out.mkdir(parents=True, exist_ok=True)
    iso = out / f"{base}.iso"
    manifest = out / f"{base}.manifest.json"
    sums = out / f"{base}.SHA256SUMS"
    shutil.move(str(medium_dir / facts["source_iso"]), iso)
    shutil.copyfile(medium_dir / f"{facts['source_name']}.manifest.json", manifest)
    sums.write_text(f"{facts['iso_sha256']}  {iso.name}\n{facts['manifest_sha256']}  {manifest.name}\n")
    facts = {**facts, "base": base, "iso": iso.name, "manifest": manifest.name, "sums": sums.name,
             "sums_sha256": sha256(sums)}
    (out / "release.json").write_text(json.dumps(facts, indent=2) + "\n")
    (out / "release.env").write_text(
        "".join(f"{k}={v}\n" for k, v in facts.items() if k != "distribution" and v is not None))
    return facts


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--medium-dir", type=pathlib.Path, required=True)
    ap.add_argument("--record-dir", type=pathlib.Path, required=True)
    ap.add_argument("--revision", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--out", type=pathlib.Path, required=True)
    args = ap.parse_args()
    try:
        facts = verify(args.medium_dir, args.record_dir, args.revision, args.version)
    except (Refused, OSError, ValueError) as error:
        print(f"::error::medium not verified: {error}")
        return 1
    facts = lay_out(facts, args.medium_dir, args.out)
    for key in ("iso", "iso_bytes", "iso_sha256", "build_id", "revision", "image_id", "redistributable"):
        print(f"{key}: {facts[key]}")
    if facts["redistributable"] != "yes":
        print(f"::warning::the medium's manifest says: {facts['distribution']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Check that every test fixture is what the provenance file says it is.

    check_fixtures.py PROVENANCE            check every fixture
    check_fixtures.py PROVENANCE --write    write the ones from bootc's source

Three checks. Every JSON file under the fixtures directory is listed in the
provenance file, and every listed file is there. A fixture of kind
`ci-capture` has the recorded SHA-256: it is, byte for byte, the file in the
artifact of the CI run it names. A fixture of kind `upstream-fixture` is
compared with its source, as follows.

bootc keeps samples of `bootc status` output beside its code, as YAML
(`crates/lib/src/fixtures/spec-*.yaml`). `tur-update` reads JSON only, so the
samples are kept here as JSON. This script is the conversion, and the proof
that the JSON files are those samples and nothing else.

For each entry of kind `upstream-fixture` in the provenance file it fetches the
named file at the named tag, checks that the bytes have the recorded git blob
identifier, converts them, and compares the result byte for byte with the
committed fixture.

The conversion keeps what bootc would read:

* a YAML timestamp stays the text it was written as (bootc parses it later);
* a scalar that looks like a fraction stays text too: no member of the status
  is a fraction, and bootc reads `version: 20231230.1` as the text it is;
* an enum written as a YAML tag (`signature: !ostreeRemote "fedora"`) becomes
  the JSON object serde writes for it (`{"ostreeRemote": "fedora"}`);
* the order of the members is kept.

It runs in CI (the `update` workflow). It reads a public repository and sends
nothing but the request.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys
import urllib.request

import yaml


class Loader(yaml.SafeLoader):
    """SafeLoader without timestamps and fractions, with serde's enum tags."""


# Timestamps and fractions stay strings: remove the implicit resolvers for them.
TEXT_TAGS = ("tag:yaml.org,2002:timestamp", "tag:yaml.org,2002:float")
Loader.yaml_implicit_resolvers = {
    first: [(tag, pattern) for tag, pattern in resolvers if tag not in TEXT_TAGS]
    for first, resolvers in yaml.SafeLoader.yaml_implicit_resolvers.items()
}


def _enum_variant(loader: Loader, suffix: str, node: yaml.Node):
    if isinstance(node, yaml.ScalarNode):
        return {suffix: loader.construct_scalar(node)}
    if isinstance(node, yaml.MappingNode):
        return {suffix: loader.construct_mapping(node, deep=True)}
    return {suffix: loader.construct_sequence(node, deep=True)}


Loader.add_multi_constructor("!", _enum_variant)


def convert(text: str) -> str:
    """The JSON text of one YAML (or JSON) sample."""
    document = yaml.load(text, Loader=Loader)  # noqa: S506 - a SafeLoader subclass
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def blob_sha(data: bytes) -> str:
    """The identifier git gives a file with this content."""
    return hashlib.sha1(b"blob %d\x00" % len(data) + data).hexdigest()  # noqa: S324


def fetch(repository: str, ref: str, path: str) -> bytes:
    url = f"https://raw.githubusercontent.com/{repository}/{ref}/{path}"
    request = urllib.request.Request(url, headers={"User-Agent": "fixture-check"})
    with urllib.request.urlopen(request, timeout=60) as response:  # noqa: S310
        return response.read()


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    provenance_path = pathlib.Path(sys.argv[1])
    write = "--write" in sys.argv[2:]
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    directory = provenance_path.parent
    failures = 0
    checked = 0
    # Every fixture is listed, and every listed fixture is there.
    listed = {entry["file"] for entry in provenance["fixtures"]}
    present = {
        path.relative_to(directory).as_posix()
        for path in directory.rglob("*.json")
        if path != provenance_path
    }
    for name in sorted(listed ^ present):
        where = "is not listed in the provenance file" if name in present else "is listed but missing"
        print(f"FAIL {name} {where}")
        failures += 1
    for entry in provenance["fixtures"]:
        if entry["kind"] == "ci-capture" and (directory / entry["file"]).is_file():
            # The file is the bytes of the run's artifact.
            found = hashlib.sha256((directory / entry["file"]).read_bytes()).hexdigest()
            if found != entry["sha256"]:
                print(f"FAIL {entry['file']}: sha256 is {found}, recorded {entry['sha256']}")
                failures += 1
            else:
                source = entry["source"]
                print(f"ok   {entry['file']} is {source['path']} of run {source['run_id']} ({found[:12]})")
        if entry["kind"] != "upstream-fixture":
            continue
        source = entry["source"]
        data = fetch(source["repository"], source["tag"], source["path"])
        found = blob_sha(data)
        if found != source["blob_sha"]:
            print(f"FAIL {entry['file']}: upstream blob is {found}, recorded {source['blob_sha']}")
            failures += 1
            continue
        converted = convert(data.decode("utf-8"))
        target = directory / entry["file"]
        if write:
            target.write_text(converted, encoding="utf-8")
            print(f"wrote {entry['file']} from {source['path']} at {source['tag']}")
        elif not target.is_file() or target.read_text(encoding="utf-8") != converted:
            print(f"FAIL {entry['file']}: differs from {source['path']} at {source['tag']}")
            failures += 1
        else:
            print(f"ok   {entry['file']} is {source['path']} at {source['tag']} ({found})")
        checked += 1
    print(f"{checked} upstream fixtures, {failures} failures")
    if checked == 0:
        print("FAIL: the provenance file names no upstream fixture")
        return 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

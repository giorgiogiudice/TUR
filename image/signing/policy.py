#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Add TUR's rule for its own registry path to the image's container policy.

    python3 -I -B image/signing/policy.py /usr/share/containers/policy.json
    python3 -I -B image/signing/policy.py --print

Run by image/Containerfile, inside the image, with the image's own Python
(ADR-0023; docs/architecture/publishing.md, section 5). `--print` prints the
rule alone, for the tests that compare it with what a booted system holds.

Fedora 45's containers-common ships the vendor policy as
/usr/share/containers/policy.json and no /etc/containers/policy.json; an
administrator's /etc/containers/policy.json, if there ever is one, replaces
the vendor file whole (containers-policy.json(5)). TUR is the vendor here, so
its rule goes into the vendor file, in /usr, where every update brings it
again; /etc is left alone.

The rule covers one scope, the repository ghcr.io/giorgiogiudice/tur, under
the `docker` transport (what bootc's `registry` transport uses). An image
named by that repository is accepted only with a sigstore signature that

- verifies with TUR's release key (/usr/share/tur/signing/release.pub),
- carries a signed entry timestamp of Sigstore's public Rekor log, checked
  offline against the log's key (/usr/share/tur/signing/rekor.pub), so every
  signature the computer accepts is in the public transparency log, and
- names ghcr.io/giorgiogiudice/tur as the signed repository.

Everything else in Fedora's policy is kept as it is, the default included:
other registries, other transports (`oci:` and `containers-storage:`, with
which the tests and `bootc install` work) and Red Hat's rules. The script
fails if the rule is already there, if anything else would change, or if an
/etc/containers/policy.json exists that would hide the vendor file.

The keys are read when an image of the scope is checked, not when the policy
is loaded: until the owner adds release.pub (publishing.md, section 8), every
image from the scope is refused and nothing else is affected.
"""

from __future__ import annotations

import copy
import json
import os
import sys

SCOPE = "ghcr.io/giorgiogiudice/tur"
RULE = [
    {
        "type": "sigstoreSigned",
        "keyPath": "/usr/share/tur/signing/release.pub",
        "rekorPublicKeyPath": "/usr/share/tur/signing/rekor.pub",
        "signedIdentity": {"type": "matchRepository"},
    }
]
SHADOW = "/etc/containers/policy.json"


def main() -> int:
    if sys.argv[1:] == ["--print"]:
        print(json.dumps({SCOPE: RULE}, indent=4))
        return 0
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    if os.path.lexists(SHADOW):
        sys.exit(f"{SHADOW} exists and would hide {path}: TUR's rule would not apply")
    with open(path, encoding="utf-8") as stream:
        before = json.load(stream)
    docker = before.get("transports", {}).get("docker", {})
    if SCOPE in docker:
        sys.exit(f"{path} already has a rule for {SCOPE}: {docker[SCOPE]}")
    after = copy.deepcopy(before)
    after.setdefault("transports", {}).setdefault("docker", {})[SCOPE] = RULE
    text = json.dumps(after, indent=4) + "\n"
    with open(path, "w", encoding="utf-8") as stream:
        stream.write(text)

    # What was written, read back: the rule and nothing else changed.
    with open(path, encoding="utf-8") as stream:
        check = json.load(stream)
    added = check["transports"]["docker"].pop(SCOPE)
    if added != RULE or check != before:
        sys.exit(f"{path}: reading back gave something other than Fedora's policy plus TUR's rule")
    print(f"{path}: rule added for {SCOPE}; default {before.get('default')}; "
          f"other docker scopes kept: {sorted(docker)}; transports kept: {sorted(before.get('transports', {}))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

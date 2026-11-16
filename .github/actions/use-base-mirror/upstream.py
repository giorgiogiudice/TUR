#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Ask a public registry directly whether it has a manifest.

    upstream.py REGISTRY/REPOSITORY@DIGEST

Part of .github/actions/use-base-mirror. Speaks the registry API itself
(anonymous bearer token, then HEAD on the manifest), so that no
containers-registries.conf(5) mirror can answer in the registry's place:
skopeo and podman read the drop-ins in registries.conf.d even when
CONTAINERS_REGISTRIES_CONF names another file, in the versions on the hosted
runners. Prints one line; exits 0 when the manifest is there, 1 when the
registry says it is not, 2 when the registry cannot be asked.
"""

import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

ACCEPT = ", ".join([
    "application/vnd.oci.image.index.v1+json",
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
])


def head(url: str, token: str | None):
    headers = {"Accept": ACCEPT, "User-Agent": "tur-ci-base-mirror"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return urllib.request.urlopen(urllib.request.Request(url, method="HEAD", headers=headers), timeout=20)


def probe(ref: str) -> tuple[int, str]:
    """(0, "present ...") when REF's manifest is there, (1, ...) when the
    registry says it is not, (2, ...) when the registry cannot be asked."""
    name, _, digest = ref.partition("@")
    registry, _, repo = name.partition("/")
    # Docker Hub's names say docker.io; its registry API is elsewhere.
    host = "registry-1.docker.io" if registry == "docker.io" else registry
    url = f"https://{host}/v2/{repo}/manifests/{digest}"
    token = None
    for _ in range(2):
        try:
            with head(url, token) as r:
                got = r.headers.get("Docker-Content-Digest", digest)
                return 0, f"present ({r.status}, {got})"
        except urllib.error.HTTPError as e:
            if e.code == 401 and token is None:
                challenge = e.headers.get("WWW-Authenticate", "")
                params = dict(re.findall(r'(\w+)="([^"]*)"', challenge))
                if "realm" not in params:
                    return 2, "not reachable: 401 without a bearer challenge"
                query = {k: v for k, v in params.items() if k in ("service", "scope")}
                query.setdefault("scope", f"repository:{repo}:pull")
                try:
                    with urllib.request.urlopen(params["realm"] + "?" + urllib.parse.urlencode(query), timeout=20) as t:
                        body = json.load(t)
                    token = body.get("token") or body.get("access_token")
                except (urllib.error.URLError, OSError, ValueError) as err:
                    return 2, f"not reachable: token: {err}"
                continue
            if e.code == 404:
                return 1, "absent (manifest unknown)"
            return 2, f"not reachable: HTTP {e.code}"
        except (urllib.error.URLError, OSError) as err:
            return 2, f"not reachable: {getattr(err, 'reason', err)}"
    return 2, "not reachable: refused after authenticating"


def main() -> int:
    code, text = probe(sys.argv[1])
    print(text)
    return code


if __name__ == "__main__":
    sys.exit(main())

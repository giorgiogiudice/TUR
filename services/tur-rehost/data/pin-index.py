#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Writes services/tur-rehost/data/index.json: what tur-rehost may fetch,
from where, and the SHA-256 each file must have.

A maintainer's tool. It reads public release pages and package registries
and downloads the files it pins into a scratch directory to hash them; it
never runs, unpacks to disk or builds anything it downloads (npm tarballs
are read in memory, two members only: package.json and binding.gyp).

    pin-index.py WANTED.json SCRATCH_DIR > index.json

WANTED.json lists Electron versions and native-module artifacts:

    {"electron": ["43.7.7", ...],
     "npm_packages": ["@vscode/ripgrep-linux-x64@1.18.0", ...],
     "npm_prebuilds": {"better-sqlite3@13.0.3": "prebuilds/linux-x64.node", ...},
     "release_prebuilds": {"keytar@7.9.0": "https://github.com/...tar.gz", ...},
     "source_builds": ["native-reg@1.1.1", ...]}

Where a digest is published (Electron's SHASUMS256.txt, a GitHub asset
digest) the downloaded file must match it, and the record says so; where
none is published (npm publishes SHA-512 integrity, which is kept beside),
the digest is the one computed here, and the record says that too.
"""

import hashlib
import io
import json
import os
import re
import sys
import tarfile
import time
import urllib.parse
import urllib.request

AGENT = "tur-rehost-pin/1"


def get(url, limit=1 << 30):
    request = urllib.request.Request(url, headers={"User-Agent": AGENT})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                return response.read(limit)
        except OSError:
            if attempt == 2:
                raise
            time.sleep(3)
    raise RuntimeError(url)


def get_json(url):
    return json.loads(get(url, 64 << 20))


def download(url, scratch):
    name = hashlib.sha256(url.encode()).hexdigest()[:24]
    path = os.path.join(scratch, name)
    if not os.path.exists(path):
        data = get(url)
        with open(path + ".part", "wb") as out:
            out.write(data)
        os.replace(path + ".part", path)
    with open(path, "rb") as handle:
        data = handle.read()
    return data, hashlib.sha256(data).hexdigest(), len(data)


def shasums(url):
    out = {}
    for line in get(url, 4 << 20).decode("utf-8", "replace").splitlines():
        parts = line.split()
        if len(parts) == 2 and re.fullmatch(r"[0-9a-f]{64}", parts[0]):
            out[parts[1].lstrip("*")] = parts[0]
    return out


def npm_meta(ident):
    name, version = ident.rsplit("@", 1)
    return get_json("https://registry.npmjs.org/" + name.replace("/", "%2f") + "/" + urllib.parse.quote(version, safe=""))


def npm_tarball(ident, scratch):
    meta = npm_meta(ident)
    url = meta["dist"]["tarball"]
    data, sha256, size = download(url, scratch)
    integrity = meta["dist"].get("integrity")
    if integrity and integrity.startswith("sha512-"):
        import base64

        expected = base64.b64decode(integrity[7:])
        if hashlib.sha512(data).digest() != expected:
            raise SystemExit(f"{ident}: npm integrity does not match the tarball")
    return {"url": url, "sha256": sha256, "size": size, "npm_integrity": integrity,
            "hash_source": "computed by pin-index.py; the npm registry's SHA-512 integrity matched"}, meta, data


def members(data, names):
    found = {}
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        for member in archive.getmembers():
            rel = member.name.split("/", 1)[1] if "/" in member.name else member.name
            if member.isfile() and rel in names and member.size < (4 << 20):
                found[rel] = archive.extractfile(member).read()
    return found


def version_key(text):
    core = text.split("+")[0]
    main, _, pre = core.partition("-")
    numbers = [int(x) if x.isdigit() else 0 for x in main.split(".")[:3]]
    while len(numbers) < 3:
        numbers.append(0)
    return (numbers, 0 if pre else 1, pre)


def satisfies(version, spec):
    """npm ranges as the packages here write them: exact, ^, ~, >=, *, x."""
    if "-" in version.split("+")[0]:
        return spec.strip().lstrip("=^~") == version
    v = version_key(version)[0]
    for alternative in spec.split("||"):
        part = alternative.strip()
        if part in ("", "*", "latest", "x"):
            return True
        match = re.fullmatch(r"([\^~]|>=)?\s*v?(\d+)(?:\.(\d+|x|\*))?(?:\.(\d+|x|\*))?(?:-[0-9A-Za-z.-]+)?", part)
        if not match:
            continue
        op, major, minor, patch = match.groups()
        base = [int(major), int(minor) if minor and minor.isdigit() else 0, int(patch) if patch and patch.isdigit() else 0]
        if op == ">=":
            if v >= base:
                return True
        elif op == "^":
            if v >= base and (v[0] == base[0] if base[0] else (v[1] == base[1] if base[1] else v[2] == base[2])):
                return True
        elif op == "~":
            if v >= base and v[0] == base[0] and v[1] == base[1]:
                return True
        else:
            wild_minor = not (minor and minor.isdigit())
            wild_patch = not (patch and patch.isdigit())
            if v[0] == base[0] and (wild_minor or v[1] == base[1]) and (wild_minor or wild_patch or v[2] == base[2]):
                return True
    return False


def resolve(name, spec):
    meta = get_json("https://registry.npmjs.org/" + name.replace("/", "%2f"))
    versions = [v for v in meta.get("versions", {}) if satisfies(v, spec or "*")]
    stable = [v for v in versions if "-" not in v] or versions
    if not stable:
        raise SystemExit(f"{name}: nothing satisfies {spec!r}")
    return name + "@" + max(stable, key=version_key)


def main():
    wanted = json.load(open(sys.argv[1]))
    scratch = sys.argv[2]
    os.makedirs(scratch, exist_ok=True)
    headers_index = {row["version"]: row for row in get_json("https://artifacts.electronjs.org/headers/dist/index.json")}
    index = {
        "schema_version": 1,
        "checked": time.strftime("%Y-%m-%d", time.gmtime()),
        "generated_by": "services/tur-rehost/data/pin-index.py; do not edit by hand",
        "electron": {},
        "modules": {},
    }
    for version in wanted["electron"]:
        base = f"https://github.com/electron/electron/releases/download/v{version}/"
        sums = shasums(base + "SHASUMS256.txt")
        assets = {a["name"]: a for a in get_json(f"https://api.github.com/repos/electron/electron/releases/tags/v{version}")["assets"]}
        entry = {"abi": int(headers_index[version]["modules"]), "chromium": headers_index[version]["chrome"], "node": headers_index[version]["node"]}
        for key, name in (("linux_x64", f"electron-v{version}-linux-x64.zip"), ("ffmpeg_open_codecs", f"ffmpeg-v{version}-linux-x64.zip")):
            digest = (assets[name].get("digest") or "").removeprefix("sha256:") or None
            if digest and digest != sums[name]:
                raise SystemExit(f"{name}: GitHub's digest and SHASUMS256.txt differ")
            entry[key] = {"url": base + name, "sha256": sums[name], "size": assets[name]["size"],
                          "hash_source": f"{base}SHASUMS256.txt" + (" and GitHub's asset digest" if digest else "")}
        hbase = f"https://artifacts.electronjs.org/headers/dist/v{version}/"
        hsums = shasums(hbase + "SHASUMS256.txt")
        hname = f"node-v{version}-headers.tar.gz"
        data, sha256, size = download(hbase + hname, scratch)
        if sha256 != hsums[hname]:
            raise SystemExit(f"{hname}: does not match {hbase}SHASUMS256.txt")
        entry["headers"] = {"url": hbase + hname, "sha256": sha256, "size": size, "hash_source": hbase + "SHASUMS256.txt"}
        index["electron"][version] = entry
        print("electron", version, file=sys.stderr)

    for ident in wanted["npm_packages"]:
        artifact, meta, _ = npm_tarball(ident, scratch)
        index["modules"][ident] = {"tier": "npm-package", "licence": meta.get("license"), "artifact": artifact}
        print("npm-package", ident, file=sys.stderr)
    for ident, member in wanted["npm_prebuilds"].items():
        artifact, meta, data = npm_tarball(ident, scratch)
        # A path ending in "/" takes every file below it (a Linux build that
        # needs a library beside it); otherwise that one file.
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            inside = [m.name.split("/", 1)[1] for m in archive.getmembers() if m.isfile() and "/" in m.name]
        if member.endswith("/"):
            names = [name for name in inside if name.startswith(member)]
        else:
            names = [name for name in inside if name == member]
        if not names:
            raise SystemExit(f"{ident}: {member} is not in the tarball")
        index["modules"][ident] = {"tier": "npm-prebuild", "licence": meta.get("license"), "artifact": artifact, "files": sorted(names)}
        print("npm-prebuild", ident, file=sys.stderr)
    for ident, url in wanted["release_prebuilds"].items():
        data, sha256, size = download(url, scratch)
        path = urllib.parse.urlparse(url).path.split("/")
        repo, tag, asset = path[1] + "/" + path[2], urllib.parse.unquote(path[5]), urllib.parse.unquote(path[6])
        release = get_json(f"https://api.github.com/repos/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}")
        published = next((a.get("digest") for a in release.get("assets", []) if a["name"] == asset), None)
        if published and published.removeprefix("sha256:") != sha256:
            raise SystemExit(f"{url}: GitHub's digest differs from the file")
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            nodes = [m.name for m in archive.getmembers() if m.isfile() and m.name.endswith(".node")]
        meta = npm_meta(ident)
        index["modules"][ident] = {"tier": "release-prebuild", "licence": meta.get("license"),
                                   "artifact": {"url": url, "sha256": sha256, "size": size,
                                                "hash_source": "GitHub's asset digest" if published else "computed by pin-index.py: the release publishes no digest"},
                                   "files": nodes}
        print("release-prebuild", ident, file=sys.stderr)
    for ident in wanted["source_builds"]:
        artifact, meta, data = npm_tarball(ident, scratch)
        found = members(data, {"package.json", "binding.gyp"})
        package = json.loads(found["package.json"])
        gyp = found.get("binding.gyp", b"").decode("utf-8", "replace")
        required = sorted(set(re.findall(r"""require\(\s*['"]([@A-Za-z0-9_./-]+)['"]\s*\)""", gyp)))
        required = [name.split("/")[0] if not name.startswith("@") else "/".join(name.split("/")[:2]) for name in required]
        headers = []
        for name in sorted(set(required)):
            spec = (package.get("dependencies") or {}).get(name) or (package.get("devDependencies") or {}).get(name) or "*"
            dependency = resolve(name, spec)
            dep_artifact, dep_meta, _ = npm_tarball(dependency, scratch)
            headers.append({"package": dependency, "range": spec, "licence": dep_meta.get("license"), "artifact": dep_artifact})
        index["modules"][ident] = {
            "tier": "source-build", "licence": meta.get("license"), "artifact": artifact,
            "node_gyp": "binding.gyp" in found, "os": package.get("os") or [],
            "build_packages": headers,
        }
        print("source-build", ident, [h["package"] for h in headers], file=sys.stderr)
    json.dump(index, sys.stdout, indent=1, sort_keys=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

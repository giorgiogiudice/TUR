# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Downloads, with a cache directory and a record of every file fetched
(URL, size, SHA-256), and the official locations of each Linux runtime.

Everything fetched here is the open-source Linux build of a runtime, or an
open-source library's Linux build, from its project's own release location.
Nothing is committed to the repository. The licences are listed in
docs/reports/exp-rehost-1.md.
"""

import hashlib
import json
import os
import struct
import time
import urllib.error
import urllib.parse
import urllib.request
import zlib

USER_AGENT = "rehost-experiment/1 (CI)"

FETCHED = []  # every download of this process: dicts for the evidence


class FetchError(Exception):
    pass


def _open(url, headers=None, timeout=120):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    return urllib.request.urlopen(request, timeout=timeout)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(4 << 20)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def download(url, dest, sha256=None, attempts=3, what=""):
    """Fetch url to dest unless it is already there. Returns the record."""
    record = {"url": url, "path": dest, "what": what, "cached": False}
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        record["cached"] = True
    else:
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        error = None
        for attempt in range(attempts):
            try:
                started = time.time()
                with _open(url, timeout=180) as response, open(dest + ".part", "wb") as out:
                    while True:
                        chunk = response.read(4 << 20)
                        if not chunk:
                            break
                        out.write(chunk)
                os.replace(dest + ".part", dest)
                record["seconds"] = round(time.time() - started, 1)
                error = None
                break
            except urllib.error.HTTPError as problem:
                error = f"HTTP {problem.code}"
                if problem.code in (403, 404, 410):
                    break
            except (urllib.error.URLError, OSError, TimeoutError) as problem:
                error = str(problem)
            time.sleep(3 * (attempt + 1))
        if error:
            try:
                os.unlink(dest + ".part")
            except OSError:
                pass
            raise FetchError(f"{url}: {error}")
    record["size"] = os.path.getsize(dest)
    record["sha256"] = sha256_file(dest)
    if sha256 and record["sha256"] != sha256.lower():
        os.unlink(dest)
        raise FetchError(f"{url}: SHA-256 is {record['sha256']}, expected {sha256}")
    record["pinned"] = bool(sha256)
    FETCHED.append({key: value for key, value in record.items() if key != "path"})
    return record


def get(url, timeout=60, attempts=3):
    error = None
    for attempt in range(attempts):
        try:
            with _open(url, timeout=timeout) as response:
                return response.read()
        except urllib.error.HTTPError as problem:
            error = f"HTTP {problem.code}"
            if problem.code in (403, 404, 410):
                break
        except (urllib.error.URLError, OSError, TimeoutError) as problem:
            error = str(problem)
        time.sleep(2 * (attempt + 1))
    raise FetchError(f"{url}: {error}")


def get_json(url, cache_file=None):
    if cache_file and os.path.exists(cache_file):
        with open(cache_file, "rb") as handle:
            return json.loads(handle.read())
    raw = get(url)
    if cache_file:
        os.makedirs(os.path.dirname(cache_file), exist_ok=True)
        with open(cache_file, "wb") as handle:
            handle.write(raw)
    return json.loads(raw)


def exists(url):
    try:
        request = urllib.request.Request(url, method="HEAD", headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status == 200
    except (urllib.error.URLError, OSError, TimeoutError):
        return False


def github_asset(repo, tag, asset):
    return f"https://github.com/{repo}/releases/download/{urllib.parse.quote(tag, safe='')}/{urllib.parse.quote(asset, safe='')}"


# ----------------------------------------------------- one member of a remote zip


def _range(url, start, end):
    with _open(url, headers={"Range": f"bytes={start}-{end}"}, timeout=180) as response:
        if response.status != 206:
            raise FetchError(f"{url}: the server ignored the range request (status {response.status})")
        return response.read(), response.headers.get("Content-Range", "")


def remote_zip_member(url, wanted, dest, what=""):
    """Download one member of a zip archive on a server that answers range
    requests, without the rest of the archive (Godot's export templates are
    one 1.2 GB zip of which one 70 MB file is needed). `wanted` is a list of
    member names, tried in order. Returns the record."""
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        record = {"url": url, "member": "(cached)", "size": os.path.getsize(dest), "sha256": sha256_file(dest), "cached": True, "what": what}
        FETCHED.append(record)
        return record
    with _open(url, headers={"Range": "bytes=0-0"}) as response:
        content_range = response.headers.get("Content-Range", "")
        final_url = response.geturl()
    if "/" not in content_range:
        raise FetchError(f"{url}: no range support")
    total = int(content_range.rsplit("/", 1)[1])
    tail, _ = _range(final_url, max(0, total - 70000), total - 1)
    at = tail.rfind(b"PK\x05\x06")
    if at == -1:
        raise FetchError(f"{url}: no end-of-central-directory record")
    entries, cd_size, cd_offset = struct.unpack_from("<HII", tail, at + 10)
    if cd_offset == 0xFFFFFFFF or cd_size == 0xFFFFFFFF or entries == 0xFFFF:
        locator = tail.rfind(b"PK\x06\x07", 0, at)
        if locator == -1:
            raise FetchError(f"{url}: zip64 without a locator")
        eocd64 = struct.unpack_from("<Q", tail, locator + 8)[0]
        record64, _ = _range(final_url, eocd64, eocd64 + 55)
        cd_size, cd_offset = struct.unpack_from("<QQ", record64, 40)
    directory, _ = _range(final_url, cd_offset, cd_offset + cd_size - 1)
    members = {}
    position = 0
    while position + 46 <= len(directory) and directory[position : position + 4] == b"PK\x01\x02":
        method, = struct.unpack_from("<H", directory, position + 10)
        csize, usize = struct.unpack_from("<II", directory, position + 20)
        name_len, extra_len, comment_len = struct.unpack_from("<HHH", directory, position + 28)
        local = struct.unpack_from("<I", directory, position + 42)[0]
        name = directory[position + 46 : position + 46 + name_len].decode("utf-8", "replace")
        extra = directory[position + 46 + name_len : position + 46 + name_len + extra_len]
        cursor = 0
        while cursor + 4 <= len(extra):
            tag, length = struct.unpack_from("<HH", extra, cursor)
            if tag == 1:
                fields = extra[cursor + 4 : cursor + 4 + length]
                index = 0
                if usize == 0xFFFFFFFF:
                    usize = struct.unpack_from("<Q", fields, index)[0]
                    index += 8
                if csize == 0xFFFFFFFF:
                    csize = struct.unpack_from("<Q", fields, index)[0]
                    index += 8
                if local == 0xFFFFFFFF:
                    local = struct.unpack_from("<Q", fields, index)[0]
            cursor += 4 + length
        members[name] = (method, csize, usize, local)
        position += 46 + name_len + extra_len + comment_len
    chosen = next((name for name in wanted if name in members), None)
    if chosen is None:
        raise FetchError(f"{url}: none of {wanted} in the archive; it has {sorted(members)[:60]}")
    method, csize, usize, local = members[chosen]
    head, _ = _range(final_url, local, local + 29)
    name_len, extra_len = struct.unpack_from("<HH", head, 26)
    start = local + 30 + name_len + extra_len
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    request = urllib.request.Request(final_url, headers={"User-Agent": USER_AGENT, "Range": f"bytes={start}-{start + csize - 1}"})
    inflate = zlib.decompressobj(-15) if method == 8 else None
    if method not in (0, 8):
        raise FetchError(f"{url}: member {chosen} uses compression method {method}")
    with urllib.request.urlopen(request, timeout=600) as response, open(dest + ".part", "wb") as out:
        while True:
            chunk = response.read(4 << 20)
            if not chunk:
                break
            out.write(inflate.decompress(chunk) if inflate else chunk)
        if inflate:
            out.write(inflate.flush())
    os.replace(dest + ".part", dest)
    if os.path.getsize(dest) != usize:
        raise FetchError(f"{url}: member {chosen} is {os.path.getsize(dest)} bytes, the directory says {usize}")
    record = {"url": url, "member": chosen, "size": usize, "compressed": csize, "of_archive": total, "sha256": sha256_file(dest), "cached": False, "what": what}
    FETCHED.append(record)
    return record


# ------------------------------------------------------------- package indexes


def npm_metadata(name, version, cache):
    safe = name.replace("/", "%2f")
    try:
        return get_json(f"https://registry.npmjs.org/{safe}/{version}", os.path.join(cache, "npm-meta", f"{name.replace('/', '__')}@{version}.json"))
    except (FetchError, ValueError):
        return None


def npm_tarball(name, version, cache):
    """The path of name@version's tarball from the npm registry, or None."""
    meta = npm_metadata(name, version, cache)
    if not meta or "dist" not in meta:
        return None, None
    dest = os.path.join(cache, "npm", f"{name.replace('/', '__')}-{version}.tgz")
    try:
        download(meta["dist"]["tarball"], dest, what=f"npm {name}@{version}")
    except FetchError:
        return None, meta
    return dest, meta


def nuget_package(package, version, cache):
    lower = package.lower()
    dest = os.path.join(cache, "nuget", f"{lower}.{version.lower()}.nupkg")
    try:
        download(f"https://api.nuget.org/v3-flatcontainer/{lower}/{version.lower()}/{lower}.{version.lower()}.nupkg", dest, what=f"nuget {package} {version}")
    except FetchError:
        return None
    return dest


def maven_artifact(group, artifact, version, cache, classifier=None):
    name = f"{artifact}-{version}" + (f"-{classifier}" if classifier else "") + ".jar"
    dest = os.path.join(cache, "maven", name)
    url = f"https://repo1.maven.org/maven2/{group.replace('.', '/')}/{artifact}/{version}/{name}"
    try:
        download(url, dest, what=f"maven {group}:{artifact}:{version}" + (f":{classifier}" if classifier else ""))
    except FetchError:
        return None
    return dest


def maven_versions(group, artifact):
    try:
        raw = get(f"https://repo1.maven.org/maven2/{group.replace('.', '/')}/{artifact}/maven-metadata.xml").decode("utf-8", "replace")
    except FetchError:
        return []
    import re

    return re.findall(r"<version>([^<]+)</version>", raw)


# -------------------------------------------------------------- Linux runtimes


def electron_linux(version, cache):
    """Electron's own Linux build, from the project's GitHub releases (MIT;
    it bundles Chromium and Node.js under their licences, listed in the
    LICENSES.chromium.html the archive carries)."""
    name = f"electron-v{version}-linux-x64.zip"
    dest = os.path.join(cache, "runtimes-github", name)
    record = download(f"https://github.com/electron/electron/releases/download/v{version}/{name}", dest, what=f"Electron {version} for Linux")
    try:
        sums = get(f"https://github.com/electron/electron/releases/download/v{version}/SHASUMS256.txt").decode("utf-8", "replace")
        listed = next((line.split()[0] for line in sums.splitlines() if line.strip().endswith(name)), None)
        record["publisher_sha256_matches"] = listed == record["sha256"] if listed else None
    except FetchError:
        record["publisher_sha256_matches"] = None
    return dest, record


def electron_abi(version, cache):
    """NODE_MODULE_VERSION of an Electron release, from Electron's header
    index (what node-gyp reads)."""
    for url in ("https://artifacts.electronjs.org/headers/dist/index.json", "https://electronjs.org/headers/index.json"):
        try:
            index = get_json(url, os.path.join(cache, "meta", "electron-headers-index.json"))
        except (FetchError, ValueError):
            continue
        for row in index:
            if row.get("version", "").lstrip("v") == version:
                return str(row.get("modules") or "")
    return ""


def nwjs_versions(cache):
    try:
        return get_json("https://nwjs.io/versions.json", os.path.join(cache, "meta", "nwjs-versions.json"))
    except (FetchError, ValueError):
        return {}


def nwjs_linux(version, cache, sdk=False):
    """NW.js's own Linux build from dl.nwjs.io (MIT)."""
    flavour = "nwjs-sdk" if sdk else "nwjs"
    name = f"{flavour}-v{version}-linux-x64.tar.gz"
    dest = os.path.join(cache, "runtimes", name)
    record = download(f"https://dl.nwjs.io/v{version}/{name}", dest, what=f"NW.js {version} for Linux")
    return dest, record


def godot_template(major, minor, patch, cache, mono=False):
    """The Linux release export template of a Godot version: one member of
    the project's export-templates archive on GitHub (MIT)."""
    version = f"{major}.{minor}" + (f".{patch}" if patch else "")
    tag = f"{version}-stable"
    archive = f"Godot_v{tag}_{'mono_' if mono else ''}export_templates.tpz"
    url = f"https://github.com/godotengine/godot/releases/download/{tag}/{archive}"
    names = ["templates/linux_release.x86_64", "templates/linux_x11_64_release"]
    dest = os.path.join(cache, "runtimes-github", f"godot-{version}{'-mono' if mono else ''}-linux-release.x86_64")
    record = remote_zip_member(url, names, dest, what=f"Godot {version} Linux export template")
    os.chmod(dest, 0o755)
    return dest, record


def love_linux(version, cache):
    """LÖVE's own AppImage from the project's GitHub releases (zlib)."""
    name = f"love-{version}-x86_64.AppImage"
    dest = os.path.join(cache, "runtimes-github", name)
    record = download(f"https://github.com/love2d/love/releases/download/{version}/{name}", dest, what=f"LÖVE {version} for Linux")
    os.chmod(dest, 0o755)
    return dest, record


def renpy_sdk(version, cache):
    """Ren'Py's SDK from renpy.org (MIT, with LGPL parts): the engine and
    its Linux libraries."""
    short = ".".join(version.split(".")[:3])
    name = f"renpy-{short}-sdk.tar.bz2"
    dest = os.path.join(cache, "runtimes", name)
    record = download(f"https://www.renpy.org/dl/{short}/{name}", dest, what=f"Ren'Py {short} SDK")
    return dest, record

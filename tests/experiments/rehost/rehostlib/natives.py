# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Native Node modules: finding the Linux build of each one a Windows or
macOS Electron payload carries, and putting it where the payload looks.

For every package that owns a foreign .node file, in this order:

  carried            the package already holds a Linux x86-64 build (packages
                     built with prebuildify ship every platform)
  platform-package   the binary lives in a per-platform npm package
                     (@scope/name-win32-x64-msvc): the linux-x64 package of
                     the same version is fetched from the npm registry
  npm-prebuild       the package's own npm tarball, same version, holds a
                     Linux prebuild
  release-prebuild   the package publishes prebuilds as release assets
                     (prebuild-install, node-pre-gyp): the Linux asset for
                     N-API or for this Electron's ABI is fetched
  needs-build        none of those, but the source is in the npm tarball:
                     it would have to be compiled for this Electron
  dead-end           the package cannot be identified, or is not on npm

Everything fetched is the module's own published build, at the version the
payload names. Nothing is compiled here.
"""

import io
import os
import re
import tarfile

from . import fetch
from .detect import kind_from_head

PLATFORM_TOKENS = [
    "win32-x64-msvc",
    "win32-x64-gnu",
    "win32-x64",
    "windows-x64",
    "win-x64",
    "darwin-x64",
    "darwin-arm64",
    "darwin-universal",
    "macos-x64",
    "macos-arm64",
    "osx-x64",
]
LINUX_TOKENS = ["linux-x64-gnu", "linux-x64-glibc", "linux-x64"]


def tar_members(path):
    """{relative path: TarInfo} of an npm tarball (members sit in package/)."""
    out = {}
    try:
        archive = tarfile.open(path)
    except (tarfile.TarError, OSError):
        return None, {}
    for member in archive.getmembers():
        if member.isfile():
            out[member.name.split("/", 1)[1] if "/" in member.name else member.name] = member
    return archive, out


def is_linux_x64(data):
    kind = kind_from_head(data[:4096])
    return bool(kind) and kind[0] == "elf" and "x86_64" in kind[1]


def rank(path, abi):
    """Lower is better among candidate Linux builds of one module."""
    lower = path.lower()
    if "musl" in lower or "arm" in lower or "ia32" in lower:
        return 99
    score = 10
    if "napi" in lower:
        score = 1
    if abi and (f"abi{abi}" in lower or f"-v{abi}-" in lower):
        score = 0 if "electron" in lower else 2
    elif "abi" in lower:
        score = 50  # built for one ABI, and not this one
    if "electron" in lower and score > 2:
        score = 3
    return score


class Resolver:
    def __init__(self, payload, electron_version, cache, log=print, runtime="electron"):
        self.payload = payload
        self.version = electron_version
        self.cache = cache
        self.log = log
        self.runtime = runtime
        self.abi = fetch.electron_abi(electron_version, cache) if runtime == "electron" and electron_version else ""
        self.changed = False

    def put(self, rel, data):
        if self.payload.asar:
            self.payload.asar.put_unpacked(rel, data, executable=rel.endswith(".node"))
        else:
            target = os.path.join(self.payload.directory, rel)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, "wb") as handle:
                handle.write(data)
        self.changed = True

    def over(self, foreign, data, applied):
        """Replace foreign builds that sit where a build-from-source would
        put them (build/Release): loaders look there first."""
        for item in foreign:
            if "/build/" in "/" + item["path"] or "/bin/" in "/" + item["path"]:
                self.put(item["path"], data)
                applied.append(item["path"])

    def resolve_all(self, natives):
        groups = {}
        for item in natives:
            groups.setdefault(item["package_dir"] or "?" + item["path"], []).append(item)
        results = []
        for directory, items in sorted(groups.items()):
            foreign = [item for item in items if item["format"] != "elf"]
            if not foreign:
                continue
            first = foreign[0]
            result = {"package": first["package"], "version": first["version"], "dir": first["package_dir"], "files": [item["path"] for item in foreign], "status": "dead-end", "detail": "", "applied": []}
            try:
                self.resolve(result, foreign, [item for item in items if item["format"] == "elf" and "x86_64" in item["archs"]])
            except (fetch.FetchError, OSError, tarfile.TarError, ValueError, KeyError) as problem:
                result["detail"] = f"error while resolving: {problem}"
            self.log(f"    native {result['package']}@{result['version']}: {result['status']} {result['detail']}")
            results.append(result)
        if self.changed and self.payload.asar:
            self.payload.asar.save()
        return results

    def resolve(self, result, foreign, carried):
        name, version, directory = result["package"], result["version"], result["dir"]
        if not name or directory is None:
            result["detail"] = "no package.json above the file"
            return
        package = self.payload.read_json(directory + "/package.json") or {}

        carried = sorted((item for item in carried if rank(item["path"], self.abi) < 50), key=lambda item: rank(item["path"], self.abi))
        if carried:
            result["status"] = "carried"
            result["detail"] = carried[0]["path"]
            data = self.payload.read(carried[0]["path"], 256 << 20)
            if data:
                self.over(foreign, data, result["applied"])
            return

        token = next((token for token in PLATFORM_TOKENS if token in name), None)
        if token:
            for linux in LINUX_TOKENS:
                sibling = name.replace(token, linux)
                tarball, _ = fetch.npm_tarball(sibling, version, self.cache)
                if not tarball:
                    continue
                archive, members = tar_members(tarball)
                if not members:
                    continue
                base = directory[: -len(name)] + sibling if directory.endswith(name) else os.path.dirname(directory) + "/" + sibling.split("/")[-1]
                count = 0
                for rel, member in members.items():
                    self.put(f"{base}/{rel}", archive.extractfile(member).read())
                    count += 1
                result["status"] = "platform-package"
                result["detail"] = f"{sibling}@{version} from npm ({count} files) at {base}"
                result["applied"].append(base)
                return
            result["detail"] = f"no Linux sibling of {name}@{version} on npm; "

        tarball, meta = fetch.npm_tarball(name, version, self.cache)
        members = {}
        archive = None
        if tarball:
            archive, members = tar_members(tarball)
        candidates = []
        for rel, member in members.items():
            lower = rel.lower()
            if lower.endswith(".node") and "linux" in lower and ("x64" in lower or "x86_64" in lower):
                candidates.append(rel)
        candidates.sort(key=lambda rel: rank(rel, self.abi))
        if candidates and rank(candidates[0], self.abi) < 50:
            data = archive.extractfile(members[candidates[0]]).read()
            if is_linux_x64(data):
                for rel in candidates:
                    if rank(rel, self.abi) < 50:
                        self.put(f"{directory}/{rel}", archive.extractfile(members[rel]).read())
                        result["applied"].append(f"{directory}/{rel}")
                self.over(foreign, data, result["applied"])
                result["status"] = "npm-prebuild"
                result["detail"] = f"{candidates[0]} in the npm tarball of {name}@{version}"
                return

        found = self.release_prebuild(name, version, package or (meta or {}))
        if found:
            url, files = found
            for rel, data in files.items():
                target = next((item["path"] for item in foreign if os.path.basename(item["path"]) == os.path.basename(rel)), f"{directory}/{rel}")
                self.put(target, data)
                result["applied"].append(target)
            result["status"] = "release-prebuild"
            result["detail"] = url
            return

        if meta is None and not tarball:
            result["status"] = "dead-end"
            result["detail"] += f"{name}@{version} is not on the npm registry"
        elif any(rel in members for rel in ("binding.gyp", "Cargo.toml", "CMakeLists.txt")) or (meta or {}).get("gypfile"):
            result["status"] = "needs-build"
            result["detail"] += "no Linux prebuild published; the source is in the npm tarball" + (" (N-API)" if "napi" in str(package.get("dependencies", {})) or "node-addon-api" in str(package.get("dependencies", {})) else "")
        else:
            result["status"] = "dead-end"
            result["detail"] += "no Linux prebuild and no source in the npm tarball"

    def release_prebuild(self, name, version, package):
        """prebuild-install's and node-pre-gyp's naming of release assets."""
        urls = []
        short = name.split("/")[-1]
        repository = package.get("repository")
        repository = repository.get("url") if isinstance(repository, dict) else repository
        match = re.search(r"github\.com[/:]([^/]+/[^/#.]+)", repository or "")
        binary = package.get("binary") if isinstance(package.get("binary"), dict) else {}
        napi = [str(value) for value in (binary.get("napi_versions") or [])][::-1] or ["9", "8", "6", "4", "3"]
        runtimes = [f"napi-v{value}" for value in napi[:5]]
        if self.abi:
            runtimes.append(f"electron-v{self.abi}")
        if binary.get("host") and (binary.get("package_name") or binary.get("remote_path")):
            for runtime in runtimes:
                values = {
                    "version": version,
                    "name": short,
                    "module_name": binary.get("module_name", short),
                    "platform": "linux",
                    "arch": "x64",
                    "libc": "glibc",
                    "configuration": "Release",
                    "node_abi": runtime if runtime.startswith("electron") else "napi",
                    "napi_build_version": runtime.split("-v")[-1],
                    "node_abi_napi": "napi",
                    "toolset": "",
                }
                text = "/".join(part.strip("/") for part in (binary["host"], binary.get("remote_path", ""), binary.get("package_name", "{module_name}-v{version}-{node_abi}-{platform}-{arch}.tar.gz")) if part)
                for key, value in values.items():
                    text = text.replace("{" + key + "}", value)
                if "{" not in text:
                    urls.append(text)
        if match and ("prebuild-install" in str(package.get("dependencies", {})) or "prebuild-install" in str(package.get("scripts", {})) or not urls):
            for tag in (f"v{version}", version):
                for runtime in runtimes:
                    urls.append(f"https://github.com/{match.group(1)}/releases/download/{tag}/{short}-v{version}-{runtime}-linux-x64.tar.gz")
        seen = set()
        for url in urls[:14]:
            if url in seen:
                continue
            seen.add(url)
            dest = os.path.join(self.cache, "prebuilds", re.sub(r"[^A-Za-z0-9._-]", "_", url)[-180:])
            try:
                fetch.download(url, dest, attempts=1, what=f"prebuild of {name}@{version}")
            except fetch.FetchError:
                continue
            files = {}
            try:
                with tarfile.open(dest) as archive:
                    for member in archive.getmembers():
                        if member.isfile() and member.name.endswith(".node"):
                            data = archive.extractfile(member).read()
                            if is_linux_x64(data):
                                files[member.name.lstrip("./")] = data
            except (tarfile.TarError, OSError):
                continue
            if files:
                return url, files
        return None


def summarise(results):
    counts = {}
    for result in results:
        counts[result["status"]] = counts.get(result["status"], 0) + 1
    return counts


def read_tar_bytes(data):
    return tarfile.open(fileobj=io.BytesIO(data))

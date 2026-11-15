# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Building a native Node module from its npm source for one Electron
version: the "source-build" tier of rehostlib/natives.py.

Run 43557782687 found that about half of the foreign native modules in the
Electron corpus publish no Linux build at all ("needs-build"), and that
those were what kept MarkText, Hyper and others from starting. This tier
asks how many of them build from the source in their own npm tarball with
nothing but Fedora's compiler and Electron's published headers: what a
build service run by TUR would face.

    build(name, version, electron_version, tarball, cache, log)
        -> ({file name: bytes of the Linux x86-64 .node}, detail)

Only node-gyp projects (binding.gyp) are tried; Rust (napi-rs) and CMake
projects are reported as such. A result, good or bad, is kept in
CACHE/builds so that the pretend variant and later runs do not build again.
The build runs the package's own build description, which can execute
commands: it happens in a throw-away CI container only, never on a
development machine (AGENTS.md).
"""

import json
import os
import re
import shutil
import subprocess
import tarfile
import time

NODE_GYP = "node-gyp@11.2.0"  # npm, 2025-04-01; pinned


def _safe(text):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", text)


def _elf_x64(data):
    return data[:4] == b"\x7fELF" and len(data) > 20 and data[18:20] == b"\x3e\x00"


def tools():
    """What is missing to build at all."""
    return [tool for tool in ("node", "npm", "npx", "make", "g++", "python3") if not shutil.which(tool)]


def _recipe():
    """A digest of this file: a cached result is reused only by the recipe
    that produced it (the CI cache is restored across changes)."""
    import hashlib

    with open(os.path.abspath(__file__), "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()[:10]


def build(name, version, electron_version, tarball, cache, log=print, timeout=420):
    key = os.path.join(cache, "builds", "r-" + _recipe(), _safe(f"{name}-{version}-electron-{electron_version}"))
    marker = os.path.join(key, "result.json")
    if os.path.isfile(marker):
        with open(marker, encoding="utf-8") as handle:
            result = json.load(handle)
        files = {}
        for item in result.get("files", []):
            with open(os.path.join(key, item), "rb") as handle:
                files[item] = handle.read()
        return files, result["detail"] + " (cached)"

    def finish(files, detail):
        os.makedirs(key, exist_ok=True)
        for item, data in files.items():
            with open(os.path.join(key, item), "wb") as handle:
                handle.write(data)
        with open(marker, "w", encoding="utf-8") as handle:
            json.dump({"files": sorted(files), "detail": detail}, handle)
        return files, detail

    missing = tools()
    if missing:
        return {}, "no build tools here: " + ", ".join(missing)  # not cached: an environment fault
    if not tarball:
        return finish({}, "no npm tarball")
    work = os.path.join(cache, "builds", "work", _safe(f"{name}-{version}"))
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    try:
        with tarfile.open(tarball) as archive:
            for member in archive.getmembers():
                parts = member.name.split("/", 1)
                if len(parts) < 2 or not (member.isfile() or member.isdir()) or ".." in parts[1].split("/"):
                    continue
                member.name = parts[1]
                if hasattr(tarfile, "data_filter"):
                    archive.extract(member, work, filter="data")
                else:
                    archive.extract(member, work)
    except (tarfile.TarError, OSError) as problem:
        return finish({}, f"the npm tarball could not be unpacked: {problem}")
    package = {}
    try:
        with open(os.path.join(work, "package.json"), encoding="utf-8") as handle:
            package = json.load(handle)
    except (OSError, ValueError):
        pass
    systems = package.get("os") if isinstance(package.get("os"), list) else []
    # npm's "os": a list of systems, or of exclusions ("!win32"); run
    # 43754196480 read ["!win32"] (fs-xattr) as "not for Linux".
    allowed = [item for item in systems if not str(item).startswith("!")]
    if "!linux" in systems or (allowed and "linux" not in allowed):
        shutil.rmtree(work, ignore_errors=True)
        return finish({}, f"the package says it is not for Linux (os: {', '.join(systems)})")
    if not os.path.isfile(os.path.join(work, "binding.gyp")):
        kind = "Rust (napi-rs)" if os.path.isfile(os.path.join(work, "Cargo.toml")) else "CMake" if os.path.isfile(os.path.join(work, "CMakeLists.txt")) else "no build description"
        shutil.rmtree(work, ignore_errors=True)
        return finish({}, f"not a node-gyp project ({kind}): not tried")
    home = os.path.join(cache, "builds", "home")
    os.makedirs(home, exist_ok=True)
    env = dict(os.environ, HOME=home, npm_config_cache=os.path.join(cache, "builds", "npm-cache"), npm_config_update_notifier="false", npm_config_fund="false", npm_config_audit="false")
    started = time.time()
    with open(os.path.join(work, "binding.gyp"), encoding="utf-8", errors="replace") as handle:
        gyp = handle.read()
    declared = set(package.get("dependencies") or {})
    # Header packages the build description uses but the package lists only
    # as a development dependency (run 43754196480: native-reg, "Cannot find
    # module 'node-addon-api'"); fetched at their newest version.
    headers = [name for name in ("node-addon-api", "nan") if name in gyp and name not in declared]
    steps = [
        # The package's dependencies (header packages such as nan or
        # node-addon-api), without running anyone's install scripts.
        ["npm", "install", "--ignore-scripts", "--omit=dev", "--no-package-lock", "--no-save"] if declared else None,
        ["npm", "install", "--ignore-scripts", "--no-package-lock", "--no-save", *headers] if headers else None,
        ["npx", "--yes", NODE_GYP, "rebuild", f"--target={electron_version}", "--arch=x64", "--dist-url=https://electronjs.org/headers", f"--devdir={os.path.join(cache, 'builds', 'node-gyp')}", "--jobs=max"],
    ]
    for command in steps:
        if command is None:
            continue
        try:
            done = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, errors="replace", timeout=timeout)
        except subprocess.TimeoutExpired:
            shutil.rmtree(work, ignore_errors=True)
            return finish({}, f"{command[0] if command[0] == 'npm' else 'node-gyp'} took longer than {timeout} s")
        if done.returncode != 0:
            text = (done.stderr + "\n" + done.stdout).strip().splitlines()
            causes = [line.strip() for line in text if re.search(r"(?i)error|not found|no such|cannot find|fatal", line) and not line.startswith("gyp ERR!")]
            error = causes[0] if causes else next((line for line in text if re.search(r"error:|Error:|fatal", line)), text[-1] if text else "failed")
            log(f"      build of {name}@{version} failed: {' / '.join(causes[:5])[:800]}")
            shutil.rmtree(work, ignore_errors=True)
            return finish({}, f"{'npm install' if command[0] == 'npm' else 'node-gyp'} failed: {error.strip()[:160]}")
    files = {}
    release = os.path.join(work, "build", "Release")
    for root, _, names in os.walk(release):
        for item in names:
            if item.endswith(".node"):
                with open(os.path.join(root, item), "rb") as handle:
                    data = handle.read()
                if _elf_x64(data):
                    files[item] = data
    shutil.rmtree(work, ignore_errors=True)
    seconds = round(time.time() - started)
    if not files:
        return finish({}, f"node-gyp finished in {seconds} s but built no Linux .node (a target for another platform only)")
    return finish(files, f"built from the npm source with {NODE_GYP} against Electron {electron_version}'s headers in {seconds} s: {', '.join(sorted(files))}")

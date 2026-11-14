#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The layer store and the sandbox of the layers experiment (a prototype;
nothing here is part of tur-runtime).

A store on one file system:

    bases/<id>/root/        a Wine prefix without its drive C: (registry files,
                            dosdevices, .update-timestamp, an empty drive_c)
    bases/<id>/c/           that prefix's drive C:
    bases/<id>/home/        the HOME the base was made with (not shared)
    bases/<id>/base.json
    layers/<id>/root/       the writable layer over a base's root/
    layers/<id>/c/          the writable layer over drive C:
    layers/<id>/work-root/  overlayfs work directories (same file system)
    layers/<id>/work-c/
    layers/<id>/home/       the private HOME, the person's data for this layer
    layers/<id>/regdelta/   what the layer changed in each registry file
    layers/<id>/layer.json  what lies below it, bottom first, and its grants
    plain/<id>/prefix/      an ordinary prefix, as tur-runtime makes today
    plain/<id>/home/

Drive C: is a second overlay, mounted on the first one's empty drive_c: a
case-folded drive C: cannot be a subdirectory of an overlay whose root is not
case-folded, and this way both cases use one layout.

Inside the sandbox every application sees the same two paths, whatever lies
under them: /tur/prefix and /tur/home. A base is therefore the same bytes for
every application: no path in it names an application.

The sandbox is tur-runtime's (services/tur-runtime/src/sandbox.rs), written
out again here because the prototype may not change the service: the same
namespaces, the same read-only system, a private /tmp, one X socket. The one
difference is how the prefix gets there: `--bind` for a plain prefix, and

    --overlay-src BASE/root [--overlay-src LOWER/root ...]
    --overlay LAYER/root LAYER/work-root /tur/prefix
    --overlay-src BASE/c [--overlay-src LOWER/c ...]
    --overlay LAYER/c LAYER/work-c /tur/prefix/drive_c

for a layered one. bubblewrap (0.11 or later, not setuid) mounts overlayfs
itself, inside the user namespace it has just made, with the `userxattr`
option; no privilege and no helper is involved.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import pwd
import re
import shutil
import stat
import subprocess
import time

import winereg

SANDBOX_PREFIX = "/tur/prefix"
SANDBOX_HOME = "/tur/home"
REG_FILES = ("system.reg", "user.reg", "userdef.reg")
XDG_DIRS = {"DESKTOP": "Desktop", "DOCUMENTS": "Documents", "DOWNLOAD": "Downloads", "MUSIC": "Music",
            "PICTURES": "Pictures", "VIDEOS": "Videos", "TEMPLATES": "Templates"}
SYS_DIRECTORIES = ("/sys/block", "/sys/bus", "/sys/class", "/sys/dev", "/sys/devices")
SUPERVISE = ('"$@"; status=$?; echo "tur-supervise: exit $status at $(date +%s.%N)" >&2; '
             'wineserver --wait; exit $status')
SUPERVISE_LINE = re.compile(r"tur-supervise: exit (\d+) at ([0-9.]+)")


# ------------------------------------------------------------------ the trees


def rmtree(path: pathlib.Path) -> None:
    """Deletes a tree, including overlayfs's work/work (mode 000)."""
    if not path.exists() and not path.is_symlink():
        return
    try:
        shutil.rmtree(path)
        return
    except OSError:
        pass
    # Something in it cannot be listed: give every directory back to its
    # owner, top down, and delete again.
    for directory, names, _files in os.walk(path):
        for name in names:
            try:
                os.chmod(os.path.join(directory, name), 0o700)
            except OSError:
                pass
    shutil.rmtree(path)


def walk(root: pathlib.Path):
    """Every entry under root: (relative path, lstat), sorted, links not followed."""
    stack = [""]
    while stack:
        relative = stack.pop()
        directory = root / relative if relative else root
        try:
            names = sorted(os.listdir(directory))
        except OSError:
            continue
        for name in names:
            child = f"{relative}/{name}" if relative else name
            info = os.lstat(root / child)
            yield child, info
            if stat.S_ISDIR(info.st_mode):
                stack.append(child)


def file_sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def tree_hash(roots: list[pathlib.Path]) -> dict:
    """A digest of trees by content: every path, its type, its permissions,
    a link's target, a file's bytes. `strict` also covers modification times."""
    content, strict = hashlib.sha256(), hashlib.sha256()
    files = size = 0
    for root in roots:
        for relative, info in sorted(walk(root)):
            kind = stat.S_IFMT(info.st_mode)
            line = f"{root.name}/{relative}\0{kind:o}\0{stat.S_IMODE(info.st_mode):o}\0"
            if stat.S_ISLNK(info.st_mode):
                line += os.readlink(root / relative)
            elif stat.S_ISREG(info.st_mode):
                line += file_sha256(root / relative)
                files += 1
                size += info.st_size
            elif stat.S_ISCHR(info.st_mode):
                line += f"chr:{info.st_rdev}"
            content.update(line.encode("utf-8", errors="surrogateescape") + b"\n")
            strict.update(f"{line}\0{info.st_mtime_ns}\n".encode("utf-8", errors="surrogateescape"))
    return {"sha256": content.hexdigest(), "strict_sha256": strict.hexdigest(), "files": files, "bytes": size}


def disk_use(roots: list[pathlib.Path]) -> dict:
    """Bytes as files say (apparent) and as the file system allocated them
    (512-byte blocks), every inode once; and how many of each kind."""
    seen: set[tuple[int, int]] = set()
    apparent = allocated = files = directories = whiteouts = links = 0
    for root in roots:
        if not root.exists():
            continue
        entries = [("", os.lstat(root))] + list(walk(root))
        for _relative, info in entries:
            identity = (info.st_dev, info.st_ino)
            if identity in seen:
                continue
            seen.add(identity)
            allocated += info.st_blocks * 512
            if stat.S_ISREG(info.st_mode):
                apparent += info.st_size
                files += 1
            elif stat.S_ISDIR(info.st_mode):
                directories += 1
            elif stat.S_ISLNK(info.st_mode):
                links += 1
            elif stat.S_ISCHR(info.st_mode) and info.st_rdev == 0:
                whiteouts += 1
    return {"apparent_bytes": apparent, "allocated_bytes": allocated, "files": files,
            "directories": directories, "symlinks": links, "whiteouts": whiteouts}


def command(arguments: list[str], timeout: int = 120, env: dict | None = None) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(arguments, capture_output=True, text=True, errors="replace", timeout=timeout,
                              env=env, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired as error:
        return subprocess.CompletedProcess(arguments, 124, str(error.stdout or ""), f"timed out after {timeout}s")
    except OSError as error:
        return subprocess.CompletedProcess(arguments, 127, "", str(error))


def casefolded(directory: pathlib.Path) -> bool | None:
    done = command(["lsattr", "-d", str(directory)], env={"LC_ALL": "C", "PATH": "/usr/bin:/bin:/usr/sbin"})
    if done.returncode != 0 or not done.stdout.strip():
        return None
    return "F" in done.stdout.split()[0]


def set_casefold(directory: pathlib.Path) -> tuple[bool, str]:
    done = command(["chattr", "+F", str(directory)], env={"LC_ALL": "C", "PATH": "/usr/bin:/bin:/usr/sbin"})
    return done.returncode == 0, (done.stderr or done.stdout).strip()


def host_links() -> list[str]:
    """/bin, /lib and so on, as tur-runtime recreates them."""
    arguments: list[str] = []
    for path in ("/bin", "/sbin", "/lib", "/lib64", "/lib32", "/libx32"):
        if os.path.islink(path):
            arguments += ["--symlink", os.readlink(path), path]
        elif os.path.isdir(path):
            arguments += ["--ro-bind", path, path]
    return arguments


def ntsync_usable() -> bool:
    try:
        if not stat.S_ISCHR(os.stat("/dev/ntsync").st_mode):
            return False
        os.close(os.open("/dev/ntsync", os.O_RDWR | os.O_CLOEXEC))
        return True
    except OSError:
        return False


# ------------------------------------------------------------------ the store


class Store:
    def __init__(self, root: pathlib.Path, display: str | None = None, logs: pathlib.Path | None = None) -> None:
        self.root = pathlib.Path(root)
        self.display = display
        self.logs = logs
        self.user = pwd.getpwuid(os.getuid()).pw_name
        self.root.mkdir(parents=True, exist_ok=True)
        self.settings_path = self.root / "settings.json"
        self.settings = {"overlay_src_bottom_first": True, "disable_userns": True, "ntsync": False}
        if self.settings_path.is_file():
            self.settings.update(json.loads(self.settings_path.read_text()))
        self.counter = 0

    def save_settings(self) -> None:
        self.settings_path.write_text(json.dumps(self.settings, indent=2))

    # ---------------------------------------------------------------- paths

    def base(self, ident: str) -> pathlib.Path:
        return self.root / "bases" / ident

    def layer(self, ident: str) -> pathlib.Path:
        return self.root / "layers" / ident

    def plain(self, ident: str) -> pathlib.Path:
        return self.root / "plain" / ident

    def meta(self, ident: str) -> dict:
        return json.loads((self.layer(ident) / "layer.json").read_text())

    def save_meta(self, ident: str, meta: dict) -> None:
        path = self.layer(ident) / "layer.json"
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(meta, indent=2))
        os.replace(temporary, path)

    def plain_meta(self, ident: str) -> dict:
        path = self.plain(ident) / "plain.json"
        return json.loads(path.read_text()) if path.is_file() else {"grants": []}

    def save_plain_meta(self, ident: str, meta: dict) -> None:
        (self.plain(ident) / "plain.json").write_text(json.dumps(meta, indent=2))

    def lower_dirs(self, lowers: list[str]) -> list[pathlib.Path]:
        """The directories of a list of "base:<id>" and "layer:<id>", bottom first."""
        found = []
        for lower in lowers:
            kind, _, ident = lower.partition(":")
            found.append(self.base(ident) if kind == "base" else self.layer(ident))
        return found

    # -------------------------------------------------------------- sandbox

    def view(self, kind: str, ident: str) -> list[str]:
        """The bubblewrap arguments that put a prefix and a home in place."""
        if kind == "plain":
            directory = self.plain(ident)
            return ["--bind", str(directory / "prefix"), SANDBOX_PREFIX,
                    "--bind", str(directory / "home"), SANDBOX_HOME]
        if kind == "base":      # only while a base is made or updated
            directory = self.base(ident)
            return ["--bind", str(directory / "root"), SANDBOX_PREFIX,
                    "--bind", str(directory / "c"), SANDBOX_PREFIX + "/drive_c",
                    "--bind", str(directory / "home"), SANDBOX_HOME]
        directory = self.layer(ident)
        lowers = self.lower_dirs(self.meta(ident)["lowers"])
        if not self.settings["overlay_src_bottom_first"]:
            lowers = list(reversed(lowers))
        arguments: list[str] = []
        for part, work, target in (("root", "work-root", SANDBOX_PREFIX),
                                   ("c", "work-c", SANDBOX_PREFIX + "/drive_c")):
            for lower in lowers:
                arguments += ["--overlay-src", str(lower / part)]
            arguments += ["--overlay", str(directory / part), str(directory / work), target]
        return arguments + ["--bind", str(directory / "home"), SANDBOX_HOME]

    def grants(self, kind: str, ident: str) -> list[dict]:
        if kind == "layer":
            return self.meta(ident).get("grants", [])
        if kind == "plain":
            return self.plain_meta(ident).get("grants", [])
        return []

    def grant(self, kind: str, ident: str, directory: pathlib.Path, write: bool = True) -> None:
        entry = {"path": str(directory), "access": "read-write" if write else "read-only"}
        if kind == "layer":
            meta = self.meta(ident)
            if entry not in meta.setdefault("grants", []):
                meta["grants"].append(entry)
            self.save_meta(ident, meta)
        else:
            meta = self.plain_meta(ident)
            if entry not in meta["grants"]:
                meta["grants"].append(entry)
            self.save_plain_meta(ident, meta)

    def sandbox(self, kind: str, ident: str, files: tuple[str, ...] = (), chdir: str | None = None,
                network: bool = True) -> list[str]:
        """The whole bubblewrap command line, up to and including `--`."""
        arguments = ["bwrap", "--unshare-user", "--unshare-ipc", "--unshare-pid", "--unshare-uts",
                     "--unshare-cgroup-try"]
        if not network:
            arguments.append("--unshare-net")
        if self.settings["disable_userns"]:
            arguments.append("--disable-userns")
        arguments += ["--die-with-parent", "--new-session", "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc"]
        arguments += host_links()
        arguments += ["--proc", "/proc", "--dev", "/dev"]
        if self.settings["ntsync"]:
            arguments += ["--dev-bind", "/dev/ntsync", "/dev/ntsync"]
        for directory in SYS_DIRECTORIES:
            arguments += ["--ro-bind-try", directory, directory]
        arguments += ["--tmpfs", "/tmp"]
        if self.display:
            socket = f"/tmp/.X11-unix/X{self.display.lstrip(':').split('.')[0]}"
            if os.path.exists(socket):
                arguments += ["--ro-bind", socket, socket]
        arguments += self.view(kind, ident)
        for file in files:
            arguments += ["--ro-bind", file, file]
        for entry in self.grants(kind, ident):
            option = "--bind" if entry["access"] == "read-write" else "--ro-bind"
            arguments += [option, entry["path"], entry["path"]]
        arguments += ["--remount-ro", "/", "--chdir", chdir or SANDBOX_PREFIX, "--"]
        return arguments

    def environment(self, creating: bool = False, extra: dict | None = None) -> dict:
        env = {"PATH": "/usr/bin:/bin", "HOME": SANDBOX_HOME, "USER": self.user, "LOGNAME": self.user,
               "LANG": "C.UTF-8", "WINEPREFIX": SANDBOX_PREFIX, "WINEDEBUG": "fixme-all",
               "WINEDLLOVERRIDES": "winemenubuilder.exe="}
        if creating:
            env["WINEARCH"] = "win64"
            env["WINEDLLOVERRIDES"] = "mscoree,mshtml=;winemenubuilder.exe="
        if self.display:
            env["DISPLAY"] = self.display
        env.update(extra or {})
        return env

    def run(self, kind: str, ident: str, argv: list[str], *, wine: bool = True, timeout: int = 900,
            creating: bool = False, files: tuple[str, ...] = (), chdir: str | None = None,
            label: str | None = None, env: dict | None = None) -> dict:
        """Runs a command in the sandbox of a prefix. With `wine`, under the
        supervisor: the command, then `wineserver --wait`, so that the
        registry is on disk when this returns."""
        full = self.sandbox(kind, ident, files=files, chdir=chdir)
        full += ["/bin/sh", "-c", SUPERVISE, "tur-supervise", *argv] if wine else argv
        started_wall, started = time.time(), time.monotonic()
        done = command(full, timeout=timeout, env=self.environment(creating, env))
        seconds = time.monotonic() - started
        result = {"argv": argv, "exit": done.returncode, "seconds": round(seconds, 3),
                  "stdout": done.stdout, "stderr": done.stderr}
        match = SUPERVISE_LINE.search(done.stderr or "")
        if match:
            result["program_seconds"] = round(float(match.group(2)) - started_wall, 3)
        if self.logs is not None:
            self.counter += 1
            self.logs.mkdir(parents=True, exist_ok=True)
            name = re.sub(r"[^A-Za-z0-9._-]+", "-", label or f"{kind}-{ident}")[:80]
            (self.logs / f"{self.counter:04d}-{name}.log").write_text(
                "$ " + " ".join(full) + f"\nexit {done.returncode} after {seconds:.2f} s\n--- stdout\n"
                + (done.stdout or "")[-200_000:] + "\n--- stderr\n" + (done.stderr or "")[-200_000:])
        return result

    # ------------------------------------------------------- making prefixes

    def make_home(self, home: pathlib.Path) -> None:
        """A private HOME with the directories Wine links the profile's
        folders to, so that documents live outside the layer."""
        (home / ".config").mkdir(parents=True, exist_ok=True)
        lines = []
        for key, name in XDG_DIRS.items():
            (home / name).mkdir(exist_ok=True)
            lines.append(f'XDG_{key}_DIR="$HOME/{name}"')
        (home / ".config" / "user-dirs.dirs").write_text("\n".join(lines) + "\n")

    def create_plain(self, ident: str, casefold: bool = False) -> dict:
        """A prefix as tur-runtime makes one today: wineboot --init, then
        Wine's own updating switched off."""
        directory = self.plain(ident)
        rmtree(directory)
        started = time.monotonic()
        (directory / "prefix").mkdir(parents=True)
        self.make_home(directory / "home")
        result: dict = {"id": ident, "casefold_asked": casefold}
        if casefold:
            (directory / "prefix" / "drive_c").mkdir()
            result["casefold_set"], result["casefold_message"] = set_casefold(directory / "prefix" / "drive_c")
        self.save_plain_meta(ident, {"grants": []})
        boot = self.run("plain", ident, ["wine", "wineboot", "--init"], creating=True, timeout=900,
                        label=f"create-plain-{ident}")
        (directory / "prefix" / ".update-timestamp").write_text("disable\n")
        result.update(seconds=round(time.monotonic() - started, 2), wineboot_exit=boot["exit"],
                      wineboot_seconds=boot["seconds"], program_seconds=boot.get("program_seconds"),
                      ok=boot["exit"] == 0 and (directory / "prefix" / "system.reg").is_file(),
                      disk=disk_use([directory / "prefix"]),
                      casefolded=casefolded(directory / "prefix" / "drive_c"))
        if not result["ok"]:
            result["stderr"] = boot["stderr"][-3000:]
        return result

    def create_base(self, ident: str, casefold: bool = False, settle: bool = True) -> dict:
        """A base layer: a prefix made by wineboot --init with drive C: beside
        it, Wine's updating switched off, started once more so that what Wine
        writes on a first start is in the base, then described."""
        directory = self.base(ident)
        rmtree(directory)
        started = time.monotonic()
        (directory / "root" / "drive_c").mkdir(parents=True)
        (directory / "c").mkdir()
        self.make_home(directory / "home")
        result: dict = {"id": ident, "casefold_asked": casefold}
        if casefold:
            result["casefold_set"], result["casefold_message"] = set_casefold(directory / "c")
        boot = self.run("base", ident, ["wine", "wineboot", "--init"], creating=True, timeout=900,
                        label=f"create-base-{ident}")
        (directory / "root" / ".update-timestamp").write_text("disable\n")
        result.update(wineboot_exit=boot["exit"], wineboot_seconds=boot["seconds"],
                      create_seconds=round(time.monotonic() - started, 2))
        if settle:
            again = self.run("base", ident, ["wine", "cmd", "/c", "exit"], timeout=300, label=f"settle-base-{ident}")
            result["settle_exit"], result["settle_seconds"] = again["exit"], again["seconds"]
        result["ok"] = boot["exit"] == 0 and (directory / "root" / "system.reg").is_file()
        if not result["ok"]:
            result["stderr"] = boot["stderr"][-3000:]
        result.update(self.describe_base(ident))
        return result

    def describe_base(self, ident: str, wine_version: str | None = None) -> dict:
        directory = self.base(ident)
        described = {"hash": tree_hash([directory / "root", directory / "c"]),
                     "disk": disk_use([directory / "root", directory / "c"]),
                     "casefolded": casefolded(directory / "c"),
                     "registry_bytes": {name: (directory / "root" / name).stat().st_size
                                        for name in REG_FILES if (directory / "root" / name).is_file()},
                     "profile_links": {relative: os.readlink(directory / "c" / relative)
                                       for relative, info in walk(directory / "c")
                                       if stat.S_ISLNK(info.st_mode) and relative.startswith("users/")}}
        record = {"id": ident, "wine": wine_version, **described}
        (directory / "base.json").write_text(json.dumps(record, indent=2))
        return described

    def verify_base(self, ident: str) -> dict:
        """Hashes a base again and compares with what was recorded."""
        directory = self.base(ident)
        recorded = json.loads((directory / "base.json").read_text())["hash"]
        now = tree_hash([directory / "root", directory / "c"])
        return {"base": ident, "unchanged": now["sha256"] == recorded["sha256"],
                "times_unchanged": now["strict_sha256"] == recorded["strict_sha256"],
                "sha256": now["sha256"], "recorded_sha256": recorded["sha256"], "files": now["files"]}

    def create_layer(self, ident: str, lowers: list[str], fold_work: bool | None = None) -> dict:
        """An application's layer: four empty directories, a home, a record.
        Over a case-folded drive C: the layer's drive C: is case-folded too
        (overlayfs wants every layer to agree), and its work directory as
        `fold_work` says (by default like the layer)."""
        directory = self.layer(ident)
        rmtree(directory)
        started = time.monotonic()
        for name in ("root", "c", "work-root", "work-c", "regdelta"):
            (directory / name).mkdir(parents=True)
        self.make_home(directory / "home")
        meta = {"id": ident, "lowers": lowers, "grants": [], "casefold": False}
        bottom = self.lower_dirs(lowers)[0]
        result: dict = {"id": ident, "lowers": lowers}
        if casefolded(bottom / "c"):
            meta["casefold"] = True
            result["casefold_set"] = set_casefold(directory / "c")
            if self.settings.get("fold_work", True) if fold_work is None else fold_work:
                result["casefold_work_set"] = set_casefold(directory / "work-c")
        self.save_meta(ident, meta)
        result["seconds"] = round(time.monotonic() - started, 4)
        result["disk"] = disk_use([directory])
        return result

    def drop_layer(self, ident: str) -> dict:
        started = time.monotonic()
        rmtree(self.layer(ident))
        return {"seconds": round(time.monotonic() - started, 3), "gone": not self.layer(ident).exists()}

    def layer_disk(self, ident: str) -> dict:
        directory = self.layer(ident)
        return disk_use([directory / "root", directory / "c"])

    # ------------------------------------------------------------- registry

    def lower_registry(self, lowers: list[str]) -> dict[str, winereg.Registry]:
        """The registry of everything below a layer: the base's files, with
        the recorded changes of each lower layer replayed in order."""
        directories = self.lower_dirs(lowers)
        registries = {}
        for name in REG_FILES:
            path = directories[0] / "root" / name
            registry = winereg.Registry.load(path) if path.is_file() else winereg.Registry()
            for directory in directories[1:]:
                delta = directory / "regdelta" / f"{name}.json"
                if delta.is_file():
                    registry, _report = winereg.apply(registry, json.loads(delta.read_text()))
            registries[name] = registry
        return registries

    def capture(self, ident: str) -> dict:
        """Records what a layer changed in the registry, relative to what
        lies below it now. To be called before what lies below changes."""
        directory, meta = self.layer(ident), self.meta(ident)
        started = time.monotonic()
        below = self.lower_registry(meta["lowers"])
        report: dict = {"files": {}}
        for name in REG_FILES:
            path = directory / "root" / name
            if path.is_file():
                mine = winereg.Registry.load(path)
                delta = winereg.diff(below[name], mine)
                report["files"][name] = {"copied_up": True, "bytes": path.stat().st_size,
                                         "warnings": mine.warnings[:5], **winereg.delta_size(delta)}
            else:
                delta = {"format": 1, "ops": []}
                report["files"][name] = {"copied_up": False}
            text = json.dumps(delta)
            (directory / "regdelta" / f"{name}.json").write_text(text)
            report["files"][name]["delta_bytes"] = len(text)
        meta["captured_against"] = meta["lowers"]
        self.save_meta(ident, meta)
        report["seconds"] = round(time.monotonic() - started, 3)
        return report

    def materialise(self, ident: str, new_lowers: list[str], noise: dict | None = None,
                    prefer: str = "mine") -> dict:
        """The registry rebase: replays the layer's recorded changes on the
        registry of `new_lowers` and writes the result into the layer."""
        directory, meta = self.layer(ident), self.meta(ident)
        started = time.monotonic()
        below = self.lower_registry(new_lowers)
        report: dict = {"from": meta["lowers"], "to": new_lowers, "files": {}}
        for name in REG_FILES:
            recorded = directory / "regdelta" / f"{name}.json"
            # A layer that never ran has recorded nothing: it changed nothing.
            delta = json.loads(recorded.read_text()) if recorded.is_file() else {"format": 1, "ops": []}
            target = directory / "root" / name
            result, applied = winereg.apply(below[name], delta, (noise or {}).get(name), prefer)
            entry = {"ops": len(delta["ops"]), "applied": applied["applied"], "already": applied["already"],
                     "noise_skipped": applied["noise_skipped"], "conflicts": applied["conflicts"]}
            if winereg.same(result, below[name]) and len(new_lowers) == 1:
                # Nothing of the layer's own: let the base's file show through.
                if target.exists():
                    target.unlink()
                entry["written"] = False
            else:
                result.save(target)
                entry["written"] = True
                entry["bytes"] = target.stat().st_size
            report["files"][name] = entry
        meta["lowers"] = new_lowers
        self.save_meta(ident, meta)
        report["conflicts"] = sum(len(entry["conflicts"]) for entry in report["files"].values())
        report["seconds"] = round(time.monotonic() - started, 3)
        return report

    def rebase_files(self, ident: str, old_base: str, new_base: str) -> dict:
        """The file half of a rebase. A file in the layer that is byte for
        byte the old base's file is only there because something opened or
        touched it: it would hide the new base's file, so it is dropped. A
        file that differs is the application's doing and is kept."""
        directory = self.layer(ident)
        started = time.monotonic()
        report = {"stale_dropped": [], "stale_bytes": 0, "kept_modified": [], "both_changed": [],
                  "hides_new_base_file": [], "whiteouts": [], "opaque_directories": []}
        for part in ("root", "c"):
            old_root, new_root = self.base(old_base) / part, self.base(new_base) / part
            for relative, info in list(walk(directory / part)):
                path = directory / part / relative
                shown = f"{part}/{relative}"
                if stat.S_ISCHR(info.st_mode) and info.st_rdev == 0:
                    report["whiteouts"].append(shown)
                    continue
                if stat.S_ISDIR(info.st_mode):
                    try:
                        if os.getxattr(path, "user.overlay.opaque") == b"y":
                            report["opaque_directories"].append(shown)
                    except OSError:
                        pass
                    continue
                if not stat.S_ISREG(info.st_mode) or (part == "root" and relative in REG_FILES):
                    continue
                old = old_root / relative
                if not old.is_file() or old.is_symlink():
                    # The layer's own file. Does the new base bring one of
                    # the same name, which this one would hide?
                    if (new_root / relative).is_file():
                        report["hides_new_base_file"].append(shown)
                    continue
                if old.stat().st_size == info.st_size and file_sha256(old) == file_sha256(path):
                    path.unlink()
                    report["stale_dropped"].append(shown)
                    report["stale_bytes"] += info.st_size
                    continue
                new = new_root / relative
                if new.is_file() and (new.stat().st_size != old.stat().st_size
                                      or file_sha256(new) != file_sha256(old)):
                    report["both_changed"].append(shown)
                else:
                    report["kept_modified"].append(shown)
        report["seconds"] = round(time.monotonic() - started, 3)
        return report

    # ---------------------------------------------------- snapshots, resets

    def snapshot(self, ident: str, name: str) -> dict:
        """A copy of the layer's two trees and its registry record, sharing
        blocks where the file system can."""
        directory = self.layer(ident)
        target = directory / "snapshots" / name
        rmtree(target)
        target.mkdir(parents=True)
        started = time.monotonic()
        done = command(["cp", "-a", "--reflink=auto", str(directory / "root"), str(directory / "c"),
                        str(directory / "regdelta"), str(directory / "layer.json"), str(target)], timeout=600)
        return {"name": name, "exit": done.returncode, "stderr": done.stderr[-500:],
                "seconds": round(time.monotonic() - started, 3), "disk": disk_use([target])}

    def rollback(self, ident: str, name: str) -> dict:
        directory = self.layer(ident)
        source = directory / "snapshots" / name
        started = time.monotonic()
        for part in ("root", "c", "regdelta"):
            rmtree(directory / part)
            done = command(["cp", "-a", "--reflink=auto", str(source / part), str(directory / part)], timeout=600)
            if done.returncode != 0:
                return {"ok": False, "stderr": done.stderr[-500:]}
        shutil.copy2(source / "layer.json", directory / "layer.json")
        for work in ("work-root", "work-c"):
            rmtree(directory / work)
            (directory / work).mkdir()
            if work == "work-c" and casefolded(directory / "c"):
                set_casefold(directory / work)
        return {"ok": True, "seconds": round(time.monotonic() - started, 3)}

    def reset(self, ident: str) -> dict:
        """Factory reset: the layer's own files and registry go; its home
        (the person's documents) and its record stay."""
        directory = self.layer(ident)
        started = time.monotonic()
        folded = casefolded(directory / "c")
        for part in ("root", "c", "work-root", "work-c", "regdelta"):
            rmtree(directory / part)
            (directory / part).mkdir()
        if folded:
            set_casefold(directory / "c")
            set_casefold(directory / "work-c")
        return {"seconds": round(time.monotonic() - started, 3), "disk": self.layer_disk(ident)}

    # ----------------------------------------------------------- programs

    def find_program(self, kind: str, ident: str, name: str, beside: str | None = None) -> str | None:
        """Where a program of the application is, as a path inside the
        sandbox. Looked for in the layer and the layers below it, not in a
        base (that is Windows itself); nearest layer first, shortest path."""
        if kind == "plain":
            trees = [self.plain(ident) / "prefix" / "drive_c"]
        else:
            lowers = self.lower_dirs(self.meta(ident)["lowers"])[1:]
            trees = [self.layer(ident) / "c"] + [lower / "c" for lower in reversed(lowers)]
        wanted = name.lower()
        for tree in trees:
            found = sorted((relative for relative, info in walk(tree)
                            if relative.rsplit("/", 1)[-1].lower() == wanted and stat.S_ISREG(info.st_mode)
                            and not relative.lower().startswith("windows/")),
                           key=lambda relative: (relative.count("/"), len(relative)))
            if beside:
                near = [relative for relative in found
                        if f"{SANDBOX_PREFIX}/drive_c/{relative}".rsplit("/", 1)[0] == beside.rsplit("/", 1)[0]]
                found = near or found
            if found:
                return f"{SANDBOX_PREFIX}/drive_c/{found[0]}"
        return None

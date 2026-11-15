#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The layers experiment: Windows prefixes composed from a shared base layer
and thin application layers. Hypotheses and kill criteria are in README.md
beside this file; they were written before anything here ran.

Run by .github/workflows/exp-layers.yml, as an unprivileged user, inside a
Fedora container (and, for the kernel TUR ships, inside a virtual machine).
Never run on a development machine (AGENTS.md).

    experiment.py --work DIR --evidence DIR --apps apps.json --phases A,B,...

Every phase writes what it measured into EVIDENCE/results.json under its own
name, as it finishes; a phase that fails is recorded with its error and the
next one runs. Nothing is retried and nothing is hidden. The phases:

    download        fetch installers (for a machine without a network)
    preflight       versions; whether bubblewrap can mount overlayfs here
                    without privilege; which way --overlay-src stacks
    old-base        (old Wine) a base layer, the five applications installed
                    on layers over it and checked, what Wine itself changes
                    in a registry by merely running, a plain prefix to update
    h1              prefix against layer: time and bytes, on each file system
    update          (new Wine) the old base updated by Wine once; registry
                    differences between old, updated and fresh bases; Wine's
                    own update of a plain prefix and of a layered one
    rebase          the five layers rebased onto the updated and onto the
                    fresh base, started, checked, removed
    layered         every installer of apps.json on layers over the fresh
                    base: install, start, check
    plain           the same on plain prefixes, as tur-runtime makes them
    disk            ten applications: bytes layered and not; identical files
    groups          two layers composed and separated; a runtime layer
    safety          what an application can reach and change
    extras          snapshot and rollback, factory reset, trial layers
    remove-layered  uninstall and drop every layer; the bases hashed again
    mini            (virtual machine) two applications on layers, per file system
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import platform
import re
import shutil
import stat
import subprocess
import sys
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent.parent / "windows-runtime"))

import apps as rt  # noqa: E402  (tests/windows-runtime/apps.py: the display, the checks, the downloads)
import layerlib  # noqa: E402
import winereg  # noqa: E402
from layerlib import SANDBOX_PREFIX, Store  # noqa: E402

FIVE = ["7zip-exe", "notepadpp-nsis", "putty-msi", "innosetup", "vlc-nsis"]
UNINSTALL = ("software\\microsoft\\windows\\currentversion\\uninstall",
             "software\\wow6432node\\microsoft\\windows\\currentversion\\uninstall")
INSTALL_ARGUMENTS = {
    "nsis": ["/S"],
    "seven-zip-installer": ["/S"],
    "inno-setup": ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"],
}
UNINSTALL_ARGUMENTS = {
    "nsis": ["/S"],
    "seven-zip-installer": ["/S"],
    "inno-setup": ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"],
}


def log(text: str) -> None:
    print(text, flush=True)


def short(text: str, lines: int = 8) -> str:
    return "\n".join((text or "").strip().splitlines()[-lines:])


# ------------------------------------------------------------- the bench


class World:
    """What tests/windows-runtime/apps.py calls a world: where downloads,
    shared directories and evidence of one pass are."""

    def __init__(self, bench: "Bench", name: str) -> None:
        self.downloads = bench.work / "downloads"
        self.shared = bench.work / "shared" / name
        self.profiles = bench.work / "profiles" / name
        self.evidence = bench.evidence / "passes" / name
        for directory in (self.downloads, self.shared, self.profiles, self.evidence):
            directory.mkdir(parents=True, exist_ok=True)

    def env(self, display: bool = True) -> dict[str, str]:
        env = dict(os.environ)
        env.pop("WAYLAND_DISPLAY", None)
        env["DISPLAY"] = rt.DISPLAY
        return env

    def tur_json(self, arguments: list[str], timeout: int = 1200) -> tuple[int, dict, str]:
        """Stands in for `tur-runtime installer identify` in Inno Setup's
        check: only the Inno Setup loader's marker is looked for."""
        if arguments[:2] == ["installer", "identify"]:
            data = pathlib.Path(arguments[2]).read_bytes()
            kind = "inno-setup" if b"rDlPtS" in data else "unknown"
            return 0, {"installer": {"kind": kind}}, ""
        return 2, {}, "not available in the layers experiment"


class AppContext(rt.Context):
    """apps.py's checks, pointed at a prefix of this experiment."""

    def __init__(self, store: Store, world: World, app: dict, kind: str, ident: str, exe: str,
                 command: list[str], evidence: pathlib.Path) -> None:
        super().__init__(world, app, {"main_executable": exe, "profile": ""}, command,
                         exe.rsplit("/", 1)[1].lower(), evidence)
        self.store, self.kind, self.ident, self.exe = store, kind, ident, exe

    def grant(self) -> None:
        self.store.grant(self.kind, self.ident, self.shared)

    def tool(self, name: str) -> str:
        found = self.store.find_program(self.kind, self.ident, name, beside=self.exe)
        if not found:
            raise RuntimeError(f"{name} was not installed beside the main program")
        return found

    def run_tool(self, profile: str, arguments: list[str], timeout: int = 300) -> subprocess.CompletedProcess:
        done = self.store.run(self.kind, self.ident, ["wine", profile, *arguments], timeout=timeout,
                              chdir=profile.rsplit("/", 1)[0], label=f"tool-{self.ident}")
        return subprocess.CompletedProcess(arguments, done["exit"], done["stdout"], done["stderr"])


class Bench:
    def __init__(self, work: pathlib.Path, evidence: pathlib.Path, apps: list[dict], build: pathlib.Path | None,
                 wine_label: str) -> None:
        self.work, self.evidence, self.build = work, evidence, build
        self.apps = {app["id"]: app for app in apps}
        self.wine_label = wine_label
        self.results_path = evidence / "results.json"
        self.results = json.loads(self.results_path.read_text()) if self.results_path.is_file() else {}
        self.state_path = work / "state.json"
        self.state = json.loads(self.state_path.read_text()) if self.state_path.is_file() else {}
        self.stores: dict[str, Store] = {}
        self.display_ok = True

    def store(self, variant: str = "ext4") -> Store:
        if variant not in self.stores:
            root = self.work / "fs" / variant / "store"
            self.stores[variant] = Store(root, display=rt.DISPLAY, logs=self.evidence / "logs" / variant)
        return self.stores[variant]

    def record(self, name: str, data) -> None:
        self.results[name] = data
        temporary = self.results_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(self.results, indent=1, default=str))
        os.replace(temporary, self.results_path)

    def remember(self, **values) -> None:
        self.state.update(values)
        self.state_path.write_text(json.dumps(self.state, indent=1))

    def wine_version(self) -> str:
        return (rt.run(["wine", "--version"], timeout=60).stdout.strip().splitlines() or ["unknown"])[0]


# ------------------------------------------------------- one application


def value_string(text: str) -> str | None:
    """The string of a raw registry value line, or None when it is not one."""
    named = winereg.value_name(text)
    if named is None:
        return None
    data = text[named[1]:].lstrip(" \t=")
    for tag in ("str(2):", "str(7):", "str:"):
        if data.startswith(tag):
            data = data[len(tag):]
    if not data.startswith('"') or not data.rstrip().endswith('"'):
        return None
    units = winereg.unescape(data.strip()[1:-1])
    return "".join(chr(unit) for unit in units)


def uninstall_entries(before: dict[str, winereg.Registry], after: dict[str, winereg.Registry]) -> list[dict]:
    """The uninstall entries a registry has and another has not."""
    found = []
    for name in ("system.reg", "user.reg"):
        for ident, key in after[name].keys.items():
            path = "\\".join("".join(chr(unit) for unit in part) for part in ident)
            if not any(path.startswith(root + "\\") and path.count("\\") == root.count("\\") + 1
                       for root in UNINSTALL):
                continue
            if name in before and ident in before[name].keys:
                continue
            entry = {"file": name, "key": path, "key_name": key.name.rsplit("\\\\", 1)[-1]}
            for value in key.values.values():
                label = winereg.raw_value_name(value)
                if label.lower() in ("uninstallstring", "displayname", "quietuninstallstring"):
                    entry[label.lower()] = value_string(value)
                if label.lower() == "windowsinstaller":
                    entry["windowsinstaller"] = value.endswith("dword:00000001")
            found.append(entry)
    return found


def split_command(text: str) -> list[str]:
    """A Windows command line as program and arguments."""
    text = text.strip()
    if text.startswith('"'):
        end = text.find('"', 1)
        if end != -1:
            return [text[1:end]] + text[end + 1:].split()
    lowered = text.lower()
    at = lowered.find(".exe")
    if at != -1:
        return [text[:at + 4]] + text[at + 4:].split()
    return text.split()


def load_registries(directory: pathlib.Path) -> dict[str, winereg.Registry]:
    found = {}
    for name in ("system.reg", "user.reg"):
        if (directory / name).is_file():
            found[name] = winereg.Registry.load(directory / name)
    return found


class AppRun:
    """One application in one prefix of the experiment, plain or layered."""

    def __init__(self, bench: Bench, store: Store, world: World, app: dict, kind: str, ident: str) -> None:
        self.bench, self.store, self.world, self.app, self.kind, self.ident = bench, store, world, app, kind, ident
        self.evidence = world.evidence / ident
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.result: dict = {"id": app["id"], "prefix": ident, "kind": kind, "name": app["name"],
                             "version": app["version"], "installer_kind": app["expected_kind"]}
        for name in rt.STAGES:
            self.result[name] = {"status": "not-run"}

    def stage(self, name: str, status: str, **fields) -> None:
        self.result[name] = {"status": status, **fields}
        detail = fields.get("detail")
        log(f"{status.upper():8} {self.ident}: {name}" + (f"\n         {detail}" if detail else ""))

    def prefix_root(self) -> pathlib.Path:
        if self.kind == "plain":
            return self.store.plain(self.ident) / "prefix"
        return self.store.layer(self.ident) / "root"

    def own_disk(self) -> dict:
        if self.kind == "plain":
            return layerlib.disk_use([self.store.plain(self.ident) / "prefix"])
        return self.store.layer_disk(self.ident)

    def exe(self) -> str | None:
        return self.store.find_program(self.kind, self.ident, self.app["main_executable"])

    def command(self, exe: str) -> list[str]:
        return [sys.executable, str(HERE / "lrun.py"), "--store", str(self.store.root), "--kind", self.kind,
                "--id", self.ident, "--exe", exe, "--display", rt.DISPLAY]

    def install(self) -> bool:
        app, kind = self.app, self.app["expected_kind"]
        download: dict = {}
        installer = rt.download(self.world, app, download)
        self.result["download"] = download.get("download")
        if installer is None:
            self.stage("install", "not-run", detail="the installer could not be downloaded or its hash differs")
            return False
        if self.kind == "plain":
            # What the registry held before, to find the uninstall entry later.
            for name in ("system.reg", "user.reg"):
                source = self.prefix_root() / name
                if source.is_file():
                    shutil.copy2(source, self.store.plain(self.ident) / f"before-{name}")
        target = f"{SANDBOX_PREFIX}/drive_c/Program Files/{self.ident}"
        files, wine = (str(installer),), True
        if kind == "msi":
            argv = ["wine", "msiexec", "/i", rt.windows_path(installer), "/qn"]
        elif kind in INSTALL_ARGUMENTS:
            argv = ["wine", str(installer), *INSTALL_ARGUMENTS[kind]]
        elif kind == "portable-zip":
            argv, wine = ["unzip", "-q", "-o", str(installer), "-d", target], False
        elif kind == "plain-exe":
            argv, wine = ["install", "-D", "-m", "0644", "--", str(installer), f"{target}/{installer.name}"], False
        else:
            self.stage("install", "fail", detail=f"no unattended mode for {kind}")
            return False
        before = self.own_disk()
        done = self.store.run(self.kind, self.ident, argv, wine=wine, files=files, timeout=480,
                              label=f"install-{self.ident}")
        after = self.own_disk()
        exe = self.exe()
        ok = done["exit"] == 0 and exe is not None
        self.stage("install", "pass" if ok else "fail", seconds=done["seconds"],
                   program_seconds=done.get("program_seconds"), exit=done["exit"], main_executable=exe,
                   bytes_before=before["apparent_bytes"], bytes_after=after["apparent_bytes"],
                   allocated_after=after["allocated_bytes"], files_after=after["files"],
                   whiteouts=after["whiteouts"],
                   detail=None if ok else f"exit {done['exit']}, main program {exe}: {short(done['stderr'])}")
        return ok

    def launch(self) -> None:
        exe = self.exe()
        if exe is None:
            self.stage("launch", "fail", detail="the main program is not there")
            return
        if not self.bench.display_ok:
            self.stage("launch", "not-run", detail="the virtual display or the tools that look at it are missing")
            return
        klass, wanted = exe.rsplit("/", 1)[1].lower(), self.app.get("window_title")
        try:
            launch = rt.Launch(self.world, self.command(exe), self.app.get("launch_arguments", []),
                               self.evidence / "launch.txt")
            window, seconds = launch.wait_for_window(klass, None, 120)
            if window is None:
                seen = [found.describe() for found in launch.new_windows()]
                exited = launch.process.poll()
                launch.kill()
                self.stage("launch", "fail", seconds=round(seconds, 1),
                           detail=f"no window of class {klass!r} within 120 s (exit status so far: {exited}); "
                                  f"windows seen: {seen}; output: "
                                  f"{short(launch.log_path.read_text(errors='replace'), 4)}")
                return
            time.sleep(3)
            own = [found for found in launch.new_windows()
                   if klass in (found.instance.lower(), found.klass.lower())
                   and found.width >= 120 and found.height >= 60 and rt.is_viewable(found)]
            titled = [found for found in own if wanted and re.search(wanted, found.title)]
            largest = sorted(own, key=lambda found: found.width * found.height, reverse=True)
            shown = (titled or largest or [window])[0]
            rt.screenshot(self.evidence / "launch.png")
            ended = launch.close(klass)
            ok = not wanted or bool(titled)
            self.stage("launch", "pass" if ok else "fail", seconds_to_window=round(seconds, 2),
                       window=shown.describe(), closed=ended,
                       detail=None if ok else f"no window titled like {wanted!r}: {[w.title for w in own]}")
        except Exception as error:  # noqa: BLE001 - a harness error is a result
            self.stage("launch", "fail", detail=f"harness error: {error}")
            traceback.print_exc()

    def check(self) -> None:
        exe = self.exe()
        if exe is None:
            self.stage("check", "fail", detail="the main program is not there")
            return
        check = rt.CHECKS[self.app["check"]["kind"]]
        if not self.bench.display_ok and self.app["check"]["kind"] in ("open-file-title", "putty-raw"):
            self.stage("check", "not-run", detail="this check looks at a window, and there is no display")
            return
        try:
            context = AppContext(self.store, self.world, self.app, self.kind, self.ident, exe,
                                 self.command(exe), self.evidence)
            started = time.monotonic()
            description = check(context)
            self.stage("check", "pass", seconds=round(time.monotonic() - started, 1), detail=description)
        except Exception as error:  # noqa: BLE001 - a failed check is a result
            self.stage("check", "fail", detail=str(error))
            if not isinstance(error, RuntimeError):
                traceback.print_exc()

    def uninstall(self) -> None:
        """The application's own uninstaller, then the prefix or layer is
        dropped, then a look for anything left."""
        kind = self.app["expected_kind"]
        ran: dict = {"ran": False}
        try:
            if self.kind == "plain":
                directory = self.store.plain(self.ident)
                before = {name: winereg.Registry.load(directory / f"before-{name}")
                          for name in ("system.reg", "user.reg") if (directory / f"before-{name}").is_file()}
                after = load_registries(directory / "prefix")
            else:
                before = self.store.lower_registry(self.store.meta(self.ident)["lowers"])
                after = dict(before)
                after.update(load_registries(self.store.layer(self.ident) / "root"))
            entries = uninstall_entries(before, after)
            ran["entries"] = entries
            argv = None
            for entry in entries:
                if entry.get("windowsinstaller") or (entry.get("uninstallstring") or "").lower().startswith("msiexec"):
                    argv = ["wine", "msiexec", "/x", entry["key_name"], "/qn"]
                elif entry.get("uninstallstring") and kind in UNINSTALL_ARGUMENTS:
                    argv = ["wine", *split_command(entry["uninstallstring"]), *UNINSTALL_ARGUMENTS[kind]]
                if argv:
                    break
            if argv:
                done = self.store.run(self.kind, self.ident, argv, timeout=600, label=f"uninstall-{self.ident}")
                ran.update(ran=True, argv=argv, exit=done["exit"], seconds=done["seconds"],
                           main_program_left=self.exe() is not None, left=self.own_disk())
        except Exception as error:  # noqa: BLE001
            ran["error"] = str(error)
            traceback.print_exc()
        started = time.monotonic()
        if self.kind == "plain":
            layerlib.rmtree(self.store.plain(self.ident))
            gone = not self.store.plain(self.ident).exists()
        else:
            gone = self.store.drop_layer(self.ident)["gone"]
        servers = rt.run(["pgrep", "-a", "wineserver"], timeout=10).stdout.strip()
        left = ([] if gone else ["its directory still exists"]) + ([f"a Wine server: {servers}"] if servers else [])
        status = "fail" if left else ("partial" if ran.get("ran") and ran.get("exit") != 0 else "pass")
        self.stage("uninstall", status, uninstaller=ran, drop_seconds=round(time.monotonic() - started, 3),
                   left=left, detail="; ".join(left) or None)


def run_pass(bench: Bench, name: str, store: Store, kind: str, ids: list[str], *, base: str | None = None,
             stages: tuple[str, ...] = rt.STAGES, prefix: str = "", existing: bool = False,
             casefold: bool = False) -> dict:
    """The stages for each application, in order. A layer or a plain prefix
    is created first unless `existing`."""
    world = World(bench, name)
    results: list[dict] = []
    bench.record(f"pass:{name}", {"kind": kind, "base": base, "wine": bench.wine_label, "apps": results})
    for app_id in ids:
        app = bench.apps[app_id]
        ident = f"{prefix}{app_id}"
        log(f"\n=== {name}: {app['name']} {app['version']} ({ident}, {kind}) ===")
        run = AppRun(bench, store, world, app, kind, ident)
        try:
            ok = True
            if not existing:
                if kind == "plain":
                    created = store.create_plain(ident, casefold=casefold)
                    ok = created["ok"]
                    run.result["created"] = {key: created.get(key) for key in
                                             ("seconds", "wineboot_seconds", "ok", "casefolded")}
                    run.result["created"]["bytes"] = created["disk"]["apparent_bytes"]
                else:
                    created = store.create_layer(ident, [f"base:{base}"])
                    run.result["created"] = {"seconds": created["seconds"],
                                             "bytes": created["disk"]["apparent_bytes"]}
            if ok and "install" in stages:
                ok = run.install()
            if ok and "launch" in stages:
                run.launch()
            if ok and "check" in stages:
                run.check()
            if ok:
                run.result["disk"] = run.own_disk()
            if "uninstall" in stages:
                run.uninstall()
        except Exception as error:  # noqa: BLE001 - one application must not stop the others
            traceback.print_exc()
            run.result["harness_error"] = str(error)
        results.append(run.result)
        bench.record(f"pass:{name}", {"kind": kind, "base": base, "wine": bench.wine_label, "apps": results})
    return bench.results[f"pass:{name}"]


# ------------------------------------------------------------ the phases


def phase_preflight(bench: Bench) -> dict:
    store = bench.store()
    result: dict = {"wine": bench.wine_version(), "kernel": platform.release(),
                    "bubblewrap": rt.run(["bwrap", "--version"]).stdout.strip(),
                    "bwrap_mode": stat.filemode(os.stat(shutil.which("bwrap") or "/usr/bin/bwrap").st_mode),
                    "uid": os.getuid(), "user": store.user, "python": platform.python_version()}
    for key in ("user.max_user_namespaces", "kernel.unprivileged_userns_clone",
                "kernel.apparmor_restrict_unprivileged_userns"):
        path = pathlib.Path("/proc/sys") / key.replace(".", "/")
        result[key] = path.read_text().strip() if path.is_file() else None
    result["selinux"] = rt.run(["getenforce"]).stdout.strip() or "not available"
    result["ntsync_usable"] = layerlib.ntsync_usable()
    store.settings["ntsync"] = result["ntsync_usable"]

    # Can bubblewrap build tur-runtime's sandbox here, with and without
    # --disable-userns? (A container may not let it write the limit.)
    scratch = store.root / "preflight"
    layerlib.rmtree(scratch)
    for name in ("a", "b", "upper", "work"):
        (scratch / name).mkdir(parents=True)
    (scratch / "a" / "which").write_text("first\n")
    (scratch / "b" / "which").write_text("second\n")
    (scratch / "a" / "only-a").write_text("a\n")
    base_arguments = ["--unshare-user", "--unshare-ipc", "--unshare-pid", "--unshare-uts", "--unshare-cgroup-try",
                      "--die-with-parent", "--new-session", "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc",
                      *layerlib.host_links(), "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]
    for disable in (True, False):
        done = rt.run(["bwrap", *base_arguments, *(["--disable-userns"] if disable else []),
                       "--remount-ro", "/", "--", "/usr/bin/true"], timeout=60)
        result[f"sandbox_{'with' if disable else 'without'}_disable_userns"] = {
            "exit": done.returncode, "stderr": short(done.stderr, 3)}
        if done.returncode == 0:
            store.settings["disable_userns"] = disable
            break
    extra = ["--disable-userns"] if store.settings["disable_userns"] else []
    done = rt.run(["bwrap", *base_arguments, *extra, "--overlay-src", str(scratch / "a"), "--overlay-src",
                   str(scratch / "b"), "--overlay", str(scratch / "upper"), str(scratch / "work"), "/merged",
                   "--remount-ro", "/", "--", "/bin/sh", "-c",
                   "cat /merged/which /merged/only-a && echo written > /merged/new && rm /merged/only-a "
                   "&& grep ' /merged ' /proc/self/mountinfo"], timeout=60)
    result["overlay_in_sandbox"] = {"exit": done.returncode, "stdout": done.stdout, "stderr": short(done.stderr, 5)}
    result["overlay_works_unprivileged"] = done.returncode == 0
    if done.returncode == 0:
        store.settings["overlay_src_bottom_first"] = done.stdout.splitlines()[0] == "second"
        result["overlay_src_bottom_first"] = store.settings["overlay_src_bottom_first"]
        result["upper_after"] = sorted(
            f"{relative} ({'whiteout' if stat.S_ISCHR(info.st_mode) else 'file'})"
            for relative, info in layerlib.walk(scratch / "upper"))
        result["lower_untouched"] = (scratch / "a" / "only-a").is_file() and not (scratch / "a" / "new").exists()
    store.save_settings()
    layerlib.rmtree(scratch)
    result["settings"] = store.settings
    return result


def propagate_settings(bench: Bench, variant: str) -> Store:
    """A store on another file system uses what the preflight found."""
    store = bench.store(variant)
    store.settings.update(bench.store().settings)
    store.save_settings()
    return store


def noise_of(store: Store, base: str, ident: str) -> dict:
    """What Wine changes in a registry by merely starting and stopping,
    twice, in an empty layer over a base."""
    created = store.create_layer(ident, [f"base:{base}"])
    runs = [store.run("layer", ident, ["wine", "cmd", "/c", "exit"], timeout=300, label=f"noise-{ident}")
            for _ in range(2)]
    captured = store.capture(ident)
    return {"created": created["seconds"], "runs": [{"exit": run["exit"], "seconds": run["seconds"],
                                                     "program_seconds": run.get("program_seconds")} for run in runs],
            "captured": captured, "disk": store.layer_disk(ident),
            "files": sorted(relative for part in ("root", "c")
                            for relative, info in layerlib.walk(store.layer(ident) / part)
                            if not stat.S_ISDIR(info.st_mode))[:200]}


def noise_sets(store: Store, idents: list[str]) -> dict:
    """The (key, value) pairs the noise layers touched, per registry file."""
    found: dict = {name: set() for name in layerlib.REG_FILES}
    for ident in idents:
        for name in layerlib.REG_FILES:
            path = store.layer(ident) / "regdelta" / f"{name}.json"
            if path.is_file():
                found[name] |= winereg.touched(json.loads(path.read_text()))
    return found


def fresh_noise(store: Store) -> dict:
    """The noise pairs of the new Wine over the fresh base, measured once."""
    if not store.layer("noise-fresh").is_dir():
        noise_of(store, "fresh", "noise-fresh")
    return noise_sets(store, ["noise-fresh"])


def noise_listing(store: Store, ident: str) -> dict:
    listing = {}
    for name in layerlib.REG_FILES:
        path = store.layer(ident) / "regdelta" / f"{name}.json"
        if path.is_file():
            delta = json.loads(path.read_text())
            listing[name] = [{"key": op["key"], "op": op["op"],
                              "values": [winereg.raw_value_name(text) for text, _old in op.get("set", [])]
                              + [f"-{value}" for value, _old in op.get("del", [])]} for op in delta["ops"]][:400]
    return listing


def phase_old_base(bench: Bench) -> dict:
    """Under the old Wine: the base, the five applications on layers, the
    noise, and one plain prefix with an application for Wine's own update."""
    store = bench.store()
    result: dict = {"wine": bench.wine_version()}
    result["base"] = store.create_base("old")
    store.describe_base("old", result["wine"])
    bench.record("old-base", result)
    result["roundtrip"] = {}
    for name in layerlib.REG_FILES:
        path = store.base("old") / "root" / name
        data = path.read_bytes().decode("utf-8", errors="surrogateescape")
        registry = winereg.Registry.parse(data)
        result["roundtrip"][name] = {"identical": registry.dump() == data, "bytes": len(data),
                                     **registry.stats(), "first_warnings": registry.warnings[:5]}
    result["noise"] = noise_of(store, "old", "noise-old")
    result["noise_listing"] = noise_listing(store, "noise-old")
    bench.record("old-base", result)
    run_pass(bench, "old-layered", store, "layer", FIVE, base="old", stages=("install", "launch", "check"),
             prefix="h3-")
    result["captured"] = {app: store.capture(f"h3-{app}") for app in FIVE if store.layer(f"h3-{app}").is_dir()}
    result["verify_base"] = store.verify_base("old")
    bench.record("old-base", result)
    # A plain prefix with 7-Zip in it, kept for Wine's own update.
    run_pass(bench, "old-plain", store, "plain", ["7zip-exe"], stages=("install", "launch", "check"), prefix="keep-")
    bench.remember(old_wine=result["wine"])
    return result


def registry_difference(store: Store, left: str, right: str) -> dict:
    found = {}
    for name in layerlib.REG_FILES:
        one, two = store.base(left) / "root" / name, store.base(right) / "root" / name
        if one.is_file() and two.is_file():
            found[name] = winereg.delta_size(winereg.diff(winereg.Registry.load(one), winereg.Registry.load(two)))
    return found


def file_difference(store: Store, left: str, right: str) -> dict:
    """How the files of two bases differ, by content."""
    def index(base: str) -> dict:
        return {f"{part}/{relative}": (info.st_size, base, part, relative)
                for part in ("root", "c") for relative, info in layerlib.walk(store.base(base) / part)
                if stat.S_ISREG(info.st_mode)}
    one, two = index(left), index(right)
    changed = changed_bytes = 0
    for name in set(one) & set(two):
        a, b = one[name], two[name]
        if a[0] != b[0] or layerlib.file_sha256(store.base(left) / a[2] / a[3]) != \
                layerlib.file_sha256(store.base(right) / b[2] / b[3]):
            changed += 1
            changed_bytes += b[0]
    return {"only_left": len(set(one) - set(two)), "only_right": len(set(two) - set(one)),
            "changed": changed, "changed_bytes": changed_bytes, "same": len(set(one) & set(two)) - changed}


def phase_update(bench: Bench) -> dict:
    """Under the new Wine: the old base updated once by Wine itself; Wine's
    update of a plain prefix (today's migration) and of a layered one."""
    store = bench.store()
    result: dict = {"wine": bench.wine_version(), "old_wine": bench.state.get("old_wine")}
    if not store.base("old").is_dir():
        raise RuntimeError("there is no old base: the old-base phase did not run")
    # 1. The base, updated once.
    layerlib.rmtree(store.base("upd"))
    started = time.monotonic()
    done = layerlib.command(["cp", "-a", str(store.base("old")), str(store.base("upd"))], timeout=900)
    result["copy_base"] = {"exit": done.returncode, "seconds": round(time.monotonic() - started, 2)}
    update = store.run("base", "upd", ["wine", "wineboot", "--update"], creating=True, timeout=1200,
                       label="update-base")
    settle = store.run("base", "upd", ["wine", "cmd", "/c", "exit"], timeout=300, label="settle-updated-base")
    result["update_base"] = {"exit": update["exit"], "seconds": update["seconds"],
                             "program_seconds": update.get("program_seconds"), "settle_exit": settle["exit"],
                             "timestamp_file": (store.base("upd") / "root" / ".update-timestamp").read_text().strip(),
                             "stderr": short(update["stderr"], 6)}
    result["updated_base"] = store.describe_base("upd", result["wine"])
    bench.record("update", result)
    # 2. How the bases differ.
    if store.base("fresh").is_dir():
        result["registry_old_to_fresh"] = registry_difference(store, "old", "fresh")
        result["registry_updated_to_fresh"] = registry_difference(store, "upd", "fresh")
        result["files_old_to_fresh"] = file_difference(store, "old", "fresh")
    result["registry_old_to_updated"] = registry_difference(store, "old", "upd")
    result["files_old_to_updated"] = file_difference(store, "old", "upd")
    bench.record("update", result)
    # 3. Today's way: Wine updates a plain prefix that holds an application.
    if store.plain("keep-7zip-exe").is_dir():
        before = layerlib.disk_use([store.plain("keep-7zip-exe") / "prefix"])
        done = store.run("plain", "keep-7zip-exe", ["wine", "wineboot", "--update"], creating=True, timeout=1200,
                         label="update-plain")
        result["update_plain"] = {"exit": done["exit"], "seconds": done["seconds"],
                                  "program_seconds": done.get("program_seconds"),
                                  "bytes_before": before["apparent_bytes"],
                                  "bytes_after": layerlib.disk_use(
                                      [store.plain("keep-7zip-exe") / "prefix"])["apparent_bytes"]}
        bench.record("update", result)
        run_pass(bench, "updated-plain", store, "plain", ["7zip-exe"], stages=("launch", "check", "uninstall"),
                 prefix="keep-", existing=True)
    # 4. The naive way on layers: Wine updates a layer over the old base.
    if store.layer("h3-7zip-exe").is_dir():
        store.copy_layer("h3-7zip-exe", "naive-7zip-exe")
        before = store.layer_disk("naive-7zip-exe")
        done = store.run("layer", "naive-7zip-exe", ["wine", "wineboot", "--update"], creating=True, timeout=1200,
                         label="update-naive-layer")
        after = store.layer_disk("naive-7zip-exe")
        result["update_layer_naively"] = {"exit": done["exit"], "seconds": done["seconds"],
                                          "bytes_before": before["apparent_bytes"],
                                          "bytes_after": after["apparent_bytes"], "files_after": after["files"]}
        bench.record("update", result)
        run_pass(bench, "naively-updated-layer", store, "layer", ["7zip-exe"],
                 stages=("launch", "check", "uninstall"), prefix="naive-", existing=True)
    result["verify_old_base"] = store.verify_base("old")
    return result


def phase_rebase(bench: Bench) -> dict:
    """The five layers made under the old Wine, rebased onto a base of the
    new Wine: registry three-way, stale files dropped; then started, checked
    and removed under the new Wine. Once onto the updated base, once onto a
    fresh one."""
    store = bench.store()
    result: dict = {"wine": bench.wine_version(), "old_wine": bench.state.get("old_wine"), "targets": {}}
    noise_layers = ["noise-old"] if store.layer("noise-old").is_dir() else []
    for target in ("upd", "fresh"):
        if not store.base(target).is_dir():
            result["targets"][target] = {"status": "NOT RUN: that base does not exist"}
            continue
        result[f"noise_{target}"] = noise_of(store, target, f"noise-{target}")
        result[f"noise_{target}_listing"] = noise_listing(store, f"noise-{target}")
        bench.record("rebase", result)
    for target in ("upd", "fresh"):
        if not store.base(target).is_dir():
            continue
        noise = noise_sets(store, noise_layers + [f"noise-{target}"])
        entry: dict = {"noise_pairs": {name: len(pairs) for name, pairs in noise.items()}, "apps": {}}
        prefix = f"r{target[0]}-"
        for app in FIVE:
            source, ident = store.layer(f"h3-{app}"), f"{prefix}{app}"
            if not source.is_dir():
                entry["apps"][app] = {"status": "NOT RUN: no layer from the old Wine"}
                continue
            try:
                copied = store.copy_layer(f"h3-{app}", ident)
                before = store.layer_disk(ident)
                started = time.monotonic()
                captured = store.capture(ident)
                files = store.rebase_files(ident, "old", target)
                registry = store.materialise(ident, [f"base:{target}"], noise)
                seconds = round(time.monotonic() - started, 3)
                (bench.evidence / "rebase").mkdir(exist_ok=True)
                (bench.evidence / "rebase" / f"{ident}.json").write_text(
                    json.dumps({"captured": captured, "files": files, "registry": registry}, indent=1))
                entry["apps"][app] = {
                    "copy_exit": copied["exit"], "copy_seconds": copied["seconds"], "rebase_seconds": seconds,
                    "capture_seconds": captured["seconds"], "files_seconds": files["seconds"],
                    "registry_seconds": registry["seconds"], "delta": captured["files"],
                    "conflicts": registry["conflicts"],
                    "conflict_list": [dict(found, file=name) for name, report in registry["files"].items()
                                      for found in report["conflicts"]][:60],
                    "registry_files": {name: {key: report[key] for key in
                                              ("ops", "applied", "already", "noise_skipped", "written")}
                                       for name, report in registry["files"].items()},
                    "stale_files_dropped": len(files["stale_dropped"]), "stale_bytes": files["stale_bytes"],
                    "stale_examples": files["stale_dropped"][:12], "kept_modified": files["kept_modified"][:20],
                    "both_changed": files["both_changed"][:20],
                    "hides_new_base_file": files["hides_new_base_file"][:20], "whiteouts": len(files["whiteouts"]),
                    "opaque_directories": len(files["opaque_directories"]),
                    "bytes_before": before["apparent_bytes"],
                    "bytes_after": store.layer_disk(ident)["apparent_bytes"]}
            except Exception as error:  # noqa: BLE001
                traceback.print_exc()
                entry["apps"][app] = {"status": f"harness error: {error}"}
            result["targets"][target] = entry
            bench.record("rebase", result)
        done = run_pass(bench, f"rebased-{target}", store, "layer",
                        [app for app in FIVE if store.layer(f"{prefix}{app}").is_dir()],
                        stages=("launch", "check"), prefix=prefix, existing=True)
        # What Wine made of the registry that was written: read it back after
        # the application ran, and compare with what the rebase wrote.
        for app in FIVE:
            ident = f"{prefix}{app}"
            if not store.layer(ident).is_dir() or "delta" not in entry["apps"].get(app, {}):
                continue
            again = store.capture(ident)
            entry["apps"][app]["delta_after_running"] = again["files"]
            log_text = "".join(path.read_text(errors="replace")
                               for path in sorted((bench.evidence / "passes" / f"rebased-{target}" / ident).glob("*.txt")))
            entry["apps"][app]["registry_complaints"] = [line for line in log_text.splitlines()
                                                         if "registry file" in line or "Malformed" in line
                                                         or "Unrecognized input" in line][:10]
        run_pass(bench, f"rebased-{target}-removal", store, "layer",
                 [app for app in FIVE if store.layer(f"{prefix}{app}").is_dir()],
                 stages=("uninstall",), prefix=prefix, existing=True)
        entry["pass"] = [{"id": app["id"], "launch": app["launch"]["status"], "check": app["check"]["status"]}
                         for app in done["apps"]]
        entry["verify_base"] = store.verify_base(target)
        result["targets"][target] = entry
        bench.record("rebase", result)
    result["verify_old_base"] = store.verify_base("old") if store.base("old").is_dir() else None
    return result


def phase_h1(bench: Bench, variants: list[str]) -> dict:
    """Creating a prefix today against creating a layer: time and bytes, on
    each file system; start-up of Wine in each; the cost of a copy."""
    result: dict = {"wine": bench.wine_version(), "variants": {}}
    for variant in variants:
        directory = bench.work / "fs" / variant
        if not directory.is_dir():
            result["variants"][variant] = {"status": "NOT RUN: that file system was not made"}
            continue
        fold = variant == "ext4cf"
        store = propagate_settings(bench, variant)
        entry: dict = {"file_system": layerlib.command(["findmnt", "-no", "FSTYPE,OPTIONS", "-T", str(directory)]
                                                       ).stdout.strip(), "casefold": fold}
        try:
            entry["plain_first"] = store.create_plain("p1", casefold=fold)
            entry["base"] = store.create_base("fresh", casefold=fold)
            store.describe_base("fresh", result["wine"])
            entry["plain_second"] = store.create_plain("p2", casefold=fold)
            result["variants"][variant] = entry
            bench.record("h1", result)
            # Layers: five, for the spread of the time to make one.
            entry["layers"] = []
            for index in range(5):
                made = store.create_layer(f"l{index}", ["base:fresh"])
                entry["layers"].append({"seconds": made["seconds"], "bytes": made["disk"]["apparent_bytes"],
                                        "allocated": made["disk"]["allocated_bytes"],
                                        "casefold": made.get("casefold_set")})
            first = store.run("layer", "l0", ["wine", "cmd", "/c", "exit"], timeout=300, label="first-run-layer")
            if first["exit"] != 0 and fold:
                # Try the other way of making the work directory.
                entry["first_run_folded_work"] = {"exit": first["exit"], "stderr": short(first["stderr"], 6)}
                store.create_layer("l0", ["base:fresh"], fold_work=False)
                first = store.run("layer", "l0", ["wine", "cmd", "/c", "exit"], timeout=300,
                                  label="first-run-layer-unfolded-work")
                entry["work_directory_folded"] = False
                if first["exit"] == 0:
                    store.settings["fold_work"] = False
                    store.save_settings()
            entry["layer_first_run"] = {"exit": first["exit"], "seconds": first["seconds"],
                                        "program_seconds": first.get("program_seconds"),
                                        "stderr": short(first["stderr"], 6) if first["exit"] else "",
                                        "disk": store.layer_disk("l0"),
                                        "files": sorted(f"{part}/{relative}" for part in ("root", "c")
                                                        for relative, info in layerlib.walk(store.layer("l0") / part)
                                                        if not stat.S_ISDIR(info.st_mode))[:60]}
            entry["layer_merged_casefold"] = None
            if fold:
                seen = store.run("layer", "l0", ["/bin/sh", "-c",
                                                 f"lsattr -d {SANDBOX_PREFIX}/drive_c {SANDBOX_PREFIX}/drive_c/windows; "
                                                 f"ls -d {SANDBOX_PREFIX}/drive_c/WINDOWS/SYSTEM32/KERNEL32.DLL"],
                                 wine=False, timeout=60, label="casefold-in-layer")
                entry["layer_merged_casefold"] = {"exit": seen["exit"], "stdout": seen["stdout"],
                                                  "stderr": short(seen["stderr"], 4)}
            # Start-up, alternating so that neither always runs first.
            samples: dict = {"plain": [], "layer": []}
            if first["exit"] == 0:
                for _ in range(6):
                    for kind, ident in (("plain", "p2"), ("layer", "l0")):
                        done = store.run(kind, ident, ["wine", "cmd", "/c", "exit"], timeout=300,
                                         label=f"startup-{kind}")
                        samples[kind].append({"exit": done["exit"], "seconds": done["seconds"],
                                              "program_seconds": done.get("program_seconds")})
            entry["startup"] = samples
            # A throwaway copy, as the autopilot makes one today.
            started = time.monotonic()
            done = layerlib.command(["cp", "-a", "--reflink=auto", str(store.plain("p2")),
                                     str(store.plain("copy"))], timeout=900)
            entry["copy_plain"] = {"exit": done.returncode, "seconds": round(time.monotonic() - started, 3),
                                   "disk": layerlib.disk_use([store.plain("copy")])}
            entry["verify_base"] = store.verify_base("fresh")
            # The second plain prefix stays on the main file system only (the
            # safety phase prints its sandbox beside a layer's).
            for name in ("p1", "copy") + (() if variant == "ext4" else ("p2",)):
                layerlib.rmtree(store.plain(name))
            for index in range(5):
                store.drop_layer(f"l{index}")
        except Exception as error:  # noqa: BLE001
            traceback.print_exc()
            entry["harness_error"] = str(error)
        result["variants"][variant] = entry
        bench.record("h1", result)
    return result


def phase_disk(bench: Bench) -> dict:
    """Ten applications on layers: what the store holds; what the same ten
    cost as plain prefixes; files that are stored more than once."""
    store = bench.store()
    layered = bench.results.get("pass:layered", {}).get("apps", []) \
        + bench.results.get("pass:layered-second", {}).get("apps", [])
    idents = [app["prefix"] for app in layered if store.layer(app["prefix"]).is_dir()]
    result: dict = {"layers": {}}
    for ident in idents:
        result["layers"][ident] = store.layer_disk(ident)
    base = layerlib.disk_use([store.base("fresh") / "root", store.base("fresh") / "c"])
    result["base"] = base
    result["layered_total_bytes"] = base["apparent_bytes"] + sum(
        entry["apparent_bytes"] for entry in result["layers"].values())
    result["layered_total_allocated"] = base["allocated_bytes"] + sum(
        entry["allocated_bytes"] for entry in result["layers"].values())
    plain = {app["id"]: app.get("disk", {}).get("apparent_bytes")
             for app in bench.results.get("pass:plain", {}).get("apps", [])}
    result["plain_bytes_by_application"] = plain
    names = [app["id"] for app in layered if store.layer(app["prefix"]).is_dir()]
    if names and all(plain.get(name) for name in names):
        # A second copy of an application counts its plain prefix again.
        result["plain_total_bytes_for_the_same"] = sum(plain[name] for name in names)
    result["applications"] = idents
    # Identical files: across layers, and against the base.
    by_hash: dict[str, list] = {}
    base_hashes: dict[str, int] = {}
    for part in ("root", "c"):
        for relative, info in layerlib.walk(store.base("fresh") / part):
            if stat.S_ISREG(info.st_mode) and info.st_size:
                base_hashes[layerlib.file_sha256(store.base("fresh") / part / relative)] = info.st_size
    total = unique = shadow = 0
    for ident in idents:
        for part in ("root", "c"):
            for relative, info in layerlib.walk(store.layer(ident) / part):
                if not stat.S_ISREG(info.st_mode) or not info.st_size:
                    continue
                digest = layerlib.file_sha256(store.layer(ident) / part / relative)
                total += info.st_size
                if digest in base_hashes:
                    shadow += info.st_size
                by_hash.setdefault(digest, []).append((ident, info.st_size))
    for digest, places in by_hash.items():
        unique += places[0][1]
    result["identical_files"] = {"layer_bytes": total, "unique_bytes": unique, "saved_by_one_store": total - unique,
                                 "bytes_identical_to_a_base_file": shadow,
                                 "files": sum(len(places) for places in by_hash.values()),
                                 "distinct_files": len(by_hash)}
    return result


def reg_query(store: Store, ident: str, key: str, value: str | None = None) -> dict:
    argv = ["wine", "reg", "query", key] + (["/v", value] if value else [])
    done = store.run("layer", ident, argv, timeout=180, label=f"reg-query-{ident}")
    return {"exit": done["exit"], "stdout": short(done["stdout"], 6)}


def phase_groups(bench: Bench) -> dict:
    """Two applications in separate layers, composed so that one sees the
    other, and separated again; and a runtime library layer."""
    store = bench.store()
    result: dict = {}
    done = run_pass(bench, "group-install", store, "layer", ["7zip-exe", "notepadpp-nsis"], base="fresh",
                    stages=("install",), prefix="g-")
    if not all(app["install"]["status"] == "pass" for app in done["apps"]):
        return {"status": "NOT RUN: the two applications did not both install"}
    seven, npp = "g-7zip-exe", "g-notepadpp-nsis"
    world = World(bench, "groups")
    base = ["base:fresh"]
    noise = fresh_noise(store)
    result["noise_pairs"] = {name: len(pairs) for name, pairs in noise.items()}
    result["captured"] = {seven: store.capture(seven)["files"], npp: store.capture(npp)["files"]}
    seven_exe = f"{SANDBOX_PREFIX}/drive_c/Program Files/7-Zip/7z.exe"

    def look(label: str) -> dict:
        seen = store.run("layer", npp, ["/bin/sh", "-c", f'test -f "{seven_exe}" && echo present || echo absent'],
                         wine=False, timeout=60, label=f"group-look-{label}")
        tool = store.run("layer", npp, ["wine", seven_exe, "i"], timeout=180, label=f"group-7z-{label}")
        return {"7z.exe": seen["stdout"].strip(), "7z_runs": tool["exit"] == 0 and "7-Zip" in tool["stdout"],
                "7z_exit": tool["exit"],
                "registry_7zip": reg_query(store, npp, "HKLM\\Software\\7-Zip"),
                "registry_notepadpp": reg_query(store, npp, "HKLM\\Software\\Notepad++"),
                "registry_own_value": reg_query(store, npp, "HKCU\\Software\\TUR-Layers", "grouped")}

    def npp_check(label: str) -> dict:
        run = AppRun(bench, store, world, bench.apps["notepadpp-nsis"], "layer", npp)
        run.evidence = world.evidence / label
        run.evidence.mkdir(parents=True, exist_ok=True)
        run.check()
        return run.result["check"]

    result["separate_before"] = look("before")
    bench.record("groups", result)
    # Join: Notepad++'s layer over the base and 7-Zip's layer.
    seven_hash = layerlib.tree_hash([store.layer(seven) / "root", store.layer(seven) / "c"])
    result["join"] = store.materialise(npp, base + [f"layer:{seven}"], noise)
    result["joined"] = look("joined")
    added = store.run("layer", npp, ["wine", "reg", "add", "HKCU\\Software\\TUR-Layers", "/v", "grouped", "/t",
                                     "REG_SZ", "/d", "yes", "/f"], timeout=180, label="group-reg-add")
    result["joined_reg_add_exit"] = added["exit"]
    result["joined_check"] = npp_check("joined")
    # From the group, Notepad++ deletes a file of 7-Zip's: its own view only.
    removed = store.run("layer", npp, ["/bin/sh", "-c", f'rm "{seven_exe}" && echo removed'], wine=False,
                        timeout=60, label="group-remove-other")
    result["delete_in_group"] = {"exit": removed["exit"], "stdout": removed["stdout"].strip(),
                                 "seen_after": look("after-delete")["7z.exe"]}
    after = layerlib.tree_hash([store.layer(seven) / "root", store.layer(seven) / "c"])
    result["other_layer_unchanged_while_lower"] = after["sha256"] == seven_hash["sha256"]
    bench.record("groups", result)
    # Separate again.
    result["captured_in_group"] = store.capture(npp)["files"]
    result["separate"] = store.materialise(npp, base, noise)
    result["separate_after"] = look("separated")
    result["separate_check"] = npp_check("separated")
    # 7-Zip on its own is what it was.
    run = AppRun(bench, store, world, bench.apps["7zip-exe"], "layer", seven)
    run.check()
    result["other_application_check"] = run.result["check"]
    bench.record("groups", result)

    # A runtime library layer.
    build = bench.build
    if not build or not (build / "needs.exe").is_file() or not (build / "turrt.dll").is_file():
        result["runtime_layer"] = {"status": "NOT RUN: the test programs were not built"}
        return result
    runtime: dict = {}
    store.create_layer("rt-tur", base)
    store.create_layer("needs", base)
    files = (str(build / "needs.exe"), str(build / "turrt.dll"))
    put = store.run("layer", "rt-tur", ["install", "-D", "-m", "0644", str(build / "turrt.dll"),
                                        f"{SANDBOX_PREFIX}/drive_c/windows/system32/turrt.dll"], wine=False,
                    files=files, label="runtime-layer-file")
    key = store.run("layer", "rt-tur", ["wine", "reg", "add", "HKLM\\Software\\TUR\\Runtime", "/v", "Version",
                                        "/t", "REG_SZ", "/d", "1.0", "/f"], timeout=180, label="runtime-layer-reg")
    runtime["made"] = {"file_exit": put["exit"], "registry_exit": key["exit"],
                       "captured": store.capture("rt-tur")["files"], "disk": store.layer_disk("rt-tur")}
    program = f"{SANDBOX_PREFIX}/drive_c/Program Files/needs/needs.exe"
    store.run("layer", "needs", ["install", "-D", "-m", "0755", str(build / "needs.exe"), program], wine=False,
              files=files, label="needs-file")
    store.capture("needs")

    def needs(label: str) -> dict:
        done = store.run("layer", "needs", ["wine", program], timeout=180, label=f"needs-{label}")
        return {"exit": done["exit"], "stdout": done["stdout"].strip()[-200:],
                "missing_dll": "turrt.dll" in done["stderr"]}

    runtime["without"] = needs("without")
    store.capture("needs")
    runtime["join"] = {key: value for key, value in store.materialise("needs", base + ["layer:rt-tur"], noise).items()
                       if key in ("conflicts", "seconds")}
    runtime["with"] = needs("with")
    store.capture("needs")
    runtime["separate"] = {key: value for key, value in store.materialise("needs", base, noise).items()
                           if key in ("conflicts", "seconds")}
    runtime["without_again"] = needs("without-again")
    result["runtime_layer"] = runtime
    result["verify_base"] = store.verify_base("fresh")
    return result


SAFETY_SCRIPT = r"""
echo "== id"; id
echo "== capabilities"; grep -E '^Cap(Inh|Prm|Eff|Bnd)' /proc/self/status
echo "== mounts of the prefix"; grep -E ' /tur' /proc/self/mountinfo
try() { label="$1"; shift; if "$@" >/dev/null 2>/tmp/err; then echo "RESULT $label: allowed"; else echo "RESULT $label: refused ($(head -c 160 /tmp/err | tr '\n' ' '))"; fi; }
try see-base-on-host ls "$BASE"
try see-other-layer-on-host ls "$OTHER"
try see-own-layer-on-host ls "$OWN"
try see-store ls "$STORE"
try see-oldroot ls /oldroot
try write-base-on-host sh -c "echo x > '$BASE/c/tur-was-here'"
try write-other-layer-on-host sh -c "echo x > '$OTHER/c/tur-was-here'"
try remount-prefix-rw mount -o remount,rw /tur/prefix
try unmount-prefix umount /tur/prefix
try unmount-drive-c umount -l /tur/prefix/drive_c
try mount-overlay-of-own mount -t overlay overlay -o lowerdir=/usr:/etc /tmp
try new-user-namespace unshare -U true
try change-base-file-in-view sh -c "echo changed >> /tur/prefix/drive_c/windows/win.ini"
try delete-base-file-in-view rm /tur/prefix/drive_c/windows/system32/notepad.exe
try create-file-in-view sh -c "echo new > /tur/prefix/drive_c/windows/tur-new-file"
try write-system-directory sh -c "echo x > /usr/tur-was-here"
try write-root sh -c "echo x > /tur-was-here"
echo "== done"
"""


def phase_safety(bench: Bench) -> dict:
    """What an application's sandbox can reach and change, on layers."""
    store = bench.store()
    base = ["base:fresh"]
    store.create_layer("s-own", base)
    store.create_layer("s-other", base)
    (store.layer("s-other") / "c" / "other-secret.txt").write_text("another application's file\n")
    other_before = layerlib.tree_hash([store.layer("s-other") / "root", store.layer("s-other") / "c"])
    env = {"BASE": str(store.base("fresh")), "OTHER": str(store.layer("s-other")),
           "OWN": str(store.layer("s-own")), "STORE": str(store.root)}
    done = store.run("layer", "s-own", ["/bin/sh", "-c", SAFETY_SCRIPT], wine=False, timeout=120,
                     label="safety", env=env)
    (bench.evidence / "safety-output.txt").write_text(done["stdout"] + "\n--- stderr\n" + done["stderr"])
    outcomes = {}
    for line in done["stdout"].splitlines():
        if line.startswith("RESULT "):
            name, _, outcome = line[7:].partition(": ")
            outcomes[name] = outcome
    result: dict = {"exit": done["exit"], "outcomes": outcomes,
                    "capabilities": [line for line in done["stdout"].splitlines() if line.startswith("Cap")],
                    "id": next((line for line in done["stdout"].splitlines() if line.startswith("uid=")), None)}
    # A Windows program does the same through Wine.
    wine = store.run("layer", "s-own", ["wine", "cmd", "/c", "del C:\\windows\\system32\\winhlp32.exe & "
                                        "echo by-wine> C:\\windows\\tur-by-wine.txt"], timeout=180,
                     label="safety-wine")
    result["wine_exit"] = wine["exit"]
    own = store.layer("s-own")
    result["own_layer_after"] = sorted(
        f"{part}/{relative}" + (" (whiteout)" if stat.S_ISCHR(info.st_mode) else "")
        for part in ("root", "c") for relative, info in layerlib.walk(own / part)
        if not stat.S_ISDIR(info.st_mode))[:80]
    result["base_unchanged"] = store.verify_base("fresh")
    after = layerlib.tree_hash([store.layer("s-other") / "root", store.layer("s-other") / "c"])
    result["other_layer_unchanged"] = after["sha256"] == other_before["sha256"]
    result["host_has_no_trace"] = not (store.base("fresh") / "c" / "tur-was-here").exists() \
        and not (store.layer("s-other") / "c" / "tur-was-here").exists()
    # The command lines, for the report.
    plain_example = None
    for directory in sorted((store.root / "plain").glob("*")):
        plain_example = store.sandbox("plain", directory.name)
        break
    (bench.evidence / "sandbox-layered.txt").write_text(" \\\n  ".join(store.sandbox("layer", "s-own")) + "\n")
    if plain_example:
        (bench.evidence / "sandbox-plain.txt").write_text(" \\\n  ".join(plain_example) + "\n")
    # Two sandboxes on one layer at once.
    first = subprocess.Popen(store.sandbox("layer", "s-own") + ["/bin/sh", "-c", "sleep 6"],
                             env=store.environment(), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    time.sleep(2)
    second = store.run("layer", "s-own", ["/bin/sh", "-c", "echo second-sandbox-started"], wine=False, timeout=60,
                       label="safety-second-sandbox")
    first_error = first.communicate(timeout=30)[1]
    result["two_sandboxes_one_layer"] = {"second_exit": second["exit"], "second_stdout": second["stdout"].strip(),
                                         "second_stderr": short(second["stderr"], 4),
                                         "first_exit": first.returncode, "first_stderr": short(first_error, 4)}
    for ident in ("s-own", "s-other"):
        store.drop_layer(ident)
    return result


def phase_extras(bench: Bench) -> dict:
    """Snapshot and rollback of a layer, factory reset with the documents
    kept, and a trial layer over an application (the autopilot's copy)."""
    store = bench.store()
    result: dict = {}
    world = World(bench, "extras")
    ident = "notepadpp-nsis"
    if not store.layer(ident).is_dir():
        return {"status": "NOT RUN: the layered pass left no Notepad++ layer"}
    app = bench.apps["notepadpp-nsis"]
    trees = [store.layer(ident) / "root", store.layer(ident) / "c"]
    # Snapshot, damage, roll back.
    before = layerlib.tree_hash(trees)
    result["snapshot"] = store.snapshot(ident, "before-update")
    exe = store.find_program("layer", ident, app["main_executable"])
    damage = store.run("layer", ident, ["/bin/sh", "-c", f'rm "{exe}" && echo removed'], wine=False, timeout=60,
                       label="extras-damage")
    store.run("layer", ident, ["wine", "reg", "add", "HKLM\\Software\\Notepad++", "/v", "TUR-Damage", "/t",
                               "REG_SZ", "/d", "1", "/f"], timeout=180, label="extras-damage-registry")
    result["damaged"] = {"exit": damage["exit"],
                         "main_program_gone": store.find_program("layer", ident, app["main_executable"]) is None}
    result["rollback"] = store.rollback(ident, "before-update")
    after = layerlib.tree_hash(trees)
    result["rollback_restored_bytes"] = after["sha256"] == before["sha256"]
    run = AppRun(bench, store, world, app, "layer", ident)
    run.check()
    result["check_after_rollback"] = run.result["check"]
    layerlib.rmtree(store.layer(ident) / "snapshots")
    bench.record("extras", result)

    # A trial layer over an application: nothing is copied.
    seven = "7zip-exe"
    if store.layer(seven).is_dir():
        store.capture(seven)
        seven_hash = layerlib.tree_hash([store.layer(seven) / "root", store.layer(seven) / "c"])
        trials = []
        for index in range(3):
            started = time.monotonic()
            store.create_layer(f"trial-{index}", ["base:fresh"])
            joined = store.materialise(f"trial-{index}", ["base:fresh", f"layer:{seven}"], fresh_noise(store))
            trials.append({"seconds": round(time.monotonic() - started, 3),
                           "registry_seconds": joined["seconds"],
                           "bytes": store.layer_disk(f"trial-{index}")["apparent_bytes"]})
        run = AppRun(bench, store, world, bench.apps[seven], "layer", "trial-0")
        run.check()
        result["trial_layers"] = {"made": trials, "check_in_trial": run.result["check"],
                                  "trial_bytes_after_check": store.layer_disk("trial-0")["apparent_bytes"],
                                  "application_layer_unchanged": layerlib.tree_hash(
                                      [store.layer(seven) / "root", store.layer(seven) / "c"])["sha256"]
                                  == seven_hash["sha256"]}
        for index in range(3):
            store.drop_layer(f"trial-{index}")
        bench.record("extras", result)

    # Factory reset: the layer goes, the documents stay. On the portable
    # Notepad++, which has no uninstaller that a later phase would test.
    ident, app = "notepadpp-zip", bench.apps["notepadpp-zip"]
    if not store.layer(ident).is_dir():
        result["reset"] = {"status": "NOT RUN: the layered pass left no layer of the portable Notepad++"}
        return result
    user = store.user
    note = store.run("layer", ident, ["wine", "cmd", "/c",
                                      f"echo kept> C:\\users\\{user}\\Documents\\tur-note.txt"], timeout=180,
                     label="extras-note")
    home_note = store.layer(ident) / "home" / "Documents" / "tur-note.txt"
    layer_note = store.layer(ident) / "c" / "users" / user / "Documents" / "tur-note.txt"
    result["reset"] = {"note_exit": note["exit"], "note_in_home": home_note.is_file(),
                       "note_in_layer": layer_note.is_file(), "bytes_before": store.layer_disk(ident)["apparent_bytes"]}
    result["reset"].update(store.reset(ident))
    result["reset"].update(note_in_home_after=home_note.is_file(),
                           main_program_after=store.find_program("layer", ident, app["main_executable"]),
                           bytes_after=store.layer_disk(ident)["apparent_bytes"])
    started = store.run("layer", ident, ["wine", "cmd", "/c", "ver"], timeout=180, label="extras-after-reset")
    result["reset"]["wine_starts_after"] = started["exit"] == 0
    store.drop_layer(ident)
    result["verify_base"] = store.verify_base("fresh")
    return result


def phase_remove_layered(bench: Bench) -> dict:
    store = bench.store()
    ids = [app["id"] for app in bench.results.get("pass:layered", {}).get("apps", [])
           if store.layer(app["prefix"]).is_dir()]
    run_pass(bench, "layered-removal", store, "layer", ids, stages=("uninstall",), existing=True)
    others = sorted(path.name for path in (store.root / "layers").glob("*"))
    for extra in others:
        store.drop_layer(extra)
    result = {"bases": {base: store.verify_base(base) for base in ("old", "upd", "fresh")
                        if (store.base(base) / "base.json").is_file()},
              "other_layers_dropped": others,
              "layers_left": sorted(path.name for path in (store.root / "layers").glob("*")),
              "wine_servers_left": rt.run(["pgrep", "-a", "wineserver"], timeout=10).stdout.strip()}
    return result


def phase_mini(bench: Bench, variants: list[str]) -> dict:
    """Two applications on layers on each file system (the virtual machine's
    job: the kernel TUR ships)."""
    result: dict = {}
    for variant in variants:
        store = bench.store(variant)
        measured = bench.results.get("h1", {}).get("variants", {}).get(variant, {})
        if not store.base("fresh").is_dir():
            result[variant] = "NOT RUN: no base on that file system"
            continue
        if measured.get("layer_first_run", {}).get("exit") != 0:
            result[variant] = "NOT RUN: Wine did not start in a layer on that file system (see h1)"
            continue
        done = run_pass(bench, f"mini-{variant}", store, "layer", ["7zip-exe", "notepadpp-nsis"], base="fresh")
        result[variant] = {"apps": [{"id": app["id"], **{name: app[name]["status"] for name in rt.STAGES}}
                                    for app in done["apps"]], "verify_base": store.verify_base("fresh")}
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--work", required=True, type=pathlib.Path)
    parser.add_argument("--evidence", required=True, type=pathlib.Path)
    parser.add_argument("--apps", required=True, type=pathlib.Path)
    parser.add_argument("--build", type=pathlib.Path, help="where needs.exe and turrt.dll were built")
    parser.add_argument("--phases", required=True)
    parser.add_argument("--variants", default="ext4,ext4cf,btrfs")
    parser.add_argument("--only", default="", help="for the download phase: comma-separated application ids")
    options = parser.parse_args()

    options.evidence.mkdir(parents=True, exist_ok=True)
    apps = json.loads(options.apps.read_text())["applications"]
    bench = Bench(options.work.resolve(), options.evidence.resolve(), apps, options.build, "")
    bench.wine_label = bench.wine_version()
    variants = [name for name in options.variants.split(",") if name]
    everything = [app["id"] for app in apps]
    try:
        display = rt.start_display(bench.evidence)
    except OSError as error:
        # Without a display the applications cannot show a window; what does
        # not need one still runs, and the launches say what happened.
        log(f"the virtual display could not be started: {error}")
        display = []
    looked = rt.run(["xwininfo", "-display", rt.DISPLAY, "-root"], timeout=30)
    bench.display_ok = looked.returncode == 0
    if not bench.display_ok:
        log(f"the virtual display cannot be looked at (xwininfo: {short(looked.stderr, 2)}); "
            "launches and window checks are recorded as not run")

    def layered(bench: Bench) -> dict:
        store = bench.store()
        done = run_pass(bench, "layered", store, "layer", everything, base="fresh",
                        stages=("install", "launch", "check"))
        # Two more, to make ten: second copies of two applications, installed only.
        run_pass(bench, "layered-second", store, "layer", ["7zip-exe", "putty-exe"], base="fresh",
                 stages=("install",), prefix="second-")
        return {"verify_base": store.verify_base("fresh"),
                "apps": [{"id": app["id"], **{name: app[name]["status"] for name in rt.STAGES}}
                         for app in done["apps"]]}

    def plain(bench: Bench) -> dict:
        done = run_pass(bench, "plain", bench.store(), "plain", everything)
        return {"apps": [{"id": app["id"], **{name: app[name]["status"] for name in rt.STAGES}}
                         for app in done["apps"]]}

    def download(bench: Bench) -> dict:
        world = World(bench, "download")
        wanted = [name for name in options.only.split(",") if name] or everything
        found = {}
        for name in wanted:
            entry: dict = {}
            rt.download(world, bench.apps[name], entry)
            found[name] = entry.get("download")
        return found

    phases = {
        "download": download,
        "preflight": phase_preflight,
        "old-base": phase_old_base,
        "h1": lambda bench: phase_h1(bench, variants),
        "update": phase_update,
        "rebase": phase_rebase,
        "layered": layered,
        "plain": plain,
        "disk": phase_disk,
        "groups": phase_groups,
        "safety": phase_safety,
        "extras": phase_extras,
        "remove-layered": phase_remove_layered,
        "mini": lambda bench: phase_mini(bench, variants),
    }
    failed = []
    try:
        for name in options.phases.split(","):
            log(f"\n########## phase {name} ({bench.wine_label}, kernel {platform.release()})")
            started = time.monotonic()
            try:
                data = phases[name](bench)
                if isinstance(data, dict):
                    data["phase_seconds"] = round(time.monotonic() - started, 1)
                bench.record(name, data)
            except Exception as error:  # noqa: BLE001 - recorded, and the next phase runs
                traceback.print_exc()
                failed.append(name)
                previous = bench.results.get(name) if isinstance(bench.results.get(name), dict) else {}
                bench.record(name, {**previous, "phase_error": f"{type(error).__name__}: {error}",
                                    "traceback": traceback.format_exc()[-3000:]})
            if name == "preflight" and not bench.results["preflight"].get("overlay_works_unprivileged"):
                log("bubblewrap cannot mount overlayfs here without privilege: the layered phases cannot run")
    finally:
        for process in display:
            process.terminate()
    log(f"\nphases with a harness error: {failed or 'none'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

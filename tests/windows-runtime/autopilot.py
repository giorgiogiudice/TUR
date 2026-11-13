#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Proves tur-runtime's compatibility autopilot on programs that are broken on
purpose, and on real applications with a fault injected.

Run by the `autopilot` job of .github/workflows/windows-runtime.yml on a
hosted Ubuntu runner. It is never run on a development machine (AGENTS.md).

    autopilot.py --tur-runtime BIN --tur-catalogue BIN --build-dir DIR \
        --work-dir DIR --evidence DIR [--reflink-work-dir DIR] \
        [--apps apps.json --real ID,ID,... --downloads DIR]

The broken programs (tests/windows-runtime/autopilot/, built by build.sh
there) are installed with `tur-runtime app install`. For each one:

  1. a content hash of its real prefix and private home is taken;
  2. `tur-runtime app autopilot <app-id> --check <check>`: the failure
     class, the remedy found, the attempts and the time are compared with
     what CASES expects;
  3. the real prefix must hash the same as before: nothing was changed;
  4. the compatibility record it wrote is appended to a scratch catalogue
     entry, which `tur-catalogue validate` must accept and `tur-catalogue
     route` must show as the wine route's settings;
  5. `app autopilot --apply <result>`: a backup must exist afterwards, and
     the program, started with `app launch`, must now pass its check;
  6. the autopilot again: the application must pass unchanged;
  7. `app autopilot --revert`: the prefix must hash as it did before the
     apply.

The real applications of apps.json named with --real are downloaded from
their publishers (pinned size and SHA-256), installed, and run by the
autopilot unchanged, which must pass. Then one DLL their main program
imports is disabled for that program in the prefix (a stale override, as a
leftover setting leaves behind), the autopilot must find the remedy, the
remedy is applied, and the autopilot must pass the application unchanged
again. The time of every step is recorded.

One case runs a second time with the data directory on btrfs, to measure
copy-on-write copies (--reflink-work-dir).

The exit status is 1 when anything differs from what is expected. The table
is written either way.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import stat
import struct
import subprocess
import sys
import time
import traceback
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import apps  # noqa: E402  (the real-application harness beside this file)

CONSOLE_CHECK = {
    "schema_version": 1,
    "window": False,
    "exit": "zero",
    "timeout_seconds": 60,
    "settle_seconds": 0,
    "output_contains": ["TUR-CHECK-PASS"],
}

# The deliberately broken programs: what is installed, what failure the
# autopilot must see, and what it must find.
CASES = [
    {"name": "winver", "what": "refuses any Windows older than 11",
     "files": {"winver.exe": None}, "portable": True,
     "class": "exited-with-error", "outcome": "fixed", "found": ["windows-version-win11"]},
    {"name": "dllover", "what": "ships a d3dx9_43.dll that fails to initialise",
     "files": {"dllover.exe": None, "d3dx9_43.dll": None},
     "class": "dll-init-failed", "outcome": "fixed", "found": ["dll-builtin:d3dx9_43"]},
    {"name": "combo", "what": "both of the above: a broken DLL, then a version check",
     "files": {"combo.exe": None, "d3dx9_43.dll": None},
     "class": "dll-init-failed", "outcome": "fixed",
     "found": ["dll-builtin:d3dx9_43", "windows-version-win11"]},
    {"name": "mfc", "what": "imports mfc140u.dll, a Microsoft component",
     "files": {"mfc.exe": None}, "portable": True,
     "class": "microsoft-component-missing", "outcome": "unsuppliable", "found": []},
    {"name": "casepath", "what": "its archive spells a directory Plugins and plugins",
     "files": {"casepath.exe": None, "Plugins/one.txt": b"one\n", "plugins/two.txt": b"two\n"},
     "class": "exited-with-error", "outcome": "fixed", "found": ["drive-c-case-insensitive"]},
    {"name": "codepage", "what": "Japanese resources; crashes off code page 932",
     "files": {"codepage.exe": None}, "portable": True,
     "class": "crash", "outcome": "fixed", "found": ["locale-ja"]},
    {"name": "vdesktop", "what": "refuses a screen larger than 1024x768",
     "files": {"vdesktop.exe": None}, "portable": True,
     "class": "exited-with-error", "outcome": "fixed", "found": ["virtual-desktop-1024x768"]},
    {"name": "dpi", "what": "refuses fewer than 120 DPI",
     "files": {"dpi.exe": None}, "portable": True,
     "class": "exited-with-error", "outcome": "fixed", "found": ["dpi-120"]},
]

# DLLs of Wine's that a fault is injected into, in this order, when the
# main program imports one of them.
INJECTABLE = ["comctl32.dll", "comdlg32.dll", "shlwapi.dll", "uxtheme.dll", "oleaut32.dll",
              "ole32.dll", "shell32.dll", "version.dll", "winmm.dll", "imm32.dll", "ws2_32.dll"]

WINE_DLL_DIRECTORIES = [
    "/usr/lib64/wine/x86_64-windows", "/usr/lib64/wine/i386-windows", "/usr/lib/wine/x86_64-windows",
    "/usr/lib/wine/i386-windows", "/usr/lib/x86_64-linux-gnu/wine/x86_64-windows",
    "/usr/lib/i386-linux-gnu/wine/i386-windows",
]

log = apps.log
run = apps.run


class Checks:
    def __init__(self) -> None:
        self.failed: list[str] = []

    def check(self, where: str, what: str, ok: bool, detail: str = "") -> bool:
        line = f"{'PASS' if ok else 'FAIL'} {where}: {what}"
        if not ok and detail:
            line += "\n       " + detail.strip().replace("\n", "\n       ")[:3000]
        log(line)
        if not ok:
            self.failed.append(f"{where}: {what}")
        return ok


def content_hash(roots: list[pathlib.Path]) -> str:
    """Names, kinds, permissions, sizes and contents of everything under
    the roots; links by their targets. Times are left out: a restored
    backup does not keep sub-second modification times."""
    digest = hashlib.sha256()
    for root in roots:
        digest.update(f"root {root}\n".encode())
        if not root.exists():
            digest.update(b"absent\n")
            continue
        for directory, names, files in os.walk(root):
            names.sort()
            for name in sorted(names + files):
                path = pathlib.Path(directory) / name
                info = path.lstat()
                relative = str(path.relative_to(root))
                if stat.S_ISLNK(info.st_mode):
                    digest.update(f"l {relative} {os.readlink(path)}\n".encode())
                elif stat.S_ISDIR(info.st_mode):
                    digest.update(f"d {relative} {info.st_mode & 0o7777:o}\n".encode())
                elif stat.S_ISREG(info.st_mode):
                    digest.update(f"f {relative} {info.st_mode & 0o7777:o} {info.st_size} ".encode())
                    digest.update(apps.sha256_of(path).encode() + b"\n")
                else:
                    digest.update(f"o {relative}\n".encode())
            names[:] = [name for name in names if not (pathlib.Path(directory) / name).is_symlink()]
    return digest.hexdigest()


def pe_imports(path: pathlib.Path) -> list[str]:
    """The names of the DLLs a PE program imports, in lower case."""
    data = path.read_bytes()
    if data[:2] != b"MZ":
        return []
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        return []
    sections_count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + (112 if magic == 0x20B else 96)
    import_rva = struct.unpack_from("<I", data, directories + 8)[0]
    sections = []
    for index in range(sections_count):
        at = optional + optional_size + index * 40
        virtual_size, address, raw_size, raw_pointer = struct.unpack_from("<IIII", data, at + 8)
        sections.append((address, max(virtual_size, raw_size), raw_pointer))

    def offset(rva: int) -> int | None:
        for address, size, raw in sections:
            if address <= rva < address + size:
                return rva - address + raw
        return None

    names = []
    at = offset(import_rva) if import_rva else None
    while at is not None and at + 20 <= len(data) and len(names) < 256:
        name_rva = struct.unpack_from("<I", data, at + 12)[0]
        if name_rva == 0:
            break
        name_at = offset(name_rva)
        if name_at is None:
            break
        names.append(data[name_at:data.index(b"\0", name_at)].decode("ascii", "replace").lower())
        at += 20
    return names


def wine_builtins() -> set[str]:
    found = set()
    for directory in WINE_DLL_DIRECTORIES:
        try:
            found.update(name.lower()[:-4] for name in os.listdir(directory) if name.lower().endswith(".dll"))
        except OSError:
            pass
    return found


class Driver:
    def __init__(self, arguments: argparse.Namespace, checks: Checks, work: pathlib.Path,
                 world: apps.World) -> None:
        self.arguments = arguments
        self.checks = checks
        self.work = work
        self.world = world
        self.catalogue = pathlib.Path(arguments.tur_catalogue).resolve()

    def tur(self, arguments: list[str], timeout: int = 1800) -> tuple[subprocess.CompletedProcess, float]:
        started = time.monotonic()
        done = self.world.tur_run(arguments, timeout=timeout)
        return done, round(time.monotonic() - started, 1)

    def tur_json(self, arguments: list[str], timeout: int = 1800) -> tuple[int, dict, str, float]:
        done, seconds = self.tur(arguments, timeout)
        try:
            document = json.loads(done.stdout)
        except json.JSONDecodeError:
            document = {}
        return done.returncode, document, done.stderr, seconds

    def paths(self, app_id: str) -> tuple[pathlib.Path, pathlib.Path]:
        base = self.world.data / "tur/runtime/prefixes" / app_id
        return base / "prefix", base / "home"

    def autopilot(self, app_id: str, check: pathlib.Path | None, evidence: pathlib.Path,
                  label: str) -> tuple[int, dict, float]:
        arguments = ["app", "autopilot", app_id, "--json"]
        if check is not None:
            arguments += ["--check", str(check)]
        status, document, stderr, seconds = self.tur_json(arguments)
        (evidence / f"{label}.json").write_text(json.dumps(document, indent=2) + "\n")
        (evidence / f"{label}.stderr.txt").write_text(stderr)
        result = document.get("result", {})
        if result:
            stamp = result.get("result_id", "@").split("@", 1)[1]
            kept = self.world.data / "tur/runtime/autopilot" / app_id / stamp
            apps.copy_evidence(kept, evidence / f"{label}-files")
            log(f"  {label}: {result.get('outcome')} in {seconds} s, "
                f"{len(result.get('attempts', [])) - 1} attempts, found "
                f"{[found['label'] for found in result.get('found', [])]}")
        return status, result, seconds

    def validate_record(self, name: str, app_id: str, record: pathlib.Path, found: list[str],
                        evidence: pathlib.Path) -> dict:
        """Appends the record to a scratch catalogue entry; tur-catalogue must
        accept it and route the settings."""
        directory = self.work / "catalogue" / name
        shutil.rmtree(directory, ignore_errors=True)
        directory.mkdir(parents=True)
        entry_id = f"org.tur-autopilot.{name}"
        text = (
            "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"
            f"schema_version = 1\nid = \"{entry_id}\"\nname = \"Autopilot case {name}\"\n"
            "publisher = \"TUR\"\ncategory = \"utility\"\n\n[[editions]]\nkind = \"windows\"\n"
            "obtain = \"publisher-page\"\ndownload_page = \"https://example.org/download\"\n\n"
            + record.read_text()
        )
        (directory / f"{entry_id}.toml").write_text(text)
        host = self.work / "catalogue" / "host.json"
        host.write_text(json.dumps({"arch": "x86_64", "flatpak": True, "distro_containers": True,
                                    "wine": True, "proton": True, "android_runtime": False,
                                    "arm_translation": False, "google_play": False,
                                    "windows_guest": False, "windows_licence": False,
                                    "darling_enabled": False, "gpu": "integrated",
                                    "memory_mib": 16384}))
        validated = run([str(self.catalogue), "validate", str(directory)])
        routed = run([str(self.catalogue), "--catalogue", str(directory), "route", entry_id,
                      "--host", str(host), "--json"])
        (evidence / "catalogue-validate.txt").write_text(validated.stdout + validated.stderr)
        (evidence / "catalogue-route.json").write_text(routed.stdout + routed.stderr)
        where = f"{name}: catalogue"
        self.checks.check(where, "tur-catalogue validate accepts the record", validated.returncode == 0,
                          validated.stdout + validated.stderr)
        settings = None
        try:
            options = json.loads(routed.stdout)["options"]
            wine = next(option for option in options if option["route"] == "wine")
            settings = wine.get("settings")
            self.checks.check(where, "the router keeps the wine route's rating unknown",
                              wine["rating"] == "unknown", json.dumps(wine)[:500])
        except (json.JSONDecodeError, KeyError, StopIteration) as error:
            self.checks.check(where, "tur-catalogue route answers", False, f"{error}: {routed.stdout[:500]}")
        self.checks.check(where, "the router shows the settings beside the wine route",
                          settings is not None and settings.get("remedies") == found, json.dumps(settings))
        return {"validated": validated.returncode == 0, "settings": settings}

    def case(self, case: dict, work_label: str = "") -> dict:
        name = case["name"] + work_label
        app_id = f"tur.test.autopilot-{name}"
        evidence = self.arguments.evidence / "cases" / name
        evidence.mkdir(parents=True, exist_ok=True)
        row: dict = {"case": name, "what": case["what"], "app_id": app_id, "expected": {
            "class": case["class"], "outcome": case["outcome"], "found": case["found"]}}
        where = name
        log(f"\n=== {name}: {case['what']} ===")
        built = self.arguments.build_dir / case["name"]
        # The program, as a person would get it: one file, or an archive.
        if case.get("portable"):
            installer = built / next(iter(case["files"]))
            install = ["app", "install", str(installer), "--portable", "--app-id", app_id, "--json"]
        else:
            installer = self.work / "packages" / f"{name}.zip"
            installer.parent.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(installer, "w") as archive:
                for member, content in case["files"].items():
                    if content is None:
                        archive.write(built / member, member)
                    else:
                        archive.writestr(member, content)
            install = ["app", "install", str(installer), "--app-id", app_id, "--json"]
        status, installed, stderr, seconds = self.tur_json(install)
        row["install_seconds"] = seconds
        if not self.checks.check(where, "the program installs", status == 0, stderr):
            return row
        check = self.work / "checks" / f"{name}.json"
        check.parent.mkdir(parents=True, exist_ok=True)
        check.write_text(json.dumps(CONSOLE_CHECK, indent=2) + "\n")
        prefix, home = self.paths(app_id)

        # 1-3. The search, on copies only.
        before = content_hash([prefix, home])
        status, result, seconds = self.autopilot(app_id, check, evidence, "autopilot")
        after = content_hash([prefix, home])
        found = [remedy["label"] for remedy in result.get("found", [])]
        failure = result.get("failure") or {}
        attempts = max(len(result.get("attempts", [])) - 1, 0)
        row.update({"seconds": seconds, "outcome": result.get("outcome"), "class": failure.get("class"),
                    "code": failure.get("code"), "found": found, "attempts": attempts,
                    "snapshot": result.get("snapshot"), "exit_status": status,
                    "result_id": result.get("result_id"),
                    "attempt_seconds": [round(sum(a["timings"].values()), 1) for a in result.get("attempts", [])]})
        self.checks.check(where, f"the failure is {case['class']}", failure.get("class") == case["class"],
                          json.dumps(failure)[:1500])
        self.checks.check(where, f"the outcome is {case['outcome']}", result.get("outcome") == case["outcome"],
                          json.dumps(result.get("attempts", []))[:3000])
        self.checks.check(where, f"the remedies found are {case['found']}", sorted(found) == sorted(case["found"]),
                          str(found))
        expected_status = 0 if case["outcome"] == "fixed" else 10
        self.checks.check(where, f"the exit status is {expected_status}", status == expected_status, str(status))
        self.checks.check(where, "the real prefix is byte for byte what it was (content hash)", before == after)
        self.checks.check(where, "the autopilot's own fingerprints agree",
                          (result.get("real_prefix") or {}).get("unchanged") is True)
        if case["outcome"] == "unsuppliable":
            unsuppliable = result.get("unsuppliable") or {}
            self.checks.check(where, "the Windows guest is suggested", unsuppliable.get("route") == "windows-guest",
                              json.dumps(unsuppliable))
            self.checks.check(where, "no remedy was tried", attempts == 0, str(attempts))
            row["suggestion"] = unsuppliable.get("route")
            status, _, _, _ = self.tur_json(["app", "autopilot", "--apply", result.get("result_id", ""), "--json"])
            self.checks.check(where, "--apply refuses a result without a remedy", status == 6, str(status))
            return row
        if result.get("outcome") != "fixed":
            return row

        # 4. The shareable record.
        stamp = result["result_id"].split("@", 1)[1]
        record = self.world.data / "tur/runtime/autopilot" / app_id / stamp / "record.toml"
        if self.checks.check(where, "a compatibility record was written", record.is_file()):
            row["catalogue"] = self.validate_record(name, app_id, record, found, evidence)

        # 5. The person applies it.
        before_apply = content_hash([prefix, home])
        status, applied, stderr, seconds = self.tur_json(["app", "autopilot", "--apply", result["result_id"], "--json"])
        row["apply_seconds"] = seconds
        (evidence / "apply.json").write_text(json.dumps(applied, indent=2) + "\n")
        self.checks.check(where, "--apply succeeds", status == 0, json.dumps(applied)[:3000] + stderr)
        backup = (applied.get("applied") or {}).get("backup", "")
        self.checks.check(where, "a backup was taken first",
                          bool(backup) and (self.world.data / "tur/runtime/backups" / app_id / f"{backup}.tar.gz").is_file(),
                          backup)
        launched, seconds = self.tur(["app", "launch", app_id], timeout=180)
        (evidence / "launch-after-apply.txt").write_text(launched.stdout + launched.stderr)
        row["launch_after_apply"] = launched.returncode
        self.checks.check(where, "app launch now passes the program's own check",
                          launched.returncode == 0 and "TUR-CHECK-PASS" in launched.stdout,
                          launched.stdout[-1500:] + launched.stderr[-1500:])

        # 6. Unchanged, it passes now.
        status, again, seconds = self.autopilot(app_id, check, evidence, "autopilot-after-apply")
        row["after_apply"] = again.get("outcome")
        self.checks.check(where, "the autopilot now passes the application unchanged",
                          again.get("outcome") == "passed-without-remedy", json.dumps(again.get("attempts", []))[:2000])

        # 7. The person undoes it.
        status, reverted, stderr, seconds = self.tur_json(["app", "autopilot", app_id, "--revert", "--json"])
        self.checks.check(where, "--revert succeeds", status == 0, stderr)
        self.checks.check(where, "after --revert the prefix is what it was before --apply",
                          content_hash([prefix, home]) == before_apply)
        status, listed, _, _ = self.tur_json(["app", "autopilot", app_id, "--results", "--json"])
        (evidence / "results.json").write_text(json.dumps(listed, indent=2) + "\n")
        self.checks.check(where, "--results lists both runs and no applied remedy",
                          len(listed.get("results", [])) == 2 and listed.get("applied") is None,
                          json.dumps(listed)[:1500])
        return row

    def real(self, app: dict, builtins: set[str]) -> dict:
        app_id = app["app_id"]
        where = f"real {app['id']}"
        evidence = self.arguments.evidence / "real" / app["id"]
        evidence.mkdir(parents=True, exist_ok=True)
        row: dict = {"case": app["id"], "what": f"{app['name']} {app['version']}", "app_id": app_id}
        log(f"\n=== {app['name']} {app['version']} ({app['id']}) ===")
        installer = apps.download(self.world, app, row)
        if not self.checks.check(where, "the pinned installer was downloaded", installer is not None,
                                 json.dumps(row.get("download"))):
            return row
        arguments = ["app", "install", str(installer), "--app-id", app_id, "--json", *app["install_options"]]
        if app.get("main_executable"):
            arguments += ["--main-exe", app["main_executable"]]
        status, installed, stderr, seconds = self.tur_json(arguments)
        row["install_seconds"] = seconds
        if not self.checks.check(where, "it installs", status == 0, stderr):
            return row
        record = installed.get("record", {})
        main = pathlib.Path(record.get("main_executable", ""))
        exe = main.name
        prefix, home = self.paths(app_id)

        # Unchanged, it passes.
        status, baseline, seconds = self.autopilot(app_id, None, evidence, "autopilot-unchanged")
        row["unchanged"] = {"outcome": baseline.get("outcome"), "seconds": seconds,
                            "snapshot": baseline.get("snapshot")}
        self.checks.check(where, "unchanged, it passes its check (a window of its own)",
                          baseline.get("outcome") == "passed-without-remedy",
                          json.dumps(baseline.get("attempts", []))[:2000])

        # The fault: one DLL its main program imports, disabled for it.
        imports = pe_imports(main)
        beside = {path.name.lower() for path in main.parent.glob("*") if path.suffix.lower() == ".dll"}
        target = next((dll for dll in INJECTABLE if dll in imports), None)
        if target is None:
            target = next((dll for dll in imports if dll in beside), None)
        row["imports"] = imports
        if not self.checks.check(where, "a DLL to disable was found among its imports", target is not None,
                                 str(imports)):
            return row
        base = target[:-4]
        expected = f"dll-builtin:{base}" if base in builtins else f"dll-native-builtin:{base}"
        row["fault"] = f"{base} disabled for {exe}"
        row["expected"] = {"class": "dll-missing", "found": [expected]}
        env = dict(os.environ, WINEPREFIX=str(prefix), HOME=str(home), WINEDEBUG="-all",
                   WINEDLLOVERRIDES="mscoree,mshtml=;winemenubuilder.exe=")
        env.pop("DISPLAY", None)
        key = f"HKCU\\Software\\Wine\\AppDefaults\\{exe}\\DllOverrides"
        injected = run(["wine", "reg", "add", key, "/v", base, "/d", "", "/f"], env=env, timeout=300)
        run(["wineserver", "-w"], env=env, timeout=120)
        self.checks.check(where, f"the fault was injected ({row['fault']})", injected.returncode == 0,
                          injected.stdout + injected.stderr)

        status, result, seconds = self.autopilot(app_id, None, evidence, "autopilot-fault")
        found = [remedy["label"] for remedy in result.get("found", [])]
        failure = result.get("failure") or {}
        row.update({"seconds": seconds, "outcome": result.get("outcome"), "class": failure.get("class"),
                    "code": failure.get("code"), "found": found,
                    "attempts": max(len(result.get("attempts", [])) - 1, 0),
                    "snapshot": result.get("snapshot"),
                    "attempt_seconds": [round(sum(a["timings"].values()), 1) for a in result.get("attempts", [])]})
        self.checks.check(where, "the failure is dll-missing", failure.get("class") == "dll-missing",
                          json.dumps(failure)[:1500])
        self.checks.check(where, f"the remedy found is {expected}", found == [expected], str(found))
        self.checks.check(where, "the real prefix was not changed",
                          (result.get("real_prefix") or {}).get("unchanged") is True)
        if result.get("outcome") == "fixed":
            status, applied, stderr, seconds = self.tur_json(["app", "autopilot", "--apply", result["result_id"], "--json"])
            row["apply_seconds"] = seconds
            (evidence / "apply.json").write_text(json.dumps(applied, indent=2) + "\n")
            self.checks.check(where, "--apply succeeds", status == 0, json.dumps(applied)[:3000] + stderr)
            status, again, seconds = self.autopilot(app_id, None, evidence, "autopilot-after-apply")
            row["after_apply"] = again.get("outcome")
            self.checks.check(where, "after --apply it passes unchanged", again.get("outcome") == "passed-without-remedy",
                              json.dumps(again.get("attempts", []))[:2000])
        done, seconds = self.tur(["app", "uninstall", app_id, "--remove-backups", "--json"])
        row["uninstall_seconds"] = seconds
        return row


def table(rows: list[dict], heading: str) -> list[str]:
    lines = [f"### {heading}", "",
             "| Case | What is wrong | Failure class (code) | Remedy found | Attempts | Autopilot time | Copy | Apply, then |",
             "|---|---|---|---|---|---|---|---|"]
    for row in rows:
        snapshot = row.get("snapshot") or {}
        copy = f"{snapshot.get('method', '-')}, {snapshot.get('seconds', 0):.1f} s" if snapshot else "-"
        found = " + ".join(f"`{label}`" for label in row.get("found") or []) or (
            "none: needs a Microsoft component; Windows guest suggested" if row.get("suggestion") else "none")
        after = row.get("after_apply") or "-"
        lines.append(
            f"| {row['case']} | {row['what']} | {row.get('class') or '-'} ({row.get('code') or '-'}) | {found} | "
            f"{row.get('attempts', '-')} | {row.get('seconds', '-')} s | {copy} | {after} |")
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-runtime", required=True)
    parser.add_argument("--tur-catalogue", required=True)
    parser.add_argument("--build-dir", required=True, type=pathlib.Path)
    parser.add_argument("--work-dir", required=True, type=pathlib.Path)
    parser.add_argument("--reflink-work-dir", type=pathlib.Path)
    parser.add_argument("--evidence", required=True, type=pathlib.Path)
    parser.add_argument("--apps", type=pathlib.Path)
    parser.add_argument("--real", default="", help="comma-separated ids of apps.json")
    parser.add_argument("--downloads", type=pathlib.Path)
    parser.add_argument("--only", default="", help="comma-separated case names")
    arguments = parser.parse_args()
    arguments.evidence = arguments.evidence.resolve()
    arguments.build_dir = arguments.build_dir.resolve()
    arguments.evidence.mkdir(parents=True, exist_ok=True)

    checks = Checks()
    environment = apps.describe_environment()
    (arguments.evidence / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    processes = apps.start_display(arguments.evidence)
    rows: list[dict] = []
    reflink_rows: list[dict] = []
    real_rows: list[dict] = []
    try:
        work = arguments.work_dir.resolve()
        world = apps.World(arguments.tur_runtime, work, arguments.evidence,
                           downloads=arguments.downloads.resolve() if arguments.downloads else None)
        driver = Driver(arguments, checks, work, world)
        cases = [case for case in CASES if not arguments.only or case["name"] in arguments.only.split(",")]
        for case in cases:
            try:
                rows.append(driver.case(case))
            except Exception as error:  # noqa: BLE001 - one case must not stop the others
                traceback.print_exc()
                checks.check(case["name"], "the harness ran the case", False, str(error))

        # The assistant's two tools, as P@x would call them.
        for label, command in [
            ("pax-plan", ["app", "autopilot", "--plan", "--json", "--failure=dll-init-failed", "--",
                          "tur.test.autopilot-dllover"]),
            ("pax-results", ["app", "autopilot", "--results", "--json", "--", "tur.test.autopilot-dllover"]),
        ]:
            status, document, stderr, _ = driver.tur_json(command)
            (arguments.evidence / f"{label}.json").write_text(json.dumps(document, indent=2) + "\n")
            checks.check("assistant tools", f"{label} answers in JSON", status == 0 and bool(document), stderr)

        if arguments.reflink_work_dir:
            reflink = arguments.reflink_work_dir.resolve()
            world_b = apps.World(arguments.tur_runtime, reflink, arguments.evidence)
            driver_b = Driver(arguments, checks, reflink, world_b)
            for case in [case for case in CASES if case["name"] in ("winver", "dllover")]:
                try:
                    row = driver_b.case(case, work_label="-btrfs")
                    reflink_rows.append(row)
                    checks.check(row["case"], "the copies were copy-on-write (reflink)",
                                 (row.get("snapshot") or {}).get("method") == "reflink", json.dumps(row.get("snapshot")))
                except Exception as error:  # noqa: BLE001
                    traceback.print_exc()
                    checks.check(case["name"] + "-btrfs", "the harness ran the case", False, str(error))

        if arguments.apps and arguments.real:
            builtins = wine_builtins()
            known = {app["id"]: app for app in json.loads(arguments.apps.read_text())["applications"]}
            for identifier in arguments.real.split(","):
                try:
                    real_rows.append(driver.real(known[identifier], builtins))
                except Exception as error:  # noqa: BLE001
                    traceback.print_exc()
                    checks.check(identifier, "the harness ran the application", False, str(error))
    finally:
        for process in processes:
            process.terminate()

    lines = ["## Compatibility autopilot", "",
             f"Ubuntu's Wine on a hosted runner ({environment.get('wine', '?')}); Xvfb 1280x800, no GPU. "
             "Every attempt ran in a throwaway copy of the prefix, in the sandbox, without network.", ""]
    lines += table(rows, "Deliberately broken programs (ext4 with the casefold feature)")
    if reflink_rows:
        lines += [""] + table(reflink_rows, "The same programs with the data directory on btrfs (copy-on-write)")
    if real_rows:
        lines += [""] + table(real_rows, "Real applications, one imported DLL disabled for the main program")
        lines += ["", "| Application | Unchanged: outcome, time | Fault | Install | Uninstall |", "|---|---|---|---|---|"]
        for row in real_rows:
            unchanged = row.get("unchanged") or {}
            lines.append(f"| {row['case']} | {unchanged.get('outcome', '-')}, {unchanged.get('seconds', '-')} s | "
                         f"{row.get('fault', '-')} | {row.get('install_seconds', '-')} s | "
                         f"{row.get('uninstall_seconds', '-')} s |")
    lines += ["", f"Checks failed: {len(checks.failed)}"] + [f"- {item}" for item in checks.failed]
    (arguments.evidence / "autopilot.md").write_text("\n".join(lines) + "\n")
    (arguments.evidence / "autopilot.json").write_text(json.dumps(
        {"cases": rows, "reflink": reflink_rows, "real": real_rows, "failed": checks.failed,
         "environment": environment}, indent=2) + "\n")
    log("\n".join(lines))
    return 1 if checks.failed else 0


if __name__ == "__main__":
    sys.exit(main())

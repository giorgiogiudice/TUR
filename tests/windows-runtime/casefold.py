#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Case-insensitive prefixes (ext4 case folding), measured with a test program.

Run by .github/workflows/windows-runtime.yml on a hosted Ubuntu runner, after
the job has made an ext4 file system with the casefold feature on a loop
device. It is never run on a development machine (AGENTS.md).

    casefold.py --tur-runtime BINARY --program casefold.exe \
        --casefold-root DIR --plain-root DIR --evidence DIR [--rounds N]

--casefold-root is a directory on the file system with the casefold feature;
--plain-root one on the runner's own disk, which has not got it.

What it does, through tur-runtime only:

  1. installs casefold.exe (tests/windows-runtime/casefold.c) as a portable
     program three times: with --casefold and with --no-casefold on the
     case-folding file system, and with --casefold on the runner's disk.
     The record is compared with what `lsattr -d` says of drive_c;
  2. "semantics": what names in another case resolve to, in each prefix;
  3. "plugins": a zip archive that spells one directory two ways
     (App/Plugins/one.txt, App/plugins/two.txt), installed with and without
     case folding, and the files asked for through the other spelling;
  4. "bench": timings of creating, opening and searching for files under
     names in the same and in another case, with and without case folding,
     in alternating order, several rounds.

Exit status 1 when something that must hold does not: an installation
failing, a record that disagrees with the directory, a prefix on the
runner's disk that fails instead of falling back, or a lookup Windows
programs depend on (a file found under another case; a second file of the
same name refused) failing in either mode. Everything else is reported as
measured.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import statistics
import subprocess
import sys
import zipfile

TAG = "tur-casefold: "
SIZES = (100, 1000, 5000)
OPERATIONS = 2000


def log(text: str) -> None:
    print(text, flush=True)


def run(command: list[str], env: dict[str, str], timeout: int = 900) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(command, env=env, capture_output=True, text=True, errors="replace",
                              timeout=timeout)
    except subprocess.TimeoutExpired as error:
        out = error.stdout.decode(errors="replace") if isinstance(error.stdout, bytes) else (error.stdout or "")
        return subprocess.CompletedProcess(command, 124, out, f"[timed out after {timeout}s]")


def tagged(output: str) -> list[dict]:
    found = []
    for line in output.splitlines():
        line = line.rstrip("\r")
        if line.startswith(TAG):
            try:
                found.append(json.loads(line[len(TAG):]))
            except json.JSONDecodeError:
                found.append({"unparsed": line})
    return found


class Results:
    def __init__(self) -> None:
        self.problems: list[str] = []

    def must(self, ok: bool, what: str) -> bool:
        log(f"{'PASS' if ok else 'FAIL'}  {what}")
        if not ok:
            self.problems.append(what)
        return ok


R = Results()


class World:
    def __init__(self, tur: str, root: pathlib.Path) -> None:
        self.tur = tur
        self.root = root
        self.data = root / "xdg-data"
        self.state = root / "xdg-state"
        for directory in (self.data, self.state):
            directory.mkdir(parents=True, exist_ok=True)

    def env(self) -> dict[str, str]:
        env = dict(os.environ)
        for name in ("DISPLAY", "WAYLAND_DISPLAY"):
            env.pop(name, None)
        env["XDG_DATA_HOME"] = str(self.data)
        env["XDG_STATE_HOME"] = str(self.state)
        return env

    def tur_run(self, arguments: list[str], timeout: int = 900) -> subprocess.CompletedProcess:
        done = run([self.tur, *arguments], self.env(), timeout)
        log(f"$ tur-runtime {' '.join(arguments)}\n  -> exit status {done.returncode}")
        if done.returncode != 0:
            log("  " + "\n  ".join((done.stderr or done.stdout).strip().splitlines()[-12:]))
        return done

    def drive_c(self, app_id: str) -> pathlib.Path:
        return self.data / "tur/runtime/prefixes" / app_id / "prefix/drive_c"


def lsattr_folded(path: pathlib.Path) -> bool | None:
    done = subprocess.run(["lsattr", "-d", str(path)], capture_output=True, text=True,
                          env=dict(os.environ, LC_ALL="C"))
    if done.returncode != 0 or not done.stdout.strip():
        return None
    return "F" in done.stdout.split()[0]


def install(world: World, source: pathlib.Path, app_id: str, mode: str, portable: bool) -> dict:
    """Installs and returns what was recorded and measured about drive C:."""
    arguments = ["app", "install", str(source), "--app-id", app_id, f"--{mode}", "--json"]
    if portable:
        arguments.append("--portable")
    done = world.tur_run(arguments)
    try:
        report = json.loads(done.stdout)
    except json.JSONDecodeError:
        report = {}
    record = (report.get("record") or {}).get("case_folding")
    folded = lsattr_folded(world.drive_c(app_id))
    outcome = {"app_id": app_id, "mode": mode, "status": done.returncode, "record": record,
               "lsattr_casefolded": folded, "warnings": report.get("warnings"),
               "seconds": report.get("seconds")}
    R.must(done.returncode == 0, f"{app_id}: installed with --{mode}")
    R.must(record is not None and record.get("casefolded") == folded,
           f"{app_id}: the record ({record}) agrees with lsattr (casefolded: {folded})")
    return outcome


def launch(world: World, app_id: str, arguments: list[str], timeout: int = 1800) -> list[dict]:
    done = world.tur_run(["app", "launch", app_id, "--", *arguments], timeout=timeout)
    lines = tagged(done.stdout)
    if done.returncode != 0 or not lines:
        R.must(False, f"{app_id}: casefold.exe {arguments[0]} ran (exit status {done.returncode})")
    return lines


def uninstall(world: World, app_id: str) -> None:
    done = world.tur_run(["app", "uninstall", app_id, "--json"])
    R.must(done.returncode == 0, f"{app_id}: removed, nothing left")


def semantics(world: World, app_id: str) -> dict[str, dict]:
    return {line.get("test", "?"): line for line in launch(world, app_id, ["semantics", "C:\\tur-casefold"])}


def plugins_archive(program: pathlib.Path, target: pathlib.Path) -> pathlib.Path:
    """A portable application whose archive spells one directory two ways,
    as archives put together on case-sensitive systems sometimes do."""
    with zipfile.ZipFile(target, "w") as archive:
        archive.write(program, "App/cftest.exe")
        archive.writestr("App/Plugins/one.txt", "one\r\n")
        archive.writestr("App/plugins/two.txt", "two\r\n")
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-runtime", required=True)
    parser.add_argument("--program", required=True, type=pathlib.Path)
    parser.add_argument("--casefold-root", required=True, type=pathlib.Path)
    parser.add_argument("--plain-root", required=True, type=pathlib.Path)
    parser.add_argument("--evidence", required=True, type=pathlib.Path)
    parser.add_argument("--rounds", type=int, default=3)
    arguments = parser.parse_args()
    tur = str(pathlib.Path(arguments.tur_runtime).resolve())
    program = arguments.program.resolve()
    evidence = arguments.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    folding = World(tur, arguments.casefold_root.resolve())
    plain = World(tur, arguments.plain_root.resolve())
    results: dict = {"installs": [], "semantics": {}, "plugins": {}, "bench": []}

    log("\n=== Install the test program ===")
    prefixes = {"on": (folding, "local.casefold-on", "casefold"),
                "off": (folding, "local.casefold-off", "no-casefold"),
                "plain-disk": (plain, "local.casefold-plain", "casefold")}
    for label, (world, app_id, mode) in prefixes.items():
        outcome = install(world, program, app_id, mode, portable=True)
        outcome["label"] = label
        results["installs"].append(outcome)
    on, off, fallback = results["installs"]
    R.must(on["lsattr_casefolded"] is True, "with --casefold on the case-folding file system, drive_c has F")
    R.must(off["lsattr_casefolded"] is False and (off["record"] or {}).get("reason") == "not-requested",
           "with --no-casefold, drive_c has no F and the record says it was not asked for")
    R.must(fallback["status"] == 0 and (fallback["record"] or {}).get("requested") is True
           and fallback["lsattr_casefolded"] is not True,
           f"on the runner's own disk, --casefold falls back to a case-sensitive prefix and says why: "
           f"{fallback['record']}; warnings {fallback['warnings']}")

    log("\n=== Name semantics ===")
    for label, (world, app_id, _mode) in prefixes.items():
        results["semantics"][label] = semantics(world, app_id)
    for label in prefixes:
        found = results["semantics"][label]
        R.must(found.get("open-other-case", {}).get("same_file") is True,
               f"{label}: a file is found, and is the same file, under another case")
        R.must(found.get("create-new-other-case", {}).get("refused") is True,
               f"{label}: a second file whose name differs only in case is refused")

    log("\n=== An archive that spells a directory two ways ===")
    archive = plugins_archive(program, folding.root / "plugins-app.zip")
    for label, mode in (("on", "casefold"), ("off", "no-casefold")):
        app_id = f"local.plugins-{label}"
        outcome = install(folding, archive, app_id, mode, portable=False)
        outcome["label"] = f"plugins-{label}"
        results["installs"].append(outcome)
        results["plugins"][label] = launch(folding, app_id, ["plugins"])
        uninstall(folding, app_id)

    log("\n=== Lookups, timed ===")
    for round_number in range(1, arguments.rounds + 1):
        order = ("on", "off") if round_number % 2 else ("off", "on")
        for label in order:
            world, app_id, _mode = prefixes[label]
            for size in SIZES:
                lines = launch(world, app_id, ["bench", f"C:\\tur-bench-{round_number}", str(size), str(OPERATIONS)])
                for line in lines:
                    line.update(mode=label, round=round_number)
                    results["bench"].append(line)
    for label, (world, app_id, _mode) in prefixes.items():
        uninstall(world, app_id)

    results["problems"] = R.problems
    (evidence / "casefold.json").write_text(json.dumps(results, indent=2, ensure_ascii=False))
    (evidence / "casefold.md").write_text(render(results))
    log("\n" + render(results))
    return 1 if R.problems else 0


def render(results: dict) -> str:
    lines = ["### Case-insensitive prefixes: the test program", ""]
    lines += ["| Prefix | Asked | drive_c has F (lsattr) | Recorded reason | Warnings |", "|---|---|---|---|---|"]
    for outcome in results["installs"]:
        record = outcome.get("record") or {}
        warnings = ", ".join(warning.get("code", "?") for warning in outcome.get("warnings") or []) or "none"
        lines.append(f"| {outcome.get('label')} | --{outcome['mode']} | {outcome['lsattr_casefolded']} | "
                     f"{record.get('reason')} {('(' + record['detail'] + ')') if record.get('detail') else ''} | "
                     f"{warnings} |")
    lines += ["", "Name semantics (what each lookup observed):", "",
              "| Test | " + " | ".join(results["semantics"]) + " |",
              "|---|" + "---|" * len(results["semantics"])]
    tests = []
    for found in results["semantics"].values():
        tests += [name for name in found if name not in tests]
    for name in tests:
        cells = []
        for found in results["semantics"].values():
            entry = dict(found.get(name, {}))
            for key in ("test", "stored", "asked"):
                entry.pop(key, None)
            cells.append(json.dumps(entry, ensure_ascii=False).replace("|", "\\|"))
        lines.append(f"| {name} | " + " | ".join(cells) + " |")
    lines += ["", "Archive with App/Plugins/one.txt and App/plugins/two.txt:", "",
              "| Asked for | casefolded | not casefolded |", "|---|---|---|"]
    opened = {label: {line.get("asked"): line for line in lines_ if line.get("plugins") == "open"}
              for label, lines_ in results["plugins"].items()}
    for asked in opened.get("on", opened.get("off", {})):
        cells = [("found" if opened.get(label, {}).get(asked, {}).get("found") else
                  f"not found (error {opened.get(label, {}).get(asked, {}).get('error')})")
                 for label in ("on", "off")]
        lines.append(f"| {asked} | " + " | ".join(cells) + " |")
    for label, lines_ in results["plugins"].items():
        listing = [line.get("directories") for line in lines_ if line.get("plugins") == "listing"]
        lines.append(f"\nDirectories named like 'plugins' ({label}): {listing}")
    lines += ["", "Lookups, microseconds per operation (median over rounds; min to max):", "",
              "| Operation | Files | casefolded | not casefolded | ratio |", "|---|---|---|---|---|"]
    keyed: dict = {}
    for line in results["bench"]:
        if "bench" in line:
            keyed.setdefault((line["bench"], line["files"]), {}).setdefault(line["mode"], []).append(line)
    for (name, files), modes in sorted(keyed.items(), key=lambda item: (item[0][0], item[0][1])):
        cells, medians = [], {}
        for label in ("on", "off"):
            values = [entry["microseconds_per_operation"] for entry in modes.get(label, [])]
            ok = all(entry["succeeded"] == modes[label][0]["succeeded"] for entry in modes.get(label, []))
            if values:
                medians[label] = statistics.median(values)
                succeeded = modes[label][0]["succeeded"]
                cells.append(f"{medians[label]:.1f} ({min(values):.1f} to {max(values):.1f}); "
                             f"{succeeded} of {modes[label][0]['operations']} found{'' if ok else ', varied'}")
            else:
                cells.append("not measured")
        ratio = (f"{medians['off'] / medians['on']:.2f}x" if medians.get("on") and medians.get("off") else "")
        lines.append(f"| {name} | {files} | " + " | ".join(cells) + f" | {ratio} |")
    lines += ["", "ratio: not casefolded divided by casefolded; above 1 means case folding was faster.", ""]
    if results.get("problems"):
        lines += ["**Did not hold:**", ""] + [f"- {problem}" for problem in results["problems"]] + [""]
    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())

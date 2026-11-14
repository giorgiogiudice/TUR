#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Runs one of the experiment's readers with three sets of rights and
tabulates which facts each could read (docs/reports/exp-inwin-1.md):

  elevated     as the job runs: an administrator with a full token
  restricted   the same account without its administrator rights
               (runas /trustlevel:0x20000), as a program started by a
               person who has not answered a consent prompt
  standard     a new local account that is not an administrator, made for
               this run on the throwaway runner (CreateProcessWithLogonW)

    rights.py --script checker/check.py --name checker --out DIR

The readers only read. This script makes one local account on the runner.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import pathlib
import secrets
import shutil
import subprocess
import sys
import time

PUBLIC = pathlib.Path(r"C:\Users\Public\inwin")
ACCOUNT = "inwinstd"


def classify(row):
    if row.get("ok"):
        return "read"
    error = row.get("error", "")
    low = error.lower()
    if "permissionerror" in low or "denied" in low or "administrator" in low or "elevat" in low or "privilege" in low or "winerror 5" in low:
        return "refused"
    if "filenotfounderror" in low or "winerror 2" in low or "winerror 3" in low:
        return "absent"
    return "failed"


def wait_for(path, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if path.exists() and path.stat().st_size > 2:
            time.sleep(1)
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                pass
        time.sleep(2)
    return None


def run_standard(command_line, work):
    """Start a command as the standard account; wait for it to end."""

    class StartupInfo(ctypes.Structure):
        _fields_ = [("cb", wt.DWORD), ("reserved", wt.LPWSTR), ("desktop", wt.LPWSTR), ("title", wt.LPWSTR)] + \
                   [(name, wt.DWORD) for name in ("x", "y", "xs", "ys", "xc", "yc", "fill", "flags")] + \
                   [("show", wt.WORD), ("reserved2", wt.WORD), ("reserved3", ctypes.c_void_p)] + \
                   [(name, wt.HANDLE) for name in ("stdin", "stdout", "stderr")]

    class ProcessInformation(ctypes.Structure):
        _fields_ = [("process", wt.HANDLE), ("thread", wt.HANDLE), ("pid", wt.DWORD), ("tid", wt.DWORD)]

    password = "Tq7!" + secrets.token_urlsafe(18)
    subprocess.run(["net.exe", "user", ACCOUNT, "/delete"], capture_output=True)
    made = subprocess.run(["net.exe", "user", ACCOUNT, password, "/add"], capture_output=True, text=True)
    if made.returncode != 0:
        return f"the account could not be made: exit {made.returncode}"
    startup = StartupInfo()
    startup.cb = ctypes.sizeof(StartupInfo)
    information = ProcessInformation()
    advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
    buffer = ctypes.create_unicode_buffer(command_line)
    ok = advapi32.CreateProcessWithLogonW(ctypes.c_wchar_p(ACCOUNT), ctypes.c_wchar_p("."), ctypes.c_wchar_p(password), 1, None, buffer,
                                          0x08000000, None, ctypes.c_wchar_p(str(work)), ctypes.byref(startup), ctypes.byref(information))
    if not ok:
        return f"CreateProcessWithLogonW failed: error {ctypes.get_last_error()}"
    kernel32 = ctypes.WinDLL("kernel32")
    kernel32.WaitForSingleObject(wt.HANDLE(information.process), 240000)
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--script", required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--out", required=True)
    arguments = parser.parse_args()
    here = pathlib.Path(__file__).resolve().parent
    out = pathlib.Path(arguments.out)
    out.mkdir(parents=True, exist_ok=True)
    notes = {}

    # A copy every account can read, and a place each can write.
    if PUBLIC.exists():
        shutil.rmtree(PUBLIC, ignore_errors=True)
    shutil.copytree(here, PUBLIC / "src")
    (PUBLIC / "out").mkdir(parents=True, exist_ok=True)
    subprocess.run(["icacls.exe", str(PUBLIC), "/grant", "*S-1-5-32-545:(OI)(CI)M", "/T", "/Q"], capture_output=True)
    script = PUBLIC / "src" / arguments.script
    results = {}

    target = out / f"{arguments.name}-elevated.json"
    done = subprocess.run([sys.executable, str(script), "--out", str(target)], capture_output=True, text=True, timeout=900)
    print("elevated:", done.stdout.strip()[-300:], done.stderr.strip()[-600:])
    results["elevated"] = wait_for(target, 5)

    target = PUBLIC / "out" / f"{arguments.name}-restricted.json"
    command = f'"{sys.executable}" "{script}" --out "{target}"'
    done = subprocess.run(["runas.exe", "/trustlevel:0x20000", command], capture_output=True, text=True, timeout=120)
    notes["restricted_start"] = f"runas exit {done.returncode}"
    results["restricted"] = wait_for(target, 240 if done.returncode == 0 else 10)

    target = PUBLIC / "out" / f"{arguments.name}-standard.json"
    problem = run_standard(command.replace("-restricted.json", "-standard.json"), PUBLIC / "out")
    if problem:
        notes["standard_start"] = problem
    results["standard"] = wait_for(target, 10 if problem else 60)
    subprocess.run(["net.exe", "user", ACCOUNT, "/delete"], capture_output=True)

    for level, result in results.items():
        if result is not None:
            (out / f"{arguments.name}-{level}.json").write_text(json.dumps(result, indent=1), encoding="utf-8")

    names = []
    for result in results.values():
        for row in (result or {}).get("probes", []):
            if row["name"] not in names:
                names.append(row["name"])
    lines = [f"### {arguments.name}: what each set of rights could read", ""]
    for level, result in results.items():
        rights = next((row.get("value") for row in (result or {}).get("probes", []) if row["name"] == "rights"), None)
        lines.append(f"- **{level}**: {'NOT RUN (' + notes.get(level + '_start', 'no result') + ')' if result is None else json.dumps(rights)}")
    lines += ["", "| Fact | Elevated | Restricted | Standard account | Where it lives |", "|---|---|---|---|---|"]
    matrix = []
    for name in names:
        if name == "rights":
            continue
        cells = []
        where = ""
        for level in ("elevated", "restricted", "standard"):
            row = next((row for row in (results[level] or {}).get("probes", []) if row["name"] == name), None)
            cells.append("NOT RUN" if row is None else classify(row))
            where = where or (row or {}).get("where", "")
        matrix.append({"fact": name, "elevated": cells[0], "restricted": cells[1], "standard": cells[2], "where": where})
        lines.append(f"| {name} | {cells[0]} | {cells[1]} | {cells[2]} | `{where}` |")
    advice = (results.get("elevated") or {}).get("advice")
    if advice:
        lines += ["", "What the checker would say on this machine:", ""] + [f"- {line}" for line in advice]
    (out / f"{arguments.name}-rights.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (out / f"{arguments.name}-rights.json").write_text(json.dumps({"notes": notes, "matrix": matrix}, indent=1), encoding="utf-8")
    print("\n".join(lines))


if __name__ == "__main__":
    main()

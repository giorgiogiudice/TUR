#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The integration pieces and the safety rules, measured (CI only).

    integration.py --out DIR

Reads WINCLI_PREFIX (a Windows environment of its own, which it changes),
WINCLI_WINE and the build directory WINCLI_BUILD (hello.exe).

1. safety      every case of safety_cases.py judged by tur-winshell; the
               ones marked `run` then run for real with a mapped folder
               holding canary files, through the router and, for the
               recursive deletes, through plain Wine as well, to show what
               the rule prevents.
2. paths       winpath.py against Wine's own winepath, both directions,
               and arguments that embed a path.
3. bridge      a TUR command (a Linux program) used from cmd.exe: in a
               pipe, in for /f, in a batch file, with its exit code.
4. binfmt      ./hello.exe, ./hello.bat, ./args.cmd and ./hello.ps1 started
               by name from bash through binfmt_misc and tur-winshell.
"""

from __future__ import annotations

import argparse
import getpass
import json
import os
import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "winshell"))

import fixture  # noqa: E402
import safety_cases  # noqa: E402
from winpath import WinEnv  # noqa: E402

PREFIX = pathlib.Path(os.environ["WINCLI_PREFIX"])
WINE = os.environ.get("WINCLI_WINE", "wine")
BUILD = pathlib.Path(os.environ.get("WINCLI_BUILD", "/work/build"))
WINSHELL = str(HERE / "winshell" / "tur-winshell")
FIX = PREFIX / "drive_c" / "turfix"
MAPPED = PREFIX.parent / "mapped-documents"
ENV = {**os.environ, "TUR_WINPREFIX": str(PREFIX), "WINEPREFIX": str(PREFIX), "WINEDEBUG": "-all",
       "TUR_WINE": WINE, "TUR_PWSH": os.environ.get("WINCLI_PWSH", "pwsh"), "TUR_WINSHELL": WINSHELL,
       "TUR_WINCOMPAT": str(HERE / "compat" / "TurWinCompat")}
rows: list[dict] = []


def record(group: str, name: str, ok, detail: str = "") -> None:
    verdict = "PASS" if ok is True else "FAIL" if ok is False else str(ok)
    rows.append({"group": group, "test": name, "verdict": verdict, "detail": detail[:600]})
    print(f"[{group}] {verdict} {name} {detail[:200]}", flush=True)


def run(argv, timeout=180, cwd=None, env=None):
    try:
        done = subprocess.run(argv, capture_output=True, timeout=timeout, cwd=str(cwd or FIX), env=env or ENV,
                              check=False, stdin=subprocess.DEVNULL)
        return done.returncode, done.stdout.decode(errors="replace"), done.stderr.decode(errors="replace")
    except subprocess.TimeoutExpired:
        return None, "", "timed out"
    except OSError as err:
        return None, "", f"could not start: {err}"


def router(shell: str, line: str, timeout=180):
    return run([sys.executable, WINSHELL, "--shell", shell, "-c", line], timeout)


def plain_wine(line: str, timeout=120):
    env = {**ENV, "TURRUN_CMDLINE": f'cmd.exe /d /s /c "{line}"'}
    return run([WINE, r"C:\turrun\turrun.exe"], timeout, env=env)


def canary() -> None:
    """A folder of this computer, mapped as the profile's Documents."""
    if MAPPED.exists():
        shutil.rmtree(MAPPED)
    (MAPPED / "sub").mkdir(parents=True)
    (MAPPED / "canary.txt").write_text("keep me\n")
    (MAPPED / "sub" / "deep.txt").write_text("keep me too\n")
    users = WinEnv(str(PREFIX))._spelled(str(PREFIX / "drive_c"), "Users")
    documents = PREFIX / "drive_c" / users / getpass.getuser() / "Documents"
    if documents.is_symlink() or documents.is_file():
        documents.unlink()
    elif documents.exists():
        shutil.rmtree(documents)
    documents.parent.mkdir(parents=True, exist_ok=True)
    documents.symlink_to(MAPPED)


def canary_alive() -> bool:
    return (MAPPED / "canary.txt").is_file() and (MAPPED / "sub" / "deep.txt").is_file()


def safety() -> None:
    canary()
    fixture.build(FIX)
    wrong = 0
    for shell, line, action, rule, _run in safety_cases.CASES:
        code, out, err = run([sys.executable, WINSHELL, "--shell", shell, "--verdict", "-c", line], 60)
        try:
            verdict = json.loads(out)
        except ValueError:
            verdict = {"action": "error", "rule": "", "message": err[-300:]}
        ok = verdict["action"] == action and (not rule or verdict["rule"] == rule)
        wrong += not ok
        record("safety-verdict", f"{shell}: {line}", ok,
               f"expected {action} {rule}; got {verdict['action']} {verdict['rule']}")
    for shell, line, action, rule, do_run in safety_cases.CASES:
        if not do_run:
            continue
        canary()
        fixture.build(FIX)
        code, out, err = router(shell, line)
        refused = "tur-winshell: refused:" in err
        alive = canary_alive()
        if action == "refuse":
            record("safety-run", f"router, {shell}: {line}", refused and alive,
                   f"refused={refused} canary intact={alive} exit={code}")
        else:
            record("safety-run", f"router, {shell}: {line}", (not refused) and alive,
                   f"refused={refused} canary intact={alive} exit={code} {err.strip()[-200:]}")
        if rule == "R3" and shell == "cmd":
            canary()
            fixture.build(FIX)
            code, out, err = plain_wine(line)
            record("safety-hazard", f"plain Wine, no router: {line}",
                   "canary survived" if canary_alive() else "CANARY DELETED",
                   f"exit={code} (what the rule prevents: the mapped folder is the person's real folder)")
        if rule == "R1" and shell == "cmd":
            code, out, err = plain_wine(line)
            record("safety-hazard", f"plain Wine, no router: {line}", "ran" if code == 0 else "did not run",
                   f"exit={code} {(out + err).strip()[-200:]}")
    canary()


def paths() -> None:
    env = WinEnv(str(PREFIX))
    fixture.build(FIX)
    canary()
    user = getpass.getuser()

    def winepath(flag: str, value: str) -> str:
        code, out, err = run(["winepath", flag, value], 60)
        return out.strip()

    to_unix = [r"C:\turfix\notes.txt", r"C:\turfix\docs\sub\deep.txt", r"C:\TURFIX\DOCS\README.MD",
               r"C:\turfix\new folder\not yet.txt", rf"C:\users\{user}\Documents\canary.txt",
               r"C:\windows\system32", r"Z:\etc\hosts", r"C:/turfix/notes.txt", r"C:\turfix\docs\..\notes.txt"]
    for value in to_unix:
        mine = env.win_to_unix(value, str(FIX))
        theirs = winepath("-u", value)
        same = mine is not None and os.path.normpath(mine) == os.path.normpath(theirs)
        record("paths", f"to Linux: {value}", same, f"winpath: {mine} ; winepath: {theirs}")
    to_win = [str(FIX / "notes.txt"), str(FIX / "docs" / "sub"), "/etc/hosts", "/usr/bin/env"]
    for value in to_win:
        mine = env.unix_to_win(value)
        theirs = winepath("-w", value)
        record("paths", f"to Windows: {value}", (mine or "").lower() == theirs.lower(),
               f"winpath: {mine} ; winepath: {theirs}")
    # A mapped folder: TUR answers with the path a Windows program would use.
    mapped = str(MAPPED / "canary.txt")
    mine = env.unix_to_win(mapped) or ""
    back = env.win_to_unix(mine, str(FIX)) if mine else None
    record("paths", f"mapped folder to Windows: {mapped}",
           mine.lower().endswith("\\documents\\canary.txt") and back is not None
           and os.path.realpath(back) == os.path.realpath(mapped),
           f"winpath: {mine} (winepath: {winepath('-w', mapped)}); and back: {back}")
    args = [("--output=C:\\turfix\\o.txt", "--output=" + str(FIX / "o.txt")),
            ("/out:C:\\turfix\\o.txt", "/out:" + str(FIX / "o.txt")),
            ("NUL", "/dev/null"), ("https://example.com/a?b=c", "https://example.com/a?b=c"),
            ("%{http_code}", "%{http_code}"), ("docs\\readme.md", str(FIX / "docs" / "readme.md")),
            ("%USERPROFILE%\\Documents\\canary.txt",
             str(PREFIX / "drive_c" / env._spelled(str(PREFIX / "drive_c"), "Users") / user / "Documents"
                 / "canary.txt")),
            ("-sI", "-sI"), ("/b", "/b")]
    for given, expected in args:
        got = env.translate_arg(given, str(FIX))
        record("paths", f"argument: {given}", got == expected, f"got {got}; expected {expected}")
    # Through a real Linux tool: a Windows path in an argument reaches it.
    code, out, err = run([sys.executable, WINSHELL, "--argv", "curl", "-s", "-o", r"C:\turfix\page.html",
                          "https://example.com"], 120)
    record("paths", r"curl -s -o C:\turfix\page.html https://example.com (Linux curl)",
           (FIX / "page.html").is_file() and (FIX / "page.html").stat().st_size > 100, f"exit={code} {err[-200:]}")


def bridge() -> None:
    fixture.build(FIX)
    (FIX / "usesshim.bat").write_bytes(b"@echo off\r\ngetmac\r\necho after-getmac\r\n")
    tests = [
        ("pipe: tree /f /a | findstr deep", "tree /f /a | findstr deep", lambda c, o, e: "deep.txt" in o),
        ("pipe: ipconfig | findstr /i IPv4", "ipconfig | findstr /i IPv4", lambda c, o, e: "IPv4 Address" in o),
        ("for /f over a TUR command", "for /f \"delims=\" %i in ('getmac') do @echo [%i]",
         lambda c, o, e: "[Physical Address" in o),
        ("batch file calling a TUR command", "usesshim.bat",
         lambda c, o, e: "Physical Address" in o and "after-getmac" in o
         and o.index("Physical Address") < o.index("after-getmac")),
        ("sequence keeps order: getmac & echo after", "getmac & echo after",
         lambda c, o, e: "after" in o and o.index("Physical Address") < o.rindex("after")),
        ("exit code through the bridge: taskkill of nothing", "taskkill /f /im nosuchproc_tur.exe",
         lambda c, o, e: c == 128),
        ("errorlevel after a TUR command", "taskkill /f /im nosuchproc_tur.exe 2>nul & call echo rc=%errorlevel%",
         lambda c, o, e: "rc=128" in o),
    ]
    for name, line, check in tests:
        code, out, err = router("cmd", line)
        record("bridge", name, bool(check(code, out, err)), f"exit={code} out={out.strip()[-160:]!r} err={err.strip()[-160:]!r}")
    # R1 behind the lexical rule: a batch file hides the command from it.
    (FIX / "hidden.bat").write_bytes(b"@echo off\r\nformat D: /q\r\necho rc=%errorlevel%\r\n")
    code, out, err = router("cmd", "hidden.bat")
    record("bridge", "a batch file that calls format is stopped by the refusing program on PATH",
           "tur-winshell: refused:" in err and "rc=5" in out, f"exit={code} out={out.strip()[-100:]!r} err={err.strip()[-200:]!r}")


def binfmt() -> None:
    fixture.build(FIX)
    misc = pathlib.Path("/proc/sys/fs/binfmt_misc")
    if not (misc / "register").exists():
        subprocess.run(["mount", "-t", "binfmt_misc", "binfmt_misc", str(misc)], check=False)
    if not (misc / "register").exists():
        record("binfmt", "binfmt_misc is available", "NOT RUN", "could not mount binfmt_misc in this container")
        return
    interp = BUILD / "tur-run"
    interp.write_text(f"#!/bin/sh\nexec {sys.executable} {WINSHELL} --run-file \"$@\"\n")
    interp.chmod(0o755)
    names = {"TURWinExe": f":TURWinExe:M::MZ::{interp}:", "TURWinBat": f":TURWinBat:E::bat::{interp}:",
             "TURWinCmd": f":TURWinCmd:E::cmd::{interp}:", "TURWinPs1": f":TURWinPs1:E::ps1::{interp}:"}
    existing = sorted(p.name for p in misc.iterdir())
    record("binfmt", "handlers registered before the test", "info", ", ".join(existing))
    try:
        for name, rule in names.items():
            if (misc / name).exists():
                (misc / name).write_text("-1")
            (misc / "register").write_text(rule)
        shutil.copy(BUILD / "hello.exe", FIX / "hello.exe")
        for rel in ("hello.exe", "scripts/hello.bat", "scripts/args.cmd", "scripts/hello.ps1"):
            (FIX / rel).chmod(0o755)
        code, out, err = run(["bash", "-c", './hello.exe one "two words"'])
        record("binfmt", './hello.exe one "two words" from bash',
               code == 7 and "hello from exe" in out and "arg1=one" in out and "arg2=two words" in out
               and "cwd=C:\\turfix" in out, f"exit={code} out={out.strip()!r} err={err.strip()[-200:]!r}")
        code, out, err = run(["bash", "-c", "./scripts/hello.bat"])
        record("binfmt", "./scripts/hello.bat from bash", code == 0 and "hello from bat" in out,
               f"exit={code} out={out.strip()!r} err={err.strip()[-200:]!r}")
        code, out, err = run(["bash", "-c", './scripts/args.cmd one "two three"'])
        record("binfmt", './scripts/args.cmd one "two three" from bash',
               code == 3 and "first=one second=two three" in out,
               f"exit={code} out={out.strip()!r} err={err.strip()[-200:]!r}")
        code, out, err = run(["bash", "-c", "./scripts/hello.ps1 -Name World"])
        record("binfmt", "./scripts/hello.ps1 -Name World from bash", code == 0 and "hello from World" in out,
               f"exit={code} out={out.strip()!r} err={err.strip()[-200:]!r}")
        code, out, err = run(["bash", "-c", "cd scripts && PATH=$PATH:. hello.bat"])
        record("binfmt", "hello.bat by bare name with its folder on PATH", code == 0 and "hello from bat" in out,
               f"exit={code} out={out.strip()!r} err={err.strip()[-200:]!r}")
        (FIX / "scripts" / "hello.bat").chmod(0o644)
        code, out, err = run(["bash", "-c", "./scripts/hello.bat"])
        record("binfmt", "./scripts/hello.bat without the execute bit (as a download arrives)",
               "runs" if code == 0 else "does not run", f"exit={code} err={err.strip()[-200:]!r}")
    finally:
        for name in names:
            try:
                (misc / name).write_text("-1")
            except OSError:
                pass


def wild() -> None:
    """One cheap test of a wilder idea: under Wine, PowerShell's CIM cmdlets
    need Windows' management infrastructure (mi.dll), which Wine does not
    have, but the older .NET route to WMI goes through COM, which Wine does
    implement. If that works, a small module could give PowerShell for
    Windows under Wine its Get-CimInstance back."""
    import re

    import run as harness

    cand = harness.Candidate("wine-pwsh")
    if not cand.bring_up():
        record("wild", "PowerShell 7 for Windows starts under Wine", False,
               json.dumps(cand.meta.get("bring_up"))[:500])
        return
    record("wild", "PowerShell 7 for Windows starts under Wine", True, json.dumps(cand.wine_env))
    probes = [
        ("WMI through COM: Win32_OperatingSystem",
         "([wmisearcher]'SELECT Caption, Version FROM Win32_OperatingSystem').Get() | "
         "ForEach-Object { \"$($_.Caption) $($_.Version)\" }", r"\S+ \d+\.\d+"),
        ("WMI through COM: Win32_Processor",
         "([wmisearcher]'SELECT Name FROM Win32_Processor').Get() | ForEach-Object { $_.Name }", r"\S"),
        ("WMI through COM: Win32_LogicalDisk",
         "([wmisearcher]'SELECT DeviceID, Size FROM Win32_LogicalDisk').Get() | "
         "ForEach-Object { \"$($_.DeviceID) $($_.Size)\" }", r"C: \d+"),
        ("WMI through COM: number of Win32_Service",
         "@(([wmisearcher]'SELECT Name FROM Win32_Service').Get()).Count", r"^[1-9]\d*\s*$"),
        (".NET registry API", "[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("
         "'SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').GetValue('ProductName')", r"\S"),
        (".NET service API", "[System.ServiceProcess.ServiceController]::GetServices().Count", r"^[1-9]\d*\s*$"),
        ("why Get-CimInstance fails (information)",
         "try { Get-CimInstance Win32_OperatingSystem -ErrorAction Stop | Out-Null; 'it works' } "
         "catch { $_.Exception.GetType().Name + ': ' + $_.Exception.Message }", None),
        ("what PowerShell thinks it runs on (information)", "$PSVersionTable.OS", None),
    ]
    for name, script, pattern in probes:
        result = cand.run("ps", script, 240)
        text = result["stdout"].strip()
        if pattern is None:
            record("wild", name, "info", text[:300] or result["stderr"].strip()[-300:])
        else:
            record("wild", name, bool(re.search(pattern, text, re.M)),
                   f"exit={result['exit']} out={text[:200]!r} err={result['stderr'].strip()[-200:]!r}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    for step in (safety, paths, bridge, binfmt, wild):
        try:
            step()
        except Exception as err:  # noqa: BLE001 - one step must not hide the others
            record(step.__name__, "the step itself", False, f"{type(err).__name__}: {err}")
    (out_dir / "integration.json").write_text(json.dumps(rows, indent=1))
    lines = ["### Integration and safety", "", "| Group | Test | Verdict | Detail |", "|---|---|---|---|"]
    for row in rows:
        detail = row["detail"].replace("|", "\\|").replace("\n", " ")
        lines.append(f"| {row['group']} | `{row['test'].replace('|', chr(92) + '|')}` | {row['verdict']} | {detail} |")
    (out_dir / "integration.md").write_text("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

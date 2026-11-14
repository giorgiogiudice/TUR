#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Run the corpus through one candidate and write the shape of every result.

    run.py --candidate NAME --out DIR [--only REGEX] [--passes N]

Candidates on Windows (the judge):

    truth        cmd.exe for the cmd and tools groups, PowerShell 7 for ps
    ps51         Windows PowerShell 5.1 for the ps group (a noise floor: how
                 far Windows' own older shell is from its newer one)

Candidates on Linux:

    wine-cmd     Wine's cmd.exe and Wine's own tools (cmd, tools)
    wine-pwsh    PowerShell 7 for Windows under Wine (ps)
    pwsh-native  PowerShell 7 for Linux, as shipped (ps)
    pwsh-compat  PowerShell 7 for Linux with the TurWinCompat module (ps)
    router       tur-winshell (all groups)
    bash-shims   the line pasted into bash, with tur-winshell's tool names
                 on PATH (cmd, tools)

Linux candidates read their surroundings from the environment:
WINCLI_PREFIX (the Wine prefix), WINCLI_WINE, WINCLI_PWSH, WINCLI_PWSH_WIN
(Windows path of pwsh.exe inside the prefix), WINCLI_MODULE, WINCLI_WINSHELL,
WINCLI_SHIMBIN. Nothing here is run on a development machine: CI only.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import pathlib
import re
import signal
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import fixture  # noqa: E402
import shape  # noqa: E402

WINDOWS = os.name == "nt"
GROUPS = {
    "truth": {"cmd", "tools", "ps"},
    "ps51": {"ps"},
    "wine-cmd": {"cmd", "tools"},
    "wine-pwsh": {"ps"},
    "pwsh-native": {"ps"},
    "pwsh-compat": {"ps"},
    "router": {"cmd", "tools", "ps"},
    "bash-shims": {"cmd", "tools"},
}
PS_PREFIX = "$ProgressPreference = 'SilentlyContinue'; "
PWSH_ARGS = ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand"]
# Ways to start .NET under Wine, tried in order until one prints the marker.
WINE_PWSH_VARIANTS = [
    {},
    {"DOTNET_EnableWriteXorExecute": "0"},
    {"DOTNET_SYSTEM_GLOBALIZATION_INVARIANT": "1"},
    {"DOTNET_SYSTEM_GLOBALIZATION_INVARIANT": "1", "DOTNET_EnableWriteXorExecute": "0"},
    {"DOTNET_SYSTEM_GLOBALIZATION_INVARIANT": "1", "DOTNET_EnableWriteXorExecute": "0",
     "DOTNET_TieredCompilation": "0"},
]


def decode(data: bytes) -> str:
    if not data:
        return ""
    if len(data) >= 4 and data[1::2].count(0) > len(data) // 4:
        return data.decode("utf-16-le", errors="replace")
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return data.decode("cp437", errors="replace")


def encoded(script: str) -> str:
    return base64.b64encode(script.encode("utf-16-le")).decode()


class Candidate:
    """How one candidate starts a command line of a given shell."""

    def __init__(self, name: str):
        self.name = name
        self.meta: dict = {"candidate": name}
        env = os.environ
        if WINDOWS:
            self.fix = pathlib.Path(r"C:\turfix")
            self.pwsh = env.get("WINCLI_PWSH_EXE", "pwsh.exe")
        else:
            self.prefix = pathlib.Path(env["WINCLI_PREFIX"])
            self.fix = self.prefix / "drive_c" / "turfix"
            self.wine = env.get("WINCLI_WINE", "wine")
            self.pwsh = env.get("WINCLI_PWSH", "pwsh")
            self.pwsh_win = env.get("WINCLI_PWSH_WIN", r"C:\Program Files\PowerShell\7\pwsh.exe")
            self.module = env.get("WINCLI_MODULE", str(HERE / "compat" / "TurWinCompat"))
            self.winshell = env.get("WINCLI_WINSHELL", str(HERE / "winshell" / "tur-winshell"))
            self.shimbin = env.get("WINCLI_SHIMBIN", "")
            self.rcfile = self.prefix / "drive_c" / "turrun" / "rc.txt"
            self.wine_env: dict[str, str] = {}

    # -- starting things ---------------------------------------------------

    def _spawn(self, argv, env_extra=None, timeout=90, cwd=None):
        env = os.environ.copy()
        env["NO_COLOR"] = "1"
        if env_extra:
            env.update(env_extra)
        kwargs: dict = {"stdin": subprocess.DEVNULL, "stdout": subprocess.PIPE, "stderr": subprocess.PIPE,
                        "cwd": str(cwd or self.fix), "env": env}
        if not WINDOWS:
            kwargs["start_new_session"] = True
        started = time.time()
        try:
            proc = subprocess.Popen(argv, **kwargs)
        except OSError as err:
            return {"exit": None, "stdout": "", "stderr": f"could not start: {err}", "timeout": False, "secs": 0.0}
        timed_out = False
        try:
            out, err = proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            if WINDOWS:
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], capture_output=True, check=False)
            else:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except OSError:
                    pass
            try:
                out, err = proc.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
                out, err = b"", b""
        return {"exit": proc.returncode, "stdout": decode(out), "stderr": decode(err), "timeout": timed_out,
                "secs": round(time.time() - started, 2)}

    def _wine(self, cmdline: str, timeout: int, env_extra=None):
        """A Windows command line, exactly as given, in the prefix."""
        try:
            self.rcfile.unlink()
        except OSError:
            pass
        env = {"WINEPREFIX": str(self.prefix), "WINEDEBUG": "-all", "TURRUN_CMDLINE": cmdline,
               "TURRUN_RC": r"C:\turrun\rc.txt"}
        env.update(self.wine_env)
        if env_extra:
            env.update(env_extra)
        result = self._spawn([self.wine, r"C:\turrun\turrun.exe"], env, timeout)
        try:
            result["exit"] = int(self.rcfile.read_text().strip())
        except (OSError, ValueError):
            pass
        if result["timeout"]:
            # Whatever the command left running would hold the next one up.
            subprocess.run(["wineserver", "-k"], env={**os.environ, "WINEPREFIX": str(self.prefix)},
                           capture_output=True, check=False, timeout=60)
            subprocess.run(["wineserver", "-p"], env={**os.environ, "WINEPREFIX": str(self.prefix)},
                           capture_output=True, check=False, timeout=60)
        return result

    def run(self, shell: str, line: str, timeout: int) -> dict:
        name = self.name
        if name == "truth":
            if shell == "cmd":
                return self._spawn(f'cmd.exe /d /s /c "{line}"', None, timeout)
            return self._spawn([self.pwsh, *PWSH_ARGS, encoded(PS_PREFIX + line)], None, timeout)
        if name == "ps51":
            return self._spawn(["powershell.exe", *PWSH_ARGS, encoded(PS_PREFIX + line)], None, timeout)
        if name == "wine-cmd":
            return self._wine(f'cmd.exe /d /s /c "{line}"', timeout)
        if name == "wine-pwsh":
            return self._wine(f'"{self.pwsh_win}" {" ".join(PWSH_ARGS)} {encoded(PS_PREFIX + line)}', timeout)
        if name == "pwsh-native":
            return self._spawn([self.pwsh, *PWSH_ARGS, encoded(PS_PREFIX + line)], None, timeout)
        if name == "pwsh-compat":
            script = f"{PS_PREFIX}Import-Module '{self.module}' -DisableNameChecking; {line}"
            return self._spawn([self.pwsh, *PWSH_ARGS, encoded(script)], self._tur_env(), timeout)
        if name == "router":
            rc = self.prefix / "winshell-rc.txt"
            try:
                rc.unlink()
            except OSError:
                pass
            env = self._tur_env()
            env["TUR_WINSHELL_RCFILE"] = str(rc)
            result = self._spawn([sys.executable, self.winshell, "--shell", shell, "-c", line], env, timeout)
            try:
                result["exit"] = int(rc.read_text().strip())
            except (OSError, ValueError):
                pass
            return result
        if name == "bash-shims":
            env = self._tur_env()
            env["PATH"] = self.shimbin + os.pathsep + os.environ.get("PATH", "")
            return self._spawn(["bash", "-c", line], env, timeout)
        raise SystemExit(f"unknown candidate {name}")

    def _tur_env(self) -> dict:
        return {"TUR_WINPREFIX": str(self.prefix), "WINEPREFIX": str(self.prefix), "WINEDEBUG": "-all",
                "TUR_WINE": self.wine, "TUR_PWSH": self.pwsh, "TUR_WINSHELL": self.winshell,
                "TUR_WINCOMPAT": self.module, "TUR_PWSH_WIN": self.pwsh_win}

    # -- bring-up ------------------------------------------------------------

    def bring_up(self) -> bool:
        """Check the candidate starts at all; record how. False: do not run."""
        marker = "turok-" + str(os.getpid())
        if self.name in ("truth", "ps51", "pwsh-native", "pwsh-compat"):
            result = self.run("ps", f"'{marker}'; $PSVersionTable.PSVersion.ToString()", 180)
            self.meta["powershell"] = result["stdout"].strip().splitlines()[-1:] or [result["stderr"][-300:]]
            if self.name == "truth":
                self.meta["cmd"] = self.run("cmd", "ver", 60)["stdout"].strip()
            if marker not in result["stdout"]:
                self.meta["bring_up"] = {"ok": False, "exit": result["exit"], "stderr": result["stderr"][-2000:]}
                return self.name in ("truth",)
            self.meta["bring_up"] = {"ok": True}
            return True
        if self.name == "wine-pwsh":
            tried = []
            for variant in WINE_PWSH_VARIANTS:
                self.wine_env = dict(variant)
                result = self.run("ps", f"'{marker}'; $PSVersionTable.PSVersion.ToString()", 240)
                ok = marker in result["stdout"]
                tried.append({"env": variant, "ok": ok, "exit": result["exit"], "secs": result["secs"],
                              "timeout": result["timeout"], "stdout": result["stdout"][-400:],
                              "stderr": result["stderr"][-1500:]})
                if ok:
                    break
            self.meta["bring_up"] = {"ok": tried[-1]["ok"], "variants": tried}
            return tried[-1]["ok"]
        if self.name in ("wine-cmd", "router", "bash-shims"):
            shell = "cmd"
            result = self.run(shell, f"echo {marker}", 180)
            self.meta["bring_up"] = {"ok": marker in result["stdout"], "exit": result["exit"],
                                     "stdout": result["stdout"][-400:], "stderr": result["stderr"][-1500:]}
            return marker in result["stdout"]
        return True


def run_corpus(cand: Candidate, entries: list[dict], out_dir: pathlib.Path, suffix: str) -> None:
    shapes: dict = {}
    raw: dict = {}
    timeouts_in_a_row = 0
    aborted = None

    def save() -> None:
        (out_dir / f"shapes{suffix}.json").write_text(json.dumps(shapes, indent=1, sort_keys=True))
        (out_dir / f"raw{suffix}.json").write_text(json.dumps(raw, indent=1, sort_keys=True))

    for n, entry in enumerate(entries, 1):
        ident = entry["id"]
        if n % 20 == 0:
            save()  # a job that runs out of time keeps what it measured
        if aborted:
            shapes[ident] = {"notrun": aborted}
            continue
        fixture.build(cand.fix)
        pre = None
        if entry.get("pre"):
            pre = cand.run(entry["shell"], entry["pre"], 180)
        result = cand.run(entry["shell"], entry["cmd"], entry.get("timeout", 90))
        result["files"] = fixture.state(cand.fix)
        shapes[ident] = shape.compute(entry, result)
        shapes[ident]["stderr_empty"] = not result["stderr"].strip()
        shapes[ident]["answer"] = bool(re.search(r"(?m)^tur-winshell: (answer|refused|explain)", result["stderr"]))
        shapes[ident]["secs"] = result["secs"]
        record = {"cmd": entry["cmd"], "exit": result["exit"], "timeout": result["timeout"],
                  "secs": result["secs"]}
        if cand.name in ("truth", "ps51") and entry.get("help"):
            record["stdout"] = f"<help text not kept: {len(result['stdout'])} characters>"
        else:
            record["stdout"] = result["stdout"][:20000]
        record["stderr"] = result["stderr"][:6000]
        if pre:
            record["pre_exit"] = pre["exit"]
        raw[ident] = record
        timeouts_in_a_row = timeouts_in_a_row + 1 if result["timeout"] else 0
        if timeouts_in_a_row >= 6:
            aborted = "not run: the candidate timed out six times in a row and was stopped"
        print(f"[{cand.name}{suffix}] {n}/{len(entries)} {ident} exit={result['exit']} "
              f"{result['secs']}s{' TIMEOUT' if result['timeout'] else ''}", flush=True)
    save()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", required=True, choices=sorted(GROUPS))
    parser.add_argument("--out", required=True)
    parser.add_argument("--only", default="")
    parser.add_argument("--passes", type=int, default=1)
    args = parser.parse_args()

    out_dir = pathlib.Path(args.out) / args.candidate
    out_dir.mkdir(parents=True, exist_ok=True)
    cand = Candidate(args.candidate)
    entries = [e for e in corpus.CORPUS if e["group"] in GROUPS[args.candidate]]
    if args.candidate in ("truth", "ps51"):
        entries = [e for e in entries if e["truth"]]
    if os.environ.get("WINCLI_FRESH") != "1":
        entries = [e for e in entries if not e["fresh"]]
    if args.only:
        entries = [e for e in entries if re.search(args.only, e["id"])]

    fixture.build(cand.fix)
    started = time.time()
    ok = cand.bring_up()
    cand.meta["entries"] = len(entries)
    (out_dir / "meta.json").write_text(json.dumps(cand.meta, indent=1, sort_keys=True))
    if not ok:
        note = "not run: the candidate did not start (see meta.json)"
        (out_dir / "shapes.json").write_text(json.dumps({e["id"]: {"notrun": note} for e in entries}, indent=1))
        print(f"[{args.candidate}] did not start: {json.dumps(cand.meta.get('bring_up'))[:3000]}", flush=True)
    else:
        for n in range(args.passes):
            run_corpus(cand, entries, out_dir, "" if n == 0 else str(n + 1))
    cand.meta["seconds"] = round(time.time() - started)
    (out_dir / "meta.json").write_text(json.dumps(cand.meta, indent=1, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())

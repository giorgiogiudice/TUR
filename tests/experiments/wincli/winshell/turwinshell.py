# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""tur-winshell: run a Windows command line on TUR (prototype).

    tur-winshell [--shell cmd|ps] -c LINE       a line typed at a cmd or
                                                PowerShell prompt
    tur-winshell --argv TOOL [ARG...]           one Windows command, started
                                                by name from a Linux shell
    tur-winshell [--shell cmd|ps] --verdict -c LINE
                                                only say what the safety
                                                rules decide

What it does with a line:

1. The safety rules (rules.py). A refused line is not run; the reason is
   printed behind `tur-winshell: refused:`.
2. A cmd line goes to the Windows environment's cmd.exe (Wine's), with a
   directory of TUR's own commands at the front of its PATH. Those are
   small Windows programs (win32/turshim.c) that hand the work to shims.py
   on the Linux side, so `tasklist | findstr x` or a batch file can use
   them like any other command.
3. A PowerShell line goes to PowerShell 7 for Linux with the TurWinCompat
   module, or to PowerShell 7 for Windows under Wine (TUR_WINSHELL_PS=wine).
4. A command nobody has is explained, behind `tur-winshell: explain:`,
   instead of being left at "not recognized".

The 32-bit exit code is written to the file named by TUR_WINSHELL_RCFILE,
because a Linux exit status keeps only eight bits of it.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import rules  # noqa: E402
import shims  # noqa: E402
from winpath import WinEnv  # noqa: E402

# Windows commands with no equivalent here: what they are, in our words.
KNOWN = {
    "gpupdate": "refreshes Group Policy from a Windows domain. TUR has no Group Policy.",
    "gpresult": "reports the Group Policy applied to a Windows PC. TUR has no Group Policy.",
    "slmgr": "manages the licence of a Windows installation. TUR's Windows environment is not a Windows "
             "installation and has no licence to manage.",
    "wsl": "manages the Windows Subsystem for Linux. TUR is Linux: open a terminal.",
    "powercfg": "configures Windows power plans. On TUR: Settings > Power.",
    "defrag": "defragments a Windows volume. TUR's file systems do not need it.",
    "netsh": "configures Windows networking and its firewall. On TUR: Settings > Network.",
    "wuauclt": "drives Windows Update. On TUR, updates are whole-image and managed by TUR's update manager.",
    "usoclient": "drives Windows Update. On TUR, updates are whole-image and managed by TUR's update manager.",
    "msconfig": "opens Windows' start-up configuration. On TUR: Settings > Startup applications.",
    "eventvwr": "opens the Windows event log. On TUR: the journal (journalctl), or TUR Diagnostics.",
    "driverquery": "lists Windows drivers. On TUR, hardware is driven by the Linux kernel: lspci -k, lsusb.",
    "clip": "copies its input to the clipboard. On TUR: wl-copy.",
    "choice": "asks the user to pick a key in a batch file.",
    "timeout": "waits a number of seconds in a batch file. On TUR: sleep.",
    "mstsc": "opens a Remote Desktop connection. On TUR: the Remote Desktop application (KRDC).",
    "explorer": "opens the Windows file manager. On TUR: Dolphin (xdg-open .).",
    "control": "opens the Windows Control Panel. On TUR: Settings.",
    "taskmgr": "opens the Windows Task Manager. On TUR: System Monitor.",
    "wevtutil": "queries the Windows event log. On TUR: journalctl.",
    "get-winevent": "reads the Windows event log. On TUR: journalctl, or TUR Diagnostics.",
    "get-eventlog": "reads the Windows event log. On TUR: journalctl, or TUR Diagnostics.",
    "get-hotfix": "lists installed Windows updates. TUR updates as a whole image: tur-update status.",
    "get-volume": "lists Windows volumes. On TUR: lsblk, or TUR Storage.",
    "get-disk": "lists disks. On TUR: lsblk, or TUR Storage.",
    "get-wmiobject": "is the older form of Get-CimInstance, removed from PowerShell 7 on Windows too.",
}


def note(kind: str, text: str) -> None:
    sys.stderr.write(f"tur-winshell: {kind}: {text}\n")


def explain(name: str) -> None:
    head = rules.head_of(name)
    known = KNOWN.get(head)
    if known:
        note("explain", f"'{head}' {known}")
    else:
        note("explain", f"'{head}' is not a command this computer has. If it is a Windows command, P@x can say "
                        "what it does on Windows and what TUR offers instead.")


def write_rc(code: int) -> int:
    target = os.environ.get("TUR_WINSHELL_RCFILE")
    if target:
        try:
            pathlib.Path(target).write_text(f"{code}\n")
        except OSError:
            pass
    if 0 <= code <= 255:
        return code
    return (code & 0xFF) or 1


def dos_path(env: WinEnv, unix: str) -> str:
    """A path cmd.exe can start a Linux program by."""
    if env.drives.get("z") == "/":
        return "Z:" + unix.replace("/", "\\")
    return "\\\\?\\unix" + unix.replace("/", "\\")


def wine_run(cmdline: str, env: WinEnv, capture_stdout: bool = False) -> tuple[int, str, str]:
    """Start a Windows command line in the environment; (code, stdout, stderr)."""
    wine = os.environ.get("TUR_WINE", "wine")
    rc_name = f"rc-{os.getpid()}.txt"
    rc_file = env.drive_c / "turrun" / rc_name
    child = os.environ.copy()
    child.update({
        "WINEPREFIX": str(env.prefix), "WINEDEBUG": os.environ.get("WINEDEBUG", "-all"),
        "TURRUN_CMDLINE": cmdline, "TURRUN_RC": f"C:\\turrun\\{rc_name}",
        "TUR_WINPREFIX": str(env.prefix), "TUR_WINSHIM": dos_path(env, str(HERE / "tur-winshim")),
    })
    if os.environ.get("TUR_WINSHELL_BRIDGE", "1") != "0":
        child["TURRUN_PATH_PREPEND"] = "C:\\turshim"
    child.pop("TUR_WINSHELL_RCFILE", None)
    sys.stdout.flush()
    done = subprocess.run([wine, "C:\\turrun\\turrun.exe"], env=child, check=False, stderr=subprocess.PIPE,
                          stdout=subprocess.PIPE if capture_stdout else None)
    code = done.returncode
    try:
        code = int(rc_file.read_text().strip())
        rc_file.unlink()
    except (OSError, ValueError):
        pass
    return code, (done.stdout or b"").decode(errors="replace"), done.stderr.decode(errors="replace")


def run_cmd(line: str, env: WinEnv) -> int:
    code, _out, err = wine_run(f'cmd.exe /d /s /c "{line}"', env)
    sys.stderr.write(err)
    missing = re.search(r"Can't recognize '([^']+)' as an internal or external command", err)
    if missing:
        explain(missing.group(1))
    return code


def run_ps(script: str, env: WinEnv) -> int:
    backend = os.environ.get("TUR_WINSHELL_PS", "native")
    args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand"]
    quiet = "$ProgressPreference = 'SilentlyContinue'; "
    sys.stdout.flush()
    if backend == "wine":
        pwsh = os.environ.get("TUR_PWSH_WIN", r"C:\Program Files\PowerShell\7\pwsh.exe")
        encoded = base64.b64encode((quiet + script).encode("utf-16-le")).decode()
        code, _out, err = wine_run(f'"{pwsh}" {" ".join(args)} {encoded}', env)
    else:
        pwsh = os.environ.get("TUR_PWSH", "pwsh")
        module = os.environ.get("TUR_WINCOMPAT", str(HERE.parent / "compat" / "TurWinCompat"))
        full = f"{quiet}Import-Module '{module}' -DisableNameChecking; {script}"
        encoded = base64.b64encode(full.encode("utf-16-le")).decode()
        child = os.environ.copy()
        child.update({"TUR_WINPREFIX": str(env.prefix), "TUR_WINSHELL": str(HERE / "tur-winshell"),
                      "NO_COLOR": "1"})
        child.pop("TUR_WINSHELL_RCFILE", None)
        done = subprocess.run([pwsh, *args, encoded], env=child, check=False, stderr=subprocess.PIPE)
        code, err = done.returncode, done.stderr.decode(errors="replace")
    sys.stderr.write(err)
    missing = re.search(r"The term '([^']+)' is not recognized as a name of a cmdlet", err)
    if missing:
        explain(missing.group(1))
    return code


def ps_arg(arg: str) -> str:
    """An argument for a PowerShell script: a parameter name stays bare."""
    if re.fullmatch(r"-[A-Za-z]\w*:?", arg):
        return arg
    return "'" + arg.replace("'", "''") + "'"


def quote_arg(arg: str) -> str:
    if arg and not re.search(r'[\s&|<>^()"]', arg):
        return arg
    return '"' + arg + '"'


def refuse(verdict: rules.Verdict) -> int:
    note("refused", f"[{verdict.rule}] {verdict.message}")
    return write_rc(5)


def run_file(args: list[str], env: WinEnv) -> int:
    """A Windows file started by name from a Linux shell (binfmt_misc):
    a program, a batch file or a PowerShell script, with its arguments."""
    path, rest = os.path.abspath(args[0]), args[1:]
    kind = path.rsplit(".", 1)[-1].lower()
    if kind == "ps1":
        return run_ps("& '" + path.replace("'", "''") + "' " + " ".join(ps_arg(a) for a in rest), env)
    win = env.unix_to_win(path)
    if win is None:
        note("explain", f"{path} is not in a folder the Windows environment can see.")
        return 1
    line = " ".join([quote_arg(win)] + [quote_arg(a) for a in rest])
    if kind in ("bat", "cmd"):
        verdict = rules.classify_cmd(line, env, os.getcwd())
        if verdict.action == "refuse":
            return refuse(verdict)
        return run_cmd(line, env)
    code, _out, err = wine_run(line, env)
    sys.stderr.write(err)
    return code


def main(argv: list[str]) -> int:
    shell, line, tool_argv, verdict_only = "cmd", None, None, False
    n = 0
    while n < len(argv):
        arg = argv[n]
        if arg == "--run-file" and n + 1 < len(argv):
            return write_rc(run_file(argv[n + 1:], WinEnv()))
        if arg == "--shell" and n + 1 < len(argv):
            shell = argv[n + 1]
            n += 1
        elif arg == "-c" and n + 1 < len(argv):
            line = argv[n + 1]
            n += 1
        elif arg == "--verdict":
            verdict_only = True
        elif arg == "--argv":
            tool_argv = argv[n + 1:]
            break
        else:
            sys.stderr.write(__doc__ or "")
            return 2
        n += 1
    env = WinEnv()
    cwd = os.getcwd()
    if tool_argv:
        line = " ".join(quote_arg(a) for a in tool_argv)
        shell = "cmd"
    if line is None:
        sys.stderr.write(__doc__ or "")
        return 2
    verdict = rules.classify_ps(line, env, cwd) if shell == "ps" else rules.classify_cmd(line, env, cwd)
    if verdict_only:
        print(json.dumps({"action": verdict.action, "rule": verdict.rule, "message": verdict.message}))
        return 0
    if verdict.action == "refuse":
        return refuse(verdict)
    if tool_argv:
        head = rules.head_of(tool_argv[0])
        if head in shims.SHIMS:
            code = shims.SHIMS[head](tool_argv[1:], env)
            sys.stdout.flush()
            return write_rc(code)
    if shell == "ps":
        return write_rc(run_ps(line, env))
    return write_rc(run_cmd(line, env))


def shim_main(argv: list[str]) -> int:
    """Entry for win32/turshim.c: NAME ARGS, with the exit-code trailer."""
    name = rules.head_of(argv[0]) if argv else ""
    env = WinEnv()
    try:
        if name in shims.SHIMS:
            code = shims.SHIMS[name](argv[1:], env)
        else:
            explain(name)
            code = 9009
    except Exception as err:  # noqa: BLE001 - a shim must not take the shell down
        sys.stderr.write(f"tur-winshell: {name}: {type(err).__name__}: {err}\n")
        code = 1
    sys.stdout.flush()
    if os.environ.get("TURSHIM_TRAILER") == "1":
        sys.stdout.buffer.write(b"\x1eTURSHIM-RC=%d\x1e" % (code & 0xFFFFFFFF))
        sys.stdout.buffer.flush()
    return code & 0xFF

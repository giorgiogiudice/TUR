#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Records real runs of tur-apps for the application centre's fixtures.

    record.py --tur-apps BIN --catalogue DIR --fakes DIR --home DIR --out DIR

Runs on a CI runner, inside the end-to-end container of the desktop
workflow (desktop/ci/centre-e2e/inside.sh), never on a development machine.
It runs tur-apps with exactly the argument vectors tur-centre uses
(desktop/apps/tur-centre/src/appssource.cpp; tst_centre_apps holds the two
together) and writes, for each scenario, one file of what tur-apps printed,
line by line, and how it ended: desktop/apps/tur-centre/fixtures/apps/
<scenario>/runs.json once fixtures/apps/update.py has copied them out of
the job's artifact. Nobody writes these files by hand.

Scenarios, in the order they run (each starts where the last ended):

  sources    Flathub not added for a fresh user: the plan is refused
             (TUR-DESKTOP-APPS-008); Flathub is switched on; the plan is made.
  install    Chromium B.S.U. from Flathub, per user: plan, installation with
             its events, the list before and after, the update check.
  update     An older commit of it is put in place with flatpak itself (when
             Flathub still has one), so that the update check finds an update
             and tur-apps really updates it.
  remove     Removal keeping its settings and data.
  cancel     A second installation, cancelled by writing "cancel" to tur-apps
             as soon as flatpak reports its first operation.
  chrome     Google Chrome from Flathub, per user: plan, installation (Flatpak
             downloads Google's own package from dl.google.com), removal.
  failure    The one failure that cannot be had on demand from Flathub: a
             flatpak that fails half-way. Real tur-apps, against tur-apps' own
             stand-in for flatpak (services/tur-apps/tests/fakes), told to
             fail after installing the runtime; the file says so.
  installer  "I have the installer file": a plan with a file that is not the
             installer the catalogue pins for Notepad++.

Standard library only. Run under dbus-run-session.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

LICENCE = "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0"
APP = "net.sourceforge.chromium-bsu"
FLATPAK_APP = "net.sourceforge.chromium-bsu"
CHROME = "com.google.chrome"
NOTEPAD = "org.notepad-plus-plus.notepad-plus-plus"


def log(text: str) -> None:
    print(text, flush=True)


# The argument vectors of tur-centre (AppsRequest::arguments).
def plan(subject: str, installer: str | None = None) -> list[str]:
    out = ["plan", "--json"]
    if installer:
        out += ["--installer", installer]
    return out + ["--", subject]


def install(subject: str) -> list[str]:
    return ["install", "--yes", "--events", "--", subject]


def update(subject: str) -> list[str]:
    return ["update", "--events", "--", subject]


def remove(subject: str, delete: bool = False) -> list[str]:
    return ["remove", "--delete-data" if delete else "--keep-data", "--events", "--", subject]


LIST = ["list", "--json"]
LIST_UPDATES = ["list", "--check-updates", "--json"]
SOURCES = ["sources", "--json"]


def enable(name: str) -> list[str]:
    return ["sources", "enable", "--json", "--", name]


class Recorder:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.out = pathlib.Path(args.out).resolve()
        self.out.mkdir(parents=True, exist_ok=True)
        self.home = pathlib.Path(args.home).resolve()
        self.home.mkdir(parents=True, exist_ok=True)
        self.failures: list[str] = []

    def env(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        env = dict(os.environ)
        env.update({
            "HOME": str(self.home),
            "XDG_DATA_HOME": str(self.home / ".local/share"),
            "XDG_CACHE_HOME": str(self.home / ".cache"),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
        })
        env.pop("FLATPAK_USER_DIR", None)
        if extra:
            env.update(extra)
        return env

    def command(self, arguments: list[str], programs: list[str] | None = None) -> list[str]:
        return [self.args.tur_apps, "--catalogue", self.args.catalogue, *(programs or []), *arguments]

    def run(self, arguments: list[str], cancel_when=None, programs: list[str] | None = None,
            extra_env: dict[str, str] | None = None, recorded: list[str] | None = None) -> dict:
        """Runs tur-apps once and returns the recorded invocation."""
        command = self.command(arguments, programs)
        log("$ " + " ".join(command))
        started = time.monotonic()
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, env=self.env(extra_env), text=True)
        lines: list[str] = []
        cancel_after = None
        assert process.stdout is not None
        for raw in process.stdout:
            line = raw.rstrip("\n")
            if not line.strip():
                continue
            lines.append(line)
            if cancel_when is not None and cancel_after is None and cancel_when(line):
                cancel_after = len(lines)
                log(f"  -> cancel, after line {cancel_after}")
                assert process.stdin is not None
                process.stdin.write("cancel\n")
                process.stdin.flush()
        stderr = process.stderr.read() if process.stderr else ""
        status = process.wait()
        if process.stdin:
            process.stdin.close()
        took = time.monotonic() - started
        log(f"  exit {status} in {took:.1f} s; {len(lines)} lines")
        for line in lines[-3:]:
            log("  | " + line[:300])
        invocation = {
            "arguments": recorded or arguments,
            "exit_status": status,
            "stderr": stderr,
            "stdout": lines,
            "seconds": round(took, 1),
        }
        if cancel_after is not None:
            invocation["cancel_after"] = cancel_after
        return invocation

    def write(self, scenario: str, note: str, invocations: list[dict]) -> None:
        directory = self.out / scenario
        directory.mkdir(parents=True, exist_ok=True)
        document = {
            "licence": LICENCE,
            "recorded_by": ("desktop/ci/centre-e2e/record.py, in the desktop workflow's job centre-e2e; "
                            "copied by desktop/apps/tur-centre/fixtures/apps/update.py; do not edit"),
            "run": os.environ.get("GITHUB_RUN_ID", "local"),
            "scenario": scenario,
            "note": note,
            "invocations": invocations,
        }
        (directory / "runs.json").write_text(json.dumps(document, indent=1, ensure_ascii=False) + "\n")
        log(f"wrote {directory / 'runs.json'}")

    def expect(self, what: str, ok: bool) -> None:
        log(f"[{'PASS' if ok else 'FAIL'}] {what}")
        if not ok:
            self.failures.append(what)


def last_event(invocation: dict) -> dict:
    for line in reversed(invocation["stdout"]):
        try:
            return json.loads(line)
        except ValueError:
            continue
    return {}


def document(invocation: dict) -> dict:
    try:
        return json.loads("\n".join(invocation["stdout"]))
    except ValueError:
        return {}


def flatpak(*arguments: str, env: dict[str, str]) -> subprocess.CompletedProcess:
    return subprocess.run(["flatpak", *arguments], env={**env, "LC_ALL": "C.UTF-8"},
                          capture_output=True, text=True)


def older_commit(recorder: Recorder) -> str | None:
    """The commit before the current one of the application on Flathub."""
    env = recorder.env()
    reference = f"app/{FLATPAK_APP}/x86_64/stable"
    result = flatpak("remote-info", "--user", "--log", "flathub", reference, env=env)
    log(result.stdout[-2000:] + result.stderr[-2000:])
    commits = re.findall(r"^\s*(?:Commit|History):\s*([0-9a-f]{64})", result.stdout, re.MULTILINE)
    # remote-info --log prints the current commit, then "History:" and the
    # earlier ones, each as "Commit: <hash>".
    seen = []
    for commit in commits:
        if commit not in seen:
            seen.append(commit)
    return seen[1] if len(seen) > 1 else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-apps", required=True)
    parser.add_argument("--catalogue", required=True)
    parser.add_argument("--fakes", required=True)
    parser.add_argument("--home", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    args.tur_apps = str(pathlib.Path(args.tur_apps).resolve())
    args.catalogue = str(pathlib.Path(args.catalogue).resolve())
    r = Recorder(args)

    # --- sources ----------------------------------------------------------
    runs = [r.run(SOURCES), r.run(plan(APP))]
    r.expect("with Flathub not added, the plan is refused with TUR-DESKTOP-APPS-008",
             document(runs[-1]).get("error", {}).get("code") == "TUR-DESKTOP-APPS-008")
    runs.append(r.run(enable("flathub")))
    r.expect("Flathub is switched on", document(runs[-1]).get("state") == "on")
    runs += [r.run(SOURCES), r.run(plan(APP))]
    r.expect("the plan is made", document(runs[-1]).get("route") == "flatpak")
    r.write("sources", "A fresh user on the runner: Flathub not added; switched on through tur-apps.", runs)

    # --- install ----------------------------------------------------------
    runs = [r.run(LIST), r.run(plan(APP)), r.run(install(APP)), r.run(LIST), r.run(LIST_UPDATES)]
    r.expect("installed", last_event(runs[2]).get("event") == "result")
    r.write("install", "Chromium B.S.U. installed per user from Flathub, with the real flatpak.", runs)

    # --- update -----------------------------------------------------------
    commit = older_commit(r)
    note = "Chromium B.S.U.: "
    if commit:
        result = flatpak("update", "--user", "--noninteractive", f"--commit={commit}",
                         f"app/{FLATPAK_APP}/x86_64/stable", env=r.env())
        log(result.stdout[-2000:] + result.stderr[-2000:])
        note += f"moved back to the earlier commit {commit} with flatpak update --commit, then updated by tur-apps."
    else:
        note += "Flathub has no earlier commit, so there was nothing to update."
    runs = [r.run(LIST_UPDATES), r.run(update(APP)), r.run(LIST_UPDATES)]
    r.expect("updated", last_event(runs[1]).get("event") == "result")
    r.write("update", note, runs)

    # --- remove -----------------------------------------------------------
    data = r.home / ".var/app" / FLATPAK_APP
    data.mkdir(parents=True, exist_ok=True)
    (data / "recorded-by-the-test").write_text("kept\n")
    runs = [r.run(LIST), r.run(remove(APP)), r.run(LIST)]
    r.expect("removed", last_event(runs[1]).get("event") == "result")
    r.expect("its data is kept", (data / "recorded-by-the-test").exists())
    r.write("remove", "Chromium B.S.U. removed, keeping its settings and data.", runs)

    # --- cancel -----------------------------------------------------------
    def first_operation(line: str) -> bool:
        try:
            event = json.loads(line)
        except ValueError:
            return False
        return event.get("event") == "activity" and event.get("step") == "install"

    runs = [r.run(plan(APP)), r.run(install(APP), cancel_when=first_operation), r.run(LIST)]
    end = last_event(runs[1])
    r.expect("cancelled with TUR-DESKTOP-APPS-013",
             end.get("event") == "error" and end.get("error", {}).get("code") == "TUR-DESKTOP-APPS-013")
    r.expect("nothing is installed after the cancel", document(runs[2]).get("applications") == [])
    left = flatpak("list", "--user", "--columns=ref", env=r.env())
    r.expect("flatpak lists nothing for the user", left.stdout.strip() == "")
    r.write("cancel", "A second installation of Chromium B.S.U., cancelled once flatpak reported its first operation.",
            runs)

    # --- chrome -----------------------------------------------------------
    runs = [r.run(plan(CHROME)), r.run(install(CHROME)), r.run(LIST), r.run(remove(CHROME, delete=True)), r.run(LIST)]
    r.expect("Chrome installed per user", last_event(runs[1]).get("event") == "result")
    r.expect("Chrome removed", last_event(runs[3]).get("event") == "result")
    r.write("chrome", "Google Chrome from Flathub, per user: Flatpak downloads Google's package from dl.google.com.", runs)

    # --- failure (stand-in flatpak) ----------------------------------------
    state = pathlib.Path(args.home).resolve().parent / "fake-state"
    shutil.rmtree(state, ignore_errors=True)
    state.mkdir(parents=True)
    (state / "remotes").write_text("flathub\thttps://dl.flathub.org/repo/\t\n")
    (state / "fail-install").write_text("")
    fakes = pathlib.Path(args.fakes).resolve()
    programs = ["--flatpak-program", str(fakes / "flatpak"), "--curl-program", str(fakes / "curl")]
    fake_env = {"FAKE_STATE": str(state)}
    runs = [r.run(plan(APP), programs=programs, extra_env=fake_env),
            r.run(install(APP), programs=programs, extra_env=fake_env)]
    r.expect("the stand-in's failure is reported", last_event(runs[1]).get("event") == "error")
    r.write("failure", "Real tur-apps against its own stand-in for flatpak (services/tur-apps/tests/fakes/flatpak), "
            "told to fail after installing the runtime: a failure half-way, which Flathub does not give on demand.",
            runs)

    # --- installer --------------------------------------------------------
    wrong = r.home / "Downloads" / "npp-installer.exe"
    wrong.parent.mkdir(parents=True, exist_ok=True)
    wrong.write_bytes(b"MZ this is not the Notepad++ installer\n")
    runs = [r.run(plan(NOTEPAD, str(wrong)), recorded=plan(NOTEPAD, "<installer>"))]
    r.write("installer", "A file that is not the installer the catalogue pins for Notepad++.", runs)

    log("")
    if r.failures:
        log("Expectations not met (the recordings were written all the same):")
        for failure in r.failures:
            log("  - " + failure)
        return 1
    log("Every expectation met.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

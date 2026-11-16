#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Windows applications on the installed, booted TUR system, in one extra boot.

    sudo tests/boot/windows.py prepare DISK WORK_DIR
    tests/boot/windows.py boot WORK_DIR OUT_DIR [--seconds 4200] [--only IDS]

The boot test's tested disk is booted once more, after the boot test and
before the boot of tests/boot/splash.py, on a throw-away overlay (QEMU's
-snapshot). SELinux stays as installed (enforcing); nothing is written to
the disk from the host: `prepare` only reads the loader entry and copies the
kernel and initramfs out. The kernel argument `tur.windows_check` starts two
units of the boot-test image: one signs the boot test's user in to Plasma
(the file is written in the guest, so SELinux labels it), the other
(guest rootfs, usr/libexec/tur-boot-test/windows-check) runs, as that user in
that session:

1. tests/windows-runtime/apps.py --session: real Windows applications from
   their publishers (pinned hashes) installed, started from their launcher
   entries, used once and removed through tur-runtime, with Fedora's Wine
   from the image; 7-Zip is also found in Plasma's launcher (KRunner) and
   started from there;
2. tur-apps, the application centre's command-line route, installing 7-Zip
   from the installer file and removing it;
3. tests/windows-runtime/autopilot.py --session --real-only on PuTTY with a
   fault injected.

The guest asks for a screenshot or a key press with a line on the serial
console ("WINDOWS-SHOT <name>", "WINDOWS-KEY <qcode>"); this takes the
picture of the whole screen through QMP, or presses the key, at once. At
the end the guest sends what it recorded as a tar stream over the
virtio-serial port. This then judges:

- required: SELinux enforcing before and after; 7-Zip and PuTTY (the
  64-bit single program) install, start (a window of their own), do their
  one thing and uninstall with nothing left; 7-Zip found and started from
  the launcher; tur-apps installs and removes 7-Zip, its launcher entry
  appearing and going; the autopilot finds the injected fault;
- recorded: every other application of the list, the SELinux denials, the
  time of each step.

Writes OUT_DIR/windows/: shots/*.png, guest/ (what the guest sent),
windows.json and windows.md. Exit status 1 when a required check fails.
Run in CI only (ADR-0002).
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import splash  # noqa: E402
import vm  # noqa: E402

REQUIRED_APPS = ("7zip-exe", "putty-exe")
STAGES = ("install", "launch", "check", "uninstall")


def run(*cmd: str, check: bool = True) -> str:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed ({result.returncode}): {result.stderr.strip()}")
    return result.stdout


def prepare(disk: str, work: pathlib.Path) -> int:
    """Copies the kernel and initramfs of the installed deployment out and
    writes the command line. The disk is mounted read-only."""
    out = work / "windows"
    out.mkdir(parents=True, exist_ok=True)
    loop = run("losetup", "--find", "--show", "--partscan", "--read-only", disk).strip()
    mnt = pathlib.Path(tempfile.mkdtemp(prefix="tur-windows-"))
    mounted = False
    try:
        run("udevadm", "settle", "--timeout=30", check=False)
        table = json.loads(run("sfdisk", "--json", loop))["partitiontable"]["partitions"]
        root = next((p["node"] for p in table if p["type"].lower() == splash.ROOT_X86_64), None)
        if not root:
            raise RuntimeError(f"no x86-64 root partition in {table}")
        for _ in range(30):
            if os.path.exists(root):
                break
            run("partx", "--add", loop, check=False)
            time.sleep(1)
        # Read-only; a file system whose journal would need replaying is
        # read without replaying it.
        for options in ("ro", "ro,norecovery", "ro,noload"):
            if subprocess.run(["mount", "-o", options, root, str(mnt)], capture_output=True).returncode == 0:
                mounted = True
                break
        if not mounted:
            raise RuntimeError(f"{root} could not be mounted read-only")
        entries = sorted((mnt / "boot/loader/entries").glob("*.conf"))
        if not entries:
            raise RuntimeError("no loader entry on the disk")
        entry = {}
        for line in entries[-1].read_text().splitlines():
            key, _, value = line.partition(" ")
            entry[key] = value.strip()

        def locate(path: str) -> pathlib.Path:
            for candidate in (mnt / path.lstrip("/"), mnt / "boot" / path.lstrip("/")):
                if candidate.is_file():
                    return candidate
            raise RuntimeError(f"{path} is not on the disk")

        shutil.copyfile(locate(entry["linux"]), out / "vmlinuz")
        shutil.copyfile(locate(entry["initrd"]), out / "initramfs.img")
        words = []
        for word in entry["options"].split():
            if word.startswith(("console=", "tur.boot_test", "enforcing=", "selinux=")):
                continue
            words.append("tur.boot_contract=1" if word == "$tur_boot_args" else word)
        # The serial console stays the primary one: the guest's requests come
        # through it. SELinux is left as installed.
        words += ["console=tty0", "console=ttyS0,115200n8", "tur.windows_check"]
        (out / "cmdline.txt").write_text(" ".join(words) + "\n")
        (out / "entry.json").write_text(json.dumps({"file": entries[-1].name, **entry}, indent=2) + "\n")
        print(f"loader entry {entries[-1].name}: {entry.get('title')}")
        print(f"command line: {' '.join(words)}")
    finally:
        if mounted:
            run("umount", str(mnt), check=False)
        run("losetup", "--detach", loop, check=False)
        shutil.rmtree(mnt, ignore_errors=True)
        for path in out.iterdir():
            os.chmod(path, 0o644)
    return 0


def unpack(report: pathlib.Path, target: pathlib.Path) -> None:
    shutil.rmtree(target, ignore_errors=True)
    target.mkdir(parents=True, exist_ok=True)
    if not report.exists() or report.stat().st_size == 0:
        print(f"nothing arrived from the guest ({report})")
        return
    try:
        with tarfile.open(report) as archive:
            members = [m for m in archive.getmembers()
                       if (m.isfile() or m.isdir()) and not m.name.startswith("/") and ".." not in m.name]
            archive.extractall(target, members=members, filter="data")
    except (tarfile.TarError, OSError) as error:
        print(f"what the guest sent could not be read: {error}")


def load(path: pathlib.Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}


def read_status(path: pathlib.Path) -> dict:
    values = {}
    try:
        for line in path.read_text().splitlines():
            key, _, value = line.partition("=")
            values[key] = value
    except OSError:
        pass
    return values


def judge(out: pathlib.Path, timeline: list[dict]) -> int:
    guest = out / "guest" / "tur-windows-check"
    status = read_status(guest / "status")
    codes = {}
    try:
        for line in (guest / "exit-status.tsv").read_text().splitlines():
            name, _, code = line.partition("\t")
            codes[name] = int(code) if code.lstrip("-").isdigit() else code
    except OSError:
        pass
    apps = load(guest / "evidence" / "apps" / "results.json")
    autopilot = load(guest / "evidence" / "autopilot" / "autopilot.json")
    by_id = {result["id"]: result for result in apps.get("results", [])}
    checks: list[dict] = []

    def add(name: str, required: bool, ok: bool, detail: str) -> None:
        checks.append({"check": name, "required": required, "ok": bool(ok), "detail": detail})

    add("selinux_enforcing", True,
        status.get("selinux") == "Enforcing" and status.get("selinux_after") == "Enforcing",
        f"before: {status.get('selinux')}, after: {status.get('selinux_after')}")
    add("session", True, bool(status.get("plasmashell_pid")),
        f"plasmashell {status.get('plasmashell_pid') or 'did not start'}")
    environment = apps.get("environment", {})
    add("wine_from_the_image", True, str(environment.get("wine", "")).startswith("wine-11")
        and environment.get("tur_image") is True,
        f"{environment.get('wine')}; context {environment.get('context')}; ntsync {environment.get('ntsync')}")
    for app_id in [*REQUIRED_APPS, *[i for i in by_id if i not in REQUIRED_APPS]]:
        result = by_id.get(app_id)
        if result is None:
            add(f"app_{app_id}", app_id in REQUIRED_APPS, False, "not run")
            continue
        stages = {stage: (result.get(stage) or {}).get("status") for stage in STAGES}
        detail = ", ".join(f"{stage} {value}" for stage, value in stages.items())
        problems = [f"{stage}: {(result.get(stage) or {}).get('detail')}" for stage in STAGES
                    if stages[stage] != "pass" and (result.get(stage) or {}).get("detail")]
        add(f"app_{app_id}", app_id in REQUIRED_APPS, all(v == "pass" for v in stages.values()),
            detail + (("; " + " | ".join(problems))[:1500] if problems else ""))
    started = (by_id.get("7zip-exe") or {}).get("launcher_start") or {}
    add("launcher_7zip", True, started.get("status") == "pass",
        f"{started.get('status', 'not run')}: {started.get('detail') or started.get('window')}")
    add("outside_the_sandbox", True, not environment.get("outside_the_sandbox"),
        f"{environment.get('outside_the_sandbox') or 'nothing appeared in the home directory'}")

    install = load(guest / "tur-apps-install.out") if (guest / "tur-apps-install.out").exists() else {}
    entries = (guest / "tur-apps-entries.out").read_text(errors="replace") if (guest / "tur-apps-entries.out").exists() else ""
    after = (guest / "tur-apps-after.out").read_text(errors="replace") if (guest / "tur-apps-after.out").exists() else ""
    add("tur_apps_7zip", True,
        codes.get("tur-apps-install") == 0 and codes.get("tur-apps-remove") == 0
        and "tur-wine-" in entries and "tur-wine-" not in after.split("[")[0],
        f"plan {codes.get('tur-apps-plan')}, install {codes.get('tur-apps-install')}, list "
        f"{codes.get('tur-apps-list')}, remove {codes.get('tur-apps-remove')}; entry while installed: "
        f"{'yes' if 'tur-wine-' in entries else 'no'}; {str(install)[:200]}")
    real = (autopilot.get("real") or [{}])[0] if autopilot.get("real") else {}
    add("autopilot_putty", True, bool(autopilot) and not autopilot.get("failed"),
        f"unchanged: {(real.get('unchanged') or {}).get('outcome')}; fault {real.get('fault')}: class "
        f"{real.get('class')}, found {real.get('found')}, outcome {real.get('outcome')}, after apply "
        f"{real.get('after_apply')}; failed checks: {autopilot.get('failed')}")
    avc = (guest / "avc-journal.txt").read_text(errors="replace").strip() if (guest / "avc-journal.txt").exists() else ""
    denials = [line for line in avc.splitlines() if "denied" in line]
    add("selinux_denials", False, not denials, f"{len(denials)} denial(s); see guest/tur-windows-check/avc-*.txt")

    failed = [c for c in checks if c["required"] and not c["ok"]]
    document = {"checks": checks, "status": status, "exit_status": codes, "timeline": timeline,
                "failed_required": [c["check"] for c in failed]}
    (out / "windows.json").write_text(json.dumps(document, indent=2) + "\n")
    lines = ["## Windows applications on the installed TUR system", "",
             "| Check | Required | Result | Detail |", "|---|---|---|---|"]
    for c in checks:
        lines.append(f"| {c['check']} | {'yes' if c['required'] else 'no'} | {'pass' if c['ok'] else '**FAIL**'} | "
                     f"{c['detail'].replace('|', '/').replace(chr(10), ' ')[:600]} |")
    table = guest / "evidence" / "apps" / "compatibility.md"
    if table.exists():
        lines += ["", table.read_text()]
    table = guest / "evidence" / "autopilot" / "autopilot.md"
    if table.exists():
        lines += ["", table.read_text()]
    (out / "windows.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 1 if failed else 0


def boot(work: pathlib.Path, out_root: pathlib.Path, seconds: int, only: str | None) -> int:
    src = work / "windows"
    out = out_root / "windows"
    shots = out / "shots"
    shots.mkdir(parents=True, exist_ok=True)
    append = (src / "cmdline.txt").read_text().strip()
    variables = work / "OVMF_VARS.prepared.fd"
    fw_cfg = {"windows-check/only": only} if only else {}
    machine = splash.DirectMachine(
        kernel=src / "vmlinuz", initrd=src / "initramfs.img", append=append,
        disk=str(work / "disk.raw"), work=work, name="tur-windows", cpus=4, memory=6144,
        serial_log=out / "serial.log", report_tar=work / "windows-report.tar", qemu_log=out / "qemu.log",
        variables_from=str(variables) if variables.exists() else None, fw_cfg=fw_cfg,
    )
    for stale in (machine.variables, out / "serial.log", machine.report_tar):
        stale.unlink(missing_ok=True)
    timeline: list[dict] = []
    started = time.monotonic()
    qmp = machine.start()
    seen = 0
    done = False
    names: set[str] = set()
    # A picture every two minutes, whatever the guest asks for, so that a
    # dialog nobody expected is seen.
    next_periodic = started + 120
    periodic = 0
    try:
        while time.monotonic() - started < seconds and machine.alive() and not done:
            time.sleep(0.5)
            log = out / "serial.log"
            if not log.exists():
                continue
            # Complete lines only: a line still being written is read next time.
            lines = vm.clean(log.read_bytes().decode("utf-8", errors="replace")).split("\n")[:-1]
            fresh, seen = lines[seen:], len(lines)
            for line in fresh:
                at = round(time.monotonic() - started, 1)
                if vm.MARK not in line:
                    continue
                said = line.split(vm.MARK, 1)[1].strip()
                if said.startswith("WINDOWS-SHOT "):
                    name = "".join(c for c in said.split(" ", 1)[1] if c.isalnum() or c in "._-")[:120] or "shot"
                    while name in names:
                        name += "-again"
                    names.add(name)
                    try:
                        shot = vm.screenshot(qmp, work, shots / f"{name}.png")
                        (shots / f"{name}.ppm.tmp").unlink(missing_ok=True)
                        timeline.append({"at": at, "shot": name, **shot})
                    except Exception as error:  # noqa: BLE001 - a missed picture is recorded
                        timeline.append({"at": at, "shot": name, "error": str(error)})
                elif said.startswith("WINDOWS-KEY "):
                    key = said.split(" ", 1)[1].strip()
                    try:
                        qmp.command("send-key", keys=[{"type": "qcode", "data": key}])
                        timeline.append({"at": at, "key": key})
                    except Exception as error:  # noqa: BLE001
                        timeline.append({"at": at, "key": key, "error": str(error)})
                elif said.startswith("WINDOWS-CHECK"):
                    timeline.append({"at": at, "guest": said})
                    if said.startswith("WINDOWS-CHECK session ready"):
                        shot = vm.screenshot(qmp, work, shots / "000-session.png")
                        (shots / "000-session.ppm.tmp").unlink(missing_ok=True)
                    if said.startswith("WINDOWS-CHECK-DONE"):
                        done = True
            if time.monotonic() >= next_periodic and periodic < 60:
                next_periodic += 120
                periodic += 1
                try:
                    name = f"periodic-{periodic:02d}-{round(time.monotonic() - started)}s"
                    vm.screenshot(qmp, work, shots / f"{name}.png")
                    (shots / f"{name}.ppm.tmp").unlink(missing_ok=True)
                except Exception:  # noqa: BLE001 - a missed picture is not a failure
                    pass
            qmp.drain()
        if not done:
            timeline.append({"at": round(time.monotonic() - started, 1),
                             "error": "the guest did not finish" if machine.alive() else "QEMU exited"})
            if machine.alive():
                try:
                    vm.screenshot(qmp, work, shots / "zzz-timeout.png")
                    (shots / "zzz-timeout.ppm.tmp").unlink(missing_ok=True)
                except Exception:  # noqa: BLE001
                    pass
        time.sleep(3)
    finally:
        machine.quit()
        machine.variables.unlink(missing_ok=True)
    print(f"{len(names)} screenshots in {shots}; finished: {done}; "
          f"{round(time.monotonic() - started)} s")
    unpack(machine.report_tar, out / "guest")
    return judge(out, timeline)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="what", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("disk")
    p.add_argument("work")
    b = sub.add_parser("boot")
    b.add_argument("work")
    b.add_argument("out")
    b.add_argument("--seconds", type=int, default=5400)
    b.add_argument("--only", help="comma-separated ids of tests/windows-runtime/apps.json")
    j = sub.add_parser("judge", help="judge what an earlier boot brought back")
    j.add_argument("out")
    arguments = parser.parse_args()
    if arguments.what == "prepare":
        return prepare(arguments.disk, pathlib.Path(arguments.work).resolve())
    if arguments.what == "judge":
        return judge(pathlib.Path(arguments.out).resolve() / "windows", [])
    return boot(pathlib.Path(arguments.work).resolve(), pathlib.Path(arguments.out).resolve(), arguments.seconds,
                arguments.only)


if __name__ == "__main__":
    sys.exit(main())

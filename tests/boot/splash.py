#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The boot splash and the first desktop, seen in one extra boot of the tested disk.

    sudo tests/boot/splash.py prepare DISK WORK_DIR
    tests/boot/splash.py boot WORK_DIR OUT_DIR [--seconds 180]

The boot test's own boots do not show TUR's boot splash: the image sets no
`rhgb` or `splash` kernel argument, so Plymouth shows text, and the serial
console the test reads must stay the primary console. The boot test also
never signs in. This adds one boot, after the test has finished with the disk
(TUR-008):

- `prepare` (as root) reads the loader entry from the installed disk, copies
  its kernel and initramfs out, and writes an automatic sign-in for the boot
  test's user into that deployment's /etc. It is the last thing done to the
  disk.
- `boot` starts the machine directly from that kernel and initramfs with the
  entry's arguments, minus the boot test's marker and with `rhgb quiet` and
  the graphical console last, on a throw-away overlay (QEMU's -snapshot). It
  records the display every half second while it changes, then the signed-in
  desktop, and stops the machine. SELinux is permissive in this boot only,
  because the sign-in file was written by a host that cannot label it.

Writes OUT_DIR/splash/frame-*.png, OUT_DIR/screenshot-desktop.png and
OUT_DIR/splash/splash.json. The splash part is informational:
tests/boot/screenshot-text.py reads the pictures, and a person looks at them.

The same boot is the boot test's user's first Plasma session: a new account
(task D-04). `prepare` adds the kernel argument `tur.desktop_check`, which
starts the test image's desktop check in the guest. After the picture of the
desktop, `boot` waits for the guest to switch the account to TUR Light and
takes OUT_DIR/screenshot-desktop-light.png; then it reads the guest's facts
into OUT_DIR/desktop/ and judges them (tests/boot/desktop_checks.py). A
required desktop check that fails fails this step.
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
import desktop_checks  # noqa: E402
import vm  # noqa: E402

TEST_USER = "turtest"  # tests/boot/guest/rootfs/usr/libexec/tur-boot-test/guest-agent
ROOT_X86_64 = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709"  # the GPT type of an x86-64 root partition


def run(*cmd: str, check: bool = True) -> str:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed ({result.returncode}): {result.stderr.strip()}")
    return result.stdout


def prepare(disk: str, work: pathlib.Path) -> int:
    out = work / "splash"
    out.mkdir(parents=True, exist_ok=True)
    loop = run("losetup", "--find", "--show", "--partscan", disk).strip()
    mnt = pathlib.Path(tempfile.mkdtemp(prefix="tur-splash-"))
    mounted = False
    try:
        run("udevadm", "settle", "--timeout=30", check=False)
        # The partition table says which partition is the root (by its type,
        # as tests/boot/inspect-disk.py reads it); its device node may need a
        # moment, or a nudge, to appear.
        table = json.loads(run("sfdisk", "--json", loop))["partitiontable"]["partitions"]
        root = next((p["node"] for p in table if p["type"].lower() == ROOT_X86_64), None)
        if not root:
            raise RuntimeError(f"no x86-64 root partition in {table}")
        for _ in range(30):
            if os.path.exists(root):
                break
            run("partx", "--add", loop, check=False)
            time.sleep(1)
        run("mount", root, str(mnt))
        mounted = True
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
            if word.startswith("console=") or word.startswith("tur.boot_test"):
                continue
            # The GRUB variable the counting fragment fills in (ADR-0014); an
            # ordinary, counted-as-good boot sets it to this.
            words.append("tur.boot_contract=1" if word == "$tur_boot_args" else word)
        # Plymouth shows text, not the splash, when a serial console is
        # listed, unless told to leave serial consoles alone (seen in the
        # first run of this step).
        words += ["console=ttyS0,115200n8", "console=tty0", "rhgb", "quiet", "plymouth.ignore-serial-consoles",
                  "enforcing=0"]
        # The test image's desktop check (task D-04) runs in this boot only.
        words += ["tur.desktop_check"]
        (out / "cmdline.txt").write_text(" ".join(words) + "\n")

        # The deployment's /etc, by the ostree= argument (a link inside /ostree).
        ostree = next(w.split("=", 1)[1] for w in words if w.startswith("ostree="))
        deployment = pathlib.Path(os.path.realpath(mnt / ostree.lstrip("/")))
        if not str(deployment).startswith(str(mnt) + "/") or not (deployment / "etc").is_dir():
            raise RuntimeError(f"{ostree} does not lead to a deployment on the disk ({deployment})")
        conf = deployment / "etc/plasmalogin.conf.d"
        conf.mkdir(exist_ok=True)
        (conf / "zz-tur-splash-test.conf").write_text(
            "# Written by tests/boot/splash.py for one throw-away boot.\n"
            f"[Autologin]\nUser={TEST_USER}\nSession=plasma.desktop\n"
        )
        (out / "entry.json").write_text(json.dumps({"file": entries[-1].name, **entry}, indent=2) + "\n")
        print(f"loader entry {entries[-1].name}: {entry.get('title')}")
        print(f"command line: {' '.join(words)}")
        print(f"automatic sign-in for {TEST_USER} in {conf.relative_to(mnt)}")
    finally:
        if mounted:
            run("umount", str(mnt), check=False)
        run("losetup", "--detach", loop, check=False)
        shutil.rmtree(mnt, ignore_errors=True)
        for path in out.iterdir():
            os.chmod(path, 0o644)
    return 0


class DirectMachine(vm.Machine):
    """The boot test's machine, started from a kernel and initramfs on an overlay."""

    def __init__(self, *, kernel: pathlib.Path, initrd: pathlib.Path, append: str, **kwargs):
        super().__init__(**kwargs)
        self.kernel, self.initrd, self.append = kernel, initrd, append

    def command(self) -> list[str]:
        return super().command() + ["-snapshot", "-kernel", str(self.kernel), "-initrd", str(self.initrd),
                                    "-append", self.append]


# Seconds after the picture of the dark desktop at which the guest switches to
# TUR Light (it counts from its own start, which is a moment after QEMU's).
LIGHT_AFTER = 30


def wait_for_marker(machine: vm.Machine, log: pathlib.Path, marker: str, timeout: float) -> bool:
    """Wait until the guest prints TUR-BOOT-TEST: <marker> on the serial console."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline and machine.alive():
        if log.exists() and (vm.MARK + marker) in vm.clean(log.read_bytes().decode("utf-8", errors="replace")):
            return True
        time.sleep(1)
    return False


def unpack_desktop_facts(report: pathlib.Path, target: pathlib.Path) -> None:
    """The guest's desktop check sends one tar stream over the virtio-serial port."""
    shutil.rmtree(target, ignore_errors=True)
    target.mkdir(parents=True, exist_ok=True)
    if not report.exists() or report.stat().st_size == 0:
        print(f"no desktop facts arrived ({report})")
        return
    try:
        with tarfile.open(report) as archive:
            members = [m for m in archive.getmembers()
                       if (m.isfile() or m.isdir()) and not m.name.startswith("/") and ".." not in m.name]
            archive.extractall(target, members=members, filter="data")
    except (tarfile.TarError, OSError) as error:
        print(f"the desktop facts could not be read: {error}")


def boot(work: pathlib.Path, out: pathlib.Path, seconds: int) -> int:
    src = work / "splash"
    shots = out / "splash"
    shots.mkdir(parents=True, exist_ok=True)
    append = (src / "cmdline.txt").read_text().strip()
    variables = work / "OVMF_VARS.prepared.fd"
    machine = DirectMachine(
        kernel=src / "vmlinuz", initrd=src / "initramfs.img", append=append,
        disk=str(work / "disk.raw"), work=work, name="tur-splash",
        serial_log=shots / "serial.log", report_tar=work / "splash-report.tar", qemu_log=shots / "qemu.log",
        variables_from=str(variables) if variables.exists() else None,
        # When the guest's desktop check may switch to TUR Light: after the
        # picture of the dark desktop, which is taken at `seconds`.
        fw_cfg={"desktop-check/light-at": str(seconds + LIGHT_AFTER)},
    )
    for stale in (machine.variables, shots / "serial.log", machine.report_tar):
        stale.unlink(missing_ok=True)
    result: dict = {"command_line": append, "frames": [], "desktop": None}
    started = time.monotonic()
    qmp = machine.start()
    ppm = work / "splash.ppm"
    last = work / "splash-last.ppm"
    try:
        while time.monotonic() - started < seconds:
            time.sleep(0.5)
            if not machine.alive():
                result["error"] = "QEMU exited"
                break
            try:
                ppm.unlink(missing_ok=True)
                qmp.command("screendump", filename=str(ppm))
                qmp.drain()
            except Exception as error:  # noqa: BLE001 - a missed frame is not a failure
                result.setdefault("missed", []).append(str(error))
                continue
            changed = vm.changed_fraction(last, ppm) if last.exists() else 1.0
            if changed is None or changed >= 0.01:
                width, height, rgb = vm.read_ppm(ppm)
                at = round(time.monotonic() - started, 1)
                name = f"frame-{len(result['frames']) + 1:03d}-{at:05.1f}s.png"
                if len(result["frames"]) < 80:
                    vm.write_png(shots / name, width, height, rgb)
                    result["frames"].append({"file": name, "at_seconds": at, "changed": changed,
                                             **vm.describe_image(width, height, rgb)})
                shutil.copyfile(ppm, last)
        if machine.alive():
            shot = vm.screenshot(qmp, work, out / "screenshot-desktop.png")
            shot["at_seconds"] = round(time.monotonic() - started, 1)
            result["desktop"] = shot
            (out / "screenshot-desktop.ppm.tmp").unlink(missing_ok=True)
        # Task D-04: the guest switches the account to TUR Light; then its facts.
        log = shots / "serial.log"
        if machine.alive() and wait_for_marker(machine, log, "DESKTOP-LIGHT-APPLIED", LIGHT_AFTER + 240):
            time.sleep(3)
            shot = vm.screenshot(qmp, work, out / "screenshot-desktop-light.png")
            shot["at_seconds"] = round(time.monotonic() - started, 1)
            result["desktop_light"] = shot
            (out / "screenshot-desktop-light.ppm.tmp").unlink(missing_ok=True)
        result["desktop_check_sent"] = machine.alive() and wait_for_marker(machine, log, "DESKTOP-CHECK-DONE sent", 150)
    finally:
        machine.quit()
        machine.variables.unlink(missing_ok=True)
    (shots / "splash.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{len(result['frames'])} frames in {shots}; desktop: {result['desktop']}")
    print(f"light desktop: {result.get('desktop_light')}; facts sent: {result.get('desktop_check_sent')}")
    unpack_desktop_facts(machine.report_tar, out / "desktop")
    return desktop_checks.write(out, desktop_checks.judge(out))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="what", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("disk")
    p.add_argument("work")
    b = sub.add_parser("boot")
    b.add_argument("work")
    b.add_argument("out")
    b.add_argument("--seconds", type=int, default=180)
    args = ap.parse_args()
    if args.what == "prepare":
        return prepare(args.disk, pathlib.Path(args.work).resolve())
    return boot(pathlib.Path(args.work).resolve(), pathlib.Path(args.out).resolve(), args.seconds)


if __name__ == "__main__":
    sys.exit(main())

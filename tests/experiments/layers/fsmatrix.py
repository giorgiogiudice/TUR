#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What overlayfs does, mounted without privilege, on each file system and
with each way of case folding: the layers experiment's file-system matrix.

    fsmatrix.py --fs-root DIR --user NAME --out FILE.json     (as root)

DIR holds mounted file systems named ext4, ext4cf (ext4 with the casefold
feature) and btrfs, each owned by NAME (fs-setup.sh makes them). For every
case below, as NAME:

  1. the layers are prepared: directories given the case-folding attribute
     where the case says so, while they are empty, then filled;
  2. in a new user and mount namespace (`unshare --user --map-root-user
     --mount`: no privilege on the machine), overlayfs is mounted with the
     `userxattr` option and a list of operations is tried, each recorded as
     it went, with the error when there was one;
  3. the same layers are mounted by bubblewrap's --overlay, as the sandbox
     would.

Root is used only to read the kernel's messages after each case (overlayfs
says why it refused a mount there and nowhere else). Nothing here needs Wine.
"""

from __future__ import annotations

import argparse
import errno
import fcntl
import json
import os
import pathlib
import pwd
import shutil
import struct
import subprocess
import sys
import time

FS_IOC_GETFLAGS = 0x80086601
FS_CASEFOLD_FL = 0x40000000
MIB = 1 << 20

# name, file system of the lower layers, of the upper layer, and which
# directories get the case-folding attribute: "lower", "upper", "work"
# (the whole layer, from its root) or "lower-sub", "upper-sub" (only the
# directory Dir inside it, as a prefix has a folded drive_c in an ordinary
# directory).
CASES = [
    ("ext4", "ext4", "ext4", [], 1),
    ("ext4, three lower layers", "ext4", "ext4", [], 3),
    ("btrfs", "btrfs", "btrfs", [], 1),
    ("lower on ext4, upper on btrfs", "ext4", "btrfs", [], 1),
    ("ext4 with the casefold feature, nothing folded", "ext4cf", "ext4cf", [], 1),
    ("casefold: lower, upper and work folded", "ext4cf", "ext4cf", ["lower", "upper", "work"], 1),
    ("casefold: lower, upper and work folded, three lower layers", "ext4cf", "ext4cf",
     ["lower", "upper", "work"], 3),
    ("casefold: lower and upper folded, work not", "ext4cf", "ext4cf", ["lower", "upper"], 1),
    ("casefold: lower folded, upper not", "ext4cf", "ext4cf", ["lower"], 1),
    ("casefold: upper folded, lower not", "ext4cf", "ext4cf", ["upper", "work"], 1),
    ("casefold: a folded directory inside an ordinary lower layer", "ext4cf", "ext4cf", ["lower-sub"], 1),
    ("casefold: a folded directory inside ordinary lower and upper layers", "ext4cf", "ext4cf",
     ["lower-sub", "upper-sub"], 1),
    ("casefold: folded lower on ext4, upper on btrfs", "ext4cf", "btrfs", ["lower"], 1),
]


def run(arguments: list[str], timeout: int = 300, **options) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(arguments, capture_output=True, text=True, errors="replace", timeout=timeout, **options)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(arguments, 124, "", f"timed out after {timeout}s")
    except OSError as error:
        return subprocess.CompletedProcess(arguments, 127, "", str(error))


def chattr_fold(directory: pathlib.Path) -> str:
    done = run(["chattr", "+F", str(directory)], env={"LC_ALL": "C", "PATH": "/usr/bin:/usr/sbin:/bin"})
    return "set" if done.returncode == 0 else f"refused: {done.stderr.strip()}"


def folded(path: pathlib.Path) -> bool | str:
    try:
        fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    except OSError as error:
        return f"open: {errno.errorcode.get(error.errno, error.errno)}"
    try:
        flags = struct.unpack("i", fcntl.ioctl(fd, FS_IOC_GETFLAGS, b"\0" * 8)[:4])[0]
        return bool(flags & FS_CASEFOLD_FL)
    except OSError as error:
        return f"ioctl: {errno.errorcode.get(error.errno, error.errno)}"
    finally:
        os.close(fd)


# ----------------------------------------------------------------- prepare


def prepare(spec: dict) -> dict:
    """As the user: the layers of one case."""
    lower_root, upper_root = pathlib.Path(spec["lower_dir"]), pathlib.Path(spec["upper_dir"])
    for root in (lower_root, upper_root):
        shutil.rmtree(root, ignore_errors=True)
        root.mkdir(parents=True)
    notes: dict = {}
    lowers = [lower_root / f"lower{index}" for index in range(spec["lowers"])]
    for directory in lowers:
        directory.mkdir()
        if "lower" in spec["fold"]:
            notes[f"fold {directory.name}"] = chattr_fold(directory)
    for name in ("upper", "work", "upper2", "work2", "upper3", "work3"):
        directory = upper_root / name
        directory.mkdir()
        if name.rstrip("23") in spec["fold"]:
            notes[f"fold {name}"] = chattr_fold(directory)
    (upper_root / "merged").mkdir()
    (upper_root / "merged3").mkdir()
    bottom = lowers[0]
    (bottom / "Dir").mkdir()
    if "lower-sub" in spec["fold"]:
        notes["fold lower Dir"] = chattr_fold(bottom / "Dir")
    if "upper-sub" in spec["fold"]:
        for name in ("upper", "upper2", "upper3"):
            (upper_root / name / "Dir").mkdir()
            notes[f"fold {name} Dir"] = chattr_fold(upper_root / name / "Dir")
    (bottom / "Dir" / "File.txt").write_text("lower")
    (bottom / "Dir" / "Sub").mkdir()
    (bottom / "Dir" / "Sub" / "deep.txt").write_text("deep")
    (bottom / "del.txt").write_text("delete me")
    (bottom / "Windows" / "System32").mkdir(parents=True)
    (bottom / "Windows" / "System32" / "kernel32.dll").write_bytes(b"MZ" + b"\0" * 4094)
    block = os.urandom(MIB)
    for name in ("big.bin", "big2.bin"):
        with open(bottom / name, "wb") as handle:
            for _ in range(64):
                handle.write(block)
    (bottom / "many").mkdir()
    for index in range(2000):
        (bottom / "many" / f"f{index:04d}.dat").write_bytes(b"x")
    # Higher lower layers: one file each, and one that shadows the bottom's.
    for index, directory in enumerate(lowers[1:], start=1):
        (directory / "Dir").mkdir()
        (directory / "Dir" / f"from-layer-{index}.txt").write_text(f"layer {index}")
        (directory / "which.txt").write_text(f"layer {index}")
    (bottom / "which.txt").write_text("layer 0")
    os.sync()
    notes["lower root folded"] = folded(bottom)
    notes["lower Dir folded"] = folded(bottom / "Dir")
    notes["upper root folded"] = folded(upper_root / "upper")
    return notes


# ------------------------------------------------------------------- child


def attempt(results: dict, name: str, function) -> None:
    started = time.monotonic()
    try:
        value = function()
        results[name] = {"ok": True, "seconds": round(time.monotonic() - started, 4)}
        if value is not None:
            results[name]["value"] = value
    except OSError as error:
        results[name] = {"ok": False, "error": errno.errorcode.get(error.errno, str(error.errno)),
                         "message": error.strerror}
    except Exception as error:  # noqa: BLE001
        results[name] = {"ok": False, "error": type(error).__name__, "message": str(error)[:200]}


def expect(condition: bool, text: str):
    if not condition:
        raise AssertionError(text)


def child(spec: dict) -> dict:
    """Inside the new user and mount namespace."""
    lower_root, upper_root = pathlib.Path(spec["lower_dir"]), pathlib.Path(spec["upper_dir"])
    lowers = [lower_root / f"lower{index}" for index in range(spec["lowers"])]
    merged = upper_root / "merged"
    upper = upper_root / "upper"
    # In the mount option the first directory is the top one.
    lowerdir = ":".join(str(directory) for directory in reversed(lowers))
    options = f"lowerdir={lowerdir},upperdir={upper},workdir={upper_root / 'work'},userxattr"
    mount = run(["mount", "-t", "overlay", "overlay", "-o", options, str(merged)])
    out: dict = {"mount": {"ok": mount.returncode == 0, "stderr": mount.stderr.strip()[-400:]}, "ops": {}}
    if mount.returncode != 0:
        return out
    with open("/proc/self/mountinfo") as handle:
        out["mountinfo"] = next((line.strip()[-400:] for line in handle if f" {merged} " in line), None)
    ops = out["ops"]
    bottom = lowers[0]

    attempt(ops, "read a lower file", lambda: expect((merged / "Dir" / "File.txt").read_text() == "lower", "content"))
    attempt(ops, "higher lower layer wins", lambda: (merged / "which.txt").read_text())
    attempt(ops, "create a file", lambda: ((merged / "Dir" / "new.txt").write_text("new"),
                                           expect((upper / "Dir" / "new.txt").is_file(), "not in the upper layer"))[1])

    def modify():
        with open(merged / "Dir" / "File.txt", "a") as handle:
            handle.write("+upper")
        expect((bottom / "Dir" / "File.txt").read_text() == "lower", "the lower file changed")
        expect((merged / "Dir" / "File.txt").read_text() == "lower+upper", "the change is not visible")
    attempt(ops, "change a lower file (copy up)", modify)

    def delete():
        os.unlink(merged / "del.txt")
        expect((bottom / "del.txt").is_file(), "the lower file is gone")
        expect(not (merged / "del.txt").exists(), "still visible")
        info = os.lstat(upper / "del.txt")
        return f"whiteout: mode {oct(info.st_mode)}, device {info.st_rdev}"
    attempt(ops, "delete a lower file (whiteout)", delete)
    attempt(ops, "list a merged directory", lambda: sorted(os.listdir(merged / "Dir")))
    attempt(ops, "rename a lower file", lambda: os.rename(merged / "Dir" / "Sub" / "deep.txt",
                                                          merged / "Dir" / "Sub" / "deep2.txt"))

    def rename_new_directory():
        (merged / "NewDir").mkdir()
        (merged / "NewDir" / "f").write_text("x")
        os.rename(merged / "NewDir", merged / "NewDir2")
    attempt(ops, "rename a directory made in the upper layer", rename_new_directory)
    attempt(ops, "rename a directory of a lower layer", lambda: os.rename(merged / "Dir" / "Sub",
                                                                         merged / "Dir" / "Sub2"))

    def replace_tree():
        shutil.rmtree(merged / "Windows")
        (merged / "Windows").mkdir()
        expect(os.listdir(merged / "Windows") == [], "the lower directory's entries show through")
        expect((bottom / "Windows" / "System32" / "kernel32.dll").is_file(), "the lower tree is gone")
    attempt(ops, "delete a lower tree and make the directory again", replace_tree)
    attempt(ops, "hard link", lambda: os.link(merged / "Dir" / "new.txt", merged / "Dir" / "new-link.txt"))
    attempt(ops, "symbolic link", lambda: os.symlink("new.txt", merged / "Dir" / "new-symlink"))
    attempt(ops, "user extended attribute", lambda: os.setxattr(merged / "Dir" / "new.txt", "user.tur", b"1"))

    def lock():
        with open(merged / "Dir" / "new.txt", "r+") as handle:
            fcntl.lockf(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    attempt(ops, "byte-range lock", lock)

    def copy_up_by_chmod():
        os.chmod(merged / "big.bin", 0o600)
        return {"upper_bytes_allocated": os.stat(upper / "big.bin").st_blocks * 512}
    attempt(ops, "chmod of a 64 MiB lower file (copy up)", copy_up_by_chmod)

    def copy_up_by_open():
        os.close(os.open(merged / "big2.bin", os.O_RDWR))
        return {"upper_bytes_allocated": os.stat(upper / "big2.bin").st_blocks * 512}
    attempt(ops, "opening a 64 MiB lower file for writing, writing nothing (copy up)", copy_up_by_open)

    # Case. What a Windows program would rely on.
    attempt(ops, "case: the merged Dir has the case-folding attribute", lambda: folded(merged / "Dir"))
    attempt(ops, "case: a lower file found under another case",
            lambda: os.path.exists(merged / "Dir" / "FILE.TXT"))
    attempt(ops, "case: a lower directory found under another case",
            lambda: os.path.exists(merged / "DIR" / "File.txt"))

    def created_other_case():
        (merged / "Dir" / "CaseNew.txt").write_text("1")
        return os.path.exists(merged / "Dir" / "casenew.TXT")
    attempt(ops, "case: a new file found under another case", created_other_case)

    def write_other_case():
        (merged / "Dir" / "FILE.TXT").write_text("other case")
        names = [name for name in os.listdir(merged / "Dir") if name.lower() == "file.txt"]
        return {"names": names, "File.txt reads": (merged / "Dir" / "File.txt").read_text()}
    attempt(ops, "case: writing a lower file under another case", write_other_case)

    def delete_other_case():
        (merged / "Dir" / "Gone.txt").write_text("1")
        os.unlink(merged / "Dir" / "GONE.TXT")
        return not os.path.exists(merged / "Dir" / "Gone.txt")
    attempt(ops, "case: deleting under another case", delete_other_case)

    # What a lookup costs through the overlay, against the lower layer itself.
    def stat_many(directory: pathlib.Path) -> float:
        started = time.perf_counter()
        for index in range(2000):
            os.stat(directory / f"f{index:04d}.dat")
        return round((time.perf_counter() - started) / 2000 * 1e6, 2)
    attempt(ops, "stat of 2000 lower files through the overlay, first pass (microseconds each)",
            lambda: stat_many(merged / "many"))
    attempt(ops, "the same, second pass", lambda: stat_many(merged / "many"))
    attempt(ops, "the same files on the lower layer itself", lambda: stat_many(bottom / "many"))

    def missing_many() -> float:
        started = time.perf_counter()
        for index in range(2000):
            os.path.exists(merged / "many" / f"nothing{index:04d}.dat")
        return round((time.perf_counter() - started) / 2000 * 1e6, 2)
    attempt(ops, "2000 names that are not there, through the overlay (microseconds each)", missing_many)

    # The same upper layer mounted a second time while the first is mounted.
    again = run(["mount", "-t", "overlay", "overlay", "-o", options, str(upper_root / "merged3")])
    out["second_mount_of_same_upper"] = {"ok": again.returncode == 0, "stderr": again.stderr.strip()[-300:]}
    return out


# ------------------------------------------------------------------ parent


def kernel_messages() -> list[str]:
    done = run(["dmesg", "--read-clear", "--notime"], timeout=30)
    return [line for line in done.stdout.splitlines() if "overlay" in line.lower() or "casefold" in line.lower()][-12:]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fs-root", type=pathlib.Path)
    parser.add_argument("--user")
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--prepare")
    parser.add_argument("--child")
    options = parser.parse_args()
    if options.prepare:
        print(json.dumps(prepare(json.loads(options.prepare))))
        return 0
    if options.child:
        print(json.dumps(child(json.loads(options.child))))
        return 0

    account = pwd.getpwnam(options.user)
    as_user = ["setpriv", f"--reuid={account.pw_uid}", f"--regid={account.pw_gid}", "--clear-groups"]
    me = [sys.executable, str(pathlib.Path(__file__).resolve())]
    kernel_messages()
    report: dict = {"kernel": os.uname().release, "cases": []}
    report["mount_version"] = run(["mount", "--version"]).stdout.strip()
    report["bubblewrap"] = run(["bwrap", "--version"]).stdout.strip()
    for path in ("/sys/fs/ext4/features/casefold", "/sys/module/overlay/parameters/redirect_dir",
                 "/sys/module/overlay/parameters/index", "/sys/module/overlay/parameters/metacopy",
                 "/sys/module/overlay/parameters/xino_auto", "/sys/module/overlay/parameters/nfs_export"):
        try:
            report[path] = pathlib.Path(path).read_text().strip()
        except OSError as error:
            report[path] = f"not readable: {error.strerror}"
    for number, (name, lower_fs, upper_fs, fold, lowers) in enumerate(CASES):
        entry: dict = {"case": name, "lower_fs": lower_fs, "upper_fs": upper_fs, "fold": fold, "lowers": lowers}
        lower_mount, upper_mount = options.fs_root / lower_fs, options.fs_root / upper_fs
        if not lower_mount.is_dir() or not upper_mount.is_dir():
            entry["status"] = "NOT RUN: a file system is missing"
            report["cases"].append(entry)
            continue
        spec = {"lower_dir": str(lower_mount / "matrix" / f"case{number}-lower"),
                "upper_dir": str(upper_mount / "matrix" / f"case{number}-upper"), "fold": fold, "lowers": lowers}
        for directory in (lower_mount / "matrix", upper_mount / "matrix"):
            directory.mkdir(exist_ok=True)
            os.chown(directory, account.pw_uid, account.pw_gid)
        done = run(as_user + me + ["--prepare", json.dumps(spec)])
        try:
            entry["prepared"] = json.loads(done.stdout)
        except json.JSONDecodeError:
            entry["status"] = f"prepare failed: {done.stderr.strip()[-400:]}"
            report["cases"].append(entry)
            continue
        done = run(as_user + ["unshare", "--user", "--map-root-user", "--mount"] + me + ["--child", json.dumps(spec)])
        try:
            entry.update(json.loads(done.stdout))
        except json.JSONDecodeError:
            entry["status"] = f"exit {done.returncode}: {done.stderr.strip()[-400:]}"
        entry["kernel_messages"] = kernel_messages()
        # The same through bubblewrap, on a second upper layer.
        upper_root = pathlib.Path(spec["upper_dir"])
        sources: list[str] = []
        for index in range(lowers):
            sources += ["--overlay-src", f"{spec['lower_dir']}/lower{index}"]
        done = run(as_user + ["bwrap", "--unshare-user", "--unshare-pid", "--die-with-parent", "--ro-bind", "/usr",
                              "/usr", "--ro-bind", "/etc", "/etc", "--symlink", "usr/bin", "/bin", "--symlink",
                              "usr/lib64", "/lib64", "--symlink", "usr/lib", "/lib", "--proc", "/proc", "--dev",
                              "/dev", *sources, "--overlay", str(upper_root / "upper2"), str(upper_root / "work2"),
                              "/merged", "--", "/bin/sh", "-c",
                              "cat /merged/Dir/File.txt && echo && echo x > /merged/Dir/by-bwrap.txt && "
                              "(test -e /merged/Dir/FILE.TXT && echo other-case-found || echo other-case-not-found)"])
        entry["bubblewrap"] = {"ok": done.returncode == 0, "stdout": done.stdout.strip(),
                               "stderr": done.stderr.strip()[-300:]}
        entry["kernel_messages_bubblewrap"] = kernel_messages()
        report["cases"].append(entry)
        options.out.write_text(json.dumps(report, indent=1))
        mounted = entry.get("mount", {}).get("ok")
        print(f"{'MOUNTED' if mounted else 'REFUSED'}  {name}: bubblewrap {'ok' if entry['bubblewrap']['ok'] else 'failed'}",
              flush=True)
        for directory in (spec["lower_dir"], spec["upper_dir"]):
            subprocess.run(["rm", "-rf", directory], check=False)
    options.out.write_text(json.dumps(report, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())

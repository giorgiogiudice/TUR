#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Synthetic tests of the launcher: every state the check-list knows, the view, the layers.

    sudo synthetic.py OUT_DIR

No Microsoft software and no virtual machine. A 2 GiB sparse disk image is
laid out the way a disk looks after TUR was installed beside Windows: an EFI
system partition (FAT32, with a stand-in file under Windows' boot manager's
name), a Microsoft reserved partition, an NTFS volume made by mkfs.ntfs with
a Windows directory, an ext4 partition standing in for TUR's own, an NTFS
recovery partition, free space. It is attached as a loop device and put
through the cases below. NTFS made by mkfs.ntfs is not NTFS left by Windows:
the same checks run against volumes left by real Windows in the experiment
stage; this job proves the code paths and the refusals.

Writes OUT_DIR/synthetic.json and synthetic.md. Exits 1 when a case fails:
these are tests of TUR's own prototype, not findings about Windows.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import mmap
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True

from dualrunlib import checklist, gpt, ntfs, ovmfvars, qemu, session  # noqa: E402
from dualrunlib.blockio import Dev  # noqa: E402

LAUNCHER = str(HERE / "tur-dualrun")
MIB = 1024 * 1024
RESULTS: list[dict] = []
FACTS: dict = {}


def sh(*argv: str, check: bool = True, stdin: str | None = None) -> subprocess.CompletedProcess:
    done = subprocess.run(list(argv), capture_output=True, text=True, input=stdin)
    if check and done.returncode != 0:
        raise RuntimeError(f"{' '.join(argv)}: exit {done.returncode}: {done.stderr.strip()[:600]}")
    return done


def launcher(*argv: str) -> tuple[int, dict, str]:
    done = subprocess.run([sys.executable, LAUNCHER, *argv], capture_output=True, text=True)
    try:
        value = json.loads(done.stdout)
    except ValueError:
        value = {"unparsed": done.stdout[-400:]}
    return done.returncode, value, done.stderr


def case(name: str, expectation: str):
    """Decorator: run a case, record pass/fail and its evidence."""
    def wrap(function):
        started = time.time()
        try:
            evidence = function()
            RESULTS.append({"case": name, "expects": expectation, "result": "pass", "evidence": evidence or ""})
        except AssertionError as error:
            RESULTS.append({"case": name, "expects": expectation, "result": "FAIL", "evidence": str(error)[:900]})
        except Exception as error:  # noqa: BLE001
            RESULTS.append({"case": name, "expects": expectation, "result": "ERROR",
                            "evidence": f"{type(error).__name__}: {error}"[:900] + " | "
                            + traceback.format_exc().strip().splitlines()[-3][:200]})
        RESULTS[-1]["seconds"] = round(time.time() - started, 2)
        print(f"[{RESULTS[-1]['result']}] {name}: {RESULTS[-1]['evidence']}"[:400], flush=True)
        return function
    return wrap


def direct_write(path: str, offset: int, data: bytes) -> None:
    """Write through O_DIRECT so that an error from the device arrives at once."""
    assert offset % 4096 == 0 and len(data) % 4096 == 0
    buffer = mmap.mmap(-1, len(data))
    buffer.write(data)
    fd = os.open(path, os.O_WRONLY | os.O_DIRECT)
    try:
        os.pwritev(fd, [buffer], offset)
    finally:
        os.close(fd)
        buffer.close()


def plain_write(path: str, offset: int, data: bytes) -> None:
    fd = os.open(path, os.O_WRONLY)
    try:
        os.pwrite(fd, data, offset)
        os.fsync(fd)
    finally:
        os.close(fd)


def read(path: str, offset: int, length: int) -> bytes:
    with Dev(path) as dev:
        return dev.pread(offset, length)


def sha_range(path: str, offset: int, length: int) -> str:
    digest = hashlib.sha256()
    with Dev(path) as dev:
        position = 0
        while position < length:
            chunk = dev.pread(offset + position, min(8 * MIB, length - position))
            digest.update(chunk)
            position += len(chunk)
    return digest.hexdigest()


def flush(*nodes: str) -> None:
    sh("sync")
    for node in nodes:
        sh("blockdev", "--flushbufs", node, check=False)


@contextlib.contextmanager
def mounted(node: str, work: pathlib.Path):
    point = work / "mnt"
    point.mkdir(exist_ok=True)
    driver = None
    for candidate, options in (("ntfs-3g", "rw"), ("ntfs3", "rw")):
        if sh("mount", "-t", candidate, "-o", options, node, str(point), check=False).returncode == 0:
            driver = candidate
            break
    if driver is None:
        raise RuntimeError("no NTFS driver could mount the synthetic volume")
    FACTS["ntfs_driver_used_to_build"] = driver
    try:
        yield point
    finally:
        sh("umount", str(point))
        flush(node)


def codes(decision: dict) -> list[str]:
    return sorted(r["code"] for r in decision["refusals"])


def check(disk: str, **options) -> dict:
    facts = checklist.survey(disk)
    return checklist.decide(facts, need_kvm=False, guest_mb=256, reserve_mb=64, **options) | {"facts": facts}


def windows_part(decision: dict) -> dict:
    return next(p for p in decision["facts"]["partitions"] if p["index"] == 3)


def main() -> int:
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dualrun-syn-"))
    image = work / "disk.img"
    state = work / "state"
    state.mkdir()
    sh("truncate", "-s", "2G", str(image))
    sh("sgdisk", "-n", "1:2048:+100M", "-t", "1:ef00", "-c", "1:EFI system partition",
       "-n", "2:0:+128M", "-t", "2:0c01", "-c", "2:Microsoft reserved partition",
       "-n", "3:0:+600M", "-t", "3:0700", "-c", "3:Basic data partition",
       "-n", "4:0:+300M", "-t", "4:8300", "-c", "4:root",
       "-n", "5:0:+200M", "-t", "5:2700", "-c", "5:Recovery", str(image))
    loop = sh("losetup", "--find", "--show", "--partscan", str(image)).stdout.strip()
    sh("udevadm", "settle", check=False)
    part = {i: f"{loop}p{i}" for i in range(1, 6)}
    FACTS.update(loop=loop, dm_targets=sorted(session.targets()))
    try:
        sh("mkfs.vfat", "-F", "32", "-n", "SYSTEM", part[1])
        sh("mmd", "-i", part[1], "::EFI", "::EFI/Microsoft", "::EFI/Microsoft/Boot", "::EFI/Boot", "::EFI/fedora")
        standin = work / "standin.efi"
        standin.write_bytes(b"TUR stand-in for a boot manager; not a Microsoft file\n" * 64)
        shim = work / "shim.efi"
        shim.write_bytes(b"TUR stand-in for shim\n" * 64)
        sh("mcopy", "-i", part[1], str(standin), "::EFI/Microsoft/Boot/bootmgfw.efi")
        sh("mcopy", "-i", part[1], str(standin), "::EFI/Boot/bootx64.efi")
        sh("mcopy", "-i", part[1], str(shim), "::EFI/fedora/shimx64.efi")
        sh("mkfs.ntfs", "-Q", "-F", "-L", "Windows", part[3])
        sh("mkfs.ntfs", "-Q", "-F", "-L", "Recovery", part[5])
        sh("mkfs.ext4", "-q", "-F", "-L", "tur-root", part[4])
        with mounted(part[3], work) as point:
            (point / "Windows" / "System32").mkdir(parents=True)
            (point / "Users" / "Public" / "Documents").mkdir(parents=True)
            (point / "Windows" / "System32" / "notes.txt").write_text("synthetic\n")
        flush(loop, *part.values())
        tur_sha = sha_range(part[4], 0, 300 * MIB)
        whole_sha = sha_range(str(image), 0, 2048 * MIB)
        FACTS["tur_partition_sha256"] = tur_sha

        def hiberfil(content: bytes | None) -> None:
            with mounted(part[3], work) as point:
                target = point / "hiberfil.sys"
                if target.exists():
                    target.unlink()
                if content is not None:
                    target.write_bytes(content)

        # ---- the check-list ------------------------------------------------

        @case("clean volume", "may start; Windows' partitions 1,2,3,5 given; TUR's partition 4 hidden")
        def _():
            d = check(loop)
            assert d["ok"], codes(d)
            assert d["give"] == [1, 2, 3, 5] and d["hide"] == [4], (d["give"], d["hide"])
            state3 = windows_part(d)["content"]
            assert state3["kind"] == "ntfs" and state3["label"] == "Windows", state3
            assert state3["ntfs"]["hibernation"] == "none" and state3["ntfs"]["journal_clean"] is True
            esp = next(p for p in d["facts"]["partitions"] if p["index"] == 1)["esp"]
            assert esp["boot_manager"] and esp["other_systems"] == ["fedora"], esp
            FACTS["survey_clean"] = {"ntfs": state3["ntfs"], "esp": esp, "direct_reads": d["facts"]["disk"]["direct_reads"]}
            return f"give {d['give']}, hide {d['hide']}; label '{state3['label']}'; ESP other systems {esp['other_systems']}"

        for label, content, expected, state_name in (
            ("hiberfil.sys 'hibr' + data (1 MiB)", b"hibr" + bytes(MIB - 4), ["TUR-DUALRUN-010"], "hibernated"),
            ("hiberfil.sys 'HIBR' + data (1 MiB)", b"HIBR" + b"\x01" * (MIB - 4), ["TUR-DUALRUN-010"], "hibernated"),
            ("hiberfil.sys 'hibr', 4 bytes (resident)", b"hibr", ["TUR-DUALRUN-010"], "hibernated"),
            ("hiberfil.sys 'wake' (already resumed)", b"wake" + bytes(MIB - 4), [], "used"),
            ("hiberfil.sys all zeros", bytes(MIB), [], "empty"),
            ("hiberfil.sys of length 0", b"", [], "empty"),
            ("hiberfil.sys with unknown content", b"\x13\x37\xbe\xef" * 1024, ["TUR-DUALRUN-013"], "unknown"),
        ):
            @case(label, f"hibernation '{state_name}'; refusals {expected or 'none'}")
            def _(content=content, expected=expected, state_name=state_name):
                hiberfil(content)
                d = check(loop)
                got = windows_part(d)["content"]["ntfs"]
                assert got["hibernation"] == state_name, got
                assert codes(d) == expected, codes(d)
                if expected:
                    text = checklist.render(d["refusals"][0])
                    assert "force" not in text.lower()
                return f"state {got['hibernation']}; refusals {codes(d) or 'none'}"

        @case("protected session of a hibernated volume", "still refused unless the experiment switch is given")
        def _():
            hiberfil(b"hibr" + bytes(MIB - 4))
            assert codes(check(loop, mode="protected")) == ["TUR-DUALRUN-010"]
            assert codes(check(loop, mode="protected", experiment_allow_hibernated=True)) == []
            assert codes(check(loop, mode="direct", experiment_allow_hibernated=True)) == ["TUR-DUALRUN-010"]
            hiberfil(None)
            assert check(loop)["ok"]
            return "refused in both modes; the experiment switch works in protected mode only"

        @case("volume marked dirty", "may start (Windows runs its own check), with a note")
        def _():
            with Dev(part[3]) as dev:
                fs = ntfs.Ntfs(dev)
                _, attrs = fs.record(3)
                info = next(a for a in attrs if a.type == 0x70)
                where = fs.mft_offset + 3 * fs.record_size + info.value_offset + 10
            old = read(part[3], where - where % 4096, 4096)
            patched = bytearray(old)
            patched[where % 4096] |= 1
            plain_write(part[3], where - where % 4096, bytes(patched))
            try:
                d = check(loop)
                got = windows_part(d)["content"]["ntfs"]
                assert got["dirty"] is True, got
                assert d["ok"] and any("disk check" in n for n in d["notes"]), (codes(d), d["notes"])
            finally:
                plain_write(part[3], where - where % 4096, old)
            return "dirty flag read; not a refusal; note shown"

        def journal_page(lsn: int, in_use: int, flags: int) -> bytes:
            page = bytearray(4096)
            page[0:4] = b"RSTR"
            struct.pack_into("<HHQIIHhh", page, 4, 30, 9, 0, 4096, 4096, 48, 1, 1)
            struct.pack_into("<QHHHH", page, 48, lsn, 1, 0xFFFF, in_use, flags)
            struct.pack_into("<H", page, 30, 0x0007)
            for i in range(1, 9):
                page[30 + 2 * i:32 + 2 * i] = page[i * 512 - 2:i * 512]
                struct.pack_into("<H", page, i * 512 - 2, 0x0007)
            return bytes(page)

        @case("journal says the volume was not unmounted", "may start, with a note; clean flag read correctly both ways")
        def _():
            with Dev(part[3]) as dev:
                fs = ntfs.Ntfs(dev)
                _, attrs = fs.record(2)
                data = next(a for a in attrs if a.type == 0x80 and not a.name)
                where = data.runs[0][1] * fs.cluster
            old = read(part[3], where, 8192)
            try:
                plain_write(part[3], where, journal_page(2000, 0, 0) + journal_page(1000, 0xFFFF, 2))
                d = check(loop)
                got = windows_part(d)["content"]["ntfs"]
                assert got["journal_clean"] is False, got["journal"]
                assert d["ok"] and any("not shut down properly" in n for n in d["notes"]), d["notes"]
                plain_write(part[3], where, journal_page(2000, 0, 2) + journal_page(1000, 0, 0))
                assert windows_part(check(loop))["content"]["ntfs"]["journal_clean"] is True
                torn = bytearray(journal_page(3000, 0, 0))
                torn[1022:1024] = b"\xde\xad"
                plain_write(part[3], where, bytes(torn) + journal_page(1000, 0, 2))
                assert windows_part(check(loop))["content"]["ntfs"]["journal_clean"] is True
            finally:
                plain_write(part[3], where, old)
            return "unclean, clean and torn restart pages read as expected"

        @case("root directory unreadable", "refused: the hibernation state is unknown")
        def _():
            with Dev(part[3]) as dev:
                fs = ntfs.Ntfs(dev)
                where = fs.mft_offset + 5 * fs.record_size
            old = read(part[3], where - where % 4096, 4096)
            patched = bytearray(old)
            patched[where % 4096:where % 4096 + 4] = b"BAAD"
            plain_write(part[3], where - where % 4096, bytes(patched))
            try:
                d = check(loop)
                assert codes(d) == ["TUR-DUALRUN-013"], codes(d)
            finally:
                plain_write(part[3], where - where % 4096, old)
            return "TUR-DUALRUN-013"

        @case("BitLocker signature", "refused without the key; with the key still refused while the boot "
              "configuration cannot be read")
        def _():
            old = read(part[3], 0, 4096)
            patched = bytearray(old)
            patched[3:11] = b"-FVE-FS-"
            plain_write(part[3], 0, bytes(patched))
            try:
                d = check(loop)
                assert codes(d) == ["TUR-DUALRUN-013", "TUR-DUALRUN-030"], codes(d)
                d2 = check(loop, bitlocker_key_available=True)
                assert codes(d2) == ["TUR-DUALRUN-013"], codes(d2)
            finally:
                plain_write(part[3], 0, old)
            return "030 and 013; with the key: 013 (no readable boot configuration on the synthetic disk)"

        @case("Windows partition mounted by TUR", "refused, naming the mount point")
        def _():
            with mounted(part[3], work) as point:
                d = check(loop)
                assert "TUR-DUALRUN-020" in codes(d), codes(d)
                evidence = next(r["evidence"] for r in d["refusals"] if r["code"] == "TUR-DUALRUN-020")
                assert str(point) in evidence or "in use" in evidence, evidence
                code, value, _ = launcher("attach", loop, "--state", str(state), "--no-kvm",
                                          "--guest-mb", "256", "--reserve-mb", "64")
                assert code == 3 and not session.view_exists(session.view_name(d["facts"]["disk"]["guid"])), value
            assert check(loop)["ok"]
            return evidence

        @case("no Windows boot manager on the EFI system partition", "refused")
        def _():
            sh("mmove", "-i", part[1], "::EFI/Microsoft/Boot/bootmgfw.efi", "::EFI/Microsoft/Boot/gone.efi")
            flush(part[1])
            try:
                assert codes(check(loop)) == ["TUR-DUALRUN-002"]
            finally:
                sh("mmove", "-i", part[1], "::EFI/Microsoft/Boot/gone.efi", "::EFI/Microsoft/Boot/bootmgfw.efi")
                flush(part[1])
            return "TUR-DUALRUN-002"

        @case("a Linux filesystem in a partition typed as Windows data", "hidden from the guest, with a note")
        def _():
            sh("sgdisk", "-t", "4:0700", loop)
            sh("partprobe", loop, check=False)
            sh("udevadm", "settle", check=False)
            try:
                d = check(loop)
                assert d["ok"] and 4 in d["hide"] and 4 not in d["give"], (codes(d), d["give"], d["hide"])
                assert any("partition 4" in n for n in d["notes"]), d["notes"]
            finally:
                sh("sgdisk", "-t", "4:8300", loop)
                sh("partprobe", loop, check=False)
                sh("udevadm", "settle", check=False)
            return "; ".join(d["notes"])

        @case("not enough free memory", "refused")
        def _():
            facts = checklist.survey(loop)
            d = checklist.decide(facts, need_kvm=False, guest_mb=64 * 1024 * 1024)
            assert codes(d) == ["TUR-DUALRUN-040"], codes(d)
            return d["refusals"][0]["evidence"]

        @case("refusal text", "every refusal renders with a reason and an action and never suggests forcing")
        def _():
            for code in checklist.REFUSALS:
                text = checklist.render(checklist.refusal(code, "evidence"))
                assert "Reason:" in text and "Action:" in text and "force" not in text.lower(), code
            return f"{len(checklist.REFUSALS)} refusal texts"

        # ---- the view ------------------------------------------------------

        table = None
        with Dev(loop) as dev:
            table = gpt.read_gpt(dev)
        offsets = {p.index: p.first_lba * 512 for p in table.partitions}
        name = session.view_name(table.disk_guid)
        view = f"/dev/mapper/{name}"

        @case("attach (direct)", "one device of the disk's size; Windows' partitions mapped; TUR's reads as zeros")
        def _():
            code, record, err = launcher("attach", loop, "--state", str(state), "--no-kvm",
                                         "--guest-mb", "256", "--reserve-mb", "64")
            assert code == 0, (record, err[-300:])
            FACTS["hidden_as"] = record["hidden_as"]
            size = int(sh("blockdev", "--getsize64", view).stdout)
            assert size == 2048 * MIB, size
            with Dev(view) as dev:
                seen = gpt.read_gpt(dev)
            assert seen.disk_guid == table.disk_guid and len(seen.partitions) == 5
            assert sha_range(view, offsets[3], 8 * MIB) == sha_range(part[3], 0, 8 * MIB)
            assert sha_range(view, offsets[1], 8 * MIB) == sha_range(part[1], 0, 8 * MIB)
            hidden = read(view, offsets[4], MIB) if record["hidden_as"] != "error" else None
            real = read(part[4], 0, MIB)
            assert real.strip(b"\0"), "the stand-in partition should hold data"
            if hidden is not None:
                assert not hidden.strip(b"\0"), "TUR's partition is readable through the view"
            return f"size {size}; hidden ranges as '{record['hidden_as']}'; TUR's partition reads as zeros"

        @case("write to TUR's partition through the view", "fails or is discarded; the real partition is unchanged")
        def _():
            outcome = "accepted and discarded"
            try:
                direct_write(view, offsets[4], b"\xee" * 4096)
            except OSError as error:
                outcome = f"refused ({error.strerror})"
            flush(view)
            assert sha_range(part[4], 0, 300 * MIB) == tur_sha, "TUR's partition changed"
            FACTS["write_to_hidden"] = outcome
            return outcome

        @case("the kernel's lock", "a mapped partition cannot be mounted while the view exists; no second view")
        def _():
            point = work / "mnt2"
            point.mkdir(exist_ok=True)
            done = sh("mount", "-t", "vfat", "-o", "ro", part[1], str(point), check=False)
            if done.returncode == 0:
                sh("umount", str(point))
                raise AssertionError("the EFI system partition could be mounted while the guest has it")
            in_use = checklist.survey(loop)["partitions"][0]["in_use"]
            assert in_use["exclusive_open"] == "busy" and in_use["holders"], in_use
            code, value, _ = launcher("attach", loop, "--state", str(state), "--no-kvm",
                                      "--guest-mb", "256", "--reserve-mb", "64")
            assert code == 3 and "TUR-DUALRUN-020" in json.dumps(value) or "TUR-DUALRUN-050" in json.dumps(value), value
            return f"mount refused: {done.stderr.strip()[:80]}; holders {in_use['holders']}; second attach refused"

        @case("writes land where they should", "a write through the view into a Windows partition is on that partition")
        def _():
            marker = (b"TUR-DUALRUN-MARKER" + bytes(4096))[:4096]
            direct_write(view, offsets[2] + 64 * MIB, marker)
            assert read(part[2], 64 * MIB, 4096) == marker
            return "marker written at +64 MiB of the reserved partition and read back from the real partition"

        @case("the partition table is a private copy", "a write to it does not reach the disk and is reported at detach")
        def _():
            before = sha_range(loop, 0, MIB)
            direct_write(view, 40 * 512 - (40 * 512) % 4096, b"\x5a" * 4096)
            flush(view)
            assert sha_range(loop, 0, MIB) == before, "the real first megabyte changed"
            code, result, err = launcher("detach", loop, "--state", str(state))
            assert code == 0, (result, err[-300:])
            assert result["partition_table"]["guest_changed_its_copy"] is True, result["partition_table"]
            assert not session.view_exists(name)
            assert sha_range(loop, 0, MIB) == before
            assert sha_range(part[4], 0, 300 * MIB) == tur_sha
            return f"reported: sectors {result['partition_table'].get('differing_sectors_of_the_copy')}"

        @case("after detach", "the partitions can be mounted again; the base token is stable when nothing changed")
        def _():
            first = checklist.survey(loop)["base_token"]
            code, record, err = launcher("attach", loop, "--state", str(state), "--no-kvm",
                                         "--guest-mb", "256", "--reserve-mb", "64")
            assert code == 0, err[-300:]
            code, result, err = launcher("detach", loop, "--state", str(state))
            assert code == 0 and result["base_token_before"] == result["base_token_after"] == first, result
            assert result["partition_table"]["guest_changed_its_copy"] is False
            with mounted(part[3], work) as point:
                assert (point / "Windows" / "System32" / "notes.txt").read_text() == "synthetic\n"
            return "token unchanged over an idle session; volume mounts and reads"

        # ---- layers --------------------------------------------------------

        overlay = str(work / "layer.qcow2")
        target = offsets[2] + 8 * MIB          # inside the reserved partition: no filesystem to upset
        length = 96 * MIB
        base_before = sha_range(str(image), 0, 2048 * MIB)

        def protected() -> dict:
            code, record, err = launcher("attach", loop, "--state", str(state), "--mode", "protected",
                                         "--overlay", overlay, "--no-kvm", "--guest-mb", "256", "--reserve-mb", "64")
            assert code == 0, (record, err[-300:])
            return record

        def fill_layer() -> None:
            sh("qemu-io", "-f", "qcow2", "-c", f"write -P 0xab {target} {length}", overlay)

        @case("protected session, thrown away", "the view is read-only; after discard every byte of the disk is as before")
        def _():
            protected()
            try:
                direct_write(view, offsets[2] + 4096, b"\x11" * 4096)
                raise AssertionError("a read-only view accepted a write")
            except OSError as error:
                refused = error.strerror
            fill_layer()
            seen = sh("qemu-io", "-f", "qcow2", "-c", f"read -P 0xab {target} {MIB}", overlay).stdout
            assert "pattern verification failed" not in seen.lower(), seen
            code, value, _ = launcher("attach", loop, "--state", str(state), "--mode", "protected",
                                      "--overlay", overlay + ".2", "--no-kvm", "--guest-mb", "256", "--reserve-mb", "64")
            assert code == 3, value
            code, result, err = launcher("discard", loop, "--state", str(state))
            assert code == 0, (result, err[-300:])
            assert sha_range(str(image), 0, 2048 * MIB) == base_before, "the disk changed"
            assert not os.path.exists(overlay)
            return f"write to the view refused ({refused}); layer held 96 MiB; disk identical after discard"

        @case("protected session, kept; the commit interrupted and run again", "after the second run the disk "
              "holds the layer; the undo record puts it back; a changed disk refuses the undo")
        def _():
            protected()
            fill_layer()
            environment = dict(os.environ, TUR_DUALRUN_TEST_KILL_COMMIT_AFTER="1.0")
            first = subprocess.run([sys.executable, LAUNCHER, "commit", loop, "--state", str(state),
                                    "--rate", str(24 * MIB)], capture_output=True, text=True, env=environment)
            interrupted = json.loads(first.stdout)
            flush(loop, part[2])
            assert interrupted.get("interrupted_for_test"), interrupted
            partial = read(part[2], 8 * MIB, length)
            done_bytes = partial.count(0xAB)
            layer = session.read_json(session.layer_path(str(state), name))
            assert layer and layer["status"] == "committing", layer
            code, value, _ = launcher("attach", loop, "--state", str(state), "--no-kvm",
                                      "--guest-mb", "256", "--reserve-mb", "64", "--mode", "protected",
                                      "--overlay", overlay + ".3")
            assert code == 3, "a new protected session was allowed while a commit is unfinished"
            code, result, err = launcher("commit", loop, "--state", str(state))
            assert code == 0 and result.get("committed") and result.get("resumed"), (result, err[-300:])
            flush(loop, part[2])
            assert read(part[2], 8 * MIB, length) == b"\xab" * length, "the layer is not fully on the disk"
            assert sha_range(part[4], 0, 300 * MIB) == tur_sha
            code, undone, err = launcher("undo", loop, "--state", str(state))
            assert code == 0 and undone.get("undone"), (undone, err[-300:])
            flush(loop, part[2])
            assert sha_range(str(image), 0, 2048 * MIB) == base_before, "undo did not restore the disk"
            # A second kept session; then the disk changes underneath; undo must refuse.
            protected()
            fill_layer()
            code, result, err = launcher("commit", loop, "--state", str(state))
            assert code == 0 and result.get("committed"), (result, err[-300:])
            old = read(part[2], 0, 4096)
            plain_write(part[2], 0, b"\x77" * 4096)
            flush(loop, part[2])
            code, refused, _ = launcher("undo", loop, "--state", str(state))
            assert code == 3 and "TUR-DUALRUN-072" in json.dumps(refused), refused
            plain_write(part[2], 0, old)
            flush(loop, part[2])
            code, undone, err = launcher("undo", loop, "--state", str(state))
            assert code == 0, (undone, err[-300:])
            flush(loop, part[2])
            assert sha_range(str(image), 0, 2048 * MIB) == base_before
            return (f"killed after {interrupted.get('seconds_before_kill')} s with {done_bytes // MIB} of 96 MiB on "
                    f"the disk; second run completed; undo restored the disk; undo refused (072) after the disk changed")

        # ---- firmware variables, boot files, identity ---------------------

        @case("guest firmware variables", "a boot entry for Windows' boot manager on the EFI system partition, first in BootOrder")
        def _():
            template = qemu.firmware()["vars_blank"]
            data = bytearray(pathlib.Path(template).read_bytes())
            before = len(data)
            esp = table.partitions[0]
            first = ovmfvars.add_windows_entry(data, esp.index, esp.part_guid, esp.first_lba, esp.sectors)
            second = ovmfvars.add_windows_entry(data, esp.index, esp.part_guid, esp.first_lba, esp.sectors)
            entries = ovmfvars.boot_entries(bytes(data))
            assert len(data) == before and first != second
            assert entries["boot_order"][:2] == [second, first], entries["boot_order"]
            target_path = entries["entries"][first]["path"]
            assert esp.part_guid in target_path and "bootmgfw.efi" in target_path, target_path
            FACTS["firmware_template"] = template
            FACTS["variables_in_template"] = len(ovmfvars.variables(pathlib.Path(template).read_bytes()))
            return f"Boot{first}: {target_path}"

        @case("copy of the person's boot files", "every file under EFI with its checksum")
        def _():
            dest = work / "esp-backup"
            code, value, err = launcher("esp-backup", loop, "--dest", str(dest))
            assert code == 0, err[-300:]
            manifest = json.loads((dest / "manifest.json").read_text())["files"]
            assert len(manifest) == 3, manifest
            copy = (dest / "files" / "EFI" / "Microsoft" / "Boot" / "bootmgfw.efi").read_bytes()
            assert copy == standin.read_bytes()
            return ", ".join(sorted(manifest))

        @case("this machine's identity strings", "readable (which ones; values are not recorded)")
        def _():
            identity = qemu.host_identity()
            FACTS["identity_fields_readable"] = sorted(identity)
            assert "manufacturer" in identity or "product" in identity, identity.keys()
            arguments = qemu.build({"code": "c", "vars": "v", "disk": {"path": "d"}, "identity": identity})
            assert "-smbios" in arguments
            return ", ".join(sorted(identity))

        @case("everything left as found", "TUR's stand-in partition and the whole disk are byte-identical to the start")
        def _():
            hiberfil(None)
            flush(loop, *part.values())
            assert sha_range(part[4], 0, 300 * MIB) == tur_sha
            assert not session.view_exists(name)
            return "TUR's partition unchanged; no view left"

    finally:
        subprocess.run(["dmsetup", "remove_all"], check=False, capture_output=True)
        subprocess.run(["umount", str(work / "mnt")], check=False, capture_output=True)
        subprocess.run(["losetup", "-D"], check=False, capture_output=True)

    failed = [r for r in RESULTS if r["result"] != "pass"]
    (out / "synthetic.json").write_text(json.dumps({"facts": FACTS, "results": RESULTS}, indent=2, default=str) + "\n")
    lines = ["## dualrun: synthetic tests (no Microsoft software, no virtual machine)", "",
             f"{len(RESULTS) - len(failed)} of {len(RESULTS)} cases pass. Device-mapper targets on this runner: "
             f"{', '.join(FACTS.get('dm_targets', []))}. Hidden ranges were mapped as `{FACTS.get('hidden_as')}`; "
             f"a write to one was {FACTS.get('write_to_hidden')}. NTFS volumes were built with "
             f"`{FACTS.get('ntfs_driver_used_to_build')}`.", "",
             "| Case | Expected | Result | Evidence |", "|---|---|---|---|"]
    for r in RESULTS:
        evidence = str(r["evidence"]).replace("|", "\\|").replace("\n", " ")[:500]
        lines.append(f"| {r['case']} | {r['expects']} | {r['result']} | {evidence} |")
    (out / "synthetic.md").write_text("\n".join(lines) + "\n")
    print(f"{len(RESULTS) - len(failed)} of {len(RESULTS)} cases pass")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

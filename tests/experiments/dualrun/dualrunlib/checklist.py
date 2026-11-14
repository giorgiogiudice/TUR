# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Look at a disk that holds Windows, and decide whether its Windows may be started.

`survey()` only reads. `decide()` turns what was read into: the partitions
the guest gets, the ones it must never see, and refusals. A refusal has a
code, a sentence that says what is wrong in plain words, what the person can
do, and the evidence. There is no "force" switch: nothing here can turn a
refusal into a start.

The rule for unknowns: a fact that could not be read is not "fine". If the
hibernation state of the Windows volume cannot be established, that is a
refusal of its own.
"""

from __future__ import annotations

import errno
import hashlib
import os
import pathlib
import stat

from . import bcd, fat, gpt, ntfs
from .blockio import Dev

GIVEN_KINDS = {"ntfs", "bitlocker", "fat", "exfat", "refs"}

REFUSALS = {
    "TUR-DUALRUN-001": (
        "No Windows was found on this disk.",
        "The disk has no partition that holds a Windows system TUR can start.",
        "Choose the disk Windows is installed on."),
    "TUR-DUALRUN-002": (
        "Windows' start-up files were not found.",
        "The disk must have exactly one EFI system partition and Windows' boot manager on it. "
        "Windows installed the old BIOS way cannot be started inside TUR.",
        "Nothing was changed. If Windows starts by itself on this computer, report this with the details below."),
    "TUR-DUALRUN-003": (
        "The disk's partition table cannot be used as it is.",
        "TUR reads the table to give Windows its own partitions and nothing else, and could not read it safely.",
        "Start Windows by itself and let it check the disk, then try again."),
    "TUR-DUALRUN-010": (
        "Windows is asleep on its disk, not shut down.",
        "Windows was hibernated, or was shut down with Fast Startup, which saves part of its memory to the disk. "
        "Starting it inside TUR now would throw that away or wake it on a computer it does not recognise, "
        "and could damage files.",
        "Start Windows by itself, then choose Restart (not Shut down), and come back to TUR. "
        "To stop this happening again, turn off Fast Startup in Windows' power options."),
    "TUR-DUALRUN-011": (
        "Windows is set to wake from hibernation at its next start.",
        "Windows' start-up settings say it will resume a saved session. Starting it inside TUR now would lose "
        "that session and could damage files.",
        "Start Windows by itself, then choose Restart (not Shut down), and come back to TUR."),
    "TUR-DUALRUN-013": (
        "TUR could not tell whether Windows was shut down completely.",
        "The check that Windows is not hibernated could not be finished, so TUR treats the disk as unsafe to start.",
        "Start Windows by itself, choose Restart, and come back to TUR. If this keeps happening, report it "
        "with the details below."),
    "TUR-DUALRUN-020": (
        "A Windows partition is in use by TUR.",
        "A partition Windows needs is open on TUR's side. Two systems must never write to the same partition "
        "at once.",
        "Close the files and windows that use it, eject it in the file manager, and try again."),
    "TUR-DUALRUN-030": (
        "This Windows is locked with BitLocker.",
        "The key that unlocks it is sealed inside this computer's security chip and is released only when "
        "Windows starts by itself. Started inside TUR, Windows will ask for its 48-digit recovery key "
        "every time. TUR does not have that key and does not try to get around the lock.",
        "Find the recovery key first (for a Microsoft account: account.microsoft.com/devices/recoverykey), "
        "then start again and say that you have it. Or use Restart into Windows."),
    "TUR-DUALRUN-040": (
        "There is not enough free memory to start Windows beside what is running.",
        "Windows needs memory of its own while it runs inside TUR.",
        "Close some applications and try again, or use Restart into Windows."),
    "TUR-DUALRUN-041": (
        "This computer cannot run a virtual machine right now.",
        "Hardware virtualisation is switched off in the computer's firmware settings, or is not available.",
        "Switch on virtualisation (Intel VT-x or AMD-V) in the firmware settings, or use Restart into Windows."),
    "TUR-DUALRUN-050": (
        "Windows is already running inside TUR, or its last session was not closed properly.",
        "TUR never starts the same Windows twice.",
        "Switch to the running Windows, or let TUR finish closing the last session."),
    "TUR-DUALRUN-070": (
        "Changes from the last protected session are still waiting.",
        "Windows was last used in a protected session whose changes were neither kept nor thrown away.",
        "Choose Keep or Throw away for that session first."),
    "TUR-DUALRUN-072": (
        "Windows has changed since this was saved.",
        "Windows was started by itself (or its disk was changed) after this session or restore point was made. "
        "Applying it now would mix two different states and damage Windows.",
        "The saved session or restore point can only be thrown away."),
}


def refusal(code: str, evidence: str, partition: int | None = None) -> dict:
    title, say, do = REFUSALS[code]
    return {"code": code, "title": title, "say": say, "do": do, "evidence": evidence, "partition": partition}


def render(item: dict) -> str:
    """The refusal as the person would see it (the form of docs/architecture/storage.md)."""
    return (f"{item['code']}\n{item['title']}\n\nReason:\n{item['say']}\n\nAction:\n{item['do']}\n\n"
            f"Technical detail:\n{item['evidence']}\n")


# -- reading ----------------------------------------------------------------

def _linux_content(dev: Dev) -> str | None:
    """A Linux filesystem or container inside a partition, whatever its type GUID says."""
    try:
        if dev.size >= 2048 and dev.pread(1024 + 56, 2) == b"\x53\xef":
            return "ext"
        head = dev.pread(0, 16) if dev.size >= 16 else b""
        if head[:4] == b"XFSB":
            return "xfs"
        if head[:6] == b"LUKS\xba\xbe":
            return "luks"
        if dev.size >= 65536 + 72 and dev.pread(65536 + 64, 8) == b"_BHRfS_M":
            return "btrfs"
        if dev.size >= 4096 and dev.pread(4086, 10) in (b"SWAPSPACE2", b"SWAP-SPACE"):
            return "swap"
    except (OSError, ValueError):
        return None
    return None


def partition_node(disk_path: str, index: int) -> str | None:
    """The kernel's device node of partition `index` of a block device, if there is one."""
    real = os.path.realpath(disk_path)
    try:
        if not stat.S_ISBLK(os.stat(real).st_mode):
            return None
    except OSError:
        return None
    name = os.path.basename(real)
    for candidate in (f"{name}p{index}", f"{name}{index}"):
        number = pathlib.Path(f"/sys/class/block/{candidate}/partition")
        if number.exists() and number.read_text().strip() == str(index) \
                and pathlib.Path(f"/sys/class/block/{name}/{candidate}").exists():
            return f"/dev/{candidate}"
    return None


def _in_use(node: str | None) -> dict:
    """Mounts, swap, holders and the kernel's own answer to an exclusive open."""
    out: dict = {"mounted_at": [], "swap": False, "holders": [], "exclusive_open": None}
    if node is None:
        return out
    try:
        info = os.stat(node)
    except OSError as error:
        out["error"] = str(error)
        return out
    device = f"{os.major(info.st_rdev)}:{os.minor(info.st_rdev)}"
    try:
        for line in pathlib.Path("/proc/self/mountinfo").read_text().splitlines():
            fields = line.split()
            if len(fields) > 4 and fields[2] == device:
                out["mounted_at"].append(fields[4].replace("\\040", " "))
        for line in pathlib.Path("/proc/swaps").read_text().splitlines()[1:]:
            if line.split() and os.path.realpath(line.split()[0]) == os.path.realpath(node):
                out["swap"] = True
    except OSError:
        pass
    holders = pathlib.Path(f"/sys/class/block/{os.path.basename(os.path.realpath(node))}/holders")
    if holders.is_dir():
        out["holders"] = sorted(p.name for p in holders.iterdir())
    try:
        fd = os.open(node, os.O_RDONLY | os.O_EXCL | os.O_CLOEXEC)
        os.close(fd)
        out["exclusive_open"] = "free"
    except OSError as error:
        out["exclusive_open"] = "busy" if error.errno == errno.EBUSY else f"not tested ({error.strerror})"
    return out


def _esp(dev: Dev) -> dict:
    out: dict = {"boot_manager": None, "bcd": None}
    try:
        volume = fat.Fat(dev)
        out["label"] = volume.label
        out["boot_manager"] = volume.exists("EFI/Microsoft/Boot/bootmgfw.efi")
        out["other_systems"] = sorted(
            {path.split("/")[1] for path, _ in volume.walk("EFI")
             if path.count("/") >= 2 and path.split("/")[1].lower() not in ("microsoft", "boot")})
        if out["boot_manager"] and volume.exists("EFI/Boot/bootx64.efi"):
            fallback = hashlib.sha256(volume.read("EFI/Boot/bootx64.efi")).hexdigest()
            manager = hashlib.sha256(volume.read("EFI/Microsoft/Boot/bootmgfw.efi")).hexdigest()
            out["fallback_loader_is_windows"] = fallback == manager
        if volume.exists("EFI/Microsoft/Boot/BCD"):
            raw = volume.read("EFI/Microsoft/Boot/BCD")
            out["bcd_sha256"] = hashlib.sha256(raw).hexdigest()
            try:
                out["bcd"] = bcd.summary(bcd.read_store(raw))
            except (bcd.BcdError, IndexError, ValueError, Exception) as error:  # noqa: BLE001
                out["bcd_error"] = str(error)
    except (fat.FatError, OSError, ValueError, IndexError) as error:
        out["error"] = str(error)
    return out


def host_facts() -> dict:
    memory = {}
    try:
        for line in pathlib.Path("/proc/meminfo").read_text().splitlines():
            key, _, value = line.partition(":")
            if key in ("MemTotal", "MemAvailable"):
                memory[key] = int(value.split()[0]) // 1024
    except OSError:
        pass
    return {"memory_total_mb": memory.get("MemTotal"), "memory_available_mb": memory.get("MemAvailable"),
            "kvm": os.access("/dev/kvm", os.R_OK | os.W_OK)}


def survey(disk_path: str) -> dict:
    """Everything the check-list needs, read from the disk without changing it."""
    out: dict = {"disk": {"path": disk_path}, "partitions": [], "host": host_facts()}
    try:
        disk = Dev(disk_path)
    except OSError as error:
        out["disk"]["error"] = f"cannot open: {error}"
        return out
    with disk:
        out["disk"].update(bytes=disk.size, sector=disk.sector, block_device=disk.is_block, direct_reads=disk.direct)
        try:
            table = gpt.read_gpt(disk)
        except (gpt.GptError, OSError, ValueError) as error:
            out["disk"]["gpt_error"] = str(error)
            return out
        out["disk"].update(guid=table.disk_guid, sectors=table.total_sectors, head_sectors=table.head_sectors,
                           tail_start=table.tail_start)
        token = hashlib.sha256()
        token.update(disk.pread(0, table.head_sectors * table.sector))
        token.update(disk.pread(table.tail_start * table.sector, (table.total_sectors - table.tail_start) * table.sector))
        for part in sorted(table.partitions, key=lambda p: p.first_lba):
            node = partition_node(disk_path, part.index)
            view = disk.sub(part.first_lba * table.sector, part.sectors * table.sector)
            entry: dict = {
                "index": part.index, "node": node, "type_guid": part.type_guid, "type": part.type_name,
                "role": part.role, "part_guid": part.part_guid, "first_lba": part.first_lba,
                "sectors": part.sectors, "attributes": f"{part.attributes:#x}", "name": part.name,
            }
            if part.role in ("windows-data", "windows-recovery", "esp"):
                entry["content"] = ntfs.probe(view)
                linux = _linux_content(view)
                if linux:
                    entry["content"]["kind"] = f"linux:{linux}"
            if part.role == "esp":
                entry["esp"] = _esp(view)
            entry["in_use"] = _in_use(node)
            if part.role != "other":
                try:
                    token.update(view.pread(0, min(view.size, 8192)))
                    content = entry.get("content", {})
                    if content.get("kind") == "ntfs":
                        journal = content["ntfs"].get("journal") or {}
                        token.update(repr([(p.get("current_lsn"), p.get("flags"), p.get("client_in_use"))
                                           for p in journal.get("pages", [])]).encode())
                        token.update(repr(content["ntfs"].get("hiberfil")).encode())
                    if part.role == "esp":
                        token.update(str(entry["esp"].get("bcd_sha256")).encode())
                except (OSError, ValueError):
                    token.update(b"unreadable")
            out["partitions"].append(entry)
        out["base_token"] = token.hexdigest()
    return out


# -- deciding ---------------------------------------------------------------

def decide(facts: dict, *, mode: str = "direct", guest_mb: int = 4096, reserve_mb: int = 2048,
           bitlocker_key_available: bool = False, need_kvm: bool = True,
           experiment_allow_hibernated: bool = False) -> dict:
    """Refusals, notes, and which partitions the guest gets."""
    refusals: list[dict] = []
    notes: list[str] = []
    give: list[int] = []
    hide: list[int] = []
    disk = facts["disk"]
    if "error" in disk or "gpt_error" in disk:
        refusals.append(refusal("TUR-DUALRUN-003", disk.get("error") or disk["gpt_error"]))
        return {"ok": False, "refusals": refusals, "notes": notes, "give": give, "hide": hide}

    esps = [p for p in facts["partitions"] if p["role"] == "esp"]
    if len(esps) != 1:
        refusals.append(refusal("TUR-DUALRUN-002", f"EFI system partitions on the disk: {len(esps)}"))
    elif not esps[0].get("esp", {}).get("boot_manager"):
        refusals.append(refusal("TUR-DUALRUN-002",
                                f"partition {esps[0]['index']}: EFI/Microsoft/Boot/bootmgfw.efi not found "
                                f"({esps[0].get('esp', {}).get('error', 'no such file')})", esps[0]["index"]))
    store = esps[0].get("esp", {}).get("bcd") if len(esps) == 1 else None
    resume_pending = store.get("resume_pending") if store else None

    system_found = False
    for part in facts["partitions"]:
        kind = part.get("content", {}).get("kind", "")
        if part["role"] == "other":
            hide.append(part["index"])
            continue
        if part["role"] == "msr":
            give.append(part["index"])
            continue
        if kind not in GIVEN_KINDS:
            hide.append(part["index"])
            notes.append(f"partition {part['index']} has a Windows type but holds {kind or 'nothing readable'}; "
                         "it is not given to Windows")
            continue
        give.append(part["index"])
        if part["role"] != "windows-data":
            continue
        where = f"partition {part['index']} ({part.get('content', {}).get('label') or 'no label'})"
        if kind == "bitlocker":
            system_found = True
            if not bitlocker_key_available:
                refusals.append(refusal("TUR-DUALRUN-030", f"{where}: BitLocker signature -FVE-FS-", part["index"]))
            if resume_pending is None and not experiment_allow_hibernated:
                refusals.append(refusal("TUR-DUALRUN-013",
                                        f"{where} is encrypted and the boot configuration could not be read",
                                        part["index"]))
        elif kind == "ntfs":
            state = part["content"]["ntfs"]
            if state.get("has_windows_directory") is not False:
                system_found = True       # found, or the root could not be read (refused below)
            hibernation = state.get("hibernation")
            if hibernation == "hibernated":
                if not (experiment_allow_hibernated and mode == "protected"):
                    detail = state.get("hiberfil", {})
                    refusals.append(refusal(
                        "TUR-DUALRUN-010",
                        f"{where}: hiberfil.sys begins with '{detail.get('magic_text')}' "
                        f"({detail.get('size')} bytes); journal clean: {state.get('journal_clean')}", part["index"]))
            elif hibernation not in ("none", "empty", "used"):
                refusals.append(refusal(
                    "TUR-DUALRUN-013",
                    f"{where}: hibernation state '{hibernation}'; errors: {state.get('errors') or state.get('error')}",
                    part["index"]))
            if state.get("journal_clean") is False and hibernation in ("none", "empty", "used"):
                notes.append(f"{where} was not shut down properly; Windows will check it when it starts, "
                             "as it would by itself")
            if state.get("dirty"):
                notes.append(f"{where} is marked for a disk check; Windows will run it when it starts")
    if not system_found and not any(r["code"] == "TUR-DUALRUN-002" for r in refusals):
        refusals.append(refusal("TUR-DUALRUN-001", "no NTFS volume with a Windows directory, and no BitLocker volume"))
    if resume_pending and not (experiment_allow_hibernated and mode == "protected"):
        refusals.append(refusal("TUR-DUALRUN-011", "boot configuration: boot manager element 26000005 (resume) is set"))

    for part in facts["partitions"]:
        if part["index"] not in give:
            continue
        use = part.get("in_use", {})
        busy = []
        if use.get("mounted_at"):
            busy.append("mounted at " + ", ".join(use["mounted_at"]))
        if use.get("swap"):
            busy.append("used as swap")
        if use.get("holders"):
            busy.append("held by " + ", ".join(use["holders"]))
        if use.get("exclusive_open") == "busy" and not busy:
            busy.append("the kernel reports it as in use")
        if busy:
            refusals.append(refusal("TUR-DUALRUN-020", f"partition {part['index']} ({part['node']}): "
                                    + "; ".join(busy), part["index"]))

    host = facts.get("host", {})
    available = host.get("memory_available_mb")
    if available is not None and available < guest_mb + reserve_mb:
        refusals.append(refusal("TUR-DUALRUN-040", f"free: {available} MiB; Windows would get {guest_mb} MiB "
                                f"and TUR keeps {reserve_mb} MiB"))
    if need_kvm and not host.get("kvm"):
        refusals.append(refusal("TUR-DUALRUN-041", "/dev/kvm cannot be opened"))
    return {"ok": not refusals, "refusals": refusals, "notes": notes, "give": sorted(give), "hide": sorted(hide)}

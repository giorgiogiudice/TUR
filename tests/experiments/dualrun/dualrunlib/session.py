# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Give the guest Windows' partitions and nothing else; layers over them.

**The view.** The guest never gets the disk. It gets one device-mapper
device with the disk's size and layout, in which

- each Windows partition is a `linear` mapping onto the real partition;
- the first and last sectors (protective MBR, both copies of the partition
  table) are a private copy in a file ("the shadow"), so nothing the guest
  writes there reaches the real table, and a change is seen afterwards;
- every other range (TUR's own partitions, other systems, unallocated
  space) reads as zeros and refuses writes (`flakey ... error_writes` over
  a `zero` device), or `zero` or `error` where that target is missing. No
  mapping to those ranges exists, so no fault in the guest or in QEMU can
  reach them.

**The lock.** Device-mapper opens each mapped partition exclusively. While
the view exists the kernel refuses to mount those partitions, and creating
the view fails if one is mounted. The view's name is derived from the
disk's GUID, so a second session on the same Windows cannot be created.

**Layers** (protected sessions). The view is created read-only and the
guest writes to a qcow2 file on TUR's side whose backing file is the view.
At the end the layer is thrown away (nothing ever reached the disk) or
kept: `commit` first saves what it is about to overwrite (the undo record),
then copies the layer into the real partitions; it can be interrupted and
run again. `undo` puts the saved blocks back, only while the disk is still
in the state the commit left it in.

Every function here needs root; QEMU itself does not.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import signal
import subprocess
import time

from . import checklist, gpt
from .blockio import Dev


class SessionError(Exception):
    """An operation could not be done; `refusal` holds the plain-words form when there is one."""

    def __init__(self, message: str, refusal: dict | None = None):
        super().__init__(message)
        self.refusal = refusal
        self.refusals = [refusal] if refusal else []
        self.facts = None


def run(*argv: str, stdin: str | None = None, check: bool = True) -> subprocess.CompletedProcess:
    done = subprocess.run(list(argv), input=stdin, capture_output=True, text=True)
    if check and done.returncode != 0:
        raise SessionError(f"{' '.join(argv)}: exit {done.returncode}: {done.stderr.strip()[:500]}")
    return done


def view_name(disk_guid: str) -> str:
    return "tur-dualrun-" + disk_guid.replace("-", "")[:12]


def view_exists(name: str) -> bool:
    return run("dmsetup", "info", name, check=False).returncode == 0


def open_count(name: str) -> int:
    out = run("dmsetup", "info", "-c", "--noheadings", "-o", "open", name).stdout.strip()
    return int(out or 0)


def targets() -> set[str]:
    run("modprobe", "dm-flakey", check=False)
    out = run("dmsetup", "targets", check=False).stdout
    return {line.split()[0] for line in out.splitlines() if line.split()}


def write_json(path: pathlib.Path, value: dict) -> None:
    """Replace a file so that a power cut leaves the old or the new content."""
    temporary = path.with_suffix(path.suffix + ".new")
    with open(temporary, "w") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def read_json(path: pathlib.Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def plan(facts: dict, decision: dict, hidden: str) -> list[dict]:
    """The view as consecutive ranges of 512-byte sectors covering the whole disk."""
    disk = facts["disk"]
    unit = disk["sector"] // 512
    segments = [{"start": 0, "sectors": disk["head_sectors"] * unit, "kind": "shadow-head"}]
    position = disk["head_sectors"]
    for part in sorted(facts["partitions"], key=lambda p: p["first_lba"]):
        if part["first_lba"] > position:
            segments.append({"start": position * unit, "sectors": (part["first_lba"] - position) * unit,
                             "kind": "gap"})
        given = part["index"] in decision["give"]
        if given and not part["node"]:
            raise SessionError(f"partition {part['index']} has no device node (an image file cannot be attached; "
                               "use a loop device with partitions)")
        segments.append({"start": part["first_lba"] * unit, "sectors": part["sectors"] * unit,
                         "kind": "given" if given else "hidden", "partition": part["index"],
                         "node": part["node"] if given else None})
        position = part["first_lba"] + part["sectors"]
    if disk["tail_start"] > position:
        segments.append({"start": position * unit, "sectors": (disk["tail_start"] - position) * unit, "kind": "gap"})
    segments.append({"start": disk["tail_start"] * unit, "sectors": (disk["sectors"] - disk["tail_start"]) * unit,
                     "kind": "shadow-tail"})
    for segment in segments:
        segment["hidden_as"] = hidden if segment["kind"] in ("hidden", "gap") else None
    total = sum(s["sectors"] for s in segments)
    if total != disk["sectors"] * unit or any(a["start"] + a["sectors"] != b["start"]
                                               for a, b in zip(segments, segments[1:])):
        raise SessionError("internal error: the view does not cover the disk exactly")
    return segments


def table(segments: list[dict], name: str, loop: str) -> str:
    lines = []
    for s in segments:
        if s["kind"] == "shadow-head":
            lines.append(f"{s['start']} {s['sectors']} linear {loop} 0")
        elif s["kind"] == "shadow-tail":
            lines.append(f"{s['start']} {s['sectors']} linear {loop} {segments[0]['sectors']}")
        elif s["kind"] == "given":
            lines.append(f"{s['start']} {s['sectors']} linear {s['node']} 0")
        elif s["hidden_as"] == "flakey":
            lines.append(f"{s['start']} {s['sectors']} flakey /dev/mapper/{name}-void 0 0 1 1 error_writes")
        else:
            lines.append(f"{s['start']} {s['sectors']} {s['hidden_as']}")
    return "\n".join(lines) + "\n"


def _shadow_bytes(disk_path: str, facts: dict) -> tuple[bytes, bytes]:
    disk = facts["disk"]
    with Dev(disk_path) as dev:
        head = dev.pread(0, disk["head_sectors"] * disk["sector"])
        tail = dev.pread(disk["tail_start"] * disk["sector"], (disk["sectors"] - disk["tail_start"]) * disk["sector"])
    return head, tail


def attach(disk_path: str, state_root: str, *, mode: str = "direct", hidden: str = "auto",
           owner: int | None = None, options: dict | None = None, skip_checks: bool = False) -> dict:
    """Check, then create the view. Returns the session record. Raises SessionError with the refusals."""
    options = options or {}
    facts = checklist.survey(disk_path)
    if facts["disk"].get("guid") and view_exists(view_name(facts["disk"]["guid"])):
        error = SessionError("a session on this disk already exists",
                             checklist.refusal("TUR-DUALRUN-050", f"view {view_name(facts['disk']['guid'])}"))
        raise error
    decision = checklist.decide(facts, mode=mode, **options)
    if not decision["ok"] and not skip_checks:
        error = SessionError("refused: " + ", ".join(r["code"] for r in decision["refusals"]))
        error.refusals = decision["refusals"]
        error.facts = facts
        raise error
    if skip_checks and any(r["code"] in ("TUR-DUALRUN-003", "TUR-DUALRUN-002", "TUR-DUALRUN-020")
                           for r in decision["refusals"]):
        raise SessionError("the disk cannot be attached: "
                           + "; ".join(r["evidence"] for r in decision["refusals"]))
    name = view_name(facts["disk"]["guid"])
    state = pathlib.Path(state_root) / name
    if view_exists(name) or (state / "session.json").exists():
        error = SessionError(f"{name} already exists", checklist.refusal("TUR-DUALRUN-050", f"view {name}"))
        error.refusals = [error.refusal]
        raise error
    state.mkdir(parents=True, exist_ok=True)
    os.chmod(state, 0o711)
    if hidden == "auto":
        available = targets()
        hidden = "flakey" if {"flakey", "zero"} <= available else ("zero" if "zero" in available else "error")
    segments = plan(facts, decision, hidden)
    head, tail = _shadow_bytes(disk_path, facts)
    shadow = state / "shadow.img"
    with open(shadow, "wb") as handle:
        handle.write(head + tail)
        handle.flush()
        os.fsync(handle.fileno())
    os.chmod(shadow, 0o600)
    loop = None
    void = False
    try:
        loop = run("losetup", "--find", "--show", str(shadow)).stdout.strip()
        if hidden == "flakey":
            longest = max((s["sectors"] for s in segments if s["kind"] in ("hidden", "gap")), default=8)
            run("dmsetup", "create", f"{name}-void", "--table", f"0 {longest} zero")
            void = True
        arguments = ["dmsetup", "create", name] + (["--readonly"] if mode == "protected" else [])
        run(*arguments, stdin=table(segments, name, loop))
    except SessionError:
        if void:
            run("dmsetup", "remove", f"{name}-void", check=False)
        if loop:
            run("losetup", "-d", loop, check=False)
        shadow.unlink(missing_ok=True)
        raise
    run("udevadm", "settle", check=False)
    node = os.path.realpath(f"/dev/mapper/{name}")
    if owner is not None:
        os.chown(node, owner, -1)
        os.chmod(node, 0o600)
    esp = next((p for p in facts["partitions"] if p["role"] == "esp" and p["index"] in decision["give"]), None)
    record = {
        "name": name, "device": f"/dev/mapper/{name}", "node": node, "disk": disk_path, "mode": mode,
        "hidden_as": hidden, "loop": loop, "void": void, "state": str(state), "owner": owner,
        "created": time.time(), "base_token": facts["base_token"],
        "shadow_sha256": hashlib.sha256(head + tail).hexdigest(),
        "disk_guid": facts["disk"]["guid"], "bytes": facts["disk"]["bytes"],
        "give": decision["give"], "hide": decision["hide"], "notes": decision["notes"],
        "refusals_overridden_for_experiment": [r["code"] for r in decision["refusals"]],
        "segments": segments,
        "esp": None if esp is None else {"index": esp["index"], "part_guid": esp["part_guid"],
                                         "first_lba": esp["first_lba"], "sectors": esp["sectors"]},
    }
    write_json(state / "session.json", record)
    return record


def _table_changes(disk_path: str, record: dict) -> dict:
    """Did the guest write to its copy of the partition table?"""
    state = pathlib.Path(record["state"])
    try:
        now = (state / "shadow.img").read_bytes()
    except OSError as error:
        return {"error": str(error)}
    changed = hashlib.sha256(now).hexdigest() != record["shadow_sha256"]
    out: dict = {"guest_changed_its_copy": changed}
    if changed:
        with Dev(disk_path) as dev:
            sector = dev.sector
        facts = checklist.survey(disk_path)
        head_bytes = facts["disk"]["head_sectors"] * sector
        head, tail = _shadow_bytes(disk_path, facts)
        original = head + tail
        differing = sorted({i // sector for i in range(0, min(len(now), len(original)), 1)
                            if now[i] != original[i]})[:16] if len(now) == len(original) else []
        out["differing_sectors_of_the_copy"] = differing
        out["head_differs"] = now[:head_bytes] != original[:head_bytes]
        out["tail_differs"] = now[head_bytes:] != original[head_bytes:]
        out["note"] = ("Windows wrote to the partition table inside the session. The real table was not "
                       "changed. Whether to apply such a change is a decision for the person.")
    return out


def detach(disk_path: str, state_root: str, name: str | None = None, *, force: bool = False) -> dict:
    """Remove the view. Refuses while something has it open, unless `force` (after a kill)."""
    if name is None:
        with Dev(disk_path) as dev:
            name = view_name(gpt.read_gpt(dev).disk_guid)
    state = pathlib.Path(state_root) / name
    record = read_json(state / "session.json") or {"name": name, "state": str(state), "loop": None, "void": True,
                                                    "shadow_sha256": None}
    result: dict = {"name": name}
    if view_exists(name):
        users = open_count(name)
        if users and not force:
            raise SessionError(f"{name} is still open by {users} program(s)")
        run("blockdev", "--flushbufs", f"/dev/mapper/{name}", check=False)
        result["partition_table"] = _table_changes(disk_path, record) if record.get("shadow_sha256") else {}
        for attempt in range(20):
            done = run("dmsetup", "remove", name, check=False)
            if done.returncode == 0:
                break
            time.sleep(0.5)
        else:
            raise SessionError(f"could not remove {name}: {done.stderr.strip()}")
    if run("dmsetup", "info", f"{name}-void", check=False).returncode == 0:
        run("dmsetup", "remove", f"{name}-void", check=False)
    if record.get("loop"):
        run("losetup", "-d", record["loop"], check=False)
    run("udevadm", "settle", check=False)
    (state / "session.json").unlink(missing_ok=True)
    (state / "shadow.img").unlink(missing_ok=True)
    after = checklist.survey(disk_path)
    result["base_token_before"] = record.get("base_token")
    result["base_token_after"] = after.get("base_token")
    result["after"] = after
    write_json(state / "last-session.json", {k: v for k, v in result.items() if k != "after"})
    return result


# -- layers -----------------------------------------------------------------

def layer_path(state_root: str, name: str) -> pathlib.Path:
    return pathlib.Path(state_root) / name / "layer.json"


def open_layer(record: dict, state_root: str, overlay: str, owner: int | None = None) -> dict:
    """Create the qcow2 layer over a read-only view (a protected session)."""
    if record["mode"] != "protected":
        raise SessionError("a layer needs a protected (read-only) view")
    path = layer_path(state_root, record["name"])
    if path.exists():
        error = SessionError("a layer is already waiting", checklist.refusal("TUR-DUALRUN-070", str(path)))
        raise error
    run("qemu-img", "create", "-q", "-f", "qcow2", "-F", "raw", "-b", record["device"], overlay)
    if owner is not None:
        os.chown(overlay, owner, -1)
    layer = {"overlay": overlay, "status": "open", "base_token": record["base_token"], "created": time.time(),
             "disk": record["disk"], "name": record["name"]}
    write_json(path, layer)
    return layer


def discard(disk_path: str, state_root: str, name: str) -> dict:
    """Throw the layer away. Nothing of it ever reached the disk."""
    path = layer_path(state_root, name)
    layer = read_json(path)
    if layer is None:
        raise SessionError("there is no layer to throw away")
    if layer["status"] != "open":
        raise SessionError(f"the layer is in state '{layer['status']}': a commit has begun and must be finished")
    result = detach(disk_path, state_root, name) if view_exists(name) else {"name": name}
    pathlib.Path(layer["overlay"]).unlink(missing_ok=True)
    path.unlink(missing_ok=True)
    result["discarded"] = True
    return result


def _extents(overlay: str) -> list[tuple[int, int]]:
    """Byte ranges the layer holds itself (what a commit will overwrite)."""
    out = run("qemu-img", "map", "--output=json", "-f", "qcow2", overlay).stdout
    ranges: list[tuple[int, int]] = []
    for entry in json.loads(out):
        if entry.get("depth") == 0 and entry.get("present", True) and (entry.get("data") or entry.get("zero")):
            if ranges and ranges[-1][0] + ranges[-1][1] == entry["start"]:
                ranges[-1] = (ranges[-1][0], ranges[-1][1] + entry["length"])
            else:
                ranges.append((entry["start"], entry["length"]))
    return ranges


def commit(disk_path: str, state_root: str, name: str, *, rate: int | None = None,
           test_kill_after: float | None = None) -> dict:
    """Keep the layer: save an undo record, then copy the layer into the real partitions.

    Safe to run again after an interruption at any point: the undo record is
    complete and on disk before the first block is copied, and copying the
    whole layer again gives the same result.
    """
    path = layer_path(state_root, name)
    layer = read_json(path)
    if layer is None:
        raise SessionError("there is no layer to keep")
    state = path.parent
    result: dict = {"name": name, "resumed": layer["status"] == "committing"}
    if view_exists(name):
        result["closed_view"] = {k: v for k, v in detach(disk_path, state_root, name).items() if k != "after"}
    if layer["status"] == "open":
        token = checklist.survey(disk_path).get("base_token")
        if token != layer["base_token"]:
            error = SessionError("the disk changed since the layer was opened",
                                 checklist.refusal("TUR-DUALRUN-072", f"token {token} != {layer['base_token']}"))
            error.refusals = [error.refusal]
            raise error
    elif layer["status"] != "committing":
        raise SessionError(f"the layer is in state '{layer['status']}'")
    record = attach(disk_path, state_root, mode="direct", owner=None, skip_checks=True)
    try:
        if layer["status"] == "open":
            ranges = _extents(layer["overlay"])
            undo_data = state / "undo.blocks"
            digest = hashlib.sha256()
            with Dev(record["device"]) as base, open(undo_data, "wb") as handle:
                for offset, length in ranges:
                    position = 0
                    while position < length:
                        chunk = base.pread(offset + position, min(4 * 1024 * 1024, length - position))
                        handle.write(chunk)
                        digest.update(chunk)
                        position += len(chunk)
                handle.flush()
                os.fsync(handle.fileno())
            os.chmod(undo_data, 0o600)
            layer.update(status="committing", undo={"ranges": ranges, "blocks": str(undo_data),
                                                    "bytes": sum(length for _, length in ranges),
                                                    "sha256": digest.hexdigest()})
            write_json(path, layer)
        result["bytes_in_layer"] = layer["undo"]["bytes"]
        started = time.time()
        arguments = ["qemu-img", "commit", "-q", "-d", "-f", "qcow2"] + (["-r", str(rate)] if rate else [])
        process = subprocess.Popen(arguments + [layer["overlay"]], stderr=subprocess.PIPE, text=True)
        if test_kill_after is not None:
            time.sleep(test_kill_after)
            if process.poll() is None:
                process.send_signal(signal.SIGKILL)
                process.wait()
                result["interrupted_for_test"] = True
                result["seconds_before_kill"] = round(time.time() - started, 2)
                return result
        _, stderr = process.communicate()
        if process.returncode != 0:
            raise SessionError(f"qemu-img commit failed: {stderr.strip()[:400]}")
        run("blockdev", "--flushbufs", record["device"], check=False)
        result["commit_seconds"] = round(time.time() - started, 2)
    finally:
        closed = detach(disk_path, state_root, name, force=True)
        result["partition_table"] = closed.get("partition_table")
    layer.update(status="committed", committed=time.time(), valid_for_token=closed["base_token_after"])
    pathlib.Path(layer["overlay"]).unlink(missing_ok=True)
    write_json(state / "undo.json", layer)
    path.unlink(missing_ok=True)
    result["committed"] = True
    result["undo_valid_for_token"] = closed["base_token_after"]
    return result


def undo(disk_path: str, state_root: str, name: str) -> dict:
    """Put back what the last commit overwrote. Only while the disk is as that commit left it."""
    state = pathlib.Path(state_root) / name
    saved = read_json(state / "undo.json")
    if saved is None or "undo" not in saved:
        raise SessionError("there is no restore point")
    if saved["status"] == "committed":
        token = checklist.survey(disk_path).get("base_token")
        if token != saved["valid_for_token"]:
            error = SessionError("the disk changed since the restore point was made",
                                 checklist.refusal("TUR-DUALRUN-072", f"token {token} != {saved['valid_for_token']}"))
            error.refusals = [error.refusal]
            raise error
        saved["status"] = "undoing"
        write_json(state / "undo.json", saved)
    elif saved["status"] != "undoing":
        raise SessionError(f"the restore point is in state '{saved['status']}'")
    record = attach(disk_path, state_root, mode="direct", owner=None, skip_checks=True)
    written = 0
    try:
        target = os.open(record["device"], os.O_WRONLY)
        try:
            with open(saved["undo"]["blocks"], "rb") as source:
                for offset, length in saved["undo"]["ranges"]:
                    position = 0
                    while position < length:
                        chunk = source.read(min(4 * 1024 * 1024, length - position))
                        if not chunk:
                            raise SessionError("the restore point is shorter than its index says")
                        os.pwrite(target, chunk, offset + position)
                        position += len(chunk)
                        written += len(chunk)
            os.fsync(target)
        finally:
            os.close(target)
    finally:
        closed = detach(disk_path, state_root, name, force=True)
    saved.update(status="undone", undone=time.time())
    write_json(state / "undo.json", saved)
    pathlib.Path(saved["undo"]["blocks"]).unlink(missing_ok=True)
    return {"name": name, "undone": True, "bytes_restored": written, "base_token_after": closed["base_token_after"],
            "partition_table": closed.get("partition_table")}

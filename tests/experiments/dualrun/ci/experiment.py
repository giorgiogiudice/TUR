#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The dualrun experiment: a real Windows, installed "natively", started as TUR's guest and back.

    experiment.py --work DIR --iso WINDOWS.iso --out DIR [--scenarios a,b] [--round-trips N]

Runs on one CI runner, never on a development machine. The Windows is
Microsoft's evaluation medium (tests/experiments/dualrun/README.md says how
it is obtained and what is never kept). Two virtual machine configurations
share one raw disk image:

  pc     stands in for the person's computer: the disk as a plain file on an
         emulated NVMe controller, its own firmware variables, SMBIOS
         strings, TPM and network address. Windows is installed here and
         "starts natively" here.
  guest  stands in for TUR's guest: the same disk attached as a loop device
         and given through tur-dualrun's device-mapper view (or a layer over
         it), other firmware variables with the launcher's boot entry, other
         SMBIOS strings unless carried, another TPM or none, Hyper-V
         enlightenments, no sleep states.

Both are QEMU/KVM. Nothing here is a bare-metal result.

Every scenario is independent after the installation and records what it
saw; a failure is a finding, the next scenario still runs. Results go to
OUT/results.json as they are produced, and OUT/results.md at the end.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import pathlib
import random
import shutil
import string
import subprocess
import sys
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "ci"))
sys.dont_write_bytecode = True

import guestscripts as gs  # noqa: E402
from dualrunlib import ovmfvars, qemu  # noqa: E402
from dualrunlib.qmp import QmpError  # noqa: E402
from winvm import Agent, AgentError, Machine  # noqa: E402

LAUNCHER = str(HERE / "tur-dualrun")
os.environ["MTOOLS_SKIP_CHECK"] = "1"      # mtools' geometry check does not apply to partitions
MIB = 1024 * 1024
GIB = 1024 * MIB

PC_IDENTITY = {
    "manufacturer": "Example Computers", "product": "Deskbox 5", "version": "1.0", "serial": "PC-SN-0001",
    "uuid": "11111111-2222-3333-4444-555555555555", "sku": "DB5", "family": "Deskbox",
    "board_manufacturer": "Example Computers", "board_product": "DB5-MB", "board_version": "A1",
    "board_serial": "MB-SN-0001", "chassis_manufacturer": "Example Computers", "chassis_serial": "CH-SN-0001",
    "bios_vendor": "Example Firmware", "bios_version": "F.10", "bios_date": "03/14/2024",
}
PC_DISK_SERIAL, PC_MAC = "PCNVME0001", "52:54:00:aa:00:01"
GUEST_IDENTITY = {"uuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"}
GUEST_DISK_SERIAL, GUEST_MAC = "TURGUEST0001", "52:54:00:bb:00:02"

ESTIMATE_MINUTES = {
    "roundtrips": 45, "states": 25, "faults": 30, "layers": 40, "files": 12, "integration": 14,
    "seamless": 12, "remoteapp": 16, "controller": 10, "savedstate": 14, "final": 10,
}
ORDER = ["roundtrips", "states", "faults", "layers", "files", "integration", "seamless", "remoteapp",
         "controller", "savedstate", "final"]


def sh(*argv: str, check: bool = True, timeout: float | None = None, env: dict | None = None,
       stdin: str | None = None) -> subprocess.CompletedProcess:
    done = subprocess.run(list(argv), capture_output=True, text=True, timeout=timeout, env=env, input=stdin)
    if check and done.returncode != 0:
        raise RuntimeError(f"{' '.join(argv[:6])}…: exit {done.returncode}: {done.stderr.strip()[-500:]}")
    return done


def tool(*argv: str, root: bool = True, env_extra: dict | None = None) -> tuple[int, dict, str]:
    """Run tur-dualrun; returns (exit code, its JSON, its stderr)."""
    command = [sys.executable, LAUNCHER, *argv]
    if root:
        keep = ["--preserve-env=TUR_DUALRUN_TEST_KILL_COMMIT_AFTER"] if env_extra else []
        command = ["sudo", *keep, *command]
    env = dict(os.environ, **(env_extra or {}))
    done = subprocess.run(command, capture_output=True, text=True, env=env)
    try:
        value = json.loads(done.stdout)
    except ValueError:
        value = {"unparsed": done.stdout[-500:]}
    return done.returncode, value, done.stderr


def sparse_sha(path: pathlib.Path, start: int = 0, length: int | None = None) -> str:
    """SHA-256 over the data extents of a sparse file (offsets included), skipping holes."""
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        end = os.fstat(handle.fileno()).st_size if length is None else start + length
        position = start
        while position < end:
            try:
                data_at = os.lseek(handle.fileno(), position, os.SEEK_DATA)
            except OSError:
                break
            if data_at >= end:
                break
            hole_at = min(os.lseek(handle.fileno(), data_at, os.SEEK_HOLE), end)
            digest.update(f"{data_at}:{hole_at - data_at};".encode())
            os.lseek(handle.fileno(), data_at, os.SEEK_SET)
            remaining = hole_at - data_at
            while remaining > 0:
                chunk = handle.read(min(8 * MIB, remaining))
                if not chunk:
                    break
                digest.update(chunk)
                remaining -= len(chunk)
            position = hole_at
    return digest.hexdigest()


def content_sha(path: pathlib.Path, start: int, length: int) -> str:
    """SHA-256 of a byte range, holes read as zeros (content, whatever the allocation)."""
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        handle.seek(start)
        remaining = length
        while remaining > 0:
            chunk = handle.read(min(8 * MIB, remaining))
            if not chunk:
                break
            digest.update(chunk)
            remaining -= len(chunk)
    return digest.hexdigest()


class Session:
    def __init__(self, kind: str, tag: str):
        self.kind, self.tag = kind, tag
        self.machine: Machine | None = None
        self.loop: str | None = None
        self.record: dict | None = None
        self.overlay: str | None = None
        self.info: dict = {"tag": tag, "kind": kind}
        self.ready = False


class Experiment:
    def __init__(self, args):
        self.work = pathlib.Path(args.work)
        self.out = pathlib.Path(args.out)
        self.vm = self.work / "vm"
        self.store = self.work / "store"
        self.iso = pathlib.Path(args.iso)
        self.disk = self.vm / "windows.img"
        self.round_trips = args.round_trips
        self.deadline = time.time() + args.budget_minutes * 60
        for directory in (self.vm, self.store, self.out, self.out / "sessions", self.out / "facts"):
            directory.mkdir(parents=True, exist_ok=True)
        self.firmware = qemu.firmware()
        self.password = "".join(random.SystemRandom().choice(string.ascii_letters + string.digits) for _ in range(14)) + "aZ7"
        self.uid = os.getuid()
        self.results: dict = {
            "meta": {"started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                     "dispatched_by": os.environ.get("DISPATCHED_BY", ""),
                     "commit": os.environ.get("GITHUB_SHA", ""), "run": os.environ.get("GITHUB_RUN_ID", ""),
                     "firmware": self.firmware},
            "sessions": [], "scenarios": {},
        }
        self.tur_partition: dict | None = None
        self.guest_vars = self.vm / "vars-guest.fd"
        self.pc_vars = self.vm / "vars-pc.fd"
        self.counter = 0
        self.windows_ok = True

    # -- bookkeeping --------------------------------------------------------

    def log(self, message: str) -> None:
        print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)

    def save(self) -> None:
        temporary = self.out / "results.json.new"
        temporary.write_text(json.dumps(self.results, indent=1, default=str) + "\n")
        os.replace(temporary, self.out / "results.json")

    def minutes_left(self) -> float:
        return (self.deadline - time.time()) / 60

    def host_facts(self) -> None:
        meta = self.results["meta"]
        meta["qemu"] = sh("qemu-system-x86_64", "--version").stdout.splitlines()[0]
        meta["kernel"] = os.uname().release
        devices = sh("qemu-system-x86_64", "-device", "help", check=False).stdout
        meta["qemu_has"] = {name: (f'"{name}"' in devices) for name in ("hv-balloon", "vmbus-bridge", "nvme", "tpm-crb")}
        meta["dm_targets"] = sorted(line.split()[0] for line in sh("sudo", "dmsetup", "targets", check=False).stdout.splitlines() if line.split())
        meta["cpu"] = next((line.split(":", 1)[1].strip() for line in pathlib.Path("/proc/cpuinfo").read_text().splitlines()
                            if line.startswith("model name")), "")
        meta["host_cpus"] = os.cpu_count()
        meta["host_memory_mb"] = int(next(line.split()[1] for line in pathlib.Path("/proc/meminfo").read_text().splitlines()
                                          if line.startswith("MemTotal"))) // 1024
        for name in ("media.json", "terms.json"):
            path = self.out / name
            if path.exists():
                meta[name.split(".")[0]] = json.loads(path.read_text())
        self.save()

    # -- machines -----------------------------------------------------------

    def pc_cfg(self) -> dict:
        return {
            "memory_mb": 8192, "cpus": 4, "code": self.firmware["code"], "vars": str(self.pc_vars),
            "identity": PC_IDENTITY, "hyperv": [],
            "disk": {"path": str(self.disk), "format": "raw", "controller": "nvme", "serial": PC_DISK_SERIAL},
            "net": {"restrict": True, "mac": PC_MAC},
        }

    def start_native(self, tag: str, *, timeout: float = 900, cfg_extra: dict | None = None,
                     expect_resume: bool = False) -> Session:
        s = Session("native", tag)
        cfg = self.pc_cfg()
        cfg.update(cfg_extra or {})
        s.machine = Machine(f"pc-{tag}", cfg, self.out / "sessions" / tag, self.vm / "run" / tag, self.vm / "tpm-pc")
        self.log(f"native start: {tag}")
        s.machine.start()
        self._wait_ready(s, timeout, expect_resume)
        return s

    def attach(self, tag: str, *, mode: str = "direct", hidden: str = "auto", guest_mb: int = 6144,
               allow_hibernated: bool = False) -> tuple[Session, int, dict]:
        """Loop device, check-list, view (and layer). Returns the session and the launcher's answer."""
        s = Session("guest", tag)
        s.loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        arguments = ["attach", s.loop, "--state", str(self.store), "--owner", str(self.uid), "--mode", mode,
                     "--hidden", hidden, "--guest-mb", str(guest_mb), "--reserve-mb", "1024"]
        if mode == "protected":
            s.overlay = str(self.vm / f"layer-{tag}.qcow2")
            pathlib.Path(s.overlay).unlink(missing_ok=True)
            arguments += ["--overlay", s.overlay]
        if allow_hibernated:
            arguments.append("--experiment-allow-hibernated")
        code, value, err = tool(*arguments)
        if code == 0:
            s.record = value
        else:
            self.release_loop(s)
        s.info["attach"] = {"exit": code, "refusals": [r["code"] for r in value.get("refusals", [])] if code else [],
                            "mode": mode, "hidden_as": value.get("hidden_as"), "notes": value.get("notes"),
                            "give": value.get("give"), "hide": value.get("hide"),
                            "error": None if code == 0 else (value.get("error") or err[-300:])}
        return s, code, value

    def release_loop(self, s: Session) -> None:
        if s.loop:
            sh("sudo", "losetup", "-d", s.loop, check=False)
            sh("sudo", "udevadm", "settle", check=False)
            s.loop = None

    def start_guest(self, tag: str, *, mode: str = "direct", identity: str = "generic", tpm: str = "guest",
                    controller: str = "nvme", hidden: str = "auto", balloon: bool = True, hyperv: bool = True,
                    memory_mb: int = 6144, cpus: int = 2, net: dict | None = None, timeout: float = 900,
                    allow_hibernated: bool = False, vars_template: str | None = None,
                    incoming: str | None = None, attached: Session | None = None,
                    usb_disks: list | None = None) -> Session:
        if attached is None:
            s, code, value = self.attach(tag, mode=mode, hidden=hidden, guest_mb=memory_mb,
                                         allow_hibernated=allow_hibernated)
            if code != 0:
                s.info["refused"] = True
                return s
        else:
            s = attached
        record = s.record
        variables = self.guest_vars
        if vars_template or mode == "protected":
            variables = self.vm / f"vars-{tag}.fd"
            if vars_template is None and self.guest_vars.exists():
                shutil.copyfile(self.guest_vars, variables)
        if not variables.exists():
            session_file = str(pathlib.Path(record["state"]) / "session.json")
            arguments = ["vars", "--session", session_file, "--out", str(variables)]
            if vars_template:
                arguments += ["--template", vars_template]
            code, value, err = tool(*arguments, root=False)
            s.info["firmware_entry"] = value if code == 0 else {"error": err[-300:]}
            if code != 0:
                shutil.copyfile(self.firmware["vars_blank"], variables)
        carried = identity == "carried"
        cfg = {
            "memory_mb": memory_mb, "cpus": cpus, "code": self.firmware["code"], "vars": str(variables),
            "identity": PC_IDENTITY if carried else GUEST_IDENTITY,
            "hyperv_auto": hyperv, "hyperv": [], "no_sleep_states": True,
            "balloon": balloon and hyperv and bool(self.results["meta"].get("qemu_has", {}).get("hv-balloon")),
            "disk": {"path": getattr(s, "overlay", None) if mode == "protected" else record["device"],
                     "format": "qcow2" if mode == "protected" else "raw", "controller": controller,
                     "serial": PC_DISK_SERIAL if carried else GUEST_DISK_SERIAL},
            "net": dict({"restrict": True, "mac": PC_MAC if carried else GUEST_MAC}, **(net or {})),
            "usb_disks": usb_disks or [],
        }
        if incoming:
            cfg["incoming"] = incoming
        tpm_dir = {"guest": self.vm / "tpm-guest", "pc": self.vm / "tpm-pc", "none": None}[tpm]
        s.info.update(mode=mode, identity=identity, tpm=tpm, controller=controller, memory_mb=memory_mb, cpus=cpus)
        s.machine = Machine(f"guest-{tag}", cfg, self.out / "sessions" / tag, self.vm / "run" / tag, tpm_dir)
        self.log(f"guest start: {tag} (mode {mode}, identity {identity}, tpm {tpm}, {controller})")
        # udev may have put the node's owner back after a change event; the helper set it once.
        sh("sudo", "chown", str(self.uid), record["node"], check=False)
        try:
            s.machine.start()
        except RuntimeError as error:
            s.info["start_error"] = str(error)[-400:]
            return s
        s.info["hyperv"] = s.machine.hyperv_used
        s.info["balloon_device"] = bool(s.machine.cfg.get("balloon"))
        if incoming is None:
            self._wait_ready(s, timeout, False)
        return s

    def _wait_ready(self, s: Session, timeout: float, expect_resume: bool) -> None:
        m = s.machine
        hello = None
        deadline = time.time() + timeout
        next_look = time.time() + 240
        while time.time() < deadline and m.alive():
            hello = m.agent.wait_hello(15, m.alive)
            if hello:
                break
            if expect_resume and m.seconds() > 40:
                try:
                    if m.agent.ps("'alive'", timeout=10).strip().endswith("alive"):
                        hello = {"ev": "resumed"}
                        break
                except AgentError:
                    pass
            if time.time() > next_look:
                _, text = m.screenshot("waiting", keep=False)
                self.log(f"  {s.tag}: {m.seconds():.0f} s, screen says: {text[:160]}")
                next_look = time.time() + 240
        s.info["start_to_agent_s"] = m.seconds() if hello else None
        s.info["hello"] = hello
        s.ready = bool(hello)
        if not hello:
            _, text = m.screenshot("no-agent", keep=True)
            s.info["no_agent"] = {"qemu_alive": m.alive(), "after_s": m.seconds(), "screen_text": text}
            self.log(f"  {s.tag}: NO AGENT after {m.seconds():.0f} s; screen: {text[:200]}")
        else:
            self.log(f"  {s.tag}: agent after {s.info['start_to_agent_s']} s")

    def facts(self, s: Session, tag: str | None = None) -> dict | None:
        tag = tag or s.tag
        try:
            value = s.machine.agent.json(gs.FACTS, timeout=420)
        except (AgentError, ValueError) as error:
            s.info["facts_error"] = str(error)[:300]
            return None
        (self.out / "facts" / f"{tag}.json").write_text(json.dumps(value, indent=1) + "\n")
        licence = value.get("licence", {})
        s.info["facts"] = {
            "windows": f"{value.get('caption')} {value.get('version')}",
            "computer": value.get("computer"), "product": value.get("product"),
            "licence_status": licence.get("status"), "licence_reason": licence.get("reason"),
            "grace_minutes": licence.get("grace_minutes"), "rearms_left": licence.get("rearms_left"),
            "machine_guid": value.get("machine_guid"),
            "devices_present": value.get("devices_present"), "devices_known": value.get("devices_known"),
            "devices_with_problems": value.get("devices_with_problems"),
            "boot_drivers": value.get("boot_drivers"), "tpm": value.get("tpm"), "secure_boot": value.get("secure_boot"),
            "volumes": value.get("volumes"), "partitions": value.get("partitions"), "disks": value.get("disks"),
            "power": value.get("power"), "boot_type": value.get("boot_type"), "firmware": value.get("firmware"),
            "windows_boot_utc": value.get("boot"),
            "pnp_events": {e["k"]: e["n"] for e in value.get("system_events", [])
                           if any(w in e["k"] for w in ("Kernel-PnP", "UserPnp", "DeviceSetupManager"))},
            "storage_events": {e["k"]: e["n"] for e in value.get("system_events", [])
                               if any(w in e["k"].lower() for w in ("ntfs", "disk", "stornvme", "storahci", "volmgr"))},
            "licensing_events": value.get("licensing_events"),
        }
        return value

    def end(self, s: Session, how: str = "shutdown", timeout: float = 420) -> None:
        """End the machine; for a direct guest session also remove the view."""
        m = s.machine
        if m is not None and m.alive():
            started = time.time()
            if not s.ready and how != "kill":
                how, timeout = "powerdown", 90      # no agent to ask: the power button, briefly, then off
            try:
                if how == "kill":
                    m.kill()
                elif how == "powerdown":
                    m.qmp.command("system_powerdown")
                elif how == "hybrid":
                    m.agent.ps("shutdown /s /hybrid /t 0", timeout=60)
                elif how == "hibernate":
                    m.agent.ps("shutdown /h", timeout=60)
                else:
                    m.agent.ps("shutdown /s /t 0", timeout=60)
            except (AgentError, QmpError) as error:
                s.info["end_error"] = str(error)[:200]
            if how != "kill" and not m.wait_exit(timeout):
                _, text = m.screenshot("did-not-stop", keep=True)
                s.info["forced_off"] = {"after_s": round(time.time() - started), "screen_text": text}
                m.kill()
            s.info["end"] = how
            s.info["end_seconds"] = round(time.time() - started, 1)
        elif m is not None:
            m._after_exit()
        if s.kind == "guest" and s.record is not None and s.record["mode"] == "direct":
            self.detach(s)
        if s.kind == "native":
            self.results["sessions"].append(s.info)
            self.save()

    def detach(self, s: Session, force: bool = True) -> dict:
        code, value, err = tool("detach", s.loop, "--state", str(self.store), *(["--force"] if force else []))
        after = value.pop("after", {}) if isinstance(value, dict) else {}
        s.info["detach"] = {"exit": code, "partition_table": value.get("partition_table"),
                            "token_changed": value.get("base_token_before") != value.get("base_token_after"),
                            "error": None if code == 0 else (value.get("error") or err[-300:])}
        s.info["volume_after"] = self.volume_state(after)
        self.release_loop(s)
        s.info["tur_partition_unchanged"] = self.tur_unchanged()
        s.record = None
        self.results["sessions"].append(s.info)
        self.save()
        return value

    def finish_layer(self, s: Session, action: str, **extra) -> tuple[int, dict]:
        """discard or commit a protected session's layer; the launcher removes the view."""
        arguments = [action, s.loop, "--state", str(self.store)]
        if extra.get("rate"):
            arguments += ["--rate", str(extra["rate"])]
        env = {"TUR_DUALRUN_TEST_KILL_COMMIT_AFTER": str(extra["kill_after"])} if extra.get("kill_after") else None
        code, value, err = tool(*arguments, env_extra=env)
        value.pop("after", None)
        s.info[action] = dict(value, exit=code, stderr=err[-300:] if code else "")
        return code, value

    def close_layer_session(self, s: Session) -> None:
        self.release_loop(s)
        s.info["tur_partition_unchanged"] = self.tur_unchanged()
        s.record = None
        self.results["sessions"].append(s.info)
        self.save()

    @staticmethod
    def volume_state(survey: dict) -> dict:
        out = {}
        for part in survey.get("partitions", []):
            content = part.get("content", {})
            if part.get("role") == "windows-data" and content.get("kind") == "ntfs":
                state = content.get("ntfs", {})
                journal = state.get("journal") or {}
                out[f"p{part['index']} {content.get('label')}"] = {
                    "dirty": state.get("dirty"), "journal_clean": state.get("journal_clean"),
                    "hibernation": state.get("hibernation"),
                    "hiberfil": {k: (state.get("hiberfil") or {}).get(k) for k in ("present", "size", "magic_text", "head_hex")},
                    "restart_pages": [{k: p.get(k) for k in ("magic", "version", "flags", "client_in_use", "clean")}
                                      for p in journal.get("pages", [])],
                    "errors": state.get("errors"),
                }
            if part.get("role") == "esp":
                out["boot configuration"] = part.get("esp", {}).get("bcd") or {"error": part.get("esp", {}).get("bcd_error")}
                out["fallback loader is Windows' own"] = part.get("esp", {}).get("fallback_loader_is_windows")
        return out

    def inspect(self) -> tuple[dict, dict]:
        """The launcher's survey and decision of the disk as it lies (through a loop device)."""
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        try:
            _, survey, _ = tool("inspect", loop)
            code, decision, _ = tool("check", loop, "--guest-mb", "6144", "--reserve-mb", "1024")
        finally:
            sh("sudo", "losetup", "-d", loop, check=False)
            sh("sudo", "udevadm", "settle", check=False)
        answer = decision.get("decision", {})
        return self.volume_state(survey), {"exit": code, "refusals": [r["code"] for r in answer.get("refusals", [])],
                                           "notes": answer.get("notes")}

    def tur_unchanged(self) -> bool | None:
        if not self.tur_partition:
            return None
        return content_sha(self.disk, self.tur_partition["offset"], self.tur_partition["bytes"]) == self.tur_partition["sha256"]

    def esp_manifest(self, tag: str) -> dict:
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        dest = self.vm / f"esp-{tag}"
        try:
            tool("esp-backup", loop, "--dest", str(dest), "--manifest-only")
            sh("sudo", "chown", "-R", f"{self.uid}", str(dest), check=False)
            return json.loads((dest / "manifest.json").read_text())["files"]
        except (OSError, ValueError):
            return {}
        finally:
            sh("sudo", "losetup", "-d", loop, check=False)

    def firmware_entries(self, path: pathlib.Path) -> dict:
        try:
            return ovmfvars.boot_entries(path.read_bytes())
        except Exception as error:  # noqa: BLE001
            return {"error": str(error)}

    # -- installation -------------------------------------------------------

    def install(self) -> bool:
        scenario = self.results["scenarios"].setdefault("install", {})
        sh("truncate", "-s", "100G", str(self.disk))
        shutil.copyfile(self.firmware["vars_blank"], self.pc_vars)
        answer = self.vm / "answer"
        (answer / "dualrun").mkdir(parents=True, exist_ok=True)
        index = 1
        terms = self.results["meta"].get("terms", {})
        for image in terms.get("images", []):
            if "eval" in (image.get("name", "") + image.get("edition_id", "")).lower():
                index = image["index"]
        scenario["image_index"] = index
        scenario["images_on_medium"] = terms.get("images")
        text = (HERE / "guest" / "autounattend.xml.in").read_text()
        text = text.replace("@PASSWORD@", self.password).replace("@WINDOWS_MB@", "70000") \
                   .replace("@DATA_MB@", "2048").replace("@IMAGE_INDEX@", str(index))
        (answer / "autounattend.xml").write_text(text)
        (answer / "dualrun" / "setup.cmd").write_text(
            (HERE / "guest" / "setup.cmd").read_text().replace("@PASSWORD@", self.password).replace("\n", "\r\n"))
        shutil.copyfile(HERE / "guest" / "agent.ps1", answer / "dualrun" / "agent.ps1")
        answer_iso = self.vm / "answer.iso"
        sh("xorriso", "-as", "mkisofs", "-J", "-R", "-V", "DUALRUN", "-o", str(answer_iso), str(answer))
        shutil.rmtree(answer)
        cfg = {"cdroms": [{"path": str(self.iso), "bootindex": 0}, {"path": str(answer_iso)}]}
        base = self.pc_cfg()
        base["disk"]["bootindex"] = 1
        cfg["disk"] = base["disk"]
        s = Session("native", "install")
        s.machine = Machine("pc-install", dict(base, **cfg), self.out / "sessions" / "install",
                            self.vm / "run" / "install", self.vm / "tpm-pc")
        self.log("installation: start")
        s.machine.start()
        scenario["hyperv"] = s.machine.hyperv_used
        # "Press any key to boot from CD or DVD": answer it during the first start only.
        until = time.time() + 45
        while time.time() < until and s.machine.alive():
            s.machine.key("ret")
            time.sleep(0.7)
        self._wait_ready(s, min(120 * 60, max(600, (self.minutes_left() - 60) * 60)), False)
        scenario["seconds"] = s.info.get("start_to_agent_s")
        scenario["agent"] = s.ready
        if not s.ready:
            scenario["no_agent"] = s.info.get("no_agent")
            s.machine.kill()
            self.results["sessions"].append(s.info)
            self.save()
            return False
        time.sleep(60)          # let the first sign-in finish its own work
        value = self.facts(s, "install")
        scenario["windows"] = s.info.get("facts", {}).get("windows")
        try:
            s.machine.agent.ps(gs.MAKE_DOCS, timeout=120)
            scenario["power_states_offered"] = (value or {}).get("power", {}).get("sleep_states")
        except AgentError as error:
            scenario["setup_error"] = str(error)[:200]
        self.end(s, "shutdown")
        scenario["firmware_entries_after_install"] = self.firmware_entries(self.pc_vars)
        answer_iso.unlink(missing_ok=True)
        self.add_tur_partition()
        scenario["disk_allocated_gib"] = round(os.stat(self.disk).st_blocks * 512 / GIB, 1)
        self.save()
        return True

    def add_tur_partition(self) -> None:
        """A 1 GiB Linux partition with ext4 in the free space: what TUR's installer would add."""
        scenario = self.results["scenarios"]["install"]
        before = sh("sgdisk", "-p", str(self.disk)).stdout
        # Type and name as the installer of pull request 112 makes it: Linux root (x86-64), "TUR".
        sh("sgdisk", "-n", "0:0:+1G", "-t", "0:8304", "-c", "0:TUR", str(self.disk))
        listing = sh("sgdisk", "-p", str(self.disk)).stdout
        scenario["partition_table"] = listing.strip().splitlines()[-8:]
        line = [l.split() for l in listing.splitlines() if l.split() and l.split()[-1] == "TUR"][0]
        number, first, last = int(line[0]), int(line[1]), int(line[2])
        offset, size = first * 512, (last - first + 1) * 512
        made = sh("mkfs.ext4", "-q", "-F", "-L", "tur-root", "-E", f"offset={offset}", str(self.disk), f"{size // 1024}k",
                  check=False)
        with open(self.disk, "r+b") as handle:
            handle.seek(offset + 512 * MIB)
            handle.write(random.Random(7).randbytes(8 * MIB))
        self.tur_partition = {"number": number, "offset": offset, "bytes": size,
                              "sha256": content_sha(self.disk, offset, size),
                              "ext4": made.returncode == 0, "partitions_before": len(before.splitlines())}
        scenario["tur_partition"] = self.tur_partition

    # -- scenarios ----------------------------------------------------------

    def scenario_roundtrips(self, out: dict) -> None:
        out["esp_before"] = self.esp_manifest("before")
        s = self.start_native("native-0")
        if not s.ready:
            out["error"] = "the first native start after the installation did not reach the agent"
            self.end(s, "kill")
            self.windows_ok = False
            return
        time.sleep(45)
        baseline = self.facts(s)
        out["baseline_boot_drivers"] = (baseline or {}).get("boot_drivers")
        self.end(s, "shutdown")
        out["pc_firmware_entries"] = self.firmware_entries(self.pc_vars)
        plan = [("generic", "guest"), ("generic", "guest"), ("carried", "guest"), ("carried", "guest"), ("carried", "none")]
        trips = []
        for number in range(1, self.round_trips + 1):
            identity, tpm = plan[min(number, len(plan)) - 1]
            trip: dict = {"trip": number, "identity": identity, "tpm": tpm}
            g = self.start_guest(f"guest-{number}", identity=identity, tpm=tpm)
            via_layer = False
            if g.info.get("refused"):
                # The check-list refused a volume Windows shut down completely: a finding about the
                # check-list. Record what it saw, and still measure the round trip through a layer
                # that is committed at the end (the experiment switch exists only for protected mode).
                trip["direct_start_refused"] = g.info.get("attach")
                trip["volume_when_refused"], _ = self.inspect()
                self.results["sessions"].append(g.info)
                g = self.start_guest(f"guest-{number}-layer", mode="protected", allow_hibernated=True,
                                     identity=identity, tpm=tpm)
                via_layer = True
            if g.info.get("refused") or not g.machine:
                trip["guest"] = {"refused": g.info.get("attach")}
                self.results["sessions"].append(g.info)
                trips.append(trip)
                continue
            if g.ready:
                time.sleep(30)
                self.facts(g)
                if number == 1:
                    try:
                        g.info["disk_check"] = g.machine.agent.json(gs.DISK_CHECK, timeout=600)
                    except (AgentError, ValueError) as error:
                        g.info["disk_check"] = {"error": str(error)[:200]}
            self.end(g, "powerdown" if number % 2 == 0 else "shutdown")
            if via_layer:
                code, value = self.finish_layer(g, "commit")
                trip["committed_layer"] = {"exit": code, "committed": value.get("committed"),
                                           "seconds": value.get("commit_seconds"), "bytes": value.get("bytes_in_layer")}
                self.close_layer_session(g)
            trip["guest"] = {k: g.info.get(k) for k in ("start_to_agent_s", "end", "end_seconds", "forced_off",
                                                        "no_agent", "detach", "tur_partition_unchanged", "volume_after")}
            n = self.start_native(f"native-{number}")
            if n.ready:
                time.sleep(20)
                self.facts(n)
            self.end(n, "shutdown")
            trip["native"] = {k: n.info.get(k) for k in ("start_to_agent_s", "end_seconds", "forced_off", "no_agent")}
            trips.append(trip)
            out["trips"] = trips
            self.save()
            if not g.ready and not n.ready:
                self.windows_ok = False
                out["stopped"] = f"after trip {number}: Windows reached the agent in neither configuration"
                break
        out["trips"] = trips
        out["guest_firmware_entries"] = self.firmware_entries(self.guest_vars) if self.guest_vars.exists() else None

    def scenario_states(self, out: dict) -> None:
        """What a real Windows leaves on the disk after each kind of shutdown, and what the check-list says."""
        s = self.start_native("state-prepare")
        if not s.ready:
            out["error"] = "no agent"
            self.end(s, "kill")
            return
        try:
            out["hibernate_on"] = s.machine.agent.ps("powercfg /h on 2>&1; powercfg /a", timeout=120).strip()[-900:]
            s.machine.agent.ps(r"Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' "
                               "HiberbootEnabled 1", timeout=60)
        except AgentError as error:
            out["hibernate_on"] = f"error: {error}"
        self.end(s, "shutdown")
        out["after_full_shutdown"] = dict(zip(("volume", "check"), self.inspect()))

        s = self.start_native("state-fast")
        if s.ready:
            self.facts(s)
        self.end(s, "hybrid")
        out["after_fast_startup_shutdown"] = dict(zip(("volume", "check"), self.inspect()))
        refused = out["after_fast_startup_shutdown"]["check"]["refusals"]
        out["fast_startup_refused"] = any(code in ("TUR-DUALRUN-010", "TUR-DUALRUN-011") for code in refused)

        # What Windows does if it is started anyway: only in a protected session, thrown away afterwards.
        g = self.start_guest("state-fast-anyway", mode="protected", allow_hibernated=True, timeout=600)
        if g.machine:
            if g.ready:
                time.sleep(20)
                self.facts(g)
            self.end(g, "shutdown")
            self.finish_layer(g, "discard")
            self.close_layer_session(g)
        else:
            self.results["sessions"].append(g.info)
        out["started_anyway_in_a_protected_session"] = {k: g.info.get(k) for k in ("attach", "start_to_agent_s", "no_agent", "discard")}
        out["started_anyway_in_a_protected_session"]["boot_type"] = g.info.get("facts", {}).get("boot_type")

        s = self.start_native("state-after-fast")
        if s.ready:
            time.sleep(15)
            self.facts(s)
        out["native_start_after_fast_startup"] = {"start_to_agent_s": s.info.get("start_to_agent_s"),
                                                 "boot_type": s.info.get("facts", {}).get("boot_type")}
        self.end(s, "hibernate")
        out["after_hibernation"] = dict(zip(("volume", "check"), self.inspect()))
        refused = out["after_hibernation"]["check"]["refusals"]
        out["hibernation_refused"] = any(code in ("TUR-DUALRUN-010", "TUR-DUALRUN-011") for code in refused)

        s = self.start_native("state-resume", expect_resume=True)
        out["native_resume"] = {"start_to_agent_s": s.info.get("start_to_agent_s"), "hello": s.info.get("hello")}
        if s.ready:
            self.facts(s)
        self.end(s, "shutdown")
        out["after_resume_and_full_shutdown"] = dict(zip(("volume", "check"), self.inspect()))

    def scenario_faults(self, out: dict) -> None:
        """Power cuts: QEMU killed at chosen moments; the launcher killed; Windows' own check afterwards."""
        points = [("during-start", 25), ("idle", None), ("while-writing", None), ("during-shutdown", None)]
        cuts = []
        previous = None
        for name, delay in points:
            cut: dict = {"point": name}
            g = self.start_guest(f"cut-{name}", timeout=30 if delay else 900)
            if not g.machine:
                cut["refused"] = g.info.get("attach")
                self.results["sessions"].append(g.info)
                cuts.append(cut)
                continue
            if previous is not None and g.ready:
                # This start is also the follow-up start of the previous cut.
                try:
                    previous["windows_check_in_next_start"] = g.machine.agent.json(gs.DISK_CHECK, timeout=900)
                except (AgentError, ValueError) as error:
                    previous["windows_check_in_next_start"] = {"error": str(error)[:200]}
            if delay:
                time.sleep(max(0, delay - g.machine.seconds()))
            elif g.ready:
                try:
                    if name == "idle":
                        time.sleep(20)
                    elif name == "while-writing":
                        g.machine.agent.ps(gs.START_LOAD, timeout=60)
                        time.sleep(8)
                    else:
                        g.machine.agent.ps("shutdown /s /t 0", timeout=30)
                        time.sleep(4)
                except AgentError as error:
                    cut["error"] = str(error)[:200]
            cut["killed_after_s"] = g.machine.seconds()
            cut["agent_was_up"] = g.ready
            self.end(g, "kill")
            cut["volume_after"] = g.info.get("volume_after")
            cut["tur_partition_unchanged"] = g.info.get("tur_partition_unchanged")
            _, decision = self.inspect()
            cut["check_after"] = decision
            cuts.append(cut)
            previous = cut
            out["cuts"] = cuts
            self.save()

        # The launcher dies while the guest runs; `recover` must shut Windows down properly.
        died: dict = {}
        s, code, value = self.attach("launcher-killed")
        if code == 0:
            run_dir = self.vm / "run" / "launcher-killed"
            run_dir.mkdir(parents=True, exist_ok=True)
            if not self.guest_vars.exists():
                tool("vars", "--session", str(pathlib.Path(s.record["state"]) / "session.json"),
                     "--out", str(self.guest_vars), root=False)
            cfg = {"name": "guest-launcher-killed", "memory_mb": 6144, "cpus": 2, "code": self.firmware["code"],
                   "vars": str(self.guest_vars), "identity": PC_IDENTITY, "hyperv": qemu.HYPERV_SETS[1],
                   "no_sleep_states": True, "net": {"restrict": True, "mac": PC_MAC},
                   "disk": {"path": s.record["device"], "format": "raw", "controller": "nvme", "serial": PC_DISK_SERIAL},
                   "qmp": str(run_dir / "qmp.sock"), "serial": str(run_dir / "serial.sock")}
            (run_dir / "config.json").write_text(json.dumps(cfg))
            launcher = subprocess.Popen([sys.executable, LAUNCHER, "run", "--config", str(run_dir / "config.json"),
                                         "--run", str(run_dir)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(5)
            agent = Agent(cfg["serial"], self.out / "sessions" / "launcher-killed-serial.log")
            started = time.time()
            try:
                agent.connect()
                hello = agent.wait_hello(900)
                died["agent_after_s"] = round(time.time() - started + 5, 1) if hello else None
                if previous is not None and hello:
                    try:
                        previous["windows_check_in_next_start"] = agent.json(gs.DISK_CHECK, timeout=900)
                        agent.ps(gs.CLEAN_LOAD, timeout=300)
                    except (AgentError, ValueError) as error:
                        previous["windows_check_in_next_start"] = {"error": str(error)[:200]}
                launcher.kill()
                launcher.wait()
                died["launcher_killed"] = True
                time.sleep(3)
                record = json.loads((run_dir / "run.json").read_text()) if (run_dir / "run.json").exists() else {}
                died["guest_survived_the_launcher"] = bool(record) and pathlib.Path(f"/proc/{record.get('pid')}").exists()
                code, recovered, err = tool("recover", s.loop, "--state", str(self.store), "--run", str(run_dir),
                                            "--wait", "300", "--kill")
                recovered.pop("after", None)
                died["recover"] = dict(recovered, exit=code, stderr=err[-200:] if code else "")
            except AgentError as error:
                died["error"] = str(error)
                launcher.kill()
                tool("recover", s.loop, "--state", str(self.store), "--run", str(run_dir), "--wait", "5", "--kill")
            finally:
                agent.close()
            self.release_loop(s)
            volume, decision = self.inspect()
            died["volume_after"], died["check_after"] = volume, decision
            died["tur_partition_unchanged"] = self.tur_unchanged()
        else:
            died["refused"] = s.info.get("attach")
        out["launcher_killed"] = died
        self.save()

        n = self.start_native("after-cuts")
        final: dict = {"start_to_agent_s": n.info.get("start_to_agent_s"), "no_agent": n.info.get("no_agent")}
        if n.ready:
            try:
                final["disk_check"] = n.machine.agent.json(gs.DISK_CHECK, timeout=900)
                final["chkdsk"] = n.machine.agent.json(gs.CHKDSK, timeout=1500)
                final["cleanup"] = n.machine.agent.ps(gs.CLEAN_LOAD, timeout=300).strip()
                final["documents"] = n.machine.agent.json(gs.DOC_HASHES, timeout=120).get("count")
            except (AgentError, ValueError) as error:
                final["error"] = str(error)[:300]
            self.facts(n)
        self.end(n, "shutdown")
        out["native_start_after_all_cuts"] = final

    def scenario_layers(self, out: dict) -> None:
        """Protected sessions: throw away, keep, interrupted keep, undo, repair of the boot configuration."""
        n = self.start_native("layers-before")
        before = None
        if n.ready:
            try:
                n.machine.agent.ps(gs.MAKE_DOCS, timeout=120)
                before = n.machine.agent.json(gs.DOC_HASHES, timeout=120)
            except (AgentError, ValueError) as error:
                out["error"] = str(error)[:200]
        self.end(n, "shutdown")
        out["documents_before"] = (before or {}).get("count")
        if before is None:
            return
        disk_before = sparse_sha(self.disk)

        # 1. A destructive session, thrown away.
        g = self.start_guest("layer-discard", mode="protected")
        one: dict = {"attach": g.info.get("attach"), "start_to_agent_s": g.info.get("start_to_agent_s")}
        if g.machine:
            if g.ready:
                try:
                    one["destroyed"] = g.machine.agent.ps(gs.DESTROY, timeout=300).strip()[-40:]
                    one["inside_after"] = g.machine.agent.json(gs.DOC_HASHES, timeout=120)
                except (AgentError, ValueError) as error:
                    one["error"] = str(error)[:200]
            self.end(g, "shutdown")
            one["layer_bytes"] = os.path.getsize(g.overlay) if os.path.exists(g.overlay) else None
            self.finish_layer(g, "discard")
            one["discard"] = g.info.get("discard", {}).get("exit")
            self.close_layer_session(g)
        one["disk_identical_after_discard"] = sparse_sha(self.disk) == disk_before
        n = self.start_native("layer-discard-native")
        if n.ready:
            try:
                after = n.machine.agent.json(gs.DOC_HASHES, timeout=120)
                one["documents_as_before_natively"] = after == before
                one["native_view"] = {"count": after.get("count"), "public_folder": after.get("public")}
            except (AgentError, ValueError) as error:
                one["error_native"] = str(error)[:200]
        self.end(n, "shutdown")
        out["thrown_away"] = one
        self.save()

        # 2. A kept session: commit, then Windows by itself sees the change.
        def kept(tag: str, marker: str, big_mb: int = 0, **commit) -> dict:
            result: dict = {}
            g = self.start_guest(tag, mode="protected")
            result["attach"] = g.info.get("attach")
            if not g.machine:
                self.results["sessions"].append(g.info)
                return result
            if g.ready:
                try:
                    script = f"Set-Content C:\\dualrun\\marker.txt '{marker}'; Add-Content C:\\dualrun\\docs\\doc01.txt '{marker}'"
                    if big_mb:
                        script += (f"; $b = New-Object byte[] (8MB); (New-Object Random).NextBytes($b); "
                                   f"1..{big_mb // 8} | ForEach-Object {{ [IO.File]::WriteAllBytes(\"C:\\dualrun\\keep$($_).bin\", $b) }}")
                    g.machine.agent.ps(script + "; 'ok'", timeout=600)
                    result["written"] = True
                except AgentError as error:
                    result["error"] = str(error)[:200]
            self.end(g, "shutdown")
            result["layer_bytes"] = os.path.getsize(g.overlay) if os.path.exists(g.overlay) else None
            code, value = self.finish_layer(g, "commit", **commit)
            result["commit"] = {k: value.get(k) for k in ("committed", "resumed", "interrupted_for_test", "bytes_in_layer",
                                                           "commit_seconds", "seconds_before_kill", "error", "refusals")}
            result["commit"]["exit"] = code
            result["session"] = g
            return result

        def native_view(tag: str) -> dict:
            n = self.start_native(tag)
            view: dict = {"start_to_agent_s": n.info.get("start_to_agent_s"), "no_agent": n.info.get("no_agent")}
            if n.ready:
                try:
                    seen = n.machine.agent.json(gs.DOC_HASHES, timeout=120)
                    view.update(marker=seen.get("marker"), marker_text=seen.get("marker_text"), documents=seen.get("count"))
                    view["disk_check"] = n.machine.agent.json(gs.DISK_CHECK, timeout=900)
                    view["keep_files"] = n.machine.agent.ps(
                        "@(Get-ChildItem C:\\dualrun -Filter 'keep*.bin').Count", timeout=60).strip()
                except (AgentError, ValueError) as error:
                    view["error"] = str(error)[:200]
            self.end(n, "shutdown")
            return view

        two = kept("layer-keep", "kept-session-1")
        g = two.pop("session", None)
        if g is not None:
            self.close_layer_session(g)
        two["native"] = native_view("layer-keep-native")
        # The restore point of that commit is now stale: Windows ran by itself since.
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        code, value, _ = tool("undo", loop, "--state", str(self.store))
        sh("sudo", "losetup", "-d", loop, check=False)
        two["undo_after_a_native_start"] = {"exit": code, "refusals": [r["code"] for r in value.get("refusals", [])]}
        out["kept"] = two
        self.save()

        # 3. A kept session whose commit is interrupted, then run again.
        three = kept("layer-interrupted", "kept-session-2", big_mb=640, rate=96 * MIB, kill_after=3.0)
        g = three.pop("session", None)
        if g is not None:
            three["volume_after_interruption"], three["check_after_interruption"] = None, None
            code, value = self.finish_layer(g, "commit")
            three["second_run"] = {k: value.get(k) for k in ("committed", "resumed", "commit_seconds", "error")}
            three["second_run"]["exit"] = code
            # 4. Undo it straight away (the disk is as the commit left it), then look natively.
            code, value, err = tool("undo", g.loop, "--state", str(self.store))
            value.pop("after", None)
            three["undo"] = dict({k: value.get(k) for k in ("undone", "bytes_restored", "error")}, exit=code)
            self.close_layer_session(g)
        three["native_after_undo"] = native_view("layer-undo-native")
        out["interrupted_then_undone"] = three
        self.save()

        # 5. Windows' boot configuration damaged; restored from outside from TUR's copy of the person's file.
        five: dict = {}
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        backup = self.vm / "esp-copy"
        try:
            code, value, err = tool("esp-backup", loop, "--dest", str(backup))
            five["copy_taken"] = value if code == 0 else err[-200:]
            esp = f"{loop}p1"
            original = backup / "files" / "EFI" / "Microsoft" / "Boot" / "BCD"
            sh("sudo", "chmod", "-R", "a+rX", str(backup))
            if not original.exists():
                found = [p for p in (backup / "files").rglob("*") if p.name.upper() == "BCD"]
                original = found[0] if found else original
            garbage = self.vm / "garbage"
            garbage.write_bytes(b"this is not a boot configuration\n" * 300)
            sh("sudo", "env", "MTOOLS_SKIP_CHECK=1", "mcopy", "-o", "-i", esp, str(garbage), "::EFI/Microsoft/Boot/BCD")
            sh("sudo", "blockdev", "--flushbufs", esp, check=False)
        finally:
            sh("sudo", "losetup", "-d", loop, check=False)
        n = self.start_native("bcd-damaged", timeout=150)
        five["damaged_start"] = {"agent": n.ready, "screen_text": (n.info.get("no_agent") or {}).get("screen_text")}
        self.end(n, "kill" if not n.ready else "shutdown")
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        try:
            sh("sudo", "env", "MTOOLS_SKIP_CHECK=1", "mcopy", "-o", "-i", f"{loop}p1", str(original), "::EFI/Microsoft/Boot/BCD")
            sh("sudo", "blockdev", "--flushbufs", f"{loop}p1", check=False)
            five["restored_from_outside"] = True
        except RuntimeError as error:
            five["restored_from_outside"] = False
            five["error"] = str(error)[-200:]
        finally:
            sh("sudo", "losetup", "-d", loop, check=False)
            sh("sudo", "rm", "-rf", str(backup), check=False)
        n = self.start_native("bcd-restored")
        five["start_after_restore"] = {"agent": n.ready, "start_to_agent_s": n.info.get("start_to_agent_s")}
        if not n.ready:
            self.windows_ok = False
        self.end(n, "shutdown" if n.ready else "kill")
        out["boot_configuration_repaired_from_outside"] = five

    def scenario_files(self, out: dict) -> None:
        """The same files both ways: written by TUR while the guest is down, and through the guest."""
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        point = self.vm / "mnt"
        point.mkdir(exist_ok=True)
        text = f"written by TUR at {time.strftime('%H:%M:%S')} while Windows was down"
        try:
            code, decision, _ = tool("check", loop, "--guest-mb", "6144", "--reserve-mb", "1024")
            out["check_before_mount"] = {"exit": code, "notes": decision.get("decision", {}).get("notes")}
            driver = None
            for candidate in ("ntfs3", "ntfs-3g"):
                done = sh("sudo", "mount", "-t", candidate, "-o", "rw,windows_names" if candidate == "ntfs3" else "rw",
                          f"{loop}p3", str(point), check=False)
                if done.returncode == 0:
                    driver = candidate
                    break
                out.setdefault("mount_errors", {})[candidate] = done.stderr.strip()[-160:]
            out["driver"] = driver
            if driver:
                target = point / "Users" / "Public" / "Documents" / "from-tur.txt"
                sh("sudo", "sh", "-c", f"printf '%s' \"$1\" > \"$2\"", "sh", text, str(target))
                out["written_by_tur"] = True
                sh("sudo", "umount", str(point))
        finally:
            sh("sudo", "umount", str(point), check=False)
            sh("sudo", "losetup", "-d", loop, check=False)
        out["volume_after_tur_wrote"], out["check_after_tur_wrote"] = self.inspect()
        g = self.start_guest("files-guest", identity="carried")
        if g.machine and g.ready:
            try:
                seen = g.machine.agent.ps(r"Get-Content C:\Users\Public\Documents\from-tur.txt", timeout=60).strip()
                out["guest_sees_tur_file"] = seen == text
                out["windows_check_after_tur_wrote"] = g.machine.agent.json(gs.DISK_CHECK, timeout=900)
                g.machine.agent.ps(r"Set-Content C:\Users\Public\Documents\from-guest.txt 'written through the guest'; 'ok'",
                                   timeout=60)
            except (AgentError, ValueError) as error:
                out["error"] = str(error)[:200]
        if g.machine:
            self.end(g, "shutdown")
        else:
            self.results["sessions"].append(g.info)
        out["volume_after_guest"] = g.info.get("volume_after")
        loop = sh("sudo", "losetup", "--find", "--show", "--partscan", "--direct-io=on", str(self.disk)).stdout.strip()
        sh("sudo", "udevadm", "settle", check=False)
        try:
            for candidate in ("ntfs3", "ntfs-3g"):
                if sh("sudo", "mount", "-t", candidate, "-o", "ro", f"{loop}p3", str(point), check=False).returncode == 0:
                    target = point / "Users" / "Public" / "Documents" / "from-guest.txt"
                    done = sh("sudo", "cat", str(target), check=False)
                    out["tur_sees_guest_file"] = "written through the guest" in done.stdout
                    out["read_back_with"] = candidate
                    sh("sudo", "umount", str(point))
                    break
        finally:
            sh("sudo", "umount", str(point), check=False)
            sh("sudo", "losetup", "-d", loop, check=False)

    def scenario_integration(self, out: dict) -> None:
        """Memory given back, a USB device handed over, shutdown on close: with Windows' built-in drivers."""
        stick = self.vm / "stick.img"
        sh("truncate", "-s", "64M", str(stick))
        sh("mkfs.vfat", "-n", "TURSTICK", str(stick))
        note = self.vm / "hello.txt"
        note.write_text("a file on a USB device handed to the guest\n")
        sh("mcopy", "-i", str(stick), str(note), "::hello.txt")
        g = self.start_guest("integration", identity="carried")
        if not (g.machine and g.ready):
            out["error"] = g.info.get("no_agent") or g.info.get("attach")
            if g.machine:
                self.end(g, "kill")
            else:
                self.results["sessions"].append(g.info)
            return
        m = g.machine
        out["hyperv"], out["balloon_device"] = g.info.get("hyperv"), g.info.get("balloon_device")
        memory: dict = {}
        try:
            time.sleep(75)
            memory["rss_mb_idle"] = m.rss_mb()
            memory["guest_before"] = m.agent.json("Get-CimInstance Win32_OperatingSystem | Select-Object TotalVisibleMemorySize, FreePhysicalMemory | ConvertTo-Json -Compress", timeout=60)
            if g.info.get("balloon_device"):
                try:
                    memory["status_report"] = m.qmp.command("query-hv-balloon-status-report")
                except QmpError as error:
                    memory["status_report"] = f"error: {error}"
                target = 3 * GIB
                asked = None
                for attempt in range(12):
                    try:
                        m.qmp.command("balloon", value=target)
                        asked = attempt
                        break
                    except QmpError as error:
                        memory["balloon_error"] = str(error)
                        time.sleep(10)
                memory["balloon_accepted_after_attempts"] = asked
                if asked is not None:
                    time.sleep(60)
                    memory["rss_mb_after_balloon_to_3gib"] = m.rss_mb()
                    try:
                        memory["balloon_actual_mb"] = m.qmp.command("query-balloon").get("actual", 0) // MIB
                    except QmpError as error:
                        memory["balloon_actual_mb"] = f"error: {error}"
                    memory["guest_ballooned"] = m.agent.json("Get-CimInstance Win32_OperatingSystem | Select-Object TotalVisibleMemorySize, FreePhysicalMemory | ConvertTo-Json -Compress", timeout=60)
                    m.qmp.command("balloon", value=6144 * MIB)
                    time.sleep(30)
                    memory["rss_mb_after_giving_back"] = m.rss_mb()
                    memory["guest_restored"] = m.agent.json("Get-CimInstance Win32_OperatingSystem | Select-Object TotalVisibleMemorySize, FreePhysicalMemory | ConvertTo-Json -Compress", timeout=60)
        except (AgentError, QmpError, ValueError) as error:
            memory["error"] = str(error)[:300]
        out["memory"] = memory

        usb: dict = {}
        try:
            started = time.time()
            m.qmp.command("human-monitor-command", **{"command-line": f"drive_add 0 if=none,id=stick,format=raw,file={stick}"})
            m.qmp.command("device_add", driver="usb-storage", drive="stick", bus="xhci.0", removable=True, id="stickdev")
            script = ("$v = $null; 1..40 | ForEach-Object { if (-not $v) { $v = Get-Volume | Where-Object FileSystemLabel -eq 'TURSTICK'; "
                      "if (-not $v) { Start-Sleep -Milliseconds 500 } } }; "
                      "if ($v) { \"$($v.DriveLetter):\"; Get-Content \"$($v.DriveLetter):\\hello.txt\" } else { 'not seen' }")
            usb["guest_saw"] = m.agent.ps(script, timeout=120).strip()
            usb["seconds_to_appear"] = round(time.time() - started, 1)
            m.qmp.command("device_del", id="stickdev")
            time.sleep(5)
            usb["after_removal"] = m.agent.ps("@(Get-Volume | Where-Object FileSystemLabel -eq 'TURSTICK').Count", timeout=60).strip()
        except (AgentError, QmpError) as error:
            usb["error"] = str(error)[:300]
        out["usb_device"] = usb
        self.facts(g)
        self.end(g, "powerdown")
        out["close"] = {"how": "ACPI power button from the host", "seconds_to_power_off": g.info.get("end_seconds"),
                        "forced": g.info.get("forced_off"), "volume_after": g.info.get("volume_after")}

    def scenario_seamless(self, out: dict) -> None:
        import seamless
        seamless.run_agent_route(self, out)

    def scenario_remoteapp(self, out: dict) -> None:
        import seamless
        seamless.run_remoteapp(self, out)

    def scenario_controller(self, out: dict) -> None:
        """Installed on NVMe, started on AHCI; and Secure Boot enforced. Both on a layer that is thrown away."""
        for tag, kwargs in (("on-ahci", {"controller": "ahci"}),
                            ("secure-boot-enforced", {"vars_template": self.firmware.get("vars_enrolled")})):
            if tag == "secure-boot-enforced" and not self.firmware.get("vars_enrolled"):
                out[tag] = {"not_run": "no firmware variables with Microsoft's keys on this runner"}
                continue
            g = self.start_guest(tag, mode="protected", timeout=420, **kwargs)
            result: dict = {"attach": g.info.get("attach"), "start_to_agent_s": g.info.get("start_to_agent_s"),
                            "no_agent": g.info.get("no_agent"), "firmware_entry": g.info.get("firmware_entry")}
            if g.machine:
                if g.ready:
                    value = self.facts(g) or {}
                    result["secure_boot"] = value.get("secure_boot")
                    result["disks"] = value.get("disks")
                    result["boot_drivers"] = value.get("boot_drivers")
                self.end(g, "shutdown" if g.ready else "kill")
                self.finish_layer(g, "discard")
                self.close_layer_session(g)
            else:
                self.results["sessions"].append(g.info)
            out[tag] = result
            self.save()

    def scenario_savedstate(self, out: dict) -> None:
        """Can a running guest be saved to a file and resumed (the way to a near-instant start)?"""
        ahci_works = bool(self.results["scenarios"].get("controller", {}).get("on-ahci", {}).get("start_to_agent_s"))
        for controller in ("nvme", "ahci"):
            if controller == "ahci" and not ahci_works:
                out[controller] = {"not_run": "Windows did not start on AHCI in the controller scenario, or it was not run"}
                continue
            tag = f"saved-{controller}"
            g = self.start_guest(tag, mode="protected", controller=controller, tpm="none", balloon=False)
            result: dict = {"attach": g.info.get("attach"), "start_to_agent_s": g.info.get("start_to_agent_s")}
            out[controller] = result
            if not (g.machine and g.ready):
                if g.machine:
                    self.end(g, "kill")
                    self.finish_layer(g, "discard")
                    self.close_layer_session(g)
                else:
                    self.results["sessions"].append(g.info)
                continue
            m = g.machine
            state = self.vm / f"{tag}.state"
            try:
                m.agent.ps("Set-Content C:\\dualrun\\before-save.txt 'x'; 'ok'", timeout=60)
                time.sleep(20)
                m.qmp.command("stop")
                started = time.time()
                m.qmp.command("migrate", uri=f"exec:cat > {state}")
                status = {}
                while time.time() - started < 600:
                    status = m.qmp.command("query-migrate")
                    if status.get("status") in ("completed", "failed", "cancelled"):
                        break
                    time.sleep(1)
                result["save"] = {"status": status.get("status"), "error": status.get("error-desc"),
                                  "seconds": round(time.time() - started, 1),
                                  "bytes": state.stat().st_size if state.exists() else 0}
            except (QmpError, AgentError) as error:
                result["save"] = {"status": "refused", "error": str(error)[:300]}
            if result["save"].get("status") == "completed":
                m.kill()
                s2 = Session("guest", tag + "-resumed")
                s2.loop, s2.record, s2.overlay = g.loop, g.record, g.overlay
                s2 = self.start_guest(tag + "-resumed", mode="protected", controller=controller, tpm="none",
                                      balloon=False, incoming=f"exec:cat {state}", attached=s2,
                                      vars_template=None)
                resumed: dict = {"start_error": s2.info.get("start_error")}
                if s2.machine and s2.machine.alive():
                    started = time.time()
                    answer = None
                    while time.time() - started < 180 and s2.machine.alive():
                        try:
                            status = s2.machine.qmp.command("query-status")
                            if status.get("status") == "paused" and time.time() - started > 3:
                                s2.machine.qmp.command("cont")
                            answer = s2.machine.agent.ps("Test-Path C:\\dualrun\\before-save.txt", timeout=10).strip()
                            break
                        except (AgentError, QmpError):
                            time.sleep(1)
                    resumed["agent_answered"] = answer
                    resumed["seconds_from_start_to_answer"] = round(time.time() - s2.machine.started, 1) if answer else None
                    self.end(s2, "shutdown" if answer else "kill")
                result["resume"] = resumed
                g.machine = None
                state.unlink(missing_ok=True)
                self.finish_layer(s2, "discard")
                self.close_layer_session(s2)
            else:
                try:
                    m.qmp.command("cont")
                except QmpError:
                    pass
                self.end(g, "shutdown")
                state.unlink(missing_ok=True)
                self.finish_layer(g, "discard")
                self.close_layer_session(g)
            self.save()

    def scenario_final(self, out: dict) -> None:
        """Windows by itself, at the end: its own disk check, its boot timings, what changed on the boot partition."""
        n = self.start_native("native-final")
        out["start_to_agent_s"] = n.info.get("start_to_agent_s")
        if n.ready:
            try:
                time.sleep(30)
                value = self.facts(n) or {}
                out["boot_drivers"] = value.get("boot_drivers")
                out["disk_check"] = n.machine.agent.json(gs.DISK_CHECK, timeout=900)
                out["chkdsk"] = n.machine.agent.json(gs.CHKDSK, timeout=1500)
                out["windows_boot_records"] = n.machine.agent.json(gs.BOOT_TIMES, timeout=120)
                out["documents"] = n.machine.agent.json(gs.DOC_HASHES, timeout=120).get("count")
            except (AgentError, ValueError) as error:
                out["error"] = str(error)[:300]
        else:
            out["no_agent"] = n.info.get("no_agent")
        self.end(n, "shutdown" if n.ready else "kill")
        before = self.results["scenarios"].get("roundtrips", {}).get("esp_before") or {}
        after = self.esp_manifest("after")
        out["boot_partition_files"] = {
            "before": len(before), "after": len(after),
            "added": sorted(set(after) - set(before)), "removed": sorted(set(before) - set(after)),
            "changed": sorted(k for k in before if k in after and before[k]["sha256"] != after[k]["sha256"]),
        }
        baseline = self.results["scenarios"].get("roundtrips", {}).get("baseline_boot_drivers")
        if baseline is not None and out.get("boot_drivers") is not None:
            out["boot_drivers_same_as_before_any_guest_session"] = baseline == out["boot_drivers"]
            out["boot_drivers_added"] = sorted(set(out["boot_drivers"]) - set(baseline))
            out["boot_drivers_removed"] = sorted(set(baseline) - set(out["boot_drivers"]))
        out["pc_firmware_entries"] = self.firmware_entries(self.pc_vars)
        out["tur_partition_unchanged"] = self.tur_unchanged()
        out["disk_allocated_gib"] = round(os.stat(self.disk).st_blocks * 512 / GIB, 1)

    # -- driver -------------------------------------------------------------

    def cleanup(self) -> None:
        """After a scenario, whatever happened: no QEMU, no view, no loop device, no stale layer."""
        subprocess.run(["pkill", "-9", "-f", "qemu-system-x86_64"], check=False)
        subprocess.run(["pkill", "-9", "swtpm"], check=False)
        time.sleep(1)
        subprocess.run(["sudo", "umount", str(self.vm / "mnt")], check=False, capture_output=True)
        subprocess.run(["sudo", "dmsetup", "remove_all", "--force"], check=False, capture_output=True)
        subprocess.run(["sudo", "losetup", "-D"], check=False, capture_output=True)
        for stale in self.store.glob("*/session.json"):
            subprocess.run(["sudo", "rm", "-f", str(stale), str(stale.parent / "shadow.img")], check=False)
        for stale in self.store.glob("*/layer.json"):
            subprocess.run(["sudo", "rm", "-f", str(stale)], check=False)

    def run(self, wanted: list[str]) -> None:
        self.host_facts()
        try:
            installed = self.install()
        except Exception as error:  # noqa: BLE001
            installed = False
            self.results["scenarios"].setdefault("install", {})["error"] = f"{type(error).__name__}: {error}"[:600]
            self.log(traceback.format_exc())
        self.cleanup()
        self.save()
        for name in ORDER:
            if name not in wanted:
                self.results["scenarios"][name] = {"not_run": "not selected"}
                continue
            if not installed:
                self.results["scenarios"][name] = {"not_run": "the installation did not finish"}
                continue
            if not self.windows_ok and name != "final":
                self.results["scenarios"][name] = {"not_run": "Windows no longer starts; see the earlier scenarios"}
                continue
            if self.minutes_left() < ESTIMATE_MINUTES[name]:
                self.results["scenarios"][name] = {"not_run": f"time budget: {self.minutes_left():.0f} minutes left, "
                                                              f"about {ESTIMATE_MINUTES[name]} needed"}
                continue
            out: dict = {}
            self.results["scenarios"][name] = out
            started = time.time()
            self.log(f"=== scenario {name} ({self.minutes_left():.0f} minutes left)")
            try:
                getattr(self, f"scenario_{name}")(out)
            except Exception as error:  # noqa: BLE001
                out["harness_error"] = f"{type(error).__name__}: {error}"[:600]
                out["harness_trace"] = traceback.format_exc().strip().splitlines()[-6:]
                self.log(traceback.format_exc())
            out["minutes"] = round((time.time() - started) / 60, 1)
            self.cleanup()
            self.save()
        self.results["meta"]["finished_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.save()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--work", required=True)
    parser.add_argument("--iso", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--scenarios", default="")
    parser.add_argument("--round-trips", type=int, default=5)
    parser.add_argument("--budget-minutes", type=float, default=315)
    args = parser.parse_args()
    wanted = [name.strip() for name in args.scenarios.split(",") if name.strip()] or ORDER
    experiment = Experiment(args)
    try:
        experiment.run(wanted)
    finally:
        experiment.cleanup()
        try:
            import summary
            summary.write(experiment.out)
        except Exception:  # noqa: BLE001
            traceback.print_exc()
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Run boot-health scenarios: break a second deployment on purpose and prove
what the machine does about it (TUR-005; design section 11).

    tests/boot/rollback.py --disk disk.raw --images images.img --work WORK \
        --out OUT_DIR SCENARIO [SCENARIO ...]
    tests/boot/rollback.py --list

Each scenario starts from a copy-on-write overlay of the prepared disk: the
disk the boot test leaves, with a user and first-run setup done. The scenario's
name reaches the guest through QEMU's fw_cfg. The guest's scenario agent stages
the scenario's image beside the running system with
`bootc switch --transport oci` from a read-only disk and restarts. From then on
the host only watches, except where a scenario says it cuts the power (QEMU is
killed) or stands in for a person holding the power button.

Evidence, in order of weight:

1. the kernel command line of every boot, read from the serial console. The
   kernel prints it before anything can fail, so the sequence of `tur.boot`
   values exists even for a boot that never reaches userspace;
2. console text that shows *why* a boot failed (a panic, the initramfs giving
   up, the product's own lines), and QMP events (RESET, WATCHDOG);
3. reports from the guest agent in every boot that reaches it: the booted
   deployment, GRUB's environment block, what tur-boot-health recorded, and
   the journals of the boots before it;
4. the disk, read from the host after the machine has stopped.

A scenario passes only if it failed for the reason it intends. It writes
OUT_DIR/<scenario>/scenario-report.json and summary.md, and the run writes
OUT_DIR/rollback-report.json. Exit status 0 only if every required check of
every scenario passed.

"Power cut" here means the QEMU process is killed. The disk is a qcow2 overlay
opened with cache=writeback: what the guest had flushed is on the disk, what
it had only written may be lost. That is closer to a real power cut than a
raw image, but it is still not a real drive with a write cache, and it is
never reported as one.

Run in CI only (ADR-0002): never on a shared or production machine.
"""

from __future__ import annotations

import argparse
import gzip
import io
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import vm  # noqa: E402

MARK = vm.MARK
MSG_FALLBACK = "8f1c2a6e4b7d4c0f9a3e5d6b7c8a9e03"
BOOTC_ROLLBACK = "26f3b1eb24464d12aa5e7b544a6b5468"
TRIAL_ARGUMENTS = ["panic=30", "rd.shell=0", "rd.emergency=reboot", "systemd.watchdog_sec=180"]
FAILING = ["trial/2", "trial/1", "trial/0", "fallback"]

# Console texts counted per boot. They show why a boot failed.
PATTERNS = [
    ("kernel panic", r"Kernel panic - not syncing"),
    ("initramfs unpacking failed", r"Initramfs unpacking failed"),
    ("root mount failed", r"Failed to mount sysroot\.mount|sysroot\.mount: Mount process exited|wrong fs type, bad option|Unknown parameter"),
    ("initramfs emergency", r"dracut-emergency|Entering emergency mode|emergency\.target|Refusing to continue"),
    ("emergency mode", r"[Ee]mergency [Mm]ode|emergency\.target"),
    ("pid1 frozen", r"Freezing execution"),
    ("grub error", r"^error: |you need to load the kernel first|invalid magic number|kernel too small"),
    ("gate failed", r"tur-boot-health: gate: Boot health gate failed"),
    ("gate passed", r"tur-boot-health: gate: Boot health gate (passed|degraded)"),
    ("marked good", r"tur-boot-health: mark-good: counter removed"),
    ("armed", r"tur-boot-health: armed: boot_counter="),
    ("fallback step 3", r"tur-boot-health: fallback: step 3 done"),
    ("fallback step 4", r"tur-boot-health: fallback: step 4 done"),
    ("fallback done", r"tur-boot-health: fallback: step 7 done"),
    ("stale", r"tur-boot-health: classify: .*-> stale"),
    ("rejected restart", r"tur-boot-health: classify: this version was rejected"),
    ("finalize stopping", r"Stopping ostree-finalize-staged"),
    ("retry unit", r"tur-boot-retry\.service|tur-boot-emergency-retry\.service"),
    ("watchdog", r"watchdog: watchdog0|Using hardware watchdog|iTCO_wdt"),
]


def scenario(**fields) -> dict:
    base = {
        "image": None,
        "plan": {},
        "expect": None,
        "final": "previous",
        "outcome": "fallback",
        "reason": "attempts",
        "reached": 3,
        "cause": [],
        "failed_check": None,
        "kills": [],
        "hold_power_button": False,
        "timeout": 1500,
        "journals": 3,
        "manual": False,
    }
    base.update(fields)
    return base


# The scenarios of design section 11.4. `expect` is the kind of every boot
# after the one that staged the update. `cause` lists the positive markers of
# the intended failure: (what, pattern name, boots it must be seen in).
SCENARIOS = {
    "good": scenario(
        id="G1",
        title="A good update is kept and marked good",
        image="good",
        plan={"extra_reboots": "1"},
        expect=[["trial/2", "none"]],
        final="new",
        outcome="kept",
        cause=[("the gate passed in the trial boot", "gate passed", [1]), ("the boot was marked good", "marked good", [1])],
        timeout=900,
    ),
    "check-fails": scenario(
        id="F1 and L1",
        title="A required health check fails; then two more restarts",
        image="check-fails",
        plan={"extra_reboots": "2"},
        expect=[FAILING + ["none", "none"]],
        cause=[("the gate failed in every attempt", "gate failed", [1, 2, 3])],
        failed_check=r"X:always-fails \(TUR-BOOT-HEALTH-021\)",
        timeout=1200,
    ),
    "unit-fails": scenario(
        id="F2",
        title="A required unit fails",
        image="unit-fails",
        expect=[FAILING],
        cause=[("the gate failed in every attempt", "gate failed", [1, 2, 3])],
        failed_check=r"R5 \(TUR-BOOT-HEALTH-005\): tur-test-required\.service is failed",
        timeout=1200,
    ),
    "dm-crash": scenario(
        id="F3",
        title="The display manager crash-loops",
        image="dm-crash",
        expect=[FAILING],
        cause=[("the gate failed in every attempt", "gate failed", [1, 2, 3])],
        failed_check=r"R[678] \(TUR-BOOT-HEALTH-0(06|07|08|20)\)",
        timeout=1800,
    ),
    "root-unmountable": scenario(
        id="F4",
        title="The kernel command line makes the root filesystem unmountable",
        image="root-unmountable",
        expect=[FAILING],
        reached=0,
        journals=0,
        cause=[("the initramfs could not mount root in every attempt", "root mount failed", [1, 2, 3])],
        timeout=1500,
    ),
    "panic": scenario(
        id="F5",
        title="Kernel panic in early boot",
        image="panic",
        expect=[FAILING],
        reached=0,
        journals=0,
        cause=[("the kernel panicked in every attempt", "kernel panic", [1, 2, 3])],
        panic_restart=True,
        timeout=900,
    ),
    "panic-counter-only": scenario(
        id="step 4",
        title="Counting and fallback with no TUR program: armed by hand, TUR's units masked, kernel panic",
        image="panic",
        plan={"arm": "hand"},
        expect=[FAILING],
        outcome="counter-only",
        reached=0,
        journals=0,
        cause=[("the kernel panicked in every attempt", "kernel panic", [1, 2, 3])],
        panic_restart=True,
        timeout=900,
    ),
    "kernel-truncated": scenario(
        id="F6",
        title="GRUB cannot start the kernel",
        image="kernel-truncated",
        plan={"extra_reboots": "1"},
        expect=[["trial/2", "none"]],
        reason="loader",
        reached=0,
        journals=0,
        cause=[("GRUB reported an error before the kernel of the trial boot started", "grub error", [0])],
        timeout=900,
    ),
    "emergency": scenario(
        id="F7",
        title="Emergency mode in the real root",
        image="emergency",
        expect=[FAILING],
        reached=0,
        cause=[("systemd entered emergency mode in every attempt", "emergency mode", [1, 2, 3])],
        timeout=1500,
    ),
    "pid1-stops": scenario(
        id="H1",
        title="PID 1 stops; the hardware watchdog resets the machine",
        image="pid1-stops",
        expect=[FAILING],
        reached=3,
        cause=[("PID 1 froze in every attempt", "pid1 frozen", [1, 2, 3])],
        watchdog_resets=3,
        gate_failed=False,
        timeout=1800,
    ),
    "stall": scenario(
        id="H2",
        title="The boot stalls while PID 1 is alive (gate limit shortened to 40 s in the scenario image)",
        image="stall",
        expect=[FAILING],
        cause=[("the gate failed in every attempt", "gate failed", [1, 2, 3])],
        failed_check=r"R3 \(TUR-BOOT-HEALTH-020\): not within 40 s",
        timeout=1200,
    ),
    "silent-hang": scenario(
        id="H3",
        title="A hang nothing can see: the harness holds the power button after each kernel start",
        image="good",
        expect=[FAILING],
        reached=0,
        journals=0,
        hold_power_button=True,
        manual=True,
        timeout=900,
    ),
    "cut-staging": scenario(
        id="P1",
        title="Power cut while the update is being staged",
        image="good",
        plan={"extra_reboots": "1"},
        expect=[["trial/2", "none"]],
        final="new",
        outcome="kept",
        kills=[{"after": "SCENARIO-SWITCHING", "delay": 2.5, "what": "during bootc switch"}],
        stage_boots=2,
        timeout=1200,
    ),
    "cut-mark-good": scenario(
        id="P3",
        title="Power cut while the boot is being marked good",
        image="good",
        plan={"extra_reboots": "1"},
        expect=[["trial/2", "none", "none"], ["trial/2", "trial/1", "none"], ["trial/2", "none"]],
        final="new",
        outcome="kept",
        kills=[{"after": r"tur-boot-health: mark-good: counter removed", "delay": 0.0, "what": "as mark-good removed the counter"}],
        timeout=1200,
    ),
    "cut-fallback-step3": scenario(
        id="P4a",
        title="Power cut during the fallback, after the counter was made positive",
        image="unit-fails",
        expect=None,
        converge=True,
        kills=[{"after": r"tur-boot-health: fallback: step 3 done", "delay": 0.0, "what": "after step 3 of the fallback"}],
        failed_check=r"R5 \(TUR-BOOT-HEALTH-005\)",
        timeout=1800,
    ),
    "cut-fallback-step4": scenario(
        id="P4b",
        title="Power cut during the fallback, after the boot order was changed",
        image="unit-fails",
        expect=None,
        converge=True,
        kills=[{"after": r"tur-boot-health: fallback: step 4 done", "delay": 0.0, "what": "after step 4 of the fallback"}],
        failed_check=r"R5 \(TUR-BOOT-HEALTH-005\)",
        timeout=1800,
    ),
    "finalize-fails": scenario(
        id="S1",
        title="Finalisation fails: the update is not applied, and nothing is rolled back",
        image="good",
        plan={"finalize": "fail", "extra_reboots": "1"},
        expect=[["trial/2", "none"]],
        outcome="stale",
        reached=0,
        cause=[("the previous version classified the boot as stale", "stale", [1])],
        timeout=900,
    ),
}
# P2: the power is cut at several moments after systemd announces the stop of
# OSTree's finalisation. Either the old or the new version must come up whole.
for _ms in (0, 200, 500, 1000, 2000):
    SCENARIOS[f"cut-finalize-{_ms}"] = scenario(
        id=f"P2 at {_ms} ms",
        title=f"Power cut {_ms} ms after the finalisation of the update began",
        image="good",
        plan={"extra_reboots": "1"},
        expect=[["trial/2", "none"]],
        final="either",
        outcome="either",
        kills=[{"after": r"Stopping ostree-finalize-staged", "delay": _ms / 1000.0, "what": f"{_ms} ms into finalisation"}],
        timeout=1200,
    )


def props(text: str) -> dict[str, str]:
    found = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        if sep:
            found[key.strip()] = value.strip()
    return found


def load_json(path: pathlib.Path):
    try:
        return json.loads(path.read_bytes().decode("utf-8", errors="replace"))
    except (OSError, ValueError):
        return None


def json_lines(path: pathlib.Path) -> list[dict]:
    found = []
    try:
        text = path.read_bytes().decode("utf-8", errors="replace")
    except OSError:
        return found
    for line in text.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict):
            found.append(value)
    return found


class Report:
    """What the guest agent recorded in one boot."""

    def __init__(self, directory: pathlib.Path):
        self.dir = directory
        self.name = directory.name
        self.status = props(self.text("status"))

    def text(self, name: str) -> str:
        try:
            return (self.dir / name).read_bytes().decode("utf-8", errors="replace")
        except OSError:
            return ""

    def json(self, name: str):
        return load_json(self.dir / name)

    def env(self) -> dict[str, str]:
        return props(self.text("grubenv.txt"))

    def bootc(self, name: str = "bootc-status.json") -> dict:
        return ((self.json(name) or {}).get("status")) or {}

    def history(self) -> list[dict]:
        return json_lines(self.dir / "boot-health" / "history.jsonl")


class Checks:
    def __init__(self, prefix: str):
        self.prefix = prefix
        self.items: list[dict] = []

    def add(self, ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        result = "not-run" if passed is None else "pass" if passed else "fail"
        self.items.append(
            {
                "id": f"{self.prefix}.{ident}",
                "description": description,
                "required": required,
                "result": result,
                "detail": detail,
            }
        )

    def passed(self) -> bool:
        return all(item["result"] == "pass" for item in self.items if item["required"])


def run(*command: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(command, capture_output=True, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} failed ({result.returncode}): {result.stderr.strip()}")
    return result


def drive(name: str, spec: dict, args, out: pathlib.Path, work: pathlib.Path) -> dict:
    """Run the virtual machine through one scenario and return what was seen."""
    overlay = work / f"{name}.qcow2"
    overlay.unlink(missing_ok=True)
    run("qemu-img", "create", "-q", "-f", "qcow2", "-b", str(pathlib.Path(args.disk).resolve()), "-F", "raw", str(overlay))
    serial_log, report_tar = out / "serial.log", work / f"{name}-report.tar"
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    fw_cfg = {"scenario": name, "image": spec["image"]}
    fw_cfg.update(spec["plan"])
    machine = vm.Machine(
        disk=str(overlay),
        disk_format="qcow2",
        # Flushes reach the file; a kill loses what was not flushed.
        disk_cache="writeback",
        work=work,
        serial_log=serial_log,
        report_tar=report_tar,
        qemu_log=host / "qemu.log",
        name=f"tur-{name}",
        cpus=args.cpus,
        memory=args.memory,
        qemu=args.qemu,
        images_disk=str(pathlib.Path(args.images).resolve()),
        fw_cfg=fw_cfg,
        watchdog=True,
        pvpanic=True,
        variables_from=str(work / "OVMF_VARS.prepared.fd"),
    )
    for stale in (serial_log, report_tar, machine.variables, host / "qemu.log"):
        stale.unlink(missing_ok=True)
    started = time.monotonic()

    def elapsed() -> float:
        return round(time.monotonic() - started, 1)

    print(f"===== scenario {name}: {spec['title']}")
    print(f"$ {' '.join(machine.command())}", flush=True)
    watcher = vm.SerialWatcher(serial_log, elapsed, patterns=PATTERNS)
    actions: list[dict] = []
    kills = [dict(rule, pattern=re.compile(rule["after"]), due=None, done=False) for rule in spec["kills"]]
    held: set[int] = set()
    outcome = "timeout"
    shots = 0
    try:
        qmp = machine.start()
        kvm = qmp.command("query-kvm")
        if not kvm.get("enabled"):
            machine.quit()
            raise RuntimeError("KVM is not enabled")
        deadline = started + spec["timeout"]
        while time.monotonic() < deadline:
            if not machine.alive():
                outcome = "qemu-exited"
                break
            time.sleep(0.1)
            lines = watcher.pump()
            machine.collect()
            # ---- power cuts ----------------------------------------------------
            for rule in kills:
                if rule["done"]:
                    continue
                if rule["due"] is None and any(rule["pattern"].search(line) for line in lines):
                    rule["due"] = time.monotonic() + rule["delay"]
                    print(f"[{elapsed():7.1f}s] power cut armed: {rule['what']}, in {rule['delay']} s", flush=True)
                if rule["due"] is not None and time.monotonic() >= rule["due"]:
                    machine.kill()
                    rule["done"] = True
                    actions.append({"action": "power-cut", "what": rule["what"], "at_seconds": elapsed(), "boot": watcher.boot})
                    print(f"[{elapsed():7.1f}s] POWER CUT ({rule['what']}): QEMU killed in boot {watcher.boot}", flush=True)
                    time.sleep(1)
                    watcher.pump()
                    qmp = machine.start()
                    actions.append({"action": "power-on", "at_seconds": elapsed()})
                    print(f"[{elapsed():7.1f}s] power restored: QEMU started again", flush=True)
                break  # one rule at a time, in order
            # ---- a person holding the power button --------------------------------
            current = watcher.current()
            if (
                spec["hold_power_button"]
                and current is not None
                and current["tur"]
                and current["tur"].get("boot") == "trial"
                and current["boot"] not in held
            ):
                held.add(current["boot"])
                qmp.command("stop")
                time.sleep(3)
                qmp.command("system_reset")
                qmp.command("cont")
                actions.append({"action": "forced-reset", "at_seconds": elapsed(), "boot": current["boot"]})
                print(f"[{elapsed():7.1f}s] the harness froze boot {current['boot']} and reset the machine", flush=True)
            # ---- a look at the screen of each boot that reports ------------------------
            reports = sum(1 for line in watcher.agent_lines if "SCENARIO-REPORT" in line["text"])
            if reports > shots and machine.alive():
                shots = reports
                try:
                    vm.screenshot(qmp, work, out / f"screenshot-report-{reports}.png")
                except Exception as error:  # diagnosis only
                    print(f"no screenshot: {error}")
            if any("SCENARIO-COMPLETE" in line["text"] or "SCENARIO-IDLE" in line["text"] for line in watcher.agent_lines):
                outcome = "complete"
                break
        if outcome != "complete" and machine.alive():
            try:
                vm.screenshot(qmp, work, out / "screenshot-at-end.png")
            except Exception as error:  # diagnosis only
                print(f"no screenshot at the end: {error}")
    except Exception as error:
        outcome = f"error: {error}"
        print(f"[{elapsed():7.1f}s] the driver failed: {error}", flush=True)
    finished = elapsed()
    shutdown = "not-running"
    if machine.alive():
        shutdown = "clean" if (outcome == "complete" and machine.powerdown(timeout=120)) else "forced"
        machine.quit()
    watcher.pump()
    machine.collect()
    for leftover in out.glob("*.ppm.tmp"):
        leftover.unlink()
    events = [
        dict({"event": event["event"], "at_seconds": round(event["host_time"] - started, 1)}, **({"data": event["data"]} if "data" in event else {}))
        for event in machine.events
    ]
    print(f"[{elapsed():7.1f}s] scenario {name}: {outcome}; boots seen: {', '.join(watcher.sequence())}", flush=True)
    if outcome != "complete":
        text = vm.clean(serial_log.read_bytes().decode("utf-8", errors="replace")) if serial_log.exists() else ""
        tail = [line for line in text.split("\n") if line.strip()][-120:]
        print(f"----- last {len(tail)} lines of the serial console -----")
        print("\n".join(tail))
        print("----- end of serial console -----", flush=True)
    return {
        "outcome": outcome,
        "elapsed_seconds": finished,
        "shutdown": shutdown,
        "boots": watcher.boots,
        "sequence": watcher.sequence(),
        "hits": watcher.hits,
        "agent_lines": watcher.agent_lines,
        "events": events,
        "actions": actions,
        "qemu": machine.version,
        "starts": machine.starts,
        "overlay": str(overlay),
        "report_tar": str(report_tar),
    }


def unpack(report_tar: pathlib.Path, target: pathlib.Path) -> str:
    """The guest's reports, one tar archive per report on one channel."""
    if not report_tar.exists() or report_tar.stat().st_size == 0:
        return "none"
    target.mkdir(parents=True, exist_ok=True)
    try:
        with tarfile.open(fileobj=io.BytesIO(report_tar.read_bytes()), mode="r:", ignore_zeros=True) as archive:
            archive.extractall(target, filter="data")
        return "virtio-serial"
    except (tarfile.TarError, OSError, EOFError) as error:
        return f"virtio-serial (incomplete: {error})"


def postmortem(overlay: pathlib.Path, out: pathlib.Path, work: pathlib.Path) -> None:
    """Read the scenario's disk from the host, whatever happened."""
    raw = work / "postmortem.raw"
    try:
        run("qemu-img", "convert", "-O", "raw", str(overlay), str(raw))
        script = str(pathlib.Path(__file__).resolve().parent / "inspect-disk.py")
        result = run("sudo", sys.executable, script, "postmortem", str(raw), str(out / "disk"), check=False)
        (out / "host" / "postmortem.log").write_text(result.stdout + result.stderr)
        # The text rendering is kept; the binary journal would be most of the artifact.
        run("sudo", "rm", "-rf", str(out / "disk" / "journal-disk"), check=False)
    except Exception as error:  # best effort by design
        print(f"postmortem failed: {error}")
    finally:
        raw.unlink(missing_ok=True)


def in_boot(seen: dict, name: str, boot: int) -> int:
    return sum(1 for hit in seen["hits"] if hit["name"] == name and hit["boot"] == boot)


def evaluate(name: str, spec: dict, seen: dict, out: pathlib.Path, guest_source: str) -> dict:
    """Turn the evidence of one scenario into checks (design section 11.5)."""
    checks = Checks(name)
    reports = {path.name: Report(path) for path in sorted((out / "guest").glob("*")) if path.is_dir()}
    stages = [report for key, report in sorted(reports.items()) if key.startswith("stage-")]
    boots = sorted(
        (report for key, report in reports.items() if key.startswith("boot-")),
        key=lambda report: int(report.name.split("-")[1]),
    )
    stage = stages[-1] if stages else None
    last = boots[-1] if boots else None
    console = seen["boots"]
    # The boots that staged the update come before the first trial boot. If
    # no boot was ever a trial, the agent's own count of its stage attempts.
    first_trial_index = next(
        (index for index, boot in enumerate(console) if boot["tur"] and boot["tur"].get("boot") == "trial"), None
    )
    if first_trial_index is not None:
        stage_boots = first_trial_index
    else:
        stage_boots = int(stage.name.split("-")[1]) if stage else 1
    after = console[stage_boots:]
    sequence = [vm.label(boot["tur"]) if boot["tur"] else "unknown" for boot in after]

    checks.add(
        "completed",
        "The scenario ran to its end within its time bound",
        seen["outcome"] == "complete",
        f"{seen['outcome']} after {seen['elapsed_seconds']} s (bound {spec['timeout']} s); QEMU was started "
        f"{seen['starts']} time(s); guest reports: {', '.join(reports) or 'none'} ({guest_source})",
    )

    # ---- staging ---------------------------------------------------------------
    staged = stage.bootc("bootc-status-staged.json") if stage else {}
    previous = (staged.get("booted") or {}).get("ostree", {}).get("checksum")
    new = (staged.get("staged") or {}).get("ostree", {}).get("checksum")
    new_digest = ((staged.get("staged") or {}).get("image") or {}).get("imageDigest")
    new_reference = (((staged.get("staged") or {}).get("image") or {}).get("image") or {})
    milliseconds = stage.status.get("switch_milliseconds") if stage else None
    checks.add(
        "staged",
        "The guest staged the scenario's image beside the running system with bootc switch --transport oci",
        (stage.status.get("switch_exit_status") == "0" and bool(new) and new != previous) if stage else None,
        f"exit status {stage.status.get('switch_exit_status') if stage else 'no data'} in {milliseconds} ms; staged "
        f"{new_reference.get('transport')}:{new_reference.get('image')} {new_digest}; running {str(previous)[:12]}, "
        f"staged {str(new)[:12]}; armed by: {stage.status.get('armed_by') if stage else 'no data'}",
    )

    # ---- the sequence of boots, from the console --------------------------------------
    expected = spec["expect"]
    if expected is not None:
        checks.add(
            "sequence",
            "The kind of every boot after staging, read from the kernel command lines on the serial console, "
            "is exactly the expected one: no attempt uncounted, none counted twice",
            sequence in expected,
            f"seen: {sequence}; expected: {' or '.join(str(option) for option in expected)}",
        )
    else:
        head = sequence[:4]
        checks.add(
            "sequence",
            "Three counted attempts and a fallback, then only boots that the interrupted fallback explains, "
            "ending on an ordinary boot",
            head == FAILING and len(sequence) <= 10 and bool(sequence) and sequence[-1] in ("none", "trial/1", "trial/0", "fallback"),
            f"seen: {sequence}",
        )
    trials = [boot for boot in after if boot["tur"] and boot["tur"].get("boot") == "trial"]
    checks.add(
        "trial_arguments",
        "Every trial boot carried the trial arguments from GRUB's environment block, and no other boot did",
        (
            bool(trials)
            and all(boot["tur"]["trial_arguments"] == TRIAL_ARGUMENTS for boot in trials)
            and all(not boot["tur"]["trial_arguments"] for boot in console if boot["tur"] and boot["tur"].get("boot") != "trial")
            and all(boot["tur"]["contract"] == "1" and not boot["tur"]["has_dollar"] for boot in console if boot["tur"])
        )
        if console
        else None,
        f"trial boots: {[boot['tur']['trial_arguments'] for boot in trials]}",
    )

    # ---- the intended cause -------------------------------------------------------------
    first_trial = stage_boots + 1  # console boots are numbered from 1
    for what, pattern, where in spec["cause"]:
        counts = {index: in_boot(seen, pattern, first_trial + index - 1) for index in where}
        lines = [hit["line"] for hit in seen["hits"] if hit["name"] == pattern][:3]
        checks.add(
            f"cause.{pattern.replace(' ', '_')}",
            f"The intended failure happened: {what}",
            all(count > 0 for count in counts.values()),
            f"console lines matching '{pattern}' per boot after staging: {counts}; for example: {lines}",
        )
    harness = [action for action in seen["actions"] if action["action"] in ("power-cut", "forced-reset")]
    if spec.get("stage_boots"):
        # P1: the cut must have come while the update was being staged, and
        # the machine must have come back healthy with nothing staged.
        before = stage.bootc() if stage else {}
        checks.add(
            "cut_while_staging",
            "The power cut came during staging: the machine came back on the previous version, healthy, with "
            "nothing staged, and a second bootc switch succeeded",
            stage_boots == spec["stage_boots"]
            and stage is not None
            and stage.name == f"stage-{spec['stage_boots']}"
            and before.get("staged") is None
            and stage.status.get("classification") == "normal"
            and stage.status.get("verdict") in ("passed", "degraded")
            and stage.status.get("switch_exit_status") == "0",
            f"boots before the first trial: {stage_boots}; last stage report: {stage.name if stage else 'none'}; staged "
            f"before the second switch: {before.get('staged')}; classification "
            f"{stage.status.get('classification') if stage else None}, verdict {stage.status.get('verdict') if stage else None}",
        )
    if spec["kills"]:
        checks.add(
            "power_cut",
            "The power was cut at the intended moment: " + "; ".join(rule["what"] for rule in spec["kills"]),
            len([a for a in harness if a["action"] == "power-cut"]) == len(spec["kills"]),
            f"harness actions: {seen['actions']}",
        )
    elif spec["hold_power_button"]:
        checks.add(
            "forced_restarts",
            "The harness, standing in for a person, froze and reset each of the three trial boots",
            len(harness) == 3,
            f"harness actions: {seen['actions']}",
        )
    else:
        checks.add(
            "no_harness_restart",
            "The harness restarted nothing: every restart was the machine's own",
            not harness and seen["starts"] == 1,
            f"harness actions: {seen['actions'] or 'none'}; QEMU starts: {seen['starts']}",
        )
    resets = [event for event in seen["events"] if event["event"] == "RESET"]
    watchdogs = [event for event in seen["events"] if event["event"] == "WATCHDOG"]
    if spec.get("watchdog_resets"):
        checks.add(
            "watchdog",
            "Each attempt was ended by the hardware watchdog: QEMU reported a WATCHDOG event with action reset",
            len(watchdogs) == spec["watchdog_resets"] and all((event.get("data") or {}).get("action") == "reset" for event in watchdogs),
            f"WATCHDOG events: {watchdogs}",
        )
    else:
        checks.add(
            "no_watchdog",
            "The hardware watchdog did not fire",
            not watchdogs,
            f"WATCHDOG events: {watchdogs or 'none'}",
            required=False,
        )
    if spec.get("panic_restart"):
        # Each panic must be followed by a restart about 30 s later (panic=30).
        delays = []
        for index in (1, 2, 3):
            number = first_trial + index - 1
            panic = next((hit["at"] for hit in seen["hits"] if hit["name"] == "kernel panic" and hit["boot"] == number), None)
            following = next((boot["kernel_started_at"] for boot in console if boot["boot"] == number + 1), None)
            delays.append(round(following - panic, 1) if panic is not None and following is not None else None)
        checks.add(
            "panic_timeout",
            "Each panic was followed by a restart the harness did not cause, after the 30 s of panic=30",
            all(delay is not None and 29 <= delay <= 75 for delay in delays),
            f"seconds from the panic line to the next kernel start: {delays}; RESET events: {len(resets)}",
        )
    if spec["failed_check"]:
        pattern = re.compile(spec["failed_check"])
        gate_lines = [hit for hit in seen["hits"] if hit["name"] == "gate failed"]
        per_boot = {index: any(pattern.search(hit["line"]) for hit in gate_lines if hit["boot"] == first_trial + index - 1) for index in (1, 2, 3)}
        checks.add(
            "failed_check",
            f"The gate named the intended check and its error code in all three attempts: {spec['failed_check']}",
            all(per_boot.values()),
            f"per attempt: {per_boot}; console: {[hit['line'] for hit in gate_lines][:3]}",
        )

    # ---- the end state ----------------------------------------------------------------------
    outcome, final = spec["outcome"], spec["final"]
    if last is None:
        checks.add("final_report", "The surviving system reported", None, "the guest agent sent no report after staging")
        return finish(name, spec, seen, checks, reports, sequence, milliseconds)
    running = last.status.get("running")
    status = last.bootc()
    booted = (status.get("booted") or {}).get("ostree", {}).get("checksum")
    is_default = last.text("is-default.txt").strip()
    env = last.env()
    history = last.history()
    events = [event.get("event") for event in history]
    fallbacks = [event for event in history if event.get("event") == "fallback"]
    notice = last.json("boot-health/notice.json") or {}
    rejected = [item.get("digest") for item in ((last.json("boot-health/rejected.json") or {}).get("images") or [])]
    rollback_messages = json_lines(last.dir / "bootc-rollback-messages.json")
    if outcome == "either":
        # P2: whichever version the cut left, it must be whole.
        kept = running == spec["image"]
        outcome = "kept" if kept else "stale"
        final = "new" if kept else "previous"
        checks.add(
            "one_whole_version",
            "After the cut the machine runs entirely one version: the new one, or the previous one",
            (kept and booted == new) or (running == "prepared" and booted == previous),
            f"running: {running}; booted {str(booted)[:12]}; this run ended as: {outcome}",
        )
    expected_checksum = new if final == "new" else previous
    expected_marker = spec["image"] if final == "new" else "prepared"
    gate_ok = last.status.get("verdict") in ("passed", "degraded") and last.status.get("gate_unit_state") == "active"
    checks.add(
        "final_deployment",
        f"At the end the {final} version is booted, it is the default, bootc agrees, and its own gate passed",
        running == expected_marker
        and booted == expected_checksum
        and bool(booted)
        and is_default == "default"
        and status.get("rollbackQueued") is False
        and (gate_ok or outcome == "counter-only"),
        f"running: {running} (expected {expected_marker}); booted {str(booted)[:12]} (expected {str(expected_checksum)[:12]}); "
        f"ostree: {is_default}; rollbackQueued: {status.get('rollbackQueued')}; gate: {last.status.get('gate_unit_state')}, "
        f"verdict {last.status.get('verdict')}, classification {last.status.get('classification')}",
    )
    if outcome == "counter-only":
        own = json_lines(last.dir / "tur-journal-all.json")
        after_staging = [entry for entry in own if entry.get("_BOOT_ID") in {report.status.get("boot_id") for report in boots}]
        checks.add(
            "counter_alone",
            "With TUR's units masked, GRUB alone abandoned the new deployment: the block still holds what GRUB left "
            "(boot_counter=-1), the boot order is unchanged, and no TUR program ran in the surviving boot",
            env.get("boot_counter") == "-1"
            and last.status.get("gate_unit_state") == "masked"
            and not after_staging
            and booted == previous
            and running == "prepared",
            f"boot_counter={env.get('boot_counter')}; gate unit: {last.status.get('gate_unit_state')}; tur-boot-health "
            f"journal entries in the surviving boot: {len(after_staging)}; ostree says the booted deployment is {is_default}",
        )
        return finish(name, spec, seen, checks, reports, sequence, milliseconds)
    checks.add(
        "block_clean",
        "GRUB's environment block holds no boot_counter and no tur_trial_args",
        (last.dir / "grubenv.txt").exists() and "boot_counter" not in env and "tur_trial_args" not in env,
        f"grub2-editenv list: {env or 'empty'}",
    )
    checks.add(
        "no_trial_left",
        "No trial record is left",
        not (last.dir / "boot-health" / "trial.json").exists(),
        f"state files: {sorted(path.name for path in (last.dir / 'boot-health').glob('*') if path.is_file())}",
    )

    if outcome == "kept":
        other = (status.get("rollback") or {}).get("ostree", {}).get("checksum")
        checks.add(
            "marked_good",
            "The history has the marked-good event, no fallback, and bootc keeps the previous version as the rollback deployment",
            "marked-good" in events and not fallbacks and other == previous and not rejected,
            f"history: {events}; rollback deployment {str(other)[:12]} (previous {str(previous)[:12]}); rejected: {rejected or 'none'}",
        )
        ordinary = [report for report in boots if report.status.get("classification") == "normal"]
        hashes = [report.text("grubenv-sha256.txt").split()[0] for report in ordinary if report.text("grubenv-sha256.txt")]
        marked = [report for report in boots if report.status.get("classification") == "trial" and report.status.get("verdict") in ("passed", "degraded")]
        marked_hash = marked[-1].text("grubenv-sha256.txt").split()[0] if marked and marked[-1].text("grubenv-sha256.txt") else None
        checks.add(
            "next_boot_writes_nothing",
            "The boot after the one that was marked good is ordinary, and GRUB wrote nothing to the block",
            bool(ordinary) and bool(marked_hash) and all(value == marked_hash for value in hashes),
            f"block SHA-256 after marking good: {str(marked_hash)[:16]}; in the ordinary boot(s) after it: {[value[:16] for value in hashes]}",
        )
    if outcome == "stale":
        checks.add(
            "not_applied",
            'The update was not applied and nothing was rolled back: notice "The update was not applied", no rollback '
            "by bootc, no rejected image",
            notice.get("kind") == "not-applied"
            and notice.get("error_code") == "TUR-BOOT-TRIAL-001"
            and "stale" in events
            and not fallbacks
            and not rollback_messages
            and not rejected,
            f"notice: {notice.get('kind')} {notice.get('error_code')}: {notice.get('text')}; history: {events}; "
            f"bootc rollback messages: {len(rollback_messages)}; rejected: {rejected or 'none'}",
        )
    if outcome == "fallback":
        event = fallbacks[-1] if fallbacks else {}
        code = "TUR-BOOT-TRIAL-004" if spec["reason"] == "loader" else "TUR-BOOT-TRIAL-003"
        checks.add(
            "fallback_event",
            "The history has one fallback event with both versions, three attempts, the number that reached "
            "userspace and the reason",
            len(fallbacks) == 1
            and event.get("reason") == spec["reason"]
            and event.get("attempts_allowed") == 3
            and (event.get("attempts_that_reached_userspace") == spec["reached"] or spec.get("converge"))
            and (event.get("restored") or {}).get("checksum") == previous
            and (event.get("failed") or {}).get("checksum") == new,
            f"fallback events: {len(fallbacks)}; reason {event.get('reason')} (expected {spec['reason']}); attempts allowed "
            f"{event.get('attempts_allowed')}; reached userspace {event.get('attempts_that_reached_userspace')} (expected "
            f"{spec['reached']}); first failed check {event.get('first_failed_check')} {event.get('first_failed_check_code')}; "
            f"restored {str((event.get('restored') or {}).get('checksum'))[:12]}, failed {str((event.get('failed') or {}).get('checksum'))[:12]}",
        )
        checks.add(
            "rejected",
            "The failed image's digest is in the rejected list",
            bool(new_digest) and rejected == [new_digest],
            f"rejected: {rejected}; staged digest: {new_digest}",
        )
        text = notice.get("text") or ""
        wording = "could not be started by the boot loader" if spec["reason"] == "loader" else "did not start correctly after 3 attempts"
        checks.add(
            "notice",
            "The notice for the user exists and says what happened",
            notice.get("kind") == "fallback"
            and notice.get("error_code") == code
            and wording in text
            and "TUR has restored the previous version" in text
            and "The update will not be tried again automatically." in text
            and notice.get("acknowledged") is False,
            f"{notice.get('kind')} {notice.get('error_code')}: {text}",
        )
        checks.add(
            "bootc_rollback_logged",
            "The journal has bootc's own rollback message (message ID 26f3b1eb…)",
            len(rollback_messages) >= 1,
            f"{len(rollback_messages)} message(s): {[str(message.get('MESSAGE'))[:80] for message in rollback_messages][:2]}",
        )
        own = json_lines(last.dir / "tur-journal-all.json")
        said = [entry for entry in own if entry.get("MESSAGE_ID") == MSG_FALLBACK]
        checks.add(
            "journal_identifies_failure",
            "The journal of the surviving version identifies the failed deployment and the reason",
            any(
                entry.get("TUR_FAILED_IMAGE_DIGEST") == new_digest
                and entry.get("TUR_FALLBACK_REASON") == spec["reason"]
                and str(entry.get("TUR_FAILED_DEPLOYMENT", "")).startswith(str(new))
                for entry in said
            ),
            f"{[{key: entry.get(key) for key in ('TUR_ERROR_CODE', 'TUR_FALLBACK_REASON', 'TUR_FAILED_DEPLOYMENT', 'TUR_ATTEMPTS_REACHED_USERSPACE', 'TUR_FAILED_CHECK')} for entry in said][:2]}",
        )
        summary = next(iter(sorted((last.dir / "boot-health" / "evidence").glob("*/summary.json"))), None)
        exported = [item for item in ((load_json(summary) or {}).get("journals") or []) if item.get("file")] if summary else []
        files = [line for line in last.text("evidence-files.txt").splitlines() if "/journal-" in line]
        checks.add(
            "evidence",
            "The evidence directory has a journal export for each attempt that wrote a journal",
            summary is not None and (len(exported) == spec["journals"] or spec.get("converge")) and len(files) == len(exported),
            f"summary: {summary.relative_to(last.dir) if summary else 'none'}; exported journals: {len(exported)} (expected "
            f"{spec['journals']}); files: {[line.split('/')[-1] for line in files]}",
        )
        if spec["journals"]:
            # The failed attempts' own journals, read by the version that survived.
            held = {}
            for path in sorted(last.dir.glob("journal-minus-*.txt.gz")):
                try:
                    held[path.name] = gzip.decompress(path.read_bytes()).decode("utf-8", errors="replace")
                except (OSError, EOFError, ValueError):
                    continue
            marker = {
                "emergency": r"emergency\.target|Emergency Mode",
                "pid1-stops": r"tur-test-stop-pid1|Freezing execution|Caught <SEGV>",
            }.get(name, r"Boot health gate failed")
            telling = [key for key, text in held.items() if re.search(marker, text)]
            checks.add(
                "attempt_journals_survive",
                "The journals of the failed attempts are still in the persistent journal after the rollback, "
                "and show the failure",
                len(telling) >= spec["journals"],
                f"earlier boots still in the journal: {len(held)}; of them showing /{marker}/: {len(telling)}",
            )
        if spec.get("gate_failed", True) and spec["reached"] and not spec.get("converge"):
            checks.add(
                "first_failed_check_recorded",
                "The fallback event and the notice carry the first failed check and its error code",
                bool(event.get("first_failed_check")) and bool(event.get("first_failed_check_code")),
                f"{event.get('first_failed_check')} {event.get('first_failed_check_code')}: {event.get('first_failed_check_detail')}",
            )
        if spec["plan"].get("extra_reboots") == "2":
            tail = boots[-2:]
            checks.add(
                "no_loop",
                "L1: two more restarts after the fallback are ordinary boots of the previous version, with no counter",
                len(boots) >= 3
                and all(
                    report.status.get("running") == "prepared"
                    and report.status.get("classification") == "normal"
                    and not report.status.get("counter_at_start")
                    for report in tail
                )
                and sequence[-2:] == ["none", "none"],
                f"last two reports: {[(r.status.get('running'), r.status.get('classification'), r.status.get('counter_at_start')) for r in tail]}",
            )
    checks.add(
        "tur_diag",
        "tur-diag collect succeeds on the final system and the bundle contains the boot-health files",
        None,
        "NOT RUN: tur-diag is not built into the image yet (step 12 of the design's brief)",
        required=False,
    )
    return finish(name, spec, seen, checks, reports, sequence, milliseconds)


def finish(name: str, spec: dict, seen: dict, checks: Checks, reports: dict, sequence: list, milliseconds) -> dict:
    result = "pass" if checks.passed() else "fail"
    return {
        "scenario": name,
        "id": spec["id"],
        "title": spec["title"],
        "result": result,
        "manual_recovery": spec["manual"],
        "sequence_after_staging": sequence,
        "console_sequence": seen["sequence"],
        "staging_milliseconds": int(milliseconds) if milliseconds and str(milliseconds).isdigit() else None,
        "elapsed_seconds": seen["elapsed_seconds"],
        "outcome": seen["outcome"],
        "harness_actions": seen["actions"],
        "qmp_events": seen["events"],
        "boots": seen["boots"],
        "guest_reports": sorted(reports),
        "checks": checks.items,
    }


def summarise(report: dict) -> str:
    icon = {"pass": "pass", "fail": "**FAIL**", "not-run": "**NOT RUN**"}
    lines = [
        f"### {report['id']} `{report['scenario']}`: {report['result'].upper()}",
        "",
        f"{report['title']}.",
        "",
        f"Boots after staging, from the console: `{', '.join(report['sequence_after_staging']) or 'none'}`. "
        f"Staging took {report['staging_milliseconds']} ms; the scenario {report['elapsed_seconds']} s.",
        "",
        "| Check | Result | Evidence |",
        "|---|---|---|",
    ]
    for item in report["checks"]:
        label = item["description"] + ("" if item["required"] else " (informational)")
        lines.append(f"| {label} | {icon[item['result']]} | {item['detail'].replace('|', '/')[:1500]} |")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenarios", nargs="*")
    ap.add_argument("--list", action="store_true", help="print the scenario names and exit")
    ap.add_argument("--disk", help="the prepared disk (raw); never written to")
    ap.add_argument("--images", help="ext4 image holding the OCI layout of the scenario images")
    ap.add_argument("--work", help="scratch directory")
    ap.add_argument("--out", help="artifact directory")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096, help="MiB")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()
    if args.list:
        for name, spec in SCENARIOS.items():
            print(f"{name}\t{spec['id']}\t{spec['image']}\t{spec['title']}")
        return 0
    unknown = [name for name in args.scenarios if name not in SCENARIOS]
    if unknown or not args.scenarios or not (args.disk and args.images and args.work and args.out):
        ap.error(f"unknown scenarios: {unknown}" if unknown else "scenarios, --disk, --images, --work and --out are required")
    work, base = pathlib.Path(args.work).resolve(), pathlib.Path(args.out).resolve()
    work.mkdir(parents=True, exist_ok=True)
    results = []
    for name in args.scenarios:
        spec = SCENARIOS[name]
        out = base / name
        out.mkdir(parents=True, exist_ok=True)
        try:
            seen = drive(name, spec, args, out, work)
            source = unpack(pathlib.Path(seen["report_tar"]), out / "guest")
            postmortem(pathlib.Path(seen["overlay"]), out, work)
            # A report the guest wrote but could not send is still on its disk.
            for recovered in sorted((out / "disk" / "scenario-reports" / name).glob("*")):
                if recovered.is_dir() and not (out / "guest" / recovered.name / "status").exists():
                    shutil.copytree(recovered, out / "guest" / recovered.name, dirs_exist_ok=True)
                    source += f"; {recovered.name} from disk"
            pathlib.Path(seen["overlay"]).unlink(missing_ok=True)
            (out / "host" / "driver.json").write_text(json.dumps(seen, indent=1) + "\n")
            report = evaluate(name, spec, seen, out, source)
        except Exception as error:  # a scenario must never take the others with it
            import traceback

            traceback.print_exc()
            report = {
                "scenario": name,
                "id": spec["id"],
                "title": spec["title"],
                "result": "fail",
                "manual_recovery": spec["manual"],
                "sequence_after_staging": [],
                "console_sequence": [],
                "staging_milliseconds": None,
                "elapsed_seconds": None,
                "outcome": f"driver error: {error}",
                "checks": [
                    {
                        "id": f"{name}.driver",
                        "description": "The scenario driver ran",
                        "required": True,
                        "result": "fail",
                        "detail": str(error),
                    }
                ],
            }
        (out / "scenario-report.json").write_text(json.dumps(report, indent=2) + "\n")
        summary = summarise(report)
        (out / "summary.md").write_text(summary)
        print(summary, flush=True)
        results.append(report)
    overall = {
        "schema_version": 1,
        "task": "TUR-005",
        "result": "pass" if all(report["result"] == "pass" for report in results) else "fail",
        "scenarios": [
            {key: report.get(key) for key in ("scenario", "id", "title", "result", "manual_recovery", "sequence_after_staging", "staging_milliseconds", "elapsed_seconds")}
            for report in results
        ],
    }
    (base / "rollback-report.json").write_text(json.dumps(overall, indent=2) + "\n")
    table = ["| Scenario | Result | Boots after staging | Staging (ms) | Time (s) |", "|---|---|---|---|---|"]
    for report in results:
        table.append(
            f"| {report['id']} `{report['scenario']}` | {report['result'].upper()} | "
            f"`{', '.join(report['sequence_after_staging'])}` | {report['staging_milliseconds']} | {report['elapsed_seconds']} |"
        )
    (base / "summary.md").write_text("\n".join(["### Boot-health scenarios", ""] + table + [""] + [summarise(r) for r in results]))
    print("\n".join(table))
    return 0 if overall["result"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())

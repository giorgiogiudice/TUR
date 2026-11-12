#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Prove on a booted TUR system that only TUR's signed image is accepted as
an update (ADR-0023; docs/architecture/publishing.md, section 7).

    tests/boot/signed-update.py --disk disk.raw --work WORK --out OUT \
        --registry 10.0.2.2:5000/tur --digest sha256:… --signed TAG \
        --case unsigned=TAG --case other-key=TAG --case no-log=TAG \
        [--test-key release.pub] [--policy-rule rule.json]

The disk is the boot test's prepared disk (run-vm.py --prepare-only). It
starts from a copy-on-write overlay with opt/org.tur/signed-update in fw_cfg;
the guest's signed-update-agent asks bootc to switch to
ghcr.io/giorgiogiudice/tur:<tag> for each refusal case and then for the
signed image, which the host's registry (reached from the guest at
10.0.2.2) serves under that name. After the switch to the signed image the
guest restarts into it; a reporter left in /etc and /var reports from the new
version once boot health has marked the boot good.

Checks (all required):

- the policy the guest ran with is the image's own: the vendor file holds
  TUR's rule exactly as image/signing/policy.py writes it (--policy-rule),
  and no /etc/containers/policy.json exists;
- each refusal case: bootc switch failed, the container libraries said the
  source image was rejected, nothing was staged, no deployment was added,
  no image reference and no object was added to OSTree's repository;
- the signed image: bootc switch succeeded and staged exactly --digest;
- after the restart: the system runs exactly --digest, it is the product
  image (no test agent), and boot health marked the boot good.

Writes OUT/signed-update.json and OUT/summary.md; exit status 0 only if every
check passed. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import argparse
import base64
import io
import json
import pathlib
import subprocess
import sys
import tarfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import vm  # noqa: E402

PATTERNS = [
    ("kernel panic", r"Kernel panic - not syncing"),
    ("emergency mode", r"[Ee]mergency [Mm]ode"),
    ("gate failed", r"tur-boot-health: gate: Boot health gate failed"),
    ("gate passed", r"tur-boot-health: gate: Boot health gate (passed|degraded)"),
    ("marked good", r"tur-boot-health: mark-good: counter removed"),
]


def props(text: str) -> dict[str, str]:
    found = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        if sep:
            found[key.strip()] = value.strip()
    return found


def drive(args, out: pathlib.Path, work: pathlib.Path, fw_cfg: dict[str, str]) -> dict:
    overlay = work / "signed-update.qcow2"
    overlay.unlink(missing_ok=True)
    subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", str(pathlib.Path(args.disk).resolve()),
                    "-F", "raw", str(overlay)], check=True)
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    serial_log, report_tar = out / "serial.log", work / "signed-update-report.tar"
    for stale in (serial_log, report_tar):
        stale.unlink(missing_ok=True)
    machine = vm.Machine(disk=str(overlay), disk_format="qcow2", disk_cache="writeback", work=work,
                         serial_log=serial_log, report_tar=report_tar, qemu_log=host / "qemu.log",
                         name="tur-signed-update", cpus=args.cpus, memory=args.memory, qemu=args.qemu,
                         fw_cfg=fw_cfg, variables_from=str(work / "OVMF_VARS.prepared.fd"))
    machine.variables.unlink(missing_ok=True)
    started = time.monotonic()

    def elapsed() -> float:
        return round(time.monotonic() - started, 1)

    print(f"$ {' '.join(machine.command())}", flush=True)
    watcher = vm.SerialWatcher(serial_log, elapsed, patterns=PATTERNS)
    outcome = "timeout"
    try:
        qmp = machine.start()
        if not qmp.command("query-kvm").get("enabled"):
            raise RuntimeError("KVM is not enabled")
        while time.monotonic() < started + args.timeout:
            if not machine.alive():
                outcome = "qemu-exited"
                break
            time.sleep(0.2)
            watcher.pump()
            machine.collect()
            if any(line["text"].startswith("SIGNED-COMPLETE") for line in watcher.agent_lines):
                outcome = "complete"
                break
    except Exception as error:  # the evidence below says what happened
        outcome = f"error: {error}"
    if machine.alive():
        try:
            vm.screenshot(machine.qmp, work, out / "screenshot-at-end.png")
        except Exception as error:  # diagnosis only
            print(f"no screenshot: {error}")
        if not (outcome == "complete" and machine.powerdown(timeout=120)):
            machine.quit()
    watcher.pump()
    print(f"[{elapsed():7.1f}s] {outcome}; boots: {', '.join(watcher.sequence())}", flush=True)
    if outcome != "complete" and serial_log.exists():
        text = vm.clean(serial_log.read_bytes().decode("utf-8", errors="replace"))
        print("\n".join([line for line in text.split("\n") if line.strip()][-150:]))
    guest = out / "guest"
    guest.mkdir(parents=True, exist_ok=True)
    if report_tar.exists() and report_tar.stat().st_size:
        try:
            with tarfile.open(fileobj=io.BytesIO(report_tar.read_bytes()), mode="r:", ignore_zeros=True) as archive:
                archive.extractall(guest, filter="data")
        except (tarfile.TarError, OSError, EOFError) as error:
            print(f"the reports could not all be read: {error}")
    overlay.unlink(missing_ok=True)
    return {"outcome": outcome, "elapsed_seconds": elapsed(), "boots": watcher.sequence(),
            "hits": watcher.hits, "agent_lines": watcher.agent_lines}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--registry", required=True, help="the host's registry repository as the guest reaches it")
    ap.add_argument("--logical", default="ghcr.io/giorgiogiudice/tur")
    ap.add_argument("--digest", required=True, help="the digest under release")
    ap.add_argument("--signed", required=True, help="the tag of the signed image")
    ap.add_argument("--case", action="append", default=[], help="NAME=TAG of an image that must be refused")
    ap.add_argument("--test-key", help="public key to put over the image's release.pub (publishing off)")
    ap.add_argument("--policy-rule", required=True, help="the rule image/signing/policy.py --print gives")
    ap.add_argument("--timeout", type=int, default=3600)
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096)
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()
    out, work = pathlib.Path(args.out).resolve(), pathlib.Path(args.work).resolve()
    out.mkdir(parents=True, exist_ok=True)
    cases = dict(c.split("=", 1) for c in args.case)
    fw_cfg = {"signed-update": "1", "logical": args.logical, "physical": args.registry,
              "digest": args.digest, "signed": args.signed,
              "cases": " ".join(f"{name}={tag}" for name, tag in cases.items()),
              "key": base64.b64encode(pathlib.Path(args.test_key).read_bytes()).decode() if args.test_key else ""}
    seen = drive(args, out, work, fw_cfg)

    stage = props((out / "guest/stage/status").read_text()) if (out / "guest/stage/status").exists() else {}
    new = props((out / "guest/new-version/status").read_text()) if (out / "guest/new-version/status").exists() else {}
    checks: list[dict] = []

    def check(ident: str, what: str, passed: bool, detail: str) -> None:
        checks.append({"id": ident, "check": what, "passed": bool(passed), "detail": detail})
        print(f"{'PASS' if passed else 'FAIL'}  {ident} {what}: {detail}", flush=True)

    check("S0", "the guest reported from the version it updated from", bool(stage), f"{len(stage)} facts")
    rule = json.loads(pathlib.Path(args.policy_rule).read_text())
    try:
        shipped = json.loads((out / "guest/stage/policy.json").read_text())["transports"]["docker"]
        shipped_rule = {args.logical: shipped.get(args.logical)}
    except (OSError, ValueError, KeyError) as error:
        shipped_rule = {"error": str(error)}
    check("S1", "the policy is the image's own: TUR's rule, as policy.py writes it", shipped_rule == rule,
          json.dumps(shipped_rule))
    etc_policy = (out / "guest/stage/etc-policy.txt").read_text() if (out / "guest/stage/etc-policy.txt").exists() else ""
    check("S1b", "no /etc/containers/policy.json hides the vendor policy", "No such file" in etc_policy,
          etc_policy.strip()[:200])
    for name in cases:
        rc = stage.get(f"case_{name}_rc", "")
        message = stage.get(f"case_{name}_message", "")
        refs = stage.get(f"case_{name}_image_refs", "")
        objects = stage.get(f"case_{name}_objects", "")
        before = stage.get(f"case_{name}_deployments_before")
        after = stage.get(f"case_{name}_deployments_after")
        ref_parts, object_parts = refs.split("->"), objects.split("->")
        check(f"R-{name}", f"{name}: refused by the container policy", rc not in ("", "0") and bool(message),
              f"bootc switch exit status {rc}; {message or 'no rejection message'}")
        check(f"N-{name}", f"{name}: nothing staged, nothing added",
              stage.get(f"case_{name}_staged") == "none" and before == after and before is not None
              and len(ref_parts) == 2 and ref_parts[0] == ref_parts[1]
              and len(object_parts) == 2 and object_parts[0] == object_parts[1],
              f"staged {stage.get(f'case_{name}_staged')}; deployments {before!r} -> {after!r}; "
              f"image refs {refs}; repository objects {objects}")
    check("A1", "the signed image: switched and staged by its digest",
          stage.get("signed_rc") == "0" and stage.get("signed_staged") == args.digest,
          f"exit status {stage.get('signed_rc')}; staged {stage.get('signed_staged')}; "
          f"{stage.get('signed_milliseconds')} ms")
    check("A2", "after the restart the computer runs exactly the published digest",
          new.get("booted_digest") == args.digest, f"booted {new.get('booted_digest')}, build {new.get('build_id')}")
    check("A3", "it is the product image, without the test agent", new.get("test_agent_present") == "no",
          f"test agent present: {new.get('test_agent_present')}")
    check("A4", "boot health marked the new version good", new.get("mark_good") == "active",
          f"tur-boot-mark-good {new.get('mark_good')}, gate {new.get('gate')}")
    informational = {
        "test_key": stage.get("test_key"),
        "enforce_container_sigpolicy": {"rc": stage.get("enforce_flag_rc"), "staged": stage.get("enforce_flag_staged")},
        "enforce_flag_output": (out / "guest/stage/switch-enforce-flag.txt").read_text()[-600:]
        if (out / "guest/stage/switch-enforce-flag.txt").exists() else "",
    }
    passed = all(c["passed"] for c in checks)
    record = {"passed": passed, "digest": args.digest, "signed_tag": args.signed, "cases": cases,
              "checks": checks, "informational": informational, "run": seen}
    (out / "signed-update.json").write_text(json.dumps(record, indent=2) + "\n")
    lines = [f"### Signed update in a booted guest: {'passed' if passed else '**FAILED**'}", "",
             f"Digest under release: `{args.digest}` (tag `{args.signed}`); {informational['test_key']}.", "",
             "| | Check | Result | Detail |", "|---|---|---|---|"]
    lines += [f"| {c['id']} | {c['check']} | {'pass' if c['passed'] else '**FAIL**'} | "
              f"{c['detail'].replace('|', '/')[:300]} |" for c in checks]
    lines += ["", f"`bootc switch --enforce-container-sigpolicy` (informational): exit status "
              f"{informational['enforce_container_sigpolicy']['rc']}, staged "
              f"{informational['enforce_container_sigpolicy']['staged'] or 'nothing'}."]
    (out / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())

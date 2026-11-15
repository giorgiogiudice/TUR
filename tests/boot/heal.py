#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Prove TUR's self-healing loop on a booted TUR system, with injected
faults (ADR-0027; docs/architecture/self-healing.md, section 9).

    tests/boot/heal.py --disk disk.raw --work WORK --out OUT

The disk is the boot test's prepared disk (run-vm.py --prepare-only). It
starts from a copy-on-write overlay with opt/org.tur/heal in fw_cfg; the
guest's heal-agent injects the faults, lets TUR's healing service find,
repair and verify them, has P@x (scripted provider) and the person act
through the service's socket, sends a report to a stand-in receiver only
when the person confirms, and restarts once for the fix link. Its reports
come back over the boot test's virtio port; every check is made here.

Writes OUT/heal.json and OUT/summary.md; exit status 0 only if every
required check passed. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import pathlib
import subprocess
import sys
import tarfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import vm  # noqa: E402

SERVICE = "TUR-DESKTOP-SERVICE-001"
TIME = "TUR-NETWORK-TIME-001"
RESOLVER = "TUR-NETWORK-RESOLVER-001"
SPACE = "TUR-STORAGE-SPACE-001"
CRASH = "TUR-DIAG-CRASH-001"
CODES = [SERVICE, TIME, RESOLVER, SPACE, CRASH]
STUB = "../run/systemd/resolve/stub-resolv.conf"

PATTERNS = [
    ("kernel panic", r"Kernel panic - not syncing"),
    ("emergency mode", r"[Ee]mergency [Mm]ode"),
    ("marked good", r"tur-boot-health: mark-good: counter removed"),
]


def props(path: pathlib.Path) -> dict[str, str]:
    found: dict[str, str] = {}
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                found[key.strip()] = value.strip()
    return found


def document(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def text(path: pathlib.Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def drive(args, out: pathlib.Path, work: pathlib.Path) -> dict:
    overlay = work / "heal.qcow2"
    overlay.unlink(missing_ok=True)
    subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", str(pathlib.Path(args.disk).resolve()),
                    "-F", "raw", str(overlay)], check=True)
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    serial_log, report_tar = out / "serial.log", work / "heal-report.tar"
    for stale in (serial_log, report_tar):
        stale.unlink(missing_ok=True)
    machine = vm.Machine(disk=str(overlay), disk_format="qcow2", disk_cache="writeback", work=work,
                         serial_log=serial_log, report_tar=report_tar, qemu_log=host / "qemu.log",
                         name="tur-heal", cpus=args.cpus, memory=args.memory, qemu=args.qemu,
                         fw_cfg={"heal": "1"}, variables_from=str(work / "OVMF_VARS.prepared.fd"))
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
            if any(line["text"].startswith("HEAL-COMPLETE") for line in watcher.agent_lines):
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
        lines = vm.clean(serial_log.read_bytes().decode("utf-8", errors="replace")).split("\n")
        print("\n".join([line for line in lines if line.strip()][-150:]))
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


def record(state, code):
    """The newest record of `code` in a state document."""
    found = None
    for item in (state or {}).get("faults", []):
        if item.get("code") == code:
            found = item
    return found or {}


def events(state, code):
    return [event.get("kind") for event in (state or {}).get("events", []) if event.get("code") == code]


def bundle_members(data: bytes) -> dict[str, bytes]:
    """The members of a zstd-compressed tar bundle."""
    raw = subprocess.run(["zstd", "-dc"], input=data, capture_output=True, check=True).stdout
    members = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as archive:
        for member in archive.getmembers():
            if member.isfile():
                members[member.name] = archive.extractfile(member).read()
    return members


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--timeout", type=int, default=2400)
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096)
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()
    out, work = pathlib.Path(args.out).resolve(), pathlib.Path(args.work).resolve()
    out.mkdir(parents=True, exist_ok=True)
    seen = drive(args, out, work)

    a, b = out / "guest/phase-a", out / "guest/phase-b"
    sa, sb = props(a / "status"), props(b / "status")
    checks: list[dict] = []

    def check(ident: str, what: str, passed, detail, required: bool = True) -> None:
        checks.append({"id": ident, "check": what, "passed": bool(passed), "required": required,
                       "detail": str(detail)[:1500]})
        print(f"{'PASS' if passed else ('FAIL' if required else 'note')}  {ident} {what}: {str(detail)[:300]}",
              flush=True)

    rc = lambda key: sa.get(key, "")  # noqa: E731

    check("H0", "the guest reported from both boots (before and after its restart)", bool(sa) and bool(sb),
          f"phase A: {len(sa)} facts, phase B: {len(sb)} facts; outcome {seen['outcome']}")

    # ---- the product as installed ------------------------------------------------------
    # The image enables them by links under /usr/lib/systemd, which
    # `systemctl is-enabled` reports as "disabled" (measured, heal run
    # 43480677457): the links and the active state are what count.
    check("P1", "the healing timer and the request socket are wanted by the image and active; the session's "
          "notice watcher is wanted for every user",
          sa.get("timer_wanted") == "yes" and sa.get("timer_active") == "active"
          and sa.get("socket_wanted") == "yes" and sa.get("socket_active") == "active"
          and sa.get("notify_path_wanted") == "yes",
          {key: sa.get(key) for key in ("timer_wanted", "timer_enabled", "timer_active", "socket_wanted",
                                        "socket_enabled", "socket_active", "socket_path", "notify_path_wanted")})
    baseline = document(a / "state-baseline.json")
    present = [item["code"] for item in (baseline or {}).get("faults", []) if item.get("state") != "resolved"]
    check("P2", "tur-heal.service ran through systemd and found none of the five faults before any was injected",
          rc("scan_baseline_rc") == "0" and baseline is not None and not present,
          f"exit status {rc('scan_baseline_rc')}; present: {present}")

    # ---- 1. a failed service ----------------------------------------------------------------
    state = document(a / "state-service.json")
    item = record(state, SERVICE)
    applied = item.get("applied", [])
    check("S1", "chronyd killed: the service found it failed, started it by itself, and verified it runs",
          sa.get("chronyd_before") == "active" and sa.get("chronyd_injected") == "failed"
          and sa.get("chronyd_after_scan") == "active" and item.get("state") == "repaired"
          and len(applied) == 1 and applied[0].get("by") == "service" and applied[0].get("outcome") == "repaired",
          f"chronyd {sa.get('chronyd_before')} -> {sa.get('chronyd_injected')} -> {sa.get('chronyd_after_scan')}; "
          f"record {item.get('state')}, applied {json.dumps(applied)[:300]}")
    held = record(document(a / "state-service-held.json"), SERVICE)
    check("S2", "the next scan found the repair held", held.get("state") == "resolved"
          and held.get("resolved_how") == "repair-held" and sa.get("chronyd_after_second_scan") == "active",
          f"{held.get('state')} ({held.get('resolved_how')})")

    # ---- 2. network time, repaired by P@x through the service -----------------------------------
    item = record(document(a / "state-time.json"), TIME)
    check("T1", "network time switched off: found, and left for a person (not automatic)",
          sa.get("ntp_injected") == "no" and sa.get("ntp_after_scan") == "no" and item.get("state") == "present",
          f"NTP {sa.get('ntp_injected')} -> {sa.get('ntp_after_scan')}; record {item.get('state')}")
    audit = [json.loads(line) for line in text(a / "pax/audit.jsonl").splitlines() if line.strip()]
    decisions = [(entry.get("TUR_AI_ACTION"), entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION"))
                 for entry in audit if entry.get("TUR_AI_EVENT") == "decision"]
    executed = [entry.get("TUR_AI_RESULT") for entry in audit if entry.get("TUR_AI_EVENT") == "executed"]
    expected = [("org.tur.Diagnostics1:heal_history", "0", "allowed"),
                ("org.tur.Diagnostics1:heal_list_faults", "0", "allowed"),
                ("org.tur.Diagnostics1:heal_explain", "0", "allowed"),
                ("org.tur.Diagnostics1:heal_apply_remedy", "1", "allowed"),
                ("org.tur.Diagnostics1:heal_apply_remedy", "1", "allowed")]
    check("T2", "P@x (installed tur-pax, scripted provider, the image's tools): reads at class 0, asks for the "
          "repair at class 1, allowed by the policy engine; the service refused the class 2 one",
          sa.get("heal_pax_rc") == "0" and decisions == expected and executed == ["ok", "ok", "ok", "ok", "failed"],
          f"exit {sa.get('heal_pax_rc')}; decisions {decisions}; results {executed}; "
          f"stderr {text(a / 'pax/stderr.txt')[-300:]}")
    requests = document(a / "pax/requests.json") or []
    sent = json.dumps(requests)
    # A request tool's call is "ok" only when the service answered "ok":
    # true (T2); here, that the refusal reached the model as the service
    # gave it.
    check("T3", "the service's refusal of deleting logs reached P@x with its code, TUR-DIAG-HEAL-003",
          "TUR-DIAG-HEAL-003" in sent, sent[-600:])
    item = record(document(a / "state-time-pax.json"), TIME)
    applied = item.get("applied", [])
    check("T4", "network time is on again, applied as a request from the test user's account and verified",
          sa.get("ntp_after_pax") == "yes" and len(applied) == 1 and applied[0].get("by") == "request"
          and applied[0].get("uid") is not None and applied[0].get("outcome") == "repaired",
          f"NTP {sa.get('ntp_after_pax')}; applied {json.dumps(applied)[:300]}")

    # ---- 3. the resolver file --------------------------------------------------------------------
    item = record(document(a / "state-resolver.json"), RESOLVER)
    check("R1", "/etc/resolv.conf replaced by a file: found, left for a person",
          sa.get("resolver_after_scan") == "file" and sa.get("resolver_original") != "not-a-link",
          f"original {sa.get('resolver_original')}; after the scan {sa.get('resolver_after_scan')}")
    applied_doc = document(a / "resolver-apply.json") or {}
    check("R2", "the person's 'tur-diag heal apply' (through the socket) restored the link and verified it",
          rc("resolver_apply_rc") == "0" and sa.get("resolver_after_apply") == STUB
          and applied_doc.get("outcome") == "repaired",
          f"exit {rc('resolver_apply_rc')}; link {sa.get('resolver_after_apply')}; "
          f"label {sa.get('resolver_label_after_apply')}; {json.dumps(applied_doc)[:200]}")
    check("R3", "'tur-diag heal undo' put back exactly the file that was there (same SHA-256)",
          rc("resolver_undo_rc") == "0"
          and sa.get("resolver_after_undo_sha256") == sa.get("resolver_injected_sha256"),
          f"exit {rc('resolver_undo_rc')}; {sa.get('resolver_after_undo_sha256')} vs "
          f"{sa.get('resolver_injected_sha256')}")
    check("R4", "applied again, the link is the system's original one", rc("resolver_apply_again_rc") == "0"
          and sa.get("resolver_final") == sa.get("resolver_original"),
          f"final {sa.get('resolver_final')}, original {sa.get('resolver_original')}; record {item.get('state')}")

    # ---- 4. the disk almost full -----------------------------------------------------------------
    item = record(document(a / "state-space.json"), SPACE)
    check("D1", "/var filled to under 1 GiB free: found, no repair attempted by itself",
          sa.get("space_filled") == "yes" and item.get("state") == "present" and not item.get("applied"),
          f"filled {sa.get('space_filled')}; free {sa.get('space_available_filled')}; record {item.get('state')}")
    check("D2", "deleting logs asked through the socket (the person without administrator rights, as P@x would) "
          "is refused with TUR-DIAG-HEAL-003",
          rc("space_socket_rc") == "1" and "TUR-DIAG-HEAL-003" in text(a / "space-socket.json.err"),
          f"exit {rc('space_socket_rc')}; {text(a / 'space-socket.json.err')[:200]}")
    check("D3", "as administrator but not confirmed: refused with TUR-DIAG-HEAL-004",
          rc("space_unconfirmed_rc") == "1" and "TUR-DIAG-HEAL-004" in text(a / "space-unconfirmed.json.err"),
          f"exit {rc('space_unconfirmed_rc')}; {text(a / 'space-unconfirmed.json.err')[:200]}")
    confirmed = document(a / "space-confirmed.json") or {}
    cleared = record(document(a / "state-space-cleared.json"), SPACE)
    check("D4", "confirmed: logs deleted, the check run again says it is still full (not repaired, exit 4); "
          "only that one application is recorded",
          rc("space_confirmed_rc") == "4" and confirmed.get("outcome") == "not-repaired"
          and len(cleared.get("applied", [])) == 1,
          f"exit {rc('space_confirmed_rc')}; {json.dumps(confirmed)[:200]}; journal {sa.get('journal_before')} -> "
          f"{sa.get('journal_after_confirmed')}")
    check("D5", "space freed by other means: the fault cleared at the next scan",
          cleared.get("state") == "resolved" and cleared.get("resolved_how") == "cleared",
          f"{cleared.get('state')} ({cleared.get('resolved_how')})")

    # ---- 5. a crash: a report offered, sent only with consent, redacted --------------------------
    item = record(document(a / "state-crash.json"), CRASH)
    check("C1", "tur-diag killed with SIGABRT: the service found the crash, tried nothing, offered a report",
          "tur-diag" in text(a / "coredumps.txt") and item.get("state") == "report-offered"
          and item.get("subjects") == ["tur-diag"] and not item.get("applied"),
          f"record {item.get('state')}, subjects {item.get('subjects')}, evidence {item.get('evidence')}")
    counts = [sa.get(key) for key in ("receiver_count_start", "receiver_count_after_scan",
                                      "receiver_count_after_prepare", "receiver_count_after_unconfirmed",
                                      "receiver_count_after_send")]
    check("C2", "nothing reached the receiver when the fault was found, when the report was prepared, or when "
          "sending was not confirmed (TUR-DIAG-REPORT-004); one upload after the person confirmed",
          counts == ["0", "0", "0", "0", "1"] and rc("crash_report_rc") == "0" and rc("send_unconfirmed_rc") != "0"
          and "TUR-DIAG-REPORT-004" in text(a / "send-unconfirmed.txt.err") and rc("send_confirmed_rc") == "0",
          f"requests seen at each step: {counts}; prepare {rc('crash_report_rc')}, unconfirmed send "
          f"{rc('send_unconfirmed_rc')}, confirmed send {rc('send_confirmed_rc')}")
    prepared = (a / "prepared.tar.zst").read_bytes() if (a / "prepared.tar.zst").exists() else b""
    uploads = sorted(a.glob("upload-*.bin"))
    upload = uploads[0].read_bytes() if uploads else b""
    check("C3", "what was sent is exactly the report the person was shown (same SHA-256)",
          bool(upload) and hashlib.sha256(upload).hexdigest() == hashlib.sha256(prepared).hexdigest(),
          f"upload {hashlib.sha256(upload).hexdigest()[:16]} ({len(upload)} bytes), prepared "
          f"{hashlib.sha256(prepared).hexdigest()[:16]} ({len(prepared)} bytes)")
    planted = {name: sa.get(f"planted_{name}", "") for name in ("host", "ip", "user", "mac", "machine_id")}
    found_in, report, redaction = {}, {}, {}
    try:
        members = bundle_members(upload)
        everything = b"\n".join(members.values()).decode("utf-8", errors="replace").lower()
        for name, value in planted.items():
            if value and value.lower() in everything:
                found_in[name] = [path for path, data in members.items()
                                  if value.lower() in data.decode("utf-8", errors="replace").lower()]
        report = json.loads(members.get("report.json", b"{}"))
        redaction = json.loads(members.get("redaction-report.json", b"{}"))
    except (subprocess.CalledProcessError, tarfile.TarError, ValueError, OSError) as error:
        found_in = {"error": str(error)}
    check("C4", "none of the planted personal values (host name, account, public address, MAC address, machine "
          "ID) is in the upload",
          bool(upload) and all(planted.values()) and not found_in,
          f"planted {sorted(name for name, value in planted.items() if value)}; found: {found_in or 'none'}")
    replaced = {name: (category or {}).get("replacements")
                for name, category in (redaction.get("categories") or {}).items()}
    check("C5", "control: the bundle had them and replaced them (host names and user names replaced)",
          (replaced.get("hostname") or 0) > 0 and (replaced.get("username") or 0) > 0,
          f"replacements by category: {replaced}", required=False)
    check("C6", "the report says what TUR found: code, kind, component, written by the person",
          report.get("error_code") == CRASH and report.get("kind") == "crash" and report.get("component") == "diag"
          and report.get("prepared_by") == "person",
          json.dumps({key: report.get(key) for key in ("error_code", "kind", "component", "prepared_by",
                                                         "title")}))

    # ---- 6. the fix link -------------------------------------------------------------------------
    recurred = document(a / "state-recurred.json")
    check("F1", "an update naming the code, the crash again in this boot: 'happened again', report offered again",
          "recurred-after-update" in events(recurred, CRASH) and record(recurred, CRASH).get("state") == "report-offered",
          f"events {events(recurred, CRASH)}")
    fixed = record(document(b / "state-fixed.json"), CRASH)
    check("F2", "after the restart the crash stays away: resolved as fixed by the update",
          fixed.get("state") == "resolved" and fixed.get("resolved_how") == "fixed-by-update",
          f"{fixed.get('state')} ({fixed.get('resolved_how')})")
    notices = document(b / "notices-fixed.json") or []
    fixed_notice = [notice for notice in notices if notice.get("kind") == "fixed-by-update"]
    check("F3", "the person who sent the report is told: 'The problem you reported is fixed'",
          bool(fixed_notice) and fixed_notice[0].get("body", "").startswith("The problem you reported is fixed"),
          json.dumps(fixed_notice)[:400])

    # ---- awareness: every notice shown, none dropped, none twice; the history --------------------
    titles = {SERVICE: "A system service stopped working", TIME: "The clock is not kept right",
              RESOLVER: "Name lookups bypass the system resolver", SPACE: "The system disk is almost full",
              CRASH: "A TUR program crashed"}
    noticed = {"detected", "repaired", "not-repaired", "report-offered", "fixed-by-update",
               "recurred-after-update", "undone"}

    def expected_notices(state):
        """The (code, kind) of every event the person must be told about."""
        return [(event.get("code"), event.get("kind")) for event in (state or {}).get("events", [])
                if event.get("kind") in noticed or (event.get("kind") == "held" and event.get("by") == "service")]

    def recorded(path):
        return [json.loads(line) for line in text(path).splitlines() if line.strip()]

    waiting = document(a / "pending-no-session.json") or []
    still = document(a / "pending-still.json") or []
    waiting_kinds = [(notice.get("code"), notice.get("kind")) for notice in waiting]
    check("A1", "no graphical session: the automatic repair and its check are not shown, and wait; the "
          "notice program with no session bus shows nothing and marks nothing seen",
          waiting_kinds == [(SERVICE, "repaired"), (SERVICE, "held")]
          and [n.get("id") for n in still] == [n.get("id") for n in waiting]
          and "0 notice(s) shown; 2 wait" in text(a / "notify-no-session.txt.err"),
          f"waiting {waiting_kinds}; still {len(still)}; {text(a / 'notify-no-session.txt.err').strip()[-120:]}")
    shown_a = recorded(a / "notify/recorded.jsonl")
    after_sign_in = document(a / "pending-after-sign-in.json")
    first_two = [notice.get("summary") for notice in shown_a[:2]]
    check("A2", "at sign-in (tur-heal-notify.service, a notification service on the session bus) both "
          "waiting notices were shown, the automatic repair first, saying TUR did it by itself; none waits",
          sa.get("notification_server_a") == "ready" and sa.get("notify_unit_a") == "success"
          and first_two == [f"TUR: {titles[SERVICE]}"] * 2
          and "TUR did this by itself" in (shown_a[0].get("body", "") if shown_a else "")
          and "held" in (shown_a[1].get("body", "") if len(shown_a) > 1 else "")
          and after_sign_in == [],
          f"server {sa.get('notification_server_a')}, unit {sa.get('notify_unit_a')}; first two {first_two}; "
          f"waiting after {after_sign_in}")
    state_end_a = document(a / "state-recurred.json")
    expected = expected_notices(state_end_a)
    want_titles, got_titles = {}, {}
    for code, _kind in expected:
        want_titles[f"TUR: {titles.get(code, code)}"] = want_titles.get(f"TUR: {titles.get(code, code)}", 0) + 1
    for notice in shown_a:
        got_titles[notice.get("summary")] = got_titles.get(notice.get("summary"), 0) + 1
    check("A3", "from then on every event the person must know of was shown, once each: "
          "automatic repairs and whether they held, faults waiting for the person, repairs by P@x and by "
          "the person, undo, reports offered, 'happened again'",
          len(shown_a) == len(expected) and want_titles == got_titles
          and document(a / "pending-end-a.json") == [] and sa.get("notify_unit_end_a") == "success",
          f"{len(expected)} expected {expected}; {len(shown_a)} shown, by title {got_titles}; "
          f"still waiting {document(a / 'pending-end-a.json')}")
    check("A4", "every notice says where to see more (tur-diag heal history)",
          bool(shown_a) and all("tur-diag heal history" in notice.get("body", "") for notice in shown_a),
          f"{sum('tur-diag heal history' in notice.get('body', '') for notice in shown_a)} of {len(shown_a)}")
    shown_b = recorded(b / "notify/recorded.jsonl")
    check("A5", "after the restart, with no session yet, 'the problem you reported is fixed' waited, and was "
          "shown at the next sign-in, once",
          sb.get("recorded_before_sign_in_b") in ("0", "") and len(shown_b) == 1
          and shown_b[0].get("body", "").startswith("The problem you reported is fixed")
          and document(b / "pending-b.json") == [],
          f"before sign-in {sb.get('recorded_before_sign_in_b')}; shown {[n.get('body', '')[:80] for n in shown_b]}")
    history_a = document(a / "history-a.json") or []
    whos = {entry.get("who") for entry in history_a}
    kinds_h = {entry.get("kind") for entry in history_a}
    ordered = [entry.get("at_utc") for entry in history_a]
    check("A6", "the history lists, newest first, what TUR did by itself, what P@x, the person and the "
          "person as administrator did, and the reports offered, sent and not sent",
          {"TUR", "you", "you, as administrator"} <= whos
          and any((who or "").startswith("P@x") for who in whos)
          and {"repaired", "held", "undone", "report-offered", "report-sent", "report-not-sent"} <= kinds_h
          and ordered == sorted(ordered, reverse=True),
          f"{len(history_a)} entries; who {sorted(w or '-' for w in whos)}; kinds {sorted(kinds_h)}")
    check("A7", "P@x read the history and found what TUR repaired by itself",
          f"Repaired: {titles[SERVICE]}" in sent, sent[:0] + "searched the tool results sent to the provider")
    check("Z1", "the timer was put back", sb.get("timer_restored") == "active", sb.get("timer_restored"),
          required=False)

    passed = all(c["passed"] for c in checks if c["required"])
    (out / "heal.json").write_text(json.dumps({"passed": passed, "checks": checks, "run": seen}, indent=2) + "\n")
    lines = [f"### Self-healing in a booted guest: {'passed' if passed else '**FAILED**'}", "",
             "| | Check | Result | Detail |", "|---|---|---|---|"]
    for c in checks:
        result = "pass" if c["passed"] else ("**FAIL**" if c["required"] else "not required")
        lines.append(f"| {c['id']} | {c['check']} | {result} | {c['detail'].replace('|', '/')[:300]} |")
    (out / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())

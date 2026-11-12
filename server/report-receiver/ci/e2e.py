#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""End-to-end proof of problem reports (ADR-0024), run by the `reports` workflow.

The receiver runs in its container (the image built from
server/report-receiver/Containerfile), on the runner's network, with a
stand-in for the GitHub API (standin_github.py) that records every call. The
client is tur-diag, built in the same job, run as an ordinary user. Nothing
here contacts the real GitHub or any other server.

What is checked, scenario by scenario, is written to OUT/results.json and
OUT/summary.md:

  client-refusals   no receiver configured; --to not the receiver; no confirmation
  good-report       a report is sent, filed as one issue, listed, its state read
  duplicate         a second report with the same signature becomes a comment
  planted           planted personal data never reaches the stand-in, the logs
                    or the answers
  refusals          malformed, oversized, wrong type, no report.json, technical
                    profile, unknown token, unknown path, wrong method
  tracker-down      the stand-in is stopped; the report waits; filed after restart
  fixed             mark-fixed; the person sees "fixed in"
  at-rest           a bundle decrypts to the bytes sent, before the sweep only
  retention         sweep at +89 days keeps, +91 days deletes bundles, +366
                    days deletes records
  rate-limits       per address and global, on a second receiver
  logs              no planted value, token, client address or host name in any log

    e2e.py --tur-diag PATH --image IMAGE --material DIR --work DIR --out DIR
"""

import argparse
import hashlib
import json
import secrets
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

RECEIVER_PORT = 8470
LIMITS_PORT = 8471
STANDIN_PORT = 9100
REPOSITORY = "tur-ci/problem-reports"
CONTENT_TYPE = "application/vnd.tur.diag-bundle+zstd"
UID = "10001"
DAY = 86_400
HERE = Path(__file__).resolve().parent

# Personal data the client is given in a report's description. tur-diag's own
# redaction does not recognise these (diagnostics.md, "Known gaps"); the
# receiver must.
CLIENT_PLANTED = [
    ("email", "ci.person@example.org"),
    ("token", "ghp_CiPlantedToken0123456789abcdefABCDEF"),
    ("windows profile", "Rosa Klein"),
    ("secret", "Pa55word1Planted"),
]
CLIENT_DESCRIPTION = (
    "Sound stops after resume. Write to ci.person@example.org. "
    "Wine said C:\\Users\\Rosa Klein\\Desktop is missing. "
    "git used ghp_CiPlantedToken0123456789abcdefABCDEF and the share "
    "//admin:Pa55word1Planted@nas.example.net/music."
)


class Checks:
    def __init__(self):
        self.results = []

    def check(self, scenario, name, ok, detail=""):
        detail = detail if isinstance(detail, str) else json.dumps(detail, default=str)
        self.results.append({"scenario": scenario, "check": name, "passed": bool(ok), "detail": detail[:2000]})
        mark = "ok  " if ok else "FAIL"
        print(f"{mark} [{scenario}] {name}" + ("" if ok else f": {detail[:500]}"), flush=True)
        return bool(ok)

    def equal(self, scenario, name, actual, expected):
        return self.check(scenario, name, actual == expected, f"expected {expected!r}, got {actual!r}")

    def failed(self):
        return [result for result in self.results if not result["passed"]]


def run(command, check=False, **kwargs):
    kwargs.setdefault("capture_output", True)
    kwargs.setdefault("text", True)
    return subprocess.run(command, check=check, **kwargs)


def sudo(*command):
    return run(["sudo", *command], check=True)


def wait_for(condition, seconds, interval=0.5):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(interval)
    return condition()


def request(method, url, data=None, headers=None):
    """An HTTP request with urllib: (status, body text, headers)."""
    req = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, response.read().decode("utf-8", "replace"), dict(response.headers)
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace"), dict(error.headers)
    except (urllib.error.URLError, OSError) as error:
        return 0, str(error), {}


def upload(port, path=None, data=None, content_type=CONTENT_TYPE, forwarded=None, work=None):
    """An upload with curl, as tur-diag does it: (status, body, headers)."""
    body_file = work / f"answer-{secrets.token_hex(4)}"
    header_file = work / f"headers-{secrets.token_hex(4)}"
    command = ["curl", "--silent", "--show-error", "--max-time", "120", "--request", "POST",
               "--output", str(body_file), "--dump-header", str(header_file),
               "--write-out", "%{http_code}"]
    if content_type:
        command += ["--header", f"Content-Type: {content_type}"]
    if forwarded:
        command += ["--header", f"X-Forwarded-For: {forwarded}"]
    if path is not None:
        command += ["--data-binary", f"@{path}"]
    else:
        source = work / f"upload-{secrets.token_hex(4)}"
        source.write_bytes(data)
        command += ["--data-binary", f"@{source}"]
    command.append(f"http://127.0.0.1:{port}/v1/reports")
    result = run(command)
    status = int(result.stdout.strip() or 0) if result.stdout.strip().isdigit() else 0
    body = body_file.read_text(errors="replace") if body_file.exists() else result.stderr
    headers = {}
    if header_file.exists():
        for line in header_file.read_text(errors="replace").splitlines():
            if ":" in line:
                key, value = line.split(":", 1)
                headers[key.strip().lower()] = value.strip()
    return status, body, headers


def code_of(body):
    try:
        return json.loads(body).get("code")
    except ValueError:
        return None


class Standin:
    def __init__(self, work, token_file):
        self.calls_path = work / "standin-calls.jsonl"
        self.state_path = work / "standin-state.json"
        self.token_file = token_file
        self.process = None

    def start(self):
        self.process = subprocess.Popen(
            [sys.executable, str(HERE / "standin_github.py"), "--port", str(STANDIN_PORT),
             "--repository", REPOSITORY, "--token-file", str(self.token_file),
             "--calls", str(self.calls_path), "--state", str(self.state_path)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not wait_for(lambda: port_open(STANDIN_PORT), 15):
            raise RuntimeError("the stand-in did not start")

    def stop(self):
        if self.process:
            self.process.terminate()
            self.process.wait(10)
            self.process = None
        wait_for(lambda: not port_open(STANDIN_PORT), 10)

    def calls(self):
        if not self.calls_path.exists():
            return []
        return [json.loads(line) for line in self.calls_path.read_text().splitlines() if line.strip()]

    def issues(self):
        return [call for call in self.calls() if call["path"] == f"/repos/{REPOSITORY}/issues" and call["authorized"]]

    def comments(self):
        return [call for call in self.calls() if call["path"].endswith("/comments") and call["authorized"]]


def port_open(port):
    with socket.socket() as probe:
        probe.settimeout(0.5)
        return probe.connect_ex(("127.0.0.1", port)) == 0


class Receiver:
    def __init__(self, work, name, port, image, token, limits):
        self.name = name
        self.port = port
        self.image = image
        self.root = work / name
        self.data = self.root / "data"
        etc = self.root / "etc"
        (etc / "keys").mkdir(parents=True)
        self.data.mkdir(parents=True)
        (etc / "keys" / "ci.key").write_text(secrets.token_hex(32) + "\n")
        (etc / "github-token").write_text(token + "\n")
        config = {
            "listen": f"127.0.0.1:{port}",
            "public_url": f"http://127.0.0.1:{port}",
            "data_dir": "/var/lib/tur-report-receiver",
            "keys_dir": "/etc/tur-report-receiver/keys",
            "current_key": "ci",
            "limits": limits,
            "github": {
                "api_url": f"http://127.0.0.1:{STANDIN_PORT}",
                "repository": REPOSITORY,
                "token_file": "/etc/tur-report-receiver/github-token",
                "labels": ["problem-report"],
            },
            "timing": {
                "file_interval_seconds": 1,
                "retry_initial_seconds": 2,
                "retry_max_seconds": 4,
                "sweep_interval_seconds": 3600,
                "upload_timeout_seconds": 30,
            },
        }
        (etc / "config.json").write_text(json.dumps(config, indent=2))
        sudo("chown", "-R", f"{UID}:{UID}", str(etc), str(self.data))
        sudo("chmod", "0700", str(etc / "keys"), str(self.data))
        sudo("chmod", "0400", str(etc / "keys" / "ci.key"), str(etc / "github-token"))
        sudo("chmod", "0644", str(etc / "config.json"))
        self.etc = etc

    def start(self):
        run(["docker", "rm", "--force", self.name])
        result = run([
            "docker", "run", "--detach", "--name", self.name, "--network", "host",
            "--read-only", "--cap-drop", "all", "--security-opt", "no-new-privileges",
            "--user", f"{UID}:{UID}",
            "--volume", f"{self.etc / 'config.json'}:/etc/tur-report-receiver/config.json:ro",
            "--volume", f"{self.etc / 'keys'}:/etc/tur-report-receiver/keys:ro",
            "--volume", f"{self.etc / 'github-token'}:/etc/tur-report-receiver/github-token:ro",
            "--volume", f"{self.data}:/var/lib/tur-report-receiver:rw",
            self.image, "serve", "--config", "/etc/tur-report-receiver/config.json",
        ])
        if result.returncode != 0:
            raise RuntimeError(f"docker run failed: {result.stderr}")
        started = wait_for(
            lambda: request("GET", f"http://127.0.0.1:{self.port}/v1/reports/{'0' * 64}")[0] == 404, 30)
        if not started:
            raise RuntimeError(f"{self.name} did not start:\n{self.logs()}")

    def logs(self):
        result = run(["docker", "logs", self.name])
        return result.stdout + result.stderr

    def admin(self, *arguments):
        return run(["docker", "exec", self.name, "tur-report-receiver", *arguments,
                    "--config", "/etc/tur-report-receiver/config.json"])

    def files(self, directory):
        result = run(["sudo", "ls", "-1", str(self.data / directory)])
        return [line for line in result.stdout.splitlines() if line.strip()]

    def stop(self):
        run(["docker", "rm", "--force", self.name])


class Test:
    def __init__(self, args):
        self.args = args
        self.work = args.work
        self.out = args.out
        self.checks = Checks()
        self.tur_diag = str(args.tur_diag.resolve())
        self.state_dir = self.work / "client-state"
        self.config_good = self.work / "client.conf"
        self.config_empty = self.work / "client-empty.conf"
        self.receiver_url = f"http://127.0.0.1:{RECEIVER_PORT}"
        self.config_good.write_text(f"# the CI receiver\nRECEIVER_URL={self.receiver_url}\n")
        self.config_empty.write_text("# TUR has no receiver yet\nRECEIVER_URL=\n")
        self.github_token = "standin-" + secrets.token_hex(16)
        token_file = self.work / "standin-token"
        token_file.write_text(self.github_token + "\n")
        self.standin = Standin(self.work, token_file)
        self.receiver = None
        self.limits = None
        self.tokens = []
        self.answers = []
        self.reports = {}
        self.admin_output = ""
        planted = json.loads((args.material / "planted-values.json").read_text())
        self.planted = [(item["category"], item["value"]) for item in planted] + CLIENT_PLANTED
        self.host_names = sorted({socket.gethostname(), socket.gethostname().split(".")[0]})

    # ------------------------------------------------------------ helpers

    def diag(self, *arguments, stdin=subprocess.DEVNULL):
        return run([self.tur_diag, *arguments], stdin=stdin)

    def prepare(self, title, code, component="audio", description="Prepared by the reports workflow."):
        result = self.diag("report", "prepare", "--title", title, "--component", component,
                           "--error-code", code, "--description", description, "--since-boot",
                           "--json", "--state-dir", str(self.state_dir), "--config", str(self.config_good))
        if result.returncode != 0:
            raise RuntimeError(f"prepare failed: {result.stderr}")
        return json.loads(result.stdout)

    def send(self, prepared, *extra, config=None, to=None, confirm=True):
        arguments = ["report", "send", "--to", to or self.receiver_url, "--state-dir", str(self.state_dir)]
        if config is not None:
            arguments += ["--config", str(config)]
        if confirm:
            arguments.append("--yes")
        arguments += [*extra, prepared["prepared_file"]]
        return self.diag(*arguments)

    def status(self, report_id):
        result = self.diag("report", "status", report_id, "--json", "--state-dir", str(self.state_dir),
                           "--config", str(self.config_good))
        if result.returncode != 0:
            return {"error": result.stderr}
        return json.loads(result.stdout)

    def received_count(self):
        return self.receiver.logs().count("event=received")

    # ---------------------------------------------------------- scenarios

    def client_refusals(self):
        name = "client-refusals"
        prepared = self.prepare("Client refusals", "TUR-AUDIO-CI-001", description=CLIENT_DESCRIPTION)
        self.reports["first"] = prepared
        check = self.checks.check
        check(name, "the preview says nothing was sent", prepared["sent"] is False)
        check(name, "the preview lists report.json among the files",
              any(file["path"] == "report.json" for file in prepared["bundle"]["files"]))
        check(name, "the prepared bundle is within 8 MiB", prepared["bundle"]["bytes"] <= 8 * 1024 * 1024,
              prepared["bundle"]["bytes"])
        before = (len(self.standin.calls()), self.received_count())

        result = self.send(prepared, config=self.config_empty)
        check(name, "with RECEIVER_URL empty, sending is refused (TUR-DIAG-REPORT-002)",
              result.returncode == 1 and "TUR-DIAG-REPORT-002" in result.stderr, result.stderr)
        result = self.send(prepared, config=None)
        check(name, "with no configuration file at all, sending is refused (TUR-DIAG-REPORT-002)",
              result.returncode == 1 and "TUR-DIAG-REPORT-002" in result.stderr, result.stderr)
        result = self.send(prepared, config=self.config_good, confirm=False)
        check(name, "without --yes and without a terminal, sending is refused (TUR-DIAG-REPORT-004)",
              result.returncode == 1 and "TUR-DIAG-REPORT-004" in result.stderr, result.stderr)
        result = self.send(prepared, config=self.config_good, to="http://127.0.0.1:9999")
        check(name, "--to another address is refused (TUR-DIAG-REPORT-003)",
              result.returncode == 1 and "TUR-DIAG-REPORT-003" in result.stderr, result.stderr)
        time.sleep(2)
        after = (len(self.standin.calls()), self.received_count())
        self.checks.equal(name, "nothing reached the receiver or the stand-in", after, before)
        check(name, "the prepared file is still there", Path(prepared["prepared_file"]).exists())

    def good_report(self):
        name = "good-report"
        prepared = self.reports["first"]
        result = self.send(prepared, "--json", config=self.config_good)
        if not self.checks.check(name, "tur-diag report send succeeds with --yes", result.returncode == 0,
                                 result.stderr):
            return
        sent = json.loads(result.stdout)
        self.reports["first-id"] = sent["report_id"]
        self.checks.equal(name, "the receipt's state is received", sent["state"], "received")
        self.checks.check(name, "the prepared file was removed after sending",
                          not Path(prepared["prepared_file"]).exists())
        issues = wait_for(lambda: self.standin.issues(), 30)
        self.checks.equal(name, "one issue was filed", len(issues), 1)
        if issues:
            body = json.loads(issues[0]["body"])
            self.checks.check(name, "its title names the code and component",
                              body["title"].startswith("[TUR-AUDIO-CI-001] audio "), body["title"])
            self.checks.check(name, "its body names the report", sent["report_id"] in body["body"])
            self.checks.equal(name, "it carries the configured label", body["labels"], ["problem-report"])
            self.checks.check(name, "the body is a summary, not the bundle",
                              "checksums.sha256" not in body["body"] and len(body["body"]) < 60_000)
        state = wait_for(lambda: self.status(sent["report_id"]).get("state") == "filed", 15)
        self.checks.check(name, "tur-diag report status says filed", state, self.status(sent["report_id"]))
        listed = self.diag("report", "list", "--json", "--state-dir", str(self.state_dir))
        document = json.loads(listed.stdout)
        self.checks.equal(name, "tur-diag report list shows the report",
                          [report["report_id"] for report in document["sent"]], [sent["report_id"]])
        self.checks.check(name, "the list shows no status token or status address",
                          "status_url" not in listed.stdout and "/v1/reports/" not in listed.stdout)
        record = json.loads((self.state_dir / "sent" / f"{sent['report_id']}.json").read_text())
        self.tokens.append(record["status_url"].rsplit("/", 1)[-1])
        mode = (self.state_dir / "sent" / f"{sent['report_id']}.json").stat().st_mode & 0o777
        self.checks.equal(name, "the local record is readable by its owner only", oct(mode), "0o600")

    def duplicate(self):
        name = "duplicate"
        prepared = self.prepare("Still no sound", "TUR-AUDIO-CI-001")
        result = self.send(prepared, "--json", config=self.config_good)
        if not self.checks.check(name, "the second report is sent", result.returncode == 0, result.stderr):
            return
        comments = wait_for(lambda: self.standin.comments(), 30)
        self.checks.equal(name, "it became one comment", len(comments), 1)
        if comments:
            self.checks.equal(name, "on issue 1", comments[0]["path"], f"/repos/{REPOSITORY}/issues/1/comments")
        self.checks.equal(name, "no second issue was opened", len(self.standin.issues()), 1)
        # The same with bundles sent as they are.
        for part in ("a", "b"):
            status, body, _ = upload(RECEIVER_PORT, path=self.args.material / f"duplicate-{part}.tar.zst",
                                     work=self.work)
            self.answers.append(body)
            self.checks.equal(name, f"raw duplicate {part} is accepted", status, 202)
        wait_for(lambda: len(self.standin.comments()) >= 2, 30)
        self.checks.equal(name, "the raw pair is one issue and one comment",
                          (len(self.standin.issues()), len(self.standin.comments())), (2, 2))

    def planted(self):
        name = "planted"
        status, body, _ = upload(RECEIVER_PORT, path=self.args.material / "planted.tar.zst", work=self.work)
        self.answers.append(body)
        if not self.checks.equal(name, "the planted bundle is accepted", status, 202):
            return
        receipt = json.loads(body)
        self.reports["planted-id"] = receipt["report_id"]
        self.tokens.append(receipt["status_token"])
        filed = wait_for(lambda: any("TUR-AUDIO-RESUME-001" in json.loads(call["body"]).get("title", "")
                                     for call in self.standin.issues()), 30)
        self.checks.check(name, "its summary was filed", filed)
        everything = "\n".join(call["body"] for call in self.standin.calls())
        lower = everything.lower()
        leaked = [f"{category}: {value}" for category, value in self.planted if value.lower() in lower]
        self.checks.equal(name, "no planted value reached the stand-in (bundle and client description)",
                          leaked, [])
        hosts = [host for host in self.host_names if len(host) >= 4 and host.lower() in lower]
        self.checks.equal(name, "the runner's host name never reached the stand-in", hosts, [])
        self.checks.check(name, "the summaries keep what is useful", "TUR-BOOT-GATE-003" in everything
                          and "Failed to start tur-update-stage.service" in everything)

    def refusals(self):
        name = "refusals"
        cases = [
            ("random bytes", dict(data=secrets.token_bytes(2048)), 422, "TUR-DIAG-RECEIVER-005"),
            ("a bundle without report.json", dict(path=self.args.material / "unsigned.tar.zst"), 422,
             "TUR-DIAG-RECEIVER-006"),
            ("a report with the technical profile", dict(path=self.args.material / "technical.tar.zst"), 422,
             "TUR-DIAG-RECEIVER-007"),
            ("a good bundle of the wrong media type",
             dict(path=self.args.material / "duplicate-a.tar.zst", content_type="application/octet-stream"),
             415, "TUR-DIAG-RECEIVER-004"),
            ("9 MiB", dict(data=secrets.token_bytes(9 * 1024 * 1024)), 413, "TUR-DIAG-RECEIVER-003"),
        ]
        stored_before = len(self.receiver.files("bundles"))
        for label, options, status, code in cases:
            got_status, body, _ = upload(RECEIVER_PORT, work=self.work, **options)
            self.answers.append(body)
            self.checks.equal(name, f"{label}: {status} {code}", (got_status, code_of(body)), (status, code))
        detail = json.loads(upload(RECEIVER_PORT, data=b"not a bundle", work=self.work)[1]).get("detail_code")
        self.checks.equal(name, "a damaged upload names the bundle code", detail, "TUR-DIAG-BUNDLE-002")
        for label, method, path, status, code in [
            ("an unknown token", "GET", "/v1/reports/" + "0" * 64, 404, "TUR-DIAG-RECEIVER-011"),
            ("a malformed token", "GET", "/v1/reports/../../etc/passwd", 404, None),
            ("an unknown path", "GET", "/v1/other", 404, "TUR-DIAG-RECEIVER-001"),
            ("a wrong method", "DELETE", "/v1/reports", 405, "TUR-DIAG-RECEIVER-002"),
        ]:
            got_status, body, _ = request(method, f"http://127.0.0.1:{RECEIVER_PORT}{path}")
            self.answers.append(body)
            if code:
                self.checks.equal(name, f"{label}: {status} {code}", (got_status, code_of(body)), (status, code))
            else:
                self.checks.equal(name, f"{label}: {status}", got_status, status)
        self.checks.equal(name, "nothing refused was stored", len(self.receiver.files("bundles")), stored_before)

    def tracker_down(self):
        name = "tracker-down"
        self.standin.stop()
        self.checks.check(name, "the stand-in is down", not port_open(STANDIN_PORT))
        prepared = self.prepare("While GitHub is down", "TUR-NETWORK-CI-001", component="network")
        result = self.send(prepared, "--json", config=self.config_good)
        if not self.checks.check(name, "the report is accepted while the tracker is down",
                                 result.returncode == 0, result.stderr):
            self.standin.start()
            return
        report_id = json.loads(result.stdout)["report_id"]
        time.sleep(8)
        self.checks.equal(name, "its state stays received", self.status(report_id).get("state"), "received")
        logs = self.receiver.logs()
        self.checks.check(name, "the receiver logged the failed attempts",
                          "event=filing-failed" in logs and "reason=network" in logs)
        self.standin.start()
        filed = wait_for(lambda: any("TUR-NETWORK-CI-001" in json.loads(call["body"]).get("title", "")
                                     for call in self.standin.issues()), 40)
        self.checks.check(name, "after the restart it is filed", filed)
        state = wait_for(lambda: self.status(report_id).get("state") == "filed", 15)
        self.checks.check(name, "and its state is filed", state)

    def fixed(self):
        name = "fixed"
        result = self.receiver.admin("mark-fixed", "--issue", "1", "--version", "0.2.1")
        self.admin_output += result.stdout + result.stderr
        self.checks.check(name, "mark-fixed marks the two reports of issue 1",
                          result.returncode == 0 and "2 reports" in result.stdout, result.stdout + result.stderr)
        state = self.status(self.reports.get("first-id", ""))
        self.checks.equal(name, "tur-diag report status shows fixed in 0.2.1",
                          (state.get("state"), state.get("fixed_in")), ("fixed", "0.2.1"))

    def at_rest(self):
        name = "at-rest"
        report_id = self.reports.get("planted-id")
        if not report_id:
            self.checks.check(name, "the planted report exists", False)
            return
        stored = self.receiver.files("bundles")
        raw = run(["sudo", "cat", str(self.receiver.data / "bundles" / f"{report_id}.bin")], text=False).stdout
        original = (self.args.material / "planted.tar.zst").read_bytes()
        self.checks.check(name, "the stored bundle is sealed, not the upload",
                          raw.startswith(b"TURR1") and raw != original and original[:64] not in raw)
        self.checks.check(name, "every report has a stored bundle", len(stored) >= 6, stored)
        result = self.receiver.admin("decrypt", "--report", report_id, "--output",
                                     "/var/lib/tur-report-receiver/decrypted.bin")
        self.admin_output += result.stdout + result.stderr
        plain = run(["sudo", "cat", str(self.receiver.data / "decrypted.bin")], text=False).stdout
        sudo("rm", "-f", str(self.receiver.data / "decrypted.bin"))
        self.checks.equal(name, "decrypt gives back exactly the bytes that were sent",
                          hashlib.sha256(plain).hexdigest(), hashlib.sha256(original).hexdigest())

    def retention(self):
        name = "retention"
        now = int(time.time())
        stored = len(self.receiver.files("bundles"))
        result = self.receiver.admin("sweep", "--now", str(now + 89 * DAY))
        self.admin_output += result.stdout + result.stderr
        first = json.loads(result.stdout or "{}")
        self.checks.equal(name, "at +89 days nothing is deleted", first.get("bundles_deleted"), 0)
        self.checks.equal(name, "at +89 days every bundle is still there", len(self.receiver.files("bundles")),
                          stored)
        result = self.receiver.admin("sweep", "--now", str(now + 91 * DAY))
        self.admin_output += result.stdout + result.stderr
        second = json.loads(result.stdout or "{}")
        self.checks.equal(name, "at +91 days every bundle is deleted", second.get("bundles_deleted"), stored)
        self.checks.equal(name, "the bundle directory is empty", self.receiver.files("bundles"), [])
        result = self.receiver.admin("decrypt", "--report", self.reports.get("planted-id", "R-0"), "--output",
                                     "/var/lib/tur-report-receiver/late.bin")
        self.admin_output += result.stdout + result.stderr
        self.checks.check(name, "a deleted bundle cannot be decrypted",
                          result.returncode != 0 and "retention" in result.stderr, result.stderr)
        token = self.tokens[0] if self.tokens else "0" * 64
        status, body, _ = request("GET", f"http://127.0.0.1:{RECEIVER_PORT}/v1/reports/{token}")
        self.answers.append(body)
        self.checks.equal(name, "the state can still be read", status, 200)
        result = self.receiver.admin("sweep", "--now", str(now + 366 * DAY))
        self.admin_output += result.stdout + result.stderr
        third = json.loads(result.stdout or "{}")
        self.checks.equal(name, "at +366 days every record is deleted", third.get("records_deleted"), stored)
        status, body, _ = request("GET", f"http://127.0.0.1:{RECEIVER_PORT}/v1/reports/{token}")
        self.checks.equal(name, "and the state is gone", (status, code_of(body)), (404, "TUR-DIAG-RECEIVER-011"))
        for directory in ("reports", "tokens", "summaries"):
            self.checks.equal(name, f"{directory}/ is empty", self.receiver.files(directory), [])

    def rate_limits(self):
        name = "rate-limits"
        expected = [
            ("203.0.113.10", 422, "TUR-DIAG-RECEIVER-005"),
            ("203.0.113.10", 422, "TUR-DIAG-RECEIVER-005"),
            ("203.0.113.10", 429, "TUR-DIAG-RECEIVER-008"),
            ("203.0.113.11", 422, "TUR-DIAG-RECEIVER-005"),
            ("203.0.113.11", 422, "TUR-DIAG-RECEIVER-005"),
            ("203.0.113.12", 429, "TUR-DIAG-RECEIVER-009"),
        ]
        for address, status, code in expected:
            got, body, headers = upload(LIMITS_PORT, data=b"not a bundle", forwarded=address, work=self.work)
            self.answers.append(body)
            self.checks.equal(name, f"from {address}: {status} {code}", (got, code_of(body)), (status, code))
            if status == 429:
                self.checks.check(name, f"from {address}: Retry-After is set", "retry-after" in headers, headers)
        got, body, _ = upload(LIMITS_PORT, data=b"x", forwarded="not an address", work=self.work)
        self.checks.equal(name, "a garbage X-Forwarded-For counts as the proxy itself, which is limited too",
                          got in (422, 429), True)

    def logs(self):
        name = "logs"
        texts = {
            "receiver": self.receiver.logs(),
            "limits receiver": self.limits.logs(),
            "administrator commands": self.admin_output,
        }
        (self.out / "receiver.log").write_text(texts["receiver"])
        (self.out / "receiver-limits.log").write_text(texts["limits receiver"])
        for label, text in texts.items():
            lower = text.lower()
            leaked = [f"{category}: {value}" for category, value in self.planted if value.lower() in lower]
            self.checks.equal(name, f"no planted value in the {label} log", leaked, [])
            self.checks.equal(name, f"no status token in the {label} log",
                              [token[:8] for token in self.tokens if token in text], [])
            self.checks.check(name, f"no GitHub token in the {label} log", self.github_token not in text)
            self.checks.check(name, f"no client address in the {label} log", "203.0.113." not in text)
            hosts = [host for host in self.host_names if len(host) >= 4 and host.lower() in lower]
            self.checks.equal(name, f"no host name of the runner in the {label} log", hosts, [])
        self.checks.check(name, "the receiver did log its events",
                          "event=received" in texts["receiver"] and "event=filed" in texts["receiver"])
        answers = "\n".join(self.answers).lower()
        leaked = [f"{category}: {value}" for category, value in self.planted if value.lower() in answers]
        self.checks.equal(name, "no planted value in any answer of the receiver", leaked, [])
        calls = self.standin.calls()
        self.checks.check(name, "every call to the stand-in carried the token", all(call["authorized"] for call in calls))
        self.checks.check(name, "every call named the API version",
                          all(call["api_version"] == "2022-11-28" for call in calls))
        paths = sorted({call["path"].rsplit("/", 2)[0] if call["path"].endswith("/comments") else call["path"]
                        for call in calls})
        self.checks.equal(name, "the receiver called only the issues of the configured repository", paths,
                          [f"/repos/{REPOSITORY}/issues"])
        self.checks.check(name, "no call carried the GitHub token in its body",
                          all(self.github_token not in call["body"] for call in calls))

    # -------------------------------------------------------------- driver

    def main(self):
        self.standin.start()
        self.receiver = Receiver(self.work, "tur-receiver", RECEIVER_PORT, self.args.image, self.github_token,
                                 {"uploads_per_address_per_hour": 100, "uploads_per_hour": 400})
        self.limits = Receiver(self.work, "tur-receiver-limits", LIMITS_PORT, self.args.image, self.github_token,
                               {"uploads_per_address_per_hour": 2, "uploads_per_hour": 4})
        try:
            self.receiver.start()
            self.limits.start()
            for scenario in (self.client_refusals, self.good_report, self.duplicate, self.planted, self.refusals,
                             self.tracker_down, self.fixed, self.at_rest, self.retention, self.rate_limits,
                             self.logs):
                try:
                    scenario()
                except Exception as error:  # noqa: BLE001 - a scenario's crash is a failed check
                    self.checks.check(scenario.__name__.replace("_", "-"), "the scenario ran to the end", False,
                                      repr(error))
        finally:
            for receiver in (self.receiver, self.limits):
                if receiver:
                    (self.out / f"{receiver.name}.final.log").write_text(receiver.logs())
                    receiver.stop()
            self.standin.stop()
            if self.standin.calls_path.exists():
                (self.out / "standin-calls.jsonl").write_text(self.standin.calls_path.read_text())
        return self.report()

    def report(self):
        failed = self.checks.failed()
        document = {"passed": len(self.checks.results) - len(failed), "failed": len(failed),
                    "real_github_called": False, "checks": self.checks.results}
        (self.out / "results.json").write_text(json.dumps(document, indent=2))
        lines = ["### Problem reports end to end", "",
                 f"{document['passed']} checks passed, {document['failed']} failed. The GitHub API was a "
                 "local stand-in; nothing was sent anywhere else.", "",
                 "| Scenario | Checks | Failed |", "|---|---|---|"]
        scenarios = []
        for result in self.checks.results:
            if result["scenario"] not in scenarios:
                scenarios.append(result["scenario"])
        for scenario in scenarios:
            mine = [result for result in self.checks.results if result["scenario"] == scenario]
            bad = [result for result in mine if not result["passed"]]
            lines.append(f"| {scenario} | {len(mine)} | {len(bad)} |")
        if failed:
            lines += ["", "Failed checks:", ""]
            for result in failed:
                lines.append(f"- **{result['scenario']}**: {result['check']} — `{result['detail'][:300]}`")
        (self.out / "summary.md").write_text("\n".join(lines) + "\n")
        print(f"\n{document['passed']} checks passed, {document['failed']} failed", flush=True)
        return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-diag", type=Path, required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--material", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=True)
    args.out.mkdir(parents=True, exist_ok=True)
    return Test(args).main()


if __name__ == "__main__":
    sys.exit(main())

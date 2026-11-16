#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The report loop as deployed: run by the dispatch-only `reports-deployed` workflow.

The runner is a fresh Ubuntu 24.04 machine and plays the receiver's server:
deploy/install.sh, the one documented command, installs the receiver from
the image archive CI built (Podman, systemd, Caddy with the server's own
certificate from a CA made here). A stand-in for GitHub's API records what
is filed. The client is TUR's pinned base image (Fedora Kinoite) run as a
container, with tur-diag built in the same job, an ordinary user, Fedora's
curl, trust store and crypto policy. Nothing here contacts the real GitHub
or any server outside this machine.

Scenarios, each written to OUT/results.json and OUT/summary.md:

  deploy        install.sh: packages, image (checked against its .sha256), key,
                token from a file, configuration, unit, Caddy, local and HTTPS
                checks; plain HTTP redirects
  secrets       token file 0400 and uid 10001, in no configuration, command
                line or log; key 0400
  pax-preview   P@x's diag_report_prepare command line, as its adapter builds
                it: a preview, nothing saved or sent, a hand-over to the person
  declined      the hand-over answered "no": nothing sent, the report kept
  sandbox       send from a container with P@x's sandbox host name: refused
                (TUR-DIAG-REPORT-013), nothing reaches the receiver
  person-sends  the hand-over answered "yes" on a terminal: sent over HTTPS
                through Caddy, recorded, its origin kept
  redaction     the bundle as received (decrypted on the server) carries no
                host name, account or home path of the client; the filed
                summary carries none of the planted values, including those
                tur-diag cannot recognise
  filing        one issue with the code, component, version, label and report
                ID; a second report of the same signature is a comment
  fixed         mark-fixed on the server: report status says fixed in 0.3.0;
                an update whose fixes.json lists the code makes `report fixed`
                say "the problem you reported is fixed"
  load          two reports of 56 MiB unpacked at once through Caddy: both
                accepted; the container's peak memory and CPU time measured
  logs          no planted value, token or client address in the journal
  redeploy      install.sh again without image or token: key, token and
                reports kept, service answering

    deployed_e2e.py --tur-diag PATH --image-archive FILE --client-image REF \\
                    --material DIR --work DIR --out DIR
"""

import argparse
import concurrent.futures
import hashlib
import json
import secrets
import shlex
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEPLOY = HERE.parent / "deploy"
ADAPTERS = HERE.parent.parent.parent / "services" / "tur-diagnostics" / "agent-tools.adapter.json"
DOMAIN = "reports.tur-ci.test"
URL = f"https://{DOMAIN}"
REPOSITORY = "tur-ci/problem-reports"
STANDIN_PORT = 9100
ETC = Path("/etc/tur-report-receiver")
DATA = Path("/var/lib/tur-report-receiver")
UNIT = "tur-report-receiver"
CONTAINER = "tur-report-receiver"
CLIENT = "tur-ci-client"
PERSON = "rosaklein"
CLIENT_HOST = "planted-host-4711"
PAX_SANDBOX_HOST = "tur-pax-sandbox"
CODE = "TUR-AUDIO-RESUME-001"
PROMPT = "Type yes to send:"

# Personal values in the person's own description. tur-diag replaces the
# account, the host name and the home path; the rest only the receiver
# recognises (diagnostics.md, "Known gaps").
CLIENT_PLANTED = [
    ("account", PERSON),
    ("host name", CLIENT_HOST),
    ("email", "rosa.klein@example.org"),
    ("token", "ghp_CiDeployedToken0123456789abcdefABCDEF"),
    ("windows profile", "Rosa Klein"),
    ("secret", "Pa55wordDeployed1"),
]
DESCRIPTION = (
    f"The sound stops after resume for {PERSON} on {CLIENT_HOST}. Write to rosa.klein@example.org. "
    f"The player kept /home/{PERSON}/Music/a.flac open. Wine said C:\\Users\\Rosa Klein\\Desktop "
    "is missing; git used ghp_CiDeployedToken0123456789abcdefABCDEF and the share "
    "//admin:Pa55wordDeployed1@nas.example.net/music."
)


class Checks:
    def __init__(self):
        self.results = []
        self.measurements = {}

    def check(self, scenario, name, ok, detail=""):
        detail = detail if isinstance(detail, str) else json.dumps(detail, default=str)
        self.results.append({"scenario": scenario, "check": name, "passed": bool(ok), "detail": detail[:3000]})
        print(f"{'ok  ' if ok else 'FAIL'} [{scenario}] {name}" + ("" if ok else f": {detail[:800]}"), flush=True)
        return bool(ok)

    def failed(self):
        return [result for result in self.results if not result["passed"]]


def run(command, check=False, **kwargs):
    kwargs.setdefault("capture_output", True)
    kwargs.setdefault("text", True)
    return subprocess.run(command, check=check, **kwargs)


def sudo(*command, **kwargs):
    return run(["sudo", *command], **kwargs)


def wait_for(condition, seconds, interval=1.0):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(interval)
    return condition()


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Test:
    def __init__(self, args):
        self.args = args
        self.work = args.work.resolve()
        self.out = args.out
        self.checks = Checks()
        self.token = "github_pat_CI" + secrets.token_hex(20)
        self.token_file = self.work / "github-token"
        self.calls = self.work / "standin-calls.jsonl"
        self.standin = None
        self.exchange = self.work / "exchange"
        self.gateway = None
        self.client_address = None
        self.reports = []

    # -------------------------------------------------------------- helpers

    def standin_calls(self):
        if not self.calls.exists():
            return []
        return [json.loads(line) for line in self.calls.read_text().splitlines() if line.strip()]

    def issues(self):
        return [c for c in self.standin_calls() if c["path"] == f"/repos/{REPOSITORY}/issues" and c["authorized"]]

    def comments(self):
        return [c for c in self.standin_calls() if c["path"].endswith("/comments") and c["authorized"]]

    def received(self):
        listing = sudo("ls", "-1", str(DATA / "reports"))
        return sorted(line[:-5] for line in listing.stdout.split() if line.endswith(".json"))

    def as_person(self, *command, stdin=None):
        return sudo("podman", "exec", "--interactive", "--user", PERSON,
                    "--env", f"HOME=/home/{PERSON}", "--env", f"USER={PERSON}", "--env", f"LOGNAME={PERSON}",
                    "--workdir", f"/home/{PERSON}", CLIENT, *command, input=stdin)

    def as_root(self, *command, stdin=None):
        return sudo("podman", "exec", "--interactive", CLIENT, *command, input=stdin)

    def answered(self, answer, argv):
        return self.as_person("python3", "/ci/answer.py", PROMPT, answer, "--", *argv)

    def receiver_admin(self, *arguments):
        return sudo("podman", "exec", CONTAINER, "tur-report-receiver", *arguments,
                    "--config", str(ETC / "config.json"))

    def journal(self):
        units = sudo("journalctl", "--no-pager", "-o", "cat", "-u", UNIT, "-u", "caddy").stdout
        container = sudo("journalctl", "--no-pager", "-o", "cat", f"CONTAINER_NAME={CONTAINER}").stdout
        return units + container

    # ---------------------------------------------------------------- setup

    def certificates(self):
        tls = self.work / "tls"
        tls.mkdir(parents=True, exist_ok=True)
        run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
             "-subj", "/CN=TUR CI test CA", "-keyout", str(tls / "ca.key"), "-out", str(tls / "ca.crt"),
             "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign"],
            check=True)
        run(["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", f"/CN={DOMAIN}",
             "-keyout", str(tls / "server.key"), "-out", str(tls / "server.csr")], check=True)
        (tls / "ext.cnf").write_text(f"subjectAltName=DNS:{DOMAIN}\nextendedKeyUsage=serverAuth\n")
        run(["openssl", "x509", "-req", "-in", str(tls / "server.csr"), "-CA", str(tls / "ca.crt"),
             "-CAkey", str(tls / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(tls / "ext.cnf"),
             "-out", str(tls / "server.crt")], check=True)
        sudo("install", "-m", "0644", str(tls / "ca.crt"), "/usr/local/share/ca-certificates/tur-ci-test.crt",
             check=True)
        sudo("update-ca-certificates", check=True)
        sudo("sh", "-c", f"echo '127.0.0.1 {DOMAIN}' >> /etc/hosts", check=True)
        return tls

    def start_standin(self):
        self.token_file.write_text(self.token + "\n")
        self.token_file.chmod(0o600)
        self.standin = subprocess.Popen(
            [sys.executable, str(HERE / "standin_github.py"), "--port", str(STANDIN_PORT),
             "--repository", REPOSITORY, "--token-file", str(self.token_file),
             "--calls", str(self.calls), "--state", str(self.work / "standin-state.json")],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not wait_for(lambda: run(["curl", "-s", f"http://127.0.0.1:{STANDIN_PORT}/"]).returncode == 0, 15):
            raise RuntimeError("the stand-in for GitHub did not start")

    def start_client(self):
        self.exchange.mkdir(parents=True, exist_ok=True)
        gateway = sudo("podman", "network", "inspect", "podman",
                       "--format", "{{range .Subnets}}{{.Gateway}}{{end}}").stdout.strip()
        self.gateway = gateway or "10.88.0.1"
        sudo("podman", "rm", "--force", CLIENT)
        started = sudo("podman", "run", "--detach", "--name", CLIENT, "--hostname", CLIENT_HOST,
                       "--add-host", f"{DOMAIN}:{self.gateway}",
                       "--volume", f"{self.args.tur_diag.resolve()}:/usr/bin/tur-diag:ro",
                       "--volume", f"{HERE}:/ci:ro",
                       "--volume", f"{self.exchange}:/exchange",
                       self.args.client_image, "sleep", "infinity")
        if started.returncode != 0:
            raise RuntimeError(f"the client container did not start: {started.stderr}")
        setup = f"""
set -e
# On Fedora's atomic images /home is a link to /var/home, made at boot.
mkdir -p "$(readlink -f /home)"
useradd --create-home --uid 4711 {PERSON}
install -d -m 0755 /usr/lib/tur /etc/tur /usr/share/tur/heal
printf 'TUR_VERSION=0.2.0\\nTUR_BUILD_ID=ci.1\\n' > /usr/lib/tur/release
printf 'RECEIVER_URL={URL}\\n' > /etc/tur/problem-reports.conf
cp /exchange/ca.crt /etc/pki/ca-trust/source/anchors/tur-ci-test.crt
update-ca-trust extract
chmod 0777 /exchange
"""
        sudo("cp", str(self.work / "tls" / "ca.crt"), str(self.exchange / "ca.crt"), check=True)
        result = self.as_root("sh", "-c", setup)
        if result.returncode != 0:
            raise RuntimeError(f"the client could not be set up: {result.stdout}{result.stderr}")
        address = sudo("podman", "inspect", CLIENT, "--format",
                       "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}").stdout.strip()
        self.client_address = address or None

    # ------------------------------------------------------------ scenarios

    def deploy(self, tls):
        archive = self.args.image_archive.resolve()
        result = sudo(str(DEPLOY / "install.sh"), "--domain", DOMAIN, "--repository", REPOSITORY,
                      "--image", str(archive), "--token-file", str(self.token_file),
                      "--tls-cert", str(tls / "server.crt"), "--tls-key", str(tls / "server.key"),
                      "--key-id", "ci-1", "--github-api-url", f"http://127.0.0.1:{STANDIN_PORT}",
                      "--no-firewall", "--no-ssh-hardening", "--no-auto-updates")
        (self.out / "install.log").write_text(result.stdout + result.stderr)
        self.checks.check("deploy", "install.sh finishes and reaches HTTPS (exit 0)", result.returncode == 0,
                          f"exit {result.returncode}: {result.stderr[-1500:]}")
        active = sudo("systemctl", "is-active", UNIT).stdout.strip()
        self.checks.check("deploy", "the unit is active", active == "active", active)
        answer = run(["curl", "-s", f"{URL}/v1/reports/{'0' * 64}"]).stdout
        self.checks.check("deploy", "HTTPS answers with the receiver's refusal of an unknown token",
                          "TUR-DIAG-RECEIVER-011" in answer, answer)
        redirect = run(["curl", "-s", "-o", "/dev/null", "-w", "%{http_code} %{redirect_url}",
                        f"http://{DOMAIN}/v1/reports/x"]).stdout
        self.checks.check("deploy", "plain HTTP only redirects to HTTPS",
                          redirect.startswith(("301", "302", "308")) and "https://" in redirect, redirect)
        listening = sudo("ss", "-ltnp").stdout
        self.checks.check("deploy", "the receiver listens on the loopback interface only",
                          "127.0.0.1:8470" in listening and "0.0.0.0:8470" not in listening
                          and "*:8470" not in listening, listening)

    def secrets_kept(self):
        token = sudo("stat", "-c", "%a %u", str(ETC / "github-token")).stdout.strip()
        self.checks.check("secrets", "the token file is 0400, owned by the container's user",
                          token == "400 10001", token)
        key = sudo("stat", "-c", "%a %u", str(ETC / "keys" / "ci-1.key")).stdout.strip()
        self.checks.check("secrets", "the key file is 0400, owned by the container's user", key == "400 10001", key)
        config = sudo("cat", str(ETC / "config.json")).stdout
        self.checks.check("secrets", "the configuration names the token file, not the token",
                          self.token not in config and "github-token" in config)
        processes = run(["ps", "-eo", "args"]).stdout
        self.checks.check("secrets", "no command line carries the token", self.token not in processes)

    def pax_preview(self):
        adapters = json.loads(ADAPTERS.read_text())
        adapter = next(a for a in adapters["adapters"] if a["tool"] == "diag_report_prepare")
        inputs = {"title": "No sound after resume", "description": DESCRIPTION, "component": "audio",
                  "error_code": CODE}
        argv = ["tur-diag"]
        for argument in adapter["arguments"]:
            if "literal" in argument:
                argv.append(argument["literal"])
            elif "option" in argument and argument["input"] in inputs:
                argv.append(f"{argument['option']}={inputs[argument['input']]}")
        before = self.received()
        result = self.as_person(*argv)
        try:
            preview = json.loads(result.stdout)
        except ValueError:
            preview = {}
        self.checks.check("pax-preview", "P@x's preview runs", result.returncode == 0 and preview,
                          result.stdout[-1500:] + result.stderr[-1500:])
        self.checks.check("pax-preview", "nothing is saved and nothing is sent",
                          preview.get("sent") is False and preview.get("prepared_file") is None
                          and self.received() == before)
        prepared = self.as_person("sh", "-c", "ls ~/.local/state/tur/reports/prepared 2>/dev/null | wc -l")
        self.checks.check("pax-preview", "no prepared file exists", prepared.stdout.strip() == "0", prepared.stdout)
        handoff = preview.get("handoff") or {}
        argv = handoff.get("argv") or []
        self.checks.check("pax-preview", "the preview hands the report to the person",
                          handoff.get("by") == "person" and argv[:3] == ["tur-diag", "report", "submit"]
                          and "--yes" not in argv and "send" not in argv, handoff)
        description = preview.get("report", {}).get("description", "")
        self.checks.check("pax-preview", "the client replaced the account, the host name and the home path",
                          PERSON not in description and CLIENT_HOST not in description, description)
        return argv

    def declined(self, handoff):
        before = self.received()
        result = self.answered("no", handoff)
        (self.out / "declined.txt").write_text(result.stdout)
        self.checks.check("declined", "answering no sends nothing (TUR-DIAG-REPORT-004)",
                          result.returncode != 0 and "TUR-DIAG-REPORT-004" in result.stdout
                          and self.received() == before, result.stdout[-1500:])
        listing = self.as_person("sh", "-c", "ls ~/.local/state/tur/reports/prepared/")
        files = listing.stdout.split()
        self.checks.check("declined", "the report stays prepared, for the person to send later", len(files) == 1,
                          listing.stdout)
        return files[0] if files else None

    def sandbox(self, prepared_name):
        if not prepared_name:
            self.checks.check("sandbox", "a prepared report to try with", False)
            return
        self.as_person("cp", f"/home/{PERSON}/.local/state/tur/reports/prepared/{prepared_name}",
                       "/exchange/prepared.tar.zst")
        before = self.received()
        result = sudo("podman", "run", "--rm", "--hostname", PAX_SANDBOX_HOST,
                      "--add-host", f"{DOMAIN}:{self.gateway}",
                      "--volume", f"{self.args.tur_diag.resolve()}:/usr/bin/tur-diag:ro",
                      "--volume", f"{self.exchange}:/exchange",
                      "--env", "HOME=/tmp", self.args.client_image, "sh", "-c",
                      f"mkdir -p /etc/tur && echo RECEIVER_URL={URL} > /etc/tur/problem-reports.conf && "
                      f"tur-diag report send --to {URL} --yes /exchange/prepared.tar.zst")
        output = result.stdout + result.stderr
        self.checks.check("sandbox", "sending under P@x's sandbox host name is refused (TUR-DIAG-REPORT-013)",
                          result.returncode != 0 and "TUR-DIAG-REPORT-013" in output, output[-1500:])
        self.checks.check("sandbox", "nothing reached the receiver", self.received() == before)

    def person_sends(self, handoff):
        before = self.received()
        result = self.answered("yes", handoff)
        (self.out / "person-sends.txt").write_text(result.stdout)
        report_id = None
        for word in result.stdout.replace(".", " ").split():
            if word.startswith("R-") and len(word) == 18:
                report_id = word
        self.checks.check("person-sends", "typing yes sends the report over HTTPS",
                          result.returncode == 0 and report_id is not None, result.stdout[-2000:])
        after = self.received()
        self.checks.check("person-sends", "the receiver holds exactly one new report",
                          len(after) == len(before) + 1 and report_id in after, after)
        listed = self.as_person("tur-diag", "report", "list", "--json")
        try:
            sent = json.loads(listed.stdout)["sent"]
        except (ValueError, KeyError):
            sent = []
        self.checks.check("person-sends", "the person's list shows it",
                          any(item.get("report_id") == report_id for item in sent), listed.stdout[-1500:])
        origin = self.as_person("cat", f"/home/{PERSON}/.local/state/tur/reports/origins/{report_id}.json")
        try:
            origin_document = json.loads(origin.stdout)
        except ValueError:
            origin_document = {}
        self.checks.check("person-sends", "its origin is kept: code, version 0.2.0, build ci.1",
                          origin_document.get("error_code") == CODE and origin_document.get("version") == "0.2.0"
                          and origin_document.get("build_id") == "ci.1", origin.stdout + origin.stderr)
        if report_id:
            self.reports.append(report_id)
        return report_id, sent

    def redaction(self, report_id, sent):
        if not report_id:
            self.checks.check("redaction", "a report to look at", False)
            return
        decrypted = DATA / f"{report_id}.ci.tar.zst"
        result = self.receiver_admin("decrypt", "--report", report_id, "--output", str(decrypted))
        self.checks.check("redaction", "the stored bundle decrypts", result.returncode == 0, result.stderr)
        local = self.work / "received.tar.zst"
        sudo("cp", str(decrypted), str(local))
        sudo("chown", f"{run(['id', '-u']).stdout.strip()}", str(local))
        sudo("rm", "-f", str(decrypted))
        expected = next((item.get("bundle_sha256") for item in sent if item.get("report_id") == report_id), None)
        if expected is None:
            records = self.as_person("sh", "-c", f"cat ~/.local/state/tur/reports/sent/{report_id}.json")
            try:
                expected = json.loads(records.stdout).get("bundle_sha256")
            except ValueError:
                expected = None
        self.checks.check("redaction", "the receiver stored exactly the bytes the person's computer sent",
                          local.exists() and sha256(local) == expected, f"{expected}")
        unpacked = self.work / "received"
        unpacked.mkdir(exist_ok=True)
        run(["sh", "-c", f"zstd -dc {shlex.quote(str(local))} | tar -x -C {shlex.quote(str(unpacked))}"])
        text = "\n".join(path.read_text(errors="replace") for path in unpacked.rglob("*") if path.is_file())
        for category, value in CLIENT_PLANTED[:2] + [("home path", f"/home/{PERSON}/")]:
            self.checks.check("redaction", f"client side: the bundle sent carries no {category}",
                              value not in text, category)
        issue_bodies = "\n".join(call["body"] for call in self.issues() + self.comments())
        for category, value in CLIENT_PLANTED:
            self.checks.check("redaction", f"receiver side: the filed summary carries no {category}",
                              value not in issue_bodies, category)

    def filing(self, first):
        filed = wait_for(lambda: self.issues(), 120, 2)
        issue = json.loads(filed[0]["body"]) if filed else {}
        title = issue.get("title", "")
        self.checks.check("filing", "the first report opens one issue titled with code, component and version",
                          len(filed) == 1 and title.startswith(f"[{CODE}] audio 0.2.0:"), title)
        self.checks.check("filing", "the issue has the label and names the report",
                          issue.get("labels") == ["problem-report"] and first in issue.get("body", ""),
                          issue.get("labels"))
        second = self.answered("yes", ["tur-diag", "report", "submit", "--title=The sound stops again",
                                       "--component=audio", f"--error-code={CODE}"])
        second_id = next((word for word in second.stdout.replace(".", " ").split()
                          if word.startswith("R-") and len(word) == 18), None)
        if second_id:
            self.reports.append(second_id)
        commented = wait_for(lambda: self.comments(), 120, 2)
        self.checks.check("filing", "a second report of the same signature becomes a comment, not an issue",
                          second.returncode == 0 and len(commented) == 1 and len(self.issues()) == 1
                          and second_id is not None and second_id in commented[0]["body"],
                          second.stdout[-1500:])

    def fixed(self):
        marked = self.receiver_admin("mark-fixed", "--issue", "1", "--version", "0.3.0")
        self.checks.check("fixed", "the maintainer marks the issue fixed in 0.3.0",
                          marked.returncode == 0 and "2 reports" in marked.stdout, marked.stdout + marked.stderr)
        first = self.reports[0] if self.reports else "R-none"
        status = self.as_person("tur-diag", "report", "status", first, "--json")
        try:
            state = json.loads(status.stdout)
        except ValueError:
            state = {}
        self.checks.check("fixed", "report status says fixed in 0.3.0",
                          state.get("state") == "fixed" and state.get("fixed_in") == "0.3.0", status.stdout)
        before = json.loads(self.as_person("tur-diag", "report", "fixed", "--json").stdout or "{}")
        by_receiver = [item for item in before.get("fixed", []) if item["how"] == "receiver"]
        self.checks.check("fixed", "before the update: fixed as the receiver said, for the report checked",
                          [item["report_id"] for item in by_receiver] == [first], before)
        update = (
            "printf 'TUR_VERSION=0.3.0\\nTUR_BUILD_ID=ci.2\\n' > /usr/lib/tur/release && "
            "cat > /usr/share/tur/heal/fixes.json <<'EOF'\n"
            + json.dumps({"schema_version": 1, "about": "CI", "fixes": [
                {"code": CODE, "summary": "The sound comes back after resume.", "change": "CI"}]})
            + "\nEOF\n"
        )
        self.as_root("sh", "-c", update)
        after = self.as_person("tur-diag", "report", "fixed", "--json")
        try:
            listed = json.loads(after.stdout)
        except ValueError:
            listed = {}
        by_update = {item["report_id"]: item for item in listed.get("fixed", []) if item["how"] == "update"}
        self.checks.check("fixed", "after an update that lists the code, both reports are fixed by it",
                          sorted(by_update) == sorted(self.reports) and len(self.reports) == 2, after.stdout)
        text = self.as_person("tur-diag", "report", "fixed").stdout
        self.checks.check("fixed", "the person is told: the problem you reported is fixed",
                          "The problem you reported is fixed" in text and "0.3.0" in text, text)

    def cgroup(self):
        container_id = sudo("podman", "inspect", CONTAINER, "--format", "{{.Id}}").stdout.strip()
        found = sudo("find", "/sys/fs/cgroup", "-type", "d", "-name", f"*{container_id}*").stdout.split()
        found = [path for path in found if sudo("test", "-e", f"{path}/memory.peak").returncode == 0] or found
        return max(found, key=len) if found else None

    def read_cgroup(self, path, name):
        return sudo("cat", f"{path}/{name}").stdout

    def load(self):
        path = self.cgroup()
        usage_before = None
        if path:
            for line in self.read_cgroup(path, "cpu.stat").splitlines():
                if line.startswith("usage_usec"):
                    usage_before = int(line.split()[1])
        large = self.args.material / "large.tar.zst"

        def upload(_):
            return run(["curl", "-s", "-o", "/dev/null", "-w", "%{http_code} %{time_total}",
                        "--max-time", "300", "-H", "Content-Type: application/vnd.tur.diag-bundle+zstd",
                        "--data-binary", f"@{large}", f"{URL}/v1/reports"]).stdout

        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            answers = list(pool.map(upload, range(2)))
        self.checks.check("load", "two reports near the limits at once are both accepted",
                          all(answer.startswith("202") for answer in answers), answers)
        measured = {"bundle_bytes": large.stat().st_size, "answers": answers}
        if path:
            measured["memory_peak_bytes"] = self.read_cgroup(path, "memory.peak").strip() or None
            measured["memory_now_bytes"] = self.read_cgroup(path, "memory.current").strip() or None
            for line in self.read_cgroup(path, "cpu.stat").splitlines():
                if line.startswith("usage_usec") and usage_before is not None:
                    measured["cpu_seconds_for_two"] = (int(line.split()[1]) - usage_before) / 1e6
        stats = sudo("podman", "stats", "--no-stream", "--format", "json", CONTAINER).stdout
        measured["podman_stats"] = stats[:2000]
        self.checks.measurements["load"] = measured
        self.checks.check("load", "the container's memory was measured", bool(path), measured)

    def logs(self):
        journal = self.journal()
        values = [value for _, value in CLIENT_PLANTED] + [self.token]
        self.checks.check("logs", "no planted value or token in the receiver's or Caddy's journal",
                          not any(value in journal for value in values))
        if self.client_address:
            receiver_log = sudo("journalctl", "--no-pager", "-o", "cat", f"CONTAINER_NAME={CONTAINER}").stdout
            self.checks.check("logs", "the client's address is not in the receiver's journal",
                              self.client_address not in receiver_log, self.client_address)

    def redeploy(self):
        key_before = sudo("sha256sum", str(ETC / "keys" / "ci-1.key")).stdout.split()[:1]
        token_before = sudo("sha256sum", str(ETC / "github-token")).stdout.split()[:1]
        result = sudo(str(DEPLOY / "install.sh"), "--domain", DOMAIN, "--repository", REPOSITORY,
                      "--key-id", "ci-2", "--github-api-url", f"http://127.0.0.1:{STANDIN_PORT}",
                      "--tls-cert", str(self.work / "tls" / "server.crt"),
                      "--tls-key", str(self.work / "tls" / "server.key"),
                      "--no-firewall", "--no-ssh-hardening", "--no-auto-updates", input="")
        (self.out / "install-again.log").write_text(result.stdout + result.stderr)
        self.checks.check("redeploy", "install.sh runs again without image or token", result.returncode == 0,
                          result.stderr[-1500:])
        self.checks.check("redeploy", "the key in use is kept, no new key is made",
                          sudo("sha256sum", str(ETC / "keys" / "ci-1.key")).stdout.split()[:1] == key_before
                          and sudo("test", "-e", str(ETC / "keys" / "ci-2.key")).returncode != 0)
        self.checks.check("redeploy", "the token is kept",
                          sudo("sha256sum", str(ETC / "github-token")).stdout.split()[:1] == token_before)
        first = self.reports[0] if self.reports else "R-none"
        status = self.as_person("tur-diag", "report", "status", first)
        self.checks.check("redeploy", "the reports are kept and answer", "fixed in 0.3.0" in status.stdout,
                          status.stdout + status.stderr)

    # --------------------------------------------------------------- driver

    def main(self):
        self.out.mkdir(parents=True, exist_ok=True)
        self.work.mkdir(parents=True, exist_ok=True)
        try:
            tls = self.certificates()
            self.start_standin()
            self.deploy(tls)
            self.secrets_kept()
            self.start_client()
            handoff = self.pax_preview()
            prepared = self.declined(handoff)
            self.sandbox(prepared)
            report_id, sent = self.person_sends(handoff)
            self.redaction(report_id, sent)
            self.filing(report_id or "R-none")
            self.fixed()
            self.load()
            self.logs()
            self.redeploy()
        except Exception as error:  # noqa: BLE001 - recorded as a failed check
            self.checks.check("driver", "the test ran to its end", False, repr(error))
        finally:
            (self.out / "journal.txt").write_text(self.journal())
            sudo("podman", "rm", "--force", CLIENT)
            if self.standin:
                self.standin.terminate()
        return self.report()

    def report(self):
        results = {"results": self.checks.results, "measurements": self.checks.measurements}
        (self.out / "results.json").write_text(json.dumps(results, indent=2))
        failed = self.checks.failed()
        lines = ["# Problem reports, deployed: end to end", "",
                 f"{len(self.checks.results) - len(failed)} of {len(self.checks.results)} checks passed.", "",
                 "| Scenario | Check | |", "|---|---|---|"]
        for result in self.checks.results:
            lines.append(f"| {result['scenario']} | {result['check']} | {'ok' if result['passed'] else 'FAIL'} |")
        lines += ["", "## Measurements", "", "```json", json.dumps(self.checks.measurements, indent=2), "```", ""]
        (self.out / "summary.md").write_text("\n".join(lines))
        return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-diag", type=Path, required=True)
    parser.add_argument("--image-archive", type=Path, required=True)
    parser.add_argument("--client-image", required=True)
    parser.add_argument("--material", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    sys.exit(Test(parser.parse_args()).main())


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""Developer helper for working against GitHub Actions without local builds.

UOS is built and tested in CI, not on development machines (ADR-0002). This
tool lets a developer or agent push, wait for the result, read failures and
pull back the fixes CI can compute (lockfiles, formatting).

    scripts/dev/ci.py wait [SHA] [--timeout SECONDS]   wait for all runs on a commit
    scripts/dev/ci.py log JOB_ID [--tail N]            print the end of a job log
    scripts/dev/ci.py fixes RUN_ID COMPONENT_DIR       apply CI's Cargo.lock / rustfmt patch
    scripts/dev/ci.py artifact RUN_ID NAME DEST_DIR    download and unpack an artifact
    scripts/dev/ci.py pr --title T --body-file F       open a pull request for this branch
    scripts/dev/ci.py pr-status NUMBER                 show a pull request's state

The token comes from UOS_GH_TOKEN or from git's configured credential helper.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request
import zipfile

API = "https://api.github.com"
ROOT = pathlib.Path(__file__).resolve().parent.parent.parent


def git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True, text=True).stdout.strip()


def repo_slug() -> str:
    url = git("remote", "get-url", "origin")
    slug = url.split("github.com", 1)[1].lstrip(":/")
    return slug[:-4] if slug.endswith(".git") else slug


def token() -> str:
    if os.environ.get("UOS_GH_TOKEN"):
        return os.environ["UOS_GH_TOKEN"]
    out = subprocess.run(
        ["git", "-C", str(ROOT), "credential", "fill"],
        input="protocol=https\nhost=github.com\n\n",
        capture_output=True, text=True, check=True,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
    ).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line[len("password="):]
    sys.exit("no GitHub token: set UOS_GH_TOKEN or configure a git credential helper")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def request(method: str, path: str, body: dict | None = None, raw: bool = False):
    """Call the API. Redirects (logs, artifacts) are followed without the token."""
    url = path if path.startswith("http") else f"{API}/repos/{repo_slug()}{path}"
    headers = {
        "Authorization": f"Bearer {token()}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "uos-ci-helper",
    }
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    opener = urllib.request.build_opener(NoRedirect)
    for attempt in range(4):
        try:
            with opener.open(req, timeout=60) as resp:
                payload = resp.read()
            return payload if raw else (json.loads(payload) if payload else None)
        except urllib.error.HTTPError as err:
            if err.code in (301, 302, 307) and err.headers.get("Location"):
                plain = urllib.request.Request(err.headers["Location"], headers={"User-Agent": "uos-ci-helper"})
                with urllib.request.urlopen(plain, timeout=120) as resp:
                    return resp.read()
            if err.code in (502, 503, 504) and attempt < 3:
                time.sleep(5)
                continue
            detail = err.read().decode(errors="replace")[:500]
            sys.exit(f"GitHub API {method} {url} failed: {err.code} {detail}")
        except (urllib.error.URLError, TimeoutError):
            if attempt == 3:
                raise
            time.sleep(5)


def job_log_tail(job_id: int, tail: int) -> str:
    text = request("GET", f"/actions/jobs/{job_id}/logs", raw=True).decode(errors="replace")
    lines = [line.split(" ", 1)[1] if line[:4].isdigit() and " " in line else line for line in text.splitlines()]
    errors = [line for line in lines if "##[error]" in line]
    out = lines[-tail:]
    if errors:
        out = ["--- error annotations ---", *errors[:20], f"--- last {tail} lines ---", *out]
    return "\n".join(out)


def cmd_wait(args) -> int:
    sha = args.sha or git("rev-parse", "HEAD")
    deadline = time.time() + args.timeout
    appeared_by = time.time() + 120
    runs: list[dict] = []
    while time.time() < deadline:
        runs = request("GET", f"/actions/runs?head_sha={sha}&per_page=50")["workflow_runs"]
        if not runs and time.time() > appeared_by:
            print(f"no workflow run was triggered for {sha[:12]} (check the workflow path filters)")
            return 3
        if runs and all(run["status"] == "completed" for run in runs):
            break
        time.sleep(20)
    else:
        print(f"timed out after {args.timeout}s; runs still in progress:")
        for run in runs:
            print(f"  {run['name']}: {run['status']} {run['html_url']}")
        return 2

    failed = False
    for run in runs:
        print(f"{run['name']}: {run['conclusion']}  run {run['id']}  {run['html_url']}")
        jobs = request("GET", f"/actions/runs/{run['id']}/jobs?per_page=100")["jobs"]
        for job in jobs:
            print(f"  job {job['id']}  {job['name']}: {job['conclusion']}")
            if job["conclusion"] not in ("success", "skipped"):
                failed = True
                for step in job.get("steps", []):
                    if step["conclusion"] not in ("success", "skipped"):
                        print(f"    step failed: {step['name']}")
                print(job_log_tail(job["id"], args.tail))
        if run["conclusion"] != "success":
            failed = True
    return 1 if failed else 0


def cmd_log(args) -> int:
    print(job_log_tail(args.job_id, args.tail))
    return 0


def download_artifact(run_id: int, name: str) -> zipfile.ZipFile | None:
    artifacts = request("GET", f"/actions/runs/{run_id}/artifacts?per_page=100")["artifacts"]
    match = [a for a in artifacts if a["name"] == name]
    if not match:
        return None
    blob = request("GET", f"/actions/artifacts/{match[0]['id']}/zip", raw=True)
    return zipfile.ZipFile(io.BytesIO(blob))


def cmd_artifact(args) -> int:
    archive = download_artifact(args.run_id, args.name)
    if archive is None:
        print(f"run {args.run_id} has no artifact named {args.name}")
        return 1
    dest = pathlib.Path(args.dest)
    dest.mkdir(parents=True, exist_ok=True)
    archive.extractall(dest)
    for name in archive.namelist():
        print(dest / name)
    return 0


def cmd_fixes(args) -> int:
    component = args.component.strip("/")
    name = "ci-fixes-" + component.replace("/", "-")
    archive = download_artifact(args.run_id, name)
    if archive is None:
        print(f"run {args.run_id} has no {name} artifact: nothing to fix for {component}")
        return 0
    names = archive.namelist()
    if "Cargo.lock" in names:
        (ROOT / component / "Cargo.lock").write_bytes(archive.read("Cargo.lock"))
        print(f"updated {component}/Cargo.lock")
    if "rustfmt.patch" in names:
        patch = archive.read("rustfmt.patch")
        subprocess.run(["git", "-C", str(ROOT), "apply", "--whitespace=nowarn", "-"], input=patch, check=True)
        print(f"applied rustfmt patch to {component}")
    return 0


def cmd_pr(args) -> int:
    branch = git("rev-parse", "--abbrev-ref", "HEAD")
    body = pathlib.Path(args.body_file).read_text()
    pr = request("POST", "/pulls", {"title": args.title, "head": branch, "base": args.base, "body": body})
    print(f"#{pr['number']} {pr['html_url']}")
    return 0


def cmd_pr_status(args) -> int:
    pr = request("GET", f"/pulls/{args.number}")
    print(f"#{pr['number']} {pr['state']} merged={pr['merged']} mergeable={pr['mergeable']} head={pr['head']['sha'][:12]}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("wait")
    p.add_argument("sha", nargs="?")
    p.add_argument("--timeout", type=int, default=3000)
    p.add_argument("--tail", type=int, default=60)
    p.set_defaults(func=cmd_wait)

    p = sub.add_parser("log")
    p.add_argument("job_id", type=int)
    p.add_argument("--tail", type=int, default=150)
    p.set_defaults(func=cmd_log)

    p = sub.add_parser("fixes")
    p.add_argument("run_id", type=int)
    p.add_argument("component")
    p.set_defaults(func=cmd_fixes)

    p = sub.add_parser("artifact")
    p.add_argument("run_id", type=int)
    p.add_argument("name")
    p.add_argument("dest")
    p.set_defaults(func=cmd_artifact)

    p = sub.add_parser("pr")
    p.add_argument("--title", required=True)
    p.add_argument("--body-file", required=True)
    p.add_argument("--base", default="main")
    p.set_defaults(func=cmd_pr)

    p = sub.add_parser("pr-status")
    p.add_argument("number", type=int)
    p.set_defaults(func=cmd_pr_status)

    args = ap.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())

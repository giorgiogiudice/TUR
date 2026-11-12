#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Wait until every other workflow on a commit has finished; decide whether
the commit's image may be released (ADR-0023, decision 4;
docs/architecture/publishing.md, section 3).

    scripts/release/await.py SHA EVENT [--timeout SECONDS]

EVENT is the event of the release run: `push` or `workflow_dispatch` (the
runs that count are the commit's push and manual runs) or `pull_request`
(its pull-request runs). For every workflow the latest such run counts; the
release workflow itself, `gate` (which only reports on the others) and
`pax-live` (a test against a live provider, excluded from the gate as well)
do not.

The image may be released only when

- the `image` workflow built and tested this commit's image (it exports the
  image the release promotes), and the `boot` workflow booted this commit,
- every workflow that ran on the commit has finished with success (the
  rollback scenarios among them, when the commit changed what they test;
  each workflow runs only when its paths changed), and nothing is still
  running.

Writes `release=true|false`, `reason`, and the run IDs of image, boot and
rollback to $GITHUB_OUTPUT, and a table to $GITHUB_STEP_SUMMARY. Exits 1 when
a workflow failed, so that the release run says so; exits 0 with
release=false when there is simply nothing to release. GH_TOKEN (actions:
read) and GITHUB_REPOSITORY come from the workflow.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.request

IGNORED = {"release", "gate", "pax-live"}
GOOD = {"success", "skipped", "neutral"}


def runs_for(repo: str, sha: str) -> list[dict]:
    found, page = [], 1
    while True:
        req = urllib.request.Request(
            f"https://api.github.com/repos/{repo}/actions/runs?head_sha={sha}&per_page=100&page={page}",
            headers={"Authorization": f"Bearer {os.environ['GH_TOKEN']}", "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(req, timeout=60) as response:
            batch = json.load(response)["workflow_runs"]
        found += batch
        if len(batch) < 100 or page >= 5:
            return found
        page += 1


def latest(runs: list[dict], events: set[str]) -> dict[str, dict]:
    chosen: dict[str, dict] = {}
    for run in runs:
        if run["name"] in IGNORED or run["event"] not in events:
            continue
        if run["name"] not in chosen or run["id"] > chosen[run["name"]]["id"]:
            chosen[run["name"]] = run
    return chosen


def output(**values: str) -> None:
    lines = "".join(f"{key}={value}\n" for key, value in values.items())
    print(lines, end="")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            stream.write(lines)


def summary(text: str) -> None:
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as stream:
            stream.write(text + "\n")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sha")
    ap.add_argument("event", choices=["push", "workflow_dispatch", "pull_request"])
    ap.add_argument("--timeout", type=int, default=4 * 3600)
    ap.add_argument("--first-wait", type=int, default=45, help="seconds for the other workflows to be created")
    args = ap.parse_args()
    repo = os.environ["GITHUB_REPOSITORY"]
    events = {"pull_request"} if args.event == "pull_request" else {"push", "workflow_dispatch"}
    started = time.monotonic()
    time.sleep(args.first_wait)
    shown = ""
    while True:
        chosen = latest(runs_for(repo, args.sha), events)
        waited = time.monotonic() - started
        if "image" not in chosen and waited > 300:
            output(release="false", reason="the image workflow did not run on this commit: no image to release")
            summary("Nothing to release: the `image` workflow did not run on this commit.")
            return 0
        pending = sorted(n for n, r in chosen.items() if r["status"] != "completed")
        state = ", ".join(f"{n}={r['status'] if r['status'] != 'completed' else r['conclusion']}"
                          for n, r in sorted(chosen.items()))
        if state != shown:
            print(f"[{waited:6.0f} s] {state}", flush=True)
            shown = state
        if not pending and "image" in chosen:
            break
        if waited > args.timeout:
            output(release="false", reason=f"still running after {args.timeout} s: {', '.join(pending)}")
            return 1
        time.sleep(60)

    table = ["| Workflow | Run | Result |", "|---|---|---|"]
    table += [f"| {n} | [{r['id']}]({r['html_url']}) | {r['conclusion']} |" for n, r in sorted(chosen.items())]
    summary("### Workflows on this commit\n\n" + "\n".join(table) + "\n")
    failed = sorted(f"{n} ({r['conclusion']})" for n, r in chosen.items() if r["conclusion"] not in GOOD)
    ids = {f"{n}_run": str(chosen[n]["id"]) if n in chosen else "" for n in ("image", "boot", "rollback")}
    if failed:
        output(release="false", reason="not every workflow passed: " + ", ".join(failed), **ids)
        summary("Not released: " + ", ".join(failed))
        return 1
    if chosen["image"]["conclusion"] != "success" or chosen.get("boot", {}).get("conclusion") != "success":
        output(release="false", reason="this commit's image was not both built (image) and booted (boot) by CI", **ids)
        summary("Nothing to release: the image of this commit was not both built and booted by CI.")
        return 0
    output(release="true", reason="every workflow on this commit passed", **ids)
    return 0


if __name__ == "__main__":
    sys.exit(main())

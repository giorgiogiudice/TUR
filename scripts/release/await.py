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

Two ways to rehearse without waiting, never used for publishing (the run
then writes `rehearsal=true`): --image-run takes a run of the image
workflow that already passed (a manual run of the release workflow), and
--fallback-branch, on a pull request whose commit did not run the image
workflow (it changed only the release itself), takes the branch's latest
successful image run. Boot and rollback runs of that run's commit are used
alongside.

Writes `release=true|false`, `rehearsal`, `reason`, and the run IDs of image, boot and
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


def api(repo: str, path: str) -> dict:
    req = urllib.request.Request(
        f"https://api.github.com/repos/{repo}{path}",
        headers={"Authorization": f"Bearer {os.environ['GH_TOKEN']}", "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(req, timeout=60) as response:
        return json.load(response)


def runs_for(repo: str, sha: str) -> list[dict]:
    found, page = [], 1
    while True:
        batch = api(repo, f"/actions/runs?head_sha={sha}&per_page=100&page={page}")["workflow_runs"]
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


def rehearse_with(repo: str, image_run: dict, why: str) -> int:
    """Use an image run that already passed (a rehearsal, never published)."""
    if image_run.get("name") != "image" or image_run.get("conclusion") != "success":
        output(release="false", reason=f"run {image_run.get('id')} is not a successful run of the image workflow")
        return 1
    others = {}
    for run in runs_for(repo, image_run["head_sha"]):
        if run["name"] in ("boot", "rollback") and run["conclusion"] == "success":
            if run["name"] not in others or run["id"] > others[run["name"]]["id"]:
                others[run["name"]] = run
    reason = (f"rehearsal ({why}) with the image tested by image run {image_run['id']} "
              f"(commit {image_run['head_sha'][:12]})")
    output(release="true", rehearsal="true", reason=reason, image_run=str(image_run["id"]),
           boot_run=str(others["boot"]["id"]) if "boot" in others else "",
           rollback_run=str(others["rollback"]["id"]) if "rollback" in others else "")
    summary("Rehearsal: " + reason + ". Nothing is published from such a run.")
    return 0 if "boot" in others else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sha")
    ap.add_argument("event", choices=["push", "workflow_dispatch", "pull_request"])
    ap.add_argument("--timeout", type=int, default=4 * 3600)
    ap.add_argument("--first-wait", type=int, default=45, help="seconds for the other workflows to be created")
    ap.add_argument("--image-run", help="do not wait: rehearse with this run of the image workflow (manual runs)")
    ap.add_argument("--fallback-branch", help="pull requests: when the image workflow did not run on this commit, "
                                              "rehearse with its latest successful run on this branch")
    args = ap.parse_args()
    repo = os.environ["GITHUB_REPOSITORY"]
    if args.image_run:
        return rehearse_with(repo, api(repo, f"/actions/runs/{args.image_run}"), "a person asked for it")
    events = {"pull_request"} if args.event == "pull_request" else {"push", "workflow_dispatch"}
    started = time.monotonic()
    time.sleep(args.first_wait)
    shown = ""
    while True:
        chosen = latest(runs_for(repo, args.sha), events)
        waited = time.monotonic() - started
        if "image" not in chosen and waited > 300 and args.fallback_branch and args.event == "pull_request":
            found = api(repo, f"/actions/workflows/image.yml/runs?branch={args.fallback_branch}"
                              "&event=pull_request&status=success&per_page=1")["workflow_runs"]
            if found:
                return rehearse_with(repo, found[0], "the image workflow did not run on this commit")
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
    output(release="true", rehearsal="false", reason="every workflow on this commit passed", **ids)
    return 0


if __name__ == "__main__":
    sys.exit(main())

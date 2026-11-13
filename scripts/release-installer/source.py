#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Check that a run of the `iso` workflow may be the source of a release.

    source.py RUN_ID OUT.env

The released medium must be exactly one that CI built and tested, so the run
must be:

- a run of .github/workflows/iso.yml in this repository (not from a fork);
- on the branch main, started by a push or by hand;
- completed, with the conclusion "success" (every job green);
- of a commit that is on main (main is that commit or ahead of it);
- still holding the medium artifact (uploaded only when `iso` is started by
  hand with `upload_medium`, kept three days) and the test record artifact
  (uploaded on every run), neither expired.

Writes source_sha, source_run_url, source_created, medium_artifact and
record_artifact to OUT.env. Uses GITHUB_API_URL, GITHUB_REPOSITORY, GH_TOKEN.
"""

from __future__ import annotations

import pathlib
import sys

import ghapi

WORKFLOW_PATH = ".github/workflows/iso.yml"
MEDIUM_ARTIFACT = "tur-installer-medium-DO-NOT-REDISTRIBUTE"
RECORD_ARTIFACT = "tur-installer-medium-test"


def check(run_id: str) -> tuple[dict, list[str]]:
    """Return (facts, problems)."""
    if not run_id.isdigit():
        return {}, [f"source_run must be the numeric id of a run, not {run_id!r}"]
    run = ghapi.call("GET", f"/actions/runs/{run_id}")
    repo = ghapi.repository()
    problems = []
    if run.get("path") != WORKFLOW_PATH:
        problems.append(f"run {run_id} is a run of {run.get('path')}, not {WORKFLOW_PATH}")
    if (run.get("head_repository") or {}).get("full_name") != repo:
        problems.append(f"run {run_id} built code from {(run.get('head_repository') or {}).get('full_name')}, not {repo}")
    if run.get("head_branch") != "main":
        problems.append(f"run {run_id} ran on branch {run.get('head_branch')!r}, not main")
    if run.get("event") not in ("push", "workflow_dispatch"):
        problems.append(f"run {run_id} was started by {run.get('event')!r}, not by a push to main or by hand")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        problems.append(f"run {run_id} is {run.get('status')}/{run.get('conclusion')}, not completed/success")
    sha = run.get("head_sha", "")
    if len(sha) != 40:
        problems.append(f"run {run_id} names no commit")
    else:
        compare = ghapi.call("GET", f"/compare/{sha}...main")
        if compare.get("status") not in ("identical", "ahead"):
            problems.append(f"commit {sha} is not on main (main is {compare.get('status')} of it)")
    artifacts = ghapi.call("GET", f"/actions/runs/{run_id}/artifacts?per_page=100").get("artifacts", [])
    found = {a["name"]: a for a in artifacts}
    medium = found.get(MEDIUM_ARTIFACT)
    if medium is None:
        problems.append(
            f"run {run_id} did not upload the medium (artifact {MEDIUM_ARTIFACT}). "
            "Start the iso workflow on main by hand with upload_medium ticked, wait for it to pass, "
            "and release from that run.")
    elif medium.get("expired"):
        problems.append(
            f"the medium of run {run_id} has expired (artifacts are kept three days). "
            "Start the iso workflow on main again with upload_medium ticked and release from the new run.")
    record = found.get(RECORD_ARTIFACT)
    if record is None or record.get("expired"):
        problems.append(f"run {run_id} has no test record (artifact {RECORD_ARTIFACT}), or it has expired")
    facts = {
        "source_sha": sha,
        "source_run_url": run.get("html_url", ""),
        "source_created": run.get("created_at", ""),
        "source_event": run.get("event", ""),
        "medium_artifact": MEDIUM_ARTIFACT,
        "medium_artifact_bytes": str((medium or {}).get("size_in_bytes", "")),
        "medium_artifact_expires": (medium or {}).get("expires_at", ""),
        "record_artifact": RECORD_ARTIFACT,
    }
    return facts, problems


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    facts, problems = check(sys.argv[1].strip())
    for problem in problems:
        print(f"::error::{problem}")
    if problems:
        return 1
    pathlib.Path(sys.argv[2]).write_text("".join(f"{k}={v}\n" for k, v in facts.items()))
    for key, value in facts.items():
        print(f"{key}: {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

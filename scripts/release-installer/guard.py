#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Decide whether this run of release-installer may publish, or is a dry run.

    guard.py            print the decision; write publish=true|false to $GITHUB_OUTPUT
    guard.py --require  the same, and exit 1 unless the decision is to publish

Everything below must hold to publish. Each is read from the environment,
compared with an exact value, and anything else counts as "no":

  EVENT              workflow_dispatch          started by hand
  REF                refs/heads/main            from main
  PUBLISH_INPUT      true                       the person ticked "publish"
  RELEASES_ENABLED   1                          repository variable TUR_RELEASES_ENABLED
  HAVE_ENDPOINT      true                       secret TUR_DOWNLOADS_ENDPOINT is set
  HAVE_BUCKET        true                       secret TUR_DOWNLOADS_BUCKET is set
  HAVE_ACCESS_KEY    true                       secret TUR_DOWNLOADS_ACCESS_KEY is set
  HAVE_SECRET_KEY    true                       secret TUR_DOWNLOADS_SECRET_KEY is set
  PUBLIC_URL         https://…                  repository variable TUR_DOWNLOADS_PUBLIC_URL
  REDISTRIBUTABLE    yes                        the medium's manifest does not say "not for redistribution"
  TAG_FREE           yes                        neither the tag nor a release of it exists

The workflow passes the secrets' presence as `secrets.X != ''`, never their
values.
"""

from __future__ import annotations

import os
import re
import sys

RULES = [
    ("EVENT", lambda v: v == "workflow_dispatch", "the workflow was not started by hand (workflow_dispatch)"),
    ("REF", lambda v: v == "refs/heads/main", "the workflow was not started from main"),
    ("PUBLISH_INPUT", lambda v: v == "true", "the input 'publish' is not ticked"),
    ("RELEASES_ENABLED", lambda v: v == "1", "the repository variable TUR_RELEASES_ENABLED is not 1"),
    ("HAVE_ENDPOINT", lambda v: v == "true", "the secret TUR_DOWNLOADS_ENDPOINT is not set"),
    ("HAVE_BUCKET", lambda v: v == "true", "the secret TUR_DOWNLOADS_BUCKET is not set"),
    ("HAVE_ACCESS_KEY", lambda v: v == "true", "the secret TUR_DOWNLOADS_ACCESS_KEY is not set"),
    ("HAVE_SECRET_KEY", lambda v: v == "true", "the secret TUR_DOWNLOADS_SECRET_KEY is not set"),
    ("PUBLIC_URL", lambda v: bool(re.fullmatch(r"https://[A-Za-z0-9.-]+(:[0-9]+)?(/[A-Za-z0-9._~/-]*)?", v)),
     "the repository variable TUR_DOWNLOADS_PUBLIC_URL is not an https:// address"),
    ("REDISTRIBUTABLE", lambda v: v == "yes", "the medium's manifest says it is not for redistribution (issue #77)"),
    ("TAG_FREE", lambda v: v == "yes", "the tag or a release of this version already exists"),
]


def decide(env: dict[str, str]) -> tuple[bool, list[str]]:
    reasons = [reason for name, ok, reason in RULES if not ok(env.get(name, "").strip())]
    return not reasons, reasons


def main() -> int:
    require = sys.argv[1:] == ["--require"]
    if sys.argv[1:] not in ([], ["--require"]):
        print(__doc__, file=sys.stderr)
        return 2
    publish, reasons = decide(dict(os.environ))
    if publish:
        print("PUBLISH: every condition holds")
    else:
        print("DRY RUN: nothing will be uploaded or released, because")
        for reason in reasons:
            print(f"  - {reason}")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as out:
            out.write(f"publish={'true' if publish else 'false'}\n")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
            if publish:
                summary.write("### Publishing: every guard holds\n\n")
            else:
                summary.write("### Dry run: nothing is uploaded or released\n\n")
                summary.writelines(f"- {reason}\n" for reason in reasons)
                summary.write("\n")
    if require and not publish:
        print("::error::refusing to publish")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Write the release notes of an installer medium from the template in the repository.

    notes.py TEMPLATE.md KNOWN-ISSUES.md DIR OUT.md

DIR holds release.json (written by verify.py) and signing.env (sign.sh).
From the environment: GITHUB_SERVER_URL, GITHUB_REPOSITORY, SOURCE_RUN_URL,
TUR_DOWNLOADS_PUBLIC_URL (may be empty in a dry run), DRY_RUN (true/false),
RELEASE_DATE (YYYY-MM-DD; default today, UTC).

The template uses {{name}} for values and {{#name}}…{{/name}} for a block kept
only when `name` is true. {{known_issues}} is the body of KNOWN-ISSUES.md
without its title. HTML comments in the template are dropped. A name the
template uses that is not defined, or a {{ left over, stops the script.
"""

from __future__ import annotations

import datetime
import json
import math
import os
import pathlib
import re
import sys

PREFIX = "releases"


def strip_comments(text: str) -> str:
    return re.sub(r"<!--.*?-->\n?", "", text, flags=re.S)


def known_issues_body(text: str) -> str:
    lines = strip_comments(text).strip().splitlines()
    if lines and lines[0].startswith("# "):
        lines = lines[1:]
    return "\n".join(lines).strip()


def env_file(path: pathlib.Path) -> dict[str, str]:
    values = {}
    if path.exists():
        for line in path.read_text().splitlines():
            key, sep, value = line.partition("=")
            if sep:
                values[key.strip()] = value.strip()
    return values


def stick_size(iso_bytes: int) -> int:
    """The smallest common USB stick size (GB, as sold) that holds the medium."""
    for size in (4, 8, 16, 32, 64, 128):
        if iso_bytes <= size * 1000**3 * 0.93:  # sticks hold a little less than their label
            return size
    return 256


def values(folder: pathlib.Path, known: str) -> dict:
    facts = json.loads((folder / "release.json").read_text())
    signing = env_file(folder / "signing.env")
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com").rstrip("/")
    repo = os.environ.get("GITHUB_REPOSITORY", "giorgiogiudice/TUR")
    repo_url = f"{server}/{repo}"
    tag = facts["tag"]
    public = os.environ.get("TUR_DOWNLOADS_PUBLIC_URL", "").strip().rstrip("/")
    dry_run = os.environ.get("DRY_RUN", "true") != "false"
    base_dl = f"{public}/{PREFIX}/{facts['version']}" if public else ""
    assets = f"{repo_url}/releases/download/{tag}"

    def link(name: str) -> str:
        return f"{base_dl}/{name}" if base_dl else f"(not uploaded: dry run) {name}"

    smallest = facts.get("smallest_disk_bytes")
    return {
        **{k: ("" if v is None else v) for k, v in facts.items()},
        "date": os.environ.get("RELEASE_DATE") or datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d"),
        "repo_url": repo_url,
        "assets_url": assets,
        "release_page": f"{repo_url}/releases/tag/{tag}",
        "source_run_url": os.environ.get("SOURCE_RUN_URL", "(unknown)"),
        "commit_url": f"{repo_url}/commit/{facts['revision']}",
        "guide_url": f"{repo_url}/blob/{tag}/docs/guides/install-on-a-test-pc.md",
        "licence_url": f"{repo_url}/blob/{tag}/LICENSE.md",
        "notices_url": f"{repo_url}/blob/{tag}/THIRD_PARTY_NOTICES.md",
        "process_url": f"{repo_url}/blob/{tag}/docs/releases/README.md",
        "issues_url": f"{repo_url}/issues",
        "iso_url": link(facts["iso"]),
        "sums_url": link(facts["sums"]),
        "manifest_url": link(facts["manifest"]),
        "iso_gb": f"{facts['iso_bytes'] / 1000**3:.1f}",
        "stick_gb": stick_size(int(facts["iso_bytes"])),
        "smallest_disk_gib": math.ceil(smallest / 1024**3) if isinstance(smallest, int) else 20,
        "keyless_identity": signing.get("keyless_identity", ""),
        "keyless_issuer": signing.get("keyless_issuer", "https://token.actions.githubusercontent.com"),
        "keyless": signing.get("keyless") == "yes",
        "release_key": signing.get("release_key") == "yes",
        "dry_run": dry_run,
        "known_issues": known,
    }


def render(template: str, data: dict) -> str:
    text = strip_comments(template)

    def block(match: re.Match) -> str:
        name = match.group(1)
        if name not in data:
            raise SystemExit(f"template: unknown block {name!r}")
        return match.group(2) if data[name] is True else ""

    text = re.sub(r"\{\{#(\w+)\}\}\n?(.*?)\{\{/\1\}\}\n?", block, text, flags=re.S)

    def value(match: re.Match) -> str:
        name = match.group(1)
        if name not in data or isinstance(data[name], bool):
            raise SystemExit(f"template: unknown value {name!r}")
        return str(data[name])

    text = re.sub(r"\{\{(\w+)\}\}", value, text)
    if "{{" in text or "}}" in text:
        raise SystemExit("template: a {{ … }} was left over")
    return re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    template, known, folder, out = (pathlib.Path(a) for a in sys.argv[1:])
    data = values(folder, known_issues_body(known.read_text()))
    if not (data["keyless"] or data["release_key"]):
        raise SystemExit("signing.env says nothing was signed")
    notes = render(template.read_text(), data)
    out.write_text(notes)
    print(f"notes: {out} ({len(notes)} characters)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

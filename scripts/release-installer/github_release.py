#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Create the GitHub pre-release of an installer medium.

    github_release.py check  --tag TAG
    github_release.py create --tag TAG --sha SHA --name NAME --notes notes.md FILE...

`check` exits 1 if the tag, or a release of it, already exists (a release is
never replaced or moved).

`create`:
1. checks again that neither the tag nor a release of it exists;
2. creates a **draft** release of TAG on commit SHA, marked pre-release and
   with make_latest "false", body = notes.md;
3. attaches FILE... (the small files: checksums, signatures, manifest; the
   medium itself is on object storage, linked from the notes);
4. publishes the draft (draft false; pre-release and make_latest "false"
   again), which creates the tag on SHA;
5. reads everything back: the release is published, a pre-release, not
   "latest", of TAG, with exactly these assets at these sizes, the body as
   given; the tag points at SHA.

If anything fails before step 4 the draft is deleted, so the run can be
repeated. Uses GITHUB_API_URL, GITHUB_REPOSITORY, GH_TOKEN (contents: write).
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import urllib.parse

import ghapi

TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z.]+$")


def taken(tag: str) -> list[str]:
    found = []
    if ghapi.exists(f"/git/ref/tags/{tag}"):
        found.append(f"the tag {tag} exists")
    if ghapi.exists(f"/releases/tags/{tag}"):
        found.append(f"a release of {tag} exists")
    return found


def content_type(name: str) -> str:
    if name.endswith(".json"):
        return "application/json"
    if name.endswith((".SHA256SUMS", ".pub", ".txt", ".md")):
        return "text/plain"
    return "application/octet-stream"


def create(tag: str, sha: str, name: str, notes: str, files: list[pathlib.Path]) -> dict:
    release = ghapi.call("POST", "/releases", {
        "tag_name": tag,
        "target_commitish": sha,
        "name": name,
        "body": notes,
        "draft": True,
        "prerelease": True,
        "make_latest": "false",
    })
    rid = release["id"]
    print(f"draft release {rid} created")
    try:
        upload_base = release["upload_url"].split("{", 1)[0]
        for path in files:
            query = urllib.parse.urlencode({"name": path.name})
            asset = ghapi.call("POST", "", url=f"{upload_base}?{query}", data=path.read_bytes(),
                               content_type=content_type(path.name))
            if asset.get("name") != path.name or asset.get("size") != path.stat().st_size:
                raise SystemExit(f"asset {path.name}: GitHub stored {asset.get('name')} ({asset.get('size')} bytes)")
            print(f"attached {path.name} ({path.stat().st_size} bytes)")
        # A draft creates no tag; one that appeared meanwhile was made by someone else.
        if ghapi.exists(f"/git/ref/tags/{tag}"):
            raise SystemExit(f"the tag {tag} appeared while the draft was prepared")
    except BaseException:
        print(f"deleting draft release {rid}")
        ghapi.call("DELETE", f"/releases/{rid}")
        raise
    ghapi.call("PATCH", f"/releases/{rid}", {"draft": False, "prerelease": True, "make_latest": "false"})
    return ghapi.call("GET", f"/releases/{rid}")


def read_back(release: dict, tag: str, sha: str, notes: str, files: list[pathlib.Path]) -> list[str]:
    problems = []
    if release.get("draft") is not False:
        problems.append("the release is still a draft")
    if release.get("prerelease") is not True:
        problems.append("the release is not marked pre-release")
    if release.get("tag_name") != tag:
        problems.append(f"the release is of {release.get('tag_name')}, not {tag}")
    if (release.get("body") or "").strip() != notes.strip():
        problems.append("the release notes differ from notes.md")
    want = {p.name: p.stat().st_size for p in files}
    have = {a["name"]: a["size"] for a in release.get("assets", [])}
    if want != have:
        problems.append(f"the assets are {have}, expected {want}")
    try:
        latest = ghapi.call("GET", "/releases/latest")
        if latest and latest.get("id") == release.get("id"):
            problems.append("the release is marked latest")
    except ghapi.ApiError as error:
        if error.status != 404:
            raise
    ref = ghapi.call("GET", f"/git/ref/tags/{tag}")
    target = (ref.get("object") or {})
    if target.get("type") == "tag":
        target = (ghapi.call("GET", f"/git/tags/{target.get('sha')}").get("object") or {})
    if target.get("sha") != sha:
        problems.append(f"the tag {tag} points at {target.get('sha')}, not {sha}")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["check", "create"])
    ap.add_argument("--tag", required=True)
    ap.add_argument("--sha")
    ap.add_argument("--name")
    ap.add_argument("--notes", type=pathlib.Path)
    ap.add_argument("files", nargs="*", type=pathlib.Path)
    args = ap.parse_intermixed_args()
    if not TAG_RE.match(args.tag):
        print(f"::error::{args.tag!r} is not a pre-release tag like v0.1.0-preview.1")
        return 1
    found = taken(args.tag)
    if found:
        for problem in found:
            print(f"::error::{problem}; a release is never replaced. Choose another version.")
        return 1
    if args.command == "check":
        print(f"{args.tag} is free")
        return 0
    if not (args.sha and re.fullmatch(r"[0-9a-f]{40}", args.sha) and args.name and args.notes and args.files):
        ap.error("create needs --sha (40 hex), --name, --notes and at least one file")
    missing = [str(p) for p in args.files if not p.is_file()]
    if missing:
        print(f"::error::missing: {', '.join(missing)}")
        return 1
    notes = args.notes.read_text()
    if len(notes) > 125000:
        print("::error::the release notes are longer than GitHub's 125000 characters")
        return 1
    release = create(args.tag, args.sha, args.name, notes, args.files)
    problems = read_back(release, args.tag, args.sha, notes, args.files)
    for problem in problems:
        print(f"::error::published, but {problem}")
    if problems:
        return 1
    print(f"pre-release published: {release.get('html_url')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

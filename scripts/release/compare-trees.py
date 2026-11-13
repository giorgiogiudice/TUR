#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Prove that re-chunking changed how the image is cut, not what it holds.

    sudo scripts/release/compare-trees.py TESTED PUBLISHED OUT_JSON
    scripts/release/compare-trees.py --self-test

Both are images in containers-storage. Each is mounted read-only (podman
image mount) and walked; every file, directory and link is described by its
type, permissions, owner, group, size, the SHA-256 of its content (files)
and its target (links). Modification times are left out: OSTree, which
re-chunking goes through, sets every one to the same value.

Differences are listed in OUT_JSON. The one kind re-chunking makes by
design, OSTree's own object store under /sysroot/ostree/repo/objects, is
named in EXPECTED below with its reason and does not fail the comparison;
anything else does (exit status 1): a file under /usr or /etc that differs
in content, permissions, owner or link target, or that is only in one of
the two. --self-test proves that on two small trees. Run as root, in CI
only.
"""

from __future__ import annotations

import fnmatch
import hashlib
import json
import os
import stat
import subprocess
import sys

# What re-chunking changes by design, with the reason. A path pattern
# (fnmatch, "*" crosses "/") and the kind of difference allowed there:
# "missing" (only in the tested image), "added" (only in the published one)
# or "changed". Measured in release run 43336236365: these are the only
# differences, and no file outside them differs.
EXPECTED: list[tuple[str, str, str]] = [
    ("/sysroot/ostree/repo/objects/*", kind,
     "OSTree's object store of the commit the image is made from: the base's commit in the tested image, "
     "the re-chunked image's own commit in the published one. It is built from the files compared here")
    for kind in ("missing", "added", "changed")
]


def describe(root: str) -> dict[str, tuple]:
    entries: dict[str, tuple] = {}
    for top, dirs, files in os.walk(root):
        dirs.sort()
        for name in dirs + sorted(files):
            full = os.path.join(top, name)
            rel = "/" + os.path.relpath(full, root)
            st = os.lstat(full)
            mode = stat.S_IMODE(st.st_mode)
            if stat.S_ISLNK(st.st_mode):
                entries[rel] = ("link", mode, st.st_uid, st.st_gid, os.readlink(full))
            elif stat.S_ISDIR(st.st_mode):
                entries[rel] = ("dir", mode, st.st_uid, st.st_gid)
            elif stat.S_ISREG(st.st_mode):
                h = hashlib.sha256()
                with open(full, "rb") as stream:
                    for block in iter(lambda: stream.read(1 << 20), b""):
                        h.update(block)
                entries[rel] = ("file", mode, st.st_uid, st.st_gid, st.st_size, h.hexdigest())
            else:
                entries[rel] = ("special", stat.S_IFMT(st.st_mode), mode, st.st_uid, st.st_gid, st.st_rdev)
    return entries


def expected(path: str, kind: str) -> str | None:
    for pattern, allowed_kind, reason in EXPECTED:
        if allowed_kind == kind and fnmatch.fnmatchcase(path, pattern):
            return reason
    return None


def mount(image: str) -> str:
    return subprocess.run(["podman", "image", "mount", image], check=True, capture_output=True,
                          text=True).stdout.strip()


def compare(tested: dict, published: dict) -> list[dict]:
    differences = []
    for path in sorted(set(tested) | set(published)):
        a, b = tested.get(path), published.get(path)
        if a == b:
            continue
        kind = "missing" if b is None else "added" if a is None else "changed"
        differences.append({"path": path, "kind": kind, "tested": a, "published": b,
                            "expected": expected(path, kind)})
    return differences


def self_test() -> int:
    """The allowance is narrow: a change anywhere else is caught."""
    import tempfile
    with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
        for root, text, obj in ((a, "one", "x.commit"), (b, "two", "y.commit")):
            for d in ("usr/lib", "etc", "sysroot/ostree/repo/objects/ab"):
                os.makedirs(os.path.join(root, d))
            for f in ("usr/lib/same", "etc/same"):
                open(os.path.join(root, f), "w").write("same")
            open(os.path.join(root, "usr/lib/file"), "w").write(text)
            open(os.path.join(root, "etc/conf"), "w").write(text)
            open(os.path.join(root, "sysroot/ostree/repo/objects/ab", obj), "w").write(text)
        os.chmod(os.path.join(b, "etc/same"), 0o600)
        found = {d["path"]: bool(d["expected"]) for d in compare(describe(a), describe(b))}
    want = {"/usr/lib/file": False, "/etc/conf": False, "/etc/same": False,
            "/sysroot/ostree/repo/objects/ab/x.commit": True, "/sysroot/ostree/repo/objects/ab/y.commit": True}
    print("self-test:", "passed" if found == want else f"FAILED: {found}")
    return 0 if found == want else 1


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    tested_ref, published_ref, out = sys.argv[1:]
    roots = {}
    try:
        for ref in (tested_ref, published_ref):
            roots[ref] = mount(ref)
        tested = describe(roots[tested_ref])
        published = describe(roots[published_ref])
    finally:
        for ref in roots:
            subprocess.run(["podman", "image", "unmount", ref], capture_output=True)
    differences = compare(tested, published)
    unexpected = [d for d in differences if not d["expected"]]
    files = sum(1 for e in tested.values() if e[0] == "file")
    size = sum(e[4] for e in tested.values() if e[0] == "file")
    record = {"tested": tested_ref, "published": published_ref, "entries_tested": len(tested),
              "entries_published": len(published), "files": files, "file_bytes": size,
              "differences": differences, "unexpected": len(unexpected)}
    with open(out, "w") as stream:
        json.dump(record, stream, indent=1)
    print(f"tested:    {tested_ref}: {len(tested)} entries, {files} files, {size} bytes")
    print(f"published: {published_ref}: {len(published)} entries")
    by_reason: dict[str, int] = {}
    for d in differences:
        if d["expected"]:
            by_reason[d["expected"]] = by_reason.get(d["expected"], 0) + 1
    for reason, count in by_reason.items():
        print(f"expected:  {count} difference(s): {reason}")
    for d in unexpected[:80]:
        print(f"DIFFERS:   {d['kind']:8} {d['path']}  tested={d['tested']}  published={d['published']}")
    if len(unexpected) > 80:
        print(f"... and {len(unexpected) - 80} more")
    print(f"{'the same files' if not unexpected else str(len(unexpected)) + ' unexpected difference(s)'}")
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main())

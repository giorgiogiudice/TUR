# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The throwaway directory every corpus command runs in.

It is `C:\\turfix` on the Windows judge and `<prefix>/drive_c/turfix` under
Wine, so the same command line, absolute paths included, means the same
thing on both. It is rebuilt before every command; state-changing commands
only ever touch this tree (and the throwaway registry key and task named in
corpus.py).
"""

from __future__ import annotations

import hashlib
import os
import pathlib
import shutil
import stat
import sys

# Windows line endings on purpose: these are "files on a Windows PC".
FILES: dict[str, str] = {
    "notes.txt": "alpha\r\nbeta\r\ngamma\r\nAlpha Beta\r\n",
    "data.csv": "name,size,colour\r\napple,12,red\r\nbanana,7,yellow\r\ncherry,30,red\r\n",
    "report.log": "2026-01-01 INFO start\r\n2026-01-01 ERROR disk full\r\n2026-01-02 INFO done\r\n",
    "hello world.txt": "hello world\r\n",
    "docs/readme.md": "# Readme\r\n\r\nalpha docs\r\n",
    "docs/guide.txt": "guide line one\r\nguide ALPHA two\r\n",
    "docs/sub/deep.txt": "deep file\r\n",
    "src/main.c": "int main(void) { return 0; }\r\n",
    "src/util.c": "int util(void) { return 1; }\r\n",
    "src/util.h": "int util(void);\r\n",
    "scripts/hello.bat": "@echo off\r\necho hello from bat\r\n",
    "scripts/args.cmd": "@echo off\r\necho first=%1 second=%~2\r\nexit /b 3\r\n",
    "scripts/hello.ps1": "param([string]$Name = 'ps1')\r\nWrite-Output \"hello from $Name\"\r\n",
}
DIRS = ["empty"]

SIZE = {name: len(text.encode()) for name, text in FILES.items()}
SHA256 = {name: hashlib.sha256(text.encode()).hexdigest() for name, text in FILES.items()}
MD5 = {name: hashlib.md5(text.encode()).hexdigest() for name, text in FILES.items()}  # noqa: S324
TOTAL_BYTES = sum(SIZE.values())


def _force_remove(func, path, _exc):
    """shutil.rmtree's error hook: clear read-only and retry."""
    try:
        os.chmod(path, stat.S_IWRITE | stat.S_IREAD | stat.S_IEXEC)
        parent = os.path.dirname(path)
        os.chmod(parent, stat.S_IWRITE | stat.S_IREAD | stat.S_IEXEC)
    except OSError:
        pass
    try:
        func(path)
    except OSError:
        pass


def build(root: pathlib.Path) -> None:
    """Remove and re-create the fixture tree at `root`."""
    if root.is_symlink():
        root.unlink()
    elif root.exists():
        # The hook is `onexc` from Python 3.12 (Fedora 45 has 3.15, where
        # the older name is gone); both pass (function, path, error).
        hook = {"onexc": _force_remove} if sys.version_info >= (3, 12) else {"onerror": _force_remove}
        for _ in range(3):
            shutil.rmtree(root, **hook)
            if not root.exists():
                break
    root.mkdir(parents=True, exist_ok=True)
    for name, text in FILES.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.encode())
    for name in DIRS:
        (root / name).mkdir(parents=True, exist_ok=True)


def state(root: pathlib.Path) -> dict[str, str]:
    """The tree as {relative path: kind}, links not followed.

    A file is "f:<first 16 hex of sha256>" plus ":ro" when it is read-only;
    a directory is "d"; a link (symbolic link or junction) is "l".
    """
    out: dict[str, str] = {}
    if not root.exists():
        return out
    stack = [root]
    while stack:
        here = stack.pop()
        try:
            entries = sorted(os.scandir(here), key=lambda e: e.name)
        except OSError:
            continue
        for entry in entries:
            rel = pathlib.Path(entry.path).relative_to(root).as_posix()
            try:
                if entry.is_symlink() or (hasattr(entry, "is_junction") and entry.is_junction()):
                    out[rel] = "l"
                elif entry.is_dir(follow_symlinks=False):
                    out[rel] = "d"
                    stack.append(pathlib.Path(entry.path))
                else:
                    digest = hashlib.sha256(pathlib.Path(entry.path).read_bytes()).hexdigest()[:16]
                    mode = entry.stat(follow_symlinks=False).st_mode
                    out[rel] = f"f:{digest}" + ("" if mode & stat.S_IWUSR else ":ro")
            except OSError as err:
                out[rel] = f"?:{type(err).__name__}"
    return out

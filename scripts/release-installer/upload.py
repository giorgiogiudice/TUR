#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Upload the release's files to S3-compatible object storage and read each one back.

    upload.py DIR PREFIX OUT.json FILE...

Each DIR/FILE goes to <bucket>/PREFIX/FILE with rclone (pinned by the
workflow), as a multipart upload above TUR_S3_UPLOAD_CUTOFF (default 200M) in
parts of TUR_S3_CHUNK_SIZE (default 64M). `--immutable` makes rclone refuse to
replace an object that exists with different content; an identical one is
left as it is, so a release that stopped after uploading can be run again.

After each upload the object is read back before anything else happens:

- through the storage API: it exists and has the local file's size;
- through its public address (TUR_DOWNLOADS_PUBLIC_URL/PREFIX/FILE), the way a
  person downloads it: the whole object, streamed into SHA-256 and counted,
  must be the local file's size and SHA-256, and served with the expected
  Content-Type. A bucket whose objects are not public fails here.

Configuration, from the environment (the workflow fills it from secrets):
TUR_DOWNLOADS_ENDPOINT, TUR_DOWNLOADS_BUCKET, TUR_DOWNLOADS_ACCESS_KEY,
TUR_DOWNLOADS_SECRET_KEY, TUR_DOWNLOADS_PUBLIC_URL, and TUR_DOWNLOADS_PROVIDER
(rclone's S3 provider name; default Linode). Credentials reach rclone through
its environment only and are never printed. OUT.json lists what was uploaded:
name, key, public URL, bytes, SHA-256.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import urllib.error
import urllib.request

CONTENT_TYPES = {
    ".iso": "application/x-iso9660-image",
    ".json": "application/json",
    ".SHA256SUMS": "text/plain; charset=utf-8",
    ".pub": "text/plain; charset=utf-8",
    ".md": "text/markdown; charset=utf-8",
}


def content_type(name: str) -> str:
    for suffix, kind in CONTENT_TYPES.items():
        if name.endswith(suffix):
            return kind
    return "application/octet-stream"


def need(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise SystemExit(f"{name} is not set")
    return value


def rclone_env() -> dict[str, str]:
    env = dict(os.environ)
    env.update({
        "RCLONE_CONFIG_TURDL_TYPE": "s3",
        "RCLONE_CONFIG_TURDL_PROVIDER": os.environ.get("TUR_DOWNLOADS_PROVIDER", "Linode"),
        "RCLONE_CONFIG_TURDL_ENDPOINT": need("TUR_DOWNLOADS_ENDPOINT"),
        "RCLONE_CONFIG_TURDL_ACCESS_KEY_ID": need("TUR_DOWNLOADS_ACCESS_KEY"),
        "RCLONE_CONFIG_TURDL_SECRET_ACCESS_KEY": need("TUR_DOWNLOADS_SECRET_KEY"),
        # The access key is limited to one bucket and may not create buckets.
        "RCLONE_CONFIG_TURDL_NO_CHECK_BUCKET": "true",
        "RCLONE_CONFIG_TURDL_FORCE_PATH_STYLE": os.environ.get("TUR_DOWNLOADS_PATH_STYLE", "true"),
    })
    return env


def rclone(*args: str, capture: bool = False) -> str:
    result = subprocess.run(["rclone", *args], env=rclone_env(), check=False, text=True,
                            stdout=subprocess.PIPE if capture else None)
    if result.returncode != 0:
        raise SystemExit(f"rclone {args[0]} failed (exit {result.returncode})")
    return result.stdout or ""


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(8 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def read_back(remote: str, url: str, size: int, sha: str, kind: str) -> None:
    stat = json.loads(rclone("lsjson", "--stat", remote, capture=True))
    if stat.get("Size") != size:
        raise SystemExit(f"read back {remote}: the storage reports {stat.get('Size')} bytes, expected {size}")
    request = urllib.request.Request(url, headers={"User-Agent": "tur-release-installer"})
    digest, count = hashlib.sha256(), 0
    try:
        with urllib.request.urlopen(request, timeout=600) as response:
            served = response.headers.get("Content-Type", "")
            while chunk := response.read(8 << 20):
                digest.update(chunk)
                count += len(chunk)
    except urllib.error.HTTPError as error:
        raise SystemExit(f"read back {url}: HTTP {error.code}. Can anyone download the bucket's objects? "
                         "(docs/releases/README.md, section 4, step 3)") from None
    if count != size or digest.hexdigest() != sha:
        raise SystemExit(f"read back {url}: {count} bytes, SHA-256 {digest.hexdigest()}; expected {size} bytes, {sha}")
    if served.split(";")[0].strip() != kind.split(";")[0].strip():
        raise SystemExit(f"read back {url}: served as {served!r}, expected {kind!r}")
    print(f"read back: {url}: {count} bytes, SHA-256 {sha}, {served}")


def main() -> int:
    if len(sys.argv) < 5:
        print(__doc__, file=sys.stderr)
        return 2
    folder, prefix, out = pathlib.Path(sys.argv[1]), sys.argv[2].strip("/"), pathlib.Path(sys.argv[3])
    bucket = need("TUR_DOWNLOADS_BUCKET")
    public = need("TUR_DOWNLOADS_PUBLIC_URL").rstrip("/")
    cutoff = os.environ.get("TUR_S3_UPLOAD_CUTOFF", "200M")
    chunk = os.environ.get("TUR_S3_CHUNK_SIZE", "64M")
    rclone("version")
    uploaded = []
    for name in sys.argv[4:]:
        path = folder / name
        size, sha, kind = path.stat().st_size, sha256_file(path), content_type(name)
        key = f"{prefix}/{name}"
        remote = f"turdl:{bucket}/{key}"
        url = f"{public}/{key}"
        print(f"== upload {name} ({size} bytes) to {key}")
        rclone("copyto", "--immutable", "--checksum", "--stats-one-line", "--stats", "60s",
               "--s3-upload-cutoff", cutoff, "--s3-chunk-size", chunk, "--s3-upload-concurrency", "4",
               "--header-upload", f"Content-Type: {kind}",
               str(path), remote)
        read_back(remote, url, size, sha, kind)
        uploaded.append({"name": name, "key": key, "url": url, "bytes": size, "sha256": sha})
    out.write_text(json.dumps(uploaded, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

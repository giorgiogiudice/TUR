# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The few GitHub REST calls the installer release makes, with the standard library only.

The API address comes from GITHUB_API_URL (GitHub sets it in every job; the
dry-run test points it at a stand-in), the repository from GITHUB_REPOSITORY,
the token from GH_TOKEN. The token is sent only to the configured API and
upload addresses and never printed.
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.parse
import urllib.request


class ApiError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(f"HTTP {status}: {message}")
        self.status = status


def api_url() -> str:
    return os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")


def repository() -> str:
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    if repo.count("/") != 1:
        raise SystemExit("GITHUB_REPOSITORY must be owner/name")
    return repo


def _token() -> str:
    token = os.environ.get("GH_TOKEN", "")
    if not token:
        raise SystemExit("GH_TOKEN is not set")
    return token


def call(method: str, path: str, body: dict | None = None, *, data: bytes | None = None,
         content_type: str | None = None, url: str | None = None):
    """Call the API; return the decoded JSON (or None for an empty answer).

    `path` is relative to /repos/<repository>; `url` replaces the whole address
    (used for the upload address GitHub hands back).
    """
    target = url or f"{api_url()}/repos/{repository()}{path}"
    allowed = (api_url(), os.environ.get("TUR_UPLOADS_URL", "https://uploads.github.com").rstrip("/"))
    if not target.startswith(allowed):
        raise SystemExit(f"refusing to send the token to {urllib.parse.urlsplit(target).netloc}")
    headers = {
        "Authorization": f"Bearer {_token()}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "tur-release-installer",
    }
    if body is not None:
        data = json.dumps(body).encode()
        content_type = "application/json"
    if content_type:
        headers["Content-Type"] = content_type
    request = urllib.request.Request(target, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=600) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")[:500]
        raise ApiError(error.code, detail) from None
    return json.loads(raw) if raw.strip() else None


def exists(path: str) -> bool:
    """True if GET path answers 200, False on 404; anything else is an error."""
    try:
        call("GET", path)
        return True
    except ApiError as error:
        if error.status == 404:
            return False
        raise

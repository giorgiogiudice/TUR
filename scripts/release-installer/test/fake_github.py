#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A stand-in for the parts of GitHub's REST API the installer release uses.

    fake_github.py PORT FIXTURE.json TOKEN

Serves, for every repository, what scripts/release-installer/source.py and
github_release.py call, and behaves as GitHub documents it:

- workflow runs and their artifacts, and the comparison of a commit with
  main, from FIXTURE.json ({"runs": {id: run}, "artifacts": {id: [...]},
  "compare": {sha: status}});
- tags (GET /git/ref/tags/T: 404 until made), releases (create, get by id or
  tag, latest, update, delete), asset upload at the address the release
  names. A draft makes no tag; publishing it makes a lightweight tag on
  target_commitish. "latest" is the newest published release that is neither
  a draft nor a pre-release, unless make_latest was "false". An asset whose
  name starts with "reject-" is refused (HTTP 500), so that the test can make
  a release fail half-way.

Every request must carry "Authorization: Bearer TOKEN". GET /_state returns
everything, for the test's assertions. Test use only.
"""

from __future__ import annotations

import json
import re
import sys
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOCK = threading.Lock()
STATE: dict = {"tags": {}, "releases": {}, "next": 1, "requests": []}


class Handler(BaseHTTPRequestHandler):
    fixture: dict = {}
    token = ""
    port = 0

    def log_message(self, fmt, *args):  # quiet; requests are recorded in STATE
        pass

    def reply(self, status: int, body=None) -> None:
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def body(self) -> bytes:
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def handle_any(self, method: str) -> None:
        url = urllib.parse.urlsplit(self.path)
        path, query = url.path, urllib.parse.parse_qs(url.query)
        if path == "/_state":
            with LOCK:
                return self.reply(200, STATE)
        if self.headers.get("Authorization") != f"Bearer {self.token}":
            return self.reply(401, {"message": "Bad credentials"})
        raw = self.body() if method in ("POST", "PATCH", "PUT") else b""
        with LOCK:
            STATE["requests"].append(f"{method} {path}")
            return self.route(method, path, query, raw)

    def route(self, method, path, query, raw):
        m = re.fullmatch(r"(/uploads)?/repos/([^/]+/[^/]+)(/.*)", path)
        if not m:
            return self.reply(404, {"message": "Not Found"})
        upload, repo, rest = m.group(1), m.group(2), m.group(3)
        releases = STATE["releases"]

        if upload:
            r = re.fullmatch(r"/releases/(\d+)/assets", rest)
            if method != "POST" or not r or r.group(1) not in releases:
                return self.reply(404, {"message": "Not Found"})
            name = query.get("name", [""])[0]
            rel = releases[r.group(1)]
            if any(a["name"] == name for a in rel["assets"]):
                return self.reply(422, {"message": "already_exists"})
            if name.startswith("reject-"):  # lets the test make an upload fail
                return self.reply(500, {"message": "Server Error"})
            asset = {"id": len(rel["assets"]) + 1, "name": name, "size": len(raw),
                     "content_type": self.headers.get("Content-Type")}
            rel["assets"].append(asset)
            return self.reply(201, asset)

        if method == "GET" and (r := re.fullmatch(r"/actions/runs/(\d+)", rest)):
            run = self.fixture["runs"].get(r.group(1))
            return self.reply(200, run) if run else self.reply(404, {"message": "Not Found"})
        if method == "GET" and (r := re.fullmatch(r"/actions/runs/(\d+)/artifacts", rest)):
            return self.reply(200, {"artifacts": self.fixture["artifacts"].get(r.group(1), [])})
        if method == "GET" and (r := re.fullmatch(r"/compare/([0-9a-f]{40})\.\.\.main", rest)):
            return self.reply(200, {"status": self.fixture["compare"].get(r.group(1), "diverged")})
        if method == "GET" and (r := re.fullmatch(r"/git/ref/tags/(.+)", rest)):
            sha = STATE["tags"].get(r.group(1))
            if sha is None:
                return self.reply(404, {"message": "Not Found"})
            return self.reply(200, {"ref": f"refs/tags/{r.group(1)}", "object": {"type": "commit", "sha": sha}})
        if method == "GET" and (r := re.fullmatch(r"/releases/tags/(.+)", rest)):
            for rel in releases.values():
                if rel["tag_name"] == r.group(1) and not rel["draft"]:
                    return self.reply(200, rel)
            return self.reply(404, {"message": "Not Found"})
        if method == "GET" and rest == "/releases/latest":
            candidates = [rel for rel in releases.values()
                          if not rel["draft"] and not rel["prerelease"] and rel["_latest_allowed"]]
            if not candidates:
                return self.reply(404, {"message": "Not Found"})
            return self.reply(200, max(candidates, key=lambda rel: rel["id"]))
        if method == "POST" and rest == "/releases":
            data = json.loads(raw)
            if data["tag_name"] in STATE["tags"]:
                return self.reply(422, {"message": "tag already exists"})
            rid = STATE["next"]
            STATE["next"] += 1
            rel = {
                "id": rid, "tag_name": data["tag_name"], "target_commitish": data.get("target_commitish"),
                "name": data.get("name"), "body": data.get("body"), "draft": bool(data.get("draft")),
                "prerelease": bool(data.get("prerelease")), "assets": [],
                "_latest_allowed": data.get("make_latest", "true") != "false",
                "html_url": f"http://127.0.0.1:{self.port}/{repo}/releases/tag/{data['tag_name']}",
                "upload_url": f"http://127.0.0.1:{self.port}/uploads/repos/{repo}/releases/{rid}/assets{{?name,label}}",
            }
            releases[str(rid)] = rel
            if not rel["draft"]:
                STATE["tags"][rel["tag_name"]] = rel["target_commitish"]
            return self.reply(201, rel)
        if (r := re.fullmatch(r"/releases/(\d+)", rest)) and r.group(1) in releases:
            rel = releases[r.group(1)]
            if method == "GET":
                return self.reply(200, rel)
            if method == "DELETE":
                del releases[r.group(1)]
                return self.reply(204)
            if method == "PATCH":
                data = json.loads(raw)
                for key in ("draft", "prerelease", "name", "body"):
                    if key in data:
                        rel[key] = data[key]
                if "make_latest" in data:
                    rel["_latest_allowed"] = data["make_latest"] != "false"
                if not rel["draft"] and rel["tag_name"] not in STATE["tags"]:
                    STATE["tags"][rel["tag_name"]] = rel["target_commitish"]
                return self.reply(200, rel)
        return self.reply(404, {"message": "Not Found"})

    def do_GET(self):
        self.handle_any("GET")

    def do_POST(self):
        self.handle_any("POST")

    def do_PATCH(self):
        self.handle_any("PATCH")

    def do_DELETE(self):
        self.handle_any("DELETE")


def main() -> int:
    port, fixture, token = int(sys.argv[1]), sys.argv[2], sys.argv[3]
    Handler.fixture = json.load(open(fixture))
    Handler.token = token
    Handler.port = port
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())

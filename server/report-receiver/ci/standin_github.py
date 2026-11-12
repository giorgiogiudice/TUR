#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A stand-in for the two GitHub REST endpoints the receiver calls.

    POST /repos/{owner}/{repo}/issues                 -> 201 {"number": n}
    POST /repos/{owner}/{repo}/issues/{n}/comments    -> 201, or 404 for an unknown issue

Every request is appended to CALLS (one JSON object per line, with the body
as text), before it is answered, so the test can read exactly what the
receiver sent. A request without the expected bearer token is answered 401.
Issue numbers survive a restart (STATE), so the stand-in can be stopped and
started again to play "GitHub is down". It listens on 127.0.0.1 only and
never contacts anything: the real GitHub is never called from tests.

    standin_github.py --port 9100 --repository OWNER/NAME --token-file FILE \
                      --calls calls.jsonl --state state.json
"""

import argparse
import http.server
import json
import re
import threading
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--token-file", type=Path, required=True)
    parser.add_argument("--calls", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    args = parser.parse_args()

    token = args.token_file.read_text().strip()
    lock = threading.Lock()
    state = json.loads(args.state.read_text()) if args.state.exists() else {"issues": []}
    issues_path = re.compile(r"/repos/" + re.escape(args.repository) + r"/issues")
    comments_path = re.compile(r"/repos/" + re.escape(args.repository) + r"/issues/(\d+)/comments")

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_):
            pass

        def answer(self, status, document):
            data = json.dumps(document).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def record(self, body):
            entry = {
                "method": self.command,
                "path": self.path,
                "authorized": self.headers.get("Authorization") == f"Bearer {token}",
                "api_version": self.headers.get("X-GitHub-Api-Version"),
                "accept": self.headers.get("Accept"),
                "body": body,
            }
            with lock, args.calls.open("a") as calls:
                calls.write(json.dumps(entry) + "\n")
                calls.flush()
            return entry

        def do_GET(self):
            self.record("")
            self.answer(404, {"message": "Not Found"})

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length).decode("utf-8", "replace")
            entry = self.record(body)
            if not entry["authorized"]:
                self.answer(401, {"message": "Bad credentials"})
                return
            try:
                document = json.loads(body)
            except ValueError:
                self.answer(400, {"message": "Problems parsing JSON"})
                return
            with lock:
                if issues_path.fullmatch(self.path):
                    if not isinstance(document.get("title"), str) or not isinstance(document.get("body"), str):
                        self.answer(422, {"message": "Validation Failed"})
                        return
                    number = len(state["issues"]) + 1
                    state["issues"].append({"number": number, "title": document["title"], "comments": 0})
                    args.state.write_text(json.dumps(state))
                    self.answer(201, {"number": number, "html_url": f"http://127.0.0.1/{number}"})
                    return
                match = comments_path.fullmatch(self.path)
                if match:
                    number = int(match.group(1))
                    if not 1 <= number <= len(state["issues"]):
                        self.answer(404, {"message": "Not Found"})
                        return
                    state["issues"][number - 1]["comments"] += 1
                    args.state.write_text(json.dumps(state))
                    self.answer(201, {"id": number * 1000 + state["issues"][number - 1]["comments"]})
                    return
            self.answer(404, {"message": "Not Found"})

    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()

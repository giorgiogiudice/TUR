#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The first step of the workflow pax-live: is there a key, and does the
Anthropic API accept it?

    live_ping.py MODEL

Reads the key from the environment variable TUR_PAX_API_KEY (the workflow
fills it from the repository secret CLAUDE_KEY). Never prints it, and never
puts it anywhere but the request header.

* No key: says so, writes `present=false` to $GITHUB_OUTPUT, ends with 0.
  Nothing is called.
* A key: sends one minimal request (one short message, at most 8 output
  tokens) to POST https://api.anthropic.com/v1/messages with the API version
  the adapter is pinned to. Accepted: writes `present=true`, ends with 0.
  Rejected or unreachable: reports the HTTP status and the error type the API
  gave, plainly, and ends with 1, so that nothing else runs.

Its cost: one request of about 15 input and at most 8 output tokens.
"""

import json
import os
import sys
import urllib.error
import urllib.request

ENDPOINT = "https://api.anthropic.com/v1/messages"
# The version services/tur-pax-providers/src/anthropic.rs pins (API_VERSION).
API_VERSION = "2026-10-21"


def output(name, value):
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(f"{name}={value}\n")


def main():
    model = sys.argv[1] if len(sys.argv) > 1 else "claude-haiku-5-5"
    key = os.environ.get("TUR_PAX_API_KEY", "").strip()
    if not key:
        print("::notice::The secret CLAUDE_KEY is not set. Nothing was called.", flush=True)
        output("present", "false")
        return 0
    body = json.dumps(
        {"model": model, "max_tokens": 8, "messages": [{"role": "user", "content": "Reply with OK."}]}
    ).encode()
    request = urllib.request.Request(
        ENDPOINT,
        data=body,
        method="POST",
        headers={
            "content-type": "application/json",
            "anthropic-version": API_VERSION,
            "x-api-key": key,
            "user-agent": "tur-pax-live-check",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            answer = json.loads(response.read() or b"{}")
            usage = answer.get("usage", {})
            print(
                f"The API accepted the key: HTTP {response.status}, model {answer.get('model')}, "
                f"{usage.get('input_tokens')} input and {usage.get('output_tokens')} output tokens.",
                flush=True,
            )
            output("present", "true")
            return 0
    except urllib.error.HTTPError as error:
        try:
            detail = json.loads(error.read() or b"{}")
        except ValueError:
            detail = {}
        kind = (detail.get("error") or {}).get("type", "unknown")
        message = str((detail.get("error") or {}).get("message", ""))
        # The API's own sentence, with anything that is the key taken out.
        message = message.replace(key, "[the key]")[:300]
        print(
            f"::error::The Anthropic API rejected the request: HTTP {error.code}, error type {kind}. {message}",
            flush=True,
        )
    except (urllib.error.URLError, OSError) as error:
        print(f"::error::The Anthropic API could not be reached: {type(error).__name__}.", flush=True)
    output("present", "false")
    return 1


if __name__ == "__main__":
    sys.exit(main())

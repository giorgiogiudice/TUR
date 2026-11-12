#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The first step of the workflow pax-live: is there a key, and does the
provider's API accept it?

    live_ping.py PROVIDER MODEL

PROVIDER is `anthropic` (the Messages API, secret CLAUDE_KEY) or `deepseek`
(the OpenAI-compatible Chat Completions API of DeepSeek, secret
DEEPSEEK_KEY). Reads the key from the environment variable TUR_PAX_API_KEY,
which the workflow fills from that provider's secret and from no other.
Never prints it, and never puts it anywhere but the request header.

* No key: says so, writes `present=false` to $GITHUB_OUTPUT, ends with 0.
  Nothing is called.
* A key: sends one minimal request (one short message, at most 8 output
  tokens) to the provider's public address: POST
  https://api.anthropic.com/v1/messages with the API version the adapter is
  pinned to, or POST https://api.deepseek.com/chat/completions. Accepted:
  writes `present=true`, ends with 0. Rejected or unreachable: reports the
  HTTP status and the error type the API gave, plainly, and ends with 1, so
  that nothing else runs.

Its cost: one request of about 15 input and at most 8 output tokens.
"""

import json
import os
import sys
import urllib.error
import urllib.request

# The version services/tur-pax-providers/src/anthropic.rs pins (API_VERSION).
API_VERSION = "2026-10-21"

# Each provider pax-live can call: its address, its secret, how the key is
# sent. DeepSeek's from https://api-docs.deepseek.com, read on 2026-11-12.
PROVIDERS = {
    "anthropic": {
        "endpoint": "https://api.anthropic.com/v1/messages",
        "secret": "CLAUDE_KEY",
        "name": "Anthropic",
    },
    "deepseek": {
        "endpoint": "https://api.deepseek.com/chat/completions",
        "secret": "DEEPSEEK_KEY",
        "name": "DeepSeek",
    },
}


def request_for(provider, model, key):
    """The minimal request, with the key in the provider's header only."""
    body = {"model": model, "max_tokens": 8, "messages": [{"role": "user", "content": "Reply with OK."}]}
    headers = {"content-type": "application/json", "user-agent": "tur-pax-live-check"}
    if provider == "anthropic":
        headers["anthropic-version"] = API_VERSION
        headers["x-api-key"] = key
    else:
        headers["authorization"] = f"Bearer {key}"
    return urllib.request.Request(
        PROVIDERS[provider]["endpoint"], data=json.dumps(body).encode(), method="POST", headers=headers
    )


def counts(answer):
    """The input and output tokens a response reports, in either format."""
    usage = answer.get("usage") or {}
    return (
        usage.get("input_tokens", usage.get("prompt_tokens")),
        usage.get("output_tokens", usage.get("completion_tokens")),
    )


def output(name, value):
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(f"{name}={value}\n")


def main():
    provider = sys.argv[1] if len(sys.argv) > 1 else "anthropic"
    model = sys.argv[2] if len(sys.argv) > 2 else "claude-haiku-5-5"
    if provider not in PROVIDERS:
        print(f"::error::The provider must be one of {', '.join(PROVIDERS)}. Nothing was called.", flush=True)
        output("present", "false")
        return 1
    name = PROVIDERS[provider]["name"]
    key = os.environ.get("TUR_PAX_API_KEY", "").strip()
    if not key:
        secret = PROVIDERS[provider]["secret"]
        print(f"::notice::The secret {secret} is not set. Nothing was called.", flush=True)
        output("present", "false")
        return 0
    request = request_for(provider, model, key)
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            answer = json.loads(response.read() or b"{}")
            input_tokens, output_tokens = counts(answer)
            print(
                f"The {name} API accepted the key: HTTP {response.status}, model {answer.get('model')}, "
                f"{input_tokens} input and {output_tokens} output tokens.",
                flush=True,
            )
            output("present", "true")
            return 0
    except urllib.error.HTTPError as error:
        try:
            detail = json.loads(error.read() or b"{}")
        except ValueError:
            detail = {}
        error_object = detail.get("error") if isinstance(detail, dict) else None
        if not isinstance(error_object, dict):
            error_object = {}
        kind = error_object.get("type", "unknown")
        message = str(error_object.get("message", ""))
        # The API's own sentence, with anything that is the key taken out.
        message = message.replace(key, "[the key]")[:300]
        print(
            f"::error::The {name} API rejected the request: HTTP {error.code}, error type {kind}. {message}",
            flush=True,
        )
    except (urllib.error.URLError, OSError) as error:
        print(f"::error::The {name} API could not be reached: {type(error).__name__}.", flush=True)
    output("present", "false")
    return 1


if __name__ == "__main__":
    sys.exit(main())

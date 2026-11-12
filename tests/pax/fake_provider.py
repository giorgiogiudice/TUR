#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A scripted model provider for the P@x end-to-end test.

It is a local HTTP server that answers each request with the next response of
a script, in the streaming format of the Anthropic Messages API, and records
every request it receives. It is not a model and it calls none: a script says
what the model "does", turn by turn.

A script is a list of turns. A turn is an object:

    { "text": "...",                       text the model writes (optional)
      "calls": [                           tool calls it makes (optional)
        { "id": "toolu_1",
          "name": "tool_name",
          "arguments": {...} | "raw JSON text" } ],
      "stop": "end_turn" | "tool_use" | "refusal" | "max_tokens",
      "input_tokens": 40, "output_tokens": 30 }

When the arguments are given as text they are sent exactly as written, which
is how a script repeats a key or sends something that is not valid.

Used as a module by e2e.py. Run on its own it serves one script file:

    fake_provider.py SCRIPT.json [--port N] [--repeat-last]
"""

import argparse
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def _event(name, data):
    return f"event: {name}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n"


def render(turn):
    """The turn as an event stream of the Messages API."""
    stream = _event(
        "message_start",
        {
            "type": "message_start",
            "message": {
                "id": "msg_scripted",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "scripted-model",
                "stop_reason": None,
                "stop_sequence": None,
                "usage": {"input_tokens": turn.get("input_tokens", 40), "output_tokens": 1},
            },
        },
    )
    index = 0
    text = turn.get("text", "")
    if text:
        stream += _event(
            "content_block_start",
            {"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}},
        )
        stream += _event(
            "content_block_delta",
            {"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}},
        )
        stream += _event("content_block_stop", {"type": "content_block_stop", "index": index})
        index += 1
    for call in turn.get("calls", []):
        arguments = call.get("arguments", {})
        if not isinstance(arguments, str):
            arguments = json.dumps(arguments)
        stream += _event(
            "content_block_start",
            {
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "tool_use", "id": call["id"], "name": call["name"], "input": {}},
            },
        )
        # The argument text arrives in two fragments.
        middle = len(arguments) // 2
        for fragment in (arguments[:middle], arguments[middle:]):
            stream += _event(
                "content_block_delta",
                {
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {"type": "input_json_delta", "partial_json": fragment},
                },
            )
        stream += _event("content_block_stop", {"type": "content_block_stop", "index": index})
        index += 1
    stop = turn.get("stop") or ("tool_use" if turn.get("calls") else "end_turn")
    delta = {"stop_reason": stop, "stop_sequence": None}
    if stop == "refusal":
        delta["stop_details"] = {
            "type": "refusal",
            "category": "scripted",
            "explanation": "The scripted model declined.",
        }
    stream += _event(
        "message_delta",
        {"type": "message_delta", "delta": delta, "usage": {"output_tokens": turn.get("output_tokens", 30)}},
    )
    stream += _event("message_stop", {"type": "message_stop"})
    return stream


class ScriptedProvider:
    """Serves a script on a local port and records what it is sent."""

    def __init__(self, turns, repeat_last=False, port=0):
        self.turns = list(turns)
        self.repeat_last = repeat_last
        self.requests = []
        self._lock = threading.Lock()
        self._served = 0
        provider = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_args):
                pass

            def do_POST(self):
                length = int(self.headers.get("content-length", "0"))
                body = self.rfile.read(length).decode("utf-8", "replace")
                with provider._lock:
                    provider.requests.append(
                        {
                            "path": self.path,
                            "headers": {name.lower(): value for name, value in self.headers.items()},
                            "body": body,
                        }
                    )
                    position = provider._served
                    provider._served += 1
                if position < len(provider.turns):
                    turn = provider.turns[position]
                elif provider.repeat_last and provider.turns:
                    turn = provider.turns[-1]
                else:
                    turn = None
                if turn is None:
                    payload = json.dumps(
                        {
                            "type": "error",
                            "error": {"type": "invalid_request_error", "message": "the script has no more turns"},
                        }
                    ).encode()
                    self.send_response(400)
                    self.send_header("content-type", "application/json")
                else:
                    payload = render(turn).encode()
                    self.send_response(200)
                    self.send_header("content-type", "text/event-stream")
                    self.send_header("cache-control", "no-cache")
                self.send_header("content-length", str(len(payload)))
                self.send_header("connection", "close")
                self.end_headers()
                try:
                    self.wfile.write(payload)
                    self.wfile.flush()
                except OSError:
                    # The client may stop reading: a spend limit cuts a response.
                    pass
                self.close_connection = True

        self._server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def base_url(self):
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    def start(self):
        self._thread.start()
        return self

    def stop(self):
        self._server.shutdown()
        self._server.server_close()

    def bodies(self):
        """The bodies of the requests received so far, as JSON."""
        with self._lock:
            return [json.loads(request["body"]) for request in self.requests]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("script")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--repeat-last", action="store_true")
    arguments = parser.parse_args()
    with open(arguments.script, encoding="utf-8") as handle:
        script = json.load(handle)
    turns = script["turns"] if isinstance(script, dict) else script
    provider = ScriptedProvider(turns, arguments.repeat_last, arguments.port).start()
    print(provider.base_url, flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        provider.stop()


if __name__ == "__main__":
    sys.exit(main())

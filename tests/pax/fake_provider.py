#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A scripted model provider for the P@x end-to-end test.

It is a local HTTP server that answers each request with the next response of
a script, in the streaming format of one of the four API families P@x has an
adapter for (the Anthropic Messages API, the OpenAI Responses API, the
Google Interactions API, or the OpenAI-compatible Chat Completions API of
DeepSeek and others), and records every request it receives. It is not a
model and it calls none: a script says what the model "does", turn by turn.
The formats are written by hand from the providers' documentation, the
same way as the conformance scripts of services/tur-pax-providers; they are
not recordings of a real provider.

A script is a list of turns. A turn is an object:

    { "text": "...",                       text the model writes (optional)
      "calls": [                           tool calls it makes (optional)
        { "id": "toolu_1",
          "name": "tool_name",
          "arguments": {...} | "raw JSON text" } ],
      "whole": true,                       the arguments of each call arrive
                                           in one piece, not in fragments
      "stop": "end_turn" | "tool_use" | "refusal" | "max_tokens",
      "input_tokens": 40, "output_tokens": 30 }

When the arguments are given as text they are sent exactly as written, which
is how a script repeats a key or sends something that is not valid.

Used as a module by e2e.py. Run on its own it serves one script file:

    fake_provider.py SCRIPT.json [--port N] [--repeat-last] [--wire NAME]
"""

import argparse
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def _event(name, data):
    return f"event: {name}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n"


WIRES = ("anthropic", "openai", "google", "openai-chat")


def _raw_event(name, data):
    """An event whose data is written by hand: it can repeat a key."""
    return f"event: {name}\ndata: {data}\n\n"


def _argument_text(call):
    arguments = call.get("arguments", {})
    if not isinstance(arguments, str):
        arguments = json.dumps(arguments)
    return arguments


def render(turn, wire="anthropic"):
    """The turn as an event stream in the given wire format."""
    if wire == "openai":
        return render_openai(turn)
    if wire == "google":
        return render_google(turn)
    if wire == "openai-chat":
        return render_openai_chat(turn)
    return render_anthropic(turn)


def render_openai_chat(turn):
    """The turn as a stream of the OpenAI-compatible Chat Completions API.

    Written by hand from OpenAI's Chat Completions reference and DeepSeek's
    API documentation: data-only chunks, reasoning in `reasoning_content`
    (DeepSeek's thinking mode), a streamed call that starts with its id and
    name and whose argument text follows in two deltas, or a call whose
    arguments arrive whole in one delta; the finish, a usage chunk with no
    choices, then `[DONE]`.
    """

    def chunk(choices, usage=None):
        data = {
            "id": "chatcmpl-scripted",
            "object": "chat.completion.chunk",
            "created": 1794477600,
            "model": "scripted-model",
            "choices": choices,
            "usage": usage,
        }
        return f"data: {json.dumps(data, separators=(',', ':'))}\n\n"

    def delta(fields):
        return [{"index": 0, "delta": fields, "finish_reason": None}]

    def raw_delta(fields_text):
        """A delta written by hand: the argument text exactly as scripted."""
        return (
            '{"id":"chatcmpl-scripted","object":"chat.completion.chunk","created":1794477600,'
            '"model":"scripted-model","choices":[{"index":0,"delta":%s,"finish_reason":null}],'
            '"usage":null}' % fields_text
        )

    stream = chunk(delta({"role": "assistant", "content": "", "reasoning_content": "Thinking it over."}))
    text = turn.get("text", "")
    if text:
        stream += chunk(delta({"content": text}))
    for index, call in enumerate(turn.get("calls", [])):
        arguments = _argument_text(call)
        start = {"index": index, "id": call["id"], "type": "function", "function": {"name": call["name"]}}
        if turn.get("whole"):
            start["function"]["arguments"] = arguments
            stream += f"data: {raw_delta(json.dumps({'tool_calls': [start]}))}\n\n"
            continue
        start["function"]["arguments"] = ""
        stream += chunk(delta({"tool_calls": [start]}))
        middle = len(arguments) // 2
        for fragment in (arguments[:middle], arguments[middle:]):
            stream += chunk(delta({"tool_calls": [{"index": index, "function": {"arguments": fragment}}]}))
    stop = turn.get("stop") or ("tool_use" if turn.get("calls") else "end_turn")
    if stop == "refusal":
        stream += chunk(delta({"refusal": "The scripted model declined."}))
    finish = {"max_tokens": "length", "refusal": "stop", "tool_use": "tool_calls"}.get(stop, "stop")
    stream += chunk([{"index": 0, "delta": {}, "finish_reason": finish}])
    input_tokens = turn.get("input_tokens", 40)
    output_tokens = turn.get("output_tokens", 30)
    stream += chunk(
        [],
        {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
            "prompt_tokens_details": {"cached_tokens": 0},
            "completion_tokens_details": {"reasoning_tokens": 3},
        },
    )
    return stream + "data: [DONE]\n\n"


def render_openai(turn):
    """The turn as an event stream of the OpenAI Responses API."""

    def response(status):
        return {"id": "resp_scripted", "object": "response", "status": status, "model": "scripted-model", "output": []}

    stream = _event("response.created", {"type": "response.created", "response": response("in_progress")})
    index = 0
    text = turn.get("text", "")
    if text:
        stream += _event(
            "response.output_item.added",
            {
                "type": "response.output_item.added",
                "output_index": index,
                "item": {"id": "msg_scripted", "type": "message", "status": "in_progress", "role": "assistant", "content": []},
            },
        )
        stream += _event(
            "response.output_text.delta",
            {
                "type": "response.output_text.delta",
                "item_id": "msg_scripted",
                "output_index": index,
                "content_index": 0,
                "delta": text,
            },
        )
        stream += _event(
            "response.output_item.done",
            {
                "type": "response.output_item.done",
                "output_index": index,
                "item": {
                    "id": "msg_scripted",
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text, "annotations": []}],
                },
            },
        )
        index += 1
    for call in turn.get("calls", []):
        arguments = _argument_text(call)
        item_id = f"fc_{index}"
        if not turn.get("whole"):
            stream += _event(
                "response.output_item.added",
                {
                    "type": "response.output_item.added",
                    "output_index": index,
                    "item": {
                        "type": "function_call",
                        "id": item_id,
                        "call_id": call["id"],
                        "name": call["name"],
                        "arguments": "",
                    },
                },
            )
            middle = len(arguments) // 2
            for fragment in (arguments[:middle], arguments[middle:]):
                stream += _event(
                    "response.function_call_arguments.delta",
                    {
                        "type": "response.function_call_arguments.delta",
                        "item_id": item_id,
                        "output_index": index,
                        "delta": fragment,
                    },
                )
            stream += _event(
                "response.function_call_arguments.done",
                {
                    "type": "response.function_call_arguments.done",
                    "item_id": item_id,
                    "output_index": index,
                    "arguments": arguments,
                },
            )
        stream += _event(
            "response.output_item.done",
            {
                "type": "response.output_item.done",
                "output_index": index,
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": call["id"],
                    "name": call["name"],
                    "arguments": arguments,
                    "status": "completed",
                },
            },
        )
        index += 1
    input_tokens = turn.get("input_tokens", 40)
    output_tokens = turn.get("output_tokens", 30)
    usage = {
        "input_tokens": input_tokens,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": output_tokens,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": input_tokens + output_tokens,
    }
    stop = turn.get("stop")
    if stop == "max_tokens":
        ended = response("incomplete")
        ended["incomplete_details"] = {"reason": "max_output_tokens"}
        ended["usage"] = usage
        return stream + _event("response.incomplete", {"type": "response.incomplete", "response": ended})
    if stop == "refusal":
        stream += _event(
            "response.refusal.done",
            {
                "type": "response.refusal.done",
                "item_id": "msg_refusal",
                "output_index": index,
                "content_index": 0,
                "refusal": "The scripted model declined.",
            },
        )
    ended = response("completed")
    ended["usage"] = usage
    return stream + _event("response.completed", {"type": "response.completed", "response": ended})


def render_google(turn):
    """The turn as an event stream of the Google Interactions API."""
    stream = _event(
        "interaction.created",
        {
            "interaction": {"id": "v1_scripted", "status": "in_progress", "object": "interaction", "model": "scripted-model"},
            "event_type": "interaction.created",
        },
    )
    index = 0
    text = turn.get("text", "")
    if text:
        stream += _event("step.start", {"index": index, "step": {"type": "model_output"}, "event_type": "step.start"})
        stream += _event(
            "step.delta", {"index": index, "delta": {"text": text, "type": "text"}, "event_type": "step.delta"}
        )
        stream += _event("step.stop", {"index": index, "event_type": "step.stop"})
        index += 1
    for call in turn.get("calls", []):
        arguments = _argument_text(call)
        if turn.get("whole"):
            # The step starts with its arguments complete, as a JSON object
            # written exactly as the script has it, and has no delta.
            stream += _raw_event(
                "step.start",
                '{"index":%d,"step":{"id":%s,"type":"function_call","name":%s,"arguments":%s},"event_type":"step.start"}'
                % (index, json.dumps(call["id"]), json.dumps(call["name"]), arguments),
            )
        else:
            stream += _event(
                "step.start",
                {
                    "index": index,
                    "step": {"id": call["id"], "type": "function_call", "name": call["name"], "arguments": {}},
                    "event_type": "step.start",
                },
            )
            middle = len(arguments) // 2
            for fragment in (arguments[:middle], arguments[middle:]):
                stream += _event(
                    "step.delta",
                    {
                        "index": index,
                        "delta": {"arguments": fragment, "type": "arguments_delta"},
                        "event_type": "step.delta",
                    },
                )
        stream += _event("step.stop", {"index": index, "event_type": "step.stop"})
        index += 1
    stop = turn.get("stop") or ("tool_use" if turn.get("calls") else "end_turn")
    if stop == "refusal":
        return stream + _event(
            "error",
            {"error": {"message": "Safety violations blocked the request.", "code": "safety"}, "event_type": "error"},
        )
    status = {"max_tokens": "incomplete", "tool_use": "requires_action"}.get(stop, "completed")
    input_tokens = turn.get("input_tokens", 40)
    output_tokens = turn.get("output_tokens", 30)
    stream += _event(
        "interaction.completed",
        {
            "interaction": {
                "id": "v1_scripted",
                "status": status,
                "usage": {
                    "total_tokens": input_tokens + output_tokens,
                    "total_input_tokens": input_tokens,
                    "total_cached_tokens": 0,
                    "total_output_tokens": output_tokens,
                    "total_tool_use_tokens": 0,
                    "total_thought_tokens": 0,
                },
                "object": "interaction",
                "model": "scripted-model",
            },
            "event_type": "interaction.completed",
        },
    )
    return stream + "event: done\ndata: [DONE]\n\n"


def render_anthropic(turn):
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
        arguments = _argument_text(call)
        if turn.get("whole"):
            # The block starts with its whole input, written exactly as the
            # script has it, and has no delta.
            stream += _raw_event(
                "content_block_start",
                '{"type":"content_block_start","index":%d,"content_block":{"type":"tool_use","id":%s,"name":%s,"input":%s}}'
                % (index, json.dumps(call["id"]), json.dumps(call["name"]), arguments),
            )
            stream += _event("content_block_stop", {"type": "content_block_stop", "index": index})
            index += 1
            continue
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

    def __init__(self, turns, repeat_last=False, port=0, wire="anthropic"):
        if wire not in WIRES:
            raise ValueError(f"unknown wire format: {wire}")
        self.turns = list(turns)
        self.repeat_last = repeat_last
        self.wire = wire
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
                    payload = render(turn, provider.wire).encode()
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
    parser.add_argument("--wire", choices=WIRES, default=None)
    arguments = parser.parse_args()
    with open(arguments.script, encoding="utf-8") as handle:
        script = json.load(handle)
    turns = script["turns"] if isinstance(script, dict) else script
    wire = arguments.wire or (script.get("wire") if isinstance(script, dict) else None) or "anthropic"
    provider = ScriptedProvider(turns, arguments.repeat_last, arguments.port, wire).start()
    print(provider.base_url, flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        provider.stop()


if __name__ == "__main__":
    sys.exit(main())

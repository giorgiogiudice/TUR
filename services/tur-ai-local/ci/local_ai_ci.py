#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What the local-ai workflow runs against a real engine and real models.

    local_ai_ci.py proof   --tal BIN --catalogue DIR --data DIR --runtime DIR --evidence DIR [--engine FILE]
    local_ai_ci.py table   (same options) [--models ID,...] [--label TEXT] [--remove-after]
    local_ai_ci.py ollama  --ollama-url URL --ollama-model NAME --evidence DIR
    local_ai_ci.py harness --base-url URL --key-file FILE --model ID --scenario FILE --tools DIR... --evidence DIR
    local_ai_ci.py summary --evidence DIR [--out FILE]

proof: the smallest model through tur-ai-local, from download to idle stop.
Every check that is about TUR (download, verification, refusal of a changed
file, serving on the loopback interface only, the key, idle stop) must pass,
or the command fails. What is about the model (whether its answer is
sensible, whether it calls a tool well) is measured and recorded, and never
turns a run green or red.

table: every catalogue model (or those named): download, serve, measure
generation and prompt speed, peak memory, a tool call, image input or
embeddings, and stop.

ollama: the same measurements of the smallest model through Ollama, for the
engine comparison.

harness: a minimal stand-in for P@x that speaks Chat Completions: it offers
the tools of TUR's real manifests, checks every tool call the model makes
against the tool's input schema (unknown tools, repeated keys, missing or
extra arguments are malformed), runs the call through the tool's real
adapter program WITHOUT P@x's sandbox and policy engine, and gives the
result back. It is not P@x. It exists because P@x has no Chat Completions
adapter yet.

Nothing here prints the access key.
"""

import argparse
import base64
import http.client
import json
import os
import re
import socket
import struct
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path

try:
    import tomllib
except ImportError:  # pragma: no cover - Python older than 3.11
    tomllib = None

STORY = (
    "The lighthouse stood on a rock at the end of the world, and for forty years "
    "Mara had kept its lamp burning. Every evening she climbed the hundred and "
    "twelve steps, trimmed the wick, polished the great lens and watched the beam "
    "sweep across the black water. Ships passed far out, their lights small as "
    "stars, and none of them ever knew her name. One winter morning, after a storm "
    "that had shaken the tower until the windows sang, she walked down to the "
    "shingle and found a green bottle wedged between two stones. Inside was a "
    "letter, folded small and tied with red thread. The ink had run in places, but "
    "the first line was clear: If you are reading this, the lamp still burns."
)

WEATHER_TOOL = {
    "type": "function",
    "function": {
        "name": "get_weather",
        "description": "Get the current weather for a city.",
        "parameters": {
            "type": "object",
            "properties": {"city": {"type": "string", "description": "The name of the city"}},
            "required": ["city"],
            "additionalProperties": False,
        },
    },
}


class DuplicateKey(ValueError):
    pass


def strict_json(text):
    """Parse JSON, refusing an object that repeats a key at any depth
    (AI control layer, section 22, item 1)."""

    def hook(pairs):
        seen = set()
        for key, _ in pairs:
            if key in seen:
                raise DuplicateKey(key)
            seen.add(key)
        return dict(pairs)

    return json.loads(text, object_pairs_hook=hook)


class Results:
    """Checks and measurements, written as JSON as they come."""

    def __init__(self, path, title):
        self.path = path
        self.data = {"title": title, "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "checks": [], "measurements": {}}

    def check(self, name, passed, detail="", required=True):
        self.data["checks"].append({"name": name, "passed": bool(passed), "required": required, "detail": str(detail)[:4000]})
        mark = "ok  " if passed else ("FAIL" if required else "no  ")
        print(f"  {mark} {name}" + (f"  [{str(detail)[:400]}]" if detail and not passed else ""), flush=True)
        self.save()
        return bool(passed)

    def measure(self, key, value):
        self.data["measurements"][key] = value
        self.save()

    def failed_required(self):
        return [c for c in self.data["checks"] if c["required"] and not c["passed"]]

    def save(self):
        self.path.write_text(json.dumps(self.data, indent=2))


# ---------------------------------------------------------------- tur-ai-local


class Tal:
    """The tur-ai-local program with the places of this run."""

    def __init__(self, args, data=None):
        self.binary = args.tal
        self.places = [
            "--catalogue", str(args.catalogue),
            "--models-dir", str(Path(data or args.data) / "tur" / "models"),
            "--runtime-dir", str(args.runtime),
        ]
        if args.engine:
            self.places += ["--engine", str(args.engine)]
        self.runtime = Path(args.runtime) / "tur-ai-local"

    def run(self, *arguments, timeout=3600):
        command = [self.binary, *arguments, *self.places]
        started = time.time()
        process = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return process, time.time() - started

    def json(self, *arguments, timeout=3600):
        process, elapsed = self.run(*arguments, "--json", timeout=timeout)
        document = None
        if process.returncode == 0:
            try:
                document = json.loads(process.stdout)
            except ValueError:
                document = None
        return process, document, elapsed

    def key(self):
        return (self.runtime / "api-key").read_text().strip()


class Served:
    """A model served by `tur-ai-local serve --json`, read line by line."""

    def __init__(self, tal, model, port, idle, extra=()):
        self.tal = tal
        self.model = model
        command = [tal.binary, "serve", model, "--json", "--port", str(port), "--idle-stop", str(idle),
                   "--start-timeout", "900", *extra, *tal.places]
        self.started = time.time()
        self.process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.lines = []
        self.stderr = ""
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._reader.start()
        self._err = threading.Thread(target=self._read_err, daemon=True)
        self._err.start()

    def _read(self):
        for line in self.process.stdout:
            if line.strip():
                self.lines.append(line.strip())

    def _read_err(self):
        self.stderr = self.process.stderr.read()

    def _find(self, event):
        for line in list(self.lines):
            try:
                document = json.loads(line)
            except ValueError:
                continue
            if document.get("event") == event:
                return document
        return None

    def wait_event(self, event, timeout):
        deadline = time.time() + timeout
        while True:
            found = self._find(event)
            if found is not None:
                return found
            if self.process.poll() is not None:
                # It has ended: read what is left, look once more.
                self._reader.join(timeout=5)
                self._err.join(timeout=5)
                return self._find(event)
            if time.time() >= deadline:
                return None
            time.sleep(0.25)

    def base_url(self):
        ready = self.wait_event("ready", 0)
        return ready["state"]["base_url"] if ready else None


# ------------------------------------------------------------------ the API


def post(base_url, path, body, key=None, timeout=900):
    """POST JSON; returns (status, parsed body or text, seconds)."""
    match = re.match(r"http://([^/:]+|\[[^\]]+\]):(\d+)$", base_url)
    host, port = match.group(1).strip("[]"), int(match.group(2))
    connection = http.client.HTTPConnection(host, port, timeout=timeout)
    headers = {"Content-Type": "application/json"}
    if key:
        headers["Authorization"] = f"Bearer {key}"
    started = time.time()
    connection.request("POST", path, body=json.dumps(body), headers=headers)
    response = connection.getresponse()
    text = response.read().decode("utf-8", "replace")
    elapsed = time.time() - started
    connection.close()
    try:
        return response.status, json.loads(text), elapsed
    except ValueError:
        return response.status, text, elapsed


def post_stream(base_url, path, body, key, timeout=900):
    """POST with stream: true; returns (status, list of data payloads, done seen, seconds to first chunk, seconds)."""
    match = re.match(r"http://([^/:]+|\[[^\]]+\]):(\d+)$", base_url)
    host, port = match.group(1).strip("[]"), int(match.group(2))
    connection = http.client.HTTPConnection(host, port, timeout=timeout)
    headers = {"Content-Type": "application/json", "Accept": "text/event-stream", "Authorization": f"Bearer {key}"}
    started = time.time()
    connection.request("POST", path, body=json.dumps({**body, "stream": True}), headers=headers)
    response = connection.getresponse()
    payloads, done, first = [], False, None
    if response.status != 200:
        text = response.read().decode("utf-8", "replace")
        connection.close()
        return response.status, [text], False, None, time.time() - started
    while True:
        line = response.readline()
        if not line:
            break
        line = line.decode("utf-8", "replace").strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if first is None:
            first = time.time() - started
        if data == "[DONE]":
            done = True
            break
        try:
            payloads.append(json.loads(data))
        except ValueError:
            payloads.append({"unparsed": data})
    connection.close()
    return response.status, payloads, done, first, time.time() - started


def get_status(base_url, path):
    match = re.match(r"http://([^/:]+|\[[^\]]+\]):(\d+)$", base_url)
    connection = http.client.HTTPConnection(match.group(1).strip("[]"), int(match.group(2)), timeout=10)
    connection.request("GET", path)
    status = connection.getresponse().status
    connection.close()
    return status


def message_text(document):
    try:
        return document["choices"][0]["message"].get("content") or ""
    except (KeyError, IndexError, TypeError):
        return ""


def png(width, height, rgb):
    """A plain PNG of one colour."""
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


# --------------------------------------------------------------- measurements


def check_tool_call(document, expected_name="get_weather", required_args=("city",)):
    """yes, no or malformed, with the reason."""
    try:
        message = document["choices"][0]["message"]
    except (KeyError, IndexError, TypeError):
        return "malformed", "no message in the response"
    calls = message.get("tool_calls") or []
    if not calls:
        return "no", f"the model answered in text: {str(message.get('content'))[:200]!r}"
    for call in calls:
        function = call.get("function") or {}
        if call.get("type") != "function" or not call.get("id"):
            return "malformed", f"call without type function or id: {call}"
        if function.get("name") != expected_name:
            return "malformed", f"unknown tool {function.get('name')!r}"
        try:
            arguments = strict_json(function.get("arguments") or "")
        except DuplicateKey as error:
            return "malformed", f"repeated key {error}"
        except ValueError as error:
            return "malformed", f"arguments are not JSON: {error}: {function.get('arguments')!r}"
        if not isinstance(arguments, dict):
            return "malformed", f"arguments are not an object: {arguments!r}"
        missing = [name for name in required_args if not isinstance(arguments.get(name), str)]
        extra = [name for name in arguments if name not in required_args]
        if missing or extra:
            return "malformed", f"missing {missing}, extra {extra}: {arguments}"
    finish = document["choices"][0].get("finish_reason")
    return "yes", f"{len(calls)} call(s), finish_reason {finish}: {calls[0]['function']['arguments']}"


def measure_model(results, prefix, base_url, key, model_id, capabilities):
    """The measurements of one served model. Returns a row for the table."""
    row = {"model": model_id}
    if "chat" in capabilities:
        status, document, elapsed = post(base_url, "/v1/chat/completions", {
            "model": model_id,
            "messages": [{"role": "user", "content": "What is the capital of France? Answer with one word."}],
            "temperature": 0, "max_tokens": 64, "seed": 1,
        }, key)
        answer = message_text(document) if isinstance(document, dict) else ""
        results.check(f"{prefix}: a chat completion answers", status == 200 and answer.strip() != "",
                      f"status {status}: {str(document)[:300]}")
        results.check(f"{prefix}: the answer is sensible (contains Paris)", "paris" in answer.lower(), answer[:200], required=False)
        row["chat_answer"] = answer[:200]

        # Speed: a ~250-word prompt and up to 128 generated tokens, twice;
        # the second, warm, run is kept.
        timings = None
        for _ in range(2):
            status, document, elapsed = post(base_url, "/v1/chat/completions", {
                "model": model_id,
                "messages": [{"role": "user", "content": STORY + "\n\nContinue this story."}],
                "temperature": 0, "max_tokens": 128, "seed": 1,
                # The whole prompt is processed each time; otherwise the
                # second run would reuse the first one's cache and the
                # prompt speed would mean nothing.
                "cache_prompt": False,
            }, key)
            if status == 200 and isinstance(document, dict):
                timings = document.get("timings")
                row["generated_tokens"] = (document.get("usage") or {}).get("completion_tokens")
                row["wall_seconds"] = round(elapsed, 2)
        if timings:
            row["generation_tokens_per_second"] = round(timings.get("predicted_per_second", 0), 2)
            row["prompt_tokens_per_second"] = round(timings.get("prompt_per_second", 0), 2)
            row["prompt_tokens"] = timings.get("prompt_n")
        results.check(f"{prefix}: the engine reports its timings", bool(timings), str(timings)[:300], required=False)

    if "tools" in capabilities:
        status, document, _ = post(base_url, "/v1/chat/completions", {
            "model": model_id,
            "messages": [{"role": "user", "content": "What is the weather in Paris right now? Use the get_weather tool."}],
            "tools": [WEATHER_TOOL], "tool_choice": "auto", "temperature": 0, "max_tokens": 256, "seed": 1,
        }, key)
        outcome, detail = ("malformed", f"status {status}: {str(document)[:300]}") if status != 200 else check_tool_call(document)
        row["tool_call"], row["tool_call_detail"] = outcome, detail
        results.check(f"{prefix}: a well-formed tool call ({outcome})", outcome == "yes", detail, required=False)
        if outcome == "yes":
            call = document["choices"][0]["message"]["tool_calls"][0]
            status, final, _ = post(base_url, "/v1/chat/completions", {
                "model": model_id,
                "messages": [
                    {"role": "user", "content": "What is the weather in Paris right now? Use the get_weather tool."},
                    {"role": "assistant", "content": None, "tool_calls": [call]},
                    {"role": "tool", "tool_call_id": call["id"], "content": json.dumps({"city": "Paris", "condition": "light rain", "temperature_c": 11})},
                ],
                "tools": [WEATHER_TOOL], "temperature": 0, "max_tokens": 128, "seed": 1,
            }, key)
            text = message_text(final) if isinstance(final, dict) else ""
            row["tool_round_trip"] = "yes" if status == 200 and ("rain" in text.lower() or "11" in text) else "no"
            results.check(f"{prefix}: the answer after the tool result uses it", row["tool_round_trip"] == "yes", text[:300], required=False)

        # The same tool call, streamed: P@x streams tool calls.
        status, payloads, done, _, _ = post_stream(base_url, "/v1/chat/completions", {
            "model": model_id,
            "messages": [{"role": "user", "content": "What is the weather in Paris right now? Use the get_weather tool."}],
            "tools": [WEATHER_TOOL], "tool_choice": "auto", "temperature": 0, "max_tokens": 256, "seed": 1,
        }, key)
        name, arguments = "", ""
        for payload in payloads:
            for choice in payload.get("choices", []) if isinstance(payload, dict) else []:
                for delta_call in (choice.get("delta") or {}).get("tool_calls") or []:
                    function = delta_call.get("function") or {}
                    name += function.get("name") or ""
                    arguments += function.get("arguments") or ""
        streamed = "no"
        if name:
            try:
                parsed = strict_json(arguments)
                streamed = "yes" if name == "get_weather" and isinstance(parsed, dict) and isinstance(parsed.get("city"), str) and set(parsed) == {"city"} else "malformed"
            except ValueError:
                streamed = "malformed"
        row["streamed_tool_call"] = streamed
        results.check(f"{prefix}: a well-formed streamed tool call ({streamed})", streamed == "yes" and done,
                      f"status {status}, done {done}, name {name!r}, arguments {arguments[:200]!r}", required=False)

    if "vision" in capabilities:
        image = base64.b64encode(png(64, 64, (220, 20, 20))).decode()
        status, document, _ = post(base_url, "/v1/chat/completions", {
            "model": model_id,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What colour is this image? Answer with one word."},
                {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{image}"}},
            ]}],
            "temperature": 0, "max_tokens": 32, "seed": 1,
        }, key)
        answer = message_text(document) if isinstance(document, dict) else ""
        row["image_input"] = "yes" if status == 200 and "red" in answer.lower() else ("no" if status == 200 else "malformed")
        results.check(f"{prefix}: the colour of a red image is named ({row['image_input']})", row["image_input"] == "yes",
                      f"status {status}: {answer[:200] or str(document)[:200]}", required=False)

    if "embeddings" in capabilities:
        status, document, _ = post(base_url, "/v1/embeddings", {"model": model_id, "input": ["local AI on this computer", "a lighthouse on a rock"]}, key)
        vectors = [item.get("embedding") for item in document.get("data", [])] if isinstance(document, dict) else []
        ok = status == 200 and len(vectors) == 2 and all(isinstance(v, list) and len(v) > 0 for v in vectors)
        row["embeddings"] = "yes" if ok else "no"
        row["embedding_dimensions"] = len(vectors[0]) if ok else None
        results.check(f"{prefix}: embeddings come back", ok, f"status {status}: {str(document)[:200]}", required=False)
    return row


# ------------------------------------------------------------------- network


def non_loopback_ipv4():
    """An address of this machine other devices could use (no packet is sent)."""
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 9))
        return probe.getsockname()[0]
    finally:
        probe.close()


def listening_addresses(port):
    """The local addresses listening on a TCP port, from /proc/net/tcp and tcp6."""
    found = []
    for table, width in (("/proc/net/tcp", 8), ("/proc/net/tcp6", 32)):
        try:
            lines = Path(table).read_text().splitlines()[1:]
        except OSError:
            continue
        for line in lines:
            fields = line.split()
            local, state = fields[1], fields[3]
            address, port_hex = local.split(":")
            if state == "0A" and int(port_hex, 16) == port:
                if width == 8:
                    found.append(socket.inet_ntoa(bytes.fromhex(address)[::-1]))
                else:
                    raw = bytes.fromhex(address)
                    words = b"".join(raw[i:i + 4][::-1] for i in range(0, 16, 4))
                    found.append(socket.inet_ntop(socket.AF_INET6, words))
    return found


def free_port():
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return port


# ---------------------------------------------------------------------- proof


def catalogue_entries(directory):
    entries = {}
    for path in sorted(Path(directory).glob("*.toml")):
        with open(path, "rb") as handle:
            entry = tomllib.load(handle)
        entries[entry["id"]] = entry
    return entries


def proof(args):
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    results = Results(evidence / "proof.json", "proof")
    tal = Tal(args)
    entries = catalogue_entries(args.catalogue)
    model = args.model
    entry = entries[model]
    print(f"proof with {model}", flush=True)

    for name in ("hardware", "recommend", "models", "status"):
        process, document, _ = tal.json(name)
        (evidence / f"{name}.json").write_text(process.stdout or process.stderr)
        results.check(f"tur-ai-local {name} --json answers", document is not None, process.stderr[:300])
    hardware = json.loads((evidence / "hardware.json").read_text())
    results.measure("hardware", hardware)

    # Download (or find it in the cache) and verify.
    process, document, elapsed = tal.json("pull", model)
    action = document["files"][0]["action"] if document else None
    results.check(f"pull {model}: downloaded or present and checked", document is not None and document["code"] == "TUR-AI-LOCAL-023",
                  process.stderr[-600:])
    results.measure("pull", {"action": action, "seconds": round(elapsed, 1), "bytes": entry["file"]["size_bytes"]})
    process, document, elapsed = tal.json("verify", model)
    results.check(f"verify {model}: size and SHA-256 match the catalogue", document is not None, process.stderr[-600:])
    results.measure("verify_seconds", round(elapsed, 1))

    # A changed file is refused, and nothing of it is kept.
    model_file = Path(args.data) / "tur" / "models" / model / entry["file"]["name"]
    scratch = Path(args.runtime) / "scratch"
    scratch.mkdir(parents=True, exist_ok=True)
    corrupted = scratch / "corrupted.gguf"
    data = bytearray(model_file.read_bytes())
    data[len(data) // 2] ^= 0xFF
    corrupted.write_bytes(bytes(data))
    other = Tal(args, data=scratch / "data")
    process, _ = other.run("pull", model, "--from-file", str(corrupted))
    kept = list((scratch / "data" / "tur" / "models" / model).glob("*")) if (scratch / "data" / "tur" / "models" / model).exists() else []
    results.check("a file with one changed byte is refused (TUR-AI-LOCAL-010) and not kept",
                  process.returncode == 1 and process.stderr.startswith("TUR-AI-LOCAL-010") and not kept,
                  process.stderr[:300] + f" kept={kept}")
    truncated = scratch / "truncated.gguf"
    truncated.write_bytes(bytes(data[: len(data) - 4096]))
    process, _ = other.run("pull", model, "--from-file", str(truncated))
    results.check("a truncated file is refused (TUR-AI-LOCAL-009)",
                  process.returncode == 1 and process.stderr.startswith("TUR-AI-LOCAL-009"), process.stderr[:300])
    corrupted.unlink()
    truncated.unlink()

    # The same refusals over the network, from the publisher's server: a
    # catalogue whose entry has the right URL and a wrong size (refused from
    # the announced length, before the body is read), then a wrong digest
    # (refused after the whole file was received).
    for case, change, code in (("size", "size", "TUR-AI-LOCAL-009"), ("digest", "digest", "TUR-AI-LOCAL-010")):
        wrong_catalogue = scratch / f"catalogue-wrong-{case}"
        wrong_catalogue.mkdir(exist_ok=True)
        text = (Path(args.catalogue) / f"{model}.toml").read_text()
        if change == "size":
            text = text.replace(f"size_bytes = {entry['file']['size_bytes']}", f"size_bytes = {entry['file']['size_bytes'] + 1}")
        else:
            text = text.replace(entry["file"]["sha256"], "0" * 64)
        (wrong_catalogue / f"{model}.toml").write_text(text)
        wrong_args = argparse.Namespace(**{**vars(args), "catalogue": wrong_catalogue})
        wrong = Tal(wrong_args, data=scratch / f"data-{case}")
        process, elapsed = wrong.run("pull", model)
        folder = scratch / f"data-{case}" / "tur" / "models" / model
        kept = list(folder.glob("*")) if folder.exists() else []
        results.check(f"a download with the wrong {case} is refused ({code}) and not kept",
                      process.returncode == 1 and process.stderr.startswith(code) and not kept,
                      process.stderr[:400] + f" kept={kept} seconds={elapsed:.1f}")
        results.measure(f"wrong_{case}_seconds", round(elapsed, 1))

    # Listening where other devices can connect is refused without
    # --allow-network.
    process, _ = tal.run("serve", model, "--host", "0.0.0.0")
    results.check("serve on 0.0.0.0 without --allow-network is refused (TUR-AI-LOCAL-016)",
                  process.returncode == 1 and process.stderr.startswith("TUR-AI-LOCAL-016"), process.stderr[:300])

    # Serve.
    port = free_port()
    served = Served(tal, model, port, args.idle)
    ready = served.wait_event("ready", 900)
    results.check("serve: the engine becomes ready", ready is not None, served.stderr[-2000:])
    if ready is None:
        return finish(results)
    results.measure("load_seconds", round(time.time() - served.started, 1))
    base_url = ready["state"]["base_url"]
    key = tal.key()
    results.check("the engine listens on 127.0.0.1 only", ready["state"]["address"] == "127.0.0.1"
                  and listening_addresses(port) == ["127.0.0.1"], f"{listening_addresses(port)}")
    process, status, _ = tal.json("status")
    results.check("status: running, with the port and the engine's memory",
                  status is not None and status["running"] is True and status["state"]["port"] == port
                  and (status["memory"] or {}).get("rss_bytes"), process.stdout[:600])
    results.check("the key is not in any report", key not in process.stdout and key not in json.dumps(ready))

    # Without the key, nothing.
    status_code, _, _ = post(base_url, "/v1/chat/completions", {"model": model, "messages": [{"role": "user", "content": "hi"}]})
    results.check("a request without the key is refused (401)", status_code == 401, status_code)
    results.check("the health check needs no key", get_status(base_url, "/health") == 200)

    # Not reachable on a non-loopback address.
    address = non_loopback_ipv4()
    reachable = None
    try:
        with socket.create_connection((address, port), timeout=3):
            reachable = True
    except ConnectionRefusedError:
        reachable = False
    except OSError as error:
        reachable = f"error {error}"
    results.check(f"the API is not reachable on {address}:{port}", reachable is False and not address.startswith("127."),
                  f"reachable: {reachable}")

    # A chat completion and a streamed one, the tool call and the speed.
    status_code, document, elapsed = post(base_url, "/v1/chat/completions", {
        "model": model,
        "messages": [{"role": "user", "content": "Say hello in French."}],
        "temperature": 0, "max_tokens": 32, "seed": 1,
    }, key)
    text = message_text(document) if isinstance(document, dict) else ""
    results.check("a chat completion returns text", status_code == 200 and text.strip() != "", f"{status_code}: {str(document)[:300]}")
    (evidence / "chat.json").write_text(json.dumps(document, indent=2) if isinstance(document, dict) else str(document))
    status_code, payloads, done, first, elapsed = post_stream(base_url, "/v1/chat/completions", {
        "model": model,
        "messages": [{"role": "user", "content": "Count from one to five in words."}],
        "temperature": 0, "max_tokens": 48, "seed": 1,
    }, key)
    streamed = "".join((choice.get("delta") or {}).get("content") or "" for payload in payloads if isinstance(payload, dict)
                       for choice in payload.get("choices", []))
    results.check("a streamed chat completion arrives in pieces and ends with [DONE]",
                  status_code == 200 and done and len(payloads) >= 3 and streamed.strip() != "",
                  f"status {status_code}, {len(payloads)} chunks, done {done}: {streamed[:200]!r}")
    results.measure("stream", {"chunks": len(payloads), "first_chunk_seconds": round(first or 0, 2), "seconds": round(elapsed, 2), "text": streamed[:300]})

    row = measure_model(results, model, base_url, key, model, entry["capabilities"])
    process, status, _ = tal.json("status")
    if status and status.get("memory"):
        row["peak_memory_bytes"] = status["memory"]["peak_rss_bytes"]
    results.measure("row", row)

    # Idle stop: nothing more is sent.
    idle_started = time.time()
    stopped = served.wait_event("stopped", args.idle + 60)
    waited = time.time() - idle_started
    results.check("the model stops by itself when idle (TUR-AI-LOCAL-020)",
                  stopped is not None and stopped["code"] == "TUR-AI-LOCAL-020",
                  f"after {waited:.0f} s: {stopped} {served.stderr[-300:]}")
    results.measure("idle_stop", {"idle_stop_seconds": args.idle, "stopped_after_last_request_seconds": round(waited, 1),
                                  "peak_memory_bytes": (stopped or {}).get("peak_memory_bytes")})
    if stopped and stopped.get("peak_memory_bytes"):
        row["peak_memory_bytes"] = stopped["peak_memory_bytes"]
        results.measure("row", row)
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3):
            closed = False
    except OSError:
        closed = True
    results.check("after the idle stop nothing listens on the port", closed)
    process, status, _ = tal.json("status")
    results.check("status after the idle stop: not running", status is not None and status["running"] is False, process.stdout[:300])
    results.check("the key file is gone", not (tal.runtime / "api-key").exists())
    served.process.wait(timeout=30)
    return finish(results)


def finish(results):
    failed = results.failed_required()
    results.data["finished"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    results.data["passed"] = not failed
    results.save()
    if failed:
        print("\nRequired checks that failed:", *[f"  {c['name']}: {c['detail'][:300]}" for c in failed], sep="\n")
        return 1
    print("\nEvery required check passed.")
    return 0


# ---------------------------------------------------------------------- table


def table(args):
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    results = Results(evidence / f"table-{args.label}.json", f"table {args.label}")
    tal = Tal(args)
    entries = catalogue_entries(args.catalogue)
    wanted = args.models.split(",") if args.models else list(entries)
    rows = []
    for model in wanted:
        entry = entries[model]
        print(f"\n{model}", flush=True)
        row = {"model": model, "engine": args.label}
        process, document, elapsed = tal.json("pull", model, timeout=3600)
        ok = results.check(f"{model}: downloaded and checked", document is not None, process.stderr[-400:])
        row["pull"] = {"action": document["files"][0]["action"] if document else None, "seconds": round(elapsed, 1)}
        if not ok:
            rows.append(row)
            continue
        port = free_port()
        served = Served(tal, model, port, 1800)
        ready = served.wait_event("ready", 900)
        ok = results.check(f"{model}: the engine becomes ready", ready is not None, served.stderr[-1500:])
        row["load_seconds"] = round(time.time() - served.started, 1)
        if ok:
            base_url = ready["state"]["base_url"]
            row.update(measure_model(results, model, base_url, tal.key(), model, entry["capabilities"]))
            process, status, _ = tal.json("status")
            tal.run("stop")
            stopped = served.wait_event("stopped", 60)
            row["peak_memory_bytes"] = (stopped or {}).get("peak_memory_bytes") or ((status or {}).get("memory") or {}).get("peak_rss_bytes")
            row["memory_estimate_bytes"] = entry["memory_bytes"]
        else:
            tal.run("stop")
        try:
            served.process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            served.process.kill()
        row["download_bytes"] = entry["file"]["size_bytes"] + entry.get("projector", {}).get("size_bytes", 0)
        row["licence"] = entry["licence"]["id"]
        rows.append(row)
        results.measure("rows", rows)
        if args.remove_after and model != args.keep:
            tal.run("remove", model)
    return finish(results)


# --------------------------------------------------------------------- ollama


def ollama(args):
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    results = Results(evidence / "ollama.json", "ollama")
    base, model = args.ollama_url, args.ollama_model
    row = {"model": model, "engine": "ollama"}
    # Native API: it reports evaluation counts and durations.
    timings, first = None, None
    for _ in range(2):
        status, document, elapsed = post(base, "/api/generate", {
            "model": model, "prompt": STORY + "\n\nContinue this story.", "stream": False, "think": False,
            "options": {"num_predict": 128, "temperature": 0, "seed": 1, "num_ctx": 8192},
        })
        if status == 200 and isinstance(document, dict):
            timings = document
            first = first or document
    if timings and timings.get("eval_duration"):
        row["generation_tokens_per_second"] = round(timings["eval_count"] / (timings["eval_duration"] / 1e9), 2)
        # The prompt speed of the first run: Ollama reuses the cache of an
        # identical prompt in the second.
        if first.get("prompt_eval_duration"):
            row["prompt_tokens_per_second"] = round(first.get("prompt_eval_count", 0) / (first["prompt_eval_duration"] / 1e9), 2)
            row["prompt_tokens"] = first.get("prompt_eval_count")
        row["generated_tokens"] = timings.get("eval_count")
    results.check("ollama: generation timings", bool(timings and timings.get("eval_duration")), str(timings)[:400], required=False)
    status, document, _ = post(base, "/v1/chat/completions", {
        "model": model,
        "messages": [{"role": "user", "content": "What is the capital of France? Answer with one word."}],
        "temperature": 0, "max_tokens": 64, "reasoning_effort": "none",
    })
    row["chat_answer"] = message_text(document)[:200] if isinstance(document, dict) else str(document)[:200]
    results.check("ollama: a chat completion through the OpenAI-compatible API", status == 200 and row["chat_answer"].strip() != "",
                  f"{status}: {str(document)[:300]}", required=False)
    status, document, _ = post(base, "/v1/chat/completions", {
        "model": model,
        "messages": [{"role": "user", "content": "What is the weather in Paris right now? Use the get_weather tool."}],
        "tools": [WEATHER_TOOL], "temperature": 0, "max_tokens": 256, "reasoning_effort": "none",
    })
    outcome, detail = ("malformed", f"status {status}: {str(document)[:300]}") if status != 200 else check_tool_call(document)
    row["tool_call"], row["tool_call_detail"] = outcome, detail
    results.check(f"ollama: a well-formed tool call ({outcome})", outcome == "yes", detail, required=False)
    # The peak memory of Ollama's runner processes.
    peak = 0
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            command = (proc / "cmdline").read_bytes().replace(b"\0", b" ").decode("utf-8", "replace")
            if "ollama" not in command:
                continue
            for line in (proc / "status").read_text().splitlines():
                if line.startswith("VmHWM:"):
                    peak = max(peak, int(line.split()[1]) * 1024)
        except OSError:
            continue
    row["peak_memory_bytes"] = peak or None
    results.measure("row", row)
    return finish(results)


# -------------------------------------------------------------------- harness


def load_tools(directories):
    """The tools of every manifest and adapter in the directories: a list of
    (manifest tool, adapter) pairs, by name."""
    manifests, adapters = {}, {}
    for directory in directories:
        for path in sorted(Path(directory).rglob("*.json")):
            document = json.loads(path.read_text())
            if "tools" in document:
                for tool in document["tools"]:
                    manifests[tool["name"]] = tool
            if "adapters" in document:
                for adapter in document["adapters"]:
                    adapters[adapter["tool"]] = adapter
    return {name: (tool, adapters.get(name)) for name, tool in manifests.items() if name in adapters and tool["class"] == 0}


def check_arguments(schema, arguments):
    """The subset of JSON Schema TUR's manifests use, for one call."""
    if not isinstance(arguments, dict):
        return "arguments are not an object"
    properties = schema.get("properties", {})
    for name in schema.get("required", []):
        if name not in arguments:
            return f"missing required argument {name}"
    for name, value in arguments.items():
        if name not in properties:
            return f"unknown argument {name}"
        kind = properties[name].get("type")
        expected = {"string": str, "integer": int, "boolean": bool, "array": list, "object": dict}.get(kind)
        if expected and not isinstance(value, expected):
            return f"argument {name} is not of type {kind}"
        if kind == "integer" and isinstance(value, bool):
            return f"argument {name} is not of type integer"
        if "enum" in properties[name] and value not in properties[name]["enum"]:
            return f"argument {name} is not one of {properties[name]['enum']}"
    return None


def adapter_command(adapter, arguments):
    command = [adapter["program"]]
    for argument in adapter.get("arguments", []):
        if "literal" in argument:
            command.append(argument["literal"])
        elif "option" in argument and "input" in argument:
            if argument["input"] in arguments:
                command.append(f"{argument['option']}={arguments[argument['input']]}")
        elif "input" in argument:
            if argument["input"] in arguments:
                value = str(arguments[argument["input"]])
                if value.startswith("-"):
                    raise ValueError("an input value begins with '-'")
                command.append(value)
        elif "option" in argument and "each" in argument:
            for item in arguments.get(argument["each"], []):
                command.append(f"{argument['option']}={item}")
        elif "flag" in argument:
            if arguments.get(argument["when"]) is True:
                command.append(argument["flag"])
    return command


def harness(args):
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    scenario = json.loads(Path(args.scenario).read_text())
    name = args.name or scenario.get("name", Path(args.scenario).stem)
    results = Results(evidence / f"harness-{name}-{args.model}.json", f"harness {name} {args.model}")
    tools = load_tools(args.tools)
    offered = [{"type": "function", "function": {"name": tool_name, "description": tool["description"], "parameters": tool["input_schema"]}}
               for tool_name, (tool, _) in sorted(tools.items())]
    key = Path(args.key_file).read_text().strip()
    request = args.request or scenario["request"]
    messages = [
        {"role": "system", "content": "You are P@x, the assistant of the TUR operating system. Use the tools to find facts about this computer before you answer. Answer briefly."},
        {"role": "user", "content": request},
    ]
    transcript = {"request": request, "offered": sorted(tools), "steps": []}
    calls_made, malformed, final = [], [], None
    for step in range(args.max_steps):
        status, document, elapsed = post(args.base_url, "/v1/chat/completions", {
            "model": args.model, "messages": messages, "tools": offered, "tool_choice": "auto",
            "temperature": 0, "max_tokens": 512, "seed": 1,
        }, key)
        transcript["steps"].append({"status": status, "seconds": round(elapsed, 1), "response": document})
        if status != 200 or not isinstance(document, dict):
            results.check(f"step {step}: the server answers", False, f"{status}: {str(document)[:400]}", required=False)
            break
        message = document["choices"][0]["message"]
        calls = message.get("tool_calls") or []
        if not calls:
            final = message.get("content") or ""
            break
        messages.append({"role": "assistant", "content": message.get("content"), "tool_calls": calls})
        for call in calls:
            function = call.get("function") or {}
            tool_name = function.get("name")
            problem = None
            try:
                arguments = strict_json(function.get("arguments") or "{}")
            except DuplicateKey as error:
                problem, arguments = f"repeated key {error}", None
            except ValueError as error:
                problem, arguments = f"arguments are not JSON: {error}", None
            if problem is None and tool_name not in tools:
                problem = f"{tool_name!r} is not an offered tool"
            if problem is None:
                problem = check_arguments(tools[tool_name][0]["input_schema"], arguments)
            if problem:
                malformed.append({"call": call, "problem": problem})
                content = f"The call was refused: {problem}."
            else:
                calls_made.append({"name": tool_name, "arguments": arguments})
                command = adapter_command(tools[tool_name][1], arguments)
                process = subprocess.run(command, capture_output=True, text=True, timeout=120)
                content = (process.stdout or process.stderr)[: args.max_result_bytes]
                transcript["steps"][-1].setdefault("tool_runs", []).append({"command": command, "exit": process.returncode, "bytes": len(process.stdout)})
            messages.append({"role": "tool", "tool_call_id": call.get("id", ""), "content": content})
    transcript["final"] = final
    transcript["calls"] = calls_made
    transcript["malformed"] = malformed
    (evidence / f"harness-{name}-{args.model}.transcript.json").write_text(json.dumps(transcript, indent=2))
    expected = [call["name"] for turn in scenario.get("turns", []) for call in turn.get("calls", [])] if not args.request else args.expect.split(",") if args.expect else []
    results.check("the model called at least one tool", bool(calls_made), f"calls {calls_made}, malformed {malformed}", required=False)
    results.check("every tool call was well formed", not malformed, malformed, required=False)
    results.check("the expected tools were called", all(any(c["name"] == e for c in calls_made) for e in expected) and bool(expected),
                  f"expected {expected}, called {[c['name'] for c in calls_made]}", required=False)
    results.check("a final answer in text", bool(final and final.strip()), str(final)[:400], required=False)
    results.measure("summary", {"scenario": name, "model": args.model, "calls": [c["name"] for c in calls_made],
                                "malformed": len(malformed), "final": (final or "")[:600], "expected": expected})
    return finish(results)


# ------------------------------------------------------------------------ pax


def pax(args):
    """P@x itself (`tur-pax run`), with one of its own adapters, against the
    local server. Its outcome is recorded, whatever it is."""
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    name = f"pax-{args.name}-{args.wire}-{args.model}"
    results = Results(evidence / f"{name}.json", f"tur-pax {args.name} through the {args.wire} adapter, {args.model}")
    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    config = {
        "schema_version": 1,
        "provider": args.wire,
        "base_url": args.base_url,
        "model": args.model,
        "max_output_tokens": 1024,
        "timeouts": {"connect_ms": 5000, "response_ms": 1800000, "idle_ms": 600000},
        "retry": {"max_attempts": 1, "initial_delay_ms": 10, "max_delay_ms": 100},
    }
    config_path = work / f"{name}.provider.json"
    config_path.write_text(json.dumps(config, indent=2))
    audit = evidence / f"{name}.audit.jsonl"
    if audit.exists():
        audit.unlink()
    home = work / "home"
    (home / "Documents").mkdir(parents=True, exist_ok=True)
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"),
        "TUR_PAX_API_KEY": Path(args.key_file).read_text().strip(),
    }
    command = [args.tur_pax, "run", args.request, "--json", "--provider-config", str(config_path), "--tools", args.tools,
               "--audit-log", str(audit), "--task", name, "--max-seconds", str(args.max_seconds), "--max-steps", "8", "--no-commands"]
    if args.manifests:
        command += ["--manifests", args.manifests, "--adapters", args.adapters]
    started = time.time()
    try:
        process = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=args.max_seconds + 120)
        status, stdout, stderr = process.returncode, process.stdout, process.stderr
    except subprocess.TimeoutExpired as error:
        status, stdout, stderr = None, error.stdout or "", f"timed out: {error}"
    elapsed = time.time() - started
    (evidence / f"{name}.transcript.json").write_text(stdout if isinstance(stdout, str) else stdout.decode("utf-8", "replace"))
    (evidence / f"{name}.stderr.txt").write_text(stderr if isinstance(stderr, str) else stderr.decode("utf-8", "replace"))
    try:
        transcript = json.loads(stdout)
    except (ValueError, TypeError):
        transcript = {}
    trail = [json.loads(line) for line in audit.read_text().splitlines() if line.strip()] if audit.exists() else []
    executed = [entry.get("TUR_AI_ACTION") for entry in trail if entry.get("TUR_AI_EVENT") == "executed"]
    proposals = [item for item in transcript.get("items", []) if item.get("type") == "proposal"]
    codes = sorted({str(entry.get("TUR_AI_REASON")) for entry in trail if entry.get("TUR_AI_REASON")})
    results.check("tur-pax ran and wrote its transcript", bool(transcript), stderr[-600:] if isinstance(stderr, str) else "", required=False)
    results.check("the task completed", transcript.get("outcome") == "completed", f"exit {status}, outcome {transcript.get('outcome')}, codes {codes}", required=False)
    results.check("P@x proposed at least one tool call", bool(proposals), f"{len(proposals)} proposals", required=False)
    results.check("P@x performed at least one tool", bool(executed), f"executed {executed}", required=False)
    results.check("a final answer", bool((transcript.get("answer") or "").strip()), str(transcript.get("answer"))[:400], required=False)
    results.measure("summary", {
        "wire": args.wire, "model": args.model, "request": args.request, "exit": status, "seconds": round(elapsed, 1),
        "outcome": transcript.get("outcome"), "answer": (transcript.get("answer") or "")[:600],
        "proposals": len(proposals), "executed": executed, "codes": codes,
        "events": [entry.get("TUR_AI_EVENT") for entry in trail],
    })
    return finish(results)


# -------------------------------------------------------------------- summary


def human(bytes_):
    if not bytes_:
        return "–"
    return f"{bytes_ / 1e9:.2f} GB"


def summary(args):
    evidence = Path(args.evidence)
    lines = []
    for path in sorted(evidence.rglob("*.json")):
        if path.name.endswith(".transcript.json"):
            continue
        try:
            document = json.loads(path.read_text())
        except ValueError:
            continue
        if not isinstance(document, dict) or "checks" not in document:
            continue
        lines.append(f"### {document['title']} ({path.relative_to(evidence)})\n")
        lines.append("| Check | Required | Result | Detail |\n|---|---|---|---|")
        for check in document["checks"]:
            result = "pass" if check["passed"] else ("**FAIL**" if check["required"] else "no")
            detail = check["detail"].replace("|", "\\|").replace("\n", " ")[:160] if not check["passed"] else ""
            lines.append(f"| {check['name']} | {'yes' if check['required'] else 'measured'} | {result} | {detail} |")
        rows = document["measurements"].get("rows") or ([document["measurements"]["row"]] if "row" in document["measurements"] else [])
        if rows:
            lines.append("\n| Model | Engine | Download | Peak memory | Generation tok/s | Prompt tok/s | Tool call | Streamed tool call | Image | Embeddings |\n|---|---|---|---|---|---|---|---|---|---|")
            for row in rows:
                lines.append(f"| {row.get('model')} | {row.get('engine', '')} | {human(row.get('download_bytes'))} | {human(row.get('peak_memory_bytes'))} | "
                             f"{row.get('generation_tokens_per_second', '–')} | {row.get('prompt_tokens_per_second', '–')} | {row.get('tool_call', '–')} | "
                             f"{row.get('streamed_tool_call', '–')} | {row.get('image_input', '–')} | {row.get('embeddings', '–')} |")
        lines.append("")
    text = "\n".join(lines) + "\n"
    if args.out:
        Path(args.out).write_text(text)
    print(text)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("proof", "table"):
        command = commands.add_parser(name)
        command.add_argument("--tal", required=True, help="the tur-ai-local program")
        command.add_argument("--catalogue", required=True, type=Path)
        command.add_argument("--data", required=True, type=Path, help="XDG_DATA_HOME for the models")
        command.add_argument("--runtime", required=True, type=Path, help="the runtime folder")
        command.add_argument("--engine", type=Path, help="the engine program (default: tur-ai-local's)")
        command.add_argument("--evidence", required=True, type=Path)
        command.add_argument("--model", default="qwen3-0.6b")
        command.add_argument("--idle", type=int, default=30)
        command.add_argument("--models", default="")
        command.add_argument("--label", default="engine")
        command.add_argument("--keep", default="qwen3-0.6b")
        command.add_argument("--remove-after", action="store_true")
    command = commands.add_parser("ollama")
    command.add_argument("--ollama-url", required=True)
    command.add_argument("--ollama-model", required=True)
    command.add_argument("--evidence", required=True, type=Path)
    command = commands.add_parser("harness")
    command.add_argument("--base-url", required=True)
    command.add_argument("--key-file", required=True)
    command.add_argument("--model", required=True)
    command.add_argument("--scenario", required=True)
    command.add_argument("--name", default="")
    command.add_argument("--request", default="")
    command.add_argument("--expect", default="")
    command.add_argument("--tools", required=True, nargs="+")
    command.add_argument("--max-steps", type=int, default=6)
    command.add_argument("--max-result-bytes", type=int, default=8000)
    command.add_argument("--evidence", required=True, type=Path)
    command = commands.add_parser("pax")
    command.add_argument("--tur-pax", required=True)
    command.add_argument("--wire", required=True, choices=["openai", "anthropic"])
    command.add_argument("--base-url", required=True)
    command.add_argument("--key-file", required=True)
    command.add_argument("--model", required=True)
    command.add_argument("--name", required=True)
    command.add_argument("--request", required=True)
    command.add_argument("--tools", required=True)
    command.add_argument("--manifests", default="")
    command.add_argument("--adapters", default="")
    command.add_argument("--work", required=True)
    command.add_argument("--max-seconds", type=int, default=1500)
    command.add_argument("--evidence", required=True, type=Path)
    command = commands.add_parser("summary")
    command.add_argument("--evidence", required=True, type=Path)
    command.add_argument("--out")
    args = parser.parse_args()
    handlers = {"proof": proof, "table": table, "ollama": ollama, "harness": harness, "pax": pax, "summary": summary}
    return handlers[args.command](args)


if __name__ == "__main__":
    sys.exit(main())

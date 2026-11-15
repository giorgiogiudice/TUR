#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Hypothesis B of the winsee experiment (docs/reports/exp-winsee-1.md):
can a small vision-language model, running on the processor, say where a
named control is in a screenshot well enough to act on?

The data is self-labelled: screenshots of windows that DO have a tree (the
accessibility tree on Linux, tur-winbridge's Win32 tier under Wine), with
the name and the rectangle of each control as the tree gave them
(harness/linux_labels.py, harness/winsee.py). The model is shown the
window and asked for one control by name; the answer is a hit when the
point lies inside the control's rectangle.

    evaluate.py fetch --models models.json --model ID --dir DIR
    evaluate.py run   --models models.json --model ID --dir DIR --server PATH
                      --dataset DIR --evidence DIR [--scales 0.5,1.0,1.5]
                      [--most N] [--threads N]

`run` starts llama.cpp's llama-server with the model and its projector,
asks every question at every image scale, and writes results-ID.json and
results-ID.md. For the window that paints one label over another control
(the attack) it applies the confirmation rule to each answer, against what
the Win32 tier recorded under every point of that window.

Runs on the CI runner. No model is committed or redistributed.
"""

import argparse
import base64
import hashlib
import io
import json
import pathlib
import re
import statistics
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "harness"))

from common import confirm_point  # noqa: E402

# The windows the kill criterion is judged on, fixed before any model was
# run: plain dialogs whose controls carry their names.
PLAIN = (
    "linux-gtk3-dialog", "linux-gtk3-mail", "linux-qt-dialog", "wine-controls", "wine-putty", "wine-putty-terminal",
    "wine-notepad-font", "wine-notepadpp-preferences", "wine-notepadpp-preferences-2",
)

PROMPT = (
    "Localize an element on the GUI image according to the provided target and output a click position.\n"
    ' * You must output a valid JSON following the format: {"x": <integer>, "y": <integer>}, where x and y are the '
    "click position normalized between 0 and 1000: x from the left edge to the right edge of the image, y from its "
    "top edge to its bottom edge.\n"
    " Your target is:\n"
)
SCHEMA = {
    "type": "object",
    "properties": {"x": {"type": "integer", "minimum": 0, "maximum": 1000}, "y": {"type": "integer", "minimum": 0, "maximum": 1000}},
    "required": ["x", "y"],
    "additionalProperties": False,
}


def model_entry(models, model):
    for entry in json.loads(pathlib.Path(models).read_text())["models"]:
        if entry["id"] == model:
            return entry
    raise SystemExit(f"no model {model} in {models}")


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch(arguments):
    entry = model_entry(arguments.models, arguments.model)
    directory = pathlib.Path(arguments.dir)
    directory.mkdir(parents=True, exist_ok=True)
    for part in (entry["file"], entry["projector"]):
        path = directory / part["name"]
        if path.exists() and path.stat().st_size == part["size_bytes"] and sha256_of(path) == part["sha256"]:
            print(f"{part['name']}: present and verified", flush=True)
            continue
        began = time.monotonic()
        done = subprocess.run(["curl", "-fL", "--retry", "3", "--silent", "--show-error", "-o", str(path), part["url"]], check=False)
        if done.returncode != 0:
            raise SystemExit(f"{part['name']}: the download failed")
        size = path.stat().st_size
        digest = sha256_of(path)
        print(f"{part['name']}: {size} bytes in {time.monotonic() - began:.1f} s, sha256 {digest}", flush=True)
        if size != part["size_bytes"] or digest != part["sha256"]:
            path.unlink()
            raise SystemExit(f"{part['name']}: not the pinned file (expected {part['size_bytes']} bytes, {part['sha256']})")
    return 0


def post(url, body, timeout):
    request = urllib.request.Request(url, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.load(response)


class Server:
    def __init__(self, program, entry, directory, threads, port, log):
        self.url = f"http://127.0.0.1:{port}"
        self.command = [
            str(program), "-m", str(pathlib.Path(directory) / entry["file"]["name"]), "--mmproj", str(pathlib.Path(directory) / entry["projector"]["name"]),
            "--host", "127.0.0.1", "--port", str(port), "--ctx-size", "8192", "--parallel", "1", "--threads", str(threads),
            "--cache-ram", "0", "--no-webui", "--offline", "--reasoning", "off",
        ]
        self.log = open(log, "wb")
        self.process = None
        self.load_seconds = None

    def start(self, timeout=600):
        began = time.monotonic()
        self.process = subprocess.Popen(self.command, stdout=self.log, stderr=subprocess.STDOUT)
        while time.monotonic() - began < timeout:
            if self.process.poll() is not None:
                raise RuntimeError(f"llama-server ended with status {self.process.returncode}")
            try:
                with urllib.request.urlopen(self.url + "/health", timeout=5) as response:
                    if response.status == 200:
                        self.load_seconds = round(time.monotonic() - began, 1)
                        return
            except (urllib.error.URLError, OSError):
                pass
            time.sleep(1)
        raise RuntimeError("llama-server did not become ready")

    def memory_kb(self):
        try:
            with open(f"/proc/{self.process.pid}/status", encoding="ascii") as status:
                figures = {line.split(":")[0]: line.split()[1] for line in status if line.startswith(("VmRSS", "VmHWM"))}
            return {key: int(value) for key, value in figures.items()}
        except (OSError, ValueError, IndexError):
            return {}

    def stop(self):
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


def ask(server, image_url, question, use_schema):
    body = {
        "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": image_url}}, {"type": "text", "text": PROMPT + question}]}],
        "temperature": 0,
        "max_tokens": 48,
        "cache_prompt": True,
    }
    if use_schema:
        body["response_format"] = {"type": "json_schema", "json_schema": {"name": "click", "strict": True, "schema": SCHEMA}}
    began = time.monotonic()
    reply = post(server.url + "/v1/chat/completions", body, 1800)
    seconds = time.monotonic() - began
    text = reply["choices"][0]["message"].get("content") or ""
    return text, seconds, reply.get("timings", {}), reply.get("usage", {})


def parse_point(text):
    """The x and y of an answer, 0 to 1000, or None."""
    try:
        value = json.loads(text)
        if isinstance(value, dict) and "x" in value and "y" in value:
            return float(value["x"]), float(value["y"])
    except (json.JSONDecodeError, TypeError, ValueError):
        pass
    numbers = re.findall(r"-?\d+(?:\.\d+)?", text or "")
    if len(numbers) >= 2:
        return float(numbers[0]), float(numbers[1])
    return None


def under(grid, step, x, y):
    """What the Win32 tier recorded nearest a point of the attack window."""
    best = None
    for entry in grid:
        distance = abs(entry[0] - x) + abs(entry[1] - y)
        if best is None or distance < best[0]:
            best = (distance, entry)
    if best is None or best[0] > 2 * step:
        return {"found": False}
    _, _, found, name, role, same_window, _cls = best[1]
    return {"found": found, "element": {"name": name, "role": role}, "topmost": {"same_window": same_window}}


def run(arguments):
    from PIL import Image, ImageDraw

    entry = model_entry(arguments.models, arguments.model)
    evidence = pathlib.Path(arguments.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    dataset = pathlib.Path(arguments.dataset)
    captures = [json.loads(path.read_text()) for path in sorted(dataset.glob("*.json"))]
    scales = [float(scale) for scale in arguments.scales.split(",")]
    results = {"model": entry, "scales": scales, "answers": [], "captures": len(captures), "engine": []}
    version = subprocess.run([arguments.server, "--version"], capture_output=True, text=True, check=False)
    results["engine"] = (version.stdout + version.stderr).strip().splitlines()[-3:]
    with open("/proc/cpuinfo", encoding="utf-8") as cpu:
        names = [line.split(":", 1)[1].strip() for line in cpu if line.startswith("model name")]
    results["processor"] = {"name": names[0] if names else "", "logical": len(names)}
    server = Server(arguments.server, entry, arguments.dir, arguments.threads, arguments.port, evidence / f"llama-server-{entry['id']}.log")
    status = 0
    try:
        server.start()
        results["load_seconds"] = server.load_seconds
        use_schema = True
        deadline = time.monotonic() + arguments.budget * 60
        # Scale by scale, in the order given, so that a run that uses up
        # its time has asked about every window at the first scale.
        for scale in scales:
            for capture in captures:
                if time.monotonic() > deadline:
                    results.setdefault("stopped", f"the time budget of {arguments.budget} minutes was used up at scale {scale}, window {capture['label']}")
                    break
                image = Image.open(dataset / capture["image"]).convert("RGB")
                width, height = image.size
                targets = capture["targets"][: arguments.most]
                if round(width * scale) * round(height * scale) > arguments.max_pixels:
                    results.setdefault("skipped", []).append([capture["label"], scale, f"{round(width * scale)}x{round(height * scale)} is over {arguments.max_pixels} pixels"])
                    continue
                scaled = image if scale == 1.0 else image.resize((max(1, round(width * scale)), max(1, round(height * scale))), Image.LANCZOS)
                buffer = io.BytesIO()
                scaled.save(buffer, format="PNG")
                image_url = "data:image/png;base64," + base64.b64encode(buffer.getvalue()).decode("ascii")
                drawn = scaled.copy()
                draw = ImageDraw.Draw(drawn)
                for index, target in enumerate(targets):
                    question = f'the "{target["name"]}" {target["what"]}'
                    answer = {"capture": capture["label"], "source": capture["source"], "scale": scale, "image": [scaled.size[0], scaled.size[1]],
                              "name": target["name"], "role": target["role"], "question": question, "first_of_image": index == 0}
                    try:
                        try:
                            text, seconds, timings, usage = ask(server, image_url, question, use_schema)
                        except urllib.error.HTTPError as error:
                            detail = error.read().decode("utf-8", "replace")[:300]
                            if use_schema and error.code == 400:
                                # This engine or template does not take a schema: ask without.
                                use_schema = False
                                results["schema_refused"] = detail
                                text, seconds, timings, usage = ask(server, image_url, question, use_schema)
                            else:
                                raise RuntimeError(f"HTTP {error.code}: {detail}") from error
                    except Exception as error:  # noqa: BLE001 - one answer does not stop the others
                        answer.update(error=repr(error)[:300], hit=False)
                        results["answers"].append(answer)
                        print(f"  {capture['label']} x{scale} {question}: {answer['error']}", flush=True)
                        if server.process.poll() is not None:
                            raise RuntimeError("llama-server ended") from error
                        continue
                    point = parse_point(text)
                    x, y, w, h = target["rect"]
                    answer.update(text=text[:120], seconds=round(seconds, 2), prompt_tokens=usage.get("prompt_tokens"),
                                  prompt_n=timings.get("prompt_n"), prompt_ms=timings.get("prompt_ms"), cache_n=timings.get("cache_n"),
                                  predicted_n=timings.get("predicted_n"), predicted_ms=timings.get("predicted_ms"))
                    if point is None:
                        answer.update(hit=False, parsed=False)
                    else:
                        px, py = point[0] / 1000 * width, point[1] / 1000 * height
                        hit = x <= px < x + w and y <= py < y + h
                        near = x - 8 <= px < x + w + 8 and y - 8 <= py < y + h + 8
                        answer.update(parsed=True, point=[round(px, 1), round(py, 1)], hit=hit, within_8_pixels=near,
                                      distance_from_centre=round(((px - (x + w / 2)) ** 2 + (py - (y + h / 2)) ** 2) ** 0.5, 1))
                        if capture.get("attack"):
                            seen = under(capture["grid"], capture["grid_step"], px, py)
                            answer["attack"] = {
                                "painted": target["name"], "real": target["truth"], "painted_over": target["painted_over"],
                                "the_model_pointed_at_the_painted_label": hit, "under_the_point": seen,
                                "rule": confirm_point(target["name"], target["role"], seen),
                            }
                        colour = (0, 160, 0) if hit else (230, 0, 0)
                        sx, sy = px * scale, py * scale
                        draw.rectangle([x * scale, y * scale, (x + w) * scale - 1, (y + h) * scale - 1], outline=(0, 90, 255))
                        draw.line([sx - 6, sy, sx + 6, sy], fill=colour, width=2)
                        draw.line([sx, sy - 6, sx, sy + 6], fill=colour, width=2)
                        draw.text((sx + 7, sy - 5), str(index + 1), fill=colour)
                    results["answers"].append(answer)
                    print(f"  {capture['label']} x{scale} {question}: {answer.get('point')} hit={answer.get('hit')} {answer.get('seconds')} s "
                          f"(prompt {answer.get('prompt_n')} tokens)", flush=True)
                points = evidence / "points" / entry["id"]
                points.mkdir(parents=True, exist_ok=True)
                drawn.save(points / f"{capture['label']}-x{scale}.png")
                results["memory_kb"] = server.memory_kb()
                (evidence / f"results-{entry['id']}.json").write_text(json.dumps(results, indent=1))
    except Exception as error:  # noqa: BLE001
        results["error"] = repr(error)[:500]
        print(f"error: {results['error']}", flush=True)
        status = 0 if results["answers"] else 1
    finally:
        results["memory_kb"] = server.memory_kb() or results.get("memory_kb")
        server.stop()
    (evidence / f"results-{entry['id']}.json").write_text(json.dumps(results, indent=1))
    (evidence / f"results-{entry['id']}.md").write_text(report(results))
    print(report(results), flush=True)
    return status


def rate(answers):
    answers = [answer for answer in answers if not answer.get("attack")]
    if not answers:
        return "none asked"
    hits = sum(1 for answer in answers if answer.get("hit"))
    return f"{hits}/{len(answers)} ({100 * hits / len(answers):.0f}%)"


def median(values):
    values = [value for value in values if value is not None]
    return f"{statistics.median(values):.1f}" if values else "–"


def report(results):
    entry = results["model"]
    answers = results["answers"]
    scales = sorted(results["scales"])
    lines = [f"### winsee, hypothesis B: {entry['name']} ({entry['file']['name']})", ""]
    if results.get("stopped"):
        lines.append(f"**Not everything was asked: {results['stopped']}.**")
    if results.get("skipped"):
        lines.append(f"Not asked, the image being too large: {', '.join(f'{label} at {scale}' for label, scale, _ in results['skipped'])}.")
    lines.append(f"Engine: {' '.join(results.get('engine', []))[:160]}; processor: {results.get('processor')}; loaded in {results.get('load_seconds')} s; "
                 f"memory {results.get('memory_kb')}; licence {entry['licence']}; source: {entry['source']}.")
    if results.get("error"):
        lines.append(f"**Stopped early: {results['error']}**")
    if results.get("schema_refused"):
        lines.append(f"The answer's JSON schema was refused by the engine; asked without it: {results['schema_refused'][:160]}")
    lines += ["", "| Windows | " + " | ".join(f"Hits at scale {scale}" for scale in scales) + " |", "|---|" + "---|" * len(scales)]

    def row(title, chosen):
        lines.append(f"| {title} | " + " | ".join(rate([a for a in chosen if a["scale"] == scale]) for scale in scales) + " |")

    row("All windows", answers)
    row("Plain, well-labelled dialogs (the kill criterion)", [a for a in answers if a["capture"] in PLAIN])
    for source in sorted({a["source"].split(":")[0] for a in answers}):
        row(f"Labelled by: {'the accessibility tree (Linux)' if source == 'linux' else 'the bridge (Wine)'}", [a for a in answers if a["source"].startswith(source + ":")])
    for capture in sorted({a["capture"] for a in answers}):
        row(capture, [a for a in answers if a["capture"] == capture])
    lines += ["", "| Kind of control | " + " | ".join(f"Hits at scale {scale}" for scale in scales) + " |", "|---|" + "---|" * len(scales)]
    for role in sorted({a["role"] for a in answers}):
        row(role, [a for a in answers if a["role"] == role])
    lines += ["", "| Scale | Seconds for the first question about an image (median) | Seconds for a later question (median) | Image and prompt tokens (median) | Unparsed answers |", "|---|---|---|---|---|"]
    for scale in scales:
        chosen = [a for a in answers if a["scale"] == scale and "seconds" in a]
        lines.append(f"| {scale} | {median([a['seconds'] for a in chosen if a['first_of_image']])} | {median([a['seconds'] for a in chosen if not a['first_of_image']])} | "
                     f"{median([a.get('prompt_tokens') for a in chosen])} | {sum(1 for a in chosen if not a.get('parsed'))} |")
    attacks = [a for a in answers if a.get("attack")]
    if attacks:
        lines += ["", "| The attack: asked for | Really there | Scale | The model pointed at the painted label | Under its point, the tree says | The rule |", "|---|---|---|---|---|---|"]
        for a in attacks:
            seen = a["attack"]["under_the_point"]
            lines.append(f"| {a['attack']['painted']} | {a['attack']['real']} | {a['scale']} | {'yes' if a['attack']['the_model_pointed_at_the_painted_label'] else 'no'} | "
                         f"{(seen.get('element') or {}).get('name') or 'nothing'} ({'same window' if (seen.get('topmost') or {}).get('same_window') else 'covered or outside'}) | "
                         f"{a['attack']['rule']['decision']}: {a['attack']['rule']['why']} |")
    failed = [a for a in answers if a.get("error")]
    if failed:
        lines.append("")
        lines.append(f"{len(failed)} questions failed; the first: {failed[0]['error'][:200]}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("fetch", "run"):
        command = commands.add_parser(name)
        command.add_argument("--models", required=True)
        command.add_argument("--model", required=True)
        command.add_argument("--dir", required=True)
        if name == "run":
            command.add_argument("--server", required=True)
            command.add_argument("--dataset", required=True)
            command.add_argument("--evidence", required=True)
            command.add_argument("--scales", default="1.0,0.5,1.5")
            command.add_argument("--most", type=int, default=8)
            command.add_argument("--budget", type=float, default=105, help="minutes of asking, after which the run stops and reports")
            command.add_argument("--max-pixels", type=int, default=1600000, help="a scaled image larger than this is not asked about")
            command.add_argument("--threads", type=int, default=4)
            command.add_argument("--port", type=int, default=39377)
    arguments = parser.parse_args()
    return fetch(arguments) if arguments.command == "fetch" else run(arguments)


if __name__ == "__main__":
    sys.exit(main())

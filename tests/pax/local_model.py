#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""P@x against TUR's local AI: `tur-pax run` through the `local` preset.

    local_model.py --tur-pax PATH --models FILE --tools DIR
                   --fixture-manifests DIR --fixture-adapters DIR
                   --runtime-dir DIR --engine-log FILE --model ID
                   --work-dir DIR --evidence DIR [--max-seconds N]

Run by the `local-ai` workflow, after `tur-ai-local serve` has started a
model. P@x is used as a person would use it from a terminal, with nothing
configured by hand: `tur-pax run --provider local --model ID`. The entry
`local` of the model list gives the family (openai-chat), the address
(127.0.0.1:39281) and the key source: the key `tur-ai-local` wrote to
`tur-ai-local/api-key` in the session's runtime directory, which is given
here as XDG_RUNTIME_DIR. The tools are every component's tool set as
installed, P@x's test fixture, and the read-only command tool: it is ON,
in the real bubblewrap sandbox.

The requests are those the workflow ran before through other harnesses,
and one of P@x's own end-to-end scenarios:

* two-tools (tests/pax/scenarios/01-two-tools.json): a fixture laptop's
  hardware problems and a catalogue route;
* local-ai: which model could run locally, and why (tur-ai-local's tools);
* readonly-command (tests/pax/scenarios/03-readonly-command.json): which
  system this is and what the person's report says, from a folder the
  person granted, which needs the command tool.

The model is real, so what it does is not scripted. What it did is recorded
exactly: every tool call it made, each decision, what ran and how it ended,
the answer, the time, the requests and tokens. Those are measurements and
fail nothing. What fails the run is what TUR is responsible for:

* tur-pax started with the `local` preset and the key from the file, and
  the trail names the provider `local` beside its family;
* llama-server accepted P@x's tools: its log has no grammar error, and it
  answered P@x (a response arrived in at least one task);
* the command tool was offered;
* nothing above class 1 was allowed or performed;
* the key is in no transcript, error output, trail or file of the run.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The engine's own words when it refuses a grammar (llama.cpp b9840,
# src/llama-grammar.cpp and common/json-schema-to-grammar.cpp).
GRAMMAR_ERRORS = (
    "exceeds sane defaults",
    "failed to parse grammar",
    "error parsing grammar",
    "JSON schema conversion failed",
)

REPORT = "QUARTERLY-REPORT-OK revenue 12 units"


def scenario_request(file_name):
    return json.loads((HERE / "scenarios" / file_name).read_text())["request"]


class Results:
    """Checks and measurements, in the format the workflow's summary reads."""

    def __init__(self, path, title):
        self.path = path
        self.document = {"title": title, "checks": [], "measurements": {"tasks": []}}

    def check(self, name, passed, detail="", required=True):
        self.document["checks"].append(
            {"name": name, "passed": bool(passed), "detail": str(detail)[:1500], "required": required}
        )
        mark = "ok  " if passed else ("FAIL" if required else "no  ")
        print(f"  {mark} {name}" + (f"  [{str(detail)[:400]}]" if detail and not passed else ""), flush=True)

    def failed_required(self):
        return [check for check in self.document["checks"] if check["required"] and not check["passed"]]

    def save(self):
        self.path.write_text(json.dumps(self.document, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tur-pax", required=True, type=Path)
    parser.add_argument("--models", required=True, type=Path, help="the model list that holds the local preset")
    parser.add_argument("--tools", required=True, type=Path, help="the directory of installed tool sets")
    parser.add_argument("--fixture-manifests", required=True, type=Path)
    parser.add_argument("--fixture-adapters", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path, help="where tur-ai-local wrote tur-ai-local/api-key")
    parser.add_argument("--engine-log", required=True, type=Path)
    parser.add_argument("--model", required=True)
    parser.add_argument("--engine", default="", help="a label for the engine build, for the record")
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--max-seconds", type=int, default=1500)
    parser.add_argument("--max-steps", type=int, default=8)
    parser.add_argument("--tasks", default="", help="the tasks to run, by name, separated by commas (default: all)")
    arguments = parser.parse_args()

    work = arguments.work_dir.resolve()
    evidence = arguments.evidence.resolve()
    if work.exists():
        shutil.rmtree(work)
    evidence.mkdir(parents=True, exist_ok=True)
    home = work / "home"
    granted = home / "Documents"
    granted.mkdir(parents=True)
    (granted / "report.txt").write_text(REPORT + "\n")
    runtime = arguments.runtime_dir.resolve()
    key_file = runtime / "tur-ai-local" / "api-key"
    key = key_file.read_text().strip() if key_file.is_file() else ""

    results = Results(
        evidence / f"pax-local-{arguments.model}.json",
        f"P@x through the local preset: tur-pax run --provider local --model {arguments.model}",
    )
    print(f"P@x, the local preset, {arguments.model} ({arguments.engine})", flush=True)
    results.check("tur-ai-local wrote its key file, for its owner only", key and (key_file.stat().st_mode & 0o077) == 0,
                  f"{key_file}")

    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "LANG": "C.UTF-8",
        # Where the local preset's key file is. No key in the environment.
        "XDG_RUNTIME_DIR": str(runtime),
    }
    tasks = [
        ("two-tools", scenario_request("01-two-tools.json"), True, []),
        ("local-ai", "Which AI model could run locally on this computer, and why?", False, []),
        ("readonly-command", scenario_request("03-readonly-command.json"), False, ["--grant-read", str(granted)]),
    ]
    wanted = [name for name in arguments.tasks.split(",") if name]
    unknown = [name for name in wanted if name not in [task[0] for task in tasks]]
    if unknown:
        parser.error(f"unknown tasks: {unknown}")
    if wanted:
        tasks = [task for task in tasks if task[0] in wanted]
    local_output = []
    answered = []
    for name, request, fixture, extra in tasks:
        print(f"\n{name}: {request}", flush=True)
        audit_path = evidence / f"pax-local-{name}.audit.jsonl"
        if audit_path.exists():
            audit_path.unlink()
        command = [
            str(arguments.tur_pax),
            "run",
            request,
            "--json",
            "--provider",
            "local",
            "--model",
            arguments.model,
            "--models",
            str(arguments.models),
            "--tools",
            str(arguments.tools),
            "--audit-log",
            str(audit_path),
            "--max-steps",
            str(arguments.max_steps),
            "--max-seconds",
            str(arguments.max_seconds),
            "--task",
            f"local-{name}",
            *extra,
        ]
        if fixture:
            command += ["--manifests", str(arguments.fixture_manifests), "--adapters", str(arguments.fixture_adapters)]
        started = time.time()
        try:
            process = subprocess.run(
                command, env=environment, capture_output=True, text=True, timeout=arguments.max_seconds + 300
            )
            status, stdout, stderr = process.returncode, process.stdout, process.stderr
        except subprocess.TimeoutExpired as error:
            status = None
            stdout = error.stdout.decode("utf-8", "replace") if isinstance(error.stdout, bytes) else (error.stdout or "")
            stderr = f"timed out: {error}"
        elapsed = round(time.time() - started, 1)
        audit_text = audit_path.read_text() if audit_path.exists() else ""
        local_output += [stdout, stderr, audit_text]
        (evidence / f"pax-local-{name}.transcript.json").write_text(stdout)
        (evidence / f"pax-local-{name}.stderr.txt").write_text(stderr)
        try:
            transcript = json.loads(stdout)
        except ValueError:
            transcript = {}
        audit = [json.loads(line) for line in audit_text.splitlines() if line.strip()]

        offered = [tool.get("name") for tool in transcript.get("tools", [])]
        proposals = [item for item in transcript.get("items", []) if item.get("type") == "proposal"]
        said = [item.get("text", "") for item in transcript.get("items", []) if item.get("type") == "said"]
        decisions = [entry for entry in audit if entry.get("TUR_AI_EVENT") == "decision"]
        executed = [entry for entry in audit if entry.get("TUR_AI_EVENT") == "executed"]
        answer = transcript.get("answer") or ""
        if proposals or answer.strip() or said:
            answered.append(name)

        results.check(f"{name}: tur-pax ran with the local preset", transcript.get("provider_id") == "local"
                      and transcript.get("provider") == "openai-chat", f"exit {status}: {stderr[-600:]}")
        results.check(f"{name}: the read-only command tool was offered", "run_readonly_command" in offered, offered)
        started_events = [entry for entry in audit if entry.get("TUR_AI_EVENT") == "task_started"]
        results.check(
            f"{name}: the trail names the provider local, beside the family openai-chat",
            bool(audit)
            and {entry.get("TUR_AI_PROVIDER") for entry in audit} == {"openai-chat"}
            and [entry.get("TUR_AI_PROVIDER_ID") for entry in started_events] == ["local"],
            [(entry.get("TUR_AI_EVENT"), entry.get("TUR_AI_PROVIDER"), entry.get("TUR_AI_PROVIDER_ID")) for entry in audit][:4],
        )
        above_one = [
            entry.get("TUR_AI_ACTION")
            for entry in decisions
            if entry.get("TUR_AI_CLASS") in ("2", "3", "4") and entry.get("TUR_AI_DECISION") != "refused"
        ]
        results.check(f"{name}: no decision above class 1 is anything but refused", not above_one, above_one)
        classes = {entry.get("TUR_AI_ACTION"): entry.get("TUR_AI_CLASS") for entry in decisions}
        performed_above_one = [
            entry.get("TUR_AI_ACTION") for entry in executed if classes.get(entry.get("TUR_AI_ACTION")) not in ("0", "1")
        ]
        results.check(f"{name}: nothing above class 1 was performed", not performed_above_one, performed_above_one)

        # What the model did: measured, never a failure.
        outcome = transcript.get("outcome")
        results.check(f"{name}: the task completed", outcome == "completed",
                      f"outcome {outcome}, code {transcript.get('error_code')}, {transcript.get('message', '')}",
                      required=False)
        results.check(f"{name}: the model called at least one tool", bool(proposals), f"{len(proposals)} calls",
                      required=False)
        results.check(f"{name}: P@x performed at least one tool", bool(executed),
                      [entry.get("TUR_AI_ACTION") for entry in executed], required=False)
        if name == "readonly-command":
            results.check(f"{name}: the answer quotes the granted report", "QUARTERLY-REPORT-OK" in answer
                          or "12 units" in answer, answer[:300], required=False)
        usage = transcript.get("usage") or {}
        record = {
            "task": name,
            "request": request,
            "model": arguments.model,
            "engine": arguments.engine,
            "exit": status,
            "seconds": elapsed,
            "outcome": outcome,
            "error_code": transcript.get("error_code"),
            "message": transcript.get("message"),
            "steps": transcript.get("steps"),
            "requests": transcript.get("requests"),
            "input_tokens": usage.get("input_tokens"),
            "output_tokens": usage.get("output_tokens"),
            "usage_estimated": usage.get("estimated"),
            "tools_offered": len(offered),
            "calls": [
                {
                    "tool": item.get("tool"),
                    "class": item.get("class"),
                    "decision": item.get("decision"),
                    "reason": item.get("reason"),
                    "error_code": item.get("error_code"),
                    "executed": item.get("executed"),
                    "result": item.get("result"),
                    "exit_code": item.get("exit_code"),
                    "description": item.get("description"),
                }
                for item in proposals
            ],
            "performed": [
                {"action": entry.get("TUR_AI_ACTION"), "result": entry.get("TUR_AI_RESULT"), "exit": entry.get("TUR_AI_EXIT")}
                for entry in executed
            ],
            "said": [text[:400] for text in said],
            "answer": answer[:1500],
        }
        results.document["measurements"]["tasks"].append(record)
        print(
            f"  outcome {outcome} in {elapsed} s; calls: "
            + (", ".join(f"{call['tool']} ({call['decision']}, ran: {call['result']})" for call in record["calls"]) or "none")
            + f"; answer: {answer[:200]!r}",
            flush=True,
        )

    # The engine accepted P@x's tools: no grammar error in its log, and it
    # answered P@x at all.
    engine_log = arguments.engine_log.read_text(errors="replace") if arguments.engine_log.is_file() else ""
    grammar = [line for line in engine_log.splitlines() if any(error in line for error in GRAMMAR_ERRORS)]
    results.check("llama-server reported no grammar error for P@x's tools", not grammar, grammar[:5])
    results.check("llama-server answered P@x", bool(answered), answered)

    # The key went nowhere it should not.
    if key:
        leaked = [index for index, text in enumerate(local_output) if key in text]
        results.check("the key is in no transcript, error output or trail", not leaked, leaked)
        files = [str(path) for path in work.rglob("*") if path.is_file() and key.encode() in path.read_bytes()]
        files += [
            str(path)
            for path in evidence.rglob("*")
            if path.is_file() and path.suffix != ".log" and key.encode() in path.read_bytes()
        ]
        results.check("the key is in no file the run wrote", not files, files)

    results.save()
    lines = [f"### {results.document['title']}", "", "| Task | Outcome | Seconds | Requests | Tool calls (decision, ran) | Answer |",
             "|---|---|---|---|---|---|"]
    for record in results.document["measurements"]["tasks"]:
        calls = "; ".join(f"{call['tool']} ({call['decision']}, {call['result']})" for call in record["calls"]) or "none"
        answer = (record["answer"] or "").replace("|", "\\|").replace("\n", " ")[:200]
        lines.append(
            f"| {record['task']} | {record['outcome']} {record['error_code'] or ''} | {record['seconds']} | "
            f"{record['requests']} | {calls} | {answer} |"
        )
    (evidence / f"pax-local-{arguments.model}.md").write_text("\n".join(lines) + "\n")
    failed = results.failed_required()
    print(f"\n{len(failed)} required checks failed", flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

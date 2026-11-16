#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Speech on a real engine, for the pax-voice workflow (started by hand).

docs/architecture/pax-voice.md; .github/workflows/pax-voice.yml.

  clips      Downloads the default English and Italian voices with
             `tur-ai-local speech ensure`, speaks TUR's own test sentences
             with `tur-ai-local speech tts` (Piper), and turns each into the
             16 kHz mono 16-bit WAVE file recognition takes. Writes
             CLIPS/manifest.json: [{file, language, text, voice}]. No
             recording of a person is used, and none is made.
  recognise  Downloads each recognition model (and the voice detector),
             recognises every clip in its own language with
             `tur-ai-local speech stt`, and records the transcript, the word
             error rate against the sentence, the wall time and the peak
             memory of the recognition (tur-ai-local and whisper-cli, from
             wait4). Writes EVIDENCE/speech-results.json.
  summary    A markdown table of the results.

Only the standard library. Checks about TUR (a report that does not parse,
an engine that fails) fail the step; what a model recognises is measured and
recorded, and never fails it.
"""

import argparse
import array
import json
import os
import re
import subprocess
import sys
import time
import unicodedata
import wave

SENTENCES = [
    ("en", "Open the sound settings."),
    ("en", "Show me how much space is free on the disk."),
    ("en", "Remind me to call Marco tomorrow morning."),
    ("it", "Apri le impostazioni del suono."),
    ("it", "Mostrami quanto spazio libero c'è sul disco."),
    ("it", "Ricordami di chiamare Marco domani mattina."),
]

DEFAULT_VOICES = {"en": "voice-en-us-ljspeech-medium", "it": "voice-it-paola-medium"}
VAD = "silero-vad-v6.2.0"


def run_json(command, env, stdin=None, timeout=3600):
    """Run a tur-ai-local command with --json; return its document."""
    result = subprocess.run(
        command, env=env, input=stdin, capture_output=True, timeout=timeout
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr.decode("utf-8", "replace"))
        raise SystemExit(f"failed: {' '.join(command)}")
    return json.loads(result.stdout)


def ensure(tal, env, ids, evidence, name):
    """speech ensure, its events kept as evidence."""
    result = subprocess.run(
        [tal, "speech", "ensure", *ids, "--json"], env=env, capture_output=True, timeout=7200
    )
    with open(os.path.join(evidence, f"ensure-{name}.jsonl"), "wb") as handle:
        handle.write(result.stdout)
    if result.returncode != 0:
        sys.stderr.write(result.stderr.decode("utf-8", "replace"))
        raise SystemExit(f"speech ensure {' '.join(ids)} failed")
    events = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    for event in events:
        assert event["kind"] == "speech-ensure-event", event
    return events


def environment(data, runtime):
    env = {
        "PATH": "/usr/bin:/bin",
        "XDG_DATA_HOME": data,
        "XDG_RUNTIME_DIR": runtime,
        "HOME": data,
        "LANG": "C.UTF-8",
    }
    return env


def resample_to_16k_mono(source, target):
    """A PCM WAVE file of any rate and channel count to 16 kHz mono 16-bit,
    by averaging channels and linear interpolation. Enough for test speech."""
    with wave.open(source, "rb") as reader:
        channels = reader.getnchannels()
        width = reader.getsampwidth()
        rate = reader.getframerate()
        frames = reader.readframes(reader.getnframes())
    if width != 2:
        raise SystemExit(f"{source}: {width * 8}-bit samples, 16 expected")
    samples = array.array("h")
    samples.frombytes(frames)
    if sys.byteorder == "big":
        samples.byteswap()
    mono = [
        sum(samples[i : i + channels]) / channels for i in range(0, len(samples), channels)
    ]
    count = int(len(mono) * 16000 / rate)
    out = array.array("h")
    for index in range(count):
        position = index * rate / 16000
        left = int(position)
        right = min(left + 1, len(mono) - 1)
        fraction = position - left
        value = mono[left] * (1 - fraction) + mono[right] * fraction
        out.append(max(-32768, min(32767, int(round(value)))))
    if sys.byteorder == "big":
        out.byteswap()
    with wave.open(target, "wb") as writer:
        writer.setnchannels(1)
        writer.setsampwidth(2)
        writer.setframerate(16000)
        writer.writeframes(out.tobytes())
    return count / 16000


def words(text):
    """Lower case, accents kept, punctuation and apostrophes as spaces."""
    text = unicodedata.normalize("NFC", text.lower())
    text = re.sub(r"[^\w\s]|_", " ", text)
    return text.split()


def word_error_rate(reference, hypothesis):
    ref, hyp = words(reference), words(hypothesis)
    previous = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        current = [i]
        for j, h in enumerate(hyp, 1):
            current.append(
                min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (r != h))
            )
        previous = current
    return previous[-1] / max(1, len(ref))


def clips(args):
    os.makedirs(args.out, exist_ok=True)
    os.makedirs(args.evidence, exist_ok=True)
    env = environment(args.data, args.runtime)
    ensure(args.tal, env, list(DEFAULT_VOICES.values()), args.evidence, "voices")
    manifest = []
    timings = []
    for index, (language, text) in enumerate(SENTENCES):
        voice = DEFAULT_VOICES[language]
        spoken = os.path.join(args.out, f"spoken-{index}.wav")
        report = run_json(
            [args.tal, "speech", "tts", "--voice", voice, "--output", spoken, "--json"],
            env,
            stdin=text.encode("utf-8"),
        )
        assert report["kind"] == "tts", report
        name = f"clip-{index}-{language}.wav"
        seconds = resample_to_16k_mono(spoken, os.path.join(args.out, name))
        os.remove(spoken)
        manifest.append({"file": name, "language": language, "text": text, "voice": voice})
        timings.append(
            {
                "voice": voice,
                "text": text,
                "audio_seconds": report["audio_seconds"],
                "seconds": report["seconds"],
                "real_time_factor": report["seconds"] / max(report["audio_seconds"], 1e-6),
                "clip_seconds": seconds,
            }
        )
        print(f"{name}: {seconds:.2f} s, synthesised in {report['seconds']:.2f} s by {voice}")
    with open(os.path.join(args.out, "manifest.json"), "w") as handle:
        json.dump(manifest, handle, indent=1, ensure_ascii=False)
    with open(os.path.join(args.evidence, "speech-synthesis.json"), "w") as handle:
        json.dump(timings, handle, indent=1, ensure_ascii=False)


def recognise(args):
    os.makedirs(args.evidence, exist_ok=True)
    env = environment(args.data, args.runtime)
    with open(os.path.join(args.clips, "manifest.json")) as handle:
        manifest = json.load(handle)
    models = [model for model in args.models.split(",") if model]
    ensure(args.tal, env, [VAD], args.evidence, "vad")
    results = []
    for model in models:
        ensure(args.tal, env, [model], args.evidence, model)
        for clip in manifest:
            path = os.path.join(args.clips, clip["file"])
            command = [
                args.tal, "speech", "stt", path, "--model", model,
                "--language", clip["language"], "--vad", "--json",
            ]
            started = time.monotonic()
            process = subprocess.Popen(
                command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
            out = process.stdout.read()
            err = process.stderr.read()
            # wait4 gives this child's resource use, the largest resident set
            # of it and of whisper-cli, which it waited for.
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
            wall = time.monotonic() - started
            if process.returncode != 0:
                sys.stderr.write(err.decode("utf-8", "replace"))
                raise SystemExit(f"speech stt failed: {model} {clip['file']}")
            report = json.loads(out)
            assert report["kind"] == "stt", report
            entry = {
                "model": model,
                "clip": clip["file"],
                "language": clip["language"],
                "expected": clip["text"],
                "heard": report["text"],
                "wer": round(word_error_rate(clip["text"], report["text"]), 3),
                "audio_seconds": report["audio_seconds"],
                "recognition_seconds": round(report["seconds"], 3),
                "wall_seconds": round(wall, 3),
                "peak_memory_bytes": usage.ru_maxrss * 1024,
                "vad": report["vad"],
            }
            results.append(entry)
            print(
                f"{model} {clip['file']}: {entry['recognition_seconds']} s, "
                f"WER {entry['wer']}, peak {entry['peak_memory_bytes'] / 1e6:.0f} MB: {report['text']!r}"
            )
    machine = {
        "processors": os.cpu_count(),
        "cpuinfo_model": next(
            (line.split(":", 1)[1].strip() for line in open("/proc/cpuinfo") if line.startswith("model name")),
            None,
        ),
    }
    with open(os.path.join(args.evidence, "speech-results.json"), "w") as handle:
        json.dump({"machine": machine, "results": results}, handle, indent=1, ensure_ascii=False)


def summary(args):
    path = os.path.join(args.evidence, "speech-results.json")
    lines = ["## Speech recognition on the runner", ""]
    if not os.path.exists(path):
        lines.append("No results.")
    else:
        with open(path) as handle:
            document = json.load(handle)
        machine = document["machine"]
        lines.append(f"{machine['cpuinfo_model']}, {machine['processors']} processors.")
        lines.append("")
        lines.append("| Model | Language | Clips | Mean WER | Mean wait (s) | Peak memory (MB) |")
        lines.append("|---|---|---|---|---|---|")
        groups = {}
        for result in document["results"]:
            groups.setdefault((result["model"], result["language"]), []).append(result)
        for (model, language), rows in sorted(groups.items()):
            mean_wer = sum(row["wer"] for row in rows) / len(rows)
            mean_wait = sum(row["recognition_seconds"] for row in rows) / len(rows)
            peak = max(row["peak_memory_bytes"] for row in rows) / 1e6
            lines.append(
                f"| {model} | {language} | {len(rows)} | {mean_wer:.3f} | {mean_wait:.2f} | {peak:.0f} |"
            )
        lines.append("")
        lines.append("| Model | Clip | Heard |")
        lines.append("|---|---|---|")
        for result in document["results"]:
            heard = result["heard"].replace("|", "\\|")
            lines.append(f"| {result['model']} | {result['clip']} | {heard} |")
    synthesis = os.path.join(args.evidence, "speech-synthesis.json")
    if os.path.exists(synthesis):
        with open(synthesis) as handle:
            timings = json.load(handle)
        lines += ["", "## Speech synthesis (Piper)", "", "| Voice | Seconds of speech | Seconds to make | Real-time factor |", "|---|---|---|---|"]
        for timing in timings:
            lines.append(
                f"| {timing['voice']} | {timing['audio_seconds']:.2f} | {timing['seconds']:.2f} | {timing['real_time_factor']:.2f} |"
            )
    text = "\n".join(lines) + "\n"
    with open(args.out, "w") as handle:
        handle.write(text)
    print(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("clips", "recognise"):
        command = commands.add_parser(name)
        command.add_argument("--tal", required=True, help="the tur-ai-local program")
        command.add_argument("--data", required=True, help="XDG_DATA_HOME for the models")
        command.add_argument("--runtime", required=True, help="XDG_RUNTIME_DIR")
        command.add_argument("--evidence", required=True)
    commands.choices["clips"].add_argument("--out", required=True, help="the clips folder")
    commands.choices["recognise"].add_argument("--clips", required=True)
    commands.choices["recognise"].add_argument(
        "--models",
        default="whisper-tiny-q5-1,whisper-base-q5-1,whisper-small-q5-1,whisper-large-v3-turbo-q5-0",
    )
    command = commands.add_parser("summary")
    command.add_argument("--evidence", required=True)
    command.add_argument("--out", required=True)
    args = parser.parse_args()
    {"clips": clips, "recognise": recognise, "summary": summary}[args.command](args)


if __name__ == "__main__":
    main()

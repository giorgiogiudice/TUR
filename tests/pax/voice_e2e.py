#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""TUR's own speech through P@x: the voice turn, end to end.

    voice_e2e.py --pax PATH --ai-local PATH --clips DIR --out DIR

Run by the manually started workflow pax-voice, after its first job made
the clips: sentences in English and Italian spoken by TUR's own voices
(Piper, through tur-ai-local), listed in clips/manifest.json as
[{file, language, text, voice}]. No recording of a person is used.

Starts a private session bus (dbus-daemon) on which the bus itself starts
`tur-pax serve`, with stand-ins for the sound server's programs: the
microphone (`pw-record`) plays the clip of the turn and stays open, the
speaker (`pw-play`) swallows what it is given, and the sound server's
description (`pw-dump`) says whether another program plays sound. Local AI
is the real `tur-ai-local` with the real whisper.cpp and the free tier's
model, downloaded by the first job. The model behind P@x is the scripted
provider (fake_provider.py), through the preset llama-server, which takes no
key. Every call is made over the bus with libdbus (python3-dbus).

For each clip: StartListening, the clip, StopListening; then what was heard
(VoiceTranscript), the task it started, what the scripted model was sent,
and the audit trail. Once with another program "playing": the transcript
waits, and SendTranscript sends the person's correction.

Checks about TUR fail the run. How well the model recognised the words is
measured (word error rate, the wait) and recorded; it fails nothing.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import wave
from pathlib import Path

import dbus
import dbus.mainloop.glib
from gi.repository import GLib

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from fake_provider import ScriptedProvider  # noqa: E402

AGENT = "org.tur.Agent1"
AGENT_PATH = "/org/tur/Agent1"
RATE = 16000


def plain(value):
    if isinstance(value, (dbus.String, dbus.ObjectPath)):
        return str(value)
    if isinstance(value, dbus.Boolean):
        return bool(value)
    if isinstance(value, (dbus.Double,)):
        return float(value)
    if isinstance(value, (dbus.Int32, dbus.UInt32, dbus.Int64, dbus.UInt64, dbus.Byte)):
        return int(value)
    if isinstance(value, (list, dbus.Array, tuple, dbus.Struct)):
        return [plain(item) for item in value]
    if isinstance(value, (dict, dbus.Dictionary)):
        return {plain(key): plain(item) for key, item in value.items()}
    return value


def strings(value):
    """Every string in a JSON document."""
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        return [text for item in value for text in strings(item)]
    if isinstance(value, dict):
        return [text for item in value.values() for text in strings(item)]
    return []


def words(text):
    return re.findall(r"[\w']+", text.lower())


def word_error_rate(reference, heard):
    expected, got = words(reference), words(heard)
    if not expected:
        return 0.0 if not got else 1.0
    previous = list(range(len(got) + 1))
    for i, word in enumerate(expected, 1):
        current = [i]
        for j, other in enumerate(got, 1):
            current.append(
                min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (word != other))
            )
        previous = current
    return previous[-1] / len(expected)


def raw_16k(path, target):
    """The clip as raw 16 kHz mono 16-bit samples, for the microphone."""
    with wave.open(str(path), "rb") as clip:
        channels, width, rate = clip.getnchannels(), clip.getsampwidth(), clip.getframerate()
        frames = clip.readframes(clip.getnframes())
    if width != 2:
        raise ValueError(f"{path}: not 16-bit")
    samples = [
        int.from_bytes(frames[i : i + 2], "little", signed=True)
        for i in range(0, len(frames), 2 * channels)
    ]
    if rate != RATE:
        ratio = rate / RATE
        count = int(len(samples) / ratio)
        resampled = []
        for n in range(count):
            position = n * ratio
            left = int(position)
            right = min(left + 1, len(samples) - 1)
            fraction = position - left
            resampled.append(int(samples[left] * (1 - fraction) + samples[right] * fraction))
        samples = resampled
    target.write_bytes(b"".join(sample.to_bytes(2, "little", signed=True) for sample in samples))
    return len(samples) / RATE


class Checks:
    def __init__(self):
        self.results = []

    def check(self, name, what, ok, detail=""):
        self.results.append({"name": name, "what": what, "ok": bool(ok), "detail": str(detail)[:2000]})
        print(("PASS " if ok else "FAIL ") + f"{name}: {what}" + (f" ({detail})" if detail and not ok else ""), flush=True)
        return ok

    def failed(self):
        return [result for result in self.results if not result["ok"]]


class Test:
    def __init__(self, pax, ai_local, clips, out):
        self.pax = pax
        self.ai_local = ai_local
        self.clips = clips
        self.out = out
        self.work = out / "work"
        self.checks = Checks()
        self.signals = []
        self.measured = []
        self.provider = None
        self.bus_process = None

    # ------------------------------------------------------------- setup

    def environment(self):
        return {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.work / "home"),
            "XDG_CONFIG_HOME": str(self.work / "config"),
            "XDG_STATE_HOME": str(self.work / "state"),
            "XDG_DATA_HOME": os.environ.get("XDG_DATA_HOME", str(self.work / "data")),
            "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", str(self.work / "run")),
            "DBUS_SESSION_BUS_ADDRESS": self.address,
            "LANG": "C.UTF-8",
        }

    def script(self, name, body):
        path = self.work / "bin" / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)
        return path

    def prepare(self):
        if self.work.exists():
            shutil.rmtree(self.work)
        for folder in ("bin", "home", "config/tur/pax", "state", "bus/services", "tools/manifests", "tools/adapters"):
            (self.work / folder).mkdir(parents=True, exist_ok=True)
        data = ROOT / "services" / "tur-pax" / "tests" / "data"
        shutil.copy(data / "test-service.manifest.json", self.work / "tools/manifests/test-service.json")
        shutil.copy(data / "test-service.adapters.json", self.work / "tools/adapters/test-service.json")

        # The microphone plays the clip named in a file, then stays open; the
        # sound server says what dump.json says; the speaker swallows.
        self.clip_file = self.work / "current-clip"
        self.dump_file = self.work / "dump.json"
        self.dump_file.write_text("[]")
        record = self.script("pw-record", f'cat "$(cat {self.clip_file})"\nexec sleep 300')
        play = self.script("pw-play", "exec cat > /dev/null")
        dump = self.script("pw-dump", f"cat {self.dump_file}")

        self.provider = ScriptedProvider([{"text": "I heard you."}], repeat_last=True, wire="openai-chat").start()
        settings = {
            "schema_version": 1,
            "enabled": False,
            "provider": "llama-server",
            "model": "scripted-model",
            "base_url": self.provider.base_url + "/v1",
        }
        (self.work / "config/tur/pax/settings.json").write_text(json.dumps(settings, indent=2))

        socket = self.work / "bus" / "socket"
        self.address = f"unix:path={socket}"
        config = self.work / "bus" / "bus.conf"
        config.write_text(
            f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{self.address}</listen>
  <auth>EXTERNAL</auth>
  <servicedir>{self.work / "bus" / "services"}</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"""
        )
        models = ROOT / "services" / "tur-pax" / "data" / "models.json"
        (self.work / "bus" / "services" / f"{AGENT}.service").write_text(
            "[D-BUS Service]\n"
            f"Name={AGENT}\n"
            f"Exec={self.pax} serve --models {models} --ai-local {self.ai_local}"
            f" --manifests {self.work / 'tools/manifests'} --adapters {self.work / 'tools/adapters'}"
            f" --pw-record {record} --pw-play {play} --pw-dump {dump}"
            f" --live {ROOT / 'services' / 'tur-pax' / 'data' / 'live.json'}\n"
        )
        self.bus_log = open(self.out / "voice-bus.log", "w")
        self.bus_process = subprocess.Popen(
            ["dbus-daemon", "--nofork", f"--config-file={config}"],
            env=self.environment(),
            stdout=self.bus_log,
            stderr=subprocess.STDOUT,
        )
        deadline = time.time() + 20
        while not socket.exists() and time.time() < deadline:
            time.sleep(0.05)
        dbus.mainloop.glib.threads_init()
        dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
        self.listener = dbus.bus.BusConnection(self.address)
        self.listener.add_signal_receiver(
            self.on_signal, dbus_interface=AGENT, path=AGENT_PATH, member_keyword="member"
        )
        self.loop = GLib.MainLoop()
        threading.Thread(target=self.loop.run, daemon=True).start()
        self.client = dbus.bus.BusConnection(self.address)

    def on_signal(self, *arguments, member=None):
        self.signals.append({"member": str(member), "args": plain(list(arguments)), "at": time.time()})

    def stop(self):
        try:
            self.loop.quit()
        except Exception:  # noqa: BLE001
            pass
        if self.bus_process and self.bus_process.poll() is None:
            self.bus_process.terminate()
        if self.provider:
            self.provider.stop()
        subprocess.run(["pkill", "-f", f"{self.work}/bin/pw-record"], check=False)

    # ------------------------------------------------------------ helpers

    def call(self, method, signature="", *arguments, timeout=120):
        try:
            reply = self.client.call_blocking(AGENT, AGENT_PATH, AGENT, method, signature, list(arguments), timeout=timeout)
            return plain(reply), None
        except dbus.exceptions.DBusException as error:
            return None, f"{error.get_dbus_name()}: {error}"

    def prop(self, name):
        try:
            value = self.client.call_blocking(
                AGENT, AGENT_PATH, "org.freedesktop.DBus.Properties", "Get", "ss", [AGENT, name]
            )
            return plain(value)
        except dbus.exceptions.DBusException:
            return None

    def wait_signal(self, member, first_arg, timeout=180):
        deadline = time.time() + timeout
        while time.time() < deadline:
            for item in self.signals:
                if item["member"] == member and item["args"] and item["args"][0] == first_arg:
                    return item
            time.sleep(0.05)
        return None

    def audit(self):
        path = self.work / "state" / "tur" / "pax" / "audit.jsonl"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]

    # -------------------------------------------------------------- steps

    def switch_on(self):
        reply, error = self.call("GetConsentText", "s", "llama-server")
        if not self.checks.check("switch", "the consent text of the preset is given", error is None, error):
            return False
        _, error = self.call("Enable", "s", reply[0])
        self.checks.check("switch", "P@x is switched on", error is None, error)
        _, error = self.call("SetVoiceSettings", "s", json.dumps({"enabled": True}))
        self.checks.check("switch", "voice is switched on", error is None, error)
        status, error = self.call("GetVoiceStatus")
        status = json.loads(status) if status else {}
        (self.out / "voice-status.json").write_text(json.dumps(status, indent=2))
        self.checks.check(
            "switch",
            "local AI's recognition model is on this computer",
            status.get("recognition", {}) and status["recognition"].get("installed") in (True, "installed"),
            status.get("recognition"),
        )
        return True

    def turn(self, clip, other_sound=False):
        raw = self.work / (Path(clip["file"]).stem + ".raw")
        seconds = raw_16k(self.clips / clip["file"], raw)
        self.clip_file.write_text(str(raw))
        if other_sound:
            self.dump_file.write_text(json.dumps([{
                "type": "PipeWire:Interface:Node",
                "info": {"state": "running", "props": {"media.class": "Stream/Output/Audio", "application.name": "Firefox"}},
            }]))
        else:
            self.dump_file.write_text("[]")
        self.call("SetVoiceSettings", "s", json.dumps({"language": clip["language"]}))
        options = json.dumps({"trigger": "push-to-talk", "shown_by_caller": True})
        turn, error = self.call("StartListening", "s", options)
        name = f"{clip['language']}:{Path(clip['file']).stem}" + (" (other sound)" if other_sound else "")
        if not self.checks.check(name, "the microphone opens", error is None, error):
            return None
        self.checks.check(name, "MicrophoneOpen says so", self.prop("MicrophoneOpen") is True)
        time.sleep(seconds + 0.8)
        stopped_at = time.time()
        _, error = self.call("StopListening", "sb", turn, True)
        self.checks.check(name, "the microphone closes when the key is let go", error is None, error)
        self.checks.check(name, "MicrophoneOpen says it is closed", self.prop("MicrophoneOpen") is False)
        signal = self.wait_signal("VoiceTranscript", turn)
        if not self.checks.check(name, "what was heard is sent to the window", signal is not None):
            return None
        details = json.loads(signal["args"][1])
        heard = details.get("text", "")
        wait = signal["at"] - stopped_at
        error_rate = word_error_rate(clip["text"], heard)
        self.measured.append({
            "clip": clip["file"], "language": clip["language"], "voice": clip.get("voice"),
            "said": clip["text"], "heard": heard, "wer": round(error_rate, 3),
            "audio_seconds": round(seconds, 2), "wait_seconds": round(wait, 2),
            "recognition_seconds": details.get("seconds"), "model": details.get("model"),
            "check": details.get("check"), "other_sound": other_sound,
        })
        self.checks.check(name, "something was heard", bool(heard.strip()), details)
        left = list((Path(self.environment()["XDG_RUNTIME_DIR"]) / "tur-pax" / "voice").glob("*"))
        self.checks.check(name, "no audio is left on disk", not left, left)
        if other_sound:
            self.checks.check(name, "heard over another program's sound, it waits for the person",
                              details.get("check") == "wait" and details.get("reason") == "other_sound", details)
            corrected = heard.strip() + " (checked)"
            task, error = self.call("SendTranscript", "ss", turn, corrected)
            if not self.checks.check(name, "the person sends the corrected words", error is None, error):
                return None
            source = "voice_edited"
            request = corrected
        else:
            self.checks.check(name, "it is shown and sent", details.get("check") == "show", details)
            task = details.get("task")
            if not self.checks.check(name, "a task starts from it", bool(task), details):
                return None
            source = "voice"
            request = heard
        finished = self.wait_signal("TaskFinished", task)
        result = json.loads(finished["args"][1]) if finished else {}
        self.checks.check(name, "the task ends", result.get("outcome") == "completed", result)
        self.checks.check(name, "its request is what was sent", result.get("request") == request, result.get("request"))
        texts = []
        for sent in list(self.provider.requests):
            try:
                texts.extend(strings(json.loads(sent["body"])))
            except (ValueError, KeyError):
                pass
        self.checks.check(name, "the model was sent those words", any(request in text for text in texts))
        started = [entry for entry in self.audit() if entry.get("TUR_AI_EVENT") == "task_started"]
        self.checks.check(name, f"the audit says the request was {source}",
                          bool(started) and started[-1].get("TUR_AI_SOURCE") == source, started[-1:] if started else None)
        trail = json.dumps(self.audit())
        self.checks.check(name, "the audit keeps no words", request[:20] not in trail)
        return task

    def report(self):
        (self.out / "voice-e2e.json").write_text(json.dumps(
            {"checks": self.checks.results, "measured": self.measured, "signals": len(self.signals)}, indent=2))
        lines = ["# The clips through P@x", "",
                 "| Clip | Language | Heard | WER | Audio s | Wait s | Check |", "|---|---|---|---|---|---|---|"]
        for item in self.measured:
            lines.append(f"| {item['clip']} | {item['language']} | {item['heard']} | {item['wer']} | "
                         f"{item['audio_seconds']} | {item['wait_seconds']} | {item['check']} |")
        failed = self.checks.failed()
        lines += ["", f"{len(self.checks.results) - len(failed)} of {len(self.checks.results)} checks passed."]
        for item in failed:
            lines.append(f"- FAIL {item['name']}: {item['what']} ({item['detail']})")
        (self.out / "voice-e2e.md").write_text("\n".join(lines) + "\n")
        return 1 if failed or not self.checks.results else 0


def main():
    parser = argparse.ArgumentParser(description="TUR's own speech through P@x.")
    parser.add_argument("--pax", required=True, type=Path)
    parser.add_argument("--ai-local", required=True, type=Path)
    parser.add_argument("--clips", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    arguments = parser.parse_args()
    out = arguments.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    clips = json.loads((arguments.clips / "manifest.json").read_text())
    test = Test(arguments.pax.resolve(), arguments.ai_local.resolve(), arguments.clips.resolve(), out)
    try:
        test.prepare()
        if test.switch_on():
            for clip in clips:
                test.turn(clip)
            if clips:
                test.turn(clips[0], other_sound=True)
    except Exception:  # noqa: BLE001
        import traceback

        test.checks.check("harness", "the test ran to its end", False, traceback.format_exc())
    finally:
        test.stop()
    return test.report()


if __name__ == "__main__":
    sys.exit(main())

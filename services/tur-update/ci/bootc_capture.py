#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Hold tur-update against the real bootc and ostree of the base image.

Runs in CI only (the `update` workflow). Three steps:

    bootc_capture.py capture IMAGE OUT
        Run the image as a container and record what its bootc and ostree
        print: versions, `bootc status` in every format, as root and as an
        ordinary user, and the help of every subcommand tur-update names.
        In a container bootc reports a host that is not a bootc host; that
        is recorded as it is.

    bootc_capture.py check OUT FIXTURES [--tur-update BINARY]
        Compare the capture with what is committed: the JSON bootc printed
        must be the fixture bootc/container-base-image.json, and the version
        must be the one the fixtures were verified against. With a binary,
        take the list of commands it names (`tur-update commands --json`)
        and check each against the capture: the program exists in the image
        and every option is one its help describes.

    bootc_capture.py live IMAGE BINARY FIXTURES OUT
        Run the built tur-update inside the container, so that its one live
        command is the image's real bootc: `status --live` must end with
        TUR-UPDATE-STATE-002 (not a bootc host), and reading a fixture there
        must work.

`PODMAN` in the environment names the container program (default: podman).
Nothing here is given a secret, and nothing is uploaded by this script.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shlex
import subprocess
import sys

PODMAN = shlex.split(os.environ.get("PODMAN", "podman"))

# The help pages captured. Every command tur-update names must be among them.
HELP = [
    ["/usr/bin/bootc", "status"],
    ["/usr/bin/bootc", "upgrade"],
    ["/usr/bin/bootc", "rollback"],
    ["/usr/bin/bootc", "switch"],
    ["/usr/bin/ostree", "admin", "pin"],
]

GUEST_SCRIPT = r"""
set -u
out=/out
record() {
  # record NAME COMMAND...: stdout, stderr and the exit status of a command.
  name="$1"; shift
  "$@" >"$out/$name.out" 2>"$out/$name.err"
  echo "$?" >"$out/$name.exit"
}
record bootc-version /usr/bin/bootc --version
record ostree-version /usr/bin/ostree --version
record packages rpm -q bootc ostree ostree-libs rpm-ostree bootupd
record programs ls -l /usr/bin/bootc /usr/bin/ostree
record status-json /usr/bin/bootc status --format=json
record status-json-legacy /usr/bin/bootc status --json
record status-json-v1 /usr/bin/bootc status --format=json --format-version=1
record status-json-booted /usr/bin/bootc status --format=json --booted
record status-yaml /usr/bin/bootc status --format=yaml
record status-human /usr/bin/bootc status --format=humanreadable
record status-bad-format /usr/bin/bootc status --format=json --format-version=2
record update-timer systemctl --root=/ is-enabled bootc-fetch-apply-updates.timer
record os-release cat /usr/lib/os-release
"""


def run(arguments: list[str], **kwargs) -> subprocess.CompletedProcess:
    print("+", " ".join(shlex.quote(argument) for argument in arguments), flush=True)
    return subprocess.run(arguments, check=False, **kwargs)


def help_name(command: list[str]) -> str:
    return "help-" + "-".join(pathlib.Path(word).name for word in command)


def capture(image: str, out: pathlib.Path) -> int:
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    script = GUEST_SCRIPT
    for command in HELP:
        script += f"record {help_name(command)} {' '.join(command)} --help\n"
    base = [*PODMAN, "run", "--rm", "--network=none", "-v", f"{out}:/out", image]
    result = run([*base, "bash", "-c", script])
    if result.returncode != 0:
        print(f"the container ended with status {result.returncode}")
        return 1
    # The same question from an ordinary user.
    user = [*PODMAN, "run", "--rm", "--network=none", "--user", "1000:1000", image]
    asked = run(
        [*user, "/usr/bin/bootc", "status", "--format=json"], capture_output=True, text=True
    )
    (out / "status-json-user.out").write_text(asked.stdout, encoding="utf-8")
    (out / "status-json-user.err").write_text(asked.stderr, encoding="utf-8")
    (out / "status-json-user.exit").write_text(f"{asked.returncode}\n", encoding="utf-8")
    inspect = run(
        [*PODMAN, "image", "inspect", "--format", "{{.Size}} {{.Digest}} {{.Id}}", image],
        capture_output=True,
        text=True,
    )
    (out / "image-inspect.out").write_text(inspect.stdout, encoding="utf-8")
    run(["sudo", "chown", "-R", f"{os.getuid()}:{os.getgid()}", str(out)])
    for path in sorted(out.iterdir()):
        if path.suffix == ".exit":
            print(f"{path.stem}: exit {path.read_text().strip()}")
    for name in ("bootc-version", "ostree-version", "packages", "status-json", "status-json-user"):
        print(f"--- {name}")
        print((out / f"{name}.out").read_text(encoding="utf-8").rstrip())
        error = (out / f"{name}.err").read_text(encoding="utf-8").rstrip()
        if error:
            print(f"(stderr) {error}")
    return 0


class Checks:
    def __init__(self) -> None:
        self.failed = 0
        self.lines: list[str] = []

    def check(self, what: str, passed: bool, detail: str = "") -> None:
        mark = "pass" if passed else "FAIL"
        line = f"{mark}  {what}" + (f"  [{detail}]" if detail else "")
        print(line, flush=True)
        self.lines.append(f"| {what} | {'pass' if passed else '**FAIL**'} | {detail} |")
        if not passed:
            self.failed += 1

    def summary(self, title: str, out: pathlib.Path) -> None:
        text = f"### {title}\n\n| Check | Result | Evidence |\n|---|---|---|\n" + "\n".join(self.lines)
        with (out / "summary.md").open("a", encoding="utf-8") as handle:
            handle.write(text + "\n\n")


def read(out: pathlib.Path, name: str) -> tuple[str, str, int]:
    return (
        (out / f"{name}.out").read_text(encoding="utf-8"),
        (out / f"{name}.err").read_text(encoding="utf-8"),
        int((out / f"{name}.exit").read_text(encoding="utf-8").strip()),
    )


def check(out: pathlib.Path, fixtures: pathlib.Path, binary: str | None) -> int:
    checks = Checks()
    provenance = json.loads((fixtures / "provenance.json").read_text(encoding="utf-8"))

    version_text, _, status = read(out, "bootc-version")
    version = version_text.split()[-1] if version_text.split() else ""
    checks.check(
        "bootc in the base image is the version the fixtures were verified against",
        status == 0 and version == provenance["bootc_version"],
        f"image: {version_text.strip()!r}; fixtures: {provenance['bootc_version']}",
    )

    text, error, status = read(out, "status-json")
    checks.check("bootc status --format=json succeeds in a container", status == 0, error.strip())
    fixture = fixtures / "bootc" / "container-base-image.json"
    if not fixture.is_file():
        checks.check("the committed fixture bootc/container-base-image.json exists", False)
    else:
        try:
            same = json.loads(text) == json.loads(fixture.read_text(encoding="utf-8"))
        except json.JSONDecodeError as problem:
            same = False
            error = str(problem)
        checks.check(
            "what bootc printed is the committed fixture bootc/container-base-image.json",
            same,
            text.strip()[:300],
        )
    for name in ("status-json-legacy", "status-json-v1", "status-json-booted"):
        other, _, status = read(out, name)
        try:
            same = status == 0 and json.loads(other) == json.loads(text)
        except json.JSONDecodeError:
            same = False
        checks.check(f"{name}: the same document", same)
    _, error, status = read(out, "status-bad-format")
    checks.check(
        "--format-version=2 is refused (there is one format version)",
        status != 0,
        error.strip()[:200],
    )
    user_text, user_error, status = read(out, "status-json-user")
    try:
        same = status == 0 and json.loads(user_text) == json.loads(text)
    except json.JSONDecodeError:
        same = False
    checks.check(
        "in a container an ordinary user gets the same document (informational)",
        True,
        f"exit {status}; same document: {same}; stderr: {user_error.strip()[:200]}",
    )

    if binary:
        listed = subprocess.run(
            [binary, "commands", "--json"], check=True, capture_output=True, text=True
        )
        commands = json.loads(listed.stdout)["commands"]
        checks.check("tur-update names seven commands of other programs", len(commands) == 7, str(len(commands)))
        programs, _, _ = read(out, "programs")
        for command in commands:
            program = command["program"]
            words = [program, *command["subcommand"]]
            line = " ".join([*words, *command["options"]])
            checks.check(f"{command['id']}: {program} is in the image", program in programs)
            if words not in HELP:
                checks.check(f"{command['id']}: the help of `{' '.join(words)}` was captured", False)
                continue
            help_text, help_error, status = read(out, help_name(words))
            checks.check(f"{command['id']}: `{' '.join(words)} --help` succeeds", status == 0, help_error.strip()[:200])
            for option in command["options"]:
                flag = option.split("=", 1)[0]
                present = re.search(rf"(?<![\w-]){re.escape(flag)}(?![\w-])", help_text) is not None
                checks.check(f"{command['id']}: `{line}`: {flag} is an option its help describes", present)
            for operand in command["operands"]:
                checks.check(
                    f"{command['id']}: operand {operand} (documented in ostree-admin-pin(1), not in --help)",
                    True,
                    "informational",
                )
    checks.summary("bootc and ostree of the base image", out)
    return 1 if checks.failed else 0


def live(image: str, binary: str, fixtures: pathlib.Path, out: pathlib.Path) -> int:
    checks = Checks()
    binary = str(pathlib.Path(binary).resolve())
    fixtures = fixtures.resolve()
    base = [
        *PODMAN, "run", "--rm", "--network=none",
        "-v", f"{binary}:/usr/bin/tur-update:ro",
        "-v", f"{fixtures}:/fixtures:ro",
        image, "/usr/bin/tur-update",
    ]  # fmt: skip

    def ask(name: str, arguments: list[str]) -> subprocess.CompletedProcess:
        result = run([*base, *arguments], capture_output=True, text=True)
        (out / f"live-{name}.out").write_text(result.stdout, encoding="utf-8")
        (out / f"live-{name}.err").write_text(result.stderr, encoding="utf-8")
        (out / f"live-{name}.exit").write_text(f"{result.returncode}\n", encoding="utf-8")
        return result

    result = ask("status", ["status", "--live", "--json"])
    checks.check(
        "tur-update status --live in the container: the image's bootc is run and answers "
        "that this is not a bootc host",
        result.returncode == 1 and result.stderr.startswith("TUR-UPDATE-STATE-002\n") and result.stdout == "",
        f"exit {result.returncode}; {result.stderr.strip()[:300]}",
    )
    result = ask("check", ["check", "--live", "--json"])
    checks.check(
        "tur-update check --live in the container: the same code",
        result.returncode == 1 and result.stderr.startswith("TUR-UPDATE-STATE-002\n"),
        f"exit {result.returncode}",
    )
    result = ask("fixture", ["status", "--from", "/fixtures/bootc/tur-staged.json", "--json"])
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        report = {}
    checks.check(
        "tur-update runs on the image and reads a recorded machine there",
        result.returncode == 0 and report.get("kind") == "status" and report.get("state") == "ready",
        f"exit {result.returncode}; state {report.get('state')}",
    )
    result = ask("version", ["--version"])
    checks.check("tur-update --version", result.returncode == 0, result.stdout.strip())
    checks.summary("tur-update in the base image", out)
    return 1 if checks.failed else 0


def main() -> int:
    arguments = sys.argv[1:]
    if len(arguments) >= 3 and arguments[0] == "capture":
        return capture(arguments[1], pathlib.Path(arguments[2]).resolve())
    if len(arguments) >= 3 and arguments[0] == "check":
        binary = None
        if "--tur-update" in arguments:
            binary = arguments[arguments.index("--tur-update") + 1]
        return check(pathlib.Path(arguments[1]), pathlib.Path(arguments[2]), binary)
    if len(arguments) == 5 and arguments[0] == "live":
        return live(arguments[1], arguments[2], pathlib.Path(arguments[3]), pathlib.Path(arguments[4]))
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())

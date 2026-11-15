<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Experiment drvsynth: driver synthesis from USB captures

> **Blind engineer: do not read this file.** Your instructions are in
> [`handoff/README.md`](handoff/README.md); use nothing outside `handoff/`.

**Hypothesis (tested in stage B, by someone else):** given only a
recording of the USB traffic between a device and its Windows driver, an AI
can write a working native Linux driver for the device. If it holds, TUR
could learn a device once from its Windows driver (which TUR can already
run in Wine's user-space driver host: `docs/reports/windows-drivers-2.md`)
and every TUR computer would then support it natively.

This directory is **stage A, the test bench**. Its result is
`docs/reports/exp-drvsynth-1.md`.

## Layout and separation

| Path | Who may read it | What |
|---|---|---|
| `handoff/` | the blind engineer, and only this | the task card (`TASK.md`), the device's `lsusb -v`, the captures (pcapng and text) with user-action timelines, the rules (`README.md`) |
| `candidate/` | written by the blind engineer | the Linux driver under test (`gadget.py`); absent until a trial |
| `bench/` | the bench's maintainers | capture tooling that holds no protocol knowledge: the usbmon recorder and pcapng writer (`usbmon.py`), the scripted sessions (`sessions.py`), the device "world" client (`world.py`), the CI build and run scripts |
| `sealed/` | **never the blind engineer** | the protocol specification (`PROTOCOL.md`), the emulated device (`emulator.py`), the Windows KMDF driver and program (`windows/`), the reference Linux driver (`reference/`), two deliberately wrong drivers (`wrong/`), the scoring suite (`score.py`) |

The device, its protocol, its vendor and its name were invented for this
experiment and exist nowhere else, so no model can know them from training
data. The hand-off bundle names only what a user or a USB analyser would
see: the USB descriptors, the bytes on the wire, and what the user did and
saw. It contains no source, no protocol description and no internal names;
the session harness drives the device's "world" (temperature, buttons)
through a socket that speaks user-level terms only, and the capture's
timelines are written from those terms.

What the blind engineer must not see, besides `sealed/`: this file, `bench/`,
the report, the pull requests, and the CI logs, artifacts and step
summaries of the `exp-drvsynth` workflow (the scoring output names what is
checked). The `drvsynth-handoff` artifact is the only CI output meant for
them, and its content is what is committed under `handoff/`.

## How the bench works

All of it runs in CI (`.github/workflows/exp-drvsynth.yml`); nothing is
built or run on a development machine.

- **capture** job: builds the Windows driver (`sealed/windows/qfdesk.c`,
  KMDF 1.33) and program (`gadget.c`) with the toolchain of the
  windows-drivers experiment, on its KMDF runtime built from Microsoft's MIT
  source and its Wine patch set (both cached). The emulated device is
  plugged into the runner's USB/IP virtual host controller; Wine's driver
  host loads the driver; `bench/sessions.py` runs ten scripted sessions,
  each recorded by usbmon, and checks every action's output and the
  device's resulting state (the captures are only handed off if all
  checks pass). Artifact `drvsynth-handoff`.
- **score** job: starts a fresh emulated device (a firmware version, serial
  number and nonces that appear in no capture), then runs
  `sealed/score.py` against the reference driver, the two wrong drivers
  and, if present, `candidate/gadget.py`. Each driver runs as an
  unprivileged user that can reach the device node but not the checkout or
  the device's control socket. `score.py validate` fails the job unless
  the reference passes every feature and each wrong driver fails what it
  gets wrong. Artifact `drvsynth-sealed-score`.

Refreshing the hand-off bundle: download `drvsynth-handoff` from a green
capture run (`scripts/dev/ci.py artifact RUN drvsynth-handoff DIR`), replace
`handoff/descriptors.txt` and `handoff/captures/` with it, commit, and note
the run ID in the report. Changing a session or the device invalidates any
trial run against the old bundle.

## Running a blind trial (stage B)

1. Give the blind engineer (an AI agent, in a fresh context) this
   repository with the instruction to read `tests/experiments/drvsynth/handoff/README.md`
   and follow it. Nothing else: no summary of the experiment, no hints.
   Record the model, its settings, the date, and the full transcript.
2. The engineer writes `tests/experiments/drvsynth/candidate/gadget.py`
   and declares it final. They may not run it against the device, and must
   not see CI output of this workflow before declaring it final.
3. Open a pull request with only `candidate/`. The capture job skips itself;
   the score job scores the candidate (`score-candidate.md` in the step
   summary and artifact) alongside the reference and wrong drivers.
4. Audit the transcript for rule breaks (any file read outside `handoff/`
   voids the trial).

## What counts as success

- **Bench valid** (stage A): the capture job is green (every Windows-side
  check passed) and the score job is green (reference 18/18, the wrong
  drivers fail as designed).
- **Hypothesis supported** (stage B): the candidate passes **every** scored
  feature, 18 of 18, including the checks no guess can pass: the handshake
  that depends on a fresh device-chosen value, checksums over payloads that
  appear in no capture, the device's quirks, and values with byte orders and
  ranges the captures only partly show. Fewer than 18 is reported as the
  number passed and which failed; it does not support the hypothesis, but
  says how far synthesis got.
- One trial is one data point. Repeat with other models or other invented
  devices before drawing conclusions.

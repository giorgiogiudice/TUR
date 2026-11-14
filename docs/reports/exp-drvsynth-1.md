<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-drvsynth-1 — driver synthesis, stage A: the test bench

> **Blind engineers: do not read this report.** Use only
> `tests/experiments/drvsynth/handoff/`. This report deliberately does not
> describe the device's protocol; the specification is
> `tests/experiments/drvsynth/sealed/PROTOCOL.md`, referred to here only by
> path.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-drvsynth-1 (experiment drvsynth, stage A)
Subsystem:        Windows compatibility (Wine's driver host), as a test
                  instrument; no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  branch exp-drvsynth, pull request 122. First full run:
                  workflow run 43536094203 on 4f14f1c. The run the
                  committed hand-off captures come from: 43540502703 on fb1b93f.

SUMMARY
- What changed: a test bench for the hypothesis "given only a recording of
  the USB traffic between a device and its Windows driver, an AI can write
  a working native Linux driver for it": an invented USB device emulated in
  user space, its Windows KMDF driver and program run in Wine's driver host,
  usbmon captures of scripted sessions with user-action timelines, a
  hand-off bundle for a blind engineer, and a hidden scoring suite proven
  against a reference driver and two deliberately wrong ones.
- Why it changed: the owner wants to know whether TUR could learn a device
  once from its Windows driver (which windows-drivers-2 showed TUR can run
  in user space) and then support it natively everywhere. Stage A builds
  the instrument; stage B, the blind trial, is someone else's.

FILES
- Added: tests/experiments/drvsynth/{README.md, handoff/**, bench/**,
  sealed/**}, .github/workflows/exp-drvsynth.yml,
  docs/reports/exp-drvsynth-1.md
- Modified: none (the windows-drivers experiment's emulator attach code,
  harness, KMDF build scripts, stub and Wine patch set are reused
  unchanged)
- Removed: none

BUILD
- Command: tests/experiments/drvsynth/bench/ci-build.sh in a Fedora 45
  container, in CI only. Nothing was built or run on a development machine
  (Python and shell files were only parsed for syntax locally).
- Result: qfdesk.sys (KMDF 1.33, 90,112 bytes, imports WDFLDR.SYS and
  ntoskrnl.exe only) and gadget.exe (390,996 bytes) built with clang/lld
  and mingw-w64; the KMDF runtime (Wdf01000.sys, 1,507,328 bytes) and the
  Wine 11.20 overlay with TUR's sixteen patches built by the
  windows-drivers scripts and cached for later runs.
- Warnings: none reported for the program; the driver is compiled with -w,
  as the windows-drivers drivers are.

TESTS
- Unit tests: none (no TUR component).
- Integration tests: workflow exp-drvsynth, run 43536094203: job capture
  (145 of 145 checks PASS) and job score (suite valid: reference 18/18,
  wrong-replay 0/18, wrong-quirks 10/18 as designed). Run 43540502703
  (fb1b93f): capture green (all checks PASS), score green with the same
  18/18, 0/18 and 10/18 (suite valid).
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. The device is emulated (USB/IP); it does not
  exist as hardware.
- Performance tests: none.

DIAGNOSTICS
- Log paths: artifacts of each run: drvsynth-handoff (the bundle's
  captures, timelines, descriptors), drvsynth-sealed-capture-evidence
  (build logs, Wine logs per action, results.json/md, the emulator's log),
  drvsynth-sealed-score (score-*.json/md, validation.md). The sealed
  artifacts reveal the protocol; blind engineers must not open them.
- Diagnostic bundle path: none
- Relevant error IDs: none

RECOVERY
- Rollback procedure: revert the merge; nothing outside tests/experiments/
  drvsynth, the new workflow and this report changes.
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 6.

NEXT SAFE ACTION
- Stage B: run a blind trial as described in
  tests/experiments/drvsynth/README.md ("Running a blind trial"), with the
  transcript recorded, and score the candidate.
```

## 1. What was built

| Part | Where | What |
|---|---|---|
| The device | `sealed/emulator.py` | An invented vendor-class USB 2.0 device ("Quillfeather Labs Glimmerdesk G2", `1209:000c`, a pid.codes test ID): a desk companion with a sensor block, a 16 x 8 LED matrix with grey levels, a note store, two buttons, day/night modes and a temperature alert. Control, bulk and interrupt endpoints. Emulated in user space and plugged into the runner kernel's USB/IP virtual host controller with the windows-drivers experiment's attach code (`tests/windows-drivers/virtual-usb/usbip_devices.py`), so the host sees an ordinary USB device. A Unix socket readable only by root drives the "physical world" (temperature, humidity, light, button presses) and reports what the device shows and stores, in user terms. |
| Its protocol | `sealed/PROTOCOL.md` | Written for this experiment; exists nowhere else. It has every element the task asked for (a start-up exchange that depends on a value the device chooses each time, length-prefixed frames with a checksum that is not a stock CRC, transfers spread over several packets in both directions, modes that change what commands mean, asynchronous interrupt reports, and more than one quirk of the kind real devices have). Their details are in that file only. |
| Its Windows driver | `sealed/windows/qfdesk.c` | A KMDF 1.33 driver on TUR's KMDF runtime (built from Microsoft's MIT source) with TUR's stub, loaded by Wine's `winedevice.exe` in user space. The protocol lives in the driver; a continuous reader on the interrupt pipe keeps event reports. |
| Its Windows program | `sealed/windows/gadget.c` | `gadget.exe COMMAND`: every function of the device through the driver. Its commands and output are exactly the command-line contract the Linux driver must follow, so the timelines show what a correct Linux driver prints. |
| Capture | `bench/usbmon.py` | Reads the kernel's usbmon binary interface (`/dev/usbmonN`) and writes each event unchanged as a pcapng Enhanced Packet Block (link type USB Linux, 189); writes a readable text form from the pcapng. Every capture is cross-checked with tshark 4.6.9: same frame count. |
| Sessions | `bench/sessions.py` | Ten scripted sessions; each is one usbmon capture during which `gadget.exe` runs once per user action (79 actions), as an unprivileged user, while the world is changed between and during actions. Writes a timeline per session in user terms (what the user did, what the program printed, what happened in the room) on the capture's clock, and checks every action's output and the device's resulting state. Any failed check fails the job, so no capture is handed off unless the Windows side did what its timeline says. |
| Hand-off bundle | `handoff/` | Rules (`README.md`), task card with features in user terms and the exact command-line contract for a Python/pyusb driver (`TASK.md`), `lsusb -v` (`descriptors.txt`), captures (`captures/`: pcapng, text, timeline `.md` and `.json`, `index.md`). No source, no protocol description, no internal names, no reference to anything outside the directory. |
| Scoring suite | `sealed/score.py` | Runs a candidate against a freshly started device with a firmware version, serial number and device-chosen values that appear in no capture, and with sensor values, levels, texts, pictures and button timings checked against every number in the committed timelines so none is reused. Fifteen features plus three whole-run checks (18). Each feature is judged from the device's side (what it shows, stores and counted), not only from the candidate's output. The candidate runs as an unprivileged user with access to the device node only: not to the checkout, not to the world socket. |
| Reference and wrong drivers | `sealed/reference/`, `sealed/wrong/{replay,quirks}/` | The reference Linux driver (pyusb), written from the specification by this task's (non-blind) author. "replay": what a guess from skimming captures would do. "quirks": correct except for some details the captures show only indirectly. |
| Workflow | `.github/workflows/exp-drvsynth.yml` | `capture` (build, Wine, sessions, captures; skipped when a pull request changes only `candidate/`) and `score` (reference, wrong drivers, `validate`, candidate if present); pull-request path filter on the experiment and the workflow, `workflow_dispatch`, cancel-in-progress concurrency. The KMDF runtime and Wine overlay are cached with `actions/cache/restore` and `save`, saved as soon as they are built. |
| Experiment README | `tests/experiments/drvsynth/README.md` | Separation, how a blind trial is run (the candidate goes in `candidate/`, the workflow scores it), what counts as success. |

No Microsoft binary was downloaded or used; the only kernel modules loaded
were the stock `vhci-hcd` (with `usbip-core`) and `usbmon`. Nothing ran in
a kernel.

## 2. Separation

- Everything that reveals the protocol is under `sealed/`: the
  specification, the emulator, the Windows driver and program, the
  reference and wrong drivers, and the scorer (whose check names would hint
  at what is tested).
- `bench/` holds no protocol knowledge: the session harness speaks to the
  device's world socket in user terms (degrees, button numbers, pictures as
  rows of hex digits, note texts); the emulator does the encoding.
- The hand-off bundle is only what a user or a USB analyser would see. The
  Windows program's output format is the contract, which a user would see
  on screen.
- CI output is split: artifact `drvsynth-handoff` is exactly the bundle;
  everything else is in artifacts named `drvsynth-sealed-*`. The rules in
  `handoff/README.md` forbid the blind engineer every file outside
  `handoff/`, CI logs and summaries of any workflow, this report and the
  pull request.
- The device, vendor and product names were invented for the experiment.

## 3. Captures

From run 43536094203. The committed bundle is from run 43540502703: same
sessions, fresh device-chosen values; 1,180 USB events (session 07 has 118 rather
than 120, a matter of timing), 99,848 bytes of pcapng and 94,581 bytes
of text; per-session figures in `handoff/captures/index.md`.

| Session | Title | User actions | USB events | pcapng bytes |
|---|---|---|---|---|
| 01 | First use (includes the driver loading) | 8 | 120 | 10,168 |
| 02 | Brightness | 8 | 112 | 9,392 |
| 03 | Night mode | 10 | 140 | 11,712 |
| 04 | Sensors through the day | 10 | 100 | 8,432 |
| 05 | Notes | 11 | 152 | 12,960 |
| 06 | Pictures | 6 | 100 | 8,736 |
| 07 | Alert and buttons | 5 | 120 | 10,112 |
| 08 | Clearing the notes | 5 | 70 | 5,944 |
| 09 | Evening | 8 | 108 | 9,072 |
| 10 | Frost alert and a last picture | 8 | 160 | 13,488 |
| | **Total** | **79** | **1,182** | **100,016** |

The readable text forms total 94,719 bytes. Values vary across sessions
(brightness 0 to 100 in uneven steps, temperatures from -12.34 to 30.42 °C,
light from 0 to 131,072 lux, short and long notes, six pictures, three
button presses of different lengths, alerts above and below zero).
Every action took one run of the Windows program, so every action's
traffic includes its own start-up exchange: 79 samples of it.

Capture-job checks (run 43536094203), 145 of 145 PASS: the Wine overlay;
for each action the program's exit status and output, the device's
resulting state where the action changes it, and the picture the device
shows; every button press delivered while the program listened; each
capture non-empty and read back by tshark with the same frame count; the
driver's `DriverEntry`, `EvtDevicePrepareHardware`, `EvtDeviceD0Entry` and
continuous-reader set-up in the driver host's log; no crash or
unimplemented call in the driver host.

## 4. Scoring suite: proof that it works

Run 43536094203, job score, against a device with firmware 6.4.10 and
fresh values:

| Feature | reference | wrong-replay | wrong-quirks |
|---|---|---|---|
| info | PASS | FAIL | PASS |
| temperature | PASS | FAIL | FAIL |
| humidity | PASS | FAIL | PASS |
| light | PASS | FAIL | PASS |
| sensors | PASS | FAIL | FAIL |
| brightness | PASS | FAIL | PASS |
| modes | PASS | FAIL | FAIL |
| night-level | PASS | FAIL | FAIL |
| alert | PASS | FAIL | FAIL |
| image | PASS | FAIL | PASS |
| notes-write | PASS | FAIL | PASS |
| notes-read | PASS | FAIL | PASS |
| notes-list-delete | PASS | FAIL | PASS |
| clear-notes | PASS | FAIL | FAIL |
| events | PASS | FAIL | FAIL |
| (whole run) quirk respected | PASS | FAIL | FAIL |
| (whole run) checksums | PASS | FAIL | PASS |
| (whole run) start-up exchange | PASS | FAIL | PASS |
| **Total** | **18 / 18** | **0 / 18** | **10 / 18** |

`score.py validate`: suite valid (the reference passes everything; the
replaying driver fails everything, starting with 38 of 38 refused
start-ups; the almost-right driver fails each feature its mistakes touch
and passes each it does not). The almost-right driver's `modes` result is
timing-dependent, so `validate` does not require it either way.

## 5. What counts as success (stage B)

As in `tests/experiments/drvsynth/README.md`: the hypothesis is supported
by a trial whose candidate passes all 18; anything less is reported as the
number and list of features passed. One trial is one data point.

## 6. Limits

- **An emulated device, one invented protocol.** Its difficulty was chosen
  to be realistic for a small vendor gadget; a real device may be harder
  (larger protocols, firmware state, timing), or easier. Results of stage B
  generalise only as far as the protocol resembles real ones.
- **The captures are Linux usbmon recordings underneath Wine's driver host**,
  not a Windows USB analyser trace (USBPcap). Byte-for-byte, the transfers
  are the driver's; what differs is framing (URBs as Linux sees them) and
  that Wine's own enumeration appears in session 01.
- **The bench author is not blind.** The reference driver proves the suite
  can be passed, not that the bundle is sufficient: whether the captures
  contain enough to infer everything is what stage B measures. Every
  scored behaviour does appear in the captures at least once.
- **Isolation of the candidate** is by user and file permissions on a
  throw-away runner; the candidate's code should still be read before it is
  scored, since a malicious candidate could try other channels.
- **Timing-based checks** (the event watch) depend on the runner; the
  reference passed 18/18 in both runs so far (43536094203, 43540502703).
- The capture job's KMDF runtime build took under two minutes on the
  runner that built it (the windows-drivers report measured about 25); it
  is cached either way.

<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-drvsynth-1 — driver synthesis from USB captures: the test bench (stage A) and the first blind trial (stage B)

> **Blind engineers of any later trial on this device: do not read this
> report.** Use only `tests/experiments/drvsynth/handoff/`. Since stage B is
> done, sections 6 and 8 name some elements of the protocol, as far as an
> honest account of the result needs them; the full specification stays in
> `tests/experiments/drvsynth/sealed/PROTOCOL.md`.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-drvsynth-1 (experiment drvsynth, stages A and B)
Subsystem:        Windows compatibility (Wine's driver host), as a test
                  instrument; no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  branch exp-drvsynth, pull request 122. Bench runs
                  43536094203 (4f14f1c) and 43540502703 (fb1b93f; the
                  committed hand-off captures come from it). Blind trial:
                  candidate committed in d3926cb, scored in run 43549055344,
                  job 132307030849.

SUMMARY
- What changed: a test bench for the hypothesis "given only a recording of
  the USB traffic between a device and its Windows driver, an AI can write
  a working native Linux driver for it" (an invented USB device emulated in
  user space, its Windows KMDF driver and program run in Wine's driver
  host, usbmon captures of scripted sessions with user-action timelines, a
  hand-off bundle, a hidden scoring suite proven against a reference driver
  and two wrong ones), and the first blind trial: a fresh agent that saw
  only the bundle wrote a Linux driver in about 18 minutes that passed 18
  of 18 scored features on its first and only scoring.
- Why it changed: the owner wants to know whether TUR could learn a device
  once from its Windows driver (which windows-drivers-2 showed TUR can run
  in user space) and then support it natively everywhere.
- What it shows, and what it does not: one AI-invented protocol of small-
  gadget difficulty was recovered completely from 78 recorded program runs.
  That supports the hypothesis for devices like this one. It is one device
  and one trial, designed and solved by models of the same family
  (section 8).

FILES
- Added: tests/experiments/drvsynth/{README.md, handoff/**, bench/**,
  sealed/**}, .github/workflows/exp-drvsynth.yml,
  docs/reports/exp-drvsynth-1.md; candidate/** (the blind engineer's
  driver and notebook, committed unchanged by the lead in d3926cb)
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
- Integration tests: workflow exp-drvsynth.
  - Run 43536094203: capture 145 of 145 checks PASS; score: suite valid
    (reference 18/18, wrong-replay 0/18, wrong-quirks 10/18 as designed).
  - Run 43540502703: the same, on the commit whose captures were handed
    off.
  - Run 43549055344, job 132307030849: candidate 18/18 PASS; in the same
    job, validate: reference 18/18, wrong-replay 0/18, wrong-quirks 10/18,
    all as expected.
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. The device is emulated (USB/IP); it does not
  exist as hardware.
- Performance tests: none. The blind engineer's wall time (about 18
  minutes) is the lead's measurement.

DIAGNOSTICS
- Log paths: artifacts of each run: drvsynth-handoff (the bundle's
  captures, timelines, descriptors), drvsynth-sealed-capture-evidence
  (build logs, Wine logs per action, results.json/md, the emulator's log),
  drvsynth-sealed-score (score-*.json/md, validation.md). The sealed
  artifacts reveal the protocol. The trial's notebook:
  tests/experiments/drvsynth/candidate/NOTES.md; the audit:
  tests/experiments/drvsynth/candidate/README.md.
- Diagnostic bundle path: none
- Relevant error IDs: none

RECOVERY
- Rollback procedure: revert the merge; nothing outside tests/experiments/
  drvsynth, the new workflow and this report changes.
- Recovery tested: not applicable

KNOWN LIMITATIONS
- Section 8. In short: one emulated device, one trial; the protocol was
  invented and solved by models of one family; its building blocks are
  familiar and mostly linear; the captures came from a clean driver
  written by the protocol's designer.

NEXT SAFE ACTION
- The owner decides on stage C (section 9) and puts the question in
  section 10.3 to a lawyer before anything is built towards a product.
```

## The answer in short

- **Stage A, the bench, works.** The invented device's Windows KMDF driver
  ran in Wine's user-space driver host and did everything its program
  asked (145 of 145 checks). Ten usbmon captures (78 program runs, 1,180
  USB events, 100 kB of pcapng) with user-action timelines make the
  hand-off bundle. The hidden scoring suite passes the reference driver
  (18/18) and fails a replaying driver (0/18) and an almost-right one
  (10/18) exactly where they are wrong.
- **Stage B, the first blind trial, passed: 18 of 18.** A fresh agent
  (Claude Fable 5.2) given only the bundle wrote an 893-line Python/pyusb
  driver in about 18 minutes and 40 tool calls, without ever touching the
  device. On a device whose firmware version, serial number and
  device-chosen values it had never seen, it passed every feature,
  including the handshake that depends on a fresh value, checksums over
  unseen payloads, the device's pause after a mode change and its
  confirm-by-repeating command.
- **What that is worth.** For a small vendor gadget whose protocol is
  built from simple, mostly linear and well-known parts, captures plus a
  timeline of user actions were enough. It does not show that protocols
  designed to resist analysis, or messy real drivers, yield the same way:
  the protocol and the solver came from models of the same family, and
  the handshake was solvable by linear algebra. Section 9 proposes the
  trial that would test that.

## 1. What was built

| Part | Where | What |
|---|---|---|
| The device | `sealed/emulator.py` | An invented vendor-class USB 2.0 device ("Quillfeather Labs Glimmerdesk G2", `1209:000c`, a pid.codes test ID): a desk companion with a sensor block, a 16 x 8 LED matrix with grey levels, a note store, two buttons, day/night modes and a temperature alert. Control, bulk and interrupt endpoints. Emulated in user space and plugged into the runner kernel's USB/IP virtual host controller with the windows-drivers experiment's attach code (`tests/windows-drivers/virtual-usb/usbip_devices.py`), so the host sees an ordinary USB device. A Unix socket readable only by root drives the "physical world" (temperature, humidity, light, button presses) and reports what the device shows and stores, in user terms. |
| Its protocol | `sealed/PROTOCOL.md` | Written for this experiment; exists nowhere else. It has every element the task asked for: a start-up exchange that depends on a value the device chooses each time, length-prefixed frames with a checksum that is not a stock CRC, transfers spread over several packets in both directions, modes that change what commands mean, asynchronous interrupt reports, and more than one quirk of the kind real devices have. |
| Its Windows driver | `sealed/windows/qfdesk.c` | A KMDF 1.33 driver on TUR's KMDF runtime (built from Microsoft's MIT source) with TUR's stub, loaded by Wine's `winedevice.exe` in user space. The protocol lives in the driver; a continuous reader on the interrupt pipe keeps event reports. |
| Its Windows program | `sealed/windows/gadget.c` | `gadget.exe COMMAND`: every function of the device through the driver. Its commands and output are exactly the command-line contract the Linux driver must follow, so the timelines show what a correct Linux driver prints. |
| Capture | `bench/usbmon.py` | Reads the kernel's usbmon binary interface (`/dev/usbmonN`) and writes each event unchanged as a pcapng Enhanced Packet Block (link type USB Linux, 189); writes a readable text form from the pcapng. Every capture is cross-checked with tshark 4.6.9: same frame count. |
| Sessions | `bench/sessions.py` | Ten scripted sessions; each is one usbmon capture during which `gadget.exe` runs once per user action (78 runs; the index's 79 "actions" include the driver loading in session 01), as an unprivileged user, while the world is changed between and during actions. Writes a timeline per session in user terms (what the user did, what the program printed, what happened in the room) on the capture's clock, and checks every action's output and the device's resulting state. Any failed check fails the job, so no capture is handed off unless the Windows side did what its timeline says. |
| Hand-off bundle | `handoff/` | Rules (`README.md`), task card with features in user terms and the exact command-line contract for a Python/pyusb driver (`TASK.md`), `lsusb -v` (`descriptors.txt`), captures (`captures/`: pcapng, text, timeline `.md` and `.json`, `index.md`). No source, no protocol description, no reference to anything outside the directory. |
| Scoring suite | `sealed/score.py` | Runs a candidate against a freshly started device with a firmware version, serial number and device-chosen values that appear in no capture, and with sensor values, levels, texts, pictures and button timings checked against every number in the committed timelines so none is reused. Fifteen features plus three whole-run checks (18). Each feature is judged from the device's side (what it shows, stores and counted), not only from the candidate's output. The candidate runs as an unprivileged user with access to the device node only: not to the checkout, not to the world socket. |
| Reference and wrong drivers | `sealed/reference/`, `sealed/wrong/{replay,quirks}/` | The reference Linux driver (pyusb), written from the specification by the bench's (non-blind) author. "replay": what a guess from skimming captures would do. "quirks": correct except for some details the captures show only indirectly. |
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
  One internal name did reach the bundle: each pcapng's header records the
  capture tool as "TUR drvsynth test bench (usbmon)". It names the
  experiment, not the protocol; the blind engineer had no way to follow it
  (section 5). A later bundle should carry a neutral name there.

## 3. Captures

The committed bundle is from run 43540502703 (the first run, 43536094203,
gave the same sessions with other device-chosen values): 1,180 USB events,
99,848 bytes of pcapng and 94,581 bytes of text. Per session (from
`handoff/captures/index.md`):

| Session | Title | Program runs | USB events | pcapng bytes |
|---|---|---|---|---|
| 01 | First use (also the driver loading) | 7 | 120 | 10,168 |
| 02 | Brightness | 8 | 112 | 9,392 |
| 03 | Night mode | 10 | 140 | 11,712 |
| 04 | Sensors through the day | 10 | 100 | 8,432 |
| 05 | Notes | 11 | 152 | 12,960 |
| 06 | Pictures | 6 | 100 | 8,736 |
| 07 | Alert and buttons | 5 | 118 | 9,944 |
| 08 | Clearing the notes | 5 | 70 | 5,944 |
| 09 | Evening | 8 | 108 | 9,072 |
| 10 | Frost alert and a last picture | 8 | 160 | 13,488 |
| | **Total** | **78** | **1,180** | **99,848** |

Values vary across sessions (brightness 0 to 100 in uneven steps,
temperatures from -12.34 to 30.42 °C, light from 0 to 131,072 lux, short
and long notes, six pictures, four button presses of different lengths,
alerts above and below zero). Every action took one run of the Windows
program, so every action's traffic includes its own start-up exchange: 78
samples of it.

Capture-job checks (run 43536094203; the same in 43540502703), 145 of 145
PASS: the Wine overlay; for each action the program's exit status and
output, the device's resulting state where the action changes it, and the
picture the device shows; every button press delivered while the program
listened; each capture non-empty and read back by tshark with the same
frame count; the driver's `DriverEntry`, `EvtDevicePrepareHardware`,
`EvtDeviceD0Entry` and continuous-reader set-up in the driver host's log;
no crash or unimplemented call in the driver host.

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
| (whole run) mode-change delay respected | PASS | FAIL | FAIL |
| (whole run) checksums | PASS | FAIL | PASS |
| (whole run) handshake | PASS | FAIL | PASS |
| **Total** | **18 / 18** | **0 / 18** | **10 / 18** |

`score.py validate`: suite valid. The reference passes everything; the
replaying driver fails everything, starting with 38 of 38 refused
handshakes; the almost-right driver (no pause after a mode change,
confirm-by-repeating sent once, temperature byte order wrong, heartbeat
reports printed as events) fails each feature its mistakes touch and
passes each it does not. Its `modes` result is timing-dependent, so
`validate` does not require it either way. The same results came out of
runs 43540502703 and 43549055344.

## 5. Stage B: the blind trial and its audit

Run by the lead on 14 November 2026, after this bench was green
(`tests/experiments/drvsynth/candidate/README.md` is the lead's record):

1. **The engineer**: a fresh Claude Fable 5.2 agent with no context from
   this work.
2. **Its inputs**: a directory outside the repository holding only a copy
   of `handoff/` (README, TASK, descriptors, captures; checked before the
   trial for protocol names: none, except the capture-tool name in the
   pcapng headers noted in section 2). Instructions: read and write
   nowhere else, no network, no other agent, submit once.
3. **Its work**: about 18 minutes and 40 tool calls. It wrote its own
   analysis scripts in that directory (checksum and handshake solvers, a
   pcapng reader to cross-check the text form, a stateful model of the
   device, a replayer), its driver `gadget.py` (893 lines) and a lab
   notebook `NOTES.md` (88 lines) that cites capture times for every
   conclusion and lists what it could not determine and how the driver
   hedges.
4. **Audit**: the lead checked all 40 tool calls. Every file read, written
   or executed was inside the trial directory; no network or agent tool was
   used. The engineer itself disclosed one failed `open()` of a
   deliberately non-existent path, made while testing its own error
   handling.
5. **Submission**: committed unchanged (SHA-256 recorded in the candidate's
   README) as `tests/experiments/drvsynth/candidate/` in d3926cb, scored
   once, in run 43549055344. The engineer never ran its driver against the
   device.

**Its own claims before submitting** (from `NOTES.md`), which it could test
only offline, against the recordings:

| Claim | Its evidence |
|---|---|
| Replaying its driver against every recorded program run, with a fake transport that insists on byte-identical host transfers in the recorded order and returns the recorded answers, reproduces every run | 78 of 78 runs, output and exit status identical; 553 of 553 host transfers (78 challenge requests, 78 handshake answers, 78 closes, 157 command frames, 162 bulk-in requests) |
| The checksum model fits every frame in both directions | 319 of 319 frames (112 distinct) |
| The handshake model fits every recorded pair | 78 of 78 |
| An independent stateful model of the device predicts every reply | 162 of 162 reply frames |
| Its driver against that model, for things never recorded | all pass, with the engineer's own caveat: this shows the driver is self-consistent, not that the device behaves so |

## 6. Stage B result

Run 43549055344, job 132307030849, against a freshly started device
(firmware 8.8.22, a new serial number, fresh nonces and values):

| Feature | Candidate |
|---|---|
| info | PASS |
| temperature | PASS |
| humidity | PASS |
| light | PASS |
| sensors | PASS |
| brightness | PASS |
| modes | PASS |
| night-level | PASS |
| alert | PASS |
| image | PASS |
| notes-write | PASS |
| notes-read | PASS |
| notes-list-delete | PASS |
| clear-notes | PASS |
| events | PASS |
| (whole run) mode-change delay respected | PASS |
| (whole run) checksums | PASS |
| (whole run) handshake | PASS |
| **Total** | **18 / 18** |

The candidate's 38 runs took 13.8 s in all (the reference's took 13.5 s;
the longest run in both is the 8-second event watch). The validation in
the same job (reference 18/18, wrong-replay 0/18, wrong-quirks 10/18, as
expected) shows the suite was discriminating on that run.

How it got there, from its notebook, element by element (now that the
trial is over):

| Element | How the blind engineer recovered it |
|---|---|
| Session bracket (challenge, answer, commands, close) | Same shape in all 78 runs |
| Handshake answer from the device's nonce | Treated the 78 nonce/answer pairs as bits, found the answer is an affine function over GF(2) of the nonce (rank 33, all 78 consistent), and solved it: rotate the little-endian nonce left by 5 and XOR a constant, sent across wValue/wIndex |
| Frame layout and sequence numbers | Direct reading of 319 frames |
| Checksum | A Fletcher-like sum; the modulus 251 found from how the residue drifted at each wrap; the seed fitted; unique fit over 319 frames |
| Byte orders (one big-endian field among little-endian ones) | From the timeline's printed values: -1.25 °C was `ff 83`, 70,210 lux was `42 12 01 00` |
| Mode-dependent meaning of one command pair | From which mode the program switched to before each setting; confirmed by its stateful model |
| The pause after a mode change | Timing alone: all 32 recorded mode changes were followed by 250.6–251.0 ms of silence, every other gap under 0.5 ms; it chose 260 ms |
| Confirm-by-repeating clear | From session 08: the first answer differs, the program sends it again |
| Multi-frame note reads and writes | From session 05's 96-byte note (40 + 40 + 16) |
| Event reports, including a once-a-second report that the program never prints | From sessions 07, 09, 10 and the timelines' printed events |

## 7. What the trial measured

- **Supported, for this class of device**: given a protocol of the
  difficulty of a small vendor gadget, 78 recorded program runs and a
  timeline of what the user did and saw, an AI wrote a driver that works on
  a unit it had never seen, on first contact, with no device to test
  against, in under 20 minutes. Every one of the "checks a guess can't
  pass" passed.
- **The timelines mattered.** Mapping bytes to meanings (which bytes are
  degrees, which nibble is which pixel, which status means "armed") came
  from the printed outputs paired with the traffic. Captures without them
  would have needed far more guesswork. A product would have to collect
  the same pairing (section 10.1).
- **The engineer's caution mattered.** It separated what it was certain
  of from what it was not, and hedged the unknowns in the driver (section
  "Not determined" of its notebook): for example it read a field as 16 bits
  where only 8 had ever been used, and drained stale event reports before
  listening. The suite exercised few of these hedges; a less careful
  driver that got the recorded cases right might have scored the same.

## 8. Limits, candidly

I designed the protocol, the device, the Windows driver and the scoring
suite. These are the reasons not to read more into 18/18 than it says.

- **One device, one trial.** A single success is one data point. It says
  this protocol was recoverable by this model on this day, not how often
  it would be, and nothing about other protocols.
- **Invented by an AI, solved by an AI of the same family.** I am a Claude
  model; the blind engineer was a Claude model. When I invented "a
  realistic protocol" I drew on the same priors it would use to analyse
  one: a magic byte and a length, a Fletcher-like checksum, rotate-and-XOR,
  confirm-by-repeating, a fixed post-command delay. A protocol from someone
  else's mind could contain things neither of us would think of first.
- **The building blocks are familiar, and the handshake is linear.** The
  XOR key `0x6D2B79F5` is the well-known constant of the mulberry32
  pseudo-random generator; I reached for it, and the blind engineer
  recognised it.
  - *How much the recognition helped:* not in finding the formula. Rotate
    and XOR are linear over GF(2), so the answer is an affine function of
    the nonce, and 78 pairs pin it down by Gaussian elimination whatever
    the constant is; the engineer solved it that way. Where recognition
    did help was in the decision it then had to make blind: whether the
    constant was fixed for every unit or derived from something of this
    unit (its serial number, its firmware), which one device cannot
    reveal. Knowing the constant as a generic one, and checking that
    simple hashes of the serial and product strings did not produce it,
    let it commit to "fixed". The scoring device had a different serial
    number, so had the key been per-unit, the driver would have failed
    every feature. The recognition turned an unknowable choice into a
    confident one; it was right because I had made the constant fixed.
  - *What a harder variant would use:* a non-linear keyed function, for
    example HMAC-SHA-256 or AES over the nonce, with a key that is per
    unit (derived from the serial number with a secret the Windows driver
    holds). From captures alone that cannot be recovered at all, however
    many there are; it would have to come from the Windows driver's
    binary, which is what TUR actually has in a real case and which this
    trial did not hand over. The Fletcher-like checksum is in the same
    position: a keyed MAC instead would make every frame unforgeable
    without the key.
- **An emulated device, not hardware.** USB/IP and a Python device: no
  electrical timing, no lost packets, no firmware state machine with its
  own bugs, no power management. The pause after a mode change was exactly
  200 ms every time; real devices vary.
- **The captures came from a driver I also wrote.** It was clean and
  regular: no retries, no telemetry, no unrelated traffic, one command at
  a time, the same 250 ms pause after every mode change. That regularity
  is what made the pause visible (250.6–251.0 ms against gaps under
  0.5 ms elsewhere). A vendor driver is messier: polling, retries,
  vendor-specific initialisation, firmware checks, several programs using
  the device at once. The timelines were also perfect, written by the
  harness that drove the actions.
- **The scoring covers only what was recorded.** Every scored behaviour
  appears in the captures at least once; a device with features its owner
  never used in the recordings would leave gaps the suite does not probe.
- **The candidate is in Python and user space,** following the contract.
  That is how TUR would run such a driver (a user-space program with a
  one-device grant, as ADR-0026 proposes); it is not a kernel driver.

## 9. Proposal: stage C, a harder and fairer trial

Goal: find where synthesis from captures stops working, and whether adding
the Windows driver's binary carries it further.

1. **Protocols nobody on the solving side designed.** Either (a) written
   by a human engineer, or by a different model vendor's model, who never
   talks to the solver's side; or (b) derived from the firmware of a real
   open-hardware USB device (vendor-class, with a protocol of its own),
   with names, constants and layout changed so that it cannot be
   recognised from training data. Option (b) gives a protocol shaped by a
   real product's history.
2. **Several devices, graded.** Four or five, from the gadget level of this
   trial up to: deeper state machines (sessions, pagination, flow
   control), timing that varies and must be inferred from the device's own
   replies, retries and error recovery visible in the captures,
   compression of bulk data, and encryption with a per-unit key held by
   the Windows driver. Each with the same kind of hidden scoring suite.
3. **Two conditions per device.** Captures and timelines only (as here),
   and captures plus the Windows driver's binary (the realistic case).
   The difference measures what static analysis of the driver adds, and
   the encrypted device can only be solved in the second condition.
4. **Messier recordings.** Captures taken under a vendor-style driver that
   polls, retries and runs at the same time as other programs, and
   timelines written by a person (with their imprecisions), not by the
   harness.
5. **Repeats.** At least three independent blind runs per device and
   condition, and more than one model, recording wall time, tool calls and
   cost. Report the pass rate, not the best run.
6. **Real hardware later.** When TUR has a hardware runner with USB, one
   physical device (for example a microcontroller board running the
   stage-C firmware) attached to it: real timing and real USB, with the
   emulated device kept for CI.

The bench here is reusable: the emulator class, the capture tool, the
session harness, the world socket and the scoring structure take a new
device as data and code under `sealed/`.

## 10. What productising "learn a device from its Windows driver" would take

### 10.1 The pipeline

1. **Run the vendor's driver once, in TUR**: in Wine's user-space driver
   host where it can run (WDM and KMDF USB drivers, per windows-drivers-1
   and -2), otherwise in the Windows guest with the USB device handed to
   it (ADR-0015). Both are already designed; both need the device grant of
   ADR-0026.
2. **Record, with consent**: usbmon underneath the driver host or the
   guest's USB passthrough. The person must agree, see what is recorded,
   and be told that recordings may contain personal data (a scanner's
   pages, a device's stored notes).
3. **Label the recording**: the trial depended on the timeline. P@x would
   guide the person through the device's functions in its vendor program
   ("set the brightness to 40 now"; "press the first button") and record
   what the program showed, which turns a recording into evidence.
4. **Synthesise**: the model, behind `tur-pax`'s provider interface (no
   other component talks to a model), with the recordings, the timeline,
   the USB descriptors and, where allowed, the driver binary.
5. **Validate before use**: replay against the recordings (the blind
   engineer's own method: every host transfer reproduced), then a live
   test on the person's device, one function at a time, with the person
   confirming each result. Commands that could damage the device
   (firmware writes, calibration, anything the recordings show only once
   behind a confirmation) are never sent by a synthesised driver unless
   the person, in a dialog P@x cannot operate, allows it.
6. **Run it safely**: as a user-space program in the runtime sandbox with
   a grant for that one device (usbfs or hidraw); never in a kernel.
7. **Share it**: a catalogue entry per device (VID:PID, firmware versions
   seen, the recordings' hashes, the validation record, the model and its
   version), signed and reviewed before other TUR computers receive it.
   "Every TUR computer supports it" needs that review step; an unreviewed
   driver stays local.

### 10.2 What it will not cover

Devices that Linux already drives (most of them, and those should stay with
the kernel); protocols protected by keys that neither recordings nor the
driver binary reveal (secure elements, DRM); isochronous high-bandwidth
devices (cameras, audio interfaces) where the data path, not the control
protocol, is the work; devices whose firmware the vendor driver uploads at
start (the firmware image is the vendor's, and redistributing it is a
licence question of its own).

### 10.3 The legal question, for the owner's lawyer

This report gives no legal advice. The question to put to a lawyer, for
each market TUR would ship in:

> TUR would let a person observe the USB traffic between their own device
> and the vendor's Windows driver running on their own computer, and
> optionally analyse that driver's binary, so that an AI can write an
> independent driver that makes the device work on TUR. The generated
> driver contains no vendor code. Is this permitted reverse engineering
> for interoperability? In particular: (1) in the EU, is observing the
> traffic covered by the right to observe, study or test a program's
> functioning (Directive 2009/24/EC, Article 5(3)), and is analysing the
> driver binary covered by the decompilation-for-interoperability
> exception (Article 6) and its conditions, given that Article 8 makes
> contrary contract terms void; (2) in the United States, the
> interoperability exemption of 17 U.S.C. §1201(f) where any technical
> protection is involved, and the effect of licence terms that forbid
> reverse engineering; (3) whether TUR, or its users, may distribute the
> resulting driver to other TUR users through the catalogue, and whether a
> protocol can be protected by patents or trade secrets in a way that
> would stop that; (4) whether the person or TUR is the one "reverse
> engineering" when a model does the analysis, and whether that changes
> any of the above.

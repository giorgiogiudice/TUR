<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Hand-off bundle: write a Linux driver for the Glimmerdesk G2

You have been given a USB desk gadget, the **Quillfeather Labs Glimmerdesk
G2**, and recordings of its USB traffic while its owner used it with the
vendor's Windows driver and program. There is no Linux driver and no
documentation. Your job is to write one: a command-line program for Linux
that does everything the owner did, following the contract in
[`TASK.md`](TASK.md) exactly, so that it can be scored automatically.

## Rules

1. **Use only this directory** (`tests/experiments/drvsynth/handoff/`).
   Do not open, search, list or ask about anything else in this repository
   or its history: not the rest of `tests/experiments/drvsynth/`, not other
   experiments, not reports, pull requests, issues or CI logs, artifacts and
   summaries of any workflow. If you see something from outside this
   directory by accident, stop and say so: the trial is void.
2. No help from anyone who has seen the rest of the experiment.
3. Do not search the web for this device or its vendor; it is not on the
   web. General knowledge (USB, Linux, libusb, pyusb, Python, reverse
   engineering techniques) is allowed and expected.
4. You cannot try your driver on the device. You only have what is here.
5. Put your driver in `tests/experiments/drvsynth/candidate/gadget.py`
   (more files in that directory are fine; standard library and pyusb
   only). Say when it is final. It is scored then, once.

## What is here

| File | What it is |
|---|---|
| `TASK.md` | The features your driver must provide, in user terms, and the exact command-line contract |
| `descriptors.txt` | `lsusb -v` of the device, as a Linux computer sees it |
| `captures/index.md` | The recorded sessions |
| `captures/session-NN.pcapng` | Every USB transfer of the device during session NN, recorded on Linux with usbmon underneath the Windows driver (which ran in a compatibility layer). Opens in Wireshark or tshark (link type USB Linux) |
| `captures/session-NN.txt` | The same capture as text, one line per event; the legend is at the top of each file |
| `captures/session-NN.timeline.md`, `.json` | What the owner did during session NN, when, and what the Windows program printed, on the capture's clock |

Notes on the recordings:

- Every action in a timeline is one run of the vendor's Windows program,
  which printed exactly what the timeline shows under "saw". The program's
  output format is the one `TASK.md` asks you to reproduce.
- Times are seconds since the capture started. A timeline entry's interval
  includes starting and stopping the Windows program; the USB traffic of the
  action is somewhere inside it. The gaps between actions are idle.
- Things that happened in the room during an action (a button pressed, the
  room warming up) are listed with a single time.
- Session 01 starts before the Windows driver was loaded, so it also shows
  what the driver does when the gadget is first used.
- Values differ between sessions on purpose. The scoring will use values
  that appear in no recording, on a gadget whose serial number and firmware
  version also differ from the recorded one.

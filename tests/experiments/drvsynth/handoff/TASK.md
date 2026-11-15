<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Task card: a Linux driver for the Glimmerdesk G2

## The gadget, as its owner knows it

A desk companion that plugs into USB. It has:

- a **sensor block**: room temperature (°C, can go below zero), relative
  humidity (%), and light level (lux; from 0 in the dark to very large
  numbers in direct sunlight);
- a **16 x 8 LED matrix**; each pixel has 16 brightness levels (0 = off,
  15 = brightest), and the whole display has a brightness setting;
- **day and night modes**: in night mode the gadget runs a dim night-light
  whose level is set separately from the daytime display brightness;
- a **temperature alert**: the owner picks a temperature, and the gadget
  tells the computer when the room warms up past it;
- **two buttons**; the computer is told which button was pressed and for
  how long it was held;
- a **note store**: eight slots (0 to 7), each holding one short text of 1
  to 120 bytes;
- a firmware version and an uptime counter (seconds since it was powered).

## What your driver must do (the features that are scored)

| Feature | User terms |
|---|---|
| info | show the firmware version and the uptime |
| temperature | read the room temperature |
| humidity | read the humidity |
| light | read the light level |
| sensors | read all three at once |
| brightness | set and read the daytime display brightness |
| modes | switch between day and night mode, and tell which mode the gadget is in |
| night-level | set and read the night-light level, whichever mode the gadget is in; setting the daytime brightness while in night mode must work too, and the gadget must stay in the mode it was in |
| alert | set the alert temperature, switch the alert off, read the setting, whichever mode the gadget is in (and leave it in that mode) |
| image | show a picture on the matrix; clear the display |
| notes-write | store a note in a slot, short or long |
| notes-read | read a note back; tell an empty slot |
| notes-list-delete | list the stored notes; delete one |
| clear-notes | delete all notes at once |
| events | watch the gadget for a while and report button presses and alerts as they happen |

Every feature is checked against what the gadget itself then does, shows
and stores, with values, texts and pictures that appear in no recording. A
driver that upsets the gadget, works only sometimes, or prints anything
else fails. Success for the experiment is every feature passing.

## The command-line contract

Your driver is `tests/experiments/drvsynth/candidate/gadget.py`, run as

```text
python3 gadget.py COMMAND [ARGUMENTS]
```

- Python 3.12 with the standard library and **pyusb 1.2.1** (Ubuntu 24.04's
  `python3-usb`) over libusb-1.0. Nothing else is installed.
- Run as an ordinary user who has read-write access to the gadget's USB
  device node. Not root; no network; no other files than your directory.
- Exactly one Glimmerdesk is connected (USB ID `1209:000c`). Find it
  yourself.
- Every command is a new process; nothing is kept between runs except what
  the gadget itself remembers.
- Print exactly the lines below to standard output, each ending in a
  newline, and nothing else. Standard error is free for messages.
- Exit status 0 on success, 4 for an empty note slot (see `read-note`),
  anything else non-zero on failure.
- Each command must finish within 15 seconds (`wait-event`: within its
  SECONDS plus 5).

| Command | Arguments | Prints (example) |
|---|---|---|
| `info` | | two lines: `firmware 3.1.4` and `uptime 4302` (the version as three numbers with dots; uptime in whole seconds) |
| `read-temperature` | | `21.37` (°C, two decimals; e.g. `-0.05`) |
| `read-humidity` | | `45.2` (%, one decimal) |
| `read-light` | | `312` (lux, integer) |
| `read-sensors` | | three lines: `temperature 21.37`, `humidity 45.2`, `light 312` |
| `get-mode` | | `day` or `night` |
| `set-mode` | `day` or `night` | `mode night` |
| `set-brightness` | 0 to 100 | `brightness 40` |
| `get-brightness` | | `40` |
| `set-night-level` | 0 to 100 | `night-level 10` |
| `get-night-level` | | `10` |
| `set-alert` | a temperature from -40.0 to 125.0 with at most one decimal, or `off` | `alert 26.5` (one decimal), or `alert off` |
| `get-alert` | | `26.5`, or `off` |
| `show-image` | a file (below) | `image shown` |
| `clear-display` | | `display cleared` |
| `list-notes` | | one line per stored note, slots in increasing order: `SLOT LENGTH` (length in bytes), e.g. `0 5` and `3 17`; nothing if there are none |
| `write-note` | slot 0 to 7, then the text (one argument, 1 to 120 bytes of printable ASCII) | `note 3 stored` |
| `read-note` | slot 0 to 7 | the text; for an empty slot: `empty`, exit status 4 |
| `delete-note` | slot 0 to 7 | `note 3 deleted` |
| `clear-notes` | | `notes cleared` |
| `wait-event` | SECONDS (1 to 600) | as they happen, one line per event: `button 2 1.62` (button 1 or 2, then how long it was held in seconds, two decimals) or `alert 27.12` (the temperature that set the alert off, two decimals); nothing else; then exit after SECONDS |

`wait-event` must be listening within 2 seconds of being started; events
that happen from then until it exits must all be printed, in order.

**Picture files** (`show-image`): 8 lines of 16 characters, top row first,
leftmost pixel first; each character is a brightness level as a hexadecimal
digit `0`-`9`, `a`-`f` (`f` is brightest), and `.` also means 0. Example (a
diagonal line, two pixels per row):

```text
ff..............
..ff............
....ff..........
......ff........
........ff......
..........ff....
............ff..
..............ff
```

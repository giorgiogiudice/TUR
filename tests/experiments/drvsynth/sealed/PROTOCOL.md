<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# SEALED: the invented device's protocol

> **Blind engineers: stop reading.** This file is the answer key of the
> driver-synthesis experiment (`tests/experiments/drvsynth/README.md`). A
> trial is void if the engineer who writes the candidate driver has seen
> anything under `sealed/`.

The protocol was invented for this experiment on 2026-11-14 and exists
nowhere else. The emulator (`emulator.py`), the Windows driver and program
(`windows/`) and the reference Linux driver (`reference/gadget.py`) all
implement this text; where they disagree, this text is the intent and the
disagreement is a bug.

## 1. Device

| | |
|---|---|
| VID:PID | `1209:000c` (pid.codes test PID) |
| bcdUSB, bcdDevice | 2.00, 2.01 (bcdDevice is a hardware revision, unrelated to the firmware version) |
| Strings | manufacturer "Quillfeather Labs", product "Glimmerdesk G2", serial per unit |
| Speed | high speed |
| Configuration 1, interface 0 | class 0xFF, subclass 0x51, protocol 0x02, three endpoints |
| EP 0x02 | bulk OUT, 512: command frames |
| EP 0x81 | bulk IN, 512: response frames, one frame per transfer |
| EP 0x83 | interrupt IN, 8 bytes, bInterval 8 (16 ms): event reports |

A "desk companion": a sensor block (temperature, humidity, light), a 16 x 8
LED matrix with 16 grey levels, a store of eight short notes, two buttons,
day/night modes and a temperature alert.

All multi-byte fields are **little-endian** except the three marked
**big-endian** below (the temperature field, the authentication response
and the frame checksum).

## 2. Start-up handshake (control endpoint)

Every session starts with the handshake; bulk commands before it are
refused with status 0x81.

| Step | Setup packet | Data |
|---|---|---|
| HELLO | `C0 01 0000 0000 0008` (vendor, device-to-host, request 0x01) | 8 bytes: `03`, nonce N (4 bytes, LE), `01`, `10`, `08` |
| AUTH | `40 02 wValue wIndex 0000` (vendor, host-to-device, request 0x02) | none |
| BYE | `40 03 0000 0000 0000` | none |

- N is chosen by the device, freshly at random for every HELLO.
- The response is `R = rotl32(N, 5) XOR 0x6D2B79F5`, sent **big-endian
  across the two setup words**: `wValue = R >> 16`, `wIndex = R & 0xFFFF`.
- A wrong R stalls the request and forgets N (one try per HELLO). A correct
  R starts a session: the duplicate-sequence memory, the clear-notes arming
  and the event queue are reset.
- BYE ends the session. A new HELLO also ends any session.

## 3. Frames (bulk)

Host to device, EP 0x02:

```text
9C  L  SEQ  CMD  P[0] ... P[n-1]  CK_HI  CK_LO        L = n + 2, n <= 48
```

Device to host, EP 0x81:

```text
9D  L  SEQ  STATUS  P[0] ... P[n-1]  CK_HI  CK_LO     L = n + 2
```

**Checksum** (not a stock CRC; a Fletcher-like sum with a seed and modulus
251) over the bytes `L, SEQ, CMD|STATUS, P...`:

```text
s1 = 0x5A; s2 = 0
for b in bytes: s1 = (s1 + b) % 251; s2 = (s2 + s1) % 251
CK_HI = s2; CK_LO = s1                         (big-endian)
```

- SEQ is chosen by the host; the Windows driver starts at 1 after each
  handshake and increments (skipping 0). The device echoes it.
- **Duplicate suppression:** a frame whose SEQ equals the last executed
  frame's SEQ is not executed again; the device re-sends the cached response.
- A frame that is not well formed (magic, length) gets status 0x82; a bad
  checksum gets 0x80; both echo SEQ when there is one.

Status values: `00` OK, `01` ARMED, `02` MORE (multi-frame response
follows), `03` CHUNK (one frame of a multi-frame response), `80` bad
checksum, `81` no session, `82` bad length/frame, `83` bad argument, `84`
unknown command, `85` bad state, `86` empty.

## 4. Modes: a state machine that changes what commands mean

| Mode | Value | `SET_LEVEL` (0x21) payload means | `GET_LEVEL` (0x22) returns |
|---|---|---|---|
| day | 0 | display brightness, 1 byte 0..100 | 1 byte |
| night | 1 | night-light level, 1 byte 0..100 | 1 byte |
| setup | 2 | temperature-alert threshold, int16 LE, 0.1 °C units, -400..1250; 0x7FFF = alert off | int16 LE |

Wrong payload length for the mode: 0x82. Out of range: 0x83. The mode
survives sessions (it is device state). The Windows program switches to the
mode a setting belongs to, sets it, and switches back.

**Quirk 1, a required delay:** for 200 ms after a `SET_MODE` the device
silently drops every frame (no response; the OUT transfer itself
completes). The Windows driver waits 250 ms after every `SET_MODE`.

## 5. Commands

| CMD | Name | Payload | Response payload |
|---|---|---|---|
| 0x01 | GET_INFO | none | `fw_major fw_minor fw_patch 10 08 08` + uptime u32 LE (seconds) |
| 0x10 | SET_MODE | mode (1 byte) | mode |
| 0x11 | GET_MODE | none | mode |
| 0x21 | SET_LEVEL | mode-dependent (section 4) | none |
| 0x22 | GET_LEVEL | none | mode-dependent |
| 0x30 | GET_SENSORS | none | temperature int16 **big-endian**, 0.01 °C (**quirk 3, odd byte order**); humidity u16 LE, 0.1 %; light u32 LE, lux |
| 0x40 | FRAME_DATA | offset (1 byte), up to 32 bytes | none |
| 0x41 | FRAME_SHOW | none | none (the 64-byte buffer goes on the matrix) |
| 0x42 | DISPLAY_CLEAR | none | none (buffer and matrix cleared) |
| 0x50 | NOTE_LIST | none | used-slot bitmap (bit i = slot i), then 8 lengths |
| 0x51 | NOTE_BEGIN | slot 0..7, length 1..120 | none |
| 0x52 | NOTE_DATA | chunk index (from 0, consecutive), up to 40 bytes | none; out-of-order index: 0x85 |
| 0x53 | NOTE_END | none | none; stored if the bytes received equal the length, else 0x82 |
| 0x54 | NOTE_READ | slot | multi-frame (below); empty slot: 0x86 |
| 0x55 | NOTE_DELETE | slot | none |
| 0x56 | NOTE_CLEAR | none | **quirk 2:** first: status 0x01 ARMED; a second NOTE_CLEAR with a new SEQ within 2 s and nothing else in between clears all notes (status 0x00) |

Frame buffer (64 bytes): the matrix is 16 columns x 8 rows, 4 bits per
pixel, **column-major**: column c occupies bytes 4c..4c+3; byte 4c+k holds
row 2k in its high nibble and row 2k+1 in its low nibble. The Windows driver
sends it as two FRAME_DATA frames (offsets 0 and 32) and a FRAME_SHOW.

**Multi-frame response** (NOTE_READ): the device answers with one frame of
status 0x02 MORE, payload `length count`, then `count` frames of status 0x03
CHUNK, payload `index` + up to 40 bytes, all with the command's SEQ, each
read with its own bulk IN transfer. Notes are written the same way in
reverse (NOTE_BEGIN, NOTE_DATA x chunks of 40, NOTE_END): multi-frame
transfers in both directions.

## 6. Event reports (interrupt IN 0x83, 8 bytes)

Sent only while a session is open; events that happen outside a session are
dropped.

| Byte 0 (kind) | Byte 1 | Bytes 2..7 |
|---|---|---|
| 0x01 button | event counter (mod 256) | button (1 or 2), press duration u16 LE in 10 ms units, 0, 0, 0 |
| 0x02 alert | counter | temperature int16 **big-endian** 0.01 °C, threshold int16 LE 0.1 °C, 0, 0 |
| 0x03 heartbeat | counter | mode, uptime low 16 bits LE, day brightness, 0, 0 |

A heartbeat is sent every second during a session; it is noise to the user.
An alert is sent when the temperature rises from at-or-below the threshold
to above it.

## 7. What the scoring suite checks against this text

| Feature | What a guess cannot fake |
|---|---|
| handshake (every command) | N is fresh in every session; R must be computed |
| checksum | every command of the suite carries payloads that appear in no capture; the device counts bad checksums |
| sensors | values never captured, a negative temperature, light above 65535 |
| modes | the device's own mode and per-mode values, read back from the emulator |
| quirk 1 (delay) | the device counts frames dropped in the busy window |
| quirk 2 (repeat) | the notes are gone from the device only after a correct double NOTE_CLEAR |
| quirk 3 (byte order) | negative and large temperatures |
| events | button durations and an alert temperature chosen by the suite; heartbeats must not be printed |
| info | a firmware version that appears in no capture; the device's uptime |

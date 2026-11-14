# Lab notebook: Glimmerdesk G2 (1209:000c)

Evidence is cited as session/time (sNN t=seconds) from `handoff/captures/session-NN.txt`.
The `.txt` files were checked against the `.pcapng` files (`pcap2.py`): same event count,
payloads, setup packets, statuses and lengths in all ten sessions, nothing extra in the pcapng.

## Checks that back the model

| Check | Script | Result |
|---|---|---|
| Driver replayed against every recorded program run with a transport that insists on byte-identical host transfers in the recorded order, returns the recorded answers, and compares the printed output and exit status with the timeline | `replay.py` | 78 of 78 runs; 553 of 553 host transfers (78 challenge requests, 78 handshake answers, 78 closes, 157 command frames, 162 bulk-in requests); output identical in all runs |
| Frame checksum on every bulk frame, both directions | `cksum3.py` | 319 of 319 (112 distinct) |
| Handshake answer from the nonce | `hs1.py`, `hs2.py`, `replay.py` | 78 of 78 |
| Independent stateful model of the gadget (mode, three paged values, notes, clear-notes arming) fed all recorded commands of sessions 01-10 in order, predicting each reply frame | `sim.py` | 162 of 162 (info and sensor payloads are measurements, taken from the reply) |
| Driver against that model for things never recorded (alert/levels from night mode, notes of 1/39/40/41/79/80/81/119/120 bytes, negative values, light > 2^31, long button hold, bad arguments) | `sim.py` | all pass; this proves the driver is self-consistent, not that the gadget behaves so |

## Conclusions

1. **Session bracket** (certain). Every program run is: control IN `c0 01 0000 0000 len 8`,
   control OUT `40 02 wValue wIndex` (no data), the commands, control OUT `40 03 0000 0000`.
   78 runs, no exception (e.g. s01 t=15.013-15.015).
2. **Handshake** (certain for this gadget). The 8 challenge bytes are `03 n0 n1 n2 n3 01 10 08`;
   only n changes. The answer is an affine GF(2) function of the nonce (78 pairs, rank 33, all
   consistent), which solves to exactly: `r = rotl32(n as little endian, 5) XOR 0x6D2B79F5`,
   `wValue = r >> 16`, `wIndex = r & 0xffff`. E.g. s01 t=15.013: nonce `21 0b d4 83` -> `17aa 1dc5`.
   0x6D2B79F5 is a well-known PRNG constant (mulberry32), so I take it as fixed rather than derived
   from the serial number; simple hashes of the serial/product strings do not give it.
3. **Frames** (certain). Bulk OUT ep 0x02: `9c LEN SEQ CMD payload C1 C0`; bulk IN ep 0x81:
   `9d LEN SEQ STATUS payload C1 C0`. LEN = 2 + payload length. SEQ starts at 1 in every session,
   +1 per command, echoed by the reply. One frame per transfer; the host reads with a 512-byte buffer.
4. **Checksum** (certain). Over LEN..end of payload (header byte excluded): `a=0x5a, b=0`; per byte
   `a=(a+x)%251; b=(b+a)%251`; append `b` then `a`. Modulus 251 found from the residue drifting by 5
   per wrap; unique fit.
5. **Info** `01` -> `maj min patch 10 08 08 uptime(u32 LE)` (s01 t=15.014: 3.1.4, 4305; s09: 4370).
6. **Sensors** `30` -> temperature int16 **big endian** in 1/100 degC, humidity u16 little endian in
   1/10 %, light u32 little endian in lux (s04: -1.25 = `ff 83`, 70210 = `42 12 01 00`,
   131072 = `00 00 02 00`). The single-value commands send the same request (s04).
7. **Mode** `11` -> one byte (0 day, 1 night); `10 m` sets it and echoes m. After every `10` the
   Windows program pauses 250 ms before its next transfer, including before the close
   (s03 t=0.5125->0.7633, s01 t=17.8735->18.1242; all 32 mode changes recorded: 250.6-251.0 ms,
   while every other gap between a completion and the next transfer is under 0.5 ms). Driver: 260 ms.
8. **Paged value** (certain; confirmed by the stateful model and by s09->s10). `21 v` writes and
   `22` reads *the value of the current mode*: mode 0 = daytime brightness (1 byte), mode 1 =
   night-light level (1 byte), mode 2 = a third page used only for the alert (int16 LE, 1/10 degC,
   `ff 7f` = off). The program first asks the mode (`11`); if it is not the needed page it
   switches (`10 p`, pause), does the `21`/`22`, switches back (`10 old`, pause).
   s01 t=17.87 (night level from day), s03 t=4.35 (brightness from night), s07 (alert), s10 (-5.0 = `ce ff`).
9. **Picture** `40 off` + 32 bytes, twice (off 0x00 and 0x20), then `41`. 64 bytes = 16 columns
   x 4 bytes, left column first; within a column top pixel first, two pixels per byte, the upper
   pixel in the high nibble. Six pictures reproduced byte for byte (s06, s10). Clear = `42`.
10. **Notes**. List `50` -> bitmap (bit n = slot n) + 8 length bytes. Write `51 slot len`, then
    `52 idx` + up to 40 bytes per frame (idx 0,1,2), then `53` (s05 t=4.808: 96 bytes = 40+40+16).
    Read `54 slot` -> status 02 `len chunks`, then `chunks` more reply frames (no further
    command, same SEQ) with status 03 `idx data`; empty slot -> status 86, program prints `empty`
    (exit 4 per the task card). Delete `55 slot`.
11. **Clear notes** `56` answers status 01 and does nothing; the program immediately sends `56`
    again (next SEQ) and gets 00 (s08 t=2.6585; the list afterwards is empty). Read as "confirm by repeating".
12. **Events** arrive on interrupt IN ep 0x83 (8 bytes) only while a session is open; `wait-event`
    is just handshake, listen, close (s07, s09, s10). Byte 0 type, byte 1 a counter over all events.
    `01 seq button dur_lo dur_hi` (1/100 s: 0x1e->0.30, 0xa2->1.62, 0x54->0.84, 0x0c->0.12);
    `02 seq T(int16 BE, 1/100 degC) threshold(int16 LE, 1/10)` (0a 98 -> 27.12; fe bb -> -3.25);
    `03 seq mode uptime(u16 LE) brightness` once a second starting 1.000 s after the handshake:
    a heartbeat, printed by nobody. The program listens SECONDS + ~0.02 s (10.016, 6.019, 6.024).
13. **First use** (s01 t=0.77-14.97): only standard descriptor/status requests and two interrupt
    reads kept pending; no vendor initialisation. Nothing to reproduce.

## Not determined, and how the driver hedges

- Whether the handshake depends on anything of the gadget (the fixed challenge bytes
  `03 .. 01 10 08`, firmware, serial). One gadget only, so unknowable; the driver uses the fitted
  formula whatever those bytes are and says so on stderr if they differ.
- Width of the button duration: values seen fit one byte; read as u16 LE (bytes 3-4), like the
  heartbeat's 16-bit field in the same position.
- Light is read as u32 LE (largest seen 131072).
- Status 01 outside clear-notes: never seen. Any command answered 01 is repeated (up to 4 times).
- Writing over an occupied slot, deleting an empty slot, notes of exactly 40/80/120 bytes: never
  recorded. Chunks = ceil(len/40); if `51` is refused and the slot is occupied the driver deletes
  it and retries; delete on an empty slot (status 86) prints `empty`, exit 4, by analogy with read.
- What mode 2 means if the gadget is found in it (only possible after an interrupted run):
  `get-mode` fails; the paged commands restore whatever mode they found.
- Whether the gadget keeps events made while nobody listens. The Windows driver always has reads
  pending, so it can never see old packets; `wait-event` therefore empties the interrupt endpoint
  (40 ms reads) before the handshake, then reads continuously; identical consecutive event
  packets are printed once.
- Minimum pause after a mode change (only 250 ms seen), behaviour when it is not kept: unknown.
- Reply sanity: a reply with another SEQ is skipped (stale), bad checksum/header fails the command.
- pyusb layer could not be run here (no device): endpoints are taken from the descriptors with
  the recorded addresses as fallback; no SET_CONFIGURATION unless the device is unconfigured.

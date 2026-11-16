#!/usr/bin/env python3
"""Linux command-line driver for the Quillfeather Labs Glimmerdesk G2 (USB 1209:000c).

Protocol reconstructed from usbmon recordings of the vendor's Windows program
(see work/NOTES.md).  Summary:

  session   every program run brackets its traffic with vendor control requests:
              IN  c0 01 -> 8 bytes  [03, n0 n1 n2 n3, 01 10 08]   (n = 32-bit nonce, LE)
              OUT 40 02 wValue:wIndex = rotl32(n, 5) ^ 0x6D2B79F5 (high word : low word)
              ... commands ...
              OUT 40 03 (close)
  commands  bulk OUT ep 0x02:  9c LEN SEQ CMD payload.. C1 C0
            bulk IN  ep 0x81:  9d LEN SEQ STATUS payload.. C1 C0
            LEN = number of bytes between LEN and the checksum; SEQ = 1, 2, 3...
            per session, echoed in the reply; checksum is a Fletcher sum modulo
            251 over LEN..payload with the first sum starting at 0x5a.
  events    interrupt IN ep 0x83, 8 bytes, only while a session is open:
              01 seq button dur_lo dur_hi      button, duration in 1/100 s
              02 seq T_hi T_lo thr_lo thr_hi   alert, T in 1/100 degC (big endian)
              03 seq ...                       heartbeat, once a second (ignored)
"""

import errno
import os
import re
import signal
import sys
import time

VID = 0x1209
PID = 0x000C

# vendor control requests
REQ_CHALLENGE = 0x01
REQ_RESPONSE = 0x02
REQ_CLOSE = 0x03
HANDSHAKE_KEY = 0x6D2B79F5
HANDSHAKE_ROT = 5

# framing
HDR_OUT = 0x9C
HDR_IN = 0x9D

# commands
CMD_INFO = 0x01
CMD_SET_MODE = 0x10
CMD_GET_MODE = 0x11
CMD_SET_VALUE = 0x21      # the value of the current page (see PAGE_*)
CMD_GET_VALUE = 0x22
CMD_SENSORS = 0x30
CMD_IMG_DATA = 0x40       # [offset] + 32 bytes
CMD_IMG_SHOW = 0x41
CMD_IMG_CLEAR = 0x42
CMD_NOTE_LIST = 0x50
CMD_NOTE_BEGIN = 0x51     # [slot, length]
CMD_NOTE_DATA = 0x52      # [chunk index] + up to 40 bytes
CMD_NOTE_COMMIT = 0x53
CMD_NOTE_READ = 0x54      # [slot]
CMD_NOTE_DELETE = 0x55    # [slot]
CMD_NOTE_CLEAR = 0x56     # must be sent twice: first answer is status 01

# reply status
ST_OK = 0x00
ST_AGAIN = 0x01           # "repeat the command" (seen for CMD_NOTE_CLEAR)
ST_NOTE_HEAD = 0x02       # [length, chunks], the chunks follow as separate frames
ST_NOTE_CHUNK = 0x03      # [chunk index] + data
ST_EMPTY = 0x86           # empty note slot

# modes; the mode also selects which value CMD_SET_VALUE/CMD_GET_VALUE address
PAGE_DAY = 0              # value = daytime brightness, 1 byte, 0..100
PAGE_NIGHT = 1            # value = night-light level, 1 byte, 0..100
PAGE_ALERT = 2            # value = alert temperature, int16 LE, 1/10 degC; 0x7fff = off
ALERT_OFF = 0x7FFF

MODE_SETTLE = 0.26        # the Windows program pauses 250 ms after every mode change
LISTEN_EXTRA = 0.02       # wait-event listens this much longer than SECONDS
NOTE_CHUNK = 40
IMG_CHUNK = 32

# events
EV_BUTTON = 0x01
EV_ALERT = 0x02
EV_HEARTBEAT = 0x03

IO_TIMEOUT_MS = 3000

EXIT_OK = 0
EXIT_FAIL = 1
EXIT_USAGE = 2
EXIT_EMPTY = 4


class GadgetError(Exception):
    """Anything that makes the command fail (exit status 1)."""


class UsageError(Exception):
    """Bad command line (exit status 2); the gadget is not touched."""


class TransportTimeout(Exception):
    """A read timed out without data."""


def log(msg):
    try:
        sys.stderr.write("gadget: %s\n" % msg)
        sys.stderr.flush()
    except Exception:
        pass


# --------------------------------------------------------------------------
# pure protocol helpers
# --------------------------------------------------------------------------

def checksum(body):
    """Fletcher-style sums modulo 251 over LEN..payload; returns (C1, C0)."""
    a, b = 0x5A, 0
    for x in body:
        a = (a + x) % 251
        b = (b + a) % 251
    return bytes((b, a))


def build_frame(seq, cmd, payload=b""):
    payload = bytes(payload)
    if len(payload) + 2 > 255:
        raise GadgetError("payload too long")
    body = bytes((len(payload) + 2, seq & 0xFF, cmd & 0xFF)) + payload
    return bytes((HDR_OUT,)) + body + checksum(body)


def handshake_response(challenge):
    """wValue, wIndex for request 02 from the 8 bytes returned by request 01."""
    nonce = int.from_bytes(bytes(challenge[1:5]), "little")
    r = ((nonce << HANDSHAKE_ROT) | (nonce >> (32 - HANDSHAKE_ROT))) & 0xFFFFFFFF
    r ^= HANDSHAKE_KEY
    return (r >> 16) & 0xFFFF, r & 0xFFFF


def fmt_fixed(value, decimals):
    """Format an integer scaled by 10**decimals, e.g. (-5, 2) -> '-0.05'."""
    scale = 10 ** decimals
    sign = "-" if value < 0 else ""
    value = abs(value)
    return "%s%d.%0*d" % (sign, value // scale, decimals, value % scale)


def s16(value):
    return value - 0x10000 if value & 0x8000 else value


def parse_picture(text):
    """8 lines of 16 brightness characters -> 64 bytes, column by column,
    top pixel first, two pixels per byte (first pixel in the high nibble)."""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    lines = [ln.rstrip(" \t") for ln in lines]
    while lines and lines[-1] == "":
        lines.pop()
    while lines and lines[0] == "":
        lines.pop(0)
    if len(lines) != 8:
        raise UsageError("picture must have 8 lines, found %d" % len(lines))
    rows = []
    for n, ln in enumerate(lines):
        if len(ln) != 16:
            raise UsageError("picture line %d must have 16 characters" % (n + 1))
        row = []
        for ch in ln:
            if ch == ".":
                row.append(0)
            elif ch in "0123456789abcdefABCDEF":
                row.append(int(ch, 16))
            else:
                raise UsageError("bad character %r in picture line %d" % (ch, n + 1))
        rows.append(row)
    out = bytearray()
    for col in range(16):
        for r in range(0, 8, 2):
            out.append((rows[r][col] << 4) | rows[r + 1][col])
    return bytes(out)


# --------------------------------------------------------------------------
# USB transport (pyusb)
# --------------------------------------------------------------------------

class UsbTransport:
    def __init__(self):
        import usb.core
        import usb.util
        self._core = usb.core
        self._util = usb.util
        self.ep_out = 0x02
        self.ep_in = 0x81
        self.ep_evt = 0x83
        self.evt_size = 8
        self.intf_num = 0
        self._claimed = False

        dev = usb.core.find(idVendor=VID, idProduct=PID)
        if dev is None:
            raise GadgetError("no Glimmerdesk (1209:000c) found")
        self.dev = dev
        try:
            cfg = dev.get_active_configuration()
        except usb.core.USBError:
            dev.set_configuration()
            cfg = dev.get_active_configuration()

        # Find the vendor interface and its endpoints; fall back to the
        # addresses of the recorded gadget.
        try:
            best = None
            for intf in cfg:
                eps = {"out": None, "in": None, "evt": None, "evt_size": 8}
                for ep in intf:
                    is_in = bool(ep.bEndpointAddress & 0x80)
                    kind = ep.bmAttributes & 0x03
                    if kind == 2 and not is_in and eps["out"] is None:
                        eps["out"] = ep.bEndpointAddress
                    elif kind == 2 and is_in and eps["in"] is None:
                        eps["in"] = ep.bEndpointAddress
                    elif kind == 3 and is_in and eps["evt"] is None:
                        eps["evt"] = ep.bEndpointAddress
                        eps["evt_size"] = ep.wMaxPacketSize & 0x7FF
                if eps["out"] is not None and eps["in"] is not None:
                    if best is None or (intf.bInterfaceClass == 0xFF and best[0].bInterfaceClass != 0xFF):
                        best = (intf, eps)
            if best is not None:
                intf, eps = best
                self.intf_num = intf.bInterfaceNumber
                self.ep_out = eps["out"]
                self.ep_in = eps["in"]
                if eps["evt"] is not None:
                    self.ep_evt = eps["evt"]
                    self.evt_size = max(8, eps["evt_size"])
        except Exception as exc:  # descriptor oddities: keep the defaults
            log("descriptor scan failed (%s); using default endpoints" % exc)

        try:
            if dev.is_kernel_driver_active(self.intf_num):
                dev.detach_kernel_driver(self.intf_num)
        except Exception:
            pass
        try:
            usb.util.claim_interface(dev, self.intf_num)
            self._claimed = True
        except Exception as exc:
            log("could not claim interface %d: %s" % (self.intf_num, exc))

    def _is_timeout(self, exc):
        tmo = getattr(self._core, "USBTimeoutError", None)
        if tmo is not None and isinstance(exc, tmo):
            return True
        if getattr(exc, "errno", None) == errno.ETIMEDOUT:
            return True
        if getattr(exc, "backend_error_code", None) == -7:  # LIBUSB_ERROR_TIMEOUT
            return True
        return False

    def ctrl_in(self, request, value, index, length, timeout_ms=IO_TIMEOUT_MS):
        try:
            return bytes(self.dev.ctrl_transfer(0xC0, request, value, index, length, timeout_ms))
        except self._core.USBError as exc:
            raise GadgetError("control request %02x failed: %s" % (request, exc))

    def ctrl_out(self, request, value, index, timeout_ms=IO_TIMEOUT_MS):
        try:
            self.dev.ctrl_transfer(0x40, request, value, index, None, timeout_ms)
        except self._core.USBError as exc:
            raise GadgetError("control request %02x failed: %s" % (request, exc))

    def bulk_out(self, data, timeout_ms=IO_TIMEOUT_MS):
        try:
            n = self.dev.write(self.ep_out, data, timeout_ms)
        except self._core.USBError as exc:
            raise GadgetError("bulk write failed: %s" % exc)
        if n != len(data):
            raise GadgetError("short bulk write (%d of %d bytes)" % (n, len(data)))

    def bulk_in(self, size=512, timeout_ms=IO_TIMEOUT_MS):
        try:
            return bytes(self.dev.read(self.ep_in, size, max(1, int(timeout_ms))))
        except self._core.USBError as exc:
            if self._is_timeout(exc):
                raise TransportTimeout()
            raise GadgetError("bulk read failed: %s" % exc)

    def intr_in(self, timeout_ms):
        try:
            return bytes(self.dev.read(self.ep_evt, self.evt_size, max(1, int(timeout_ms))))
        except self._core.USBError as exc:
            if self._is_timeout(exc):
                raise TransportTimeout()
            raise GadgetError("interrupt read failed: %s" % exc)

    def close(self):
        try:
            if self._claimed:
                self._util.release_interface(self.dev, self.intf_num)
        except Exception:
            pass
        try:
            self._util.dispose_resources(self.dev)
        except Exception:
            pass


# --------------------------------------------------------------------------
# the gadget
# --------------------------------------------------------------------------

class Gadget:
    def __init__(self, transport, now=time.monotonic, sleep=time.sleep):
        self.t = transport
        self.now = now
        self.sleep = sleep
        self.seq = 0
        self.rx = b""
        self.is_open = False

    # ---- session -----------------------------------------------------------

    def open(self):
        ch = self.t.ctrl_in(REQ_CHALLENGE, 0, 0, 8)
        if len(ch) < 5:
            raise GadgetError("short handshake challenge: %s" % ch.hex())
        if len(ch) != 8 or ch[0] != 0x03:
            log("unexpected handshake challenge %s; answering as usual" % ch.hex())
        value, index = handshake_response(ch)
        self.t.ctrl_out(REQ_RESPONSE, value, index)
        self.is_open = True
        self.seq = 0
        self.rx = b""

    def close(self):
        if self.is_open:
            self.is_open = False
            self.t.ctrl_out(REQ_CLOSE, 0, 0)

    # ---- frames ------------------------------------------------------------

    def _read_frame(self):
        """One reply frame -> (seq, status, payload)."""
        deadline = self.now() + IO_TIMEOUT_MS / 1000.0
        while True:
            buf = self.rx
            if buf and buf[0] != HDR_IN:
                self.rx = b""
                raise GadgetError("bad reply header: %s" % buf.hex())
            if len(buf) >= 2:
                if buf[1] < 2:
                    self.rx = b""
                    raise GadgetError("bad reply length: %s" % buf.hex())
                total = buf[1] + 4
                if len(buf) >= total:
                    frame, self.rx = buf[:total], buf[total:]
                    if checksum(frame[1:-2]) != frame[-2:]:
                        self.rx = b""
                        raise GadgetError("bad reply checksum: %s" % frame.hex())
                    return frame[2], frame[3], frame[4:-2]
            if self.now() > deadline:
                raise GadgetError("timed out waiting for a reply")
            try:
                self.rx += self.t.bulk_in(512, IO_TIMEOUT_MS)
            except TransportTimeout:
                raise GadgetError("timed out waiting for a reply")

    def _read_reply(self, seq):
        """The next reply frame that carries our sequence number."""
        for _ in range(8):
            rseq, status, payload = self._read_frame()
            if rseq == seq:
                return status, payload
            log("ignoring reply with sequence %d (expected %d)" % (rseq, seq))
        raise GadgetError("no reply with the expected sequence number")

    def _exchange(self, cmd, payload=b""):
        self.seq = self.seq % 255 + 1
        seq = self.seq
        self.t.bulk_out(build_frame(seq, cmd, payload))
        status, data = self._read_reply(seq)
        return seq, status, data

    def request(self, cmd, payload=b"", ok=(ST_OK,), repeats=4):
        """Send a command; repeat it while the gadget answers 'again' (01).
        Returns (seq, status, payload); raises unless status is in `ok`."""
        tries = 0
        while True:
            seq, status, data = self._exchange(cmd, payload)
            if status != ST_AGAIN or ST_AGAIN in ok or tries >= repeats:
                break
            tries += 1
            if tries > 1:
                self.sleep(0.02)
        if status not in ok:
            raise GadgetError("command %02x failed with status %02x" % (cmd, status))
        return seq, status, data

    # ---- modes and paged values -------------------------------------------

    def get_mode(self):
        _, _, data = self.request(CMD_GET_MODE)
        if len(data) < 1:
            raise GadgetError("empty mode reply")
        return data[0]

    def set_mode(self, mode):
        try:
            _, _, data = self.request(CMD_SET_MODE, bytes((mode,)))
        finally:
            # the gadget needs a moment after a mode change, before the next
            # command and before the session is closed
            self.sleep(MODE_SETTLE)
        if data and data[0] != mode:
            raise GadgetError("mode change to %d not taken (gadget says %d)" % (mode, data[0]))

    def on_page(self, page, action):
        """Run action() with the gadget switched to `page`, then put the
        gadget back into the mode it was in."""
        current = self.get_mode()
        if current == page:
            return action()
        self.set_mode(page)
        try:
            result = action()
        except BaseException:
            try:
                self.set_mode(current)
            except Exception as exc:
                log("could not restore the mode: %s" % exc)
            raise
        self.set_mode(current)
        return result

    def get_value(self):
        _, _, data = self.request(CMD_GET_VALUE)
        return data

    def set_value(self, raw):
        self.request(CMD_SET_VALUE, raw)

    # ---- features ----------------------------------------------------------

    def info(self):
        _, _, d = self.request(CMD_INFO)
        if len(d) < 10:
            raise GadgetError("short info reply: %s" % d.hex())
        return (d[0], d[1], d[2]), int.from_bytes(d[6:10], "little")

    def sensors(self):
        _, _, d = self.request(CMD_SENSORS)
        if len(d) < 8:
            raise GadgetError("short sensor reply: %s" % d.hex())
        temp = s16(int.from_bytes(d[0:2], "big"))        # 1/100 degC, big endian
        hum = int.from_bytes(d[2:4], "little")           # 1/10 %
        light = int.from_bytes(d[4:8], "little")         # lux
        return temp, hum, light

    def get_level(self, page):
        d = self.on_page(page, self.get_value)
        if len(d) < 1:
            raise GadgetError("empty value reply")
        return d[0]

    def set_level(self, page, level):
        self.on_page(page, lambda: self.set_value(bytes((level,))))

    def get_alert(self):
        d = self.on_page(PAGE_ALERT, self.get_value)
        if len(d) < 2:
            raise GadgetError("short alert reply: %s" % d.hex())
        raw = int.from_bytes(d[0:2], "little")
        return None if raw == ALERT_OFF else s16(raw)

    def set_alert(self, tenths):
        raw = ALERT_OFF if tenths is None else (tenths & 0xFFFF)
        self.on_page(PAGE_ALERT, lambda: self.set_value(raw.to_bytes(2, "little")))

    def show_image(self, pixels):
        for off in range(0, len(pixels), IMG_CHUNK):
            self.request(CMD_IMG_DATA, bytes((off,)) + pixels[off:off + IMG_CHUNK])
        self.request(CMD_IMG_SHOW)

    def clear_display(self):
        self.request(CMD_IMG_CLEAR)

    def list_notes(self):
        _, _, d = self.request(CMD_NOTE_LIST)
        if len(d) < 9:
            raise GadgetError("short note list reply: %s" % d.hex())
        return [(slot, d[1 + slot]) for slot in range(8) if d[0] & (1 << slot)]

    def write_note(self, slot, text):
        head = bytes((slot, len(text)))
        try:
            self.request(CMD_NOTE_BEGIN, head)
        except GadgetError as exc:
            # Never recorded: writing over an existing note.  If the gadget
            # refuses, free the slot first and try once more.
            log("%s; trying to free slot %d first" % (exc, slot))
            if not any(s == slot for s, _ in self.list_notes()):
                raise
            self.request(CMD_NOTE_DELETE, bytes((slot,)))
            self.request(CMD_NOTE_BEGIN, head)
        for idx, off in enumerate(range(0, len(text), NOTE_CHUNK)):
            self.request(CMD_NOTE_DATA, bytes((idx,)) + text[off:off + NOTE_CHUNK])
        self.request(CMD_NOTE_COMMIT)

    def read_note(self, slot):
        """The note's bytes, or None for an empty slot."""
        seq, status, d = self.request(CMD_NOTE_READ, bytes((slot,)),
                                      ok=(ST_OK, ST_NOTE_HEAD, ST_NOTE_CHUNK, ST_EMPTY))
        if status == ST_EMPTY:
            return None
        if status == ST_OK:               # never seen: the text in one plain reply
            return d
        if status == ST_NOTE_CHUNK:       # never seen: chunk without a header
            return d[1:]
        if len(d) < 2:
            raise GadgetError("short note header: %s" % d.hex())
        length, chunks = d[0], d[1]
        parts = []
        got = 0
        while len(parts) < chunks or got < length:
            status, c = self._read_reply(seq)
            if status != ST_NOTE_CHUNK or len(c) < 1:
                raise GadgetError("unexpected reply while reading a note (status %02x)" % status)
            parts.append((c[0], c[1:]))
            got += len(c) - 1
        if sorted(i for i, _ in parts) == list(range(len(parts))):
            parts.sort(key=lambda p: p[0])        # chunk index order
        text = b"".join(data for _, data in parts)
        return text[:length]

    def delete_note(self, slot):
        """True if deleted, False if the slot was empty."""
        _, status, _ = self.request(CMD_NOTE_DELETE, bytes((slot,)), ok=(ST_OK, ST_EMPTY))
        return status == ST_OK

    def clear_notes(self):
        # The gadget answers the first request with status 01 and only does
        # it when the request is repeated.
        self.request(CMD_NOTE_CLEAR, repeats=8)

    def wait_events(self, seconds, emit):
        """Report button and alert events for `seconds` (session already open)."""
        # the Windows program listens for SECONDS plus about 0.02 s
        deadline = self.now() + seconds + LISTEN_EXTRA
        last = None
        complained = False
        while True:
            remaining = deadline - self.now()
            if remaining <= 0:
                break
            try:
                pkt = self.t.intr_in(min(remaining, 5.0) * 1000.0)
            except TransportTimeout:
                continue
            except GadgetError as exc:
                if not complained:
                    log("%s; still listening" % exc)
                    complained = True
                self.sleep(0.05)
                continue
            if len(pkt) < 2:
                continue
            kind = pkt[0]
            if kind not in (EV_BUTTON, EV_ALERT):
                continue                  # heartbeat (03) or something unknown
            if pkt == last:
                continue                  # the very same event delivered twice
            last = pkt
            if kind == EV_BUTTON and len(pkt) >= 5:
                hundredths = int.from_bytes(pkt[3:5], "little")
                emit("button %d %s" % (pkt[2], fmt_fixed(hundredths, 2)))
            elif kind == EV_ALERT and len(pkt) >= 4:
                temp = s16(int.from_bytes(pkt[2:4], "big"))
                emit("alert %s" % fmt_fixed(temp, 2))

    def drain_events(self):
        """Throw away event packets left over from before this run (the
        Windows driver always listens, so it never sees stale ones)."""
        for _ in range(32):
            try:
                self.t.intr_in(40)
            except TransportTimeout:
                return
            except GadgetError:
                return


# --------------------------------------------------------------------------
# command line
# --------------------------------------------------------------------------

def _int_arg(text, lo, hi, what):
    if not re.fullmatch(r"\+?\d+", text.strip()):
        raise UsageError("%s must be a whole number from %d to %d" % (what, lo, hi))
    value = int(text)
    if not lo <= value <= hi:
        raise UsageError("%s must be from %d to %d" % (what, lo, hi))
    return value


def _alert_arg(text):
    text = text.strip()
    if text.lower() == "off":
        return None
    m = re.fullmatch(r"([+-]?)(\d+)(?:\.(\d?)0*)?", text)
    if not m:
        raise UsageError("alert must be a temperature with at most one decimal, or 'off'")
    tenths = int(m.group(2)) * 10 + int(m.group(3) or 0)
    if m.group(1) == "-":
        tenths = -tenths
    if not -400 <= tenths <= 1250:
        raise UsageError("alert temperature must be from -40.0 to 125.0")
    return tenths


def _need(args, n, usage):
    if len(args) != n:
        raise UsageError("usage: gadget.py %s" % usage)


def plan(argv):
    """Check the command line; returns (action(gadget, emit) -> exit status, is_wait)."""
    if not argv:
        raise UsageError("usage: gadget.py COMMAND [ARGUMENTS]")
    cmd, args = argv[0], argv[1:]

    if cmd == "info":
        _need(args, 0, "info")

        def act(g, emit):
            (a, b, c), uptime = g.info()
            emit("firmware %d.%d.%d" % (a, b, c))
            emit("uptime %d" % uptime)
            return EXIT_OK
        return act

    if cmd in ("read-temperature", "read-humidity", "read-light", "read-sensors"):
        _need(args, 0, cmd)

        def act(g, emit):
            temp, hum, light = g.sensors()
            if cmd == "read-temperature":
                emit(fmt_fixed(temp, 2))
            elif cmd == "read-humidity":
                emit(fmt_fixed(hum, 1))
            elif cmd == "read-light":
                emit("%d" % light)
            else:
                emit("temperature %s" % fmt_fixed(temp, 2))
                emit("humidity %s" % fmt_fixed(hum, 1))
                emit("light %d" % light)
            return EXIT_OK
        return act

    if cmd == "get-mode":
        _need(args, 0, "get-mode")

        def act(g, emit):
            mode = g.get_mode()
            if mode == PAGE_DAY:
                emit("day")
            elif mode == PAGE_NIGHT:
                emit("night")
            else:
                raise GadgetError("gadget is in unknown mode %d" % mode)
            return EXIT_OK
        return act

    if cmd == "set-mode":
        _need(args, 1, "set-mode day|night")
        name = args[0].strip().lower()
        if name not in ("day", "night"):
            raise UsageError("mode must be 'day' or 'night'")

        def act(g, emit):
            g.set_mode(PAGE_NIGHT if name == "night" else PAGE_DAY)
            emit("mode %s" % name)
            return EXIT_OK
        return act

    if cmd in ("set-brightness", "set-night-level"):
        _need(args, 1, "%s 0..100" % cmd)
        level = _int_arg(args[0], 0, 100, "the level")
        page = PAGE_DAY if cmd == "set-brightness" else PAGE_NIGHT
        word = "brightness" if cmd == "set-brightness" else "night-level"

        def act(g, emit):
            g.set_level(page, level)
            emit("%s %d" % (word, level))
            return EXIT_OK
        return act

    if cmd in ("get-brightness", "get-night-level"):
        _need(args, 0, cmd)
        page = PAGE_DAY if cmd == "get-brightness" else PAGE_NIGHT

        def act(g, emit):
            emit("%d" % g.get_level(page))
            return EXIT_OK
        return act

    if cmd == "set-alert":
        _need(args, 1, "set-alert TEMPERATURE|off")
        tenths = _alert_arg(args[0])

        def act(g, emit):
            g.set_alert(tenths)
            emit("alert %s" % ("off" if tenths is None else fmt_fixed(tenths, 1)))
            return EXIT_OK
        return act

    if cmd == "get-alert":
        _need(args, 0, "get-alert")

        def act(g, emit):
            tenths = g.get_alert()
            emit("off" if tenths is None else fmt_fixed(tenths, 1))
            return EXIT_OK
        return act

    if cmd == "show-image":
        _need(args, 1, "show-image FILE")
        try:
            with open(args[0], "rb") as fh:
                text = fh.read().decode("latin-1")
        except OSError as exc:
            raise UsageError("cannot read %s: %s" % (args[0], exc))
        pixels = parse_picture(text)

        def act(g, emit):
            g.show_image(pixels)
            emit("image shown")
            return EXIT_OK
        return act

    if cmd == "clear-display":
        _need(args, 0, "clear-display")

        def act(g, emit):
            g.clear_display()
            emit("display cleared")
            return EXIT_OK
        return act

    if cmd == "list-notes":
        _need(args, 0, "list-notes")

        def act(g, emit):
            for slot, length in g.list_notes():
                emit("%d %d" % (slot, length))
            return EXIT_OK
        return act

    if cmd == "write-note":
        _need(args, 2, "write-note SLOT TEXT")
        slot = _int_arg(args[0], 0, 7, "the slot")
        text = os.fsencode(args[1])
        if not 1 <= len(text) <= 120:
            raise UsageError("the text must be 1 to 120 bytes long")

        def act(g, emit):
            g.write_note(slot, text)
            emit("note %d stored" % slot)
            return EXIT_OK
        return act

    if cmd == "read-note":
        _need(args, 1, "read-note SLOT")
        slot = _int_arg(args[0], 0, 7, "the slot")

        def act(g, emit):
            text = g.read_note(slot)
            if text is None:
                emit("empty")
                return EXIT_EMPTY
            emit(text)
            return EXIT_OK
        return act

    if cmd == "delete-note":
        _need(args, 1, "delete-note SLOT")
        slot = _int_arg(args[0], 0, 7, "the slot")

        def act(g, emit):
            if not g.delete_note(slot):
                emit("empty")
                return EXIT_EMPTY
            emit("note %d deleted" % slot)
            return EXIT_OK
        return act

    if cmd == "clear-notes":
        _need(args, 0, "clear-notes")

        def act(g, emit):
            g.clear_notes()
            emit("notes cleared")
            return EXIT_OK
        return act

    if cmd == "wait-event":
        _need(args, 1, "wait-event SECONDS")
        try:
            seconds = float(args[0])
        except ValueError:
            raise UsageError("SECONDS must be a number from 1 to 600")
        if not 1 <= seconds <= 600:
            raise UsageError("SECONDS must be from 1 to 600")

        def act(g, emit):
            g.wait_events(seconds, emit)
            return EXIT_OK
        act.is_wait = True
        return act

    raise UsageError("unknown command %r" % cmd)


def run(argv, make_transport, emit, now=time.monotonic, sleep=time.sleep):
    """Run one command; returns the exit status."""
    try:
        action = plan(argv)
    except UsageError as exc:
        log(str(exc))
        return EXIT_USAGE

    transport = None
    gadget = None
    status = EXIT_FAIL
    try:
        transport = make_transport()
        gadget = Gadget(transport, now=now, sleep=sleep)
        if getattr(action, "is_wait", False):
            gadget.drain_events()
        gadget.open()
        status = action(gadget, emit)
    except GadgetError as exc:
        log(str(exc))
        status = EXIT_FAIL
    except TransportTimeout:
        log("timed out")
        status = EXIT_FAIL
    finally:
        if gadget is not None:
            try:
                gadget.close()
            except Exception as exc:
                # the command itself is already done (and reported)
                log("closing the session failed: %s" % exc)
        if transport is not None:
            try:
                transport.close()
            except Exception:
                pass
    return status


def _stdout_emit(line):
    data = line if isinstance(line, bytes) else line.encode("utf-8", "surrogateescape")
    out = sys.stdout.buffer
    out.write(data + b"\n")
    out.flush()


def _on_signal(signum, frame):
    raise SystemExit(128 + signum)


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        try:
            signal.signal(sig, _on_signal)
        except Exception:
            pass
    try:
        return run(argv, UsbTransport, _stdout_emit)
    except BrokenPipeError:
        return EXIT_FAIL
    except Exception as exc:  # anything unexpected: fail, quietly on stdout
        log("error: %s: %s" % (type(exc).__name__, exc))
        return EXIT_FAIL


if __name__ == "__main__":
    sys.exit(main())

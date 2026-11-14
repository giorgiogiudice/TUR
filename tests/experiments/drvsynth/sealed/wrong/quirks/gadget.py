#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""SEALED: a deliberately wrong driver for the scoring suite's negative
control, "almost right": the handshake, framing and checksum are correct
(the reference driver's), but it misses the device's quirks and one
encoding detail:
  - no pause after a mode change (and no retry);
  - "clear all notes" sent once;
  - the temperature field read as little-endian;
  - every event report printed, heartbeats included.
The suite must fail the features these touch and pass the rest.
"""

import importlib.util
import pathlib
import struct
import sys

_path = pathlib.Path(__file__).resolve().parents[2] / "reference" / "gadget.py"
_spec = importlib.util.spec_from_file_location("reference_gadget", _path)
ref = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ref)


class AlmostRight(ref.Gadget):
    def mode_delay(self) -> None:
        pass

    def clear_notes(self) -> None:
        self.transact(0x56, accept=(0x01,))

    def event_line(self, report: bytes) -> str:
        if report[0] == 0x02:
            return f"alert {ref.decode_temperature(report[2:4]) / 100:.2f}"
        return f"button {report[2]} {struct.unpack_from('<H', report, 3)[0] / 100:.2f}"


ref.decode_temperature = lambda two: struct.unpack("<h", two)[0]

if __name__ == "__main__":
    sys.exit(ref.main(sys.argv[1:], AlmostRight))

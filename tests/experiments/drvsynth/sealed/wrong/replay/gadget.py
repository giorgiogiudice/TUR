#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""SEALED: a deliberately wrong driver for the scoring suite's negative
control, "the guess": what someone would write after only skimming the
captures. It replays one captured handshake response instead of computing
it from the device's nonce, and uses a plain 16-bit byte sum as the frame
checksum. Everything else is the reference driver. The suite must fail it.
"""

import importlib.util
import pathlib
import sys

_path = pathlib.Path(__file__).resolve().parents[2] / "reference" / "gadget.py"
_spec = importlib.util.spec_from_file_location("reference_gadget", _path)
ref = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ref)

REPLAYED = 0x3E1D5A07  # an authentication answer copied from a capture


def plain_sum(data: bytes) -> bytes:
    s = sum(data) & 0xFFFF
    return bytes([s >> 8, s & 0xFF])


class Guess(ref.Gadget):
    def connect(self) -> None:
        self.dev.ctrl_transfer(0xC0, 0x01, 0, 0, 8, timeout=ref.TIMEOUT_MS)
        self.dev.ctrl_transfer(0x40, 0x02, REPLAYED >> 16, REPLAYED & 0xFFFF, None, timeout=ref.TIMEOUT_MS)
        self.seq = 0


ref.checksum = plain_sum

if __name__ == "__main__":
    sys.exit(ref.main(sys.argv[1:], Guess))

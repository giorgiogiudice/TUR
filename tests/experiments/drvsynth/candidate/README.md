<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Blind trial candidate, 14 November 2026

`gadget.py` is the Linux driver written by the blind engineer (a fresh Claude
Fable 5.2 agent) from the hand-off bundle alone. It is committed byte for byte
as it was handed in (SHA-256 `5014140e228738cfa311e723838fdbd1c0d6a96e889e02a62e09bcef020584b0`), which is why it carries no SPDX line of
its own; it is TUR's own code under the project licence. `NOTES.md` is the
engineer's lab notebook.

How the trial was kept blind:

- The engineer worked in a directory outside the repository that held only a
  copy of `handoff/`, with instructions to read and write nowhere else, to use
  no network and no other agent, and to submit once.
- Afterwards the lead audited the engineer's 40 tool calls: every file read,
  written or executed was inside that directory, and no network or agent tool
  was used. The engineer disclosed one failed `open()` of a deliberately
  non-existent path during its own error-handling test.
- The engineer never ran the driver against the device; this commit is its
  first and only scoring.

Before handing in, the engineer reported that its model of the protocol
reproduced all 553 recorded host transfers byte for byte and all 78 recorded
program runs' output.

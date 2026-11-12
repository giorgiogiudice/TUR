<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/upstream-licensed/

Side C of the licence boundary
([desktop-experience.md](../../docs/architecture/desktop-experience.md),
section 4): code that a GPL program loads and runs through its programming
interface. Such code cannot carry the project licence. It carries its host's.

**There is none, and the design intends there to be none.** Every place where
a plugin would have been the usual answer has an answer that is a standalone
program or a data file.

If that changes, the code goes in `upstream-licensed/<host>/`, and all of the
following hold:

- every file in it carries the host's SPDX identifier (for Plasma and KWin,
  the GPL, version 2 or later) and none carries the project's;
- the directory has its own `LICENSE` and a `README.md` with the reason it
  could not be a standalone program or data;
- the reason is recorded as an amendment to
  [ADR-0016](../../docs/adr/0016-desktop-and-user-interface.md).

Two reserves are named in the design: `plasma-layout/`, a panel layout script
of about fifteen lines, if the data-only panel proves unreliable; and patches
to KWin, which are sent upstream and never live here.

`desktop/ci/boundary.py` enforces the rule in both directions: no GPL
identifier outside this directory, no project identifier inside a host's.

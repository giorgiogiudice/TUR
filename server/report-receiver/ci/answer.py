#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Runs a command with a terminal as its standard input and answers one question.

    answer.py PROMPT ANSWER -- COMMAND [ARGUMENT...]

`tur-diag report submit` asks before it sends, and only on a terminal: it
has no --yes. This plays the person at the keyboard. The command's standard
input is the slave side of a new pseudo-terminal; its output and errors are
read from a pipe. When PROMPT appears in what it wrote, ANSWER and a line
break are typed. Everything the command wrote is printed; the exit status
is the command's. Used by deployed_e2e.py inside the client container.
"""

import os
import pty
import select
import subprocess
import sys
import time

LIMIT_SECONDS = 600


def main():
    if len(sys.argv) < 5 or sys.argv[3] != "--":
        sys.exit(__doc__)
    prompt = sys.argv[1].encode()
    answer = sys.argv[2].encode() + b"\n"
    command = sys.argv[4:]
    master, slave = pty.openpty()
    process = subprocess.Popen(command, stdin=slave, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    os.close(slave)
    output = b""
    answered = False
    deadline = time.time() + LIMIT_SECONDS
    while time.time() < deadline:
        ready, _, _ = select.select([process.stdout], [], [], 1)
        if not ready:
            if process.poll() is not None:
                break
            continue
        chunk = os.read(process.stdout.fileno(), 65536)
        if not chunk:
            break
        output += chunk
        if not answered and prompt in output:
            os.write(master, answer)
            answered = True
    else:
        process.kill()
        output += b"\nanswer.py: the command did not finish in time\n"
    status = process.wait()
    os.close(master)
    sys.stdout.buffer.write(output)
    sys.stdout.buffer.write(f"\nanswer.py: answered={'yes' if answered else 'no'}\n".encode())
    sys.exit(status if status >= 0 else 1)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Tries to reach the canary listeners of the P@x end-to-end test.

    probe_sockets.py TCP_PORT ABSTRACT_NAME UNIX_PATH...

Tries a TCP connection to 127.0.0.1:TCP_PORT, a connection to the abstract
Unix socket ABSTRACT_NAME, and a connection to each Unix socket path. Prints
one JSON object: for each target, "connected" or "blocked: <error>".

The end-to-end test runs this twice: outside the sandbox, where every target
must be reached (the canaries work), and inside the read-only sandbox, where
none may be.
"""

import json
import socket
import sys


def attempt(family, address):
    """Whether a connection to the address is made.

    A connection that was made counts as reached even if the bytes after it
    cannot be sent: a canary listener accepts and closes at once, and on a
    Unix socket a send after that close fails with a broken pipe. Counting
    that as blocked made the control outside the sandbox fail by timing
    alone. Inside the sandbox the connection itself must fail, so this only
    makes the check there stricter.
    """
    try:
        with socket.socket(family, socket.SOCK_STREAM) as probe:
            probe.settimeout(3)
            probe.connect(address)
            try:
                probe.sendall(b"probe")
            except (BrokenPipeError, ConnectionResetError):
                pass
        return "connected"
    except OSError as error:
        return f"blocked: {type(error).__name__}: {error.strerror or error}"


def main():
    port = int(sys.argv[1])
    abstract = sys.argv[2]
    results = {
        "tcp": attempt(socket.AF_INET, ("127.0.0.1", port)),
        "abstract": attempt(socket.AF_UNIX, "\0" + abstract),
    }
    for path in sys.argv[3:]:
        results[path] = attempt(socket.AF_UNIX, path)
    print(json.dumps(results, sort_keys=True))


if __name__ == "__main__":
    main()

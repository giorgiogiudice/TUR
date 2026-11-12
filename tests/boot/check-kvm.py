#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""Prove that this user can use KVM, before anything expensive is built.

    tests/boot/check-kvm.py [RESULT.json]

Opens /dev/kvm read-write and asks the kernel for the KVM API version. Exits
non-zero with the reason if that fails. The boot test never falls back to
software emulation (ADR-0002), so a runner without KVM must fail here.
"""

from __future__ import annotations

import fcntl
import json
import os
import stat
import sys

KVM_GET_API_VERSION = 0xAE00  # _IO(KVMIO, 0x00)


def main() -> int:
    result: dict = {"device": "/dev/kvm", "usable": False}
    flags = set()
    try:
        with open("/proc/cpuinfo") as cpuinfo:
            for line in cpuinfo:
                if line.startswith("flags"):
                    flags = set(line.split(":", 1)[1].split())
                    break
    except OSError:
        pass
    result["cpu_virtualisation"] = "vmx" if "vmx" in flags else "svm" if "svm" in flags else None
    result["runner_is_virtual_machine"] = "hypervisor" in flags
    try:
        info = os.stat("/dev/kvm")
        result["mode"] = stat.filemode(info.st_mode)
        result["owner"] = f"{info.st_uid}:{info.st_gid}"
        fd = os.open("/dev/kvm", os.O_RDWR | os.O_CLOEXEC)
        try:
            result["api_version"] = fcntl.ioctl(fd, KVM_GET_API_VERSION)
        finally:
            os.close(fd)
        result["usable"] = result["api_version"] == 12
        if not result["usable"]:
            result["error"] = f"unexpected KVM API version {result['api_version']}"
    except OSError as error:
        result["error"] = str(error)
    print(json.dumps(result, indent=2))
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w") as out:
            json.dump(result, out, indent=2)
            out.write("\n")
    if not result["usable"]:
        print(f"KVM is not usable on this machine: {result.get('error')}", file=sys.stderr)
        return 1
    print("KVM is usable")
    return 0


if __name__ == "__main__":
    sys.exit(main())

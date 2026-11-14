# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read-only access to a disk, a partition or an image file.

Everything the check-list knows about a Windows disk is read through this
class, and it only ever opens for reading. Reads bypass the page cache
(O_DIRECT) where the kernel allows it: the guest writes to the same
partitions through device-mapper with direct I/O, which does not invalidate
what a buffered reader cached earlier, and a safety check must never decide
on stale blocks.
"""

from __future__ import annotations

import fcntl
import mmap
import os
import stat

ALIGN = 4096
BLKFLSBUF = 0x1261
BLKSSZGET = 0x1268


class Dev:
    """A read-only byte range [base, base + size) of a file or block device."""

    def __init__(self, path: str, base: int = 0, size: int | None = None):
        self.path = path
        self.base = base
        self.direct = True
        try:
            self.fd = os.open(path, os.O_RDONLY | os.O_DIRECT | os.O_CLOEXEC)
        except OSError:
            self.direct = False
            self.fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
        mode = os.fstat(self.fd).st_mode
        self.is_block = stat.S_ISBLK(mode)
        self.sector = 512
        if self.is_block:
            try:
                fcntl.ioctl(self.fd, BLKFLSBUF)  # drop cached clean pages; needs root, else ignored
            except OSError:
                pass
            try:
                raw = fcntl.ioctl(self.fd, BLKSSZGET, b"\0\0\0\0")
                self.sector = int.from_bytes(raw, "little") or 512
            except OSError:
                pass
        total = os.lseek(self.fd, 0, os.SEEK_END)
        self.size = (total - base) if size is None else size
        if self.size < 0 or base + self.size > total:
            raise ValueError(f"{path}: range {base}+{self.size} is outside the device ({total} bytes)")

    def sub(self, offset: int, size: int) -> "Dev":
        """A view of part of this range (a partition inside a disk)."""
        if offset < 0 or offset + size > self.size:
            raise ValueError("partition lies outside the disk")
        view = object.__new__(Dev)
        view.__dict__.update(self.__dict__)
        view.base = self.base + offset
        view.size = size
        return view

    def pread(self, offset: int, length: int) -> bytes:
        if offset < 0 or length < 0 or offset + length > self.size:
            raise ValueError(f"read {offset}+{length} outside the range ({self.size} bytes)")
        if length == 0:
            return b""
        absolute = self.base + offset
        if not self.direct:
            data = os.pread(self.fd, length, absolute)
        else:
            start = absolute - absolute % ALIGN
            end = absolute + length
            span = end - start
            span += -span % ALIGN
            buffer = mmap.mmap(-1, span)
            try:
                got = os.preadv(self.fd, [buffer], start)
                data = bytes(buffer[absolute - start:min(got, absolute - start + length)])
            except OSError:
                # Some filesystems refuse direct reads at the end of a file.
                self.direct = False
                data = os.pread(self.fd, length, absolute)
            finally:
                buffer.close()
        if len(data) != length:
            raise OSError(f"short read at {offset}: {len(data)} of {length} bytes")
        return data

    def close(self) -> None:
        try:
            os.close(self.fd)
        except OSError:
            pass

    def __enter__(self) -> "Dev":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

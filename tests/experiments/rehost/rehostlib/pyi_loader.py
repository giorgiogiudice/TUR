# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Runs a PyInstaller payload under an ordinary CPython of the same minor
version (the experiment's "frozen Python" case).

    pythonX.Y -I pyi_loader.py OUT SCRIPT [arguments...]

OUT holds what rehost.py took out of the Windows or macOS program:
payload.pyz (the frozen pure-Python modules, as bytecode), SCRIPT (the entry
point, as a code object or as source), data/ (the data files).

Modules are looked for in this interpreter first (its standard library, and
the packages installed for the extension modules the program needs), then in
the PYZ archive. Bytecode is only valid for the Python minor version that
wrote it, which is why the interpreter must match.
"""

import importlib.abc
import importlib.util
import marshal
import os
import struct
import sys
import types
import zlib


def main():
    out, script = sys.argv[1], sys.argv[2]
    data_dir = os.path.join(out, "data")
    with open(os.path.join(out, "payload.pyz"), "rb") as handle:
        blob = handle.read()
    if blob[:4] != b"PYZ\0":
        sys.exit("pyi_loader: payload.pyz is not a PYZ archive")
    toc_offset = struct.unpack("!I", blob[8:12])[0]
    raw = marshal.loads(blob[toc_offset:])
    toc = dict(raw) if not isinstance(raw, dict) else raw

    class Finder(importlib.abc.MetaPathFinder, importlib.abc.Loader):
        def find_spec(self, fullname, path=None, target=None):
            entry = toc.get(fullname)
            if entry is None:
                return None
            is_package = entry[0] in (1, 3)
            spec = importlib.util.spec_from_loader(fullname, self, is_package=is_package)
            if is_package:
                spec.submodule_search_locations = [os.path.join(data_dir, *fullname.split("."))]
            return spec

        def create_module(self, spec):
            return None

        def exec_module(self, module):
            kind, position, length = toc[module.__name__]
            location = os.path.join(data_dir, *module.__name__.split("."))
            module.__file__ = os.path.join(location, "__init__.py") if kind in (1, 3) else location + ".py"
            if kind == 3:  # a namespace package: nothing to run
                return
            code = marshal.loads(zlib.decompress(blob[position : position + length]))
            exec(code, module.__dict__)

        def get_data(self, path):
            with open(path, "rb") as handle:
                return handle.read()

    sys.meta_path.append(Finder())
    sys.path.insert(0, data_dir)
    sys.frozen = True
    sys._MEIPASS = data_dir
    with open(os.path.join(out, script), "rb") as handle:
        entry = handle.read()
    try:
        code = marshal.loads(entry)
        if not isinstance(code, types.CodeType):
            raise ValueError("not a code object")
    except (ValueError, EOFError, TypeError):
        code = compile(entry, script, "exec")
    module = types.ModuleType("__main__")
    module.__file__ = os.path.join(data_dir, script)
    sys.modules["__main__"] = module
    sys.argv = [os.path.join(out, script)] + sys.argv[3:]
    exec(code, module.__dict__)


if __name__ == "__main__":
    main()

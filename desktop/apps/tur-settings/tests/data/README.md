<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Test data of tur-settings

`registry/org.tur.Example.json` is the settings registry of a program that
does not exist: "another TUR program with addressable content". The tests give
its directory to tur-settings (`--registry`, `Registry.directory`) to show
what the program does with an address that is not its own:

- `tur.example#thing` is passed on to the program that owns it
  (`tests/unit/peer.cpp` stands in for it on the test's bus);
- `tur.example#old-thing` was removed and names its successor;
- `tur.example.gone` was removed and names none.

It is not one of the registries `desktop/ci/registry.py` checks against the
main branch: those are the files in `desktop/apps/*/registry/`.

<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# Wine patch set for KMDF drivers in user space

**Not under TUR's licence.** These are patches to Wine and are under Wine's
licence, the **GNU Lesser General Public License, version 2.1 or later**
([COPYING.LIB](COPYING.LIB), Wine's file, verbatim). Written for the
experiment in
[docs/reports/windows-drivers-2.md](../../docs/reports/windows-drivers-2.md),
in Wine's style, each small and meant to be proposed upstream.

| | |
|---|---|
| Against | Wine `wine-11.20` (`7298a818b79446bae153b8d5aa27235ab3703bf9`), the version Fedora 45 ships; Fedora's Staging patch set touches none of the files patched here |
| Also checked against | Wine master `cc5d1d86ee7d60649e6f858106db046c77c8b12c` (`build-wine.sh` reports whether each patch applies) |
| Applied by | `tests/windows-drivers/kmdf/build-wine.sh`, with `git apply`, in the order of [`series`](series) |
| Built | only the modules they touch (`ntoskrnl.exe`, `wdfldr.sys`), in CI; the experiment puts them over Fedora's in a throw-away container. Never shipped |

Each `.patch` file is the output of `git format-patch` with a licence note
above it, which `git apply` ignores. The author line is a placeholder: Wine
takes contributions under a real name, so an upstream submission is made by
a person.

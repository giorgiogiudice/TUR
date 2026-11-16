#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Writes image/archive-tools/tools.lock: Fedora's innoextract and msitools,
which tur-rehost uses to open Inno Setup installers and Windows Installer
packages without running them, and what they need that the base image lacks,
each pinned by the SHA-256 of the signed package. 7-Zip is in the base.

    image/archive-tools/lock.py --installed PACKAGES.csv [--out image/archive-tools/tools.lock]

It is image/wine/lock.py of pull request 136 (Wine in the image) with other
roots and another output; when that change merges, the two can become one
script with a list of roots per lock.

PACKAGES.csv is the list of packages the base image has, as
scripts/sbom-licences.py writes it from the image's SBOM (the first column
is read). The tool reads Fedora 45's repository metadata for x86-64 (the
release tree and the updates repository; never updates-testing), takes the
newest build of each package, and resolves the dependencies of ROOTS
against what is installed, the way rpm will when image/archive-tools/tools.sh
installs the result: every requirement must be met by the base or by a
package of the lock. Recommendations are not followed (the image is built
with weak dependencies off). Each package gets two places to come from: its
path in Fedora's repository and the signed copy in Fedora's build system
(Koji), which keeps a build after the updates repository has moved on.

It needs Python 3 and the `zstd` program, and the network. It reads metadata
and writes one file; it builds and installs nothing, so it may be run on a
development machine. Run it when the base pin moves (image/base.lock), and
commit the lock in a change of its own.
"""

from __future__ import annotations

import argparse
import csv
import datetime
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import tempfile
import urllib.request
import xml.etree.ElementTree as ET

HERE = pathlib.Path(__file__).resolve().parent
FEDORA = "https://dl.fedoraproject.org/pub/fedora/linux/"
KOJI = "https://kojipkgs.fedoraproject.org/packages/"
REPOSITORIES = {
    # name: path under FEDORA. Newest build wins; updates before release.
    "updates": "updates/45/Everything/x86_64/",
    "release": "releases/45/Everything/x86_64/os/",
}

# What TUR installs (image/README.md, "Archive tools for re-hosting"). 7-Zip
# (`7zip`, which provides `7zz`) is in the base image already and is not
# locked here.
ROOTS = [
    "innoextract.x86_64",  # Inno Setup installers, opened without running them
    "msitools.x86_64",     # msiextract: Windows Installer packages, opened without running them
]

NS = "{http://linux.duke.edu/metadata/common}"
NS_REPO = "{http://linux.duke.edu/metadata/repo}"
RPM = "{http://linux.duke.edu/metadata/rpm}"


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "tur-archive-tools-lock/1"})
    with urllib.request.urlopen(request, timeout=300) as response:
        return response.read()


def load(db: sqlite3.Connection, repo: str, base: str, scratch: pathlib.Path) -> str:
    """Loads one repository's primary metadata; returns its revision."""
    repomd = ET.fromstring(fetch(FEDORA + base + "repodata/repomd.xml"))
    revision = repomd.findtext(NS_REPO + "revision") or "?"
    data = next(d for d in repomd.findall(NS_REPO + "data") if d.get("type") == "primary")
    href = data.find(NS_REPO + "location").get("href")
    compressed = scratch / f"{repo}-primary.xml.zst"
    compressed.write_bytes(fetch(FEDORA + base + href))
    unpack = subprocess.Popen(["zstd", "-dc", str(compressed)], stdout=subprocess.PIPE)
    for _, element in ET.iterparse(unpack.stdout, events=("end",)):
        if element.tag != NS + "package":
            continue
        version = element.find(NS + "version")
        fmt = element.find(NS + "format")
        checksum = element.find(NS + "checksum")
        if element.findtext(NS + "arch") not in ("x86_64", "noarch") or checksum.get("type") != "sha256":
            element.clear()
            continue
        cursor = db.execute(
            "insert into pkg(repo,name,arch,epoch,ver,rel,size,loc,sha256,licence,srpm) values(?,?,?,?,?,?,?,?,?,?,?)",
            (repo, element.findtext(NS + "name"), element.findtext(NS + "arch"), version.get("epoch"),
             version.get("ver"), version.get("rel"), int(element.find(NS + "size").get("package")),
             base + element.find(NS + "location").get("href"), checksum.text, fmt.findtext(RPM + "license"),
             fmt.findtext(RPM + "sourcerpm")))
        identifier = cursor.lastrowid
        for tag, table in (("provides", "prov"), ("requires", "req")):
            entries = fmt.find(RPM + tag)
            for entry in entries if entries is not None else []:
                db.execute(f"insert into {table} values(?,?)", (identifier, entry.get("name")))
        for path in fmt.findall(NS + "file"):
            db.execute("insert into prov values(?,?)", (identifier, path.text))
        element.clear()
    unpack.wait()
    return revision


def evr_key(row: sqlite3.Row) -> tuple:
    def parts(text: str) -> list:
        return [(0, int(p)) if p.isdigit() else (1, p) for p in re.findall(r"\d+|[A-Za-z]+", text or "")]
    return (int(row["epoch"] or 0), parts(row["ver"]), parts(row["rel"]))


def newest(rows: list[sqlite3.Row]) -> sqlite3.Row:
    return max(rows, key=lambda row: (evr_key(row), row["repo"] == "updates"))


def koji_path(row: sqlite3.Row) -> str:
    """The signed copy in Koji: packages/<source>/<version>/<release>/data/signed/<key>/<arch>/<file>."""
    match = re.fullmatch(r"(.+)-([^-]+)-([^-]+)\.src\.rpm", row["srpm"])
    source, version, release = match.groups()
    build = f"{source}/{version}/{release}/"
    listing = fetch(KOJI + build + "data/signed/").decode(errors="replace")
    keys = sorted(set(re.findall(r'href="([0-9a-f]{8})/"', listing)))
    if not keys:
        raise SystemExit(f"Koji has no signed copy of {row['srpm']}")
    return f"{build}data/signed/{keys[-1]}/{row['arch']}/{row['loc'].rsplit('/', 1)[1]}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--installed", required=True, type=pathlib.Path)
    parser.add_argument("--out", type=pathlib.Path, default=HERE / "tools.lock")
    arguments = parser.parse_args()
    with arguments.installed.open() as handle:
        installed = {row[0] for row in csv.reader(handle) if row and row[0] != "package"}
    if len(installed) < 500:
        raise SystemExit(f"{arguments.installed} lists {len(installed)} packages: not an image's list")

    with tempfile.TemporaryDirectory(prefix="tur-archive-tools-lock-") as scratch_name:
        scratch = pathlib.Path(scratch_name)
        db = sqlite3.connect(scratch / "repo.db")
        db.row_factory = sqlite3.Row
        db.executescript("""
            create table pkg(id integer primary key, repo, name, arch, epoch, ver, rel, size integer, loc,
                             sha256, licence, srpm);
            create table prov(pid integer, name);
            create table req(pid integer, name);
        """)
        revisions = {repo: load(db, repo, base, scratch) for repo, base in REPOSITORIES.items()}
        db.execute("create index p on prov(name)")
        db.execute("create index n on pkg(name)")

        def package(name: str, arch: str | None = None) -> sqlite3.Row:
            rows = db.execute("select * from pkg where name=?", (name,)).fetchall()
            rows = [row for row in rows if arch is None or row["arch"] == arch]
            if not rows:
                raise SystemExit(f"no package {name} ({arch or 'any architecture'}) in Fedora 45")
            return newest(rows)

        chosen: dict[str, sqlite3.Row] = {}
        why: dict[str, str] = {}
        queue = []
        for root in ROOTS:
            name, _, arch = root.rpartition(".")
            queue.append((package(name, arch), "wanted"))
        def providers(capability: str) -> set[str]:
            return {r["name"] for r in db.execute(
                "select pkg.name from prov join pkg on pkg.id=prov.pid where prov.name=?", (capability,))}

        def present(capability: str) -> bool:
            return bool(providers(capability) & (installed | set(chosen)))

        # Conditional dependencies, "(A if B)": A is needed once B is there,
        # whether B is the base's or the lock's. They are looked at again
        # whenever the lock grows, until nothing changes.
        conditional: list[tuple[str, str, str]] = []

        def need(capability: str, wanted_by: str) -> None:
            if capability.startswith("rpmlib("):
                return  # rpm's own features
            if capability.startswith("("):
                match = re.fullmatch(r"\((\S+)(?: [<>=]+ \S+)? if (\S+)(?: [<>=]+ \S+)?\)", capability)
                if not match:
                    raise SystemExit(f"{wanted_by} needs {capability}, which this tool cannot read")
                conditional.append((match.group(1), match.group(2), wanted_by))
                return
            names = providers(capability)
            if not names:
                raise SystemExit(f"{wanted_by} needs {capability}, which no package provides")
            if names & installed or names & set(chosen):
                return
            if len(names) > 1:
                print(f"note: {capability} ({wanted_by}) is provided by {sorted(names)}; "
                      f"taking {sorted(names)[0]}", file=sys.stderr)
            queue.append((package(sorted(names)[0]), f"{wanted_by} needs {capability}"))

        while True:
            while queue:
                row, reason = queue.pop(0)
                if row["name"] in chosen or row["name"] in installed:
                    continue
                chosen[row["name"]], why[row["name"]] = row, reason
                for (capability,) in db.execute("select distinct name from req where pid=?", (row["id"],)):
                    need(capability, row["name"])
            for wanted, condition, wanted_by in list(conditional):
                if present(condition) and not present(wanted):
                    need(wanted, f"{wanted_by} (as {condition} is installed)")
            if not queue:
                break

        today = datetime.date.today().isoformat()
        roots = ", ".join(f"{chosen[name]['name']} {chosen[name]['ver']}-{chosen[name]['rel']}"
                          for name in ("innoextract", "msitools"))
        lines = [
            "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
            "#",
            "# Archive tools for re-hosting (image/README.md, \"Archive tools for",
            "# re-hosting\"): Fedora's innoextract and msitools, and what they need that",
            "# the base image lacks, each pinned by the SHA-256 of the signed package.",
            "# Written by image/archive-tools/lock.py; do not edit by hand. The fetch",
            "# step (image/archive-tools/tools.sh) takes each from Fedora's repository",
            "# or, once the updates repository has moved on, from the signed copy in",
            "# Fedora's build system, and refuses one whose checksum differs.",
            "#",
            f"# {roots}; {len(chosen)} packages, "
            f"{sum(r['size'] for r in chosen.values())} bytes to download.",
            f"# Resolved on {today} against the packages of {arguments.installed.name}, from",
            f"# Fedora 45 x86_64: updates revision {revisions['updates']}, release revision {revisions['release']}.",
            "#",
            "# file\tsha256\tsize\tlicence\tpath under " + FEDORA + "\tpath under " + KOJI + "\twhy",
        ]
        for name in sorted(chosen):
            row = chosen[name]
            lines.append("\t".join([row["loc"].rsplit("/", 1)[1], row["sha256"], str(row["size"]),
                                    row["licence"], row["loc"], koji_path(row), why[name]]))
        arguments.out.write_text("\n".join(lines) + "\n")
        print(f"{arguments.out}: {len(chosen)} packages, {roots}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

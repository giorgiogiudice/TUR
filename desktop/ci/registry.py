#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Check the settings registries of TUR's desktop programs.

    registry.py                      check every registry under desktop/apps/*/registry/
    registry.py --against REF        also: no address that REF had has disappeared
    registry.py --self-test          prove that the check catches what it should

Every settings page and every row on it has an address that keeps its meaning
for ever (docs/architecture/desktop-experience.md, section 10.3). A program's
registry file lists them; the format is schemas/settings-registry.schema.json.
This script applies the schema and the rules a schema cannot express:

  - a file is named after its program;
  - a page address occurs once in all registries, a row identifier once on
    its page, a section identifier once on its page;
  - the last item of a page's path is its title;
  - a row names a section its page declares, when the page declares any;
  - a page of the kde namespace says how it is opened, and a page of the tur
    namespace does not;
  - a page that is planned or removed has no row that is still there;
  - replaced_by, and the address a row opens, name an address that exists
    and is not removed;
  - with --against: every address REF's registries had is still registered,
    and one that was removed has not come back.

The program has its own loader (desktop/apps/tur-settings/src/registrydata.cpp)
with the same rules; a unit test holds it to the same files.

Needs the Python standard library, python3-jsonschema, and git for --against.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "schemas" / "settings-registry.schema.json"
PATTERN = "desktop/apps/*/registry/*.json"


def schema_errors(document, schema) -> list[str]:
    import jsonschema  # python3-jsonschema; the boundary job installs it

    validator = jsonschema.Draft202012Validator(schema)
    out = []
    for error in sorted(validator.iter_errors(document), key=lambda e: list(map(str, e.absolute_path))):
        where = "/".join(str(part) for part in error.absolute_path) or "(root)"
        out.append(f"{where}: {error.message[:300]}")
    return out


def addresses_of(document) -> dict[str, str]:
    """Every address a registry holds, with its status."""
    out: dict[str, str] = {}
    for page in document.get("pages", []):
        if not isinstance(page, dict) or not isinstance(page.get("page"), str):
            continue
        out[page["page"]] = page.get("status", "available")
        for row in page.get("rows", []) or []:
            if isinstance(row, dict) and isinstance(row.get("id"), str):
                out[f"{page['page']}#{row['id']}"] = row.get("status", "available")
    return out


def check(documents: dict[str, object], schema) -> list[str]:
    """Every problem in a set of registries: {file name: parsed JSON}."""
    problems: list[str] = []
    known: dict[str, str] = {}
    owners: dict[str, str] = {}

    for name, document in sorted(documents.items()):
        for message in schema_errors(document, schema):
            problems.append(f"{name}: {message}")
        if not isinstance(document, dict):
            continue
        program = document.get("program")
        if isinstance(program, str) and pathlib.PurePosixPath(name).name != program + ".json":
            problems.append(f"{name}: the file of program {program} is named {program}.json")
        for address, status in addresses_of(document).items():
            if "#" not in address and address in owners:
                problems.append(f"{name}: the page {address} is already registered in {owners[address]}")
            if "#" not in address:
                owners[address] = name
            known.setdefault(address, status)

    for name, document in sorted(documents.items()):
        if not isinstance(document, dict):
            continue
        for page in document.get("pages", []):
            if not isinstance(page, dict) or not isinstance(page.get("page"), str):
                continue
            address = page["page"]
            where = f"{name}: {address}"
            status = page.get("status", "available")
            path = page.get("path")
            if isinstance(path, list) and path and path[-1] != page.get("title"):
                problems.append(f"{where}: the last item of its path is not its title")
            if address.startswith("kde.") and "open" not in page and status == "available":
                problems.append(f"{where}: a page of KDE System Settings says how it is opened (open)")
            if address.startswith("tur.") and "open" in page:
                problems.append(f"{where}: a page of a TUR program is opened by its address, not by open")

            sections = [s.get("id") for s in page.get("sections", []) or [] if isinstance(s, dict)]
            for identifier in sorted(set(sections)):
                if sections.count(identifier) > 1:
                    problems.append(f"{where}: the section {identifier} is declared twice")

            seen: set[str] = set()
            for row in page.get("rows", []) or []:
                if not isinstance(row, dict) or not isinstance(row.get("id"), str):
                    continue
                row_where = f"{where}#{row['id']}"
                row_status = row.get("status", "available")
                if row["id"] in seen:
                    problems.append(f"{row_where}: the identifier is used twice on the page")
                seen.add(row["id"])
                if status != "available" and row_status != "removed":
                    problems.append(f"{row_where}: its page is {status}, so the row cannot be there")
                if row_status != "removed":
                    if sections and row.get("section") not in sections:
                        problems.append(f"{row_where}: it names no section of its page ({', '.join(sections)})")
                    if not sections and "section" in row:
                        problems.append(f"{row_where}: it names a section and its page declares none")
                targets = [("replaced_by", row.get("replaced_by"))]
                source = row.get("source")
                if isinstance(source, dict) and source.get("type") == "address":
                    targets.append(("source.opens", source.get("opens")))
                for field, target in targets:
                    problems += target_problems(row_where, field, target, f"{address}#{row['id']}", known)
            problems += target_problems(where, "replaced_by", page.get("replaced_by"), address, known)
    return problems


def target_problems(where: str, field: str, target, own: str, known: dict[str, str]) -> list[str]:
    if target is None:
        return []
    if target == own:
        return [f"{where}: {field} names the address itself"]
    if target not in known:
        return [f"{where}: {field} names {target}, which no registry has"]
    if known[target] == "removed":
        return [f"{where}: {field} names {target}, which is itself removed"]
    return []


def compare(before: dict[str, str], now: dict[str, str]) -> list[str]:
    """What happened to the addresses `before` had: nothing may be lost."""
    problems = []
    for address, status in sorted(before.items()):
        if address not in now:
            problems.append(f"{address} was registered and is gone. An address is never deleted: mark it "
                            "\"status\": \"removed\", with replaced_by where it has a successor")
        elif status == "removed" and now[address] != "removed":
            problems.append(f"{address} was removed and has come back. An address is never reused")
    return problems


def load_working_tree() -> tuple[dict[str, object], list[str]]:
    documents: dict[str, object] = {}
    problems: list[str] = []
    for path in sorted(ROOT.glob(PATTERN)):
        name = path.relative_to(ROOT).as_posix()
        try:
            documents[name] = json.loads(path.read_text(encoding="utf-8"))
        except (ValueError, UnicodeDecodeError) as error:
            problems.append(f"{name}: not JSON: {error}")
    return documents, problems


def load_ref(ref: str) -> dict[str, str] | None:
    """Every address in the registries of a git revision; None if it cannot be read."""
    listing = subprocess.run(["git", "-C", str(ROOT), "ls-tree", "-r", "--name-only", ref, "--", "desktop/apps"],
                             capture_output=True, text=True, check=False)
    if listing.returncode != 0:
        print(listing.stderr.strip(), file=sys.stderr)
        return None
    out: dict[str, str] = {}
    for name in listing.stdout.splitlines():
        if not pathlib.PurePosixPath(name).match(PATTERN):
            continue
        shown = subprocess.run(["git", "-C", str(ROOT), "show", f"{ref}:{name}"],
                               capture_output=True, text=True, check=False)
        if shown.returncode != 0:
            print(shown.stderr.strip(), file=sys.stderr)
            return None
        try:
            out.update(addresses_of(json.loads(shown.stdout)))
        except ValueError:
            # A registry that was broken at REF has no addresses to keep.
            continue
    return out


def self_test(schema) -> int:
    """Plant one mistake of each kind and require that each is reported."""
    good = {
        "licence": "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
        "schema_version": 1,
        "program": "org.tur.Example",
        "pages": [
            {
                "page": "tur.example",
                "title": "Example",
                "path": ["Example"],
                "sections": [{"id": "main", "title": "Main"}],
                "rows": [
                    {"id": "one", "title": "One", "kind": "switch", "section": "main",
                     "source": {"type": "dbus", "available": True, "bus": "session", "service": "org.tur.Example"},
                     "change": {"class": 1, "tool": "example_set_one"}},
                    {"id": "two", "title": "Two", "kind": "information", "section": "main",
                     "source": {"type": "file", "available": False, "waits_for": "The file", "path": "/etc/two"}},
                    {"id": "old", "title": "Old", "status": "removed", "replaced_by": "tur.example#one"},
                ],
            },
            {"page": "tur.example.later", "title": "Later", "path": ["Example", "Later"], "status": "planned"},
            {"page": "kde.kcm_example", "title": "Example module", "path": ["System Settings", "Example module"],
             "open": {"module": "kcm_example"}, "verified_with": "plasma-6.8.2"},
        ],
    }
    name = "desktop/apps/example/registry/org.tur.Example.json"

    def changed(edit):
        document = json.loads(json.dumps(good))
        edit(document)
        return document

    def page(document, index=0):
        return document["pages"][index]

    def row(document, index=0):
        return document["pages"][0]["rows"][index]

    cases = {
        "the good registry": (lambda d: None, None),
        "an unknown key": (lambda d: d.update(colour="blue"), "Additional properties"),
        "another schema version": (lambda d: d.update(schema_version=2), "schema_version"),
        "an address in upper case": (lambda d: page(d).update(page="tur.Example"), "does not match"),
        "an address outside the two namespaces": (lambda d: page(d).update(page="gnome.example"), "does not match"),
        "a row identifier with a dot": (lambda d: row(d).update(id="one.two"), "does not match"),
        "a blank title": (lambda d: row(d).update(title="   "), "does not match"),
        "an unknown kind": (lambda d: row(d).update(kind="slider"), "is not one of"),
        "a switch that does not say what changing it is": (lambda d: row(d).pop("change"), "'change' is a required"),
        "information with a change": (lambda d: row(d, 1).update(change={"class": 0}), "should not be valid"),
        "class 5": (lambda d: row(d)["change"].update({"class": 5}), "maximum"),
        "a source that is missing and does not say what it waits for":
            (lambda d: row(d, 1)["source"].pop("waits_for"), "waits_for"),
        "a source of an unknown type": (lambda d: row(d)["source"].update(type="magic"), "is not one of"),
        "a command given as one string":
            (lambda d: row(d).update(source={"type": "command", "available": True, "program": "ls -l"}),
             "is not valid under any of the given schemas"),
        "a removed row that still has a kind": (lambda d: row(d, 2).update(kind="switch"), "False schema"),
        "replaced_by on a row that is still there": (lambda d: row(d).update(replaced_by="tur.example#two"),
                                                     "should not be valid"),
        "a file not named after its program": (lambda d: d.update(program="org.tur.Other"), "is named"),
        "a row identifier used twice": (lambda d: row(d, 1).update(id="one"), "used twice"),
        "a section declared twice": (lambda d: page(d)["sections"].append({"id": "main", "title": "Again"}),
                                     "declared twice"),
        "a row in a section its page does not have": (lambda d: row(d).update(section="other"), "names no section"),
        "a path that does not end in the title": (lambda d: page(d).update(path=["Something else"]), "its path"),
        "a KDE page that does not say how it is opened": (lambda d: page(d, 2).pop("open"), "how it is opened"),
        "a TUR page with open": (lambda d: page(d).update(open={"module": "kcm_x"}), "by its address"),
        "a planned page with a row":
            (lambda d: page(d, 1).update(rows=[{"id": "x", "title": "X", "kind": "information"}]), "cannot be there"),
        "replaced_by naming nothing": (lambda d: row(d, 2).update(replaced_by="tur.example#nothing"),
                                       "which no registry has"),
        "replaced_by naming itself": (lambda d: row(d, 2).update(replaced_by="tur.example#old"), "the address itself"),
        "a row that opens an address nobody has":
            (lambda d: row(d).update(source={"type": "address", "available": True, "opens": "kde.kcm_missing"}),
             "which no registry has"),
    }
    failures = []
    for label, (edit, expected) in cases.items():
        problems = check({name: changed(edit)}, schema)
        if expected is None and problems:
            failures.append(f"{label}: is right but was reported: {problems}")
        if expected is not None and not any(expected in problem for problem in problems):
            failures.append(f"{label}: should be reported with {expected!r}; got {problems}")

    twice = check({name: good, "desktop/apps/other/registry/org.tur.Example.json": good}, schema)
    if not any("already registered" in problem for problem in twice):
        failures.append(f"a page registered by two files: not reported; got {twice}")

    before = {"tur.a": "available", "tur.a#x": "available", "tur.b": "removed"}
    lost = compare(before, {"tur.a": "available", "tur.b": "removed"})
    if len(lost) != 1 or "tur.a#x was registered and is gone" not in lost[0]:
        failures.append(f"an address that disappeared: got {lost}")
    back = compare(before, {"tur.a": "available", "tur.a#x": "removed", "tur.b": "available"})
    if len(back) != 1 or "tur.b was removed and has come back" not in back[0]:
        failures.append(f"a removed address that came back: got {back}")
    if compare(before, {"tur.a": "available", "tur.a#x": "removed", "tur.b": "removed", "tur.c": "available"}):
        failures.append("an address marked removed, and a new one: is right but was reported")

    # And through git, as --against does it: a revision that had an address
    # the working tree no longer has.
    with tempfile.TemporaryDirectory(prefix="tur-registry-") as holder:
        repository = pathlib.Path(holder)
        target = repository / name
        target.parent.mkdir(parents=True)
        target.write_text(json.dumps(good), encoding="utf-8")
        environment = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid", "GIT_COMMITTER_NAME": "t",
                       "GIT_COMMITTER_EMAIL": "t@example.invalid", "GIT_CONFIG_GLOBAL": "/dev/null",
                       "GIT_CONFIG_SYSTEM": "/dev/null", "PATH": "/usr/bin:/bin", "HOME": holder}
        for command in (["init", "--quiet"], ["add", "."], ["commit", "--quiet", "-m", "registry"]):
            subprocess.run(["git", "-C", holder, *command], check=True, env=environment, capture_output=True)
        global ROOT
        saved, ROOT = ROOT, repository
        try:
            had = load_ref("HEAD")
        finally:
            ROOT = saved
        if had is None or had.get("tur.example#one") != "available" or had.get("tur.example#old") != "removed":
            failures.append(f"reading the registries of a git revision: got {had}")

    if failures:
        for failure in failures:
            print(f"::error::registry self-test: {failure}")
        return 1
    print(f"registry self-test: {len(cases)} planted registries and the comparisons with an earlier revision "
          "judged as expected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--against", metavar="REF", help="a git revision whose addresses must all still exist")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    import jsonschema

    jsonschema.Draft202012Validator.check_schema(schema)
    if args.self_test:
        return self_test(schema)

    documents, problems = load_working_tree()
    if not documents and not problems:
        problems.append(f"no registry was found ({PATTERN})")
    problems += check(documents, schema)

    now: dict[str, str] = {}
    for document in documents.values():
        if isinstance(document, dict):
            now.update(addresses_of(document))

    compared = ""
    if args.against:
        before = load_ref(args.against)
        if before is None:
            problems.append(f"the registries of {args.against} could not be read")
        else:
            problems += compare(before, now)
            compared = (f"; {len(before)} address(es) of {args.against} are all still registered"
                        if before else f"; {args.against} has no registry yet, so there was nothing to lose")

    if problems:
        for problem in problems:
            print(f"::error::registry: {problem}")
        print(f"settings registries: {len(problems)} problem(s) in {len(documents)} file(s)")
        return 1
    pages = sum(1 for address in now if "#" not in address)
    print(f"settings registries: valid. {len(documents)} file(s), {pages} pages, {len(now) - pages} rows{compared}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

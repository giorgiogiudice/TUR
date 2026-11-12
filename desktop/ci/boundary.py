#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The licence boundary check for desktop/.

    boundary.py                  check the sources under desktop/
    boundary.py --linked DIR     check what the programs built in DIR link
    boundary.py --self-test      prove that the check catches what it should

TUR's desktop programs carry the project's noncommercial licence and must not
link or import GPL-only code (AGENTS.md, "Licence boundary rules";
docs/architecture/desktop-experience.md, section 4). The rules are data, in
desktop/ALLOWED_IMPORTS; this script applies them:

  - every file carries the project's SPDX identifier, except under
    desktop/upstream-licensed/<host>/, where every file carries the host's and
    none carries the project's; and no file outside it carries a GPL one;
  - every QML import, CMake package and target, and C++ include is allowed
    where it is used, and nothing forbidden appears anywhere;
  - no QApplication;
  - no colour literal in QML outside the generated token files, and no text a
    `deny-text` rule names;
  - with --linked: every shared library a built program needs is allowed.

It fails on anything it does not know, so a new dependency is always a
visible change to ALLOWED_IMPORTS. Needs only the Python standard library
(and, for --linked, readelf).
"""

from __future__ import annotations

import argparse
import fnmatch
import pathlib
import re
import subprocess
import sys
import tempfile

PROJECT_ID = "LicenseRef-TUR-Noncommercial-1.0.0"
SPDX = re.compile(r"SPDX-License-Identifier:\s*([^\s\"*<>]+(?:\s+(?:WITH|AND|OR)\s+[^\s\"*<>]+)*)")
HEADER_LINES = 5

# Files that cannot carry a comment. A reference image is a picture of what
# TUR's own code draws.
BINARY = {".png"}
# Not sources: never checked.
SKIPPED_DIRECTORIES = {"build", "__pycache__"}

QML_IMPORT = re.compile(r"^\s*import\s+([A-Za-z_][\w.]*)", re.MULTILINE)
QML_PATH_IMPORT = re.compile(r"^\s*import\s+\"([^\"]+)\"", re.MULTILINE)
QMLDIR_IMPORT = re.compile(r"^\s*(?:import|depends)\s+([A-Za-z_][\w.]*)", re.MULTILINE)
CPP_INCLUDE = re.compile(r"^\s*#\s*include\s*[<\"]([^>\"]+)[>\"]", re.MULTILINE)
CMAKE_COMMENT = re.compile(r"#.*$", re.MULTILINE)
CMAKE_FIND = re.compile(r"\bfind_package\s*\(([^)]*)\)", re.IGNORECASE)
CMAKE_TARGET = re.compile(r"\b([A-Za-z_][\w]*::[A-Za-z_][\w]*)\b")
CMAKE_LINK = re.compile(r"\b(?:target_link_libraries|tur_link_qml_modules)\s*\(([^)]*)\)", re.IGNORECASE)
CMAKE_KEYWORDS = {
    "REQUIRED", "COMPONENTS", "OPTIONAL_COMPONENTS", "QUIET", "CONFIG", "NO_MODULE", "EXACT", "MODULE",
    "PRIVATE", "PUBLIC", "INTERFACE", "GLOBAL", "NO_POLICY_SCOPE",
}
HEX_COLOUR = re.compile(r"[\"']#(?:[0-9A-Fa-f]{3,4}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})[\"']")
# A colour name as a whole string. Not the full SVG list: the common ones,
# which is where a hand-written colour would come from. "transparent" is not
# a colour.
NAMED_COLOUR = re.compile(
    r"[\"'](?:white|black|red|green|blue|yellow|orange|gold|goldenrod|gray|grey|lightgray|lightgrey|darkgray|"
    r"darkgrey|silver|cyan|magenta|purple|pink|brown|navy|teal|lime|maroon|olive|steelblue|dodgerblue|"
    r"royalblue|crimson|tomato|khaki|beige|ivory|white ?smoke)[\"']", re.IGNORECASE)
COLOUR_FUNCTION = re.compile(r"\bQt\.(?:hsla|hsva|color)\s*\(|\bQt\.rgba\s*\(\s*[0-9.]")


class Rules:
    def __init__(self, text: str):
        self.allow: list[tuple[str, str, list[str], str]] = []
        self.forbid: list[tuple[str, str, str]] = []
        self.literals: list[str] = []
        self.deny_text: list[tuple[list[str], re.Pattern, str]] = []
        for number, raw in enumerate(text.splitlines(), 1):
            line = raw.split("#", 1)[0].strip() if not raw.lstrip().startswith("deny-text") else raw.strip()
            if not line:
                continue
            words = line.split()
            kind = words[0]
            try:
                if kind == "allow":
                    self.allow.append((words[1], words[2], words[3].split(","), " ".join(words[4:])))
                    if not words[4:]:
                        raise IndexError
                elif kind == "forbid":
                    self.forbid.append((words[1], words[2], " ".join(words[3:])))
                    if not words[3:]:
                        raise IndexError
                elif kind == "literals":
                    self.literals += words[1].split(",")
                elif kind == "deny-text":
                    self.deny_text.append((words[1].split(","), re.compile(words[2]), " ".join(words[3:])))
                else:
                    raise ValueError
            except (IndexError, ValueError, re.error):
                raise SystemExit(f"ALLOWED_IMPORTS line {number} is not a rule: {raw!r}") from None

    def judge(self, kind: str, name: str, path: str) -> str | None:
        """None if `name` may be used in `path`; otherwise why not."""
        for rule_kind, pattern, reason in self.forbid:
            if rule_kind == kind and fnmatch.fnmatchcase(name, pattern):
                return f"forbidden: {reason}"
        elsewhere = False
        for rule_kind, pattern, paths, _licence in self.allow:
            if rule_kind == kind and fnmatch.fnmatchcase(name, pattern):
                if matches(path, paths):
                    return None
                elsewhere = True
        if elsewhere:
            return "allowed elsewhere, but not in this file (see ALLOWED_IMPORTS)"
        return "not in ALLOWED_IMPORTS"


def matches(path: str, patterns: list[str]) -> bool:
    included = any(fnmatch.fnmatchcase(path, p) for p in patterns if not p.startswith("!"))
    excluded = any(fnmatch.fnmatchcase(path, p[1:]) for p in patterns if p.startswith("!"))
    return included and not excluded


def spdx_of(text: str) -> list[str]:
    head = "\n".join(text.splitlines()[:HEADER_LINES])
    return [match.group(1) for match in SPDX.finditer(head)]


def cmake_names(text: str) -> set[str]:
    """Packages and targets a CMake file names: KF6Kirigami, Qt6::Quick, turkit."""
    text = CMAKE_COMMENT.sub("", text)
    names: set[str] = set()
    for match in CMAKE_FIND.finditer(text):
        words = match.group(1).split()
        if not words:
            continue
        package = words[0]
        names.add("Qt6" if package in ("Qt6", "Qt") else package)
        if package in ("Qt6", "Qt"):
            for word in words[1:]:
                if word in CMAKE_KEYWORDS or re.fullmatch(r"[0-9.]+", word):
                    continue
                names.add(f"Qt6::{word}")
    for match in CMAKE_TARGET.finditer(text):
        target = match.group(1)
        names.add("Qt6::" + target[4:] if target.startswith("Qt::") else target)
    for match in CMAKE_LINK.finditer(text):
        for word in match.group(1).split()[1:]:
            if word in CMAKE_KEYWORDS or "::" in word or word.startswith(("$", "-")):
                continue
            names.add(word)
    return names


def check_sources(root: pathlib.Path, rules: Rules) -> tuple[list[str], dict[str, int]]:
    """Every problem in the sources under root/desktop, and what was looked at."""
    problems: list[str] = []
    counts = {"files": 0, "qml imports": 0, "cmake names": 0, "includes": 0}
    desktop = root / "desktop"
    upstream = desktop / "upstream-licensed"

    for path in sorted(desktop.rglob("*")):
        if not path.is_file() or SKIPPED_DIRECTORIES & set(path.relative_to(desktop).parts):
            continue
        relative = path.relative_to(root).as_posix()
        counts["files"] += 1

        def problem(message: str, where: str = relative) -> None:
            problems.append(f"{where}: {message}")

        if path.suffix in BINARY:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            problem("not UTF-8 text, and not a kind of file this check knows")
            continue

        # Licence identifiers.
        found = spdx_of(text)
        inside_host = upstream in path.parents and path.parent != upstream
        if inside_host:
            if not found:
                problem("no SPDX-License-Identifier in its first lines")
            elif PROJECT_ID in found:
                problem("code under upstream-licensed/ runs inside a GPL host and cannot carry the project licence")
        else:
            if PROJECT_ID not in found:
                problem(f"no 'SPDX-License-Identifier: {PROJECT_ID}' in its first {HEADER_LINES} lines")
            for identifier in SPDX.findall(text):
                if "GPL" in identifier.upper() and "LGPL" not in identifier.upper():
                    problem(f"carries the licence identifier {identifier}; GPL code lives only under "
                            "desktop/upstream-licensed/<host>/")

        name = path.name
        if path.suffix in (".qml", ".js", ".mjs"):
            for module in QML_IMPORT.findall(text):
                counts["qml imports"] += 1
                verdict = rules.judge("qml", module, relative)
                if verdict:
                    problem(f"imports {module}: {verdict}")
            for target in QML_PATH_IMPORT.findall(text):
                resolved = (path.parent / target).resolve()
                if desktop.resolve() not in (resolved, *resolved.parents):
                    problem(f"imports the directory {target}, which is outside desktop/")
            if path.suffix == ".qml" and not matches(relative, rules.literals):
                for pattern, what in ((HEX_COLOUR, "a colour literal"), (NAMED_COLOUR, "a colour name"),
                                      (COLOUR_FUNCTION, "a colour built from numbers")):
                    for match in pattern.finditer(text):
                        line = text.count("\n", 0, match.start()) + 1
                        problem(f"{what}, {match.group(0)}; every colour comes from a token (Tur.Kit's Theme)",
                                f"{relative}:{line}")
        elif name == "qmldir":
            for module in QMLDIR_IMPORT.findall(text):
                counts["qml imports"] += 1
                verdict = rules.judge("qml", module, relative)
                if verdict:
                    problem(f"imports {module}: {verdict}")
        elif name == "CMakeLists.txt" or path.suffix == ".cmake":
            for item in sorted(cmake_names(text)):
                counts["cmake names"] += 1
                verdict = rules.judge("cmake", item, relative)
                if verdict:
                    problem(f"uses {item}: {verdict}")
        elif path.suffix in (".h", ".hpp", ".hh", ".cpp", ".cc", ".cxx"):
            for header in CPP_INCLUDE.findall(text):
                counts["includes"] += 1
                for kind, pattern, reason in rules.forbid:
                    if kind == "include" and fnmatch.fnmatchcase(header, pattern):
                        problem(f"includes {header}: forbidden: {reason}")
            code = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
            code = re.sub(r"//[^\n]*", "", code)
            for kind, pattern, reason in rules.forbid:
                if kind == "word" and re.search(rf"\b{re.escape(pattern)}\b", code):
                    problem(f"uses {pattern}: forbidden: {reason}")

        for paths, pattern, reason in rules.deny_text:
            if matches(relative, paths):
                for match in pattern.finditer(text):
                    line = text.count("\n", 0, match.start()) + 1
                    problem(f"contains {match.group(0)!r}: {reason}", f"{relative}:{line}")

    return problems, counts


def needed_libraries(path: pathlib.Path) -> list[str] | None:
    """The DT_NEEDED entries of an ELF file; None if it is not one."""
    with path.open("rb") as handle:
        if handle.read(4) != b"\x7fELF":
            return None
    result = subprocess.run(["readelf", "-d", str(path)], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        return None
    return re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", result.stdout)


def check_linked(root: pathlib.Path, build: pathlib.Path, rules: Rules) -> tuple[list[str], dict[str, int]]:
    problems: list[str] = []
    counts = {"programs and libraries": 0, "libraries needed": 0}
    for path in sorted(build.rglob("*")):
        if not path.is_file() or path.is_symlink() or path.suffix in (".o", ".a") or "CMakeFiles" in path.parts:
            continue
        needed = needed_libraries(path)
        if needed is None:
            continue
        counts["programs and libraries"] += 1
        try:
            relative = path.resolve().relative_to(root.resolve()).as_posix()
        except ValueError:
            relative = path.as_posix()
        for library in needed:
            counts["libraries needed"] += 1
            verdict = rules.judge("soname", library, relative)
            if verdict:
                problems.append(f"{relative}: links {library}: {verdict}")
    if counts["programs and libraries"] == 0:
        problems.append(f"{build}: no built program or library was found to check")
    return problems, counts


def self_test(rules_text: str) -> int:
    """Plant one violation of each kind and require that each is reported."""
    # The tag is put together here so that this file does not itself contain
    # a GPL identifier line for the check to find.
    tag = "SPDX-License-" + "Identifier"
    header = f"// {tag}: {PROJECT_ID}\n"
    hash_header = f"# {tag}: {PROJECT_ID}\n"
    gpl_header = f"// {tag}: GPL-2.0-or-later\n"
    cases = {
        # path: (content, text the report must contain) ; None means "must be clean"
        "desktop/kit/controls/Good.qml": (header + "import QtQuick\nimport Tur.Kit as Kit\nItem {}\n", None),
        "desktop/apps/centre/qml/Chart.qml": (header + "import QtQuick\nimport QtCharts\nItem {}\n", "QtCharts"),
        "desktop/apps/centre/qml/Applet.qml": (header + "import org.kde.plasma.components as PC\nItem {}\n",
                                               "org.kde.plasma.components"),
        "desktop/apps/centre/qml/Unknown.qml": (header + "import Some.Unknown.Module\nItem {}\n",
                                                "not in ALLOWED_IMPORTS"),
        "desktop/apps/centre/qml/Gold.qml": (header + "import Tur.Kit.Pax as Pax\nItem {}\n", "allowed elsewhere"),
        "desktop/apps/pax-ui/qml/Panel.qml": (header + "import Tur.Kit.Pax as Pax\nItem {}\n", None),
        "desktop/kit/controls/Kirigami.qml": (header + "import org.kde.kirigami as Kirigami\nItem {}\n",
                                              "allowed elsewhere"),
        "desktop/apps/welcome/qml/Page.qml": (header + "import org.kde.kirigami as Kirigami\nItem {}\n", None),
        "desktop/kit/controls/White.qml": (header + "import QtQuick\nText { color: \"#FFFFFF\" }\n", "colour literal"),
        "desktop/kit/controls/Named.qml": (header + "import QtQuick\nText { color: \"white\" }\n", "colour name"),
        "desktop/kit/controls/Clear.qml": (header + "import QtQuick\nRectangle { color: \"transparent\" }\n", None),
        "desktop/kit/pax/Blue.qml": (header + "import QtQuick\nimport Tur.Kit as Kit\n"
                                     "Rectangle { color: Kit.Theme.accent }\n", "blue is the system"),
        "desktop/kit/controls/NoHeader.qml": ("import QtQuick\nItem {}\n", "no 'SPDX-License-Identifier"),
        "desktop/kit/controls/Gpl.qml": (header + gpl_header + "import QtQuick\nItem {}\n",
                                         "GPL code lives only under"),
        "desktop/apps/centre/src/main.cpp": (header + "#include <QApplication>\nint main(int c, char **v) "
                                             "{ QApplication a(c, v); return a.exec(); }\n", "QApplication"),
        "desktop/apps/centre/src/good.cpp": (header + "#include <QGuiApplication>\n// not a QApplication\n"
                                             "int main(int c, char **v) { QGuiApplication a(c, v); return a.exec(); }\n",
                                             None),
        "desktop/apps/centre/CMakeLists.txt": (hash_header + "find_package(Qt6 REQUIRED COMPONENTS Core Widgets)\n"
                                               "target_link_libraries(x PRIVATE Qt6::Core KF6::KIOCore)\n",
                                               "Qt6::Widgets"),
        "desktop/apps/settings/CMakeLists.txt": (hash_header + "find_package(KF6KirigamiAddons REQUIRED)\n",
                                                 "KF6KirigamiAddons"),
        "desktop/apps/setup/CMakeLists.txt": (hash_header + "target_link_libraries(x PRIVATE Qt::Charts libfoo)\n",
                                              "Qt6::Charts"),
        "desktop/apps/welcome/CMakeLists.txt": (hash_header + "find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui "
                                                "Qml Quick)\ntarget_link_libraries(x PRIVATE Qt6::Quick turkit)\n",
                                                None),
        # libplasma stays refused; KWayland's client library, which is also
        # exported as Plasma::, is allowed to the overlay alone (ADR-0020).
        "desktop/apps/ui/CMakeLists.txt": (hash_header + "target_link_libraries(x PRIVATE Plasma::Plasma)\n",
                                           "libplasma is not for TUR's programs"),
        "desktop/apps/notes/CMakeLists.txt": (hash_header + "target_link_libraries(x PRIVATE Plasma::KWaylandClient)\n",
                                              "allowed elsewhere"),
        "desktop/apps/tur-pax-overlay/CMakeLists.txt": (hash_header + "find_package(KWayland 6.0 CONFIG REQUIRED)\n"
                                                        "target_link_libraries(x PRIVATE Plasma::KWaylandClient)\n",
                                                        None),
        "desktop/upstream-licensed/README.md": (f"<!-- {tag}: {PROJECT_ID} -->\nThe rule.\n", None),
        "desktop/upstream-licensed/plasma-layout/layout.js": (gpl_header + "var a\n",
                                                              None),
        "desktop/upstream-licensed/plasma-layout/wrong.js": (header + "var a\n", "cannot carry the project licence"),
    }
    with tempfile.TemporaryDirectory(prefix="tur-boundary-") as holder:
        root = pathlib.Path(holder)
        for relative, (content, _) in cases.items():
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        problems, _ = check_sources(root, Rules(rules_text))

    failures = []
    for relative, (_, expected) in cases.items():
        mine = [p for p in problems if p.startswith(relative + ":")]
        if expected is None and mine:
            failures.append(f"{relative} is clean but was reported: {mine}")
        if expected is not None and not any(expected in p for p in mine):
            failures.append(f"{relative} should be reported with {expected!r}; got {mine}")
    extra = [p for p in problems if "desktop/apps/centre/CMakeLists.txt" in p and "KF6::KIOCore" in p]
    if not extra:
        failures.append("a forbidden target in target_link_libraries (KF6::KIOCore) was not reported")
    extra = [p for p in problems if "desktop/apps/setup/CMakeLists.txt" in p and "libfoo" in p]
    if not extra:
        failures.append("an unknown library in target_link_libraries (libfoo) was not reported")

    rules = Rules(rules_text)
    linked = {
        ("soname", "libQt6Widgets.so.6", "desktop/build/ci/apps/x/tur-x"): False,
        ("soname", "libQt6Charts.so.6", "desktop/build/ci/apps/x/tur-x"): False,
        ("soname", "libKF6KIOCore.so.6", "desktop/build/ci/apps/x/tur-x"): False,
        ("soname", "libsomething.so.1", "desktop/build/ci/apps/x/tur-x"): False,
        ("soname", "libQt6Test.so.6", "desktop/build/ci/apps/x/tur-x"): False,
        ("soname", "libQt6Test.so.6", "desktop/build/ci/kit/tests/tst_kit"): True,
        ("soname", "libQt6Quick.so.6", "desktop/build/ci/apps/x/tur-x"): True,
        ("soname", "libturkit.so", "desktop/build/ci/apps/x/tur-x"): True,
    }
    for (kind, name, path), allowed in linked.items():
        if (rules.judge(kind, name, path) is None) != allowed:
            failures.append(f"{name} in {path} should be {'allowed' if allowed else 'refused'}")

    if failures:
        for failure in failures:
            print(f"::error::boundary self-test: {failure}")
        return 1
    print(f"boundary self-test: {len(cases)} planted files and {len(linked)} libraries judged as expected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--linked", type=pathlib.Path, metavar="DIR")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parents[2]
    rules_text = (root / "desktop" / "ALLOWED_IMPORTS").read_text(encoding="utf-8")
    if args.self_test:
        return self_test(rules_text)

    rules = Rules(rules_text)
    if args.linked:
        problems, counts = check_linked(root, args.linked, rules)
        what = "linked libraries"
    else:
        problems, counts = check_sources(root, rules)
        what = "sources"
    summary = ", ".join(f"{count} {label}" for label, count in counts.items())
    if problems:
        for problem in problems:
            print(f"::error::boundary: {problem}")
        print(f"licence boundary ({what}): {len(problems)} problem(s) in {summary}")
        return 1
    print(f"licence boundary ({what}): clean. {summary}; "
          f"{len(rules.allow)} allow rules, {len(rules.forbid)} forbid rules")
    return 0


if __name__ == "__main__":
    sys.exit(main())

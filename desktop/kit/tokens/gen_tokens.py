#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Generate tur-kit's token singletons from tokens.json, and check the tokens.

    gen_tokens.py --write    regenerate Theme.qml and ../pax/PaxTheme.qml
    gen_tokens.py --check    fail if the generated files are stale or a colour
                             pair is below its WCAG threshold (the default)
    gen_tokens.py --table    print the contrast table

tokens.json is the single source of truth (desktop-experience.md, section 5).
The generated files are committed so that the build needs no generator and a
reviewer sees every value change; the `boundary` job runs --check so they
cannot drift. Needs only the Python standard library.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
TOKENS = HERE / "tokens.json"
THEME_QML = HERE / "Theme.qml"
PAX_QML = HERE.parent / "pax" / "PaxTheme.qml"

HEADER = """\
// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// GENERATED from tokens/tokens.json by tokens/gen_tokens.py. Do not edit.
// Change tokens.json, then run: python3 desktop/kit/tokens/gen_tokens.py --write
"""

HEX = re.compile(r"^#[0-9A-F]{6}$")


def camel(token: str) -> str:
    """surface.base -> surfaceBase"""
    head, *rest = token.split(".")
    return head + "".join(part[:1].upper() + part[1:] for part in rest)


def luminance(colour: str) -> float:
    """Relative luminance, WCAG 2.2."""
    channels = [int(colour[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    linear = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in channels]
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]


def contrast(a: str, b: str) -> float:
    """Contrast ratio, WCAG 2.2: from 1 (none) to 21 (black on white)."""
    la, lb = luminance(a), luminance(b)
    if la < lb:
        la, lb = lb, la
    return (la + 0.05) / (lb + 0.05)


def load() -> dict:
    return json.loads(TOKENS.read_text(encoding="utf-8"))


def validate(tokens: dict) -> list[str]:
    errors = []
    variants = tokens["variants"]
    if variants != ["dark", "light"]:
        errors.append("this generator knows the variants dark and light only")
    groups = {"colours": tokens["colours"], "pax.colours": tokens["pax"]["colours"]}
    for group, table in groups.items():
        for name, entry in table.items():
            for variant in variants:
                value = entry.get(variant, "")
                if not HEX.match(value):
                    errors.append(f"{group}: {name} has no #RRGGBB value (upper case) for {variant}: {value!r}")
    for palette in ("trusted", "boot", "highContrast"):
        for name, value in tokens["fixed"][palette].items():
            if not HEX.match(value):
                errors.append(f"fixed.{palette}.{name}: not #RRGGBB (upper case): {value!r}")
    for name, value in tokens["pax"]["overlay"].items():
        if name != "about" and not HEX.match(value):
            errors.append(f"pax.overlay.{name}: not #RRGGBB (upper case): {value!r}")
    return errors


def colour_lines(table: dict, switch: str, indent: str = "    ") -> list[str]:
    width = max(len(camel(name)) for name in table)
    lines = []
    for name, entry in table.items():
        prop = camel(name)
        lines.append(f"{indent}// {name}: {entry['role']}")
        lines.append(f"{indent}readonly property color {prop}:{' ' * (width - len(prop))} "
                     f"{switch} ? \"{entry['light']}\" : \"{entry['dark']}\"")
    return lines


def number(value) -> str:
    return str(int(value)) if float(value).is_integer() else repr(float(value))


def render_theme(tokens: dict) -> str:
    out = [HEADER.rstrip("\n"), "pragma Singleton", "", "import QtQuick", "", "QtObject {", "    id: theme", ""]
    out += [
        "    // Light or dark. It follows the desktop's preference until a program",
        "    // assigns it; followSystem() restores that. TUR is dark unless the",
        "    // desktop says light.",
        "    property bool light: SystemAppearance.prefersLight",
        "",
        "    function followSystem(): void {",
        "        theme.light = Qt.binding(() => SystemAppearance.prefersLight)",
        "    }",
        "",
        "    // Multiplies every text size. 1 is the user's own font size; the",
        "    // accessibility tests run at 2.",
        "    property real textScale: 1.0",
        "",
        "    // Sizes are given for a 10 pt interface font and follow the user's",
        "    // font size from there.",
        "    readonly property real fontScale: theme.textScale",
        "        * (Application.font.pointSize > 0 ? Application.font.pointSize : 10) / 10",
        "",
        "    // Colours. Blue is the system. Gold is not here: it belongs to P@x",
        "    // and lives in the module Tur.Kit.Pax.",
    ]
    out += colour_lines(tokens["colours"], "theme.light")
    out.append("")
    out.append("    // Type")
    kind = tokens["type"]
    out.append(f"    readonly property string interfaceFamily: \"{kind['interface']}\"")
    out.append(f"    readonly property string monoFamily: \"{kind['mono']}\"")
    out.append(f"    readonly property real lineHeight: {number(kind['lineHeight'])}")
    out.append(f"    readonly property int measure: {number(kind['measure'])}")
    out.append(f"    readonly property real capsSpacing: {number(kind['capsSpacing'])}")
    for name, style in kind["styles"].items():
        cap = name[:1].upper() + name[1:]
        out.append(f"    // {name}: {style['use']}")
        out.append(f"    readonly property real type{cap}Size: {number(style['size'])}")
        out.append(f"    readonly property int type{cap}Weight: {number(style['weight'])}")
    for group, comment in (("space", "Space: a 4 px grid"), ("radius", "Radius"),
                           ("target", "Target sizes"), ("line", "Lines and the focus ring"),
                           ("motion", "Motion: base durations in milliseconds; use the Motion singleton")):
        out.append("")
        out.append(f"    // {comment}")
        for name, value in tokens[group].items():
            cap = name[:1].upper() + name[1:]
            prop = f"{group}{cap}" if not name.startswith(group) else name
            out.append(f"    readonly property int {prop}: {number(value)}")
    out.append("}")
    return "\n".join(out) + "\n"


def render_pax(tokens: dict) -> str:
    pax = tokens["pax"]
    out = [HEADER.rstrip("\n"), "pragma Singleton", "", "import QtQuick", "import Tur.Kit as Kit", "",
           "// Gold is P@x and nothing else (desktop-experience.md, section 5.1).",
           "// Only P@x's surfaces import Tur.Kit.Pax; the boundary check enforces it.",
           "QtObject {"]
    # "on" is a word of QML's grammar and "onX" is a signal handler, so pax.on
    # is spelled out.
    names = {"pax": "pax", "pax.fill": "fill", "pax.on": "fillText"}
    table = {names[name]: entry for name, entry in pax["colours"].items()}
    out += colour_lines(table, "Kit.Theme.light")
    out.append("")
    out.append("    // The overlay's palette is fixed: it sits over content TUR does not control.")
    for name, value in pax["overlay"].items():
        if name == "about":
            continue
        cap = name[:1].upper() + name[1:]
        out.append(f"    readonly property color overlay{cap}: \"{value}\"")
    out.append("}")
    return "\n".join(out) + "\n"


def colour_of(tokens: dict, name: str, variant: str) -> str:
    table = tokens["pax"]["colours"] if name.startswith("pax") else tokens["colours"]
    return table[name][variant]


def check_contrast(tokens: dict, show: bool) -> list[str]:
    """Every declared pair, in every variant, against its threshold and the document."""
    errors: list[str] = []
    spec = tokens["contrast"]
    thresholds = spec["thresholds"]
    rows = []
    for pair in spec["pairs"]:
        need = thresholds[pair["needs"]]
        for variant in tokens["variants"]:
            fg, bg = colour_of(tokens, pair["fg"], variant), colour_of(tokens, pair["bg"], variant)
            ratio = contrast(fg, bg)
            where = f"{variant}: {pair['fg']} on {pair['bg']}"
            rows.append((where, ratio, need, pair[variant]))
            if ratio < need:
                errors.append(f"{where} is {ratio:.2f}:1, below {need}:1 ({pair['needs']})")
            if pair[variant] is not None and abs(ratio - pair[variant]) > 0.06:
                errors.append(f"{where} is {ratio:.2f}:1 but the design states {pair[variant]}:1")
    for pair in spec["fixedPairs"]:
        need = thresholds[pair["needs"]]
        palette = tokens["pax"]["overlay"] if pair["palette"] == "pax.overlay" else tokens["fixed"][pair["palette"]]
        ratio = contrast(palette[pair["fg"]], palette[pair["bg"]])
        where = f"{pair['palette']}: {pair['fg']} on {pair['bg']}"
        rows.append((where, ratio, need, pair["ratio"]))
        if ratio < need:
            errors.append(f"{where} is {ratio:.2f}:1, below {need}:1 ({pair['needs']})")
        if pair["ratio"] is not None and abs(ratio - pair["ratio"]) > 0.06:
            errors.append(f"{where} is {ratio:.2f}:1 but the design states {pair['ratio']}:1")
    for exempt in spec["exempt"]:
        for variant in tokens["variants"]:
            if exempt[variant] is None:
                continue
            ratio = contrast(colour_of(tokens, exempt["fg"], variant), colour_of(tokens, "surface.base", variant))
            rows.append((f"{variant}: {exempt['fg']} on surface.base (exempt)", ratio, 0.0, exempt[variant]))
            if abs(ratio - exempt[variant]) > 0.06:
                errors.append(f"{variant}: {exempt['fg']} on surface.base is {ratio:.2f}:1 "
                              f"but the design states {exempt[variant]}:1")
    for forbidden in spec["forbidden"]:
        ratio = contrast(colour_of(tokens, forbidden["fg"], forbidden["variant"]),
                         colour_of(tokens, forbidden["bg"], forbidden["variant"]))
        rows.append((f"{forbidden['variant']}: {forbidden['fg']} on {forbidden['bg']} (not to be used)", ratio, 0.0, None))
    if show:
        print(f"{'pair':58s} {'ratio':>6s} {'needs':>6s} {'design':>7s}")
        for where, ratio, need, stated in rows:
            print(f"{where:58s} {ratio:6.2f} {need:6.1f} {'' if stated is None else stated:>7}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--table", action="store_true")
    args = parser.parse_args()

    tokens = load()
    errors = validate(tokens)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    outputs = {THEME_QML: render_theme(tokens), PAX_QML: render_pax(tokens)}

    if args.write:
        for path, text in outputs.items():
            path.write_text(text, encoding="utf-8")
            print(f"wrote {path.relative_to(HERE.parent.parent.parent)}")
        return 0

    for path, text in outputs.items():
        current = path.read_text(encoding="utf-8") if path.exists() else ""
        if current != text:
            errors.append(f"{path.name} is not what tokens.json generates; run gen_tokens.py --write")
    errors += check_contrast(tokens, show=args.table)
    if errors:
        for error in errors:
            print(f"::error::tokens: {error}")
        return 1
    pairs = len(tokens["contrast"]["pairs"]) * len(tokens["variants"]) + len(tokens["contrast"]["fixedPairs"])
    print(f"tokens: generated files are current; {pairs} colour pairs meet their thresholds and match the design")
    return 0


if __name__ == "__main__":
    sys.exit(main())

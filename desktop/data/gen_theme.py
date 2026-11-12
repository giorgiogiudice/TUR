#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""TUR's Plasma colour schemes, generated from the design tokens, and the checks
of everything the image installs from desktop/data/rootfs.

    gen_theme.py --write       regenerate the colour schemes, schemes.json and MANIFEST.tsv
    gen_theme.py --check       fail if anything is stale or breaks a rule below (the default)
    gen_theme.py --table       print the role table and every contrast ratio (Markdown)
    gen_theme.py --self-test   plant one fault of each kind in a copy and require each is caught

desktop/kit/tokens/tokens.json is the one source of TUR's colours (design,
section 5.2). This script maps KDE's colour roles onto those tokens (ROLES
below) and writes TurDark.colors and TurLight.colors in the format of KDE's
colour schemes, from the key names that KDE Frameworks 6.31 and Plasma 6.8
read (kcolorscheme.cpp; plasma-workspace kcms/colors/colorsapplicator.cpp).
No file here starts as a copy of one of KDE's.

The checks, each with the reason it exists:

  - the generated files are byte-for-byte what the tokens give;
  - every colour in a scheme is a token's value (nothing invented on the way);
  - no gold anywhere in the installed tree: gold is P@x's and nothing else's
    (design, 5.1), so no P@x token value, and no colour of the gold family;
  - WCAG 2.2 AA in both schemes: 4.5:1 for every text role on every
    background of its set, 3:1 for focus and hover decorations and for a
    selection against the background it sits on;
  - the KDE formats, as Plasma 6.8 reads them (the tables below name the
    source file each was read from): colour-scheme groups and keys,
    global-theme metadata and the defaults keys KLookAndFeelManager applies,
    the panel file for new accounts, the system defaults;
  - nothing in the tree names Fedora (the marks check of the image, TUR-008);
  - every installed file carries the project's SPDX identifier where its
    format has comments, and MANIFEST.tsv lists exactly the files that exist.

Needs only the Python standard library. Runs in the `boundary` job of the
desktop workflow.
"""

from __future__ import annotations

import argparse
import colorsys
import hashlib
import json
import os
import pathlib
import re
import shutil
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
TOKENS = ROOT / "desktop" / "kit" / "tokens" / "tokens.json"
ROOTFS = HERE / "rootfs"
SCHEMES_JSON = HERE / "schemes.json"
MANIFEST = HERE / "MANIFEST.tsv"

PROJECT_ID = "LicenseRef-TUR-Noncommercial-1.0.0"
SPDX_LINE = f"SPDX-License-Identifier: {PROJECT_ID}"
HEX = re.compile(r"^#[0-9A-F]{6}$")

SCHEMES = {"dark": ("TurDark", "TUR Dark"), "light": ("TurLight", "TUR Light")}
THEMES = {"dark": "org.tur.dark", "light": "org.tur.light"}

# ---- KDE's colour-scheme format (verified against the sources) -------------------
# The seven colour sets and the twelve keys of each: colorsapplicator.cpp
# (plasma-workspace v6.8.2), colorSetGroupList and colorSetKeyList; read by
# KColorSchemePrivate::initFromConfig (kcolorscheme v6.31.0).
COLOUR_SETS = ["Colors:View", "Colors:Window", "Colors:Button", "Colors:Selection", "Colors:Tooltip",
               "Colors:Complementary", "Colors:Header"]
FOREGROUNDS = ["ForegroundNormal", "ForegroundInactive", "ForegroundActive", "ForegroundLink",
               "ForegroundVisited", "ForegroundNegative", "ForegroundNeutral", "ForegroundPositive"]
BACKGROUNDS = ["BackgroundNormal", "BackgroundAlternate"]
DECORATIONS = ["DecorationFocus", "DecorationHover"]
SET_KEYS = BACKGROUNDS + FOREGROUNDS + DECORATIONS
# [WM]: colorItemListWM in colorsapplicator.cpp, the window decoration's colours.
WM_KEYS = ["activeBackground", "activeForeground", "inactiveBackground", "inactiveForeground", "activeBlend",
           "inactiveBlend"]
# [ColorEffects:Inactive] and [ColorEffects:Disabled]: effectList in
# colorsapplicator.cpp; the enumerations are StateEffects in kcolorscheme.cpp
# (intensity 0 none; colour 0 none; contrast 0 none, 1 fade towards the background).
EFFECT_KEYS = ["Enable", "ChangeSelectionColor", "IntensityEffect", "IntensityAmount", "ColorEffect",
               "ColorAmount", "Color", "ContrastEffect", "ContrastAmount"]

# ---- KDE's roles, onto TUR's tokens (a design judgement; docs/architecture/tur-theme.md) --
# One row per colour set: which token each key takes, per variant when the
# two differ. "dark:" in front of a token means "the dark variant's value in
# both schemes" (the complementary set is the dark one in a light scheme, as in
# KDE's own light scheme).
_TEXT = {
    "ForegroundNormal": "text.primary",
    "ForegroundInactive": "text.secondary",
    "ForegroundActive": "accent.text",
    "ForegroundLink": "accent.text",
    "ForegroundVisited": "accent.text",
    "ForegroundNegative": "negative",
    "ForegroundNeutral": "warning",
    "ForegroundPositive": "positive",
    "DecorationFocus": "accent.text",
    "DecorationHover": "accent",
}
ROLES: dict[str, dict[str, str | tuple[str, str]]] = {
    "Colors:Window": {"BackgroundNormal": "surface.base", "BackgroundAlternate": "surface.raised", **_TEXT},
    "Colors:View": {"BackgroundNormal": "surface.raised", "BackgroundAlternate": "surface.base", **_TEXT},
    "Colors:Button": {"BackgroundNormal": "surface.overlay", "BackgroundAlternate": "surface.sunken", **_TEXT},
    "Colors:Selection": {"BackgroundNormal": "selection", "BackgroundAlternate": "selection",
                         **{key: "selection.on" for key in FOREGROUNDS},
                         "DecorationFocus": "accent.text", "DecorationHover": "accent"},
    "Colors:Tooltip": {"BackgroundNormal": "surface.overlay", "BackgroundAlternate": "surface.raised", **_TEXT},
    "Colors:Complementary": {key: "dark:" + value for key, value in
                             {"BackgroundNormal": "surface.base", "BackgroundAlternate": "surface.raised",
                              **_TEXT}.items()},
    "Colors:Header": {"BackgroundNormal": ("surface.raised", "surface.sunken"), "BackgroundAlternate": "surface.base",
                      **_TEXT},
    "Colors:Header][Inactive": {"BackgroundNormal": "surface.base",
                                "BackgroundAlternate": ("surface.raised", "surface.sunken"), **_TEXT,
                                "ForegroundNormal": "text.secondary"},
}
WM_ROLES = {
    "activeBackground": ("surface.raised", "surface.sunken"),
    "activeForeground": "text.primary",
    "inactiveBackground": "surface.base",
    "inactiveForeground": "text.secondary",
    "activeBlend": ("surface.raised", "surface.sunken"),
    "inactiveBlend": "surface.base",
}
# The text contrast setting of the scheme ([KDE] contrast, 0 to 10, KDE's
# default 7): how strongly frames are shaded. Lower is calmer (design, 5.1).
FRAME_CONTRAST = 4

# ---- Plasma's global-theme format (verified) -------------------------------------
# The keys of contents/defaults that KLookAndFeelManager::save applies
# (plasma-workspace v6.8.2, libklookandfeel/klookandfeelmanager.cpp), and
# [Wallpaper] Image, which wallpapers/defaultwallpaper.cpp reads for a desktop
# with no picture of its own. Nothing else in the file is read. In particular
# [ksplashrc] is applied only when the package has a splash QML file
# (packageContents: "splashmainscript"); TUR's has none, so the splash is
# turned off in the system's ksplashrc instead.
LNF_KEYS = {
    "kdeglobals][KDE": {"widgetStyle"},
    "kdeglobals][General": {"ColorScheme", "font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"},
    "kdeglobals][Icons": {"Theme"},
    "kdeglobals][WM": {"activeFont"},
    "plasmarc][Theme": {"name"},
    "kcminputrc][Mouse": {"cursorTheme"},
    "kwinrc][WindowSwitcher": {"LayoutName"},
    "kwinrc][Windows": {"Placement"},
    "kwinrc][org.kde.kdecoration2": {"library", "theme", "NoPlugin", "BorderSize"},
    "Wallpaper": {"Image"},
}
LNF_EXPECTED = {
    "dark": {"kdeglobals][General": {"ColorScheme": "TurDark"}, "kdeglobals][Icons": {"Theme": "breeze-dark"}},
    "light": {"kdeglobals][General": {"ColorScheme": "TurLight"}, "kdeglobals][Icons": {"Theme": "breeze"}},
}
LNF_COMMON = {
    "kdeglobals][KDE": {"widgetStyle": "Breeze"},
    "plasmarc][Theme": {"name": "default"},
    "kcminputrc][Mouse": {"cursorTheme": "breeze_cursors"},
    "kwinrc][org.kde.kdecoration2": {"library": "org.kde.breeze", "theme": "Breeze"},
    # The wallpaper package image/branding/branding.sh installs.
    "Wallpaper": {"Image": "Tur"},
}

# ---- The panel of a new account (verified) --------------------------------------
# plasma-org.kde.plasma.desktop-appletsrc: containment keys as libplasma
# writes them; Plasma::Types: formfactor 2 horizontal, location 4 bottom edge,
# immutability 1 mutable. AppletOrder: plasma-desktop containments/panel.
# icon: Kickoff's main.xml; launchers: the task manager's main.xml.
PANEL_APPLETS = ["org.kde.plasma.kickoff", "org.kde.plasma.icontasks", "org.kde.plasma.marginsseparator",
                 "org.kde.plasma.systemtray", "org.kde.plasma.digitalclock"]
LAUNCHERS = ["applications:org.kde.dolphin.desktop", "applications:org.mozilla.firefox.desktop",
             "applications:systemsettings.desktop"]
LAUNCHER_ICON = "tur-logo-icon"
APPLETSRC = "etc/skel/.config/plasma-org.kde.plasma.desktop-appletsrc"
PLASMASHELLRC = "etc/skel/.config/plasmashellrc"

# The system defaults: content under /usr/share/tur/xdg, and /etc/xdg holds a
# link to each, because /etc/xdg is the one configuration directory that the
# user's session, the first-run setup session and the login screen all read
# (docs/architecture/tur-theme.md, section 5).
XDG_SOURCE = "usr/share/tur/xdg"
XDG_EXPECTED = {
    "kdeglobals": {"KDE": {"LookAndFeelPackage": "org.tur.dark", "DefaultDarkLookAndFeel": "org.tur.dark",
                           "DefaultLightLookAndFeel": "org.tur.light", "SingleClick": "false"}},
    "ksplashrc": {"KSplash": {"Engine": "none"}},
    "plasma-welcomerc": {"General": {"ShowUpdatePage": "false"}},
    "kicker-extra-favoritesrc": {"General": {"IgnoreDefaults": "true",
                                             "Prepend": ";".join(l.split(":", 1)[1] for l in LAUNCHERS)}},
}

# What may be in the tree at all: the generated colour schemes, the two
# global themes, the system defaults and their links, the new account's files.
ALLOWED_PATHS = [
    r"usr/share/color-schemes/Tur(Dark|Light)\.colors",
    r"usr/share/plasma/look-and-feel/org\.tur\.(dark|light)/metadata\.json",
    r"usr/share/plasma/look-and-feel/org\.tur\.(dark|light)/contents/defaults",
    r"usr/share/tur/xdg/[a-z-]+rc",
    r"usr/share/tur/xdg/kdeglobals",
    r"etc/xdg/[a-z-]+rc",
    r"etc/xdg/kdeglobals",
    r"etc/skel/\.config/plasma-org\.kde\.plasma\.desktop-appletsrc",
    r"etc/skel/\.config/plasmashellrc",
]

# ---- Gold (design, 5.1: gold is P@x and nothing else) ------------------------------
# The gold family: hue from 33 to 55 degrees with real saturation. Orange
# (TUR's warning, about 25 degrees) and the yellows of high contrast (54
# degrees, but on the trusted and system surfaces only, never here) are
# outside or excluded by name below.
GOLD_HUE = (33.0, 55.0)
GOLD_MIN_SATURATION = 0.35
GOLD_MIN_VALUE = 0.25


# ---- helpers ------------------------------------------------------------------------

def luminance(colour: str) -> float:
    channels = [int(colour[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    linear = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in channels]
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]


def contrast(a: str, b: str) -> float:
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def rgb(colour: str) -> tuple[int, int, int]:
    return tuple(int(colour[i:i + 2], 16) for i in (1, 3, 5))  # type: ignore[return-value]


def to_hex(text: str) -> str | None:
    """A colour as KConfig writes it ("r,g,b" or "#rrggbb"), as #RRGGBB."""
    text = text.strip()
    if re.fullmatch(r"#[0-9A-Fa-f]{6}", text):
        return text.upper()
    match = re.fullmatch(r"(\d{1,3}),(\d{1,3}),(\d{1,3})", text)
    if match and all(int(g) <= 255 for g in match.groups()):
        return "#" + "".join(f"{int(g):02X}" for g in match.groups())
    return None


def is_gold(colour: str) -> bool:
    r, g, b = (c / 255 for c in rgb(colour))
    hue, saturation, value = colorsys.rgb_to_hsv(r, g, b)
    return (GOLD_HUE[0] <= hue * 360 <= GOLD_HUE[1] and saturation >= GOLD_MIN_SATURATION
            and value >= GOLD_MIN_VALUE)


def load_tokens(path: pathlib.Path = TOKENS) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def token(tokens: dict, spec: str | tuple[str, str], variant: str) -> tuple[str, str]:
    """(token name, #RRGGBB) of a role's spec in a variant."""
    if isinstance(spec, tuple):
        spec = spec[0] if variant == "dark" else spec[1]
    if spec.startswith("dark:"):
        spec, variant = spec[5:], "dark"
    if spec.startswith("pax"):
        raise SystemExit(f"a KDE role is mapped to P@x's token {spec}: gold is P@x's alone")
    return spec, tokens["colours"][spec][variant]


def parse_ini(text: str) -> dict[str, dict[str, str]]:
    """KConfig's format: [group][subgroup] headers, key=value lines, # comments.

    Nested groups are joined as KConfig spells them inside one header:
    "Containments][1][Applets][2". Raises ValueError on a line it does not
    understand, so that a typo is not silently ignored."""
    groups: dict[str, dict[str, str]] = {}
    current: dict[str, str] | None = None
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            name = line[1:-1]
            if name in groups:
                raise ValueError(f"line {number}: group [{name}] appears twice")
            current = groups.setdefault(name, {})
            continue
        key, sep, value = line.partition("=")
        if not sep or current is None or not key.strip():
            raise ValueError(f"line {number}: not a key=value line inside a group: {raw!r}")
        if key.strip() in current:
            raise ValueError(f"line {number}: key {key.strip()} appears twice in its group")
        current[key.strip()] = value.strip()
    return groups


# ---- generation ---------------------------------------------------------------------

def scheme_groups(tokens: dict, variant: str) -> dict[str, dict[str, tuple[str, str]]]:
    """Every group and key of one scheme, as (token, #RRGGBB)."""
    out: dict[str, dict[str, tuple[str, str]]] = {}
    for group, roles in ROLES.items():
        out[group] = {key: token(tokens, roles[key], variant) for key in SET_KEYS}
    out["WM"] = {key: token(tokens, WM_ROLES[key], variant) for key in WM_KEYS}
    return out


def disabled_fade(tokens: dict, variant: str) -> float:
    """How far disabled text fades towards the window background so that ordinary
    text lands on text.disabled: KDE draws disabled text with an effect, not a
    colour of its own (StateEffects, ContrastFade mixes the text with the
    background by this amount). The mean over the three channels, to two
    places."""
    colours = tokens["colours"]
    fg, bg, target = (rgb(colours[name][variant]) for name in ("text.primary", "surface.base", "text.disabled"))
    amounts = [(f - t) / (f - b) for f, b, t in zip(fg, bg, target) if f != b]
    return round(sum(amounts) / len(amounts), 2)


def kconfig_colour(colour: str) -> str:
    return ",".join(str(c) for c in rgb(colour))


def render_scheme(tokens: dict, variant: str) -> str:
    file_name, name = SCHEMES[variant]
    groups = scheme_groups(tokens, variant)
    base = tokens["colours"]["surface.base"][variant]
    out = [
        f"# {SPDX_LINE}",
        "#",
        f"# {name}: TUR's {variant} colour scheme for KDE.",
        "# GENERATED from desktop/kit/tokens/tokens.json by desktop/data/gen_theme.py. Do not edit:",
        "# change the tokens (or the role table in gen_theme.py), then run",
        "#   python3 desktop/data/gen_theme.py --write",
        "# Each value is followed by the token it comes from. Gold is not here: it is P@x's.",
        "",
    ]

    def group(header: str, entries: list[tuple[str, str]], comment: str = "") -> None:
        if comment:
            out.append(f"# {comment}")
        out.append(f"[{header}]")
        out.extend(f"{key}={value}" for key, value in entries)
        out.append("")

    def coloured(header: str, values: dict[str, tuple[str, str]], keys: list[str]) -> None:
        out.append(f"[{header}]")
        width = max(len(k) for k in keys)
        for key in keys:
            token_name, colour = values[key]
            out.append(f"# {key}{' ' * (width - len(key))}  {token_name} {colour}")
            out.append(f"{key}={kconfig_colour(colour)}")
        out.append("")

    group("ColorEffects:Disabled", [
        ("Enable", "true"), ("ChangeSelectionColor", "false"),
        ("IntensityEffect", "0"), ("IntensityAmount", "0"),
        ("ColorEffect", "0"), ("ColorAmount", "0"), ("Color", kconfig_colour(base)),
        ("ContrastEffect", "1"), ("ContrastAmount", f"{disabled_fade(tokens, variant):.2f}"),
    ], "Disabled text fades towards the background until it is text.disabled.")
    group("ColorEffects:Inactive", [
        ("Enable", "false"), ("ChangeSelectionColor", "false"),
        ("IntensityEffect", "0"), ("IntensityAmount", "0"),
        ("ColorEffect", "0"), ("ColorAmount", "0"), ("Color", kconfig_colour(base)),
        ("ContrastEffect", "0"), ("ContrastAmount", "0"),
    ], "A window looks the same whether or not it has the focus.")
    for header in ROLES:
        coloured(header, groups[header], SET_KEYS)
    group("General", [("ColorScheme", file_name), ("Name", name)])
    group("KDE", [("contrast", str(FRAME_CONTRAST))], "How strongly frames are shaded (KDE's default is 7).")
    coloured("WM", groups["WM"], WM_KEYS)
    while out and out[-1] == "":
        out.pop()
    return "\n".join(out) + "\n"


def schemes_json(tokens: dict) -> str:
    """The schemes as data, for the specimen picture (desktop/data/tests) and the docs."""
    data: dict = {
        "licence": SPDX_LINE,
        "about": "GENERATED by desktop/data/gen_theme.py from desktop/kit/tokens/tokens.json. "
                 "The colours of TurDark.colors and TurLight.colors, by colour set and key, with the token each "
                 "comes from.",
        "sets": [g for g in ROLES],
        "foregrounds": FOREGROUNDS,
        "backgrounds": BACKGROUNDS,
        "decorations": DECORATIONS,
        "schemes": {},
    }
    for variant in ("dark", "light"):
        groups = scheme_groups(tokens, variant)
        data["schemes"][variant] = {
            "file": SCHEMES[variant][0] + ".colors",
            "name": SCHEMES[variant][1],
            "disabledFade": disabled_fade(tokens, variant),
            "groups": {g: {k: {"token": t, "colour": c} for k, (t, c) in keys.items()} for g, keys in groups.items()},
        }
    return json.dumps(data, indent=2) + "\n"


def generated(tokens: dict) -> dict[pathlib.Path, str]:
    return {
        ROOTFS / "usr/share/color-schemes/TurDark.colors": render_scheme(tokens, "dark"),
        ROOTFS / "usr/share/color-schemes/TurLight.colors": render_scheme(tokens, "light"),
        SCHEMES_JSON: schemes_json(tokens),
    }


def tree_files(rootfs: pathlib.Path) -> list[pathlib.Path]:
    return sorted(p for p in rootfs.rglob("*") if p.is_symlink() or p.is_file())


def render_manifest(rootfs: pathlib.Path, scheme_paths: set[str]) -> str:
    out = [
        f"# {SPDX_LINE}",
        "#",
        "# GENERATED by desktop/data/gen_theme.py --write. What image/Containerfile copies from",
        "# desktop/data/rootfs into the image, one line per file, tab-separated:",
        "#   path in the image, kind, origin, SHA-256 (or the link's target)",
        "# kind: file or link. origin: generated (from the tokens) or written (by hand, in this directory).",
    ]
    for path in tree_files(rootfs):
        relative = path.relative_to(rootfs).as_posix()
        if path.is_symlink():
            out.append(f"/{relative}\tlink\twritten\t{os.readlink(path)}")
        else:
            origin = "generated" if relative in scheme_paths else "written"
            out.append(f"/{relative}\tfile\t{origin}\t{hashlib.sha256(path.read_bytes()).hexdigest()}")
    return "\n".join(out) + "\n"


# ---- checks -------------------------------------------------------------------------

def check_generated(tokens: dict, rootfs: pathlib.Path, schemes_json_path: pathlib.Path,
                    manifest: pathlib.Path) -> list[str]:
    errors = []
    expected = generated(tokens)
    for path, text in expected.items():
        target = schemes_json_path if path == SCHEMES_JSON else rootfs / path.relative_to(ROOTFS)
        if not target.exists() or target.read_text(encoding="utf-8") != text:
            errors.append(f"{target.name}: stale or missing; run python3 desktop/data/gen_theme.py --write")
    scheme_paths = {p.relative_to(ROOTFS).as_posix() for p in expected if p != SCHEMES_JSON}
    if not manifest.exists() or manifest.read_text(encoding="utf-8") != render_manifest(rootfs, scheme_paths):
        errors.append("MANIFEST.tsv does not list the tree as it is; run python3 desktop/data/gen_theme.py --write")
    return errors


def scheme_values(text: str) -> dict[str, dict[str, str]]:
    groups = parse_ini(text)
    return {g: {k: to_hex(v) or v for k, v in keys.items()} for g, keys in groups.items()}


def check_scheme_format(groups: dict[str, dict[str, str]], where: str) -> list[str]:
    errors = []
    known = set(ROLES) | {"WM", "General", "KDE", "ColorEffects:Disabled", "ColorEffects:Inactive"}
    for group in groups:
        if group not in known:
            errors.append(f"{where}: group [{group}] is not one KDE reads")
    for group in ROLES:
        keys = groups.get(group, {})
        missing = [k for k in SET_KEYS if k not in keys]
        extra = [k for k in keys if k not in SET_KEYS]
        if missing:
            errors.append(f"{where}: [{group}] lacks {', '.join(missing)}")
        if extra:
            errors.append(f"{where}: [{group}] has keys KDE does not read: {', '.join(extra)}")
    if sorted(groups.get("WM", {})) != sorted(WM_KEYS):
        errors.append(f"{where}: [WM] must have exactly {', '.join(WM_KEYS)}")
    for group in ("ColorEffects:Disabled", "ColorEffects:Inactive"):
        if sorted(groups.get(group, {})) != sorted(EFFECT_KEYS):
            errors.append(f"{where}: [{group}] must have exactly {', '.join(EFFECT_KEYS)}")
    general = groups.get("General", {})
    if not general.get("Name") or not general.get("ColorScheme"):
        errors.append(f"{where}: [General] needs Name and ColorScheme")
    return errors


def contrast_findings(groups: dict[str, dict[str, str]]) -> list[tuple[str, str, str, float, float]]:
    """(where, fg, bg, ratio, threshold) for every pair KDE draws together."""
    rows = []
    for group in ROLES:
        values = groups.get(group, {})
        for bg_key in BACKGROUNDS:
            bg = values.get(bg_key)
            for fg_key in FOREGROUNDS:
                rows.append((f"[{group}] {fg_key} on {bg_key}", values.get(fg_key), bg, 0.0, 4.5))
            for deco in DECORATIONS:
                if group == "Colors:Selection":
                    continue  # a focus ring is drawn on the surface around a selection, not on it
                rows.append((f"[{group}] {deco} on {bg_key}", values.get(deco), bg, 0.0, 3.0))
    selection = groups.get("Colors:Selection", {}).get("BackgroundNormal")
    for group in ("Colors:Window", "Colors:View"):
        rows.append((f"[Colors:Selection] BackgroundNormal against [{group}] BackgroundNormal", selection,
                     groups.get(group, {}).get("BackgroundNormal"), 0.0, 3.0))
    wm = groups.get("WM", {})
    rows.append(("[WM] activeForeground on activeBackground", wm.get("activeForeground"), wm.get("activeBackground"),
                 0.0, 4.5))
    rows.append(("[WM] inactiveForeground on inactiveBackground", wm.get("inactiveForeground"),
                 wm.get("inactiveBackground"), 0.0, 4.5))
    out = []
    for where, fg, bg, _, need in rows:
        ratio = contrast(fg, bg) if fg and bg and HEX.match(fg) and HEX.match(bg) else 0.0
        out.append((where, fg or "?", bg or "?", ratio, need))
    return out


def check_scheme_contrast(groups: dict[str, dict[str, str]], where: str) -> list[str]:
    return [f"{where}: {w} is {r:.2f}:1, below {n}:1 (WCAG 2.2 AA)" for w, fg, bg, r, n in contrast_findings(groups)
            if r < n]


def check_scheme_tokens(groups: dict[str, dict[str, str]], tokens: dict, where: str) -> list[str]:
    allowed = {entry[v] for entry in tokens["colours"].values() for v in ("dark", "light")}
    errors = []
    for group, keys in groups.items():
        for key, value in keys.items():
            if HEX.match(value) and value not in allowed and not group.startswith("ColorEffects"):
                errors.append(f"{where}: [{group}] {key}={value} is not the value of any token")
    return errors


def colours_in(text: str) -> list[str]:
    found = [m.upper() for m in re.findall(r"#[0-9A-Fa-f]{6}\b", text)]
    for match in re.finditer(r"(?<![\d.])(\d{1,3}),(\d{1,3}),(\d{1,3})(?![\d.,])", text):
        if all(int(g) <= 255 for g in match.groups()):
            found.append("#" + "".join(f"{int(g):02X}" for g in match.groups()))
    return found


def check_gold(rootfs: pathlib.Path, tokens: dict) -> list[str]:
    # P@x's own colours: its tokens, and the gold of its overlay. The overlay's
    # neutral colours (outline, hairline, quiet text) are shared with the
    # system and are not gold.
    pax = {entry[v] for entry in tokens["pax"]["colours"].values() for v in ("dark", "light")}
    pax |= {v for k, v in tokens["pax"]["overlay"].items() if k != "about" and HEX.match(v) and is_gold(v)}
    errors = []
    for path in tree_files(rootfs):
        if path.is_symlink() or path.suffix == ".png":
            continue
        relative = path.relative_to(rootfs).as_posix()
        for colour in colours_in(path.read_text(encoding="utf-8")):
            if colour in pax:
                errors.append(f"{relative}: {colour} is one of P@x's colours; gold is P@x's alone (design, 5.1)")
            elif is_gold(colour):
                errors.append(f"{relative}: {colour} is in the gold family; gold is P@x's alone (design, 5.1)")
    return errors


def check_tree(rootfs: pathlib.Path) -> list[str]:
    errors = []
    for path in tree_files(rootfs):
        relative = path.relative_to(rootfs).as_posix()
        if not any(re.fullmatch(p, relative) for p in ALLOWED_PATHS):
            errors.append(f"{relative}: not a file this tree may hold (gen_theme.py, ALLOWED_PATHS)")
            continue
        if relative.startswith("etc/xdg/"):
            expected = os.path.relpath(f"/{XDG_SOURCE}/{path.name}", "/etc/xdg")
            if not path.is_symlink() or os.readlink(path) != expected:
                errors.append(f"{relative}: must be a link to {expected}: the content lives under /usr")
            elif not (rootfs / XDG_SOURCE / path.name).is_file():
                errors.append(f"{relative}: links to a file the tree does not have")
            continue
        if path.is_symlink():
            errors.append(f"{relative}: only /etc/xdg holds links")
            continue
        text = path.read_text(encoding="utf-8")
        head = "\n".join(text.splitlines()[:5])
        if SPDX_LINE not in head:
            errors.append(f"{relative}: no '{SPDX_LINE}' in its first five lines")
        if re.search(r"fedora", text, re.IGNORECASE):
            errors.append(f"{relative}: names Fedora; the image's marks check would fail (TUR-008)")
    for name in XDG_EXPECTED:
        if not (rootfs / "etc/xdg" / name).is_symlink():
            errors.append(f"etc/xdg/{name}: missing")
    return errors


def check_lnf(rootfs: pathlib.Path) -> list[str]:
    errors = []
    schemes = {p.name for p in (rootfs / "usr/share/color-schemes").glob("*.colors")}
    for variant, theme in THEMES.items():
        base = rootfs / "usr/share/plasma/look-and-feel" / theme
        where = f"look-and-feel/{theme}"
        try:
            meta = json.loads((base / "metadata.json").read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            errors.append(f"{where}/metadata.json: {error}")
            continue
        if meta.get("KPackageStructure") != "Plasma/LookAndFeel":
            errors.append(f"{where}/metadata.json: KPackageStructure must be Plasma/LookAndFeel")
        plugin = meta.get("KPlugin", {})
        # plasma-apply-lookandfeel refuses a package whose id differs from the
        # name it was asked for (kcms/lookandfeel/tool/lnftool.cpp).
        if plugin.get("Id") != theme:
            errors.append(f"{where}/metadata.json: KPlugin.Id must be {theme}, the directory's name")
        if plugin.get("Name") != SCHEMES[variant][1]:
            errors.append(f"{where}/metadata.json: KPlugin.Name must be {SCHEMES[variant][1]}")
        if SPDX_LINE not in json.dumps(meta) or plugin.get("License") != PROJECT_ID:
            errors.append(f"{where}/metadata.json: needs the SPDX line and License {PROJECT_ID}")
        if (base / "contents" / "splash").exists() or (base / "contents" / "layouts").exists():
            errors.append(f"{where}: a splash or layout script is code inside a GPL host (AGENTS.md); data only")
        try:
            defaults = parse_ini((base / "contents/defaults").read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            errors.append(f"{where}/contents/defaults: {error}")
            continue
        for group, keys in defaults.items():
            if group == "ksplashrc][KSplash":
                errors.append(f"{where}/contents/defaults: [ksplashrc] is not applied by Plasma 6.8 without a splash "
                              "QML file; the splash is off in the system's ksplashrc")
                continue
            if group not in LNF_KEYS:
                errors.append(f"{where}/contents/defaults: [{group}] is not read by KLookAndFeelManager")
                continue
            for key in keys:
                if key not in LNF_KEYS[group]:
                    errors.append(f"{where}/contents/defaults: [{group}] {key} is not read by KLookAndFeelManager")
        expected = {**LNF_COMMON, **LNF_EXPECTED[variant]}
        for group, keys in expected.items():
            for key, value in keys.items():
                if defaults.get(group, {}).get(key) != value:
                    errors.append(f"{where}/contents/defaults: [{group}] {key} must be {value}")
        scheme = defaults.get("kdeglobals][General", {}).get("ColorScheme", "")
        # KLookAndFeelManager::colorSchemeFile: a file whose name ends in the
        # scheme's name and ".colors".
        if not any(name.endswith(scheme + ".colors") for name in schemes):
            errors.append(f"{where}: ColorScheme {scheme} names no colour scheme in the tree")
    branding = (ROOT / "image" / "branding" / "branding.sh")
    if branding.exists() and "wallpapers/Tur" not in branding.read_text(encoding="utf-8"):
        errors.append("the global themes name the wallpaper Tur, which image/branding/branding.sh no longer installs")
    return errors


def check_panel(rootfs: pathlib.Path) -> list[str]:
    errors = []
    try:
        groups = parse_ini((rootfs / APPLETSRC).read_text(encoding="utf-8"))
        views = parse_ini((rootfs / PLASMASHELLRC).read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return [f"{APPLETSRC} or {PLASMASHELLRC}: {error}"]
    containments = {g for g in groups if re.fullmatch(r"Containments\]\[\d+", g)}
    if len(containments) != 1:
        return [f"{APPLETSRC}: exactly one containment, the panel, is expected (the desktop is made per activity "
                "by the shell)"]
    panel = containments.pop()
    pid = panel.split("][")[1]
    expected = {"formfactor": "2", "location": "4", "plugin": "org.kde.panel", "lastScreen": "0",
                "immutability": "1"}
    for key, value in expected.items():
        if groups[panel].get(key) != value:
            errors.append(f"{APPLETSRC}: the panel's {key} must be {value}")
    applets = {}
    for group, keys in groups.items():
        match = re.fullmatch(rf"Containments\]\[{pid}\]\[Applets\]\[(\d+)", group)
        if match:
            applets[match.group(1)] = keys.get("plugin", "")
    order = groups.get(f"{panel}][General", {}).get("AppletOrder", "").split(";")
    if sorted(order) != sorted(applets):
        errors.append(f"{APPLETSRC}: AppletOrder {order} does not list the panel's applets {sorted(applets)}")
    if [applets.get(i) for i in order] != PANEL_APPLETS:
        errors.append(f"{APPLETSRC}: the panel must hold, in order, {PANEL_APPLETS} (design, 7.1)")
    by_plugin = {plugin: i for i, plugin in applets.items()}
    kickoff = groups.get(f"{panel}][Applets][{by_plugin.get('org.kde.plasma.kickoff')}][Configuration][General", {})
    if kickoff.get("icon") != LAUNCHER_ICON:
        errors.append(f"{APPLETSRC}: the launcher's icon must be {LAUNCHER_ICON}, TUR's mark")
    tasks = groups.get(f"{panel}][Applets][{by_plugin.get('org.kde.plasma.icontasks')}][Configuration][General", {})
    if tasks.get("launchers", "").split(",") != LAUNCHERS:
        errors.append(f"{APPLETSRC}: the pinned applications must be {LAUNCHERS}")
    if views.get(f"PlasmaViews][Panel {pid}][Defaults", {}).get("thickness") is None:
        errors.append(f"{PLASMASHELLRC}: the panel's thickness is not set")
    return errors


def check_xdg(rootfs: pathlib.Path) -> list[str]:
    errors = []
    for name, expected in XDG_EXPECTED.items():
        path = rootfs / XDG_SOURCE / name
        try:
            groups = parse_ini(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            errors.append(f"{XDG_SOURCE}/{name}: {error}")
            continue
        for group, keys in expected.items():
            for key, value in keys.items():
                if groups.get(group, {}).get(key) != value:
                    errors.append(f"{XDG_SOURCE}/{name}: [{group}] {key} must be {value}")
    welcome = parse_ini((rootfs / XDG_SOURCE / "plasma-welcomerc").read_text(encoding="utf-8")) \
        if (rootfs / XDG_SOURCE / "plasma-welcomerc").exists() else {}
    # plasma-welcome's daemon opens its window when LastSeenVersion is empty
    # (plasma-welcome v6.8.2, src/kded/daemon.cpp).
    if not welcome.get("General", {}).get("LastSeenVersion"):
        errors.append(f"{XDG_SOURCE}/plasma-welcomerc: LastSeenVersion must not be empty, or KDE's welcome opens")
    return errors


def check_all(tokens: dict, rootfs: pathlib.Path, schemes_json_path: pathlib.Path,
              manifest: pathlib.Path) -> list[str]:
    errors = check_generated(tokens, rootfs, schemes_json_path, manifest)
    for variant in ("dark", "light"):
        path = rootfs / "usr/share/color-schemes" / (SCHEMES[variant][0] + ".colors")
        where = path.name
        try:
            groups = scheme_values(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            errors.append(f"{where}: {error}")
            continue
        errors += check_scheme_format(groups, where)
        errors += check_scheme_contrast(groups, where)
        errors += check_scheme_tokens(groups, tokens, where)
    errors += check_gold(rootfs, tokens)
    errors += check_tree(rootfs)
    errors += check_lnf(rootfs)
    errors += check_panel(rootfs)
    errors += check_xdg(rootfs)
    return errors


# ---- the table for the documentation ---------------------------------------------------

def table(tokens: dict) -> str:
    out = ["| Set | Key | Dark | Light |", "|---|---|---|---|"]
    for group in list(ROLES) + ["WM"]:
        dark, light = scheme_groups(tokens, "dark")[group], scheme_groups(tokens, "light")[group]
        for key in dark:
            d, l = dark[key], light[key]
            out.append(f"| `{group}` | `{key}` | `{d[0]}` {d[1]} | `{l[0]}` {l[1]} |")
    out += ["", "| Scheme | Pair | Ratio | Needs |", "|---|---|---|---|"]
    for variant in ("dark", "light"):
        path = ROOTFS / "usr/share/color-schemes" / (SCHEMES[variant][0] + ".colors")
        groups = scheme_values(path.read_text(encoding="utf-8")) if path.exists() else {}
        findings = contrast_findings(groups)
        lowest: dict[tuple[str, float], tuple[str, float]] = {}
        for where, _, _, ratio, need in findings:
            group = where.split("]")[0] + "]"
            if (group, need) not in lowest or ratio < lowest[(group, need)][1]:
                lowest[(group, need)] = (where, ratio)
        for (group, need), (where, ratio) in lowest.items():
            out.append(f"| {SCHEMES[variant][1]} | lowest {'text' if need > 4 else 'decoration'} pair: {where} "
                       f"| {ratio:.2f} | {need} |")
        out.append(f"| {SCHEMES[variant][1]} | disabled text (exempt): fade {disabled_fade(tokens, variant)} | — | — |")
    return "\n".join(out)


# ---- self-test ------------------------------------------------------------------------

def self_test() -> int:
    """Each kind of fault, planted in a copy of the tree, must be caught."""
    tokens = load_tokens()
    failures = []

    def plant(name: str, change, expect: str) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            rootfs = tmp_path / "rootfs"
            shutil.copytree(ROOTFS, rootfs, symlinks=True)
            shutil.copyfile(SCHEMES_JSON, tmp_path / "schemes.json")
            shutil.copyfile(MANIFEST, tmp_path / "MANIFEST.tsv")
            before = check_all(tokens, rootfs, tmp_path / "schemes.json", tmp_path / "MANIFEST.tsv")
            if before:
                failures.append(f"{name}: the unchanged copy already fails: {before[:3]}")
                return
            local_tokens = change(rootfs, tokens) or tokens
            errors = check_all(local_tokens, rootfs, tmp_path / "schemes.json", tmp_path / "MANIFEST.tsv")
            if not any(expect in e for e in errors):
                failures.append(f"{name}: not caught (expected a message containing {expect!r}; got {errors[:4]})")
            else:
                print(f"caught: {name}")

    def edit(path: str, old: str, new: str):
        def change(rootfs: pathlib.Path, _tokens: dict):
            target = rootfs / path
            text = target.read_text(encoding="utf-8")
            if old not in text:
                raise SystemExit(f"self-test: {old!r} not in {path}")
            target.write_text(text.replace(old, new, 1), encoding="utf-8")
        return change

    dark = "usr/share/color-schemes/TurDark.colors"
    plant("a gold colour in a scheme", edit(dark, "ForegroundNeutral=255,154,82", "ForegroundNeutral=232,180,76"),
          "gold")
    plant("a gold colour elsewhere in the tree", edit(f"{XDG_SOURCE}/kdeglobals", "[KDE]", "[KDE]\nAccent=#C8A040"),
          "gold family")
    plant("text below 4.5:1", edit(dark, "ForegroundInactive=169,180,198", "ForegroundInactive=60,66,80"),
          "below 4.5:1")
    plant("a hand edit of a generated scheme", edit(dark, "contrast=4", "contrast=5"), "stale")

    def token_change(_rootfs: pathlib.Path, tokens_in: dict) -> dict:
        changed = json.loads(json.dumps(tokens_in))
        changed["colours"]["accent"]["dark"] = "#2AA9F9"
        return changed

    plant("a token changed and the schemes not regenerated", token_change, "stale")
    plant("a key KDE does not read", edit(dark, "[Colors:View]\n", "[Colors:View]\nBackgroundWhatever=1,2,3\n"),
          "does not read")
    plant("a splash in the global theme", edit("usr/share/plasma/look-and-feel/org.tur.dark/contents/defaults",
                                               "[Wallpaper]", "[ksplashrc][KSplash]\nTheme=x\n\n[Wallpaper]"),
          "ksplashrc")
    plant("Fedora's name", edit(f"{XDG_SOURCE}/ksplashrc", "[KSplash]", "# Fedora\n[KSplash]"), "names Fedora")
    plant("a panel out of order", edit(APPLETSRC, "AppletOrder=2;3;4;5;6", "AppletOrder=3;2;4;5;6"), "in order")
    plant("the welcome window not suppressed", edit(f"{XDG_SOURCE}/plasma-welcomerc", "LastSeenVersion=6.8.0",
                                                    "LastSeenVersion="), "LastSeenVersion")

    def missing_link(rootfs: pathlib.Path, _tokens: dict):
        (rootfs / "etc/xdg/ksplashrc").unlink()

    plant("a system default without its link in /etc/xdg", missing_link, "etc/xdg/ksplashrc")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("self-test: every planted fault was caught")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    group = ap.add_mutually_exclusive_group()
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    group.add_argument("--table", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    tokens = load_tokens()
    if args.self_test:
        return self_test()
    if args.write:
        files = generated(tokens)
        for path, text in files.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
            print(f"wrote {path.relative_to(ROOT)}")
        scheme_paths = {p.relative_to(ROOTFS).as_posix() for p in files if p != SCHEMES_JSON}
        MANIFEST.write_text(render_manifest(ROOTFS, scheme_paths), encoding="utf-8")
        print(f"wrote {MANIFEST.relative_to(ROOT)}")
    if args.table:
        print(table(tokens))
        return 0
    errors = check_all(tokens, ROOTFS, SCHEMES_JSON, MANIFEST)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        print(f"\n{len(errors)} problem(s) in desktop/data", file=sys.stderr)
        return 1
    files = tree_files(ROOTFS)
    print(f"desktop/data: {len(files)} files, two colour schemes, two global themes; tokens, gold, contrast, "
          "formats and the manifest check out")
    return 0


if __name__ == "__main__":
    sys.exit(main())

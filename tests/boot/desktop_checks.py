#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Judge what a new account's desktop got (task D-04), from the facts the guest's
desktop check sent and the two pictures of the extra boot.

    tests/boot/desktop_checks.py OUT_DIR       re-judge the facts already in OUT_DIR/desktop

Used by tests/boot/splash.py after its extra boot. The guest side is
tests/boot/guest/rootfs/usr/libexec/tur-boot-test/desktop-check. What is
expected comes from the repository: desktop/data (the theme, its manifest
and gen_theme.py's tables) and branding/wallpaper (the picture).

Writes OUT_DIR/desktop/desktop-checks.json and desktop-checks.md. A required
check that fails, or that could not be evaluated, fails the run. Needs only
the Python standard library. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import struct
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "desktop" / "data"))
import gen_theme  # noqa: E402

WALLPAPER = ROOT / "branding" / "wallpaper" / "2560x1440.png"


# ---- pictures -------------------------------------------------------------------------

def read_png(path: pathlib.Path) -> tuple[int, int, int, bytes]:
    """Width, height, bytes per pixel and the pixels of an 8-bit RGB or RGBA PNG."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path} is not a PNG")
    pos, idat, width = 8, b"", 0
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or colour not in (2, 6) or interlace:
                raise ValueError(f"{path}: only 8-bit RGB(A), not interlaced")
            bpp = 3 if colour == 2 else 4
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = width * bpp
    out = bytearray(height * stride)
    previous = bytearray(stride)
    for y in range(height):
        kind = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for x in range(stride if kind else 0):
            a = line[x - bpp] if x >= bpp else 0
            b = previous[x]
            c = previous[x - bpp] if x >= bpp else 0
            if kind == 1:
                line[x] = (line[x] + a) & 0xFF
            elif kind == 2:
                line[x] = (line[x] + b) & 0xFF
            elif kind == 3:
                line[x] = (line[x] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
        out[y * stride:(y + 1) * stride] = line
        previous = line
    return width, height, bpp, bytes(out)


def mean(picture: tuple[int, int, int, bytes], x0: float, y0: float, x1: float, y1: float) -> tuple[float, ...]:
    """Mean colour of a rectangle given in fractions of the picture's width and height."""
    width, height, bpp, px = picture
    xs = range(int(x0 * width), max(int(x0 * width) + 1, int(x1 * width)))
    ys = range(int(y0 * height), max(int(y0 * height) + 1, int(y1 * height)))
    total = [0, 0, 0]
    count = 0
    for y in ys:
        row = y * width * bpp
        for x in xs:
            i = row + x * bpp
            total[0] += px[i]
            total[1] += px[i + 1]
            total[2] += px[i + 2]
            count += 1
    return tuple(round(t / count, 1) for t in total)


def cover(screen_w: int, screen_h: int, image_w: int, image_h: int, fx: float, fy: float) -> tuple[float, float]:
    """Where a point of the screen (in fractions) falls on a picture that is scaled
    to cover the screen and cropped evenly: Plasma's default fill mode for an
    image wallpaper ("Scaled and cropped")."""
    scale = max(screen_w / image_w, screen_h / image_h)
    shown_w, shown_h = screen_w / scale, screen_h / scale
    left, top = (image_w - shown_w) / 2, (image_h - shown_h) / 2
    return (left + fx * shown_w) / image_w, (top + fy * shown_h) / image_h


# Places where nothing but the wallpaper is expected on a new account's desktop
# at 1280 x 800: the four corners above the panel and the middle, where the mark is.
WALLPAPER_PATCHES = [
    ("top left", 0.02, 0.03, 0.08, 0.11),
    ("top right", 0.92, 0.03, 0.98, 0.11),
    ("left", 0.02, 0.45, 0.08, 0.55),
    ("right", 0.92, 0.45, 0.98, 0.55),
    ("the mark", 0.47, 0.40, 0.53, 0.50),
]
WALLPAPER_TOLERANCE = 8.0
# The panel's background between the pinned applications and the tray.
PANEL_STRIP = (0.40, 0.955, 0.65, 0.97)


def wallpaper_shown(screenshot: pathlib.Path) -> tuple[bool, str]:
    shot = read_png(screenshot)
    paper = read_png(WALLPAPER)
    worst, notes = 0.0, []
    for name, x0, y0, x1, y1 in WALLPAPER_PATCHES:
        a = mean(shot, x0, y0, x1, y1)
        px0, py0 = cover(shot[0], shot[1], paper[0], paper[1], x0, y0)
        px1, py1 = cover(shot[0], shot[1], paper[0], paper[1], x1, y1)
        b = mean(paper, px0, py0, px1, py1)
        difference = max(abs(p - q) for p, q in zip(a, b))
        worst = max(worst, difference)
        notes.append(f"{name}: screen {a}, wallpaper {b}")
    return worst <= WALLPAPER_TOLERANCE, f"largest difference {worst:.1f} of 255 (allowed {WALLPAPER_TOLERANCE}); " \
        + "; ".join(notes)


# ---- facts ----------------------------------------------------------------------------

def props(path: pathlib.Path) -> dict[str, str]:
    found = {}
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                found[key.strip()] = value.strip()
    return found


def layout(path: pathlib.Path) -> dict | None:
    """plasmashell's dumpCurrentLayoutJS as busctl --json=short prints it."""
    try:
        reply = json.loads(path.read_text())
        text = bytes(reply["data"][0]).decode("utf-8")
        start = text.index("var layout = ") + len("var layout = ")
        end = text.rindex(";\n\nplasma.loadSerializedLayout")
        return json.loads(text[start:end])
    except (OSError, ValueError, KeyError, IndexError, TypeError):
        return None


def config_value(config: dict, key: str) -> str | None:
    """A key from a dumped config, in whichever group it is."""
    for group, keys in config.items():
        if isinstance(keys, dict) and key in keys:
            return keys[key]
    return None


def scheme_colour(variant: str, group: str, key: str) -> str:
    path = gen_theme.ROOTFS / "usr/share/color-schemes" / (gen_theme.SCHEMES[variant][0] + ".colors")
    return gen_theme.parse_ini(path.read_text(encoding="utf-8"))[group][key]


def judge(out: pathlib.Path) -> list[dict]:
    facts = out / "desktop" / "tur-desktop-check"
    checks: list[dict] = []

    def check(name: str, required: bool, ok: bool | None, detail: str) -> None:
        checks.append({"name": name, "required": required,
                       "result": "not-run" if ok is None else ("pass" if ok else "fail"), "detail": detail})

    if not facts.is_dir():
        check("the guest's desktop check sent its facts", True, False,
              f"nothing in {facts}; see splash/serial.log for DESKTOP-CHECK markers")
        return checks
    status = props(facts / "status")
    dark, light = props(facts / "dark" / "settings.txt"), props(facts / "light" / "settings.txt")
    check("the account had a Plasma session", True, bool(status.get("plasmashell_pid")),
          f"plasmashell pid {status.get('plasmashell_pid') or 'none'} at uptime {status.get('plasmashell_seen_uptime')}")

    # The files the image installed are the repository's.
    manifest = [line.split("\t") for line in gen_theme.MANIFEST.read_text().splitlines() if line.startswith("/")]
    installed = {}
    for line in (facts / "installed-sha256.txt").read_text().splitlines() if (facts / "installed-sha256.txt").exists() \
            else []:
        digest, _, path = line.partition("  ")
        installed[path.strip()] = digest
    links = dict(line.split("\t", 1) for line in (facts / "installed-links.txt").read_text().splitlines()
                 if "\t" in line) if (facts / "installed-links.txt").exists() else {}
    wrong = []
    for path, kind, _origin, value in manifest:
        if kind == "file" and installed.get(path) != value:
            wrong.append(f"{path}: {installed.get(path, 'absent')}")
        if kind == "link" and links.get(path) != value:
            wrong.append(f"{path} -> {links.get(path, 'absent')}")
    check("the image has desktop/data's files as MANIFEST.tsv lists them", True, not wrong and bool(manifest),
          "; ".join(wrong) or f"{len(manifest)} files and links match")

    # A new account gets TUR Dark.
    package = (facts / "dark" / "kdedefaults" / "package")
    package = package.read_text().strip() if package.exists() else ""
    check("a new account gets the global theme TUR Dark", True, package == "org.tur.dark",
          f"kdedefaults/package: {package or 'absent'}; LookAndFeelPackage {dark.get('kdeglobals/KDE/LookAndFeelPackage')}")
    window = scheme_colour("dark", "Colors:Window", "BackgroundNormal")
    check("its colours are TUR Dark's", True,
          dark.get("kdeglobals/General/ColorScheme") == "TurDark"
          and dark.get("kdeglobals/Colors:Window/BackgroundNormal") == window,
          f"ColorScheme {dark.get('kdeglobals/General/ColorScheme')}, window background "
          f"{dark.get('kdeglobals/Colors:Window/BackgroundNormal')} (TurDark.colors: {window})")
    check("Breeze icons, cursor and window frame", True,
          dark.get("kdeglobals/Icons/Theme") == "breeze-dark" and dark.get("kcminputrc/Mouse/cursorTheme") ==
          "breeze_cursors" and dark.get("kwinrc/org.kde.kdecoration2/library") == "org.kde.breeze",
          f"icons {dark.get('kdeglobals/Icons/Theme')}, cursor {dark.get('kcminputrc/Mouse/cursorTheme')}, "
          f"decoration {dark.get('kwinrc/org.kde.kdecoration2/library')}")
    check("files open with a double click", True, dark.get("kdeglobals/KDE/SingleClick") == "false",
          f"SingleClick {dark.get('kdeglobals/KDE/SingleClick')}")

    # The panel, as plasmashell holds it.
    shell = layout(facts / "dark" / "layout.json")
    if shell is None:
        check("the panel as designed", True, None, "plasmashell's layout could not be read")
    else:
        panels = shell.get("panels", [])
        problems = []
        if len(panels) != 1:
            problems.append(f"{len(panels)} panels")
        else:
            panel = panels[0]
            plugins = [a.get("plugin") for a in panel.get("applets", [])]
            if panel.get("location") != "bottom":
                problems.append(f"location {panel.get('location')}")
            if panel.get("floating") is not True:
                problems.append(f"floating {panel.get('floating')}")
            if plugins != gen_theme.PANEL_APPLETS:
                problems.append(f"applets {plugins}")
            applets = {a.get("plugin"): a.get("config", {}) for a in panel.get("applets", [])}
            icon = config_value(applets.get("org.kde.plasma.kickoff", {}), "icon")
            launchers = config_value(applets.get("org.kde.plasma.icontasks", {}), "launchers")
            if icon != gen_theme.LAUNCHER_ICON:
                problems.append(f"launcher icon {icon}")
            if (launchers or "").split(",") != gen_theme.LAUNCHERS:
                problems.append(f"pinned {launchers}")
        check("the panel as designed: bottom, floating; launcher with TUR's mark; Files, Firefox, Settings pinned; "
              "tasks, tray, clock", True, not problems, "; ".join(problems) or json.dumps(panels)[:600])
        desktops = shell.get("desktops", [])
        image = config_value(desktops[0].get("config", {}), "Image") if desktops else None
        check("the desktop shows the global theme's wallpaper (no picture of its own)", True,
              bool(desktops) and desktops[0].get("wallpaperPlugin") == "org.kde.image" and not image,
              f"{len(desktops)} desktop(s); plugin {desktops[0].get('wallpaperPlugin') if desktops else None}; "
              f"own picture {image!r}")
    shot = out / "screenshot-desktop.png"
    if shot.exists():
        ok, detail = wallpaper_shown(shot)
        check("TUR's wallpaper is on the screen (screenshot-desktop.png against branding/wallpaper)", True, ok, detail)
    else:
        check("TUR's wallpaper is on the screen", True, None, "no screenshot-desktop.png")

    # No KDE welcome window; no Plasma splash.
    processes = (facts / "welcome-and-splash-processes.txt")
    processes = processes.read_text().strip() if processes.exists() else ""
    journal = (facts / "user-journal.txt").read_text(errors="replace") if (facts / "user-journal.txt").exists() else ""
    check("KDE's welcome window did not open", True,
          "plasma-welcome" not in processes and "Launching Welcome Center" not in journal
          and bool(dark.get("plasma-welcomerc/General/LastSeenVersion")),
          f"processes: {processes or 'none'}; LastSeenVersion {dark.get('plasma-welcomerc/General/LastSeenVersion')!r}")
    # The splash is a user service; the account's journal says whether it ran.
    splash_lines = [line for line in journal.splitlines() if "plasma-ksplash.service" in line or "ksplashqml" in line]
    check("Plasma's sign-in splash did not run", True,
          dark.get("ksplashrc/KSplash/Engine") == "none" and not splash_lines and "ksplashqml" not in processes,
          f"Engine {dark.get('ksplashrc/KSplash/Engine')}; journal: {splash_lines[:3] or 'no plasma-ksplash.service, '
                                                                     'no ksplashqml'}")

    # The themes offered.
    offered = (facts / "lookandfeel-list.txt").read_text().split() if (facts / "lookandfeel-list.txt").exists() else []
    check("the system offers TUR Dark and TUR Light", True, "org.tur.dark" in offered and "org.tur.light" in offered,
          f"plasma-apply-lookandfeel --list: {offered}")
    setup = (facts / "plasma-setup-themes.txt").read_text() if (facts / "plasma-setup-themes.txt").exists() else ""
    names = sorted(set(re.findall(r"org\.[A-Za-z0-9.]+\.desktop", setup)))
    check("plasma-setup's light/dark switch names TUR's themes", False,
          all(n in offered for n in names) and bool(names),
          f"names compiled into plasma-setup: {names or 'none found'}; a configuration cannot change them "
          "(docs/architecture/tur-theme.md, section 7)")
    login = (facts / "login-screen-package.txt").read_text().strip() if (facts / "login-screen-package.txt").exists() \
        else ""
    check("the login screen took TUR Dark (as of the boot test's earlier boots)", False, login == "org.tur.dark",
          f"/var/lib/plasmalogin/.config/kdedefaults/package: {login or 'absent'}")
    favourites = (facts / "dark" / "user-kactivitymanagerd-statsrc")
    favourites = favourites.read_text() if favourites.exists() else ""
    check("the launcher's favourites are Files, Firefox, Settings", False,
          all(app in favourites for app in ("org.kde.dolphin.desktop", "org.mozilla.firefox.desktop",
                                            "systemsettings.desktop")),
          " ".join(line for line in favourites.splitlines() if "ordering" in line)[:400] or "no favourites recorded")

    # TUR Light, applied as System Settings applies it.
    light_window = scheme_colour("light", "Colors:Window", "BackgroundNormal")
    check("TUR Light applies to the account", True,
          status.get("light_apply_status") == "0" and light.get("kdeglobals/General/ColorScheme") == "TurLight"
          and light.get("kdeglobals/Colors:Window/BackgroundNormal") == light_window
          and light.get("kdeglobals/Icons/Theme") == "breeze",
          f"exit {status.get('light_apply_status')}; ColorScheme {light.get('kdeglobals/General/ColorScheme')}; "
          f"window background {light.get('kdeglobals/Colors:Window/BackgroundNormal')} (TurLight.colors: "
          f"{light_window}); icons {light.get('kdeglobals/Icons/Theme')}")
    shots = {"dark": out / "screenshot-desktop.png", "light": out / "screenshot-desktop-light.png"}
    if all(p.exists() for p in shots.values()):
        strips = {v: mean(read_png(p), *PANEL_STRIP) for v, p in shots.items()}
        bright = {v: sum(c) / 3 for v, c in strips.items()}
        # The panel is translucent over the wallpaper while no window touches
        # it, so the light panel is lighter, not white.
        check("the panel is dark in TUR Dark and light in TUR Light (pictures)", True,
              bright["dark"] < 60 and bright["light"] - bright["dark"] >= 80,
              f"panel strip mean: dark {strips['dark']}, light {strips['light']}")
    else:
        check("the panel is dark in TUR Dark and light in TUR Light (pictures)", True, None,
              f"missing: {[str(p.name) for p in shots.values() if not p.exists()]}")
    checks += judge_programs(out, facts, status)
    return checks


# TUR's desktop programs in the account's session (issue #59): the name the
# guest's desktop check shows each under, what it is, and whether it is a
# layer-shell surface (P@x's window and overlay) or an ordinary window.
PROGRAMS = [
    ("settings", "tur-settings, from its launcher entry", False),
    ("centre", "tur-centre, from its launcher entry", False),
    ("centre-app", "tur-centre on an application's page, by its link", False),
    ("pax-ask", "tur-pax-ui's ask bar, from its launcher entry", True),
    ("pax-setup", "tur-pax-ui's setup flow, asked of the running window", True),
    ("pax-keys", "tur-pax-overlay showing P@x's keys, started by the session bus", True),
]
# The least share of the screen that must differ from the picture of the
# desktop before the program was started: a window, and a layer-shell
# surface, which is smaller.
CHANGED_WINDOW, CHANGED_SURFACE = 0.05, 0.003


def judge_programs(out: pathlib.Path, facts: pathlib.Path, status: dict[str, str]) -> list[dict]:
    checks: list[dict] = []

    def check(name: str, required: bool, ok: bool | None, detail: str) -> None:
        checks.append({"name": name, "required": required,
                       "result": "not-run" if ok is None else ("pass" if ok else "fail"), "detail": detail})

    try:
        splash = json.loads((out / "splash" / "splash.json").read_text())
    except (OSError, ValueError):
        splash = {}
    pictures = {record.get("name"): record for record in splash.get("programs") or []}
    directory = facts / "programs"
    for name, what, surface in PROGRAMS:
        pid = status.get(f"program_{name}_pid", "")
        process = status.get(f"program_{name}_process", "?")
        said = ""
        for suffix in ("err", "out"):
            path = directory / f"{name}.{suffix}"
            if path.exists():
                said += path.read_text(errors="replace")
        said = " / ".join(line.strip() for line in said.splitlines() if line.strip())
        check(f"{what}: the program runs in the session, as a Wayland client", True,
              bool(pid) and status.get(f"program_{name}_wayland_client", "0") not in ("", "0")
              and (not surface or status.get(f"program_{name}_layer_shell", "0") not in ("", "0")),
              f"{process} pid {pid or 'none'}; Qt's Wayland client library mapped: "
              f"{status.get(f'program_{name}_wayland_client', 'no data')}; LayerShellQt mapped: "
              f"{status.get(f'program_{name}_layer_shell', 'no data')}; TUR's libraries: "
              f"{status.get(f'program_{name}_tur_libraries', 'no data')}; it printed: {said[:300] or 'nothing'}")
        record = pictures.get(name) or {}
        changed = record.get("changed_from_light_desktop")
        least = CHANGED_SURFACE if surface else CHANGED_WINDOW
        check(f"{what}: the picture of the screen shows something the desktop did not", True,
              None if record.get("file") is None or changed is None else changed >= least,
              f"{record.get('file') or 'no picture'}; {changed} of the screen differs from the light desktop "
              f"(at least {least}); at {record.get('at_seconds')} s")
        warnings = [line for line in said.split(" / ") if "QML warning" in line or "qml:" in line.lower()]
        check(f"{what}: no QML warning in the session", False, bool(pid) and not warnings,
              f"{len(warnings)} line(s): {' / '.join(warnings)[:400] or 'none'}")
    names = (directory / "bus-names.txt").read_text(errors="replace") if (directory / "bus-names.txt").exists() else ""
    owned = sorted(set(re.findall(r"org\.tur\.[A-Za-z0-9.]+", names)))
    check("P@x's window and its overlay are on the session bus while the window is open", True,
          all(name in owned for name in ("org.tur.PaxUi1", "org.tur.PaxOverlay1")),
          f"names owned: {', '.join(owned) or 'none'}")
    unit = (directory / "tur-pax-unit.txt").read_text(errors="replace") \
        if (directory / "tur-pax-unit.txt").exists() else ""
    check("P@x's window started P@x's session service through the session bus", False,
          "org.tur.Agent1" in owned and "ActiveState=active" in unit,
          f"org.tur.Agent1 owned: {'org.tur.Agent1' in owned}; tur-pax.service: {' '.join(unit.split()) or 'no data'}")
    icon = (directory / "icon-org.tur.Pax.path").read_text(errors="replace").strip() \
        if (directory / "icon-org.tur.Pax.path").exists() else ""
    check("the icon theme finds P@x's icon, org.tur.Pax", True, icon.endswith("org.tur.Pax.svg"),
          f"kiconfinder6: {icon[:200] or 'no answer'}; picture for people: icon-org.tur.Pax.png "
          f"(magick exit {status.get('icon_render_status')})")
    return checks


def write(out: pathlib.Path, checks: list[dict]) -> int:
    target = out / "desktop"
    target.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if c["required"] and c["result"] != "pass"]
    (target / "desktop-checks.json").write_text(json.dumps({"checks": checks, "failed": len(failed)}, indent=2) + "\n")
    lines = ["### A new account's desktop (task D-04)", "", "| Check | Required | Result | Detail |", "|---|---|---|---|"]
    for c in checks:
        detail = c["detail"].replace("|", "\\|")[:500]
        lines.append(f"| {c['name']} | {'yes' if c['required'] else 'no'} | **{c['result']}** | {detail} |")
    lines.append("")
    lines.append(f"{len(checks)} checks; {len(failed)} required check(s) failed or could not be evaluated.")
    (target / "desktop-checks.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 1 if failed else 0


def main() -> int:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[1]).resolve()
    return write(out, judge(out))


if __name__ == "__main__":
    sys.exit(main())

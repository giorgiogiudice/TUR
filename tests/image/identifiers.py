#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Hold the names the policy engine has built in against what the image holds.

    tests/image/identifiers.py IMAGE OUT_DIR

P@x's policy engine (services/tur-pax) carries three lists of names: the
applications in which every action needs approval, the applications it never
looks at, and the units, files and packages a repair session never covers. A
wrong name fails open. The lists were checked against published sources; "the
TUR image ... is still the place that decides" (docs/architecture/
agent-policy.md, section 2.5). This looks there.

It reads the lists from the source, without changing it:

    services/tur-pax/src/policy.rs   DEFAULT_SENSITIVE_APPLICATIONS,
                                     DEFAULT_EXCLUDED_APPLICATIONS
    services/tur-pax/src/action.rs   PROTECTED_TARGETS

and from the image: the desktop files, the programs, the unit files, the
package names and the configuration paths. It writes OUT_DIR/identifiers.json
and OUT_DIR/identifiers.md and prints a summary. It decides nothing and
fails only if it cannot read one side: what it finds is for a person to
judge, and is reported in docs/reports/TUR-006.md.

What it cannot see: a window class, which exists only while the application
runs, and anything installed later as a Flatpak.

PODMAN may name the container tool, for example PODMAN="sudo podman".
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
PODMAN = shlex.split(os.environ.get("PODMAN", "podman"))

PROBE = r"""
echo "### desktop"
find /usr/share/applications /usr/share/kglobalaccel /etc/xdg/autostart /usr/share/xsessions \
     /usr/share/wayland-sessions -name '*.desktop' 2>/dev/null | LC_ALL=C sort | while read -r file; do
    field() { grep -m1 "^$1=" "$file" | cut -d= -f2- | tr '\t' ' '; }
    printf '%s\t%s\t%s\t%s\t%s\n' "$file" "$(field Name)" "$(field Exec)" "$(field Categories)" "$(field NoDisplay)"
done
echo "### programs"
find /usr/bin /usr/sbin /usr/libexec -maxdepth 3 \( -type f -o -type l \) 2>/dev/null | LC_ALL=C sort
echo "### units"
find /usr/lib/systemd/system /usr/lib/systemd/user /etc/systemd/system /etc/systemd/user -maxdepth 1 \
     \( -type f -o -type l \) 2>/dev/null | LC_ALL=C sort
echo "### packages"
rpm -qa --qf '%{NAME}\n' | LC_ALL=C sort
echo "### paths"
find /etc /usr/etc -maxdepth 2 2>/dev/null | LC_ALL=C sort
"""

# Names worth a look when no list covers them: what executes typed text, what
# holds or asks for secrets, and what a person authenticates to.
APPLICATION_HINT = re.compile(
    r"konsole|yakuake|krunner|terminal|console|kwallet|ksecret|keyring|seahorse|keepass|pinentry|askpass|"
    r"polkit|passw|kgpg|kleopatra|plasma-?login|login|authenticat|ssh-|gpg|secret|unlock|kdesu",
    re.I,
)
UNIT_HINT = re.compile(
    r"polkit|firewall|nftables|iptables|ebtables|arptables|audit|journal|logind|homed|sshd|sssd|fprint|selinux|"
    r"apparmor|fapolicy|usbguard|userdb|sddm|greetd|plasma-?login|display-manager|pcr|tpm|crypt|repart|growfs|"
    r"multipath|udisks|makefs|mkswap|fsck|bootloader|systemd-boot|bootupd|grub|ostree|bootc|lvm|mdadm|mdmon|"
    r"clevis|dbus|kwallet|keyring|pcscd|verity|integrity|accounts-daemon|sudo|krb5|restorecon|sysupdate|"
    r"measure|^tur-|shadow|pam|secret|gpg|ssh",
    re.I,
)


def parse_applications() -> dict[str, list[dict]]:
    text = (ROOT / "services/tur-pax/src/policy.rs").read_text()
    start = text.index("list DEFAULT_SENSITIVE_APPLICATIONS;")
    middle = text.index("list DEFAULT_EXCLUDED_APPLICATIONS;")
    # The second invocation of the macro ends at the first line that closes it.
    end = text.index("\n}\n", middle)
    row = re.compile(r'\(\s*"([^"]+)",\s*(DesktopFile|WindowClass|Program|Unverified),\s*"((?:[^"\\]|\\.)*)"\s*\)')
    found = {}
    for name, part in (("sensitive", text[start:middle]), ("excluded", text[middle:end])):
        found[name] = [{"id": m.group(1), "evidence": m.group(2), "source": m.group(3)} for m in row.finditer(part)]
    return found


def parse_targets() -> list[dict]:
    text = (ROOT / "services/tur-pax/src/action.rs").read_text()
    start = text.index("pub const PROTECTED_TARGETS")
    end = text.index("\n];", start)
    row = re.compile(r'protect\(\s*TargetRule::(\w+),\s*"([^"]+)",\s*PrivilegedOperation::(\w+),?\s*\)')
    return [{"rule": m.group(1), "prefix": m.group(2), "operation": m.group(3)} for m in row.finditer(text[start:end])]


def sections(text: str) -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    current = None
    for line in text.splitlines():
        if line.startswith("### "):
            current = line[4:]
            found[current] = []
        elif current is not None and line:
            found[current].append(line)
    return found


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    image, out = sys.argv[1], pathlib.Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)

    applications = parse_applications()
    targets = parse_targets()
    if len(applications["sensitive"]) < 20 or len(applications["excluded"]) < 20 or len(targets) < 40:
        print("the lists could not be read from services/tur-pax: the source changed shape", file=sys.stderr)
        return 1

    probe = subprocess.run(
        PODMAN + ["run", "--rm", "--network", "none", image, "/bin/bash", "-c", PROBE],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    seen = sections(probe)
    desktop = []
    for line in seen.get("desktop", []):
        path, name, command, categories, hidden = (line.split("\t") + [""] * 5)[:5]
        desktop.append(
            {
                "id": pathlib.PurePosixPath(path).name.removesuffix(".desktop"),
                "path": path,
                "name": name,
                "exec": command,
                "categories": categories,
                "no_display": hidden == "true",
            }
        )
    programs = sorted({pathlib.PurePosixPath(path).name: path for path in seen.get("programs", [])}.items())
    program_names = {name.lower(): path for name, path in programs}
    units = sorted({pathlib.PurePosixPath(path).name: path for path in seen.get("units", [])}.items())
    packages = seen.get("packages", [])
    paths = seen.get("paths", [])
    if not desktop or not units or not packages:
        print("the image gave no desktop files, units or packages: the probe failed", file=sys.stderr)
        return 1
    desktop_ids = {entry["id"].lower(): entry for entry in desktop}

    # ---- applications: is each built-in name in the image, and as what? ---------------
    listed: dict[str, str] = {}
    rows = []
    for which in ("sensitive", "excluded"):
        for entry in applications[which]:
            key = entry["id"].lower()
            listed[key] = which
            as_desktop = desktop_ids.get(key)
            as_program = program_names.get(key)
            rows.append(
                {
                    "list": which,
                    "id": entry["id"],
                    "evidence": entry["evidence"],
                    "desktop_file": as_desktop["path"] if as_desktop else None,
                    "program": as_program,
                }
            )

    def covered(name: str) -> str | None:
        key = name.lower()
        return listed.get(key) or listed.get(key.removesuffix(".desktop"))

    # ---- applications in the image that look like the kinds the lists are for ---------
    candidates = []
    for entry in desktop:
        terminal = "TerminalEmulator" in entry["categories"]
        if terminal or APPLICATION_HINT.search(entry["id"]) or APPLICATION_HINT.search(entry["exec"]):
            program = pathlib.PurePosixPath(entry["exec"].split(" ")[0]).name if entry["exec"] else ""
            candidates.append(
                {
                    "desktop_file": entry["path"],
                    "id": entry["id"],
                    "name": entry["name"],
                    "exec": entry["exec"],
                    "terminal_emulator": terminal,
                    "id_on_list": covered(entry["id"]),
                    "program": program,
                    "program_on_list": covered(program) if program else None,
                }
            )
    program_candidates = [
        {"program": name, "path": path, "on_list": covered(name)}
        for name, path in programs
        if APPLICATION_HINT.search(name) and not path.startswith("/usr/sbin/")
    ]

    # ---- protected targets --------------------------------------------------------------
    def matches(rule: str, prefix: str) -> list[str]:
        if rule == "Unit":
            return [name for name, _ in units if name.lower().startswith(prefix.lower())]
        if rule == "Package":
            return [name for name in packages if name.lower().startswith(prefix.lower())]
        return [path for path in paths if path.startswith(prefix)]

    target_rows = []
    for target in targets:
        found = matches(target["rule"], target["prefix"])
        target_rows.append({**target, "matches": len(found), "examples": found[:12]})
    unit_prefixes = [target["prefix"].lower() for target in targets if target["rule"] == "Unit"]
    uncovered_units = [
        {"unit": name, "path": path}
        for name, path in units
        if UNIT_HINT.search(name) and not any(name.lower().startswith(prefix) for prefix in unit_prefixes)
    ]

    in_image = [row for row in rows if row["desktop_file"] or row["program"]]
    wrong_kind = [
        row
        for row in in_image
        if (row["evidence"] == "DesktopFile" and not row["desktop_file"])
        or (row["evidence"] == "Program" and not row["program"])
    ]
    uncovered_applications = [
        entry for entry in candidates if not entry["id_on_list"] and not entry["program_on_list"]
    ]
    document = {
        "image": image,
        "counts": {
            "sensitive_applications": len(applications["sensitive"]),
            "excluded_applications": len(applications["excluded"]),
            "protected_targets": len(targets),
            "desktop_files_in_image": len(desktop),
            "programs_in_image": len(programs),
            "units_in_image": len(units),
        },
        "applications": rows,
        "applications_in_image": in_image,
        "applications_in_image_as_another_kind_of_name": wrong_kind,
        "candidates_in_image": candidates,
        "candidates_in_image_on_no_list": uncovered_applications,
        "candidate_programs_in_image": program_candidates,
        "protected_targets": target_rows,
        "unit_prefixes_matching_nothing": [
            row["prefix"] for row in target_rows if row["rule"] == "Unit" and not row["matches"]
        ],
        "units_in_image_no_prefix_covers": uncovered_units,
        "all_units": [name for name, _ in units],
        "all_desktop_files": [entry["path"] for entry in desktop],
    }
    (out / "identifiers.json").write_text(json.dumps(document, indent=1) + "\n")

    lines = [
        "### Built-in names of the policy engine, held against the image",
        "",
        f"{len(rows)} application identifiers and {len(targets)} protected targets in `services/tur-pax`; "
        f"{len(desktop)} desktop files, {len(programs)} programs and {len(units)} unit files in the image.",
        "",
        "#### Application identifiers that are in the image",
        "",
        "| List | Identifier | Kind in the list | Desktop file in the image | Program in the image |",
        "|---|---|---|---|---|",
    ]
    for row in in_image:
        lines.append(
            f"| {row['list']} | `{row['id']}` | {row['evidence']} | `{row['desktop_file'] or '-'}` | `{row['program'] or '-'}` |"
        )
    lines += [
        "",
        f"{len(rows) - len(in_image)} identifiers name nothing in the image (the application is not installed).",
        "",
        "#### Desktop files in the image that look like a terminal, a launcher, a keyring or a prompt",
        "",
        "| Desktop file | Name | Exec | Terminal | Identifier on a list | Program on a list |",
        "|---|---|---|---|---|---|",
    ]
    for entry in candidates:
        lines.append(
            f"| `{entry['desktop_file']}` | {entry['name']} | `{entry['exec'][:60]}` | "
            f"{'yes' if entry['terminal_emulator'] else ''} | {entry['id_on_list'] or '**no**'} | "
            f"{entry['program_on_list'] or ('**no**' if entry['program'] else '-')} |"
        )
    lines += [
        "",
        "#### Programs in the image with such a name",
        "",
        ", ".join(
            f"`{entry['program']}` ({entry['on_list'] or '**on no list**'})" for entry in program_candidates
        )
        or "none",
        "",
        "#### Protected targets",
        "",
        "| Rule | Prefix | In the image | Examples |",
        "|---|---|---:|---|",
    ]
    for row in target_rows:
        lines.append(
            f"| {row['rule']} | `{row['prefix']}` | {row['matches']} | {', '.join(f'`{name}`' for name in row['examples'][:6])} |"
        )
    lines += [
        "",
        "#### Units in the image with a security-relevant name that no prefix covers",
        "",
        ", ".join(f"`{entry['unit']}`" for entry in uncovered_units) or "none",
        "",
    ]
    (out / "identifiers.md").write_text("\n".join(lines))

    print(f"application identifiers: {len(rows)}; in the image: {len(in_image)}")
    print(f"in the image under another kind of name than the list says: {[row['id'] for row in wrong_kind]}")
    print(f"desktop files of the protected kinds on no list: {[entry['id'] for entry in uncovered_applications]}")
    print(f"unit prefixes that match no unit in the image: {document['unit_prefixes_matching_nothing']}")
    print(f"security-relevant units no prefix covers: {len(uncovered_units)}")
    print(f"written: {out / 'identifiers.json'}, {out / 'identifiers.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Generate the TUR build manifest from a built image.

    scripts/gen-manifest.py IMAGE OUTPUT.json [--sbom sbom.spdx.json]

Every value is read from the image or from the build inputs. Nothing is copied
from a template: a value that cannot be determined is recorded as null.
PODMAN may name the container tool, for example PODMAN="sudo podman".
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PODMAN = shlex.split(os.environ.get("PODMAN", "podman"))

PACKAGES = [
    # boot and base
    "kernel", "systemd", "dracut", "bootc", "ostree", "rpm-ostree",
    "grub2-efi-x64", "shim-x64", "systemd-boot-unsigned", "selinux-policy-targeted",
    # desktop
    "plasma-workspace", "plasma-desktop", "kwin", "qt6-qtbase", "kf6-kio",
    "sddm", "plasma-login-manager", "xdg-desktop-portal", "xdg-desktop-portal-kde",
    "xorg-x11-server-Xwayland",
    # hardware and services
    "mesa-dri-drivers", "mesa-vulkan-drivers", "pipewire", "wireplumber",
    "NetworkManager", "bluez", "udisks2", "libblockdev", "fwupd", "flatpak",
    "bubblewrap",
    # what TUR's own programs start
    "util-linux", "pciutils", "usbutils", "vulkan-tools", "python3", "sudo",
]

PROBE = r"""
set -u
k=$(ls -1 /usr/lib/modules | head -n1)
cfg=/usr/lib/modules/$k/config
echo "v:kernel_release=$k"
echo "v:kernel_config_sha256=$(sha256sum "$cfg" | cut -d' ' -f1)"
if [ -s "/usr/lib/modules/$k/initramfs.img" ]; then
    echo "v:initramfs_sha256=$(sha256sum "/usr/lib/modules/$k/initramfs.img" | cut -d' ' -f1)"
fi
for opt in CONFIG_NTSYNC CONFIG_KVM CONFIG_SECURITY_SELINUX CONFIG_ANDROID_BINDER_IPC_RUST; do
    val=$(grep -E "^${opt}=" "$cfg" | cut -d= -f2)
    echo "k:${opt}=${val:-absent}"
done
# Every binder-related option under whatever name this kernel uses, set or not.
grep -E "CONFIG_[A-Z0-9_]*BINDER[A-Z0-9_]*(=| is not set)" "$cfg" \
    | sed -E 's/^# (CONFIG_[A-Z0-9_]+) is not set$/\1=n/' | sed 's/^/k:/'
ls /usr/lib/modules/"$k"/kernel/drivers/android 2>/dev/null | sed 's/^/v:android_module=/' | head -n 5
echo "v:rpm_count=$(rpm -qa | wc -l)"
for p in __PACKAGES__; do
    if v=$(rpm -q --qf '%{EVR}' "$p" 2>/dev/null); then echo "p:$p=$v"; fi
done
sed 's/^/o:/' /usr/lib/os-release
sed 's/^/u:/' /usr/lib/tur/release
echo "v:default_target=$(systemctl get-default 2>/dev/null)"
echo "v:selinux=$(grep -E '^SELINUX=' /etc/selinux/config | cut -d= -f2)"
dm=$(readlink /etc/systemd/system/display-manager.service 2>/dev/null || true)
echo "v:display_manager=$(basename "${dm:-}" .service)"
# TUR's own programs, from the list the build wrote. Each is run for its
# version and measured as it is in the image.
sum() { sha256sum "$1" | cut -d' ' -f1; }
grep -v '^#' /usr/lib/tur/programs.tsv 2>/dev/null | while IFS=$'\t' read -r program path component tools; do
    said=$("$path" --version 2>/dev/null </dev/null | head -n1)
    manifest=; adapter=
    if [ "$tools" != "-" ]; then
        manifest=$(sum "$tools/agent-tools.json"); adapter=$(sum "$tools/agent-tools.adapter.json")
    fi
    printf 'g:%s=%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$program" "$path" "$component" "${said##* }" \
        "$(sum "$path")" "$(stat -c %s "$path")" "$tools" "$manifest" "$adapter"
done
catalogue=/usr/share/tur/catalogue
if [ -d "$catalogue" ]; then
    echo "v:catalogue_entries=$(ls -1 "$catalogue"/*.toml | wc -l)"
    echo "v:catalogue_sha256=$(cd "$catalogue" && LC_ALL=C ls -1 *.toml | xargs sha256sum | sha256sum | cut -d' ' -f1)"
fi
crates=/usr/share/licenses/tur-third-party/crates.tsv
if [ -s "$crates" ]; then
    echo "v:crates=$(grep -v '^#' "$crates" | awk -F'\t' '$2 != "rust-std"' | cut -f2,3 | sort -u | wc -l)"
    echo "v:crates_sha256=$(sum "$crates")"
    echo "v:rust=$(grep -v '^#' "$crates" | awk -F'\t' '$2 == "rust-std" { print $3; exit }')"
fi
"""


def run(cmd: list[str], **kw) -> str:
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


def unquote(value: str) -> str:
    parts = shlex.split(value)
    return parts[0] if parts else ""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("output")
    ap.add_argument("--sbom", default=None, help="SBOM file name to reference")
    args = ap.parse_args()

    probe = PROBE.replace("__PACKAGES__", " ".join(PACKAGES))
    out = run(PODMAN + ["run", "--rm", "--network", "none", args.image, "/bin/bash", "-c", probe])

    values: dict[str, str] = {}
    kconfig: dict[str, str] = {}
    packages: dict[str, str] = {}
    osrel: dict[str, str] = {}
    turrel: dict[str, str] = {}
    programs: dict[str, str] = {}
    targets = {"v": values, "k": kconfig, "p": packages, "o": osrel, "u": turrel, "g": programs}
    for line in out.splitlines():
        kind, _, rest = line.partition(":")
        key, sep, value = rest.partition("=")
        if kind not in targets or not sep:
            continue
        targets[kind][key] = unquote(value) if kind in ("o", "u") else value

    # TUR's own programs, in the order of the list in the image.
    tur_programs = []
    for name, fields in programs.items():
        path, component, version, sha256, size, tools, tools_manifest, tools_adapter = fields.split("\t")
        tur_programs.append(
            {
                "name": name,
                "version": version or None,
                "path": path,
                "sha256": sha256,
                "size_bytes": int(size),
                "component": component,
                "assistant_tools": None
                if tools == "-"
                else {"directory": tools, "manifest_sha256": tools_manifest, "adapter_sha256": tools_adapter},
            }
        )

    inspect = json.loads(run(PODMAN + ["image", "inspect", args.image]))[0]
    arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(inspect.get("Architecture"), inspect.get("Architecture"))

    lock: dict[str, str] = {}
    for line in (ROOT / "image" / "base.lock").read_text().splitlines():
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            lock[key] = value

    def pkg(name: str):
        return packages.get(name)

    manifest = {
        "schema_version": 2,
        "tur_version": turrel.get("TUR_VERSION"),
        "build_id": turrel.get("TUR_BUILD_ID"),
        "source_revision": turrel.get("TUR_SOURCE_REVISION"),
        "source_date_epoch": int(run(["git", "-C", str(ROOT), "show", "-s", "--format=%ct", "HEAD"]).strip()),
        "architecture": arch,
        "image": {
            "id": inspect.get("Id"),
            "digest": inspect.get("Digest") or None,
            "size_bytes": inspect.get("Size"),
        },
        "base": {
            "image": turrel.get("TUR_BASE_IMAGE"),
            "tag": lock.get("BASE_TAG"),
            "digest": turrel.get("TUR_BASE_DIGEST"),
            # The image's os-release is TUR's (image/branding/branding.sh); what
            # the base called itself is in the TUR release file. An image built
            # before that change has only its os-release to go by.
            "version": turrel.get("TUR_BASE_VERSION") or osrel.get("OSTREE_VERSION") or lock.get("BASE_VERSION"),
            "os_id": turrel.get("TUR_BASE_OS_ID") or osrel.get("ID"),
            "os_version_id": turrel.get("TUR_BASE_OS_VERSION_ID") or osrel.get("VERSION_ID"),
            "variant_id": turrel.get("TUR_BASE_OS_VARIANT_ID") or osrel.get("VARIANT_ID"),
        },
        "kernel_release": values.get("kernel_release"),
        "kernel_config_sha256": values.get("kernel_config_sha256"),
        "initramfs_sha256": values.get("initramfs_sha256"),
        "kernel_config": dict(sorted(kconfig.items())),
        "bootloader": {
            "grub2_efi": pkg("grub2-efi-x64"),
            "shim": pkg("shim-x64"),
            "systemd_boot": pkg("systemd-boot-unsigned"),
        },
        "desktop": {
            "name": "KDE Plasma",
            "plasma_workspace": pkg("plasma-workspace"),
            "kwin": pkg("kwin"),
            "qt6": pkg("qt6-qtbase"),
            "display_manager": values.get("display_manager") or None,
            "default_target": values.get("default_target"),
        },
        "security": {"selinux": values.get("selinux")},
        "packages": dict(sorted(packages.items())),
        "rpm_count": int(values["rpm_count"]),
        "programs": tur_programs,
        "catalogue": {
            "path": "/usr/share/tur/catalogue",
            "entries": int(values.get("catalogue_entries", 0)),
            "sha256": values.get("catalogue_sha256"),
        },
        "third_party_crates": {
            "count": int(values.get("crates", 0)),
            "list": "/usr/share/licenses/tur-third-party/crates.tsv",
            "list_sha256": values.get("crates_sha256"),
            "rust": values.get("rust"),
        },
        "runtime_versions": {
            "wine": None,
            "proton": None,
            "darling": None,
            "dxvk": None,
            "vkd3d_proton": None,
        },
        "sbom": args.sbom,
    }

    pathlib.Path(args.output).write_text(json.dumps(manifest, indent=2, sort_keys=False) + "\n")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

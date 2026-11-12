#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Write the manifest that travels beside a TUR installer medium.

    installer/iso/medium-manifest.py FACTS_DIR MANIFEST.json

FACTS_DIR is what installer/iso/build-medium.sh recorded while it built the
medium. The manifest says, with hashes, what the medium is made of: the
payload image, the installer environment and how it differs from the payload,
the boot chain, every file on the medium, the tools that assembled it, and the
command the installer runs. It invents nothing: a fact that was not recorded
is null.
"""

from __future__ import annotations

import json
import pathlib
import sys


def props(path: pathlib.Path) -> dict[str, str]:
    found = {}
    if path.exists():
        for line in path.read_text().splitlines():
            key, sep, value = line.partition("=")
            if sep:
                found[key.strip()] = value.strip()
    return found


def load(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def number(value: str | None) -> int | None:
    try:
        return int(value) if value is not None else None
    except ValueError:
        return None


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    facts, target = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    medium = props(facts / "medium.env")
    release = props(facts / "payload-tur-release.env")
    timings = {key: number(value) for key, value in props(facts / "timings.env").items()}
    changes = load(facts / "environment-package-changes.json") or {}
    oci = load(facts / "payload-oci-manifest.json") or {}
    contents = load(facts / "contents.json") or {}
    inspect = load(facts / "payload-image-inspect.json") or [{}]
    labels = ((inspect[0] if inspect else {}).get("Config") or {}).get("Labels") or {}
    boot_chain = []
    tsv = facts / "boot-chain.tsv"
    if tsv.exists():
        for line in tsv.read_text().splitlines():
            fields = line.split("\t")
            if len(fields) >= 3:
                boot_chain.append(
                    {
                        "file": fields[0],
                        "path_in_image": fields[1],
                        "sha256": fields[2],
                        "package": fields[3] if len(fields) > 3 else None,
                        "licence": fields[4] if len(fields) > 4 else None,
                    }
                )
    modules = facts / "initramfs-dracut-modules.txt"
    manifest = {
        "schema": "tur-installer-medium-manifest",
        "schema_version": 1,
        "medium": {
            "file": medium.get("medium_file"),
            "bytes": number(medium.get("medium_bytes")),
            "sha256": medium.get("medium_sha256"),
            "volume_label": medium.get("label"),
            "format": "ISO 9660 (Rock Ridge) with a GPT and an appended EFI system partition; UEFI only, no BIOS boot code",
            "kernel_arguments": medium.get("kernel_arguments"),
            "bit_reproducible": False,
            "bit_reproducible_note": (
                "Every input is fixed by the hashes below. The file itself is not claimed to be "
                "bit-for-bit reproducible: the installer environment is built with packages from "
                "Fedora's repositories as they are on the day of the build, and the filesystem "
                "images carry build-time metadata."
            ),
        },
        "source": {
            "revision": medium.get("source_revision"),
            "commit_time": number(medium.get("source_commit_time")),
            "tur_version": medium.get("tur_version"),
            "build_id": medium.get("build_id"),
        },
        "payload": {
            "what": "the TUR image that is installed, as built by scripts/build-image.sh from this revision",
            "image": medium.get("payload_image"),
            "image_id": medium.get("payload_image_id"),
            "recorded_origin": medium.get("payload_target_imgref"),
            "base_image": release.get("TUR_BASE_IMAGE") or labels.get("org.tur.base.image"),
            "base_digest": release.get("TUR_BASE_DIGEST") or labels.get("org.tur.base.digest"),
            "base_version": release.get("TUR_BASE_VERSION"),
            "oci_layout": {
                "path_on_medium": "payload",
                "reference": "payload",
                "index_sha256": medium.get("payload_index_sha256"),
                "config_digest": (oci.get("config") or {}).get("digest"),
                "layers": len(oci.get("layers") or []),
                "layer_media_types": sorted({layer.get("mediaType") for layer in oci.get("layers") or []}),
                "blobs": number(medium.get("payload_blob_count")),
                "bytes": number(medium.get("payload_blob_bytes")),
            },
            "root_filesystem": medium.get("root_fs"),
            "root_filesystem_source": medium.get("root_fs_source"),
            "install_command": medium.get("install_command"),
            "smallest_disk_accepted_bytes": number(medium.get("min_target_bytes")),
        },
        "installer_environment": {
            "what": "the payload image plus Fedora's dracut-live, an initramfs for starting from the medium, and the TUR installer",
            "image_id": medium.get("environment_image_id"),
            "kernel": medium.get("kernel"),
            "squashfs_bytes": number(medium.get("squashfs_bytes")),
            "squashfs_compression": f"zstd level {medium.get('squashfs_zstd_level')}",
            "initramfs_bytes": number(medium.get("initramfs_bytes")),
            "initramfs_dracut_modules": modules.read_text().split() if modules.exists() else None,
            "packages_added": changes.get("added"),
            "packages_changed": changes.get("changed"),
            "packages_removed": changes.get("removed"),
            "files_bootc_install_reads_identical_to_payload": changes.get("install_relevant_files_identical"),
        },
        "boot_chain": {
            "what": "taken unchanged from the payload image; the same signed binaries bootupd installs on the target disk",
            "efi_vendor_directory": medium.get("efi_vendor"),
            "files": boot_chain,
        },
        "files_on_medium": contents.get("files"),
        "payload_blobs_sha256": contents.get("payload_blobs_sha256"),
        "efi_system_partition_image_sha256": (facts / "efiboot.sha256").read_text().strip()
        if (facts / "efiboot.sha256").exists()
        else None,
        "tools": {key[len("tool_"):]: value for key, value in medium.items() if key.startswith("tool_")},
        "build_seconds": {**timings, "total": number(medium.get("total_seconds"))},
        "licences": [
            {"what": "TUR installer program and units", "licence": "LicenseRef-TUR-Noncommercial-1.0.0", "on_medium": "tur-installer/licenses/TUR-LICENSE.md"},
            {"what": "boot menu (grub.cfg)", "licence": "BSD-3-Clause", "on_medium": "tur-installer/licenses/grub-menu.LICENSE"},
            {
                "what": "everything else: Fedora packages in the payload and in the installer environment",
                "licence": "per package; see packages_added, boot_chain and the image workflow's SBOM",
                "on_medium": "/usr/share/licenses inside LiveOS/squashfs.img",
            },
        ],
        "distribution": (
            "Not for redistribution. The payload still contains Fedora's logos and release packages "
            "(issue 11), and the boot chain is Fedora's signed shim and GRUB."
        ),
    }
    target.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"manifest: {target} ({target.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

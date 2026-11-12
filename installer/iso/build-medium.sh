#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build the TUR installer medium (ADR-0018): an image for a USB stick, also
# readable as an optical disc, that starts under UEFI and installs the TUR
# image it carries onto one whole disk.
#
#   installer/iso/build-medium.sh PAYLOAD_IMAGE WORK_DIR OUT_DIR
#
# PAYLOAD_IMAGE is the image to install, in the container storage PODMAN uses
# (the image scripts/build-image.sh built). WORK_DIR receives the medium,
# WORK_DIR/medium/<name>.iso. OUT_DIR receives what describes it and is small
# enough to keep: <name>.manifest.json, SHA256SUMS and the facts they were
# made from.
#
# On the medium:
#
#   EFI system partition   Fedora's signed shim and GRUB, taken unchanged from
#                          the payload image, and the boot menu
#   /boot                  the payload's kernel, unchanged, and an initramfs
#                          built for starting from the medium
#   /LiveOS/squashfs.img   the installer environment: the payload image plus
#                          dracut-live and the installer (Containerfile)
#   /payload               the payload image as an OCI layout: what
#                          `bootc install` deploys
#   /tur-installer         what this medium is, its contents by hash, licences
#
# PODMAN and SKOPEO name the tools and must use the same storage, as root, for
# example PODMAN="sudo podman" SKOPEO="sudo skopeo".
#
# Environment:
#   TUR_MEDIUM_NAME            file name without extension (default tur-installer)
#   TUR_MEDIUM_TARGET_IMGREF   the image name the installed system records as
#                              its origin (default localhost/tur:<build id>)
#   TUR_MEDIUM_LABEL           volume label (default TUR-INSTALL)
#   TUR_MEDIUM_MIN_TARGET      smallest disk the installer accepts, in bytes
#                              (default 21474836480: the size the boot test
#                              installs to)
#   TUR_MEDIUM_ROOT_FS         root filesystem when the image configures none
#                              (default ext4, as tests/boot/build-disk.sh)
#   TUR_MEDIUM_SQUASHFS_LEVEL  zstd level of the installer environment (default 15)
#   TUR_MEDIUM_ENV_IMAGE       tag for the installer environment image
#
# Run this in CI only, never on a shared server.
set -euo pipefail

cd "$(dirname "$0")/../.."
payload="${1:?payload image reference required}"
work="${2:?work directory required}"
out="${3:?output directory required}"
read -r -a podman <<<"${PODMAN:-podman}"
read -r -a skopeo <<<"${SKOPEO:-skopeo}"

name="${TUR_MEDIUM_NAME:-tur-installer}"
label="${TUR_MEDIUM_LABEL:-TUR-INSTALL}"
min_target="${TUR_MEDIUM_MIN_TARGET:-21474836480}"
fallback_fs="${TUR_MEDIUM_ROOT_FS:-ext4}"
level="${TUR_MEDIUM_SQUASHFS_LEVEL:-15}"
env_image="${TUR_MEDIUM_ENV_IMAGE:-localhost/tur-installer-env:${name}}"
# Kernel arguments of the installer environment:
#   root=live:..., rd.live.image  dracut's dmsquash-live finds the medium by label
#   selinux=0             the installer environment's files carry no labels.
#                         bootc labels the installed system from the image's
#                         policy, the path the boot test takes on every run
#                         (its build host has no SELinux either)
#   systemd.firstboot=off no first-boot questions on the console
#   plymouth.enable=0     no boot splash: it would hold the console's keyboard
#   loglevel=3 audit=0    no kernel or audit messages written over the screens
#   console=...           the kernel also writes to the first serial port when
#                         the machine has one; the screen is the last console
# and, for each entry of the boot menu:
#   tur.live=1            the live session (graphical.target, the default
#                         target of the installer environment): the desktop,
#                         signed in as the live user, and the graphical
#                         installer
#   tur.installer=1 systemd.unit=tur-installer.target
#                         the text installer on the first console, and no
#                         desktop
kernel_arguments="root=live:CDLABEL=${label} rd.live.image selinux=0 systemd.firstboot=off plymouth.enable=0 rd.plymouth=0 loglevel=3 audit=0 console=ttyS0,115200n8 console=tty0"
live_arguments="${kernel_arguments} tur.live=1"
text_arguments="${kernel_arguments} tur.installer=1 systemd.unit=tur-installer.target"

for tool in xorriso mksquashfs mkfs.vfat mmd mcopy python3 sha256sum sfdisk; do
    command -v "${tool}" > /dev/null || { echo "missing tool: ${tool}" >&2; exit 1; }
done

mkdir -p "${work}" "${out}"
work="$(cd "${work}" && pwd)"
out="$(cd "${out}" && pwd)"
facts="${out}/facts"
iso="${work}/medium-tree"
esp="${work}/medium-esp"
medium_dir="${work}/medium"
medium="${medium_dir}/${name}.iso"
sudo rm -rf "${iso}" "${esp}" "${facts}"
mkdir -p "${iso}/boot" "${iso}/LiveOS" "${iso}/tur-installer/licenses" "${esp}" "${facts}" "${medium_dir}"
rm -f "${medium}"

started="$(date +%s)"
step_started="${started}"
step() {
    local now
    now="$(date +%s)"
    if [ -n "${step_name:-}" ]; then
        echo "${step_name}=$((now - step_started))" >> "${facts}/timings.env"
    fi
    step_name="$1"
    step_started="${now}"
    echo "== $2"
}
in_payload() { "${podman[@]}" run --rm --network none "${payload}" "$@"; }
in_env() { "${podman[@]}" run --rm --network none "${env_image}" "$@"; }
owner() { in_env rpm -qf --qf '%{NAME}-%{EVR}.%{ARCH}\t%{LICENSE}' "$1" 2> /dev/null || printf 'unknown\tunknown'; }

# ---- the payload ---------------------------------------------------------------
step payload_facts_seconds "payload: ${payload}"
payload_id="$("${podman[@]}" image inspect -f '{{.Id}}' "${payload}")"
in_payload cat /usr/lib/tur/release > "${facts}/payload-tur-release.env"
cat "${facts}/payload-tur-release.env"
build_id="$(sed -n 's/^TUR_BUILD_ID=//p' "${facts}/payload-tur-release.env")"
tur_version="$(sed -n 's/^TUR_VERSION=//p' "${facts}/payload-tur-release.env")"
revision="$(sed -n 's/^TUR_SOURCE_REVISION=//p' "${facts}/payload-tur-release.env")"
test -n "${build_id}"
target_imgref="${TUR_MEDIUM_TARGET_IMGREF:-localhost/tur:${build_id}}"
epoch="$(git show -s --format=%ct HEAD)"
"${podman[@]}" image inspect "${payload}" > "${facts}/payload-image-inspect.json"
in_payload bootc --version | tee "${facts}/bootc-version.txt"

# The root filesystem, decided as tests/boot/build-disk.sh decides it: the
# image's own installation settings, else ext4.
in_payload bootc install print-configuration --all > "${facts}/install-configuration.json" \
    || in_payload bootc install print-configuration > "${facts}/install-configuration.json" \
    || echo "bootc install print-configuration failed"
configured_fs="$(python3 - "${facts}/install-configuration.json" <<'PY'
import json
import sys

try:
    config = json.load(open(sys.argv[1]))
except (OSError, ValueError):
    config = {}
root = ((config or {}).get("filesystem") or {}).get("root") or {}
print(root.get("type") or (config or {}).get("root-fs-type") or "")
PY
)"
if [ -n "${configured_fs}" ]; then
    root_fs="${configured_fs}"
    root_fs_source="image install configuration"
    install_args=""
else
    root_fs="${fallback_fs}"
    root_fs_source="medium default (the image configures none)"
    install_args="--filesystem ${root_fs}"
fi
echo "root filesystem: ${root_fs} (${root_fs_source})"
# ADR-0014: GRUB's environment block must be on ext4 on a plain partition.
# No other layout has been tested, so no other is offered.
if [ "${root_fs}" != ext4 ]; then
    echo "the installed root filesystem would be ${root_fs}; only ext4 is tested (ADR-0014)" >&2
    exit 1
fi

rpm_query='%{NAME}\t%{EVR}\t%{ARCH}\t%{LICENSE}\n'
in_payload rpm -qa --qf "${rpm_query}" | sort > "${facts}/payload-packages.tsv"
echo "payload packages: $(wc -l < "${facts}/payload-packages.tsv")"

# What `bootc install` reads from the root it runs in. The installer
# environment must have exactly these, as the payload has them.
# shellcheck disable=SC2016  # expanded by the shell in the container
relevant='for p in /usr/lib/bootc /usr/lib/bootupd /usr/lib/efi /usr/lib/ostree /etc/ostree /usr/libexec/bootupd /usr/bin/bootc /usr/bin/bootupctl /usr/bin/ostree /usr/bin/skopeo /usr/sbin/mkfs.ext4 /usr/sbin/mke2fs /usr/sbin/mkfs.fat /usr/sbin/sfdisk /usr/sbin/wipefs /etc/selinux /etc/containers/policy.json; do if [ -e "$p" ]; then find -L "$p" -xdev -type f -print0 | sort -z | xargs -0 -r sha256sum; else echo "absent  $p"; fi; done'
in_payload /bin/sh -c "${relevant}" > "${facts}/payload-install-relevant.sha256"
echo "install-relevant files in the payload: $(wc -l < "${facts}/payload-install-relevant.sha256")"

# ---- the payload as an OCI layout on the medium ------------------------------------
step payload_copy_seconds "payload image -> OCI layout on the medium"
sudo rm -rf "${iso}/payload"
"${skopeo[@]}" --version
"${skopeo[@]}" copy --quiet --dest-compress-format zstd \
    "containers-storage:${payload}" "oci:${iso}/payload:payload"
sudo chown -R "$(id -u):$(id -g)" "${iso}/payload"
"${skopeo[@]}" inspect --raw "oci:${iso}/payload:payload" > "${facts}/payload-oci-manifest.json"
index_sha256="$(sha256sum "${iso}/payload/index.json" | cut -d' ' -f1)"
blob_count="$(find "${iso}/payload/blobs/sha256" -type f | wc -l)"
blob_bytes="$(du -sb "${iso}/payload/blobs" | cut -f1)"
echo "OCI layout: ${blob_count} blobs, ${blob_bytes} bytes, index.json sha256 ${index_sha256}"
python3 - "${facts}/payload-oci-manifest.json" "${payload_id}" "${iso}/payload" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest = json.load(open(sys.argv[1]))
want, layout = sys.argv[2], pathlib.Path(sys.argv[3])
config = manifest["config"]["digest"]
print(f"OCI manifest: config {config}; {len(manifest['layers'])} layers; "
      f"media types {sorted({layer['mediaType'].rsplit('.', 1)[-1] for layer in manifest['layers']})}")
if config != f"sha256:{want}":
    sys.exit(f"the image on the medium is not the payload: config {config}, image ID sha256:{want}")
# Every blob is what its name says. The installer repeats this on the medium.
for blob in sorted((layout / "blobs" / "sha256").iterdir()):
    digest = hashlib.sha256()
    with open(blob, "rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    if digest.hexdigest() != blob.name:
        sys.exit(f"blob {blob.name} does not have that digest")
print("every blob matches its name")
PY

# ---- the installer environment ---------------------------------------------------
step environment_build_seconds "installer environment: ${env_image} FROM ${payload}"
"${podman[@]}" build \
    --file installer/iso/Containerfile \
    --build-arg "PAYLOAD_IMAGE=${payload}" \
    --build-arg "PAYLOAD_TARGET_IMGREF=${target_imgref}" \
    --build-arg "PAYLOAD_IMAGE_ID=${payload_id}" \
    --build-arg "PAYLOAD_INDEX_SHA256=${index_sha256}" \
    --build-arg "PAYLOAD_BLOB_COUNT=${blob_count}" \
    --build-arg "PAYLOAD_BLOB_BYTES=${blob_bytes}" \
    --build-arg "PAYLOAD_INSTALL_ARGS=${install_args}" \
    --build-arg "MIN_TARGET_BYTES=${min_target}" \
    --build-arg "MEDIUM_LABEL=${label}" \
    --pull=never \
    --ignorefile installer/iso/Containerfile.containerignore \
    --tag "${env_image}" \
    .
env_id="$("${podman[@]}" image inspect -f '{{.Id}}' "${env_image}")"
in_env rpm -qa --qf "${rpm_query}" | sort > "${facts}/environment-packages.tsv"
in_env /bin/sh -c "${relevant}" > "${facts}/environment-install-relevant.sha256"
in_env cat /usr/lib/tur-installer/boot/initrd-modules.txt > "${facts}/initramfs-dracut-modules.txt"
in_env cat /usr/lib/tur-installer/payload.env > "${facts}/payload.env"
in_env rpm -q dracut dracut-live | tee "${facts}/dracut-version.txt"

# The installer environment is the payload plus dracut-live. Stop if anything
# `bootc install` reads differs, or if a package the payload has changed.
python3 - "${facts}" <<'PY'
import json
import pathlib
import re
import sys

facts = pathlib.Path(sys.argv[1])


def packages(name):
    found = {}
    for line in (facts / name).read_text().splitlines():
        fields = line.split("\t")
        if len(fields) >= 3:
            found[(fields[0], fields[2])] = {"version": fields[1], "licence": fields[3] if len(fields) > 3 else ""}
    return found


payload, env = packages("payload-packages.tsv"), packages("environment-packages.tsv")
added = [
    {"name": name, "arch": arch, "version": env[(name, arch)]["version"], "licence": env[(name, arch)]["licence"]}
    for (name, arch) in sorted(set(env) - set(payload))
]
removed = [{"name": name, "arch": arch} for (name, arch) in sorted(set(payload) - set(env))]
changed = [
    {"name": name, "arch": arch, "payload": payload[(name, arch)]["version"], "environment": env[(name, arch)]["version"]}
    for (name, arch) in sorted(set(env) & set(payload))
    if env[(name, arch)]["version"] != payload[(name, arch)]["version"]
]
same_files = (facts / "payload-install-relevant.sha256").read_text() == (
    facts / "environment-install-relevant.sha256"
).read_text()
(facts / "environment-package-changes.json").write_text(
    json.dumps({"added": added, "removed": removed, "changed": changed, "install_relevant_files_identical": same_files}, indent=2) + "\n"
)
print(f"packages added to the installer environment: {[p['name'] + '-' + p['version'] for p in added]}")
print(f"packages changed: {changed}")
print(f"packages removed: {removed}")
print(f"files bootc install reads are identical to the payload's: {same_files}")
problems = []
if not same_files:
    problems.append("files that `bootc install` reads differ between the payload and the installer environment")
if not any(p["name"] == "dracut-live" for p in added):
    problems.append("dracut-live was not added")
for item in changed + removed:
    if not re.fullmatch(r"dracut(-.*)?", item["name"]):
        problems.append(f"package {item['name']} differs from the payload")
if problems:
    sys.exit("the installer environment is not the payload plus dracut-live: " + "; ".join(problems))
PY

# ---- kernel, initramfs, boot loader ------------------------------------------------
step boot_files_seconds "boot files"
mnt="$("${podman[@]}" image mount "${env_image}")"
unmount() { "${podman[@]}" image unmount "${env_image}" > /dev/null 2>&1 || true; }
trap unmount EXIT
kver="$(sudo cat "${mnt}/usr/lib/tur-installer/boot/kernel-version")"
echo "kernel: ${kver}"
sudo cp "${mnt}/usr/lib/modules/${kver}/vmlinuz" "${iso}/boot/vmlinuz"
sudo cp "${mnt}/usr/lib/tur-installer/boot/initrd.img" "${iso}/boot/initrd.img"
sudo chown "$(id -u):$(id -g)" "${iso}/boot/vmlinuz" "${iso}/boot/initrd.img"
chmod 0644 "${iso}/boot/vmlinuz" "${iso}/boot/initrd.img"

# Fedora's signed shim, MOK manager and GRUB, as the payload ships them for
# bootupd. Every copy in the image must be the same file.
: > "${facts}/boot-chain.tsv"
vendor=""
for binary in shimx64.efi mmx64.efi grubx64.efi; do
    mapfile -t found < <(
        sudo find "${mnt}/usr/lib/efi" -type f -iname "${binary}" 2> /dev/null | sort
        sudo find "${mnt}/usr/lib/bootupd/updates" -type f -iname "${binary}" 2> /dev/null | sort
    )
    if [ "${#found[@]}" -eq 0 ]; then
        echo "no ${binary} in the image under /usr/lib/efi or /usr/lib/bootupd/updates" >&2
        exit 1
    fi
    sums="$(sudo sha256sum "${found[@]}" | cut -d' ' -f1 | sort -u)"
    if [ "$(printf '%s\n' "${sums}" | wc -l)" -ne 1 ]; then
        echo "the image holds different files named ${binary}:" >&2
        sudo sha256sum "${found[@]}" >&2
        exit 1
    fi
    sudo cp "${found[0]}" "${esp}/${binary}"
    sudo chown "$(id -u):$(id -g)" "${esp}/${binary}"
    source_path="${found[0]#"${mnt}"}"
    package="$(owner "${source_path}")"
    printf '%s\t%s\t%s\t%s\n' "${binary}" "${source_path}" "${sums}" "${package}" | tee -a "${facts}/boot-chain.tsv"
    if [ "${binary}" = grubx64.efi ]; then
        vendor="$(basename "$(dirname "${found[0]}")")"
    fi
done
echo "EFI vendor directory: ${vendor}"
test -n "${vendor}"
kernel_package="$(owner "/usr/lib/modules/${kver}/vmlinuz")"
printf '%s\t%s\t%s\t%s\n' vmlinuz "/usr/lib/modules/${kver}/vmlinuz" "$(sha256sum "${iso}/boot/vmlinuz" | cut -d' ' -f1)" "${kernel_package}" | tee -a "${facts}/boot-chain.tsv"

title="TUR ${tur_version} build ${build_id}"
sed -e "s|@LABEL@|${label}|g" -e "s|@TITLE@|${title}|g" \
    -e "s|@LIVE_ARGUMENTS@|${live_arguments}|g" -e "s|@TEXT_ARGUMENTS@|${text_arguments}|g" \
    installer/iso/third-party-licensed/grub/grub.cfg.in > "${esp}/grub.cfg"
if grep -q '@[A-Z_]*@' "${esp}/grub.cfg"; then
    echo "the boot menu still has a placeholder:" >&2
    grep -n '@[A-Z_]*@' "${esp}/grub.cfg" >&2
    exit 1
fi
cat "${esp}/grub.cfg"

# The EFI system partition: shim as the default loader, GRUB and the MOK
# manager beside it, and the menu where GRUB looks for it. No fallback
# program (fbx64.efi): starting from the medium must not add a boot entry to
# the computer's firmware.
export MTOOLS_SKIP_CHECK=1
rm -f "${work}/efiboot.img"
truncate -s 32M "${work}/efiboot.img"
mkfs.vfat -n TUR-EFI "${work}/efiboot.img" > /dev/null
mmd -i "${work}/efiboot.img" ::/EFI ::/EFI/BOOT "::/EFI/${vendor}"
mcopy -i "${work}/efiboot.img" "${esp}/shimx64.efi" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "${work}/efiboot.img" "${esp}/grubx64.efi" ::/EFI/BOOT/grubx64.efi
mcopy -i "${work}/efiboot.img" "${esp}/mmx64.efi" ::/EFI/BOOT/mmx64.efi
mcopy -i "${work}/efiboot.img" "${esp}/grub.cfg" ::/EFI/BOOT/grub.cfg
mcopy -i "${work}/efiboot.img" "${esp}/grub.cfg" "::/EFI/${vendor}/grub.cfg"
mdir -i "${work}/efiboot.img" -/ ::/ | tee "${facts}/esp-listing.txt"
# The menu a second time, in the ISO 9660 filesystem. Started from an optical
# drive, GRUB takes the disc itself as its device, not the EFI system
# partition image the firmware loaded it from, and looks for its menu there.
mkdir -p "${iso}/EFI/BOOT" "${iso}/EFI/${vendor}"
cp "${esp}/grub.cfg" "${iso}/EFI/BOOT/grub.cfg"
cp "${esp}/grub.cfg" "${iso}/EFI/${vendor}/grub.cfg"

# ---- the installer environment as a compressed, read-only filesystem ----------------
step squashfs_seconds "installer environment -> LiveOS/squashfs.img (zstd level ${level})"
sudo mksquashfs "${mnt}" "${iso}/LiveOS/squashfs.img" \
    -comp zstd -Xcompression-level "${level}" -b 1M -noappend -no-progress -mkfs-time "${epoch}" \
    | tail -n 25
sudo chown "$(id -u):$(id -g)" "${iso}/LiveOS/squashfs.img"
unmount
trap - EXIT

# ---- what the medium says about itself ----------------------------------------------
step describe_seconds "description and licences on the medium"
cp installer/iso/third-party-licensed/grub/LICENSE "${iso}/tur-installer/licenses/grub-menu.LICENSE"
cp LICENSE.md "${iso}/tur-installer/licenses/TUR-LICENSE.md"
cp "${facts}/payload.env" "${iso}/tur-installer/payload.env"
cat > "${iso}/tur-installer/README.txt" <<EOF
TUR installer medium
====================

TUR ${tur_version}, build ${build_id}
Source revision ${revision}

Starting a computer from this medium (UEFI only) runs an installer that
erases ONE WHOLE DISK, after a typed confirmation, and installs TUR on it.
Instructions: docs/guides/install-on-a-test-pc.md in the TUR source tree.

This is a pre-alpha test build for the project's own test machines. It is
NOT FOR REDISTRIBUTION: the system on it still contains Fedora's logos and
release packages, which Fedora's terms do not allow in a derived system.

What is on this medium
  EFI system partition  Fedora's signed shim and GRUB, unchanged, and a menu
  boot/                 Fedora's kernel, unchanged, and an initramfs built
                        with Fedora's dracut
  LiveOS/squashfs.img   the installer environment: the TUR image plus
                        Fedora's dracut-live and the TUR installer
  payload/              the TUR image that is installed (OCI layout)
  tur-installer/        this file, contents.json (every file by SHA-256),
                        licences

Licences
  TUR's own programs: tur-installer/licenses/TUR-LICENSE.md
  The boot menu: tur-installer/licenses/grub-menu.LICENSE (BSD-3-Clause)
  Everything else comes from Fedora packages. Each package's licence is in
  the installer environment and in the installed system under
  /usr/share/licenses, and contents.json names the packages of the boot chain.
EOF

python3 - "${iso}" "${facts}" > "${iso}/tur-installer/contents.json" <<'PY'
import hashlib
import json
import pathlib
import sys

root, facts = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
files = []
for path in sorted(root.rglob("*")):
    relative = path.relative_to(root).as_posix()
    if not path.is_file() or relative.startswith("payload/blobs/") or relative == "tur-installer/contents.json":
        continue
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    files.append({"path": relative, "bytes": path.stat().st_size, "sha256": digest.hexdigest()})
blobs = sorted(p.name for p in (root / "payload" / "blobs" / "sha256").iterdir())
print(json.dumps({"files": files, "payload_blobs_sha256": blobs}, indent=2))
PY
cp "${iso}/tur-installer/contents.json" "${facts}/contents.json"
sha256sum "${work}/efiboot.img" | cut -d' ' -f1 > "${facts}/efiboot.sha256"

# ---- the medium ----------------------------------------------------------------------
# ISO 9660 with Rock Ridge names, a GPT, and the EFI system partition appended
# as partition 2 and named in the El Torito catalogue. UEFI firmware starts it
# from a USB stick (through the GPT) and from an optical drive (through the
# catalogue). There is no BIOS boot code: TUR targets UEFI (specification 4.1).
step xorriso_seconds "medium: ${medium}"
xorriso -version 2>&1 | sed -n 1,2p
SOURCE_DATE_EPOCH="${epoch}" xorriso -as mkisofs \
    -iso-level 3 -r -V "${label}" \
    -partition_offset 16 \
    -append_partition 2 C12A7328-F81F-11D2-BA4B-00A0C93EC93B "${work}/efiboot.img" \
    -appended_part_as_gpt \
    -iso_mbr_part_type EBD0A0A2-B9E5-4433-87C0-68B6B72699C7 \
    -c boot.catalog \
    -e --interval:appended_partition_2:all:: -no-emul-boot \
    -o "${medium}" "${iso}" 2>&1 | tail -n 25
xorriso -indev "${medium}" -report_el_torito plain -report_system_area plain 2>&1 | tee "${facts}/xorriso-report.txt" | tail -n 40
sfdisk --json "${medium}" > "${facts}/medium-partitions.json" || echo "sfdisk could not describe the medium"
sfdisk -l "${medium}" | tee "${facts}/medium-partitions.txt" || true

step finish_seconds "manifest"
medium_sha256="$(sha256sum "${medium}" | cut -d' ' -f1)"
medium_bytes="$(stat -c %s "${medium}")"
{
    echo "name=${name}"
    echo "medium_file=${name}.iso"
    echo "medium_bytes=${medium_bytes}"
    echo "medium_sha256=${medium_sha256}"
    echo "label=${label}"
    echo "kernel_arguments=${live_arguments}"
    echo "kernel_arguments_text_mode=${text_arguments}"
    echo "payload_image=${payload}"
    echo "payload_image_id=${payload_id}"
    echo "payload_target_imgref=${target_imgref}"
    echo "payload_index_sha256=${index_sha256}"
    echo "payload_blob_count=${blob_count}"
    echo "payload_blob_bytes=${blob_bytes}"
    echo "environment_image=${env_image}"
    echo "environment_image_id=${env_id}"
    echo "kernel=${kver}"
    echo "efi_vendor=${vendor}"
    echo "root_fs=${root_fs}"
    echo "root_fs_source=${root_fs_source}"
    echo "install_command=bootc install to-disk --source-imgref oci:/run/initramfs/live/payload:payload --target-imgref ${target_imgref} --generic-image --wipe ${install_args} <the chosen disk>"
    echo "min_target_bytes=${min_target}"
    echo "squashfs_bytes=$(stat -c %s "${iso}/LiveOS/squashfs.img")"
    echo "squashfs_zstd_level=${level}"
    echo "initramfs_bytes=$(stat -c %s "${iso}/boot/initrd.img")"
    echo "tur_version=${tur_version}"
    echo "build_id=${build_id}"
    echo "source_revision=${revision}"
    echo "source_commit_time=${epoch}"
    echo "tool_xorriso=$(xorriso -version 2>&1 | sed -n 's/^xorriso version *: *//p' | head -n 1)"
    echo "tool_mksquashfs=$(mksquashfs -version 2>&1 | head -n 1)"
    echo "tool_mkfs_vfat=$(mkfs.vfat --help 2>&1 | head -n 1)"
    echo "tool_mtools=$(mcopy -V 2>&1 | head -n 1)"
    echo "tool_skopeo=$("${skopeo[@]}" --version)"
    echo "tool_podman=$("${podman[@]}" --version)"
    echo "total_seconds=$(($(date +%s) - started))"
} > "${facts}/medium.env"
step "" "done"
cat "${facts}/medium.env" "${facts}/timings.env"

python3 installer/iso/medium-manifest.py "${facts}" "${out}/${name}.manifest.json"
echo "${medium_sha256}  ${name}.iso" > "${out}/${name}.SHA256SUMS"
(cd "${out}" && sha256sum "${name}.manifest.json") >> "${out}/${name}.SHA256SUMS"
cat "${out}/${name}.SHA256SUMS"
sudo rm -rf "${iso}" "${esp}" "${work}/efiboot.img"
ls -la "${medium_dir}" "${out}"
df -h "${work}" | tail -n 1

# shellcheck shell=bash disable=SC2034  # its variables are read by the installers that source it
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The installation engine of the TUR installer medium (ADR-0018 and its
# amendment of 13 November 2026). One copy of everything that decides about
# disks and writes to them, used by both installers on the medium:
#
#   - the text installer, /usr/libexec/tur-installer/tur-install, which
#     sources this file and runs as root on the first console;
#   - the graphical installer, which runs as the live session's user and
#     reaches this file only through /usr/libexec/tur-installer/tur-install-engine,
#     started with pkexec for each step (polkit action org.tur.installer.engine).
#
# What it does, and only this:
#
#   - checks the image on the medium against its SHA-256 sums;
#   - checks the computer (UEFI, x86-64, memory, Secure Boot state);
#   - lists whole disks with their stable identity and, in plain words, what
#     is on them. Read-only: lsblk, a few bytes with head, and an EFI system
#     partition mounted read-only in a private mount namespace;
#   - derives the confirmation phrase of a disk ("ERASE" and the end of its
#     serial number: installer/tur-installer-core, plan.rs);
#   - immediately before writing, looks at the chosen disk again: same stable
#     path, serial number, size and content, still not in use, not the medium;
#   - runs the one installation command the boot test proves:
#
#       bootc install to-disk --source-imgref oci:<medium>/payload:payload \
#           --target-imgref <name> --generic-image --wipe [--filesystem ext4] <disk>
#
#   - checks the result has an EFI system partition and a root partition, and
#     keeps the installer's log on the installed disk, under
#     /var/log/tur-installer/<session> (specification 28.3).
#
# The caller defines two functions before sourcing, or gets the defaults
# below: `transcript LINE` (the log and the serial port) and `mark TEXT` (a
# state line, "TUR-INSTALLER: TEXT"). tests/iso reads the state lines from the
# serial port; they are the same whichever installer runs.
#
# Error codes: where a condition already has a code in
# installer/tur-installer-core/src/codes.rs, that code is used. Conditions
# only this medium has use TUR-INSTALL-MEDIUM-NNN (ADR-0018 lists them).

readonly INSTALLER_DIR=/usr/lib/tur-installer
readonly PAYLOAD_ENV="${INSTALLER_DIR}/payload.env"
readonly RELEASE=/usr/lib/tur/release
readonly LIVE_MOUNT=/run/initramfs/live
readonly PAYLOAD_DIR="${LIVE_MOUNT}/payload"
readonly SESSION=/run/tur-installer
readonly LOG="${SESSION}/installer.log"
readonly INSTALL_LOG="${SESSION}/install.log"
readonly WIZARD_TRANSCRIPT="${SESSION}/wizard/transcript"
# Below this much memory the installer refuses to continue: the rule of
# installer/tur-installer-core (preflight.rs, MIN_RAM_BYTES).
readonly MIN_RAM_BYTES=$((3 * 1024 * 1024 * 1024))
readonly ESP_GUID=c12a7328-f81f-11d2-ba4b-00a0c93ec93b
readonly ROOT_GUID=4f68bce3-e8cd-4db1-96e7-fbcaf984b709
readonly ESP_PROBE="${SESSION}/esp-probe"

# The session directory is readable by the live session's user, so that the
# graphical installer can show the log and save it to a USB stick. It holds
# serial numbers of disks and nothing secret.
mkdir -p "${SESSION}"
chmod 0755 "${SESSION}" 2> /dev/null
: >> "${LOG}"
chmod 0644 "${LOG}" 2> /dev/null

# ---- output ---------------------------------------------------------------------

# The first serial port, when there is one that can be written to. (Named
# serial_port, not serial: functions use a local "serial" for a disk's serial
# number, which would hide this flag from transcript().)
if [ -z "${serial_port:-}" ]; then
    serial_port=no
    if [ -c /dev/ttyS0 ] && { exec 3> /dev/ttyS0; } 2> /dev/null; then
        serial_port=yes
    fi
fi

if ! declare -F transcript > /dev/null; then
    # transcript LINE: the log and the serial port, not the screen.
    transcript() {
        printf '%s\n' "$1" >> "${LOG}"
        if [ "${serial_port}" = yes ]; then printf '%s\r\n' "$1" >&3 2> /dev/null; fi
    }
fi

if ! declare -F mark > /dev/null; then
    # mark TEXT: a state line for the transcript only.
    mark() { transcript "TUR-INSTALLER: $1"; }
fi

human() {
    awk -v b="$1" 'BEGIN {
        split("B KiB MiB GiB TiB", unit, " ");
        i = 1; while (b >= 1024 && i < 5) { b /= 1024; i++ }
        if (i == 1) printf "%d %s", b, unit[i]; else printf "%.1f %s", b, unit[i]
    }'
}

# ---- what is on the medium ---------------------------------------------------------

# value FILE KEY: the value of KEY=VALUE in a file, nothing if absent.
value() { sed -n "s/^$2=//p" "$1" 2> /dev/null | head -n 1; }

tur_version="$(value "${RELEASE}" TUR_VERSION)"
tur_build_id="$(value "${RELEASE}" TUR_BUILD_ID)"
tur_revision="$(value "${RELEASE}" TUR_SOURCE_REVISION)"
payload_ref="$(value "${PAYLOAD_ENV}" PAYLOAD_OCI_REF)"
payload_target="$(value "${PAYLOAD_ENV}" PAYLOAD_TARGET_IMGREF)"
payload_image_id="$(value "${PAYLOAD_ENV}" PAYLOAD_IMAGE_ID)"
payload_index_sha256="$(value "${PAYLOAD_ENV}" PAYLOAD_INDEX_SHA256)"
payload_blob_count="$(value "${PAYLOAD_ENV}" PAYLOAD_BLOB_COUNT)"
payload_blob_bytes="$(value "${PAYLOAD_ENV}" PAYLOAD_BLOB_BYTES)"
payload_install_args="$(value "${PAYLOAD_ENV}" PAYLOAD_INSTALL_ARGS)"
min_target_bytes="$(value "${PAYLOAD_ENV}" MIN_TARGET_BYTES)"
# shellcheck disable=SC2034  # read by the installers that source this file
readonly tur_version tur_build_id tur_revision payload_ref payload_target payload_image_id \
    payload_index_sha256 payload_blob_count payload_blob_bytes payload_install_args min_target_bytes

payload_state=unknown
payload_detail=""

# verify_payload [PROGRESS_FUNCTION]: check that the image on the medium is
# complete and is the one this installer environment was built with. Every
# file of an OCI layout is named by its SHA-256, and the index is fixed by a
# hash inside this environment. PROGRESS_FUNCTION, if given, is called with
# the number of files checked and the number expected, before each file.
verify_payload() {
    local progress="${1:-}" index="${PAYLOAD_DIR}/index.json" sum count=0 bad=0 file name started="${SECONDS}"
    if [ -z "${payload_index_sha256}" ] || [ -z "${payload_blob_count}" ]; then
        payload_state=missing
        payload_detail="this installer environment does not describe an installation image"
        return 1
    fi
    if [ ! -f "${index}" ]; then
        payload_state=missing
        payload_detail="no installation image at ${PAYLOAD_DIR}"
        return 1
    fi
    sum="$(sha256sum "${index}" | cut -d' ' -f1)"
    if [ "${sum}" != "${payload_index_sha256}" ]; then
        payload_state=different
        payload_detail="the index of the installation image is not the one this installer was built with"
        return 1
    fi
    for file in "${PAYLOAD_DIR}"/blobs/sha256/*; do
        [ -f "${file}" ] || continue
        if [ -n "${progress}" ]; then "${progress}" "${count}" "${payload_blob_count}"; fi
        count=$((count + 1))
        name="${file##*/}"
        sum="$(sha256sum "${file}" | cut -d' ' -f1)"
        if [ "${sum}" != "${name}" ]; then
            bad=$((bad + 1))
            mark "payload damaged file=${name}"
        fi
    done
    if [ -n "${progress}" ]; then "${progress}" "${count}" "${payload_blob_count}"; fi
    if [ "${bad}" -ne 0 ]; then
        payload_state=damaged
        payload_detail="${bad} of ${count} files of the installation image are damaged"
        return 1
    fi
    if [ "${count}" != "${payload_blob_count}" ]; then
        payload_state=incomplete
        payload_detail="the installation image has ${count} files, ${payload_blob_count} are expected"
        return 1
    fi
    payload_state=verified
    payload_detail="${count} files, $(human "${payload_blob_bytes:-0}"), checked in $((SECONDS - started)) s"
    return 0
}

# ---- checks on the computer ----------------------------------------------------------

# One entry per check, in four parallel arrays: an identifier, the state
# (ok, stop or note), the line the text installer shows, and the code of a
# check that stops the installation. `blocking` holds "CODE|what to do" for
# every check that stops it.
check_lines=()
check_ids=()
check_states=()
check_codes=()
blocking=()

secure_boot_state() {
    local file byte
    file=/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
    if [ ! -d /sys/firmware/efi ]; then
        echo unknown
        return
    fi
    if [ ! -r "${file}" ]; then
        # UEFI firmware without the variable has no Secure Boot.
        echo unsupported
        return
    fi
    byte="$(od -An -t u1 -j4 -N1 "${file}" 2> /dev/null | tr -d ' ')"
    case "${byte}" in
        1) echo enabled ;;
        0) echo disabled ;;
        *) echo unknown ;;
    esac
}

# add_check ID STATE LINE [CODE]
add_check() {
    check_ids+=("$1")
    check_states+=("$2")
    check_lines+=("$3")
    check_codes+=("${4:-}")
}

memory_bytes=0
secure_boot=unknown

run_checks() {
    local arch ram_kib
    check_lines=() check_ids=() check_states=() check_codes=() blocking=()
    if [ -d /sys/firmware/efi ]; then
        add_check boot-mode ok "[ok]    Started by UEFI firmware"
        mark "check boot_mode=uefi"
    else
        add_check boot-mode stop "[STOP]  Not started by UEFI firmware" TUR-INSTALL-PREFLIGHT-001
        blocking+=("TUR-INSTALL-PREFLIGHT-001|This computer was not started in UEFI mode. TUR starts only from UEFI. Enable UEFI boot (and turn off legacy or CSM boot) in the firmware settings, then start from this medium again.")
        mark "check boot_mode=not-uefi code=TUR-INSTALL-PREFLIGHT-001"
    fi
    arch="$(uname -m)"
    if [ "${arch}" = x86_64 ]; then
        add_check processor ok "[ok]    64-bit x86 processor"
    else
        add_check processor stop "[STOP]  Processor architecture: ${arch}" TUR-INSTALL-PREFLIGHT-003
        blocking+=("TUR-INSTALL-PREFLIGHT-003|TUR needs a 64-bit x86 processor. This one reports ${arch}.")
    fi
    mark "check architecture=${arch}"
    ram_kib="$(sed -n 's/^MemTotal:[[:space:]]*\([0-9]*\) kB/\1/p' /proc/meminfo)"
    memory_bytes=$((${ram_kib:-0} * 1024))
    if [ "${memory_bytes}" -ge "${MIN_RAM_BYTES}" ]; then
        add_check memory ok "[ok]    Memory: $(human "${memory_bytes}") usable"
    else
        add_check memory stop "[STOP]  Memory: $(human "${memory_bytes}") usable" TUR-INSTALL-PREFLIGHT-005
        blocking+=("TUR-INSTALL-PREFLIGHT-005|This computer has $(human "${memory_bytes}") of usable memory. TUR needs at least $(human "${MIN_RAM_BYTES}").")
    fi
    mark "check memory_bytes=${memory_bytes}"
    secure_boot="$(secure_boot_state)"
    add_check secure-boot note "[note]  Secure Boot in the firmware: ${secure_boot}"
    mark "check secure_boot=${secure_boot}"
    if [ "${payload_state}" = verified ]; then
        add_check payload ok "[ok]    Installation image on this medium: ${payload_detail}"
        mark "check payload=verified image_id=${payload_image_id}"
    else
        add_check payload stop "[STOP]  Installation image on this medium: ${payload_state}" TUR-INSTALL-MEDIUM-001
        blocking+=("TUR-INSTALL-MEDIUM-001|The installation image on this medium cannot be used: ${payload_detail}. Write the medium again, to another USB stick if you can, and compare its checksum before you do.")
        mark "check payload=${payload_state} code=TUR-INSTALL-MEDIUM-001"
    fi
}

# ---- disks ------------------------------------------------------------------------

d_name=() d_model=() d_serial=() d_wwn=() d_size=() d_tran=() d_byid=() d_status=() d_code=() d_why=() d_index=()
d_holds=() d_words=() d_windows=() d_bitlocker=() d_linux=() d_ntfs=() d_parts=()
selectable=0

prop() {
    lsblk -dn -b -o "$2" "/dev/$1" 2> /dev/null | head -n 1 | sed 's/^[[:space:]]*//; s/[[:space:]]*$//'
}

# The disks the installer environment itself runs from.
live_disks() {
    local mount source parent
    for mount in "${LIVE_MOUNT}" /run/initramfs/isoscan /run/install/repo; do
        source="$(findmnt -n -o SOURCE "${mount}" 2> /dev/null | head -n 1)"
        [ -b "${source}" ] || continue
        parent="$(lsblk -dn -o PKNAME "${source}" 2> /dev/null | head -n 1 | tr -d '[:space:]')"
        echo "${parent:-${source##*/}}"
    done
    if [ -e /run/initramfs/livedev ]; then
        source="$(readlink -f /run/initramfs/livedev)"
        if [ -b "${source}" ]; then
            parent="$(lsblk -dn -o PKNAME "${source}" 2> /dev/null | head -n 1 | tr -d '[:space:]')"
            echo "${parent:-${source##*/}}"
        fi
    fi
}

# by_id NAME SERIAL: the /dev/disk/by-id link that names the disk best: one
# that carries the serial number, else a WWN link.
by_id() {
    local name="$1" token link target fallback=""
    token="$(printf '%s' "$2" | tr ' ' '_')"
    for link in /dev/disk/by-id/*; do
        [ -L "${link}" ] || continue
        target="$(readlink -f "${link}")"
        [ "${target}" = "/dev/${name}" ] || continue
        if [ -n "${token}" ] && [ "${link#*"${token}"}" != "${link}" ]; then
            echo "${link}"
            return
        fi
        case "${link##*/}" in
            wwn-* | nvme-eui.*) [ -n "${fallback}" ] || fallback="${link}" ;;
        esac
    done
    echo "${fallback}"
}

in_use() {
    lsblk -ln -o MOUNTPOINTS "/dev/$1" 2> /dev/null | grep -q '[^[:space:]]' && return 0
    lsblk -ln -o TYPE "/dev/$1" 2> /dev/null | grep -qvE '^(disk|part)[[:space:]]*$' && return 0
    return 1
}

# What is on a disk now, as text for the screen.
contents() {
    lsblk -ln -o NAME,SIZE,FSTYPE,LABEL,PARTTYPENAME "/dev/$1" 2> /dev/null | tail -n +2
}

# A value that changes when the partitions or filesystems on a disk change.
fingerprint() {
    lsblk -ln -b -o NAME,SIZE,FSTYPE,UUID,PARTUUID,PARTTYPE "/dev/$1" 2> /dev/null | sha256sum | cut -d' ' -f1
}

# has_microsoft_boot PARTITION: does this EFI system partition hold
# EFI/Microsoft/Boot? Read-only: mounted read-only, in a mount namespace of
# its own that ends with the command, so nothing else sees the mount and it
# cannot outlive the look. FAT lookups ignore case.
has_microsoft_boot() {
    mkdir -p "${ESP_PROBE}"
    # shellcheck disable=SC2016  # expanded by the inner shell
    unshare --mount --propagation private /bin/sh -c '
        mount -t vfat -o ro,nodev,nosuid,noexec "$1" "$2" 2> /dev/null || exit 2
        test -d "$2/EFI/Microsoft/Boot"
        rc=$?
        umount "$2" 2> /dev/null
        exit "$rc"' _ "$1" "${ESP_PROBE}"
}

# classify NAME: what is on a disk, in plain words. Sets `holds` (for the
# screen), `words` (for the transcript), `windows` and `bitlocker` (yes or
# no), and the counts `linux_count` and `ntfs_count`. Reads only.
classify() {
    local name="$1" part ptype fstype magic ntfs=0 bitlocker_count=0 linux=0 esp=0 other=0
    local -a found=()
    windows=no bitlocker=no
    while read -r part; do
        [ -b "/dev/${part}" ] || continue
        # One query per field: lsblk's list output leaves an empty column
        # blank, which would shift the columns after it.
        ptype="$(prop "${part}" PARTTYPE)"
        ptype="${ptype,,}"
        fstype="$(prop "${part}" FSTYPE)"
        # BitLocker: lsblk's (blkid's) own detection, or the signature
        # "-FVE-FS-" at byte 3 of the volume.
        magic="$(head -c 11 "/dev/${part}" 2> /dev/null | tail -c 8 | tr -d '\000')"
        if [ "${fstype}" = BitLocker ] || [ "${magic}" = "-FVE-FS-" ]; then
            bitlocker_count=$((bitlocker_count + 1))
            continue
        fi
        case "${fstype}" in
            ntfs) ntfs=$((ntfs + 1)) ;;
            ext2 | ext3 | ext4 | xfs | btrfs | f2fs | crypto_LUKS | LVM2_member | swap | linux_raid_member | squashfs | erofs)
                linux=$((linux + 1))
                ;;
            vfat)
                if [ "${ptype}" = "${ESP_GUID}" ]; then
                    esp=$((esp + 1))
                    if has_microsoft_boot "/dev/${part}"; then windows=yes; fi
                else
                    other=$((other + 1))
                fi
                ;;
            "") ;;
            *) other=$((other + 1)) ;;
        esac
    done < <(lsblk -ln -o NAME "/dev/${name}" 2> /dev/null)
    if [ "${windows}" = yes ]; then found+=("Windows (EFI/Microsoft/Boot)"); fi
    if [ "${bitlocker_count}" -gt 0 ]; then
        bitlocker=yes
        found+=("BitLocker-encrypted volumes: ${bitlocker_count}")
    fi
    if [ "${ntfs}" -gt 0 ]; then found+=("NTFS: ${ntfs}"); fi
    if [ "${linux}" -gt 0 ]; then found+=("Linux filesystems: ${linux}"); fi
    if [ "${esp}" -gt 0 ] && [ "${windows}" = no ]; then found+=("EFI system partitions: ${esp}"); fi
    if [ "${other}" -gt 0 ]; then found+=("other filesystems: ${other}"); fi
    if [ "${#found[@]}" -eq 0 ]; then
        if [ "$(contents "${name}" | grep -c .)" -eq 0 ]; then
            holds="nothing (no partitions)"
        else
            holds="partitions without a recognised filesystem"
        fi
    else
        holds="$(printf '%s; ' "${found[@]}")"
        holds="${holds%; }"
    fi
    linux_count="${linux}"
    ntfs_count="${ntfs}"
    words="windows=${windows},bitlocker=${bitlocker_count},ntfs=${ntfs},linux=${linux},esp=${esp},other=${other}"
}

# serial_end I: the end of a disk's serial number, for the screen.
serial_end() {
    local s="${d_serial[$1]:-${d_wwn[$1]}}"
    if [ -z "${s}" ]; then
        echo "none reported"
    elif [ "${#s}" -gt 8 ]; then
        echo "...${s: -8}"
    else
        echo "${s}"
    fi
}

scan_disks() {
    local name type live index=0 i size serial wwn link status code why holds words windows bitlocker linux_count ntfs_count
    udevadm settle --timeout=15 > /dev/null 2>&1
    d_name=() d_model=() d_serial=() d_wwn=() d_size=() d_tran=() d_byid=() d_status=() d_code=() d_why=() d_index=()
    d_holds=() d_words=() d_windows=() d_bitlocker=() d_linux=() d_ntfs=() d_parts=()
    selectable=0
    live=" $(live_disks | sort -u | tr '\n' ' ')"
    while read -r name type; do
        [ "${type}" = disk ] || continue
        case "${name}" in
            zram* | loop* | ram* | nbd* | fd* | sr*) continue ;;
        esac
        size="$(prop "${name}" SIZE)"
        serial="$(prop "${name}" SERIAL)"
        wwn="$(prop "${name}" WWN)"
        link="$(by_id "${name}" "${serial}")"
        status=ok code="" why=""
        if [ "${live#* "${name}" }" != "${live}" ]; then
            status=installer-medium code=TUR-INSTALL-PREFLIGHT-008
            why="this is the medium the installer runs from"
        elif [ "${size:-0}" -eq 0 ]; then
            status=no-medium code=TUR-INSTALL-TARGET-006
            why="the drive holds no medium"
        elif [ "$(prop "${name}" RO)" = 1 ]; then
            status=read-only code=TUR-INSTALL-PREFLIGHT-012
            why="the disk is read-only"
        elif in_use "${name}"; then
            status=in-use code=TUR-INSTALL-PREFLIGHT-007
            why="something on the disk is mounted or open"
        elif [ -z "${link}" ] || { [ -z "${serial}" ] && [ -z "${wwn}" ]; }; then
            status=no-stable-identity code=TUR-INSTALL-TARGET-007
            why="the disk reports no serial number, so it cannot be told apart safely"
        elif [ "${size}" -lt "${min_target_bytes:-0}" ]; then
            status=too-small code=TUR-INSTALL-PREFLIGHT-006
            why="too small: $(human "${size}"); TUR needs at least $(human "${min_target_bytes}")"
        fi
        i=${#d_name[@]}
        d_name[i]="${name}"
        d_model[i]="$(prop "${name}" MODEL)"
        d_serial[i]="${serial}"
        d_wwn[i]="${wwn}"
        d_size[i]="${size:-0}"
        d_tran[i]="$(prop "${name}" TRAN)"
        d_byid[i]="${link}"
        d_status[i]="${status}"
        d_code[i]="${code}"
        d_why[i]="${why}"
        if [ "${status}" = ok ]; then
            index=$((index + 1))
            d_index[i]="${index}"
        else
            d_index[i]="-"
        fi
        d_parts[i]="$(contents "${name}" | grep -c .)"
        classify "${name}"
        d_holds[i]="${holds}"
        d_words[i]="${words}"
        d_windows[i]="${windows}"
        d_bitlocker[i]="${bitlocker}"
        d_linux[i]="${linux_count}"
        d_ntfs[i]="${ntfs_count}"
        mark "disk index=${d_index[i]} name=${name} size=${d_size[i]} serial=\"${serial}\" transport=${d_tran[i]:-unknown} status=${status} code=${code:-none} by_id=${link:-none} partitions=${d_parts[i]} holds=${words}"
    done < <(lsblk -dn -o NAME,TYPE 2> /dev/null)
    selectable="${index}"
    mark "disks total=${#d_name[@]} selectable=${selectable}"
}

# The storage inventory in the form installer/tur-installer-core reads, for
# the transcript only: real output from a real machine is what that library's
# fixtures lack (issue 16).
dump_inventory() {
    local line
    [ "${serial_port}" = yes ] || return 0
    lsblk --json --bytes --output-all 2> /dev/null | while IFS= read -r line; do
        printf 'TUR-INSTALLER-LSBLK| %s\r\n' "${line}" >&3
    done
    find /dev/disk/by-id /dev/disk/by-path -maxdepth 1 -type l -printf '%p -> %l\n' 2> /dev/null | sort \
        | while IFS= read -r line; do
            printf 'TUR-INSTALLER-LINKS| %s\r\n' "${line}" >&3
        done
}

# phrase_of SERIAL WWN SIZE: the phrase that confirms erasing a disk: "ERASE"
# and the last eight letters and digits of its serial number, or of its WWN
# when it has no serial number.
phrase_of() {
    local source cleaned
    for source in "$1" "$2"; do
        cleaned="$(printf '%s' "${source}" | tr -cd '[:alnum:]' | tr '[:lower:]' '[:upper:]')"
        if [ -n "${cleaned}" ]; then
            if [ "${#cleaned}" -gt 8 ]; then cleaned="${cleaned: -8}"; fi
            echo "ERASE ${cleaned}"
            return
        fi
    done
    echo "ERASE $3"
}

# phrase_for I: the phrase for disk I of the last scan.
phrase_for() {
    phrase_of "${d_serial[$1]}" "${d_wwn[$1]}" "${d_size[$1]}"
}

# ---- before writing ---------------------------------------------------------------

engine_code=""
engine_message=""
target_device=""

# revalidate BY_ID SERIAL WWN SIZE FINGERPRINT: look at the chosen disk again,
# immediately before writing. It must be the same disk, behind the same
# stable path, with the same content. On success sets `target_device`; on
# failure sets `engine_code` and `engine_message` and returns 1. Nothing is
# written either way.
revalidate() {
    local link="$1" serial="$2" wwn="$3" size="$4" print="$5" device name
    engine_code="" engine_message="" target_device=""
    udevadm settle --timeout=15 > /dev/null 2>&1
    if [ -z "${link}" ] || [ "${link#/dev/disk/by-id/}" = "${link}" ]; then
        engine_code=TUR-INSTALL-IDENTITY-001
        engine_message="The disk was not named by a stable path (${link:-none})."
        return 1
    fi
    if [ ! -L "${link}" ]; then
        engine_code=TUR-INSTALL-IDENTITY-001
        engine_message="The chosen disk is no longer there (${link})."
        return 1
    fi
    device="$(readlink -f "${link}")"
    name="${device##*/}"
    if [ ! -b "${device}" ] || [ "$(prop "${name}" TYPE)" != disk ]; then
        engine_code=TUR-INSTALL-IDENTITY-001
        engine_message="The chosen disk is no longer there (${link})."
        return 1
    fi
    if [ "$(prop "${name}" SERIAL)" != "${serial}" ] || [ "$(prop "${name}" WWN)" != "${wwn}" ]; then
        engine_code=TUR-INSTALL-IDENTITY-003
        engine_message="A different disk is now in the place of the chosen one."
        return 1
    fi
    if [ "$(prop "${name}" SIZE)" != "${size}" ]; then
        engine_code=TUR-INSTALL-IDENTITY-002
        engine_message="The chosen disk changed size."
        return 1
    fi
    if live_disks | grep -qx "${name}"; then
        engine_code=TUR-INSTALL-PREFLIGHT-008
        engine_message="The chosen disk is the medium the installer runs from."
        return 1
    fi
    if [ "$(prop "${name}" RO)" = 1 ]; then
        engine_code=TUR-INSTALL-PREFLIGHT-012
        engine_message="The chosen disk is read-only."
        return 1
    fi
    if in_use "${name}"; then
        engine_code=TUR-INSTALL-PREFLIGHT-007
        engine_message="Something on the chosen disk is mounted or open."
        return 1
    fi
    if [ "${size}" -lt "${min_target_bytes:-0}" ]; then
        engine_code=TUR-INSTALL-PREFLIGHT-006
        engine_message="The chosen disk is smaller than TUR needs."
        return 1
    fi
    if [ "$(fingerprint "${name}")" != "${print}" ]; then
        engine_code=TUR-INSTALL-CONFIRM-004
        engine_message="What is on the chosen disk changed after you confirmed."
        return 1
    fi
    target_device="${device}"
    mark "revalidated name=${name} device=${device} by_id=${link}"
    return 0
}

# ---- writing ----------------------------------------------------------------------

install_status=0
install_seconds=0
install_command=""

# The installation command, as words. The same for both installers.
install_words() {
    local -a extra=()
    read -r -a extra <<< "${payload_install_args}"
    install_command_words=(bootc install to-disk
        --source-imgref "oci:${PAYLOAD_DIR}:${payload_ref}"
        --target-imgref "${payload_target}"
        --generic-image --wipe "${extra[@]}"
        "$1")
}

# run_install DEVICE [POLL_FUNCTION]: run the installation onto DEVICE, which
# `revalidate` returned. Its output goes to INSTALL_LOG. POLL_FUNCTION, if
# given, is called about every two seconds while it runs and once after,
# with the process ID (empty after the end). Sets `install_status` and
# `install_seconds`; returns the status.
run_install() {
    local device="$1" poll="${2:-}" pid started inhibitor=""
    local -a install_command_words=()
    install_words "${device}"
    install_command="${install_command_words[*]}"
    : > "${INSTALL_LOG}"
    chmod 0644 "${INSTALL_LOG}" 2> /dev/null
    mark "installing device=${device} command=\"${install_command}\""
    # Nothing may suspend the computer, or let it be switched off by the
    # power button's default, while the disk is written. Best effort: the
    # installation does not depend on it.
    if command -v systemd-inhibit > /dev/null 2>&1; then
        systemd-inhibit --what=sleep:idle:shutdown:handle-power-key:handle-suspend-key:handle-lid-switch \
            --who="TUR installer" --why="TUR is being installed" --mode=block sleep infinity \
            > /dev/null 2>&1 < /dev/null &
        inhibitor=$!
    fi
    started="${SECONDS}"
    # A session of its own: no key pressed on a console can reach it.
    setsid --wait "${install_command_words[@]}" > "${INSTALL_LOG}" 2>&1 < /dev/null &
    pid=$!
    while kill -0 "${pid}" 2> /dev/null; do
        if [ -n "${poll}" ]; then "${poll}" "${pid}"; fi
        sleep 2
    done
    wait "${pid}"
    install_status=$?
    install_seconds=$((SECONDS - started))
    if [ -n "${inhibitor}" ]; then kill "${inhibitor}" 2> /dev/null; fi
    if [ -n "${poll}" ]; then "${poll}" ""; fi
    sync
    udevadm settle --timeout=15 > /dev/null 2>&1
    mark "install-finished status=${install_status} seconds=${install_seconds}"
    return "${install_status}"
}

# check_installed DEVICE: the least a finished installation must have left:
# an EFI system partition and a root partition.
check_installed() {
    local types
    types="$(lsblk -ln -o PARTTYPE "$1" 2> /dev/null | tr '[:upper:]' '[:lower:]')"
    printf '%s\n' "${types}" | grep -q "${ESP_GUID}" && printf '%s\n' "${types}" | grep -q "${ROOT_GUID}"
}

# root_partition DEVICE: the path of the root partition on an installed disk.
root_partition() {
    lsblk -ln -o PATH,PARTTYPE "$1" 2> /dev/null | awk -v g="${ROOT_GUID}" 'tolower($2) == g { print $1; exit }'
}

# record_root_features DEVICE: what the installed root filesystem is made
# with, for the record. TUR's root is not given ext4's casefold feature: the
# root holds /boot, and GRUB, which reads it at every start, refuses an ext4
# filesystem with a feature it does not know (ADR-0018, amendment of
# 13 November 2026; issue 81).
record_root_features() {
    local root features
    root="$(root_partition "$1")"
    [ -n "${root}" ] || return 0
    features="$(dumpe2fs -h "${root}" 2> /dev/null | sed -n 's/^Filesystem features:[[:space:]]*//p' | tr ' ' ',')"
    mark "root-filesystem device=${root} features=${features:-unknown}"
}

# keep_log DEVICE SESSION_ID: copy the installer's log to the installed disk,
# /var/log/tur-installer/SESSION_ID (specification 28.3). The installed
# system's /var is the ostree state root's var directory on the root
# partition; right after `bootc install` it may not hold log/ yet (the
# installed system's tmpfiles make it at the first start), and then it is
# made here. Every directory and file made here gets the SELinux label the
# installed system's own policy gives /var/log (its file_contexts, in the
# deployment), so that the installed system, which enforces SELinux, sees
# them as it would have made them. Nothing else on the disk is changed.
kept_log_path=""

# label_on FILE LABEL: set FILE's SELinux label without SELinux running here.
label_on() {
    if command -v setfattr > /dev/null 2>&1; then
        setfattr -h -n security.selinux -v "$2" "$1" 2>> "${LOG}"
    elif command -v chcon > /dev/null 2>&1; then
        chcon -h "$2" "$1" 2>> "${LOG}"
    else
        return 1
    fi
}

keep_log() {
    local device="$1" id="$2" root mnt="${SESSION}/target" var="" dest label="" file status=0 contexts made=""
    local candidate
    kept_log_path=""
    root="$(root_partition "${device}")"
    if [ -z "${root}" ]; then
        mark "log-kept result=no reason=no-root-partition"
        return 1
    fi
    mkdir -p "${mnt}"
    if ! mount -t ext4 -o nodev,nosuid,noexec "${root}" "${mnt}" 2>> "${LOG}"; then
        mark "log-kept result=no reason=mount-failed"
        return 1
    fi
    for candidate in "${mnt}"/ostree/deploy/*/var; do
        if [ -d "${candidate}" ]; then var="${candidate}"; break; fi
    done
    if [ -z "${var}" ]; then
        umount "${mnt}" 2>> "${LOG}"
        mark "log-kept result=no reason=no-state-root-var"
        return 1
    fi
    # The label of /var/log: from the disk if it has the directory, else from
    # the deployment's policy.
    if [ -d "${var}/log" ] && command -v getfattr > /dev/null 2>&1; then
        label="$(getfattr --only-values -h -n security.selinux "${var}/log" 2> /dev/null | tr -d '\000')"
    fi
    if [ -z "${label}" ]; then
        for contexts in "${mnt}"/ostree/deploy/*/deploy/*/etc/selinux/targeted/contexts/files/file_contexts; do
            [ -f "${contexts}" ] || continue
            label="$(awk '$1 == "/var/log(/.*)?" { print $NF; exit }' "${contexts}")"
            [ -n "${label}" ] && break
        done
    fi
    [ -d "${var}/log" ] || made="${var}/log"
    dest="${var}/log/tur-installer/${id}"
    mkdir -p "${dest}" || status=1
    for file in "${LOG}" "${INSTALL_LOG}" "${SESSION}"/*.json "${WIZARD_TRANSCRIPT}"; do
        [ -f "${file}" ] && cp "${file}" "${dest}/" 2>> "${LOG}"
    done
    if [ -n "${made}" ]; then chmod 0755 "${made}"; fi
    chmod 0750 "${var}/log/tur-installer" "${dest}" 2> /dev/null
    chmod 0640 "${dest}"/* 2> /dev/null
    if [ -n "${label}" ]; then
        for file in ${made:+"${made}"} "${var}/log/tur-installer" "${dest}" "${dest}"/*; do
            label_on "${file}" "${label}" || status=1
        done
    else
        status=1
    fi
    kept_log_path="/var/log/tur-installer/${id}"
    sync
    umount "${mnt}" 2>> "${LOG}" || status=1
    mark "log-kept result=$([ "${status}" -eq 0 ] && echo yes || echo partly) path=${kept_log_path} selinux_label=${label:-none} made_var_log=$([ -n "${made}" ] && echo yes || echo no)"
    return "${status}"
}

# A name for this installation, for the log directory on the installed disk.
session_id() {
    local random
    random="$(tr -d '-' < /proc/sys/kernel/random/uuid 2> /dev/null | cut -c1-8)"
    printf '%s-%s\n' "$(date -u +%Y%m%dT%H%M%SZ)" "${random:-0}"
}

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
#   - or, on a disk that holds Windows and has free space the person made in
#     Windows, installs TUR into that space only, sharing Windows' EFI system
#     partition (ADR-0029; "beside Windows" below): one new partition, then
#     `bootc install to-filesystem`, then "Windows" in TUR's boot menu;
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
# Installing into the free space beside Windows (ADR-0029), per disk: whether
# it is possible, why not, the free stretch (bytes), the EFI system partition
# that is shared, its size, free space and what TUR's boot loader needs there,
# and the partitions just before and after the free stretch.
d_beside=() d_bcode=() d_bwhy=() d_bstart=() d_bsize=() d_besp=() d_bespuuid=() d_bespfree=() d_bespsize=()
d_bneed=() d_bafter=() d_bbefore=()
# Removing TUR from beside Windows: whether, TUR's partition and its size.
d_remove=() d_rpart=() d_rsize=()
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
    d_beside=() d_bcode=() d_bwhy=() d_bstart=() d_bsize=() d_besp=() d_bespuuid=() d_bespfree=() d_bespsize=()
    d_bneed=() d_bafter=() d_bbefore=()
    d_remove=() d_rpart=() d_rsize=()
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
        d_beside[i]=no d_bcode[i]="" d_bwhy[i]="" d_bstart[i]=0 d_bsize[i]=0 d_besp[i]="" d_bespuuid[i]=""
        d_bespfree[i]=0 d_bespsize[i]=0 d_bneed[i]=0 d_bafter[i]="" d_bbefore[i]=""
        d_remove[i]=no d_rpart[i]="" d_rsize[i]=0
        if [ "${status}" = ok ]; then
            remove_look "${name}"
            d_remove[i]="${r_possible}" d_rpart[i]="${r_part}" d_rsize[i]="${r_size}"
            if [ "${r_possible}" = yes ]; then
                mark "remove index=${d_index[i]} name=${name} possible=yes partition=${r_part:-none} size=${r_size} esp=${r_esp:-none} state=${r_state}"
            fi
        fi
        if [ "${status}" = ok ] && [ "${windows}" = yes ]; then
            beside_look "${name}"
            d_beside[i]="${b_possible}" d_bcode[i]="${b_code}" d_bwhy[i]="${b_why}"
            d_bstart[i]="${b_start}" d_bsize[i]="${b_size}" d_besp[i]="${b_esp}" d_bespuuid[i]="${b_esp_uuid}"
            d_bespfree[i]="${b_esp_free}" d_bespsize[i]="${b_esp_size}" d_bneed[i]="${b_needed}"
            d_bafter[i]="${b_after}" d_bbefore[i]="${b_before}"
            mark "beside index=${d_index[i]} name=${name} possible=${b_possible} code=${b_code:-none} free_start=${b_start} free_size=${b_size} after=${b_after:-none} before=${b_before:-none} esp=${b_esp:-none} esp_uuid=${b_esp_uuid:-none} esp_size=${b_esp_size} esp_free=${b_esp_free} esp_needed=${b_needed} why=\"${b_why}\""
        fi
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

# phrase_of SERIAL WWN SIZE [WORD]: the phrase that confirms what is done to
# a disk: WORD ("ERASE" unless given) and the last eight letters and digits
# of its serial number, or of its WWN when it has no serial number. Installing
# into the free space beside Windows is confirmed with "BESIDE" (ADR-0029).
phrase_of() {
    local source cleaned word="${4:-ERASE}"
    for source in "$1" "$2"; do
        cleaned="$(printf '%s' "${source}" | tr -cd '[:alnum:]' | tr '[:lower:]' '[:upper:]')"
        if [ -n "${cleaned}" ]; then
            if [ "${#cleaned}" -gt 8 ]; then cleaned="${cleaned: -8}"; fi
            echo "${word} ${cleaned}"
            return
        fi
    done
    echo "${word} $3"
}

# phrase_for I: the phrase for disk I of the last scan.
phrase_for() {
    phrase_of "${d_serial[$1]}" "${d_wwn[$1]}" "${d_size[$1]}"
}

# remove_phrase_for I: the phrase that confirms removing TUR from disk I.
remove_phrase_for() {
    phrase_of "${d_serial[$1]}" "${d_wwn[$1]}" "${d_size[$1]}" REMOVE
}

# beside_phrase_for I: the phrase that confirms installing beside Windows on
# disk I of the last scan.
beside_phrase_for() {
    phrase_of "${d_serial[$1]}" "${d_wwn[$1]}" "${d_size[$1]}" BESIDE
}

# ---- beside Windows: looking (ADR-0029) -------------------------------------------
#
# TUR can be installed into free space that the person made on a Windows disk
# with Windows' own Shrink Volume. Only the free space is written, plus TUR's
# own files added to the EFI system partition Windows already has; no Windows
# partition is changed, moved or resized. Everything in this section reads
# only.

readonly ALIGN_BYTES=$((1024 * 1024))
readonly LOADER_FILES=/usr/lib/bootupd/updates/EFI
readonly LOADER_FILES_USR=/usr/lib/efi
# Room kept free on the EFI system partition beyond what TUR's boot loader
# takes, for FAT's rounding and for Windows' own updates of its files there.
readonly ESP_MARGIN_BYTES=$((4 * 1024 * 1024))
readonly WINDOWS_ENTRY_TEMPLATE="${INSTALLER_DIR}/grub/windows-entry.cfg.in"
# The least free space TUR is installed into beside Windows (the owner's
# decision of 14 November 2026: 25 GiB).
readonly BESIDE_MIN_BYTES=$((25 * 1024 * 1024 * 1024))
# The firmware's default and "Restart into Windows" (ADR-0029); in the image.
readonly DUALBOOT=/usr/bin/tur-dualboot
readonly TUR_PARTLABEL=TUR
# What TUR found on the shared EFI system partition before it installed, kept
# there in a directory of its own so that TUR can be removed again, also
# after an interrupted installation, and Windows left exactly as it was:
# the partition table, the SHA-256 of every file, the firmware's boot order
# and a copy of Windows' fallback boot loader.
readonly STATE_DIR=EFI/tur
readonly STATE_FALLBACK=windows-bootx64.efi

# esp_look PARTITION COMMAND...: run COMMAND inside an EFI system partition
# mounted read-only, in a mount namespace of its own that ends with the
# command (as has_microsoft_boot does). The command runs in the partition's
# top directory. Status 90: the partition could not be mounted.
esp_look() {
    local part="$1"
    shift
    mkdir -p "${ESP_PROBE}"
    # shellcheck disable=SC2016  # expanded by the inner shell
    unshare --mount --propagation private /bin/sh -c '
        part="$1" dir="$2"
        shift 2
        mount -t vfat -o ro,nodev,nosuid,noexec "$part" "$dir" 2> /dev/null || exit 90
        cd "$dir" || exit 91
        "$@"
        rc=$?
        cd /
        umount "$dir" 2> /dev/null
        exit "$rc"' _ "${part}" "${ESP_PROBE}" "$@"
}

# esp_manifest PARTITION: every file on an EFI system partition with its
# SHA-256, sorted by path ("HASH  ./PATH"), without TUR's own record of what
# was there before (STATE_DIR).
esp_manifest() {
    # shellcheck disable=SC2016  # expanded by the inner shell
    esp_look "$1" /bin/sh -c 'find . -type f -print0 | sort -z | xargs -0 -r sha256sum' \
        | { grep -viE "^[0-9a-f]+  \./${STATE_DIR}(\.new)?/" || true; }
}

# tur_partitions NAME: the partitions of disk NAME that TUR's installer made
# beside Windows: named TUR_PARTLABEL, of the type "Linux root (x86-64)".
tur_partitions() {
    local part
    while read -r part; do
        if [ "$(prop "${part}" PARTLABEL)" = "${TUR_PARTLABEL}" ] \
            && [ "$(prop "${part}" PARTTYPE | tr '[:upper:]' '[:lower:]')" = "${ROOT_GUID}" ]; then
            echo "${part}"
        fi
    done < <(lsblk -ln -o NAME,TYPE "/dev/$1" 2> /dev/null | awk '$2 == "part" { print $1 }')
}

# remove_look NAME: can TUR be removed from disk NAME, where it was installed
# (or began to be installed) beside Windows? Sets r_possible (yes or no),
# r_part and r_size (TUR's partition, when there is one), r_esp (the shared
# EFI system partition) and r_state (yes when it holds TUR's record of what
# was there before). Reads only.
remove_look() {
    local name="$1" part esps=() parts=() leftover=no
    r_possible=no r_part="" r_size=0 r_esp="" r_state=no
    [ "$(prop "${name}" PTTYPE)" = gpt ] || return 1
    while read -r part; do
        if [ "$(prop "${part}" PARTTYPE | tr '[:upper:]' '[:lower:]')" = "${ESP_GUID}" ]; then esps+=("${part}"); fi
    done < <(lsblk -ln -o NAME,TYPE "/dev/${name}" 2> /dev/null | awk '$2 == "part" { print $1 }')
    [ "${#esps[@]}" -eq 1 ] || return 1
    r_esp="/dev/${esps[0]}"
    # Only beside Windows: a disk TUR has to itself is erased, not "removed from".
    esp_look "${r_esp}" test -f EFI/Microsoft/Boot/bootmgfw.efi || return 1
    if esp_look "${r_esp}" test -f "${STATE_DIR}/table-before.txt"; then r_state=yes; fi
    # An installation cut short while the record was being written leaves
    # only an unfinished directory of TUR's.
    if esp_look "${r_esp}" test -e "${STATE_DIR}" || esp_look "${r_esp}" test -e "${STATE_DIR}.new"; then leftover=yes; fi
    mapfile -t parts < <(tur_partitions "${name}")
    if [ "${#parts[@]}" -gt 1 ]; then return 1; fi
    if [ "${#parts[@]}" -eq 1 ]; then
        r_part="/dev/${parts[0]}"
        r_size="$(prop "${parts[0]}" SIZE)"
    fi
    if [ -z "${r_part}" ] && [ "${r_state}" = no ] && [ "${leftover}" = no ]; then return 1; fi
    r_possible=yes
    return 0
}

# loader_bytes: what TUR's boot loader takes on an EFI system partition, as
# bootupd copies it from this root (the same files as the payload's:
# build-medium.sh checks them file by file), each file rounded up to 4 KiB.
loader_bytes() {
    local source="${LOADER_FILES}"
    [ -d "${source}" ] || source="${LOADER_FILES_USR}"
    find "${source}" -type f -printf '%s\n' 2> /dev/null \
        | awk '{ total += int(($1 + 4095) / 4096) * 4096 } END { print total + 0 }'
}

# free_region NAME: the largest stretch of disk NAME that no partition uses,
# shrunk to whole MiB, as "START_BYTES SIZE_BYTES"; nothing if there is none.
free_region() {
    local name="$1" sector start end sectors first last best_start=0 best_size=0
    sector="$(blockdev --getss "/dev/${name}" 2> /dev/null)"
    sector="${sector:-512}"
    while read -r start end sectors; do
        first=$(((start * sector + ALIGN_BYTES - 1) / ALIGN_BYTES * ALIGN_BYTES))
        last=$(((end + 1) * sector / ALIGN_BYTES * ALIGN_BYTES))
        if [ "${last}" -gt "${first}" ] && [ $((last - first)) -gt "${best_size}" ]; then
            best_start="${first}" best_size=$((last - first))
        fi
    done < <(sfdisk --list-free "/dev/${name}" 2> /dev/null \
        | awk '$1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ { print $1, $2, $3 }')
    if [ "${best_size}" -gt 0 ]; then echo "${best_start} ${best_size}"; fi
}

# neighbours NAME START SIZE: the partitions just before and just after a
# stretch of disk NAME ("BEFORE AFTER", "-" for none).
neighbours() {
    local name="$1" start="$2" size="$3" part pstart psize before=- after=- before_end=-1 after_start=-1
    while read -r part; do
        # lsblk's START is in 512-byte units whatever the disk's sectors are.
        pstart=$(($(prop "${part}" START) * 512))
        psize="$(prop "${part}" SIZE)"
        if [ $((pstart + psize)) -le "${start}" ] && [ $((pstart + psize)) -gt "${before_end}" ]; then
            before="${part}" before_end=$((pstart + psize))
        fi
        if [ "${pstart}" -ge $((start + size)) ] && { [ "${after_start}" -lt 0 ] || [ "${pstart}" -lt "${after_start}" ]; }; then
            after="${part}" after_start="${pstart}"
        fi
    done < <(lsblk -ln -o NAME,TYPE "/dev/${name}" 2> /dev/null | awk '$2 == "part" { print $1 }')
    echo "${before} ${after}"
}

# beside_look NAME: can TUR be installed into free space on disk NAME, beside
# the Windows on it? Sets b_possible (yes or no), b_code and b_why when not,
# and what was found: b_start and b_size (the free stretch, bytes), b_before
# and b_after (its neighbours), b_esp (the EFI system partition), b_esp_uuid,
# b_esp_size, b_esp_free, b_needed (what TUR needs on it). Reads only.
beside_look() {
    local name="$1" pttype part esps=() region needed fallback=0 df_line pair
    b_possible=no b_code="" b_why="" b_start=0 b_size=0 b_before="" b_after="" b_esp="" b_esp_uuid=""
    b_esp_size=0 b_esp_free=0 b_needed=0
    pttype="$(prop "${name}" PTTYPE)"
    if [ "${pttype}" != gpt ]; then
        b_code=TUR-INSTALL-BESIDE-001
        b_why="the disk has no GPT partition table (${pttype:-none}): Windows on it starts in the old BIOS way, and TUR starts only by UEFI"
        return 1
    fi
    while read -r part; do
        [ -b "/dev/${part}" ] || continue
        if [ "$(prop "${part}" PARTTYPE | tr '[:upper:]' '[:lower:]')" = "${ESP_GUID}" ]; then esps+=("${part}"); fi
    done < <(lsblk -ln -o NAME,TYPE "/dev/${name}" 2> /dev/null | awk '$2 == "part" { print $1 }')
    if [ "${#esps[@]}" -ne 1 ]; then
        b_code=TUR-INSTALL-BESIDE-002
        b_why="the disk has ${#esps[@]} EFI system partitions; TUR shares one, and only when there is exactly one"
        return 1
    fi
    b_esp="/dev/${esps[0]}"
    b_esp_uuid="$(prop "${esps[0]}" UUID)"
    if ! esp_look "${b_esp}" test -f EFI/Microsoft/Boot/bootmgfw.efi; then
        b_code=TUR-INSTALL-BESIDE-002
        b_why="the EFI system partition does not hold Windows Boot Manager (EFI/Microsoft/Boot/bootmgfw.efi)"
        return 1
    fi
    if tur_partitions "${name}" | grep -q .; then
        b_code=TUR-INSTALL-BESIDE-005
        b_why="TUR is already on this disk, or an installation of it was interrupted. Remove TUR from this disk first, then install again"
        return 1
    fi
    if esp_look "${b_esp}" test -e EFI/fedora; then
        b_code=TUR-INSTALL-BESIDE-005
        if esp_look "${b_esp}" test -e "${STATE_DIR}"; then
            b_why="TUR is already on this disk, or an installation of it was interrupted. Remove TUR from this disk first, then install again"
        else
            b_why="the EFI system partition already holds another Linux system's start-up files (EFI/fedora), which TUR's would replace"
        fi
        return 1
    fi
    df_line="$(esp_look "${b_esp}" df -B1 --output=size,avail . 2> /dev/null | tail -n 1)"
    read -r b_esp_size b_esp_free <<< "${df_line}"
    b_esp_size="${b_esp_size:-0}" b_esp_free="${b_esp_free:-0}"
    # Windows' fallback boot loader is set aside and put back afterwards,
    # through a copy beside it: room for that copy too.
    fallback="$(esp_look "${b_esp}" stat -c %s EFI/BOOT/BOOTX64.EFI 2> /dev/null || echo 0)"
    needed=$(($(loader_bytes) + ${fallback:-0} + ESP_MARGIN_BYTES))
    b_needed="${needed}"
    region="$(free_region "${name}")"
    if [ -n "${region}" ]; then
        read -r b_start b_size <<< "${region}"
        pair="$(neighbours "${name}" "${b_start}" "${b_size}")"
        b_before="${pair% *}" b_after="${pair#* }"
        [ "${b_before}" != - ] || b_before=""
        [ "${b_after}" != - ] || b_after=""
    fi
    if [ "${b_size}" -lt "${BESIDE_MIN_BYTES}" ]; then
        b_code=TUR-INSTALL-BESIDE-003
        b_why="the largest free space on the disk is $(human "${b_size}"); TUR needs $(human "${BESIDE_MIN_BYTES}"). Make room in Windows: Disk Management, right-click the Windows partition, Shrink Volume"
        return 1
    fi
    if [ "${b_esp_free}" -lt "${needed}" ]; then
        b_code=TUR-INSTALL-BESIDE-004
        b_why="the EFI system partition has $(human "${b_esp_free}") free; TUR's start-up files need $(human "${needed}")"
        return 1
    fi
    b_possible=yes
    return 0
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
    local device="$1" poll="${2:-}"
    local -a install_command_words=()
    install_words "${device}"
    run_words "${device}" "${poll}" "${install_command_words[@]}"
}

# run_words DEVICE POLL_FUNCTION WORD...: run the installation command WORDS
# for DEVICE (run_install's body, shared with run_beside). POLL_FUNCTION may
# be empty.
run_words() {
    local device="$1" poll="${2:-}" pid started inhibitor=""
    shift 2
    local -a install_command_words=("$@")
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
    local device="$1" id="$2" root mnt="${SESSION}/target" var="" dest label="" label_from=none file status=0 contexts made=""
    local copied=0 labelled=yes unmounted=yes state remounted=no
    local candidate
    kept_log_path=""
    root="$(root_partition "${device}")"
    if [ -z "${root}" ]; then
        mark "log-kept result=no reason=no-root-partition"
        return 1
    fi
    mkdir -p "${mnt}"
    # bootc 1.17.1 leaves the new root mounted read-only at
    # /run/bootc/storage after `install to-disk` (TUR-009), so a new mount of
    # it shares that read-only filesystem. Recorded, then dealt with below.
    mark "log-keep before-mount=\"$(findmnt -n -S "${root}" -o TARGET,OPTIONS 2> /dev/null | tr '\n' ';')\" device_read_only=$(blockdev --getro "${root}" 2> /dev/null)"
    if ! mount -t ext4 -o rw,nodev,nosuid,noexec "${root}" "${mnt}" 2>> "${LOG}"; then
        mark "log-kept result=no reason=mount-failed $(tail -n 1 "${LOG}")"
        return 1
    fi
    if ! touch "${mnt}/.tur-installer-write-test" 2> /dev/null; then
        # The filesystem is still held read-only by bootc's mount. Make it
        # writable for the time it takes to copy the log, and read-only
        # again afterwards, as bootc left it.
        mount -o remount,rw "${mnt}" 2>> "${LOG}"
        remounted=yes
        mark "log-keep remount-rw options=$(findmnt -n -o OPTIONS "${mnt}" 2> /dev/null)"
    fi
    rm -f "${mnt}/.tur-installer-write-test"
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
        [ -n "${label}" ] && label_from=disk
    fi
    if [ -z "${label}" ]; then
        for contexts in "${mnt}"/ostree/deploy/*/deploy/*/etc/selinux/targeted/contexts/files/file_contexts \
            "${mnt}"/ostree/deploy/*/deploy/*/usr/etc/selinux/targeted/contexts/files/file_contexts \
            /etc/selinux/targeted/contexts/files/file_contexts; do
            # The last is the installer environment's own policy, which
            # build-medium.sh checks is the installed image's, file by file.
            [ -f "${contexts}" ] || continue
            label="$(awk '$1 == "/var/log(/.*)?" { print $NF; exit }' "${contexts}")"
            if [ -n "${label}" ]; then
                label_from="${contexts#"${mnt}"}"
                break
            fi
        done
    fi
    if [ -z "${label}" ]; then
        # Fedora's targeted policy's label for /var/log (when
        # neither file_contexts holds the line looked for, as in TUR-009).
        label=system_u:object_r:var_log_t:s0
        label_from=default
    fi
    [ -d "${var}/log" ] || made="${var}/log"
    mark "log-keep var=${var#"${mnt}"} var_holds=\"$(find "${var}" -mindepth 1 -maxdepth 1 -printf '%f ' 2> /dev/null)\" var_attributes=\"$(lsattr -d "${var}" 2> /dev/null | cut -d' ' -f1)\" label_from=${label_from} label=${label:-none}"
    dest="${var}/log/tur-installer/${id}"
    mkdir -p "${dest}" 2>> "${LOG}" || { status=1; mark "log-keep mkdir=failed $(tail -n 1 "${LOG}")"; }
    for file in "${LOG}" "${INSTALL_LOG}" "${SESSION}"/*.json "${WIZARD_TRANSCRIPT}" \
        "${SESSION}"/beside-table-*.txt "${SESSION}"/beside-esp-*.sha256; do
        if [ -f "${file}" ]; then
            if cp "${file}" "${dest}/" 2>> "${LOG}"; then copied=$((copied + 1)); else status=1; fi
        fi
    done
    if [ -n "${made}" ]; then chmod 0755 "${made}"; fi
    chmod 0750 "${var}/log/tur-installer" "${dest}" 2> /dev/null
    chmod 0640 "${dest}"/* 2> /dev/null
    if [ -n "${label}" ]; then
        for file in ${made:+"${made}"} "${var}/log/tur-installer" "${dest}" "${dest}"/*; do
            label_on "${file}" "${label}" || { status=1; labelled=no; }
        done
    else
        status=1
        labelled=no
    fi
    kept_log_path="/var/log/tur-installer/${id}"
    sync
    # Read-only again before letting go: this writes the journal out, so
    # the disk is consistent whatever happens to the live session next.
    if [ "${remounted}" = yes ]; then mount -o remount,ro "${mnt}" 2>> "${LOG}" || status=1; fi
    umount "${mnt}" 2>> "${LOG}" || { status=1; unmounted=no; }
    state="$(dumpe2fs -h "${root}" 2> /dev/null | sed -n 's/^Filesystem state:[[:space:]]*//p')"
    mark "log-kept result=$([ "${status}" -eq 0 ] && echo yes || echo partly) path=${kept_log_path} files=${copied} selinux_label=${label:-none} labelled=${labelled} made_var_log=$([ -n "${made}" ] && echo yes || echo no) unmounted=${unmounted} filesystem_state=\"${state}\""
    return "${status}"
}

# ---- beside Windows: writing (ADR-0029) -------------------------------------------
#
# The order, chosen so that a power cut at any moment leaves Windows as it
# was (each step's effect on the disk, if it is cut short):
#
#   1. record     the partition table, the SHA-256 of every file on the EFI
#                 system partition, the firmware's boot order and a copy of
#                 Windows' fallback boot loader are written to a directory of
#                 TUR's own on the EFI system partition (EFI/tur): from here
#                 on TUR can be removed again and Windows left exactly as it
#                 was (run_remove), whatever interrupts the installation
#   2. partition  one GPT entry is added for the free stretch (sfdisk
#                 --append); GPT keeps two copies with checksums, so a cut
#                 leaves the old table or the new one. No existing entry, and
#                 no byte outside the free stretch and the GPT, is written
#   3. filesystem ext4 is made inside the new partition only
#   4. deploy     bootc install to-filesystem into it; bootc's last step is
#                 the boot loader: bootupd adds EFI/fedora and EFI/BOOT/fbx64.efi
#                 to the shared EFI system partition, replaces
#                 EFI/BOOT/BOOTX64.EFI (Windows' fallback copy of its boot
#                 manager) with shim, and adds a "TUR" firmware boot entry at
#                 the front of the boot order. Nothing under EFI/Microsoft is
#                 written, and the "Windows Boot Manager" entry stays
#   5. fallback   Windows' own EFI/BOOT/BOOTX64.EFI is put back, through a
#                 copy and a rename: as soon as bootc's output shows that
#                 bootupd has finished, and again after bootc. A power cut
#                 between bootupd and that moment (a few seconds) leaves
#                 TUR's shim in its place and "TUR" first in the boot order;
#                 Windows' own boot entry and boot manager are untouched,
#                 and removing TUR puts the file and the order back
#   6. boot-menu  a "Windows" entry in TUR's boot menu (/boot/grub2/custom.cfg
#                 on TUR's partition)
#   7. check      every file that was on the EFI system partition is
#                 byte-identical, and every partition entry that was there is
#                 unchanged
#
# Windows' partitions are never opened for writing. The EFI system partition
# is mounted writable only inside bootupd (step 4) and for the rename of
# step 5.

beside_code=""
beside_message=""
beside_partition=""
beside_disk_state=untouched

# beside_fail CODE MESSAGE: why the installation beside Windows stopped.
beside_fail() {
    beside_code="$1" beside_message="$2"
    mark "beside result=failed code=$1 disk_state=${beside_disk_state} message=\"$2\""
    return 1
}

# The front end's stage line, if it has one (tur-install-engine does).
beside_stage_hook() {
    if declare -F beside_stage > /dev/null; then beside_stage "$1"; fi
}

# beside_revalidate START SIZE: after `revalidate` (the same disk, the same
# partitions), installing beside Windows must still be possible, into the
# same free stretch.
beside_revalidate() {
    local name="${target_device##*/}"
    if ! beside_look "${name}"; then
        engine_code="${b_code}"
        engine_message="TUR can no longer be installed beside Windows on this disk: ${b_why}."
        return 1
    fi
    if [ "${b_start}" != "$1" ] || [ "${b_size}" != "$2" ]; then
        engine_code=TUR-INSTALL-CONFIRM-004
        engine_message="The free space on the disk is not the one you confirmed."
        return 1
    fi
    mark "beside revalidated name=${name} start=$1 size=$2 esp=${b_esp} esp_uuid=${b_esp_uuid}"
    return 0
}

# esp_put_back PARTITION SOURCE PATH: write SOURCE to PATH on an EFI system
# partition through a copy beside it and a rename, so that a power cut leaves
# the old file or the new one. The only write to the partition outside bootupd.
esp_put_back() {
    mkdir -p "${ESP_PROBE}"
    # shellcheck disable=SC2016  # expanded by the inner shell
    unshare --mount --propagation private /bin/sh -c '
        mount -t vfat -o rw,nodev,nosuid,noexec "$1" "$2" || exit 90
        dir="$(dirname "$2/$4")"
        cp "$3" "$dir/TURCOPY.TMP" && sync && mv -f "$dir/TURCOPY.TMP" "$2/$4" && sync
        rc=$?
        umount "$2" || rc=95
        exit "$rc"' _ "$1" "${ESP_PROBE}" "$2" "$3"
}

# esp_save_state PARTITION DIRECTORY: copy the files of DIRECTORY into
# STATE_DIR on an EFI system partition, as a whole: into a new directory,
# which then takes the place of the old one.
esp_save_state() {
    mkdir -p "${ESP_PROBE}"
    # shellcheck disable=SC2016  # expanded by the inner shell
    unshare --mount --propagation private /bin/sh -c '
        mount -t vfat -o rw,nodev,nosuid,noexec "$1" "$2" || exit 90
        rm -rf "$2/$4.new"
        mkdir -p "$2/$4.new" && cp "$3"/* "$2/$4.new/" && sync \
            && rm -rf "$2/$4" && mv "$2/$4.new" "$2/$4" && sync
        rc=$?
        umount "$2" || rc=95
        exit "$rc"' _ "$1" "${ESP_PROBE}" "$2" "${STATE_DIR}"
}

# beside_restore_fallback WHEN: put Windows' fallback boot loader back if
# bootupd replaced it. Uses beside_fb_* set by run_beside. Returns 1 only if
# it had to be put back and could not be.
beside_fb_esp="" beside_fb_path="" beside_fb_sum="" beside_fb_file="" beside_fb_early=no beside_user_poll=""
beside_restore_fallback() {
    local now
    [ -n "${beside_fb_path}" ] || return 0
    now="$(esp_look "${beside_fb_esp}" sha256sum "${beside_fb_path}" 2> /dev/null | cut -c1-64)"
    if [ "${now}" = "${beside_fb_sum}" ]; then
        mark "beside step=fallback when=$1 state=unchanged path=${beside_fb_path}"
        return 0
    fi
    mark "beside step=fallback when=$1 state=replaced-by-bootupd path=${beside_fb_path} now=${now:-none}"
    if ! esp_put_back "${beside_fb_esp}" "${beside_fb_file}" "${beside_fb_path}" >> "${LOG}" 2>&1; then
        mark "beside step=fallback-restored when=$1 same=no"
        return 1
    fi
    now="$(esp_look "${beside_fb_esp}" sha256sum "${beside_fb_path}" 2> /dev/null | cut -c1-64)"
    mark "beside step=fallback-restored when=$1 path=${beside_fb_path} sha256=${now:-none} same=$([ "${now}" = "${beside_fb_sum}" ] && echo yes || echo no)"
    [ "${now}" = "${beside_fb_sum}" ]
}

# beside_poll PID: run_beside's poll function while bootc runs: the caller's
# own, and, once bootc's output shows that bootupd has finished, Windows'
# fallback boot loader put back at once, without waiting for bootc's end.
beside_poll() {
    if [ -n "${beside_user_poll}" ]; then "${beside_user_poll}" "$1"; fi
    if [ "${beside_fb_early}" = no ] \
        && grep -qE 'Trimming |Finalizing filesystem|Installation complete' "${INSTALL_LOG}" 2> /dev/null; then
        beside_fb_early=yes
        beside_restore_fallback early
    fi
    return 0
}

# run_beside DEVICE START SIZE [POLL_FUNCTION] [DEFAULT]: install TUR into the
# free stretch START..START+SIZE (bytes) of DEVICE, which `revalidate` and
# `beside_revalidate` returned, sharing the EFI system partition b_esp.
# DEFAULT ("tur", the default, or "windows") is the system the computer then
# starts by default (the firmware's boot order, set with tur-dualboot). Sets
# install_status, install_seconds, beside_partition, beside_disk_state
# (untouched, partial or installed) and, on failure, beside_code and
# beside_message.
run_beside() {
    local device="$1" start="$2" size="$3" poll="${4:-}" default_system="${5:-tur}" sector part esp="${b_esp}" esp_uuid="${b_esp_uuid}"
    local title="Windows" one magic
    local before_table="${SESSION}/beside-table-before.txt" after_table="${SESSION}/beside-table-after.txt"
    local esp_before="${SESSION}/beside-esp-before.sha256" esp_after="${SESSION}/beside-esp-after.sha256"
    local fallback="${SESSION}/beside-fallback.efi" fallback_path="" fallback_sum="" root_mnt="${SESSION}/beside-root"
    local grub_dir changed fs_uuid label entries order state="${SESSION}/beside-state"
    local -a words=()
    beside_code="" beside_message="" beside_partition="" beside_disk_state=untouched install_status=1 install_seconds=0
    sector="$(blockdev --getss "${device}" 2> /dev/null)"
    sector="${sector:-512}"

    # 1. What is there now.
    mark "beside step=record device=${device} esp=${esp}"
    if ! sfdisk --dump "${device}" > "${before_table}" 2>> "${LOG}"; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The partition table could not be read. Nothing was changed."
        return 1
    fi
    if ! esp_manifest "${esp}" > "${esp_before}" 2>> "${LOG}" || [ ! -s "${esp_before}" ]; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The EFI system partition could not be read. Nothing was changed."
        return 1
    fi
    rm -f "${fallback}"
    fallback_path="$(awk '{ p = substr($0, 67) } tolower(p) == "./efi/boot/bootx64.efi" { print substr(p, 3); exit }' "${esp_before}")"
    if [ -n "${fallback_path}" ]; then
        esp_look "${esp}" cat "${fallback_path}" > "${fallback}" 2>> "${LOG}"
        fallback_sum="$(sha256sum "${fallback}" | cut -c1-64)"
        if ! grep -qxF "${fallback_sum}  ./${fallback_path}" "${esp_before}"; then
            beside_fail TUR-INSTALL-MEDIUM-002 "Windows' fallback boot loader (${fallback_path}) could not be copied. Nothing was changed."
            return 1
        fi
    fi
    mark "beside recorded partitions=$(grep -c '^/dev/' "${before_table}") esp_files=$(wc -l < "${esp_before}") fallback=${fallback_path:-none} fallback_sha256=${fallback_sum:-none}"
    # Kept on the EFI system partition, in TUR's own directory, before
    # anything else is written: what removing TUR needs to leave Windows
    # exactly as it was.
    rm -rf "${state}"
    mkdir -p "${state}"
    cp "${before_table}" "${state}/table-before.txt"
    cp "${esp_before}" "${state}/esp-before.sha256"
    efibootmgr 2> /dev/null | sed -n 's/^BootOrder: //p' > "${state}/boot-order-before.txt"
    if [ -n "${fallback_path}" ]; then
        cp "${fallback}" "${state}/${STATE_FALLBACK}"
        printf '%s\n' "${fallback_path}" > "${state}/fallback-path.txt"
    fi
    printf '%s\n' \
        "TUR's record of this disk before TUR was installed beside Windows." \
        "The TUR installer uses it to remove TUR again and leave Windows as it was:" \
        "table-before.txt (the partition table), esp-before.sha256 (every file on" \
        "this partition), boot-order-before.txt (the firmware's boot order) and" \
        "${STATE_FALLBACK} (a copy of Windows' EFI\\Boot\\bootx64.efi)." \
        "Nothing here is used to start Windows or TUR." > "${state}/README.txt"
    mark "beside step=state directory=${STATE_DIR}"
    beside_disk_state=partial
    if ! esp_save_state "${esp}" "${state}" >> "${LOG}" 2>&1; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The record of what is on the disk now could not be kept on the EFI system partition. No partition was changed."
        return 1
    fi
    mark "beside step=state-kept boot_order=$(tr -d '\n' < "${state}/boot-order-before.txt")"

    # 2. One new partition entry, for the free stretch.
    beside_stage_hook prepare
    mark "beside step=partition start=${start} size=${size} first_sector=$((start / sector)) sectors=$((size / sector)) sector_size=${sector}"
    if ! printf 'start=%s, size=%s, type=%s, name="%s"\n' "$((start / sector))" "$((size / sector))" \
        "${ROOT_GUID^^}" "${TUR_PARTLABEL}" \
        | sfdisk --append --wipe never --wipe-partitions never "${device}" >> "${LOG}" 2>&1; then
        beside_disk_state=unknown
        beside_fail TUR-INSTALL-MEDIUM-002 "The new partition could not be added to the partition table."
        return 1
    fi
    udevadm settle --timeout=30 > /dev/null 2>&1
    sfdisk --dump "${device}" > "${after_table}" 2>> "${LOG}"
    changed="$(grep -vxF -f "${after_table}" "${before_table}")"
    if [ -n "${changed}" ]; then
        beside_disk_state=unknown
        mark "beside table-changed lines=\"$(printf '%s' "${changed}" | tr '\n' '|')\""
        beside_fail TUR-INSTALL-BESIDE-006 "An entry of the partition table that was there before is different now. The installation was stopped."
        return 1
    fi
    part="$(lsblk -ln -o PATH,START,TYPE "${device}" 2> /dev/null | awk -v s=$((start / 512)) '$3 == "part" && $2 == s { print $1; exit }')"
    if [ -z "${part}" ] || [ "$(lsblk -dn -b -o SIZE "${part}" 2> /dev/null | tr -d ' ')" != "${size}" ] \
        || [ "$(lsblk -dn -o PARTLABEL "${part}" 2> /dev/null | sed 's/[[:space:]]*$//')" != "${TUR_PARTLABEL}" ]; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The new partition is not where it was written (${part:-not found})."
        return 1
    fi
    beside_partition="${part}"
    mark "beside step=partitioned partition=${part} entries_unchanged=$(grep -c '^/dev/' "${before_table}")"

    # 3. Its filesystem: ext4, made as bootc makes TUR's root (-O verity),
    # labelled plainly "TUR" (nothing finds it by its label: the kernel is
    # given its UUID).
    mark "beside step=filesystem partition=${part}"
    wipefs -a "${part}" >> "${LOG}" 2>&1
    if ! mkfs.ext4 -F -q -L "${TUR_PARTLABEL}" -O verity "${part}" >> "${LOG}" 2>&1; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The filesystem could not be made on the new partition."
        return 1
    fi
    udevadm settle --timeout=30 > /dev/null 2>&1
    fs_uuid="$(blkid -p -s UUID -o value "${part}" 2> /dev/null)"
    mark "beside step=filesystem-made partition=${part} uuid=${fs_uuid}"

    # 4. TUR, with bootc; its last step adds the boot loader to the shared EFI
    # system partition and the firmware entry. Without --generic-image:
    # bootc then asks bootupd for the EFI boot loader only (--auto: the
    # medium was started by UEFI) and a firmware entry (--update-firmware).
    mkdir -p "${root_mnt}"
    if ! mount -t ext4 "${part}" "${root_mnt}" 2>> "${LOG}"; then
        beside_fail TUR-INSTALL-MEDIUM-002 "The new filesystem could not be mounted."
        return 1
    fi
    beside_stage_hook copy
    mark "beside step=deploy root=${root_mnt}"
    words=(bootc install to-filesystem
        --source-imgref "oci:${PAYLOAD_DIR}:${payload_ref}"
        --target-imgref "${payload_target}"
        "${root_mnt}")
    beside_fb_esp="${esp}" beside_fb_path="${fallback_path}" beside_fb_sum="${fallback_sum}" beside_fb_file="${fallback}"
    beside_fb_early=no beside_user_poll="${poll}"
    run_words "${device}" beside_poll "${words[@]}"
    if [ "${install_status}" -ne 0 ]; then
        # bootupd may have run before bootc failed.
        beside_restore_fallback after-failure
        umount "${root_mnt}" 2>> "${LOG}"
        beside_fail TUR-INSTALL-MEDIUM-002 "bootc stopped with an error (status ${install_status}) after ${install_seconds} s."
        return 1
    fi
    mark "beside step=deployed seconds=${install_seconds}"

    # 5. Windows' fallback boot loader, as it was (again: the first time was
    # while bootc finished).
    beside_stage_hook boot-menu
    if ! beside_restore_fallback after; then
        install_status=1
        beside_fail TUR-INSTALL-BESIDE-006 "Windows' fallback boot loader (${fallback_path}) could not be put back."
        umount "${root_mnt}" 2>> "${LOG}"
        return 1
    fi

    # 6. "Windows" in TUR's boot menu. bootupd's grub.cfg sources
    # /boot/grub2/custom.cfg after the menu entries (41_custom.cfg), so the
    # entry comes after TUR's and boot counting's entry numbers stay as they
    # are (ADR-0014).
    # With BitLocker on the disk, the entry says that Windows may ask for its
    # recovery key when started from here (the owner's decision, 14 November
    # 2026); "Restart into Windows" (tur-dualboot) does not.
    for one in $(lsblk -ln -o PATH,TYPE "${device}" 2> /dev/null | awk '$2 == "part" { print $1 }'); do
        magic="$(head -c 11 "${one}" 2> /dev/null | tail -c 8 | tr -d '\000')"
        if [ "${magic}" = "-FVE-FS-" ] || [ "$(lsblk -dn -o FSTYPE "${one}" 2> /dev/null)" = BitLocker ]; then
            title="Windows (may ask for the BitLocker recovery key)"
        fi
    done
    mark "beside step=boot-menu esp_uuid=${esp_uuid} title=\"${title}\""
    mount -o remount,rw "${root_mnt}" 2>> "${LOG}"
    grub_dir="${root_mnt}/boot/grub2"
    if [ ! -f "${grub_dir}/grub.cfg" ] || [ ! -f "${WINDOWS_ENTRY_TEMPLATE}" ] || [ -z "${esp_uuid}" ]; then
        mark "beside boot-menu result=no grub_cfg=$([ -f "${grub_dir}/grub.cfg" ] && echo yes || echo no) template=$([ -f "${WINDOWS_ENTRY_TEMPLATE}" ] && echo yes || echo no)"
    else
        sed "s/@ESP_UUID@/${esp_uuid}/g; s/@WINDOWS_TITLE@/${title}/g" "${WINDOWS_ENTRY_TEMPLATE}" > "${grub_dir}/custom.cfg.tmp" \
            && sync && mv -f "${grub_dir}/custom.cfg.tmp" "${grub_dir}/custom.cfg"
        label="$(getfattr --only-values -h -n security.selinux "${grub_dir}/grub.cfg" 2> /dev/null | tr -d '\000')"
        if [ -n "${label}" ]; then label_on "${grub_dir}/custom.cfg" "${label}"; fi
        mark "beside boot-menu result=yes sourced=$(grep -q 'custom.cfg' "${grub_dir}/grub.cfg" && echo yes || echo no) label=${label:-none} sha256=$(sha256sum "${grub_dir}/custom.cfg" | cut -c1-64)"
    fi
    sync
    mount -o remount,ro "${root_mnt}" 2>> "${LOG}"
    umount "${root_mnt}" 2>> "${LOG}"

    # 7. Windows as it was.
    beside_stage_hook check
    mark "beside step=check"
    esp_manifest "${esp}" > "${esp_after}" 2>> "${LOG}"
    changed="$(grep -vxF -f "${esp_after}" "${esp_before}")"
    sfdisk --dump "${device}" > "${after_table}" 2>> "${LOG}"
    if [ -n "${changed}" ] || grep -qvxF -f "${after_table}" "${before_table}"; then
        beside_disk_state=installed
        install_status=1
        mark "beside windows-files changed=\"$(printf '%s' "${changed}" | tr '\n' '|')\""
        beside_fail TUR-INSTALL-BESIDE-006 "A file of Windows on the EFI system partition, or a partition entry, is different after the installation."
        return 1
    fi
    # 8. Which system the computer starts by default: the person's choice.
    mark "beside step=default system=${default_system}"
    if [ -x "${DUALBOOT}" ]; then
        mark "beside default $("${DUALBOOT}" default "${default_system}" 2>&1 | tr '\n' ' ')"
    else
        mark "beside default result=no reason=no-tur-dualboot"
    fi
    entries="$(efibootmgr 2> /dev/null)"
    order="$(printf '%s\n' "${entries}" | sed -n 's/^BootOrder: //p')"
    mark "beside firmware boot_order=${order:-none} tur=\"$(printf '%s\n' "${entries}" | grep -m1 -E '^Boot[0-9A-F]{4}\*? TUR' | cut -c1-8)\" windows=\"$(printf '%s\n' "${entries}" | grep -m1 'Windows Boot Manager' | cut -c1-8)\""
    mark "beside windows-files unchanged=yes files=$(wc -l < "${esp_before}") added=$(grep -cvxF -f "${esp_before}" "${esp_after}") entries=$(grep -c '^/dev/' "${before_table}")"
    beside_disk_state=installed
    install_status=0
    return 0
}

# ---- beside Windows: removing TUR again (ADR-0029) ------------------------------------
#
# Removing TUR from a disk it shares with Windows gives the space back and
# leaves Windows as it was before TUR: also after an installation that was
# interrupted. The order, each step safe to repeat after a power cut:
#
#   1. firmware   the "TUR" boot entries are deleted and the boot order is the
#                 one recorded before TUR, without entries that no longer exist
#   2. ESP        Windows' fallback boot loader is put back if TUR's shim is
#                 in its place; TUR's own files are deleted: EFI/fedora, what
#                 bootupd added to EFI/BOOT, and last TUR's record (EFI/tur).
#                 Nothing under EFI/Microsoft is touched, and no file that
#                 was there before TUR or that Windows added since
#   3. partition  TUR's partition is wiped of its filesystem signature and
#                 its GPT entry deleted: the space is unallocated again
#   4. check      the partition table and the files on the EFI system
#                 partition are compared with the record
#
# It runs from the installer medium only: TUR cannot remove the partition it
# is running from.

remove_code=""
remove_message=""
remove_table=unknown
remove_esp=unknown

# esp_clean PARTITION NAMES_FILE: step 2, inside the EFI system partition
# mounted writable in a mount namespace of its own. NAMES_FILE lists, in
# lower case, the file names bootupd puts into EFI/BOOT.
esp_clean() {
    mkdir -p "${ESP_PROBE}"
    # shellcheck disable=SC2016  # expanded by the inner shell
    unshare --mount --propagation private /bin/sh -c '
        mount -t vfat -o rw,nodev,nosuid,noexec "$1" "$2" || exit 90
        cd "$2" || exit 91
        state="$3" names="$4" saved="$5"
        rc=0
        same() { [ -f "$1" ] && [ -f "$2" ] && cmp -s "$1" "$2"; }
        if [ -f "$state/esp-before.sha256" ]; then
            # Windows fallback boot loader: back if TUR shim is in its place.
            fallback="$(cat "$state/fallback-path.txt" 2> /dev/null)"
            if [ -n "$fallback" ] && [ -f "$state/$saved" ]; then
                if [ ! -f "$fallback" ] || same "$fallback" EFI/fedora/shimx64.efi; then
                    mkdir -p "$(dirname "$fallback")"
                    cp "$state/$saved" "$(dirname "$fallback")/TURCOPY.TMP" && sync \
                        && mv -f "$(dirname "$fallback")/TURCOPY.TMP" "$fallback" || rc=1
                    echo "fallback=restored"
                elif same "$fallback" "$state/$saved"; then
                    echo "fallback=as-before"
                else
                    echo "fallback=changed-since-by-another-system-kept"
                fi
            elif same EFI/BOOT/BOOTX64.EFI EFI/fedora/shimx64.efi; then
                # There was none before TUR: the one there is TUR shim.
                rm -f EFI/BOOT/BOOTX64.EFI || rc=1
                echo "fallback=removed-there-was-none-before"
            fi
            # What bootupd added to EFI/BOOT: by name, and only files that
            # were not there before TUR.
            if [ -d EFI/BOOT ]; then
                for file in EFI/BOOT/*; do
                    [ -f "$file" ] || continue
                    name="$(basename "$file" | tr "[:upper:]" "[:lower:]")"
                    [ "$name" = bootx64.efi ] && continue
                    grep -qx "$name" "$names" || continue
                    if grep -qi "  \./efi/boot/$name\$" "$state/esp-before.sha256"; then continue; fi
                    rm -f "$file" || rc=1
                done
                rmdir EFI/BOOT 2> /dev/null
            fi
            # EFI/fedora is TUR own: the installer refuses a disk that has one.
            if ! grep -qi "  \./efi/fedora/" "$state/esp-before.sha256"; then
                rm -rf EFI/fedora || rc=1
            fi
            sync
            echo "tur-files=removed"
        else
            echo "tur-files=no-record-only-the-unfinished-record-removed"
        fi
        # Last: the record itself, and an unfinished one.
        rm -rf "$state" "$state.new" || rc=1
        sync
        cd /
        umount "$2" || rc=95
        exit "$rc"' _ "$1" "${ESP_PROBE}" "${STATE_DIR}" "$2" "${STATE_FALLBACK}"
}

# run_remove DEVICE: remove TUR from DEVICE, which `revalidate` returned and
# for which remove_look said yes (r_part, r_esp, r_state are its answers).
# Sets remove_table and remove_esp (same, differs or unknown: the comparison
# with the record) and, on failure, remove_code and remove_message.
run_remove() {
    local device="$1" part="${r_part}" esp="${r_esp}" state="${SESSION}/remove-state" names="${SESSION}/remove-names.txt"
    local number entry order kept item wanted cleaned now_table="${SESSION}/remove-table-after.txt"
    local now_esp="${SESSION}/remove-esp-after.sha256" source
    remove_code="" remove_message="" remove_table=unknown remove_esp=unknown
    mark "remove step=start device=${device} partition=${part:-none} esp=${esp} state=${r_state}"

    # The record, read before anything is removed.
    rm -rf "${state}"
    mkdir -p "${state}"
    if [ "${r_state}" = yes ]; then
        {
            esp_look "${esp}" cat "${STATE_DIR}/table-before.txt" > "${state}/table-before.txt"
            esp_look "${esp}" cat "${STATE_DIR}/esp-before.sha256" > "${state}/esp-before.sha256"
            esp_look "${esp}" cat "${STATE_DIR}/boot-order-before.txt" > "${state}/boot-order-before.txt"
        } 2>> "${LOG}"
    fi
    # The names bootupd gives the files it puts into EFI/BOOT.
    {
        for source in "${LOADER_FILES_USR}" "${LOADER_FILES}"; do
            find "${source}" -type f -ipath '*/EFI/BOOT/*' -printf '%f\n' 2> /dev/null | tr '[:upper:]' '[:lower:]'
        done
        printf '%s\n' fbx64.efi bootia32.efi fbia32.efi
    } > "${names}"

    # 1. The firmware: no "TUR" entry, the order as before TUR.
    mark "remove step=firmware"
    while read -r entry; do
        [ -n "${entry}" ] || continue
        efibootmgr --bootnum "${entry}" --delete-bootnum >> "${LOG}" 2>&1
        mark "remove firmware deleted=${entry}"
    done < <(efibootmgr 2> /dev/null | awk '
        /^Boot[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][* ]/ {
            rest = substr($0, 9); sub(/^\*?[ ]+/, "", rest); split(rest, parts, "\t"); sub(/[ ]+$/, "", parts[1])
            if (parts[1] == "TUR") print toupper(substr($0, 5, 4))
        }')
    order="$(efibootmgr 2> /dev/null | sed -n 's/^BootOrder: //p' | tr -d ' ')"
    wanted="$(tr -d ' \n' < "${state}/boot-order-before.txt" 2> /dev/null)"
    if [ -n "${wanted}" ] && [ -n "${order}" ]; then
        # The order as it was, without entries that no longer exist; then
        # whatever exists now and was not there.
        kept=""
        for item in $(printf '%s' "${wanted}" | tr ',' ' '); do
            case ",${order^^}," in *",${item^^},"*) kept+="${kept:+,}${item^^}" ;; esac
        done
        for item in $(printf '%s' "${order}" | tr ',' ' '); do
            case ",${kept}," in *",${item^^},"*) ;; *) kept+="${kept:+,}${item^^}" ;; esac
        done
        if [ -n "${kept}" ] && [ "${kept}" != "${order^^}" ]; then efibootmgr --bootorder "${kept}" >> "${LOG}" 2>&1; fi
    fi
    mark "remove firmware boot_order=$(efibootmgr 2> /dev/null | sed -n 's/^BootOrder: //p') before_tur=${wanted:-not-recorded}"

    # 2. The EFI system partition.
    mark "remove step=esp"
    if ! cleaned="$(esp_clean "${esp}" "${names}" 2>> "${LOG}")"; then
        remove_code=TUR-INSTALL-REMOVE-002
        remove_message="TUR's files could not all be removed from the EFI system partition. Windows' own files there were not touched."
        mark "remove esp result=failed $(printf '%s' "${cleaned}" | tr '\n' ' ')"
        return 1
    fi
    mark "remove esp $(printf '%s' "${cleaned}" | tr '\n' ' ')"

    # 3. TUR's partition.
    if [ -n "${part}" ]; then
        number="$(prop "${part##*/}" PARTN)"
        mark "remove step=partition partition=${part} number=${number}"
        wipefs -a "${part}" >> "${LOG}" 2>&1
        if [ -z "${number}" ] || ! sfdisk --delete "${device}" "${number}" >> "${LOG}" 2>&1; then
            remove_code=TUR-INSTALL-REMOVE-003
            remove_message="TUR's partition could not be deleted. Windows was not touched; delete the partition in Windows' Disk Management."
            mark "remove partition result=failed"
            return 1
        fi
        udevadm settle --timeout=30 > /dev/null 2>&1
    fi

    # 4. As before TUR?
    mark "remove step=check"
    sfdisk --dump "${device}" > "${now_table}" 2>> "${LOG}"
    esp_manifest "${esp}" > "${now_esp}" 2>> "${LOG}"
    if [ -s "${state}/table-before.txt" ]; then
        # The partition lines, without the kernel's name for the disk.
        if diff <(sed -n 's|^/dev/[^ ]*[^0-9]\([0-9]*\) : |\1 : |p' "${state}/table-before.txt") \
            <(sed -n 's|^/dev/[^ ]*[^0-9]\([0-9]*\) : |\1 : |p' "${now_table}") > /dev/null; then
            remove_table=same
        else
            remove_table=differs
        fi
        # Files Windows added or changed since are Windows' own business:
        # what counts is that every file of before is there as it was, and
        # that nothing of TUR's is left.
        if grep -qvxF -f "${now_esp}" "${state}/esp-before.sha256"; then
            remove_esp=differs
        else
            remove_esp=same
        fi
    fi
    mark "remove result=done table=${remove_table} esp=${remove_esp} esp_files=$(wc -l < "${now_esp}") left_of_tur=$(grep -ciE '  \./efi/(fedora|tur)/' "${now_esp}")"
    return 0
}

# A name for this installation, for the log directory on the installed disk.
session_id() {
    local random
    random="$(tr -d '-' < /proc/sys/kernel/random/uuid 2> /dev/null | cut -c1-8)"
    printf '%s-%s\n' "$(date -u +%Y%m%dT%H%M%SZ)" "${random:-0}"
}

# shellcheck shell=bash disable=SC2034  # its variables are read by the code that sources it
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Shrinking a Windows (NTFS) partition to make room for TUR: issue 64, step 2
# (ADR-0029, "Step 2"). A library: it decides whether a volume may be shrunk,
# to what size, and shrinks it in an order meant to survive a power cut at
# every step. engine.sh sources it (shrink_look, run_shrink): both
# installers offer "Make room by shrinking Windows". It is tested on NTFS
# volumes made by mkfs.ntfs only (tests/iso/ntfs-shrink-test.py, and the
# iso workflow's beside job); nothing here has met a real Windows volume.
#
# Needs ntfs-3g's ntfsprogs (ntfsresize, ntfscat, ntfsinfo), sfdisk and
# blockdev. Defines no output functions: the caller may define
# `mark TEXT` (a state line); otherwise state lines go to standard error.
#
#   ntfs_look PARTITION
#       Whether it may be shrunk, and how far. Reads only.
#   ntfs_target_bytes WANTED
#       WANTED (bytes) checked against what ntfs_look found and the margin.
#   ntfs_shrink DISK NUMBER PARTITION NEW_BYTES
#       Dry run, then the filesystem, then the partition entry. Writes.
#
# Refusals, each with its code (medium-only codes; ADR-0029 lists them):
#
#   TUR-INSTALL-SHRINK-001  BitLocker ("-FVE-FS-" at byte 3): shrink it from
#                           Windows, or turn BitLocker off first
#   TUR-INSTALL-SHRINK-002  not an NTFS volume
#   TUR-INSTALL-SHRINK-003  hibernated, or shut down with Fast Startup
#                           (hiberfil.sys begins "hibr"): shut Windows down
#                           fully (Shift + Shut down) or turn off Fast Startup
#   TUR-INSTALL-SHRINK-004  scheduled for a check (the NTFS dirty flag), or
#                           ntfsresize cannot open it (unclean journal,
#                           corrupt): start Windows, let it check the disk,
#                           shut it down fully
#   TUR-INSTALL-SHRINK-005  too little room: the smallest size ntfsresize
#                           reports plus the margin is not smaller than now
#   TUR-INSTALL-SHRINK-006  the dry run (ntfsresize --no-action) failed
#   TUR-INSTALL-SHRINK-007  the resize or the partition change failed
#   TUR-INSTALL-SHRINK-008  afterwards the volume or the table is not as
#                           expected

# Room kept free on the Windows partition beyond the smallest size
# ntfsresize reports: Microsoft asks for 16 GB free on the Windows partition
# after setup (configure-uefigpt-based-hard-drive-partitions), for updates.
NTFS_MARGIN_BYTES="${NTFS_MARGIN_BYTES:-$((16 * 1024 * 1024 * 1024))}"
readonly NTFS_ALIGN_BYTES=$((1024 * 1024))

if ! declare -F mark > /dev/null; then
    mark() { printf 'TUR-INSTALLER: %s\n' "$1" >&2; }
fi

n_state="" n_code="" n_why="" n_size=0 n_min=0 n_cluster=0 n_info=""

# ntfs_refuse STATE CODE WHY
ntfs_refuse() {
    n_state="$1" n_code="$2" n_why="$3"
    mark "shrink look state=$1 code=$2"
    return 1
}

# ntfs_look PARTITION: may this volume be shrunk? Sets n_state (ok or the
# reason), n_code, n_why, and from ntfsresize --info: n_size (the volume's
# size), n_min (the smallest it can be resized to), n_cluster. Reads only:
# the first sector with head, hiberfil.sys's first bytes with ntfscat,
# ntfsresize --info (which opens the volume read-only).
ntfs_look() {
    local part="$1" magic hiber status
    n_state="" n_code="" n_why="" n_size=0 n_min=0 n_cluster=0 n_info=""
    magic="$(head -c 11 "${part}" 2> /dev/null | tail -c 8 | tr -d '\000')"
    case "${magic}" in
        -FVE-FS-)
            ntfs_refuse bitlocker TUR-INSTALL-SHRINK-001 "The Windows partition is encrypted with BitLocker. Shrink it from Windows (Disk Management, Shrink Volume), or turn BitLocker off first."
            return 1
            ;;
        "NTFS    ") ;;
        *)
            ntfs_refuse not-ntfs TUR-INSTALL-SHRINK-002 "This partition does not hold an NTFS volume."
            return 1
            ;;
    esac
    # Windows hibernated, or shut down with Fast Startup, leaves its session
    # in hiberfil.sys, which then begins "hibr"; a resized volume would not
    # match it when Windows resumes. (ntfs-3g refuses such a volume too.)
    hiber="$(ntfscat "${part}" hiberfil.sys 2> /dev/null | head -c 4 | tr '[:upper:]' '[:lower:]')"
    if [ "${hiber}" = hibr ]; then
        ntfs_refuse hibernated TUR-INSTALL-SHRINK-003 "Windows is hibernated, or was shut down with Fast Startup. Start Windows, then shut it down fully: hold Shift while you choose Shut down. Or turn off Fast Startup."
        return 1
    fi
    n_info="$(ntfsresize --info --no-progress-bar "${part}" 2>&1)"
    status=$?
    if printf '%s\n' "${n_info}" | grep -q 'scheduled for check'; then
        ntfs_refuse dirty TUR-INSTALL-SHRINK-004 "Windows has marked this partition to be checked. Start Windows and let it check the disk, then shut it down fully (Shift + Shut down)."
        return 1
    fi
    if printf '%s\n' "${n_info}" | grep -qi 'hibernat'; then
        ntfs_refuse hibernated TUR-INSTALL-SHRINK-003 "Windows is hibernated, or was shut down with Fast Startup. Start Windows, then shut it down fully: hold Shift while you choose Shut down."
        return 1
    fi
    if [ "${status}" -ne 0 ]; then
        ntfs_refuse unreadable TUR-INSTALL-SHRINK-004 "ntfsresize cannot use this volume (status ${status}). Start Windows, let it check the disk, then shut it down fully."
        return 1
    fi
    n_size="$(printf '%s\n' "${n_info}" | sed -n 's/^Current volume size[^0-9]*\([0-9]*\) bytes.*/\1/p' | head -n 1)"
    n_min="$(printf '%s\n' "${n_info}" | sed -n 's/^You might resize at \([0-9]*\) bytes.*/\1/p' | head -n 1)"
    n_cluster="$(printf '%s\n' "${n_info}" | sed -n 's/^Cluster size[^0-9]*\([0-9]*\) bytes.*/\1/p' | head -n 1)"
    if [ -z "${n_size}" ] || [ -z "${n_min}" ]; then
        ntfs_refuse unreadable TUR-INSTALL-SHRINK-004 "ntfsresize did not report the volume's sizes."
        return 1
    fi
    if [ $((n_min + NTFS_MARGIN_BYTES)) -ge "${n_size}" ]; then
        ntfs_refuse too-full TUR-INSTALL-SHRINK-005 "Windows' partition is too full to give room: it could shrink to $((n_min / 1048576)) MiB, and $((NTFS_MARGIN_BYTES / 1048576)) MiB more are kept free for Windows."
        return 1
    fi
    n_state=ok
    mark "shrink look state=ok size=${n_size} min=${n_min} cluster=${n_cluster} margin=${NTFS_MARGIN_BYTES}"
    return 0
}

# ntfs_target_bytes WANTED: WANTED rounded down to whole MiB, if it keeps
# the margin and is smaller than now; nothing (status 1) otherwise.
ntfs_target_bytes() {
    local wanted=$(($1 / NTFS_ALIGN_BYTES * NTFS_ALIGN_BYTES))
    if [ "${wanted}" -lt $((n_min + NTFS_MARGIN_BYTES)) ] || [ "${wanted}" -ge "${n_size}" ]; then
        return 1
    fi
    echo "${wanted}"
}

# ntfs_shrink DISK NUMBER PARTITION NEW_BYTES: shrink the NTFS volume on
# partition NUMBER of DISK (device PARTITION) to NEW_BYTES, then its
# partition entry. The order, for a power cut at each step:
#
#   1. ntfsresize --no-action: reads only;
#   2. ntfsresize: moves clusters beyond the new end, then shrinks the
#      filesystem and sets its dirty flag (Windows checks it at its next
#      start). NOT safe against a power cut while clusters move: ntfsresize
#      has no journal. Measured in tests/iso/ntfs-shrink-test.sh, not proven;
#   3. the partition entry is made smaller (sfdisk -N, the start and
#      everything else kept). A cut before this leaves a smaller volume in an
#      unchanged partition, which Windows accepts;
#   4. every other partition entry, and this one's start, type, UUID, name
#      and attributes, are compared with before.
#
# Sets s_code and s_why on failure.
ntfs_shrink() {
    local disk="$1" number="$2" part="$3" new="$4" sector before after changed start sectors now
    s_code="" s_why=""
    before="$(sfdisk --dump "${disk}" 2> /dev/null)"
    sector="$(blockdev --getss "${disk}" 2> /dev/null)"
    sector="${sector:-512}"
    mark "shrink step=dry-run partition=${part} new=${new}"
    if ! ntfsresize --no-action --no-progress-bar --size "${new}" "${part}" > /dev/null 2>&1; then
        s_code=TUR-INSTALL-SHRINK-006 s_why="The trial run of the resize failed. Nothing was changed."
        mark "shrink result=refused code=${s_code}"
        return 1
    fi
    mark "shrink step=resize partition=${part} new=${new}"
    if ! printf 'y\n' | ntfsresize --no-progress-bar --size "${new}" "${part}" > /dev/null 2>&1; then
        s_code=TUR-INSTALL-SHRINK-007 s_why="ntfsresize failed while resizing. Do not use Windows' partition before Windows has checked it."
        mark "shrink result=failed code=${s_code}"
        return 1
    fi
    mark "shrink step=resized partition=${part}"
    # The partition: whole MiB, never smaller than the filesystem.
    sectors=$((((new + NTFS_ALIGN_BYTES - 1) / NTFS_ALIGN_BYTES * NTFS_ALIGN_BYTES) / sector))
    mark "shrink step=partition number=${number} sectors=${sectors}"
    if ! printf ', %s\n' "${sectors}" | sfdisk -N "${number}" --wipe never --no-reread "${disk}" > /dev/null 2>&1; then
        s_code=TUR-INSTALL-SHRINK-007 s_why="The partition entry could not be made smaller. The filesystem was already shrunk; Windows still starts."
        mark "shrink result=failed code=${s_code}"
        return 1
    fi
    partx -u "${disk}" > /dev/null 2>&1 || blockdev --rereadpt "${disk}" > /dev/null 2>&1
    after="$(sfdisk --dump "${disk}" 2> /dev/null)"
    # Every other entry as it was; this one: only its size differs.
    changed="$(diff <(printf '%s\n' "${before}" | grep -v "^${part} :") <(printf '%s\n' "${after}" | grep -v "^${part} :"))"
    start="$(printf '%s\n' "${before}" | sed -n "s|^${part} : start= *\([0-9]*\),.*|\1|p")"
    now="$(printf '%s\n' "${after}" | sed -n "s|^${part} : start= *\([0-9]*\), size= *\([0-9]*\),.*|\1 \2|p")"
    if [ -n "${changed}" ] || [ "${now}" != "${start} ${sectors}" ] \
        || [ "$(printf '%s\n' "${before}" | grep "^${part} :" | sed 's/size= *[0-9]*,//')" \
            != "$(printf '%s\n' "${after}" | grep "^${part} :" | sed 's/size= *[0-9]*,//')" ]; then
        s_code=TUR-INSTALL-SHRINK-008 s_why="The partition table is not as expected after the resize."
        mark "shrink result=failed code=${s_code} now=\"${now}\" expected=\"${start} ${sectors}\""
        return 1
    fi
    mark "shrink result=done partition=${part} bytes=${new} sectors=${sectors}"
    return 0
}

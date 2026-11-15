#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Wine in the TUR image (image/README.md, "Wine"): Fedora's packages pinned in
# wine.lock, written by lock.py.
#
#   wine.sh fetch DIR        with network, in the stage "wine": download every
#                            package of wine.lock into DIR, each checked
#                            against its SHA-256. Nothing is installed.
#   wine.sh install DIR ID   without network, in the final stage: check each
#                            package again, then install them all with rpm in
#                            one transaction, at the time of the build ID's
#                            date (as image/branding/branding.sh does), and
#                            check what was installed.
#
# rpm resolves every dependency against the base and the lock: a package the
# base no longer provides, or a lock that misses one, stops the build. File
# triggers are not run (as for the fonts of the desktop): fontconfig's and
# others' would write caches with the time of the build in them. Wine's own
# scriptlets run; they make /usr/bin/wine and /usr/bin/wineserver, and the
# Direct3D DLLs, links managed by `alternatives` (to wine64, wineserver64 and
# Wine's own DLLs).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
lock="${here}/wine.lock"
tab="$(printf '\t')"
fedora=https://dl.fedoraproject.org/pub/fedora/linux
mirror=https://download.fedoraproject.org/pub/fedora/linux
koji=https://kojipkgs.fedoraproject.org/packages

entries() { grep -v '^#' "${lock}" | grep -v '^[[:space:]]*$'; }

fetch() {
    dir="$1"
    mkdir -p "${dir}"
    count=0
    entries > /tmp/tur-wine.lock
    while IFS="${tab}" read -r file sha256 size licence path kojipath why; do
        got=""
        for url in "${fedora}/${path}" "${mirror}/${path}" "${koji}/${kojipath}"; do
            if curl --fail --silent --show-error --location --retry 3 --max-time 900 \
                    -o "${dir}/${file}" "${url}" 2> /tmp/tur-wine-curl.err \
                && echo "${sha256}  ${dir}/${file}" | sha256sum --check --quiet - 2> /dev/null; then
                got="${url}"
                break
            fi
        done
        if [ -z "${got}" ]; then
            echo "wine: could not get ${file} with SHA-256 ${sha256} (${size} bytes; ${why}): $(cat /tmp/tur-wine-curl.err)" >&2
            exit 1
        fi
        echo "wine: ${file} (${licence}) from ${got}"
        count=$((count + 1))
    done < /tmp/tur-wine.lock
    rm -f /tmp/tur-wine.lock /tmp/tur-wine-curl.err
    echo "wine: ${count} packages, $(du -sb "${dir}" | cut -f1) bytes"
}

install_packages() {
    dir="$1"
    build_id="$2"
    SOURCE_DATE_EPOCH="$(date -u -d "$(printf '%s' "${build_id}" | cut -c1-8)" +%s)"
    export SOURCE_DATE_EPOCH
    files=""
    entries > /tmp/tur-wine.lock
    # shellcheck disable=SC2034  # every field is named; not all are used here
    while IFS="${tab}" read -r file sha256 size licence path kojipath why; do
        echo "${sha256}  ${dir}/${file}" | sha256sum --check --quiet -
        tagged="$(rpm -qp --nosignature --qf '%{LICENSE}' "${dir}/${file}")"
        if [ "${tagged}" != "${licence}" ]; then
            echo "wine: ${file}: the package says its licence is \"${tagged}\", wine.lock says \"${licence}\"" >&2
            exit 1
        fi
        files="${files} ${dir}/${file}"
    done < /tmp/tur-wine.lock
    rm -f /tmp/tur-wine.lock
    before="$(df --output=used -B1 / | tail -n 1)"
    # shellcheck disable=SC2086  # a list of files, split on purpose
    rpm -i --nosignature --notriggers ${files}
    after="$(df --output=used -B1 / | tail -n 1)"
    echo "wine: installed $(echo "${files}" | wc -w) packages; the file system grew by $((after - before)) bytes"

    # What a person and tur-runtime use, and what must not be there.
    wine --version
    test "$(readlink -f /usr/bin/wine)" = /usr/bin/wine64
    test "$(readlink -f /usr/bin/wineserver)" = /usr/bin/wineserver64
    test -f /usr/lib64/wine-wow64/wine/i386-windows/kernel32.dll
    test -f /usr/lib64/wine-wow64/wine/x86_64-windows/kernel32.dll
    test -x /usr/bin/xwininfo
    for name in wine-desktop wine-systemd wine-mono mingw32-wine-gecko mingw64-wine-gecko wine-core.i686; do
        if rpm -q "${name}" > /dev/null 2>&1; then
            echo "wine: ${name} is installed and must not be" >&2
            exit 1
        fi
    done
    if ls /usr/lib/binfmt.d/*wine* /usr/share/applications/*wine* > /dev/null 2>&1; then
        echo "wine: Wine registered itself to start Windows programs outside tur-runtime" >&2
        exit 1
    fi
    du -sb /usr/lib64/wine-wow64 /usr/share/wine
}

case "${1:-}" in
    fetch) fetch "${2:?directory required}" ;;
    install) install_packages "${2:?directory required}" "${3:?build ID required}" ;;
    *)
        echo "usage: wine.sh fetch DIR | install DIR BUILD_ID" >&2
        exit 2
        ;;
esac

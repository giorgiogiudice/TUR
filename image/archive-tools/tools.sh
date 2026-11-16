#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Archive tools in the TUR image (image/README.md, "Archive tools for
# re-hosting"): Fedora's innoextract and msitools, pinned in tools.lock,
# written by lock.py. tur-rehost runs them, always inside its tool sandbox
# without network, to open Inno Setup installers and Windows Installer
# packages without running them. 7-Zip (`7zip`, the program `7zz`) is in the
# base image already.
#
#   tools.sh fetch DIR        with network, in the stage "archive-tools":
#                             download every package of tools.lock into DIR,
#                             each checked against its SHA-256. Nothing is
#                             installed.
#   tools.sh install DIR ID   without network, in the final stage: check each
#                             package again and its licence tag against the
#                             lock, install them all with rpm in one
#                             transaction, at the time of the build ID's
#                             date, and check what was installed.
#
# The same mechanism as image/wine/wine.sh of pull request 136 (Wine in the
# image), which had not merged when this was written.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
lock="${here}/tools.lock"
tab="$(printf '\t')"
fedora=https://dl.fedoraproject.org/pub/fedora/linux
mirror=https://download.fedoraproject.org/pub/fedora/linux
koji=https://kojipkgs.fedoraproject.org/packages

entries() { grep -v '^#' "${lock}" | grep -v '^[[:space:]]*$'; }

fetch() {
    dir="$1"
    mkdir -p "${dir}"
    count=0
    entries > /tmp/tur-archive-tools.lock
    # shellcheck disable=SC2034  # every field is named; not all are used here
    while IFS="${tab}" read -r file sha256 size licence path kojipath why; do
        got=""
        for url in "${fedora}/${path}" "${mirror}/${path}" "${koji}/${kojipath}"; do
            if curl --fail --silent --show-error --location --proto =https --retry 3 --max-time 900 \
                    -o "${dir}/${file}" "${url}" 2> /tmp/tur-archive-tools-curl.err \
                && echo "${sha256}  ${dir}/${file}" | sha256sum --check --quiet - 2> /dev/null; then
                got="${url}"
                break
            fi
        done
        if [ -z "${got}" ]; then
            echo "archive-tools: could not get ${file} with SHA-256 ${sha256} (${size} bytes; ${why}): $(cat /tmp/tur-archive-tools-curl.err)" >&2
            exit 1
        fi
        echo "archive-tools: ${file} (${licence}) from ${got}"
        count=$((count + 1))
    done < /tmp/tur-archive-tools.lock
    rm -f /tmp/tur-archive-tools.lock /tmp/tur-archive-tools-curl.err
    echo "archive-tools: ${count} packages, $(du -sb "${dir}" | cut -f1) bytes"
}

install_packages() {
    dir="$1"
    build_id="$2"
    SOURCE_DATE_EPOCH="$(date -u -d "$(printf '%s' "${build_id}" | cut -c1-8)" +%s)"
    export SOURCE_DATE_EPOCH
    files=""
    entries > /tmp/tur-archive-tools.lock
    # shellcheck disable=SC2034  # every field is named; not all are used here
    while IFS="${tab}" read -r file sha256 size licence path kojipath why; do
        echo "${sha256}  ${dir}/${file}" | sha256sum --check --quiet -
        tagged="$(rpm -qp --nosignature --qf '%{LICENSE}' "${dir}/${file}")"
        if [ "${tagged}" != "${licence}" ]; then
            echo "archive-tools: ${file}: the package says its licence is \"${tagged}\", tools.lock says \"${licence}\"" >&2
            exit 1
        fi
        files="${files} ${dir}/${file}"
    done < /tmp/tur-archive-tools.lock
    rm -f /tmp/tur-archive-tools.lock
    # shellcheck disable=SC2086  # a list of files, split on purpose
    rpm -i --nosignature --notriggers ${files}
    echo "archive-tools: installed $(echo "${files}" | wc -w) packages"

    # What tur-rehost starts.
    innoextract --version | head -n 1
    msiextract --version
    command -v 7zz
    # msitools also brings wixl (building MSI packages); nothing registers
    # any of these to open files by itself.
    if ls /usr/share/applications/*innoextract* /usr/share/applications/*msi* > /dev/null 2>&1; then
        echo "archive-tools: a desktop entry registers an archive tool to open files" >&2
        exit 1
    fi
}

case "${1:-}" in
    fetch) fetch "${2:?directory required}" ;;
    install) install_packages "${2:?directory required}" "${3:?build ID required}" ;;
    *)
        echo "usage: tools.sh fetch DIR | install DIR BUILD_ID" >&2
        exit 2
        ;;
esac

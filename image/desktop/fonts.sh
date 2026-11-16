#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Installs the font packages of fonts.lock into the image. Runs in the final
# stage of image/Containerfile, without network, on the packages the stage
# "desktop" downloaded (image/desktop/build.sh fetch).
#
#   fonts.sh RPM_DIR BUILD_ID
#
# Each package is checked again against its SHA-256 and against the licence
# the lock file names, then installed with rpm, so that the package database,
# the SBOM and /usr/share/licenses know it. The time rpm records is the build
# ID's date, as image/branding/branding.sh has it, so that two builds agree.
# Fontconfig's file trigger is not run: it would write the system's font
# cache with the time of this build in it. The cache of /usr/share/fonts is
# then older than the directory, and fontconfig reads the new fonts from
# their files (the smoke test asks it for both families).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
rpms="${1:?directory of the packages required}"
build_id="${2:?build ID required}"
tab="$(printf '\t')"

SOURCE_DATE_EPOCH="$(date -u -d "$(printf '%s' "${build_id}" | cut -c1-8)" +%s)"
export SOURCE_DATE_EPOCH

files=""
grep -v '^#' "${here}/fonts.lock" > /tmp/tur-fonts.lock
while IFS="${tab}" read -r file sha256 licence path; do
    if [ -z "${file}" ]; then continue; fi
    echo "${sha256}  ${rpms}/${file}" | sha256sum -c -
    tagged="$(rpm -qp --nosignature --qf '%{LICENSE}' "${rpms}/${file}")"
    if [ "${tagged}" != "${licence}" ]; then
        echo "${file}: the package's licence is \"${tagged}\", fonts.lock says \"${licence}\" (${path})" >&2
        exit 1
    fi
    files="${files} ${rpms}/${file}"
done < /tmp/tur-fonts.lock
rm -f /tmp/tur-fonts.lock

# shellcheck disable=SC2086  # a list of files, split on purpose
rpm -i --nosignature --notriggers ${files}
rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\t%{LICENSE}\n' rsms-inter-fonts jetbrains-mono-fonts
rpm -ql rsms-inter-fonts jetbrains-mono-fonts | grep -E '\.(ttf|otf|ttc)$'

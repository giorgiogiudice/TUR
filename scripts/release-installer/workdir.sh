#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Choose the job's work directory for the medium (about 7 GB): /mnt when it
# has more room than $RUNNER_TEMP, as the iso workflow does. Writes WORK to
# $GITHUB_ENV.
set -euo pipefail

work="${RUNNER_TEMP:?}/tur-release"
here="$(df --output=avail -B1G "${RUNNER_TEMP}" | tail -1 | tr -d ' G')"
if [ -d /mnt ]; then
    there="$(df --output=avail -B1G /mnt | tail -1 | tr -d ' G')"
    if [ "${there}" -gt "${here}" ]; then
        sudo mkdir -p /mnt/tur-release
        sudo chown "$(id -u):$(id -g)" /mnt/tur-release
        work=/mnt/tur-release
    fi
fi
mkdir -p "${work}"
echo "work directory: ${work} ($(df --output=avail -h "${work}" | tail -1 | xargs) free)"
echo "WORK=${work}" >> "${GITHUB_ENV:?}"

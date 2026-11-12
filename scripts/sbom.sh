#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Generate an SPDX SBOM and a licence summary for a built TUR image.
#
#   scripts/sbom.sh IMAGE OUTPUT_DIR
#
# The image content is RPM packages plus a few TUR files, so the SBOM is built
# from the image's package database rather than by unpacking every layer.
set -euo pipefail

image="${1:?image reference required}"
out="${2:?output directory required}"
read -r -a podman <<<"${PODMAN:-podman}"

SYFT_VERSION=1.57.1
SYFT_SHA256=bf52519eb57f675d47c07d43213cb6dff6449f3d6d1147ff6701ed91f44eef0c

mkdir -p "${out}"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

curl -fsSL -o "${work}/syft.tar.gz" \
    "https://github.com/anchore/syft/releases/download/v${SYFT_VERSION}/syft_${SYFT_VERSION}_linux_amd64.tar.gz"
echo "${SYFT_SHA256}  ${work}/syft.tar.gz" | sha256sum -c -
tar -xzf "${work}/syft.tar.gz" -C "${work}" syft

# Copy the package database and OS identity out of the image.
mkdir -p "${work}/root"
# shellcheck disable=SC2016  # the script below runs inside the container
"${podman[@]}" run --rm --network none "${image}" /bin/bash -c '
    set -e
    db=/usr/lib/sysimage/rpm
    [ -e "$db/rpmdb.sqlite" ] || db=/usr/share/rpm
    [ -e "$db/rpmdb.sqlite" ] || { echo "no rpm database found" >&2; exit 1; }
    tar -C / -chf - "${db#/}" usr/lib/os-release
' | tar -xf - -C "${work}/root"
mkdir -p "${work}/root/etc"
cp "${work}/root/usr/lib/os-release" "${work}/root/etc/os-release"

"${work}/syft" scan "dir:${work}/root" \
    --source-name tur --source-version "$(cat "$(dirname "$0")/../VERSION")" \
    -o "spdx-json=${out}/sbom.spdx.json" -q

# Licence summary: one line per distinct licence expression, with a count.
"${podman[@]}" run --rm --network none "${image}" \
    rpm -qa --qf '%{LICENSE}\n' | sort | uniq -c | sort -rn > "${out}/licences.txt"

packages=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["packages"]))' "${out}/sbom.spdx.json")
echo "SBOM packages: ${packages}"
echo "Distinct licence expressions: $(wc -l < "${out}/licences.txt")"
test "${packages}" -gt 500

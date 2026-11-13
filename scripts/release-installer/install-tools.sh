#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Install the two tools the installer release uses, each pinned by version and
# SHA-256 in the workflow's environment:
#
#   cosign  COSIGN_VERSION, COSIGN_SHA256  (Apache-2.0; the same release the
#           image publishing pins)
#   rclone  RCLONE_VERSION, RCLONE_SHA256  (MIT; the linux-amd64 zip)
#
# A download whose SHA-256 differs stops the job.
set -euo pipefail

tmp="$(mktemp -d)"
curl -sSfL -o "${tmp}/cosign" \
    "https://github.com/sigstore/cosign/releases/download/${COSIGN_VERSION:?}/cosign-linux-amd64"
echo "${COSIGN_SHA256:?}  ${tmp}/cosign" | sha256sum -c -
sudo install -m 0755 "${tmp}/cosign" /usr/local/bin/cosign

zip="rclone-${RCLONE_VERSION:?}-linux-amd64"
curl -sSfL -o "${tmp}/rclone.zip" "https://github.com/rclone/rclone/releases/download/${RCLONE_VERSION}/${zip}.zip"
echo "${RCLONE_SHA256:?}  ${tmp}/rclone.zip" | sha256sum -c -
unzip -q -j "${tmp}/rclone.zip" "${zip}/rclone" -d "${tmp}"
sudo install -m 0755 "${tmp}/rclone" /usr/local/bin/rclone
rm -rf "${tmp}"

cosign version 2>&1 | grep GitVersion
rclone version | sed -n 1p

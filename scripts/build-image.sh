#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#
# Build the UOS image from the pinned base in image/base.lock.
#
#   scripts/build-image.sh [TAG] [extra podman-build arguments...]
#
# PODMAN may name the container tool, for example PODMAN="sudo podman".
# The build is deterministic for a given commit: the layer timestamp is the
# commit time and the build ID is derived from the commit.
#
# Run this in CI or on a dedicated build machine, never on a shared server.
set -euo pipefail

cd "$(dirname "$0")/.."

read -r -a podman <<<"${PODMAN:-podman}"
tag="${1:-localhost/uos:dev}"
if [ "$#" -gt 0 ]; then shift; fi

# shellcheck source=../image/base.lock
. image/base.lock

uos_version="$(cat VERSION)"
revision="$(git rev-parse HEAD)"
epoch="$(git show -s --format=%ct HEAD)"
dirty=""
if ! git diff --quiet HEAD --; then dirty=".dirty"; fi
build_id="${UOS_BUILD_ID:-$(date -u -d "@${epoch}" +%Y%m%d).g${revision:0:12}${dirty}}"

echo "base:      ${BASE_IMAGE}@${BASE_DIGEST} (${BASE_VERSION})"
echo "version:   ${uos_version}"
echo "build id:  ${build_id}"
echo "revision:  ${revision}"

"${podman[@]}" build \
    --file image/Containerfile \
    --tag "${tag}" \
    --timestamp "${epoch}" \
    --build-arg "BASE_IMAGE=${BASE_IMAGE}" \
    --build-arg "BASE_DIGEST=${BASE_DIGEST}" \
    --build-arg "BASE_VERSION=${BASE_VERSION}" \
    --build-arg "UOS_VERSION=${uos_version}" \
    --build-arg "UOS_BUILD_ID=${build_id}" \
    --build-arg "UOS_SOURCE_REVISION=${revision}" \
    "$@" \
    .

echo "${build_id}" > "${UOS_BUILD_ID_FILE:-/dev/null}"

#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Load the builder image that the `builder` job put in the Actions cache.
#
#   load-builder.sh DIR     reads DIR/image.tar.zst
#
# The cache can lose the image between the builder job and this one: it is
# evicted when the repository's caches pass their limit, and jobs can wait
# hours for a runner. The image is then taken from TUR's registry: the one
# builder.lock pins, or the one a run pushed for these inputs
# (fetch-builder.sh --ref). BUILDER_FROM_REGISTRY=no forbids that, for a
# rebuild by hand, whose image is in the cache only.
#
# Runs on the CI runner, never on a development machine (AGENTS.md).
# GHCR_TOKEN and GHCR_USER, when set, log docker in to ghcr.io first.
set -eu

dir=${1:?usage: load-builder.sh DIR}
here=$(dirname "$0")
image=${BUILDER:-localhost/tur-desktop-builder:ci}

start=$(date +%s)
if [ -f "$dir/image.tar.zst" ]; then
    zstd -d -c "$dir/image.tar.zst" | docker load
    echo "loaded $(du -h "$dir/image.tar.zst" | cut -f1) in $(($(date +%s) - start)) s"
else
    if [ "${BUILDER_FROM_REGISTRY:-yes}" != yes ]; then
        echo "::error title=Builder image::the Actions cache no longer has this run's rebuilt image; start the run again"
        exit 1
    fi
    if [ -n "${GHCR_TOKEN:-}" ]; then
        printf '%s' "$GHCR_TOKEN" | docker login ghcr.io --username "${GHCR_USER:-token}" --password-stdin > /dev/null
    fi
    if ! ref=$("$here/fetch-builder.sh" --ref); then
        echo "::error title=Builder image::the Actions cache no longer has the builder image, and TUR's registry has none for these inputs"
        exit 1
    fi
    echo "::notice title=Builder image::the Actions cache no longer has the image; taking ${ref} from TUR's registry"
    docker pull --quiet "$ref"
    docker tag "$ref" "$image"
    echo "pulled in $(($(date +%s) - start)) s"
fi
docker image inspect -f 'image: {{.Id}} created {{.Created}}' "$image"

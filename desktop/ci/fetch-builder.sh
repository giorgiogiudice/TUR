#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Take the builder image pinned in builder.lock (BUILDER_IMAGE@BUILDER_DIGEST)
# from TUR's registry and save it for the Actions cache, as build-builder.sh
# would have.
#
#   fetch-builder.sh DIR       writes DIR/image.tar.zst and DIR/packages.tsv
#   fetch-builder.sh --check   only say whether the pin is usable
#
# The pin is usable when BUILDER_DIGEST is set, the registry has it, and the
# image's label org.tur.builder.inputs equals builder-inputs.sh: it was built
# from this Containerfile, this base and this BUILDER_REVISION. Exits 3 when
# it is not (the image must then be built and pinned), 0 when it is.
#
# Runs on the CI runner, never on a development machine (AGENTS.md). Needs
# docker logged in to ghcr.io when the package is not public.
set -eu

here=$(dirname "$0")
image=${BUILDER:-localhost/tur-desktop-builder:ci}

# shellcheck source=desktop/ci/builder.lock
. "$here/builder.lock"
want=$("$here/builder-inputs.sh")

if [ -z "${BUILDER_DIGEST:-}" ]; then
    echo "builder.lock pins no builder image (BUILDER_DIGEST is empty); inputs ${want}"
    exit 3
fi
ref="${BUILDER_IMAGE}@${BUILDER_DIGEST}"
echo "pinned: ${ref}"
# skopeo reads docker's credentials too (containers-auth.json(5)).
if ! got=$(skopeo inspect --retry-times 3 --format '{{index .Labels "org.tur.builder.inputs"}}' "docker://${ref}" 2>&1); then
    echo "::warning title=Builder image::${ref} cannot be read: ${got}"
    exit 3
fi
if [ "$got" != "$want" ]; then
    echo "::warning title=Builder image::${ref} was built from inputs ${got:-unknown}; builder.lock and the Containerfile now make ${want}. Build it and pin the new digest (desktop/README.md, \"The builder image\")."
    exit 3
fi
echo "inputs: ${want}, as the Containerfile and builder.lock make them"
[ "${1:-}" = --check ] && exit 0

dir=${1:?usage: fetch-builder.sh DIR | --check}
start=$(date +%s)
docker pull --quiet "$ref"
docker tag "$ref" "$image"
mkdir -p "$dir"
docker run --rm "$image" cat /usr/share/tur-builder/packages.tsv > "$dir/packages.tsv"
docker save "$image" | zstd -T0 -3 -q -o "$dir/image.tar.zst"
echo "image: $(docker image inspect -f '{{.Id}} {{.Size}}' "$image") bytes unpacked, pulled in $(($(date +%s) - start)) s"
echo "saved: $(du -h "$dir/image.tar.zst" | cut -f1) compressed, $(wc -l < "$dir/packages.tsv") packages"

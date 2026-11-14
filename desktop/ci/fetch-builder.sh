#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Take the builder image from TUR's registry: the one builder.lock pins
# (BUILDER_IMAGE@BUILDER_DIGEST) or, when that pin is empty or was made from
# other inputs, the one an earlier run built from these inputs and pushed
# (BUILDER_IMAGE:inputs-<inputs>, push-builder.sh).
#
#   fetch-builder.sh DIR       writes DIR/image.tar.zst and DIR/packages.tsv,
#                              as build-builder.sh would have
#   fetch-builder.sh --ref     prints the image's reference by digest
#   fetch-builder.sh --check   says whether the pin itself is usable
#
# An image is usable when the registry has it and its label
# org.tur.builder.inputs equals builder-inputs.sh: it was built from this
# Containerfile, this base and this BUILDER_REVISION. Exits 3 when there is
# none (the image must then be built), 0 otherwise. Only the reference goes
# to standard output; what was found is said on standard error.
#
# Runs on the CI runner, never on a development machine (AGENTS.md). Needs
# docker logged in to ghcr.io when the package is not public; skopeo reads
# docker's credentials too (containers-auth.json(5)).
set -eu

here=$(dirname "$0")
image=${BUILDER:-localhost/tur-desktop-builder:ci}

# shellcheck source=desktop/ci/builder.lock
. "$here/builder.lock"
want=$("$here/builder-inputs.sh")

# inputs_of REF: the inputs label of an image in the registry; fails when
# the registry does not have it.
inputs_of() {
    skopeo inspect --retry-times 3 --format '{{index .Labels "org.tur.builder.inputs"}}' "docker://$1" 2>/dev/null
}

# The pinned image, when it is usable.
pinned() {
    if [ -z "${BUILDER_DIGEST:-}" ]; then
        echo "builder.lock pins no builder image (BUILDER_DIGEST is empty); inputs ${want}" >&2
        return 1
    fi
    ref="${BUILDER_IMAGE}@${BUILDER_DIGEST}"
    if ! got=$(inputs_of "$ref"); then
        echo "::warning title=Builder image::${ref}, pinned in builder.lock, cannot be read from the registry" >&2
        return 1
    fi
    if [ "$got" != "$want" ]; then
        echo "::warning title=Builder image::${ref} was built from inputs ${got:-unknown}; builder.lock and the Containerfile now make ${want}. Pin the image built from them (desktop/README.md, \"The builder image\")." >&2
        return 1
    fi
    echo "pinned: ${ref} (inputs ${want}, as the Containerfile and builder.lock make them)" >&2
    echo "$ref"
}

# The image an earlier run pushed for these inputs, by its digest.
pushed() {
    tag="${BUILDER_IMAGE}:inputs-${want}"
    digest=$(skopeo inspect --retry-times 3 --format '{{.Digest}}' "docker://${tag}" 2>/dev/null) || {
        echo "the registry has no ${tag}" >&2
        return 1
    }
    ref="${BUILDER_IMAGE}@${digest}"
    got=$(inputs_of "$ref") || return 1
    [ "$got" = "$want" ] || { echo "${tag} has inputs ${got:-unknown}, not ${want}" >&2; return 1; }
    echo "not pinned, but pushed by an earlier run for these inputs: ${ref}" >&2
    echo "$ref"
}

case "${1:-}" in
    --check)
        pinned > /dev/null || exit 3
        exit 0
        ;;
    --ref)
        pinned || pushed || exit 3
        exit 0
        ;;
esac

dir=${1:?usage: fetch-builder.sh DIR | --ref | --check}
ref=$(pinned || pushed) || exit 3
start=$(date +%s)
docker pull --quiet "$ref" >&2
docker tag "$ref" "$image"
mkdir -p "$dir"
docker run --rm "$image" cat /usr/share/tur-builder/packages.tsv > "$dir/packages.tsv"
docker save "$image" | zstd -T0 -3 -q -o "$dir/image.tar.zst"
echo "image: $(docker image inspect -f '{{.Id}} {{.Size}}' "$image") bytes unpacked, pulled in $(($(date +%s) - start)) s"
echo "saved: $(du -h "$dir/image.tar.zst" | cut -f1) compressed, $(wc -l < "$dir/packages.tsv") packages"

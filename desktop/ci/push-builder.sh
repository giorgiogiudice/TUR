#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Keep the builder image that build-builder.sh made in TUR's registry, and
# print the line that pins it in builder.lock.
#
#   push-builder.sh             pushes BUILDER_IMAGE:inputs-<inputs>
#
# Writes digest=sha256:... to $GITHUB_OUTPUT when it is set. Runs on
# the CI runner, never on a development machine (AGENTS.md). Needs docker
# logged in to ghcr.io with packages: write.
set -eu

here=$(dirname "$0")
image=${BUILDER:-localhost/tur-desktop-builder:ci}

# shellcheck source=desktop/ci/builder.lock
. "$here/builder.lock"
inputs=$("$here/builder-inputs.sh")
label=$(docker image inspect -f '{{index .Config.Labels "org.tur.builder.inputs"}}' "$image")
if [ "$label" != "$inputs" ]; then
    echo "::error::${image} was built from inputs ${label:-unknown}, not ${inputs}"
    exit 1
fi

tag="${BUILDER_IMAGE}:inputs-${inputs}"
docker tag "$image" "$tag"
docker push --quiet "$tag"
digest=$(docker image inspect -f '{{range .RepoDigests}}{{println .}}{{end}}' "$tag" \
    | sed -n "s|^${BUILDER_IMAGE}@||p" | head -n 1)
case "$digest" in sha256:*) ;; *) echo "::error::no digest for ${tag} after the push"; exit 1 ;; esac
# What the registry now serves under that digest is what was pushed.
got=$(skopeo inspect --retry-times 3 --format '{{index .Labels "org.tur.builder.inputs"}}' "docker://${BUILDER_IMAGE}@${digest}")
[ "$got" = "$inputs" ] || { echo "::error::${BUILDER_IMAGE}@${digest} has inputs ${got}, expected ${inputs}"; exit 1; }

echo "pushed: ${tag} = ${BUILDER_IMAGE}@${digest}"
echo "pin it in desktop/ci/builder.lock:"
echo "  BUILDER_DIGEST=${digest}"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
    echo "digest=${digest}" >> "$GITHUB_OUTPUT"
fi

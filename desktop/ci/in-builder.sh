#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Run a command in the desktop builder image:
#
#   in-builder.sh COMMAND [ARGUMENT...]
#
# The repository is mounted at the path it has on the runner, so that paths in
# logs and in the build directory mean the same inside and outside. The
# command runs as the runner's user, and without a network: a build or a test
# that needs to download something is a mistake.
#
# Runs on the CI runner, never on a development machine (AGENTS.md).
set -eu

image=${BUILDER:-localhost/tur-desktop-builder:ci}
exec docker run --rm --network none \
    --user "$(id -u):$(id -g)" \
    --env HOME=/tmp \
    --env "OUT=${OUT:-out/desktop}" \
    --volume "$PWD:$PWD" --workdir "$PWD" \
    "$image" "$@"

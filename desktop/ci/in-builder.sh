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

# The image does not know the runner's user. A program that asks who it is
# running as needs that user to have a name: dbus-daemon asks for every
# client that connects, and refuses one it cannot look up (the tests of
# tur-settings start a session bus of their own). So the container is given a
# passwd and a group file that name root and this user, and nothing else.
identity=$(mktemp -d)
{
    echo 'root:x:0:0:root:/root:/sbin/nologin'
    if [ "$(id -u)" -ne 0 ]; then
        echo "builder:x:$(id -u):$(id -g):TUR desktop builder:/tmp:/sbin/nologin"
    fi
} > "$identity/passwd"
{
    echo 'root:x:0:'
    if [ "$(id -g)" -ne 0 ]; then
        echo "builder:x:$(id -g):"
    fi
} > "$identity/group"
chmod 755 "$identity"
chmod 644 "$identity/passwd" "$identity/group"

exec docker run --rm --network none \
    --user "$(id -u):$(id -g)" \
    --env HOME=/tmp \
    --env "OUT=${OUT:-out/desktop}" \
    --volume "$identity/passwd:/etc/passwd:ro" \
    --volume "$identity/group:/etc/group:ro" \
    --volume "$PWD:$PWD" --workdir "$PWD" \
    "$image" "$@"

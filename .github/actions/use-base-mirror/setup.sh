#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Part of .github/actions/use-base-mirror (see action.yml there).
#
# Environment: TOKEN (read access to the mirror), ACTOR, MIRROR (for example
# ghcr.io/giorgiogiudice/tur-base), LOCK (image/base.lock).
#
# Writes the containers-registries.conf(5) drop-in
#
#   [[registry]]
#   prefix = "<BASE_IMAGE>"
#   location = "<BASE_IMAGE>"
#   mirror-by-digest-only = true
#   [[registry.mirror]]
#   location = "<MIRROR>"
#
# to /etc/containers/registries.conf.d (read by root and by every user that has
# no registries.conf of its own) and to the runner user's
# ~/.config/containers/registries.conf.d (read by that user in every case).
# A pull by digest then tries the mirror first and the upstream registry
# second; a pull by tag never uses the mirror.
set -euo pipefail

: "${TOKEN:?}" "${ACTOR:?}" "${MIRROR:?}" "${LOCK:?}"

# shellcheck source=/dev/null
. "$LOCK"
: "${BASE_IMAGE:?not set in $LOCK}"

registry="${MIRROR%%/*}"
name=50-tur-base-mirror.conf

# Credentials. podman and skopeo share an auth file: for root
# /run/containers/0/auth.json, for the runner user
# $XDG_RUNTIME_DIR/containers/auth.json. The token is passed on standard input
# and never on a command line.
root_ok=yes
if ! printf '%s' "$TOKEN" | sudo podman login --username "$ACTOR" --password-stdin "$registry" >/dev/null; then
    root_ok=no
    echo "::warning title=Base image mirror::root could not log in to ${registry}; the base comes from ${BASE_IMAGE} only"
fi
if ! printf '%s' "$TOKEN" | podman login --username "$ACTOR" --password-stdin "$registry" >/dev/null; then
    echo "::warning title=Base image mirror::the runner user could not log in to ${registry}; rootless pulls of the base come from ${BASE_IMAGE} only"
fi
if [ "$root_ok" = no ]; then
    # Without credentials the mirror would only answer "unauthorized" before
    # every pull falls back to the upstream registry. Leave the configuration
    # as it is.
    exit 0
fi

conf=$(cat <<EOF
# Written by .github/actions/use-base-mirror (TUR CI). See image/README.md,
# "The base image and its mirror". A pull of ${BASE_IMAGE} by digest tries
# the mirror first, then ${BASE_IMAGE} itself. Pulls by tag do not use it.
[[registry]]
prefix = "${BASE_IMAGE}"
location = "${BASE_IMAGE}"
mirror-by-digest-only = true

[[registry.mirror]]
location = "${MIRROR}"
EOF
)

sudo mkdir -p /etc/containers/registries.conf.d
printf '%s\n' "$conf" | sudo tee "/etc/containers/registries.conf.d/${name}" >/dev/null
user_dir="${XDG_CONFIG_HOME:-$HOME/.config}/containers/registries.conf.d"
mkdir -p "$user_dir"
printf '%s\n' "$conf" > "${user_dir}/${name}"

echo "drop-in /etc/containers/registries.conf.d/${name} and ${user_dir}/${name}:"
printf '%s\n' "$conf" | sed 's/^/  /'

# What podman made of it, as root and as the runner user. A file it cannot
# parse makes these fail, and the step with them.
echo "root:     $(sudo podman info --format '{{json .Registries}}' | python3 -c 'import json,sys; r=json.load(sys.stdin); print(json.dumps({k: v for k, v in r.items() if k != "search"}))')"
echo "rootless: $(podman info --format '{{json .Registries}}' | python3 -c 'import json,sys; r=json.load(sys.stdin); print(json.dumps({k: v for k, v in r.items() if k != "search"}))')"

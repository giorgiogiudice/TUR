#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The application centre end to end (task D-11): the real tur-centre, built
# in the desktop builder, against the real tur-apps and tur-catalogue, built
# on the runner, installing from Flathub with the real flatpak.
#
#   run.sh TUR_APPS TUR_CATALOGUE
#
# Runs on the CI runner (the desktop workflow's job centre-e2e), never on a
# development machine (AGENTS.md). Unlike in-builder.sh, the container has
# the network (Flathub; Fedora's packages for flatpak and an X server) and
# is privileged: bubblewrap, which flatpak runs applications in, needs to
# create namespaces. It is a throw-away runner.
set -eu

tur_apps=$(realpath "${1:?usage: run.sh TUR_APPS TUR_CATALOGUE}")
tur_catalogue=$(realpath "${2:?usage: run.sh TUR_APPS TUR_CATALOGUE}")
image=${BUILDER:-localhost/tur-desktop-builder:ci}
out=$PWD/${OUT:-out/desktop}/centre-e2e
mkdir -p "$out"

exec docker run --rm --privileged --shm-size=1g \
    --env "OUT=$out" \
    --env "TUR_APPS=$tur_apps" \
    --env "TUR_CATALOGUE=$tur_catalogue" \
    --env "HOST_UID=$(id -u)" --env "HOST_GID=$(id -g)" \
    --env "GITHUB_RUN_ID=${GITHUB_RUN_ID:-local}" \
    --volume "$PWD:$PWD" --workdir "$PWD" \
    "$image" sh desktop/ci/centre-e2e/inside.sh

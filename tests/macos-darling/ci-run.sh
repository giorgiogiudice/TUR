#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The macOS route's integration test, on the CI runner (workflow
# macos-darling). Never run on a development machine (AGENTS.md).
#
#     ci-run.sh TUR_RUNTIME DARLING_ARTIFACT ORACLE_ARTIFACT RESULTS
#
# TUR_RUNTIME       the tur-runtime program built by the workflow
# DARLING_ARTIFACT  the darling-runtime artifact: darling-runtime.tar.xz and
#                   build-record.json
# ORACLE_ARTIFACT   the macfw-oracle artifact: apps/*.app built from source on
#                   GitHub's Mac, dmg/*.dmg made of two of them
# RESULTS           results.json, summary.md, screenshots, logs
#
# The test runs in a container of the OS image's pinned base
# (image/base.lock), because TUR's Darling links against that base's
# libraries, as a user who is not root, because tur-runtime never runs
# anything as root. The container is privileged so that bubblewrap can make
# its namespaces inside it; Ubuntu's restriction of unprivileged user
# namespaces is lifted on the runner for the same reason (it applies to the
# runner's own processes, not to anything this test installs).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "${here}/../.." && pwd)"
tur_runtime="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
darling="$(cd "$2" && pwd)"
oracle="$(cd "$3" && pwd)"
results="$(mkdir -p "$4" && cd "$4" && pwd)"
# shellcheck source=image/base.lock
. "${root}/image/base.lock"

sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 || true
sudo sysctl -w kernel.unprivileged_userns_clone=1 2>/dev/null || true

work="${RUNNER_TEMP:-/tmp}/macos-darling-work"
rm -rf "${work}"
mkdir -p "${work}/bin"
cp "${tur_runtime}" "${work}/bin/tur-runtime"
chmod 0755 "${work}/bin/tur-runtime"

sudo podman run --rm --privileged \
    --security-opt label=disable \
    -e APPS="${APPS:-rung1-hello.app,Table Tool.app}" \
    -e MODES="${MODES:-nonroot}" \
    -e STRICT="${STRICT:-0}" \
    -v "${root}:/src:ro" \
    -v "${work}:/work" \
    -v "${darling}:/darling:ro" \
    -v "${oracle}:/oracle:ro" \
    -v "${results}:/results" \
    "${BASE_IMAGE}@${BASE_DIGEST}" \
    /bin/bash /src/tests/macos-darling/in-container.sh
sudo chown -R "$(id -u):$(id -g)" "${results}" || true

#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Darling in the OS image (owner's decision, 16 November 2026: Darling is
# installed by default and kept updated). The image build never compiles
# Darling: it takes TUR's prebuilt archive, published by the darling-runtime
# workflow, by the SHA-256 pinned in image/darling.lock, and unpacks it.
# Runs in the build stage "darling" of image/Containerfile, never on a
# development machine (AGENTS.md).
#
#   darling.sh fetch                  with network: the archive from the
#                                     repository's package registry, by
#                                     digest, checked, then `install`
#   darling.sh install FILE SHA256 ROOT [ID COMMIT]
#                                     unpack a checked archive under ROOT
#                                     (also used by tests/macos-darling)
#
# What the image gets (under /out in the build):
#
#   /usr/lib/tur/darling/             Darling, read-only: bin/darling,
#                                     bin/darlingserver, libexec/darling (the
#                                     guest system every macOS application's
#                                     prefix is layered over), share/licenses
#   /usr/lib/tur/darling.json         which build it is (read by tur-runtime)
#   /usr/share/licenses/tur-darling/  its licence files, SOURCE.txt (where the
#                                     corresponding source is) and PATCHES.txt
#
# With no build pinned yet (DARLING_SHA256 empty), the image carries no
# Darling and says so; tur-runtime then fetches TUR's build on demand when a
# person first installs a macOS application (the fallback).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=image/darling.lock
. "${here}/../darling.lock"

install_archive() {
    file="$1"
    digest="$2"
    root="$3"
    id="${4:-${DARLING_ID}}"
    commit="${5:-${DARLING_COMMIT}}"
    echo "${digest}  ${file}" | sha256sum -c -
    tree="${root}/usr/lib/tur/darling"
    rm -rf "${tree}"
    mkdir -p "${tree}" "${root}/usr/share/licenses"
    # The archive holds opt/tur-darling/...: the path Darling was built for,
    # where tur-runtime shows this tree inside an application's sandbox.
    tar --extract --xz --no-same-owner --strip-components=2 --file "${file}" --directory "${tree}"
    test -f "${tree}/bin/darling"
    test -f "${tree}/bin/darlingserver"
    test -d "${tree}/libexec/darling"
    chmod -R u-s,g-s,go-w "${tree}"
    rm -rf "${root}/usr/share/licenses/tur-darling"
    cp -a "${tree}/share/licenses/darling" "${root}/usr/share/licenses/tur-darling"
    cat > "${root}/usr/lib/tur/darling.json" <<EOF
{
  "schema_version": 1,
  "id": "${id}",
  "darling_commit": "${commit}",
  "queue_sha256": "${DARLING_QUEUE_SHA256}",
  "sha256": "${digest}",
  "source_sha256": "${DARLING_SOURCE_SHA256}",
  "repository": "${DARLING_PACKAGE}",
  "tag": "${DARLING_TAG}"
}
EOF
    echo "Darling ${id} installed at /usr/lib/tur/darling"
}

fetch() {
    mkdir -p /out
    if [ -z "${DARLING_SHA256}" ]; then
        echo "image/darling.lock pins no published Darling build yet: the image carries none"
        return 0
    fi
    work="$(mktemp -d)"
    repository="${DARLING_PACKAGE#ghcr.io/}"
    # An anonymous pull token for the public package; it goes to a file, never
    # onto a command line.
    curl -fsS --proto =https --tlsv1.2 \
        "https://ghcr.io/token?scope=repository:${repository}:pull&service=ghcr.io" \
        | python3 -I -c 'import json, sys; print("Authorization: Bearer " + json.load(sys.stdin)["token"])' \
        > "${work}/header"
    curl -fsSL --proto =https --proto-redir =https --tlsv1.2 --retry 4 \
        --max-filesize "${DARLING_SIZE}" --header "@${work}/header" \
        --output "${work}/darling-runtime.tar.xz" \
        "https://ghcr.io/v2/${repository}/blobs/sha256:${DARLING_SHA256}"
    rm -f "${work}/header"
    test "$(stat -c %s "${work}/darling-runtime.tar.xz")" = "${DARLING_SIZE}"
    install_archive "${work}/darling-runtime.tar.xz" "${DARLING_SHA256}" /out
    rm -rf "${work}"
}

case "${1:-}" in
    fetch) fetch ;;
    install) install_archive "$2" "$3" "$4" "${5:-}" "${6:-}" ;;
    *) echo "usage: $0 fetch | install FILE SHA256 ROOT [ID COMMIT]" >&2; exit 2 ;;
esac

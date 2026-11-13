#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Copy the tested, signed image to where it is published, by digest, with
# its signatures and attestations; then point a channel at it (ADR-0023;
# docs/architecture/publishing.md, sections 3 and 6).
#
#   scripts/release/publish.sh FROM_REPOSITORY DIGEST TO_REPOSITORY BUILD_ID CHANNEL
#
# Nothing is rebuilt or re-signed: the manifest and the layers are copied
# byte for byte (skopeo --preserve-digests refuses any copy that would change
# a digest), the signature tags (sha256-<digest>.sig and .att) with
# `cosign copy --only=sig,att` (cosign 3.1 marks the command deprecated); the
# digest under the build ID, under the channel and of each signature tag is
# read back from the destination and compared.
#
# The build ID tag is immutable: if it exists already, it must name DIGEST
# (a re-run), or the script stops before writing anything. The channel tag
# moves. Authentication for the destination comes from the default auth file
# (skopeo login / docker login). Run in CI only.
set -euo pipefail

from="${1:?source repository required}"
digest="${2:?digest required}"
to="${3:?destination repository required}"
build_id="${4:?build ID required}"
channel="${5:?channel required}"

# A registry on this machine is the test registry of the release workflow,
# plain HTTP; anything else must use TLS. skopeo inspect takes --tls-verify,
# copy takes --src-tls-verify and --dest-tls-verify.
tls() { case "$1" in localhost:* | 127.0.0.1:*) echo "--$2-tls-verify=false" ;; *) echo "--$2-tls-verify=true" ;; esac; }
inspect_tls() { case "$1" in localhost:* | 127.0.0.1:*) echo "--tls-verify=false" ;; *) echo "--tls-verify=true" ;; esac; }
digest_of() {
    local raw
    raw="$(skopeo inspect --raw "$(inspect_tls "$1")" "docker://$1" 2> /dev/null)" || return 1
    printf '%s' "${raw}" | sha256sum | sed 's/^/sha256:/; s/ .*//'
}
copy() { # copy SRC_REF DEST_REF
    skopeo copy --quiet --preserve-digests "$(tls "$1" src)" "$(tls "$2" dest)" "docker://$1" "docker://$2"
}

existing="$(digest_of "${to}:${build_id}" || true)"
if [ -n "${existing}" ] && [ "${existing}" != "${digest}" ]; then
    echo "${to}:${build_id} exists and names ${existing}, not ${digest}: build IDs are immutable" >&2
    exit 1
fi
if [ -z "${existing}" ]; then
    copy "${from}@${digest}" "${to}:${build_id}"
    echo "copied ${from}@${digest} to ${to}:${build_id}"
else
    echo "${to}:${build_id} already names ${digest}"
fi
# The signature and attestation tags are copied with cosign, which copies its
# own artifacts as they are, then read back and compared.
if [ "${from}" != "${to}" ]; then
    http=()
    case "${from} ${to}" in *localhost:* | *127.0.0.1:*) http=(--allow-http-registry) ;; esac
    cosign copy --only=sig,att --force "${http[@]}" "${from}@${digest}" "${to}"
fi
for suffix in sig att; do
    tag="sha256-${digest#sha256:}.${suffix}"
    if got="$(digest_of "${from}:${tag}")"; then
        test "$(digest_of "${to}:${tag}" || true)" = "${got}" \
            || { echo "${to}:${tag} is not ${from}:${tag}" >&2; exit 1; }
        echo "${to}:${tag} -> ${got}"
    fi
done
copy "${to}@${digest}" "${to}:${channel}"

for ref in "${to}:${build_id}" "${to}:${channel}" "${to}@${digest}"; do
    got="$(digest_of "${ref}")"
    if [ "${got}" != "${digest}" ]; then
        echo "${ref} names ${got}, expected ${digest}" >&2
        exit 1
    fi
    echo "${ref} -> ${got}"
done

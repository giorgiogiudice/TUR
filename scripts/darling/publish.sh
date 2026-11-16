#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Publishes a TUR Darling build to the repository's own package registry, in
# CI only (workflows darling-runtime and darling-update).
#
#     publish.sh OUT
#
# OUT is a darling-runtime artifact: darling-runtime.tar.xz,
# darling-source.tar.xz, licences.json, build-record.json. Refuses, before
# anything is pushed, when:
#
#   - a licence text of the build has not been read by a person
#     (scripts/darling/licences-reviewed.txt; owner's decision of 16
#     November 2026);
#   - the build's own check found something TUR does not ship, or a host
#     library the base lacks;
#   - an archive is not the size and SHA-256 its record says.
#
# Then pushes both archives as one OCI artifact with ORAS (pinned and
# checked), checks that the registry holds each under its own SHA-256, and
# writes OUT/manifest-entry.json: the entry for
# services/tur-runtime/darling/runtime.json and the values for
# image/darling.lock.
#
# Needs TOKEN (packages: write) and ACTOR in the environment.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/darling/darling.lock
. "${here}/darling.lock"
out="$(cd "$1" && pwd)"
record="${out}/build-record.json"

echo "== licences read by a person"
python3 -I - "${record}" "${here}/licences-reviewed.txt" <<'EOF'
import json, sys
record = json.load(open(sys.argv[1]))
reviewed = set()
for line in open(sys.argv[2]):
    line = line.strip()
    if line and not line.startswith("#"):
        reviewed.add(line.split()[0])
unread = [digest for digest in record.get("licence_texts", []) if digest not in reviewed]
if not record.get("licence_texts"):
    sys.exit("the build record lists no licence texts")
if unread:
    print(f"{len(unread)} of {len(record['licence_texts'])} licence texts have not been read:")
    for digest in unread[:40]:
        print("  " + digest)
    sys.exit("publishing is refused until they are read (licences.md) and listed in scripts/darling/licences-reviewed.txt")
print(f"all {len(record['licence_texts'])} licence texts have been read")
EOF

echo "== the build's own checks"
test "$(jq '.check.violations | length' "${record}")" = 0
test "$(jq '.check.missing_host_libraries | length' "${record}")" = 0
for name in darling-runtime darling-source; do
    key=sha256; size=size
    [ "${name}" = darling-source ] && key=source_sha256 && size=source_size
    echo "$(jq -r ".${key}" "${record}")  ${out}/${name}.tar.xz" | sha256sum -c -
    test "$(stat -c %s "${out}/${name}.tar.xz")" = "$(jq -r ".${size}" "${record}")"
done

echo "== ORAS ${ORAS_VERSION}"
tools="$(mktemp -d)"
curl -fsSL --retry 4 -o "${tools}/oras.tar.gz" "${ORAS_URL}"
echo "${ORAS_SHA256}  ${tools}/oras.tar.gz" | sha256sum -c -
curl -fsSL --retry 4 -o "${tools}/checksums.txt" "${ORAS_URL%/*}/oras_${ORAS_VERSION}_checksums.txt"
grep "  oras_${ORAS_VERSION}_linux_amd64.tar.gz$" "${tools}/checksums.txt" \
    | sed "s|  oras_.*|  ${tools}/oras.tar.gz|" | sha256sum -c -
tar -xzf "${tools}/oras.tar.gz" -C "${tools}" oras
oras="${tools}/oras"

tag="$(jq -r .tag "${record}")"
printf '%s' "${TOKEN}" | "${oras}" login ghcr.io --username "${ACTOR}" --password-stdin
(
    cd "${out}"
    "${oras}" push "${DARLING_PACKAGE}:${tag}" \
        --artifact-type application/vnd.tur.darling.runtime.v1 \
        --annotation "org.opencontainers.image.source=https://github.com/${GITHUB_REPOSITORY:-giorgiogiudice/TUR}" \
        --annotation "org.opencontainers.image.revision=${GITHUB_SHA:-unknown}" \
        --annotation "org.opencontainers.image.licenses=GPL-3.0-or-later AND MIT AND LGPL-2.1-or-later AND APSL-2.0 AND BSD-2-Clause AND BSD-3-Clause AND Apache-2.0" \
        --annotation "org.tur.darling.commit=$(jq -r .darling_commit "${record}")" \
        "darling-runtime.tar.xz:${DARLING_MEDIA_TYPE}" \
        "darling-source.tar.xz:${DARLING_SOURCE_MEDIA_TYPE}" \
        "licences.json:application/json" \
        "build-record.json:application/json"
)

echo "== the registry holds both archives under their own SHA-256"
"${oras}" manifest fetch "${DARLING_PACKAGE}:${tag}" > "${out}/oci-manifest.json"
jq -e --arg d "sha256:$(jq -r .sha256 "${record}")" --arg t "${DARLING_MEDIA_TYPE}" \
    '[.layers[] | select(.mediaType == $t and .digest == $d)] | length == 1' "${out}/oci-manifest.json"
jq -e --arg d "sha256:$(jq -r .source_sha256 "${record}")" --arg t "${DARLING_SOURCE_MEDIA_TYPE}" \
    '[.layers[] | select(.mediaType == $t and .digest == $d)] | length == 1' "${out}/oci-manifest.json"

jq --arg repo "${DARLING_PACKAGE}" '{
    id, architecture: "x86_64", fedora_release, base_digest, darling_commit, queue_sha256,
    repository: $repo, tag, sha256, size, unpacked_size, source_sha256, source_size,
    install_prefix,
    licence: "GPL-3.0-or-later AND MIT AND LGPL-2.1-or-later AND APSL-2.0 AND BSD-2-Clause AND BSD-3-Clause AND Apache-2.0"
}' "${record}" > "${out}/manifest-entry.json"
cat "${out}/manifest-entry.json"

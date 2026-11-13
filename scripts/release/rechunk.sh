#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Re-chunk the tested image into the image that is published (ADR-0023;
# docs/architecture/publishing.md, section 4).
#
#   scripts/release/rechunk.sh TOOL_IMAGE OCI_ARCHIVE IMAGE_ID WORK OUT_LAYOUT [PREVIOUS]
#
# The image CI builds has the base's layers, with Fedora's logos in them,
# and TUR's layers on top, which delete those files again. Whoever pulls it
# downloads both. `rpm-ostree compose build-chunked-oci` (rpm-ostree 2026.3
# in the image; rpm-ostree docs/build-chunked-oci.md) takes the image's
# merged file system and writes it again as a bootc image whose layers each
# hold a share of the files and no file twice: what TUR's layers removed is
# in no layer. The layers are cut by package, so that an update changes the
# layers of the packages it changes and a computer downloads only those.
#
# TOOL_IMAGE is the tested image itself, already in the host's
# containers-storage: it carries rpm-ostree. The tool runs in a privileged
# container with a container storage of its own under WORK (the host's
# podman is older than the image's container libraries, and neither should
# write the other's store). In it, the tested image is loaded from
# OCI_ARCHIVE and checked against IMAGE_ID before anything is done with it.
#
# --format-version 2 puts every parent directory into every layer, so that
# the directories' metadata does not depend on the order layers are unpacked
# in (the upstream recommendation for reproducible builds); there is no
# earlier published image whose layers version 1 would keep reusable.
# PREVIOUS, when given (`docker://…`), keeps the layer plan of a published
# image (--previous-build), so that a new build changes as few layers as the
# change needs. REGISTRY_AUTH_FILE, when set, is passed to the tool for it.
#
# OUT_LAYOUT receives an OCI layout with the image under the name `latest`,
# with the labels of the tested image, and OUT_LAYOUT.digest its manifest
# digest. Run in CI only.
set -euo pipefail

tool="${1:?tool image required}"
archive="${2:?OCI archive required}"
image_id="${3:?image ID required}"
work="${4:?work directory required}"
out="${5:?output layout required}"
previous="${6:-}"
read -r -a podman <<<"${PODMAN:-sudo podman}"

archive="$(cd "$(dirname "${archive}")" && pwd)/$(basename "${archive}")"
mkdir -p "${work}"
work="$(cd "${work}" && pwd)"
store="${work}/rechunk-storage"
sudo rm -rf "${store}" "${out}"
mkdir -p "${store}" "$(dirname "${out}")"
out="$(cd "$(dirname "${out}")" && pwd)/$(basename "${out}")"

# The labels of the tested image are given to the re-chunked one: the build
# ID, the revision and the base it was built on stay readable from the
# registry.
labels=()
# shellcheck disable=SC2016  # a Go template, for podman
while IFS= read -r label; do
    [ -n "${label}" ] && labels+=(--label "${label}")
done < <("${podman[@]}" image inspect "${tool}" \
    --format '{{range $k, $v := .Config.Labels}}{{printf "%s=%s\n" $k $v}}{{end}}')
epoch="$("${podman[@]}" image inspect "${tool}" --format '{{.Created.Unix}}')"

auth=()
if [ -n "${REGISTRY_AUTH_FILE:-}" ]; then
    auth=(-v "${REGISTRY_AUTH_FILE}:/run/tur-auth.json:ro" -e REGISTRY_AUTH_FILE=/run/tur-auth.json)
fi
plan=()
if [ -n "${previous}" ]; then plan=(--previous-build "${previous}"); fi

echo "tool:      ${tool} (rpm-ostree $("${podman[@]}" run --rm --network none "${tool}" rpm-ostree --version | sed -n "s/^ *Version: //p"))"
echo "input:     ${archive} (image sha256:${image_id})"
echo "labels:    $(( ${#labels[@]} / 2 )) carried over"
echo "plan from: ${previous:-none (first layout)}"
started="$(date +%s)"
# shellcheck disable=SC2016  # expanded by the shell in the container
"${podman[@]}" run --rm --privileged --security-opt label=disable \
    -v "${store}:/var/lib/containers" \
    -v "${archive}:/run/tur-input.oci.tar:ro" \
    -v "$(dirname "${out}"):/run/tur-output" \
    "${auth[@]}" \
    -e "SOURCE_DATE_EPOCH=${epoch}" \
    -e "TUR_IMAGE_ID=${image_id}" \
    -e "TUR_OUT=$(basename "${out}")" \
    "${tool}" /bin/bash -euo pipefail -c '
        skopeo copy --quiet oci-archive:/run/tur-input.oci.tar containers-storage:localhost/tur:tested
        loaded="$(podman image inspect --format "{{.Id}}" localhost/tur:tested)"
        echo "loaded:    sha256:${loaded}"
        [ "${loaded}" = "${TUR_IMAGE_ID}" ] || { echo "the archive is not the tested image" >&2; exit 1; }
        # The output must be a containers-storage image: rpm-ostree opens
        # the output reference first, to reuse a previous chunking, and fails
        # on an OCI layout that does not exist yet.
        rpm-ostree compose build-chunked-oci --bootc --format-version 2 "$@" \
            --from localhost/tur:tested --output containers-storage:localhost/tur:published
        skopeo copy --quiet containers-storage:localhost/tur:published "oci:/run/tur-output/${TUR_OUT}:latest"
    ' _ "${labels[@]}" "${plan[@]}"
echo "re-chunked in $(( $(date +%s) - started )) s"
sudo rm -rf "${store}"
sudo chown -R "$(id -u):$(id -g)" "${out}"

digest="$(skopeo inspect --raw "oci:${out}:latest" | sha256sum | cut -d' ' -f1)"
echo "sha256:${digest}" > "${out}.digest"
skopeo inspect "oci:${out}:latest" | python3 -c '
import json, sys
i = json.load(sys.stdin)
print("published: " + sys.argv[1])
print("layers:    %d" % len(i["Layers"]))
for key in sorted(i.get("Labels") or {}):
    print("label:     %s=%s" % (key, i["Labels"][key]))
' "sha256:${digest}"

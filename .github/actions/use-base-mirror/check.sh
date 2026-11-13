#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Part of .github/actions/use-base-mirror (see action.yml there).
#
#   check.sh [IMAGE@DIGEST]
#
# Says whether a base image digest (default: the pin in $LOCK) is in the
# mirror ($MIRROR) and in its upstream registry, and whether it resolves the
# way podman will resolve it: through the registries.conf drop-in that
# setup.sh wrote. Exits 1, with an error that says what to do, when it does
# not resolve. Fetches manifests only, no layers.
#
# AS_ROOT=no checks as the runner user (rootless podman's configuration and
# credentials) instead of as root.
set -euo pipefail

: "${MIRROR:?}"
if [ "$#" -gt 0 ]; then
    ref="$1"
else
    : "${LOCK:?}"
    # shellcheck source=/dev/null
    . "$LOCK"
    ref="${BASE_IMAGE}@${BASE_DIGEST}"
fi
image="${ref%@*}"
digest="${ref#*@}"
case "$digest" in sha256:*) ;; *) echo "::error::not a reference by digest: ${ref}"; exit 2 ;; esac

as=()
if [ "${AS_ROOT:-yes}" = yes ]; then as=(sudo); fi

err="$(mktemp)"
trap 'rm -f "$err"' EXIT

# probe LABEL REF: one line saying whether REF's manifest is in the mirror.
probe() {
    local label="$1" what="$2"
    if "${as[@]}" skopeo inspect --raw --retry-times 2 "docker://${what}" 2>"$err" >/dev/null; then
        printf '%-10s %s present\n' "$label" "$what"
        return 0
    fi
    local why
    why="$(grep -v '^$' "$err" | tail -n 1)"
    case "$why" in
        *"manifest unknown"*) why="absent (manifest unknown)" ;;
        *unauthorized*|*denied*|*"authentication required"*) why="not readable with this job's credentials: ${why}" ;;
        *) why="not reachable: ${why}" ;;
    esac
    printf '%-10s %s %s\n' "$label" "$what" "$why"
    return 1
}

echo "base:      ${ref}"
in_mirror=yes; in_upstream=yes
probe "mirror:" "${MIRROR}@${digest}" || in_mirror=no
# The upstream registry, asked directly: skopeo would ask the mirror first.
up="$(python3 -I "$(dirname "$0")/upstream.py" "${image}@${digest}")" || in_upstream=no
printf '%-10s %s %s\n' "upstream:" "${image}@${digest}" "$up"

# As podman will see it: the base reference, with the drop-in in effect.
if got="$("${as[@]}" skopeo inspect --raw --retry-times 2 "docker://${ref}" 2>"$err" | sha256sum | cut -d' ' -f1)" \
        && [ "sha256:${got}" = "${digest}" ]; then
    if [ "$in_mirror" = yes ]; then from="the mirror"
    elif [ "$in_upstream" = yes ]; then from="${image}"
    else from="a source the probes above did not reach"; fi
    echo "resolves:  yes, to ${digest}, from ${from} (the mirror is tried first)"
    if [ "$in_mirror" = no ]; then
        echo "::notice title=Base image not mirrored yet::${ref} comes from ${image}; base-mirror.yml copies it to ${MIRROR} when image/base.lock changes."
    fi
    exit 0
fi

echo "resolves:  no ($(grep -v '^$' "$err" | tail -n 1))"
echo "::error title=Base image unavailable::${ref} is in neither ${MIRROR} nor ${image}. ${image} drops a digest about a day after Fedora moves its tag, and the mirror holds only digests that base-mirror.yml copied while they were still upstream. Pin the current base in image/base.lock (image/README.md, \"The base image and its mirror\")."
exit 1

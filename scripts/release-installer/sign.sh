#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Sign the released medium and its checksum file with cosign (pinned by the
# workflow), and verify every signature back.
#
#   scripts/release-installer/sign.sh DIR BASE
#
# Signs DIR/BASE.iso and DIR/BASE.SHA256SUMS:
#
# 1. Keyless, unless TUR_SIGN_KEYLESS=0: a short-lived Sigstore certificate
#    for this workflow's own identity (GitHub's OIDC token; the job needs
#    `id-token: write`), entered in Sigstore's public Rekor log. Written as
#    <file>.keyless.sigstore.json (a Sigstore bundle: signature, certificate,
#    log entry).
# 2. With TUR's release key, when TUR_RELEASE_KEY holds it (the content of
#    the cosign private key; password in COSIGN_PASSWORD): the same key the
#    image publishing uses (docs/architecture/publishing.md, section 8).
#    Written as <file>.release-key.sigstore.json, and the key's public half as
#    BASE.release-key.pub. If the repository carries the release key's public
#    half (image/rootfs/usr/share/tur/signing/release.pub), the secret must be
#    its private half.
#
# The key is read through cosign's env:// and never written to disk or
# printed. Writes DIR/signing.env.
set -euo pipefail

dir="${1:?directory required}"
base="${2:?base name required}"
issuer=https://token.actions.githubusercontent.com
identity="${TUR_KEYLESS_IDENTITY:-${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_WORKFLOW_REF:-unknown}}"
repo_pub="$(realpath -m "${TUR_REPO_RELEASE_PUB:-image/rootfs/usr/share/tur/signing/release.pub}")"
files=("${base}.iso" "${base}.SHA256SUMS")
cd "${dir}"
for f in "${files[@]}"; do
    [ -f "${f}" ] || { echo "missing ${dir}/${f}" >&2; exit 1; }
done
cosign version 2>&1 | grep GitVersion

keyless=no
if [ "${TUR_SIGN_KEYLESS:-1}" = 1 ]; then
    for f in "${files[@]}"; do
        echo "== keyless: ${f} as ${identity}"
        cosign sign-blob --yes --timeout 30m --bundle "${f}.keyless.sigstore.json" "${f}" > /dev/null
        cosign verify-blob --timeout 30m --bundle "${f}.keyless.sigstore.json" \
            --certificate-identity "${identity}" --certificate-oidc-issuer "${issuer}" "${f}"
    done
    keyless=yes
fi

release_key=no
if [ -n "${TUR_RELEASE_KEY:-}" ]; then
    cosign public-key --key env://TUR_RELEASE_KEY > "${base}.release-key.pub"
    if [ -f "${repo_pub}" ]; then
        cmp -s "${base}.release-key.pub" "${repo_pub}" || {
            echo "the secret TUR_RELEASE_SIGNING_KEY is not the key whose public half is ${repo_pub}" >&2
            exit 1
        }
        echo "the release key is the one in ${repo_pub}"
    else
        echo "note: ${repo_pub} is not in the repository yet; the public half is published beside the medium"
    fi
    for f in "${files[@]}"; do
        echo "== release key: ${f}"
        cosign sign-blob --yes --timeout 30m --key env://TUR_RELEASE_KEY \
            --bundle "${f}.release-key.sigstore.json" "${f}" > /dev/null
        cosign verify-blob --timeout 30m --bundle "${f}.release-key.sigstore.json" \
            --key "${base}.release-key.pub" "${f}"
    done
    release_key=yes
fi

if [ "${keyless}" = no ] && [ "${release_key}" = no ]; then
    echo "nothing was signed" >&2
    exit 1
fi
{
    echo "keyless=${keyless}"
    echo "keyless_identity=${identity}"
    echo "keyless_issuer=${issuer}"
    echo "release_key=${release_key}"
} | tee signing.env

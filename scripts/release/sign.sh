#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Sign the image under release, in the registry it was pushed to for the test
# (ADR-0023; docs/architecture/publishing.md, section 5).
#
#   scripts/release/sign.sh REPOSITORY DIGEST KEY PUBLIC_KEY OUT_DIR
#
# REPOSITORY is where the image is, e.g. localhost:5000/tur; DIGEST its
# manifest digest. Two signatures and two attestations are made, all with
# cosign (pinned by the workflow), all in the format the computer's own
# container libraries read: a "sigstore attachment", the tag
# sha256-<digest>.sig beside the image, holding a simple-signing payload
# (cosign's `--new-bundle-format=false`; containers/image reads no other).
# The payload names ghcr.io/giorgiogiudice/tur as the signed repository
# wherever the image is (cosign's --sign-container-identity), because that
# is the name a TUR computer pulls by and the policy matches.
#
# 1. With TUR's release key (KEY: a file or env://VARIABLE, password in
#    COSIGN_PASSWORD), entered in Sigstore's public Rekor log. This is the
#    signature the computer checks (policy: keyPath + rekorPublicKeyPath).
# 2. Keyless: a short-lived Fulcio certificate for this workflow's own
#    identity (GitHub's OIDC token for
#    https://github.com/<repo>/.github/workflows/release.yml@<ref>), entered
#    in Rekor. For people and tools that verify with cosign; the computer's
#    container libraries cannot match a workflow identity yet.
# 3. Keyless attestations: the build manifest, and the SBOM when Rekor
#    accepts its size (6-7 MB today; it is a workflow artifact either way).
#
# Every signature is verified back with cosign before the script ends. With
# TUR_TEST_VARIANTS="unsigned=sha256:… other-key=sha256:… no-log=sha256:…"
# and TUR_OTHER_KEY, the refusal fixtures of publishing.md, section 7, are
# signed as their names say. Writes OUT_DIR/signing.env.
set -euo pipefail

repo="${1:?repository required}"
digest="${2:?digest required}"
key="${3:?release key required}"
pub="${4:?release public key required}"
out="${5:?output directory required}"
identity_repo="${TUR_IDENTITY_REPO:-ghcr.io/giorgiogiudice/tur}"
issuer=https://token.actions.githubusercontent.com
identity="https://github.com/${GITHUB_WORKFLOW_REF:?run in GitHub Actions}"
mkdir -p "${out}"

http=()
case "${repo}" in localhost:* | 127.0.0.1:*) http=(--allow-http-registry) ;; esac
# The format containers/image reads: cosign deprecates these flags (it moves
# to its bundle format, stored as an OCI referrer, which containers/image
# cannot read yet); each is checked to still work by the verification below.
legacy=(--new-bundle-format=false --use-signing-config=false)
sign=(cosign sign --yes "${http[@]}" "${legacy[@]}" --sign-container-identity "${identity_repo}")

echo "== 1. release key, entered in Rekor: ${repo}@${digest}"
"${sign[@]}" --tlog-upload=true --key "${key}" "${repo}@${digest}"
echo "== 2. keyless, as ${identity}"
"${sign[@]}" "${repo}@${digest}"

echo "== 3. attestations"
attest=(cosign attest --yes "${http[@]}" "${legacy[@]}")
manifest_attested=no sbom_attested=no
if [ -n "${TUR_BUILD_MANIFEST:-}" ]; then
    "${attest[@]}" --type https://github.com/giorgiogiudice/TUR/blob/main/schemas/build-manifest.schema.json \
        --predicate "${TUR_BUILD_MANIFEST}" "${repo}@${digest}"
    manifest_attested=yes
fi
if [ -n "${TUR_SBOM:-}" ]; then
    if "${attest[@]}" --type spdxjson --predicate "${TUR_SBOM}" "${repo}@${digest}" 2> "${out}/sbom-attest.err"; then
        sbom_attested=yes
    else
        sbom_attested="no ($(tail -n 1 "${out}/sbom-attest.err" | cut -c1-200))"
        echo "the SBOM ($(stat -c %s "${TUR_SBOM}") bytes) was not attested: ${sbom_attested}"
    fi
fi

echo "== verify"
cosign verify "${http[@]}" --key "${pub}" "${repo}@${digest}" > "${out}/verify-release-key.json"
cosign verify "${http[@]}" --certificate-identity "${identity}" --certificate-oidc-issuer "${issuer}" \
    "${repo}@${digest}" > "${out}/verify-keyless.json"
if [ "${manifest_attested}" = yes ]; then
    cosign verify-attestation "${http[@]}" --certificate-identity "${identity}" --certificate-oidc-issuer "${issuer}" \
        --type https://github.com/giorgiogiudice/TUR/blob/main/schemas/build-manifest.schema.json \
        "${repo}@${digest}" > /dev/null
fi
python3 - "${out}/verify-release-key.json" "${out}/verify-keyless.json" "${identity_repo}" "${digest}" <<'PY'
import json, sys
for path in sys.argv[1:3]:
    for entry in json.load(open(path)):
        critical = entry["critical"]
        assert critical["identity"]["docker-reference"] == sys.argv[3], critical
        assert critical["image"]["docker-manifest-digest"] == sys.argv[4], critical
        assert entry["optional"].get("Bundle"), "no Rekor entry"
print("both signatures name", sys.argv[3], "and", sys.argv[4], "and have a Rekor entry")
PY

if [ -n "${TUR_TEST_VARIANTS:-}" ]; then
    echo "== refusal fixtures"
    for pair in ${TUR_TEST_VARIANTS}; do
        name="${pair%%=*}" variant="${pair#*=}"
        case "${name}" in
            unsigned) echo "${name}: ${variant} not signed" ;;
            other-key)
                COSIGN_PASSWORD="" "${sign[@]}" --tlog-upload=true --key "${TUR_OTHER_KEY:?}" "${repo}@${variant}"
                echo "${name}: ${variant} signed with another key, entered in Rekor" ;;
            no-log)
                "${sign[@]}" --tlog-upload=false --key "${key}" "${repo}@${variant}"
                echo "${name}: ${variant} signed with the release key, not entered in Rekor" ;;
            *) echo "unknown fixture ${name}" >&2; exit 2 ;;
        esac
    done
fi

{
    echo "keyless_identity=${identity}"
    echo "keyless_issuer=${issuer}"
    echo "signed_repository=${identity_repo}"
    echo "manifest_attested=${manifest_attested}"
    echo "sbom_attested=${sbom_attested}"
} | tee "${out}/signing.env"

#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Temporary measurement for ADR-0023 (removed before the change is merged):
# what the pinned base ships for signature verification, which flags cosign
# has, and whether the base's own skopeo accepts and refuses the signatures
# cosign makes. CI only.
set -uo pipefail
cd "$(dirname "$0")/../.."
. image/base.lock
base="${BASE_IMAGE}@${BASE_DIGEST}"
P="sudo podman"
work="$RUNNER_TEMP/probe"
mkdir -p "$work"

echo "::group::base image facts"
$P pull -q "$base" > /dev/null
$P run --rm --network none "$base" /bin/bash -c '
rpm -q containers-common containers-common-extra skopeo podman bootc rpm-ostree ostree python3 jq cosign 2>&1
skopeo --version; bootc --version; rpm-ostree --version | head -3
echo "--- /etc/containers"; ls -la /etc/containers /etc/containers/registries.d /etc/containers/registries.conf.d 2>&1
echo "--- /usr/share/containers"; ls -la /usr/share/containers /usr/share/containers/registries.d 2>&1
echo "--- /etc/pki/containers"; ls -la /etc/pki/containers /usr/share/pki/containers 2>&1
for f in /etc/containers/policy.json /usr/share/containers/policy.json /etc/containers/registries.d/*.yaml /usr/share/containers/registries.d/*.yaml; do echo "### $f"; cat "$f" 2>&1; done
echo "--- owners"; rpm -qf /etc/containers/policy.json /etc/containers/registries.d/default.yaml /usr/share/containers/policy.json 2>&1
rpm -ql containers-common | grep -E "policy|registries"
echo "--- man lookup"; for m in /usr/share/man/man5/containers-policy.json.5*; do zcat -f "$m" | grep -n -i -B2 -A8 "usr/share/containers" | head -40; done
for m in /usr/share/man/man5/containers-registries.d.5*; do zcat -f "$m" | grep -n -i -B2 -A6 "usr/share\|/etc/containers/registries.d" | head -30; done
echo "--- bootc switch help"; bootc switch --help
echo "--- build-chunked-oci help"; rpm-ostree compose build-chunked-oci --help
echo "--- bootc-base-imagectl"; ls -la /usr/libexec/bootc-base-imagectl 2>&1
echo "--- bootc container lint"; bootc container lint 2>&1 | tail -5
' 2>&1
echo "::endgroup::"

echo "::group::cosign"
cosign version
cosign sign --help
cosign attest --help | sed -n '/Flags:/,$p'
cosign copy --help | sed -n '/Flags:/,$p'
echo "::endgroup::"

echo "::group::mini end-to-end"
docker run -d --name reg -p 5000:5000 "$REGISTRY_IMAGE" > /dev/null
sleep 3
mkdir -p "$work/ctx"
echo signed > "$work/ctx/a"
printf 'FROM scratch\nCOPY a /a\n' > "$work/ctx/Containerfile"
reg=localhost:5000/tur
digest_of() { skopeo inspect --tls-verify=false --format '{{.Digest}}' "docker://$reg:$1"; }
for t in signed unsigned other norekor keyless; do
    echo "$t" > "$work/ctx/a"
    $P build -q -t "$reg:$t" "$work/ctx" > /dev/null
    $P push -q --tls-verify=false "$reg:$t"
    echo "$t: $(digest_of "$t")"
done
cd "$work"
COSIGN_PASSWORD="" cosign generate-key-pair --output-key-prefix release > /dev/null 2>&1
COSIGN_PASSWORD="" cosign generate-key-pair --output-key-prefix other > /dev/null 2>&1
ls -la
common=(--yes --allow-http-registry --sign-container-identity ghcr.io/giorgiogiudice/tur)
legacy=(--new-bundle-format=false --use-signing-config=false)
export COSIGN_PASSWORD=""
echo "--- sign: release key + rekor"; cosign sign "${common[@]}" "${legacy[@]}" --tlog-upload=true --key release.key "$reg@$(digest_of signed)"; echo "rc=$?"
echo "--- sign: other key + rekor"; cosign sign "${common[@]}" "${legacy[@]}" --tlog-upload=true --key other.key "$reg@$(digest_of other)"; echo "rc=$?"
echo "--- sign: release key, no rekor"; cosign sign "${common[@]}" "${legacy[@]}" --tlog-upload=false --key release.key "$reg@$(digest_of norekor)"; echo "rc=$?"
echo "--- sign: keyless"; cosign sign "${common[@]}" "${legacy[@]}" "$reg@$(digest_of keyless)"; echo "rc=$?"
echo "--- sign: keyless default format (for comparison)"; cosign sign --yes --allow-http-registry "$reg@$(digest_of signed)"; echo "rc=$?"
echo "--- verify keyless with cosign"
cosign verify --allow-http-registry --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    --certificate-identity-regexp '^https://github.com/giorgiogiudice/TUR/' "$reg@$(digest_of keyless)" > keyless-verify.json; echo "rc=$?"
python3 -c 'import json,sys; [print(json.dumps(x.get("optional"), indent=1)) for x in json.load(open(sys.argv[1]))]' keyless-verify.json | head -40
echo "--- attest (legacy)"
echo '{"spdxVersion":"SPDX-2.3","name":"probe"}' > sbom.json
cosign attest --yes --allow-http-registry "${legacy[@]}" --type spdxjson --predicate sbom.json "$reg@$(digest_of keyless)"; echo "rc=$?"
echo "--- copy with signatures"
cosign copy --allow-http-registry "$reg@$(digest_of keyless)" localhost:5000/tur2:copied; echo "rc=$?"
curl -s http://localhost:5000/v2/tur2/tags/list; echo
skopeo inspect --tls-verify=false --format '{{.Digest}}' docker://localhost:5000/tur2:copied
echo "--- registry tags";curl -s http://localhost:5000/v2/tur/tags/list; echo
d=$(digest_of signed); sig="sha256-${d#sha256:}.sig"
echo "--- $sig manifest"; skopeo inspect --tls-verify=false --raw "docker://$reg:$sig" | python3 -m json.tool | head -60
cd - > /dev/null
cosign initialize > /dev/null 2>&1 || true
root_json=$(find ~/.sigstore -name trusted_root.json | head -1)
echo "trusted root: $root_json"
python3 scripts/release/trust-roots.py "$root_json" "$work"
cat > "$work/policy-key.json" <<'EOF'
{"default":[{"type":"insecureAcceptAnything"}],
 "transports":{"docker":{"ghcr.io/giorgiogiudice/tur":[{"type":"sigstoreSigned","keyPath":"/t/release.pub","rekorPublicKeyPath":"/t/rekor.pub","signedIdentity":{"type":"matchRepository"}}]}}}
EOF
cat > "$work/policy-fulcio.json" <<'EOF'
{"default":[{"type":"insecureAcceptAnything"}],
 "transports":{"docker":{"ghcr.io/giorgiogiudice/tur":[{"type":"sigstoreSigned","fulcio":{"caPath":"/t/fulcio.pem","oidcIssuer":"https://token.actions.githubusercontent.com","subjectEmail":"nobody@example.invalid"},"rekorPublicKeyPath":"/t/rekor.pub","signedIdentity":{"type":"matchRepository"}}]}}}
EOF
cat > "$work/tur.yaml" <<'EOF'
docker:
  ghcr.io/giorgiogiudice/tur:
    use-sigstore-attachments: true
EOF
cat > "$work/physical.yaml" <<'EOF'
docker:
  localhost:5000/tur:
    use-sigstore-attachments: true
EOF
cat > "$work/99-test.conf" <<'EOF'
[[registry]]
prefix = "ghcr.io/giorgiogiudice/tur"
location = "localhost:5000/tur"
insecure = true
EOF
for policy in key fulcio; do
  for t in signed unsigned other norekor keyless; do
    echo "--- policy=$policy image=$t"
    $P run --rm --network host -v "$work:/t:ro,z" \
        -v "$work/policy-$policy.json:/usr/share/containers/policy.json:ro,z" \
        -v "$work/tur.yaml:/usr/share/containers/registries.d/tur.yaml:ro,z" \
        -v "$work/physical.yaml:/etc/containers/registries.d/99-test.yaml:ro,z" \
        -v "$work/99-test.conf:/etc/containers/registries.conf.d/99-test.conf:ro,z" \
        "$base" skopeo copy "docker://ghcr.io/giorgiogiudice/tur:$t" oci:/tmp/out 2>&1 | tail -3
    echo "rc=${PIPESTATUS[0]}"
  done
done
echo "--- policy=key image=signed, without the registries.d entry for the physical location"
$P run --rm --network host -v "$work:/t:ro,z" \
    -v "$work/policy-key.json:/usr/share/containers/policy.json:ro,z" \
    -v "$work/tur.yaml:/usr/share/containers/registries.d/tur.yaml:ro,z" \
    -v "$work/99-test.conf:/etc/containers/registries.conf.d/99-test.conf:ro,z" \
    "$base" skopeo copy "docker://ghcr.io/giorgiogiudice/tur:signed" oci:/tmp/out 2>&1 | tail -3
echo "rc=${PIPESTATUS[0]}"
echo "--- policy=key image=signed pulled by its physical name (scope does not apply)"
$P run --rm --network host -v "$work:/t:ro,z" \
    -v "$work/policy-key.json:/usr/share/containers/policy.json:ro,z" \
    "$base" skopeo copy --src-tls-verify=false "docker://localhost:5000/tur:unsigned" oci:/tmp/out 2>&1 | tail -3
echo "rc=${PIPESTATUS[0]}"
echo "--- bootc and the image proxy"
$P run --rm --network none "$base" /bin/bash -c 'strings /usr/bin/bootc | grep -E -- "insecure-policy|experimental-image-proxy|insecureAcceptAnything" | sort -u | head'
echo "--- oci transport unaffected"
$P run --rm --network host -v "$work/policy-key.json:/usr/share/containers/policy.json:ro,z" "$base" \
    /bin/bash -c 'skopeo copy docker://localhost:5000/tur:unsigned --src-tls-verify=false oci:/tmp/l:x && skopeo copy oci:/tmp/l:x oci:/tmp/m:y && echo oci-ok'
echo "::endgroup::"

echo "::group::sigstore trusted root (TUF-verified by cosign)"
cosign initialize > /dev/null 2>&1 || true
find ~/.sigstore -name trusted_root.json | head
f=$(find ~/.sigstore -name trusted_root.json | head -1)
[ -n "$f" ] && sha256sum "$f" "$work/rekor.pub" "$work/fulcio.pem"; cat "$work/rekor.pub"
echo "::endgroup::"

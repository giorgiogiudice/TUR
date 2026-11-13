#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Dry-run test of the installer release, end to end, with no real storage and
# no real release:
#
#   scripts/release-installer/test/run.sh WORK_DIR
#
# Runs the same scripts the release-installer workflow runs, in the same
# order, against
#
#   - a local S3-compatible server: moto in server mode (MOTO_IMAGE, pinned by
#     digest in the workflow), standing in for Linode Object Storage;
#   - a stand-in for GitHub's REST API (fake_github.py), standing in for the
#     runs, artifacts, tags and releases the workflow reads and writes;
#   - a small fake medium (make_fake_medium.py), 24 MiB so that its upload is
#     split into five parts.
#
# What it shows: the source run checks refuse each kind of wrong run; the
# medium is verified and laid out; it is signed (with a throwaway key, and
# keyless when the job may request an identity token) and every signature
# verifies; the notes render; the upload is multipart and read back through
# the public address; a second upload of the same files is accepted and one
# of different content is refused; read-back detects a changed object and a
# wrong Content-Type; the pre-release is created on the right commit, as a
# pre-release, never "latest", with exactly the small files; a second release
# of the same tag is refused; a failed release leaves no draft and no tag.
#
# Needs docker, rclone, cosign, curl and python3 (the workflow installs the
# pinned rclone and cosign). Test use only.
set -euo pipefail

work="$(realpath -m "${1:?work directory required}")"
here="$(cd "$(dirname "$0")/.." && pwd)"
test="${here}/test"
moto_image="${MOTO_IMAGE:?MOTO_IMAGE required}"
rev=6f4cbebe39ee8a3b7fa5a291202ae2c64db130b1
version=0.1.0-test.1
repo=tur-test/TUR
s3=http://127.0.0.1:5000
api=http://127.0.0.1:8765
token="fake-token-$$"
rm -rf "${work}"
mkdir -p "${work}"

pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
# Run a command that must fail, and whose output must contain a phrase.
refused() {
    local phrase="$1"; shift
    local out
    if out="$("$@" 2>&1)"; then echo "${out}"; fail "expected a refusal: $*"; fi
    grep -qF -- "${phrase}" <<< "${out}" || { echo "${out}"; fail "refusal without '${phrase}': $*"; }
}

cleanup() {
    docker rm -f tur-moto > /dev/null 2>&1 || true
    if [ -n "${fake_pid:-}" ]; then kill "${fake_pid}" 2> /dev/null || true; fi
}
trap cleanup EXIT

echo "== stand-ins"
docker run -d --name tur-moto -p 127.0.0.1:5000:5000 "${moto_image}" > /dev/null
for _ in $(seq 60); do curl -sf "${s3}/" > /dev/null && break; sleep 1; done
curl -sf "${s3}/" > /dev/null || { docker logs tur-moto; fail "moto did not start"; }
curl -sf -X PUT "${s3}/tur-downloads" > /dev/null
echo "moto: $(docker image inspect -f '{{index .RepoDigests 0}}' "${moto_image}")"

run() {  # id path branch event conclusion head_repo
    printf '"%s": {"id": %s, "path": "%s", "head_branch": "%s", "event": "%s", "status": "completed", "conclusion": "%s", "head_sha": "%s", "head_repository": {"full_name": "%s"}, "html_url": "https://example.invalid/runs/%s", "created_at": "2026-11-12T21:58:56Z"}' \
        "$1" "$1" "$2" "$3" "$4" "$5" "${7:-${rev}}" "$6" "$1"
}
artifacts='[{"name": "tur-installer-medium-DO-NOT-REDISTRIBUTE", "expired": false, "size_in_bytes": 1, "expires_at": "2026-11-15T22:50:04Z"}, {"name": "tur-installer-medium-test", "expired": false}]'
other_sha=1111111111111111111111111111111111111111
cat > "${work}/fixture.json" <<EOF
{
  "runs": {
    $(run 100 .github/workflows/iso.yml main workflow_dispatch success "${repo}"),
    $(run 101 .github/workflows/iso.yml feat/x workflow_dispatch success "${repo}"),
    $(run 102 .github/workflows/iso.yml main push failure "${repo}"),
    $(run 103 .github/workflows/iso.yml main push success "${repo}"),
    $(run 104 .github/workflows/iso.yml main workflow_dispatch success "${repo}"),
    $(run 105 .github/workflows/iso.yml main workflow_dispatch success "${repo}" "${other_sha}"),
    $(run 106 .github/workflows/image.yml main workflow_dispatch success "${repo}"),
    $(run 107 .github/workflows/iso.yml main pull_request success "someone/TUR")
  },
  "artifacts": {
    "100": ${artifacts}, "101": ${artifacts}, "102": ${artifacts}, "105": ${artifacts},
    "106": ${artifacts}, "107": ${artifacts},
    "103": [{"name": "tur-installer-medium-test", "expired": false}],
    "104": [{"name": "tur-installer-medium-DO-NOT-REDISTRIBUTE", "expired": true}, {"name": "tur-installer-medium-test", "expired": false}]
  },
  "compare": {"${rev}": "ahead", "${other_sha}": "diverged"}
}
EOF
python3 "${test}/fake_github.py" 8765 "${work}/fixture.json" "${token}" &
fake_pid=$!
for _ in $(seq 30); do curl -sf "${api}/_state" > /dev/null && break; sleep 0.5; done

export GITHUB_API_URL="${api}" TUR_UPLOADS_URL="${api}" GITHUB_REPOSITORY="${repo}" GH_TOKEN="${token}"

echo "== the source run"
python3 "${here}/source.py" 100 "${work}/source.env" > /dev/null
grep -qx "source_sha=${rev}" "${work}/source.env" || fail "source.env"
pass "a green iso run on main with its medium is accepted"
refused "not main" python3 "${here}/source.py" 101 "${work}/x.env"
refused "not completed/success" python3 "${here}/source.py" 102 "${work}/x.env"
refused "with upload_medium ticked" python3 "${here}/source.py" 103 "${work}/x.env"
refused "has expired" python3 "${here}/source.py" 104 "${work}/x.env"
refused "is not on main" python3 "${here}/source.py" 105 "${work}/x.env"
refused "not .github/workflows/iso.yml" python3 "${here}/source.py" 106 "${work}/x.env"
refused "someone/TUR" python3 "${here}/source.py" 107 "${work}/x.env"
refused "numeric id" python3 "${here}/source.py" "100; true" "${work}/x.env"
pass "runs on another branch, failed, without a medium, expired, off main, of another workflow, from a fork: refused"

echo "== verify and lay out the medium"
python3 "${test}/make_fake_medium.py" "${work}/source" "${rev}" 24
python3 "${here}/verify.py" --medium-dir "${work}/source/medium" --record-dir "${work}/source/record" \
    --revision "${rev}" --version "${version}" --out "${work}/release"
fact() { sed -n "s/^$1=//p" "${work}/release/release.env"; }
base="$(fact base)" iso="$(fact iso)" sums="$(fact sums)" manifest="$(fact manifest)"
[ "${iso}" = "tur-${version}-x86_64.iso" ] || fail "release name ${iso}"
(cd "${work}/release" && sha256sum -c "${sums}")
pass "the medium is verified against its two records and laid out as ${iso}"

echo "== the guard"
guard_env=(EVENT=workflow_dispatch REF=refs/heads/main RELEASES_ENABLED=1 HAVE_ENDPOINT=true HAVE_BUCKET=true
    HAVE_ACCESS_KEY=true HAVE_SECRET_KEY=true PUBLIC_URL=https://dl.example.org REDISTRIBUTABLE=yes TAG_FREE=yes)
GITHUB_OUTPUT="${work}/guard.out" env "${guard_env[@]}" PUBLISH_INPUT=false python3 "${here}/guard.py" > /dev/null
grep -qx "publish=false" "${work}/guard.out" || fail "publish=false not written"
refused "refusing to publish" env "${guard_env[@]}" PUBLISH_INPUT=false python3 "${here}/guard.py" --require
env "${guard_env[@]}" PUBLISH_INPUT=true python3 "${here}/guard.py" --require > /dev/null
pass "the guard: dry run unless every condition holds; --require stops a dry run"

echo "== sign"
mkdir -p "${work}/keys"
( cd "${work}/keys" && COSIGN_PASSWORD="" cosign generate-key-pair --output-key-prefix release > /dev/null \
    && COSIGN_PASSWORD="" cosign generate-key-pair --output-key-prefix other > /dev/null )
# Keyless only where the workflow allows it (pull requests: TUR_TEST_KEYLESS=1),
# so that the release workflow's identity on main never signs a test file.
keyless=0
if [ -n "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ] && [ "${TUR_TEST_KEYLESS:-0}" = 1 ]; then keyless=1; fi
refused "is not the key whose public half" env TUR_SIGN_KEYLESS=0 TUR_RELEASE_KEY="$(cat "${work}/keys/release.key")" \
    COSIGN_PASSWORD="" TUR_REPO_RELEASE_PUB="${work}/keys/other.pub" "${here}/sign.sh" "${work}/release" "${base}"
TUR_SIGN_KEYLESS="${keyless}" TUR_RELEASE_KEY="$(cat "${work}/keys/release.key")" COSIGN_PASSWORD="" \
    TUR_REPO_RELEASE_PUB="${work}/keys/release.pub" "${here}/sign.sh" "${work}/release" "${base}"
cmp "${work}/release/${base}.release-key.pub" "${work}/keys/release.pub"
cosign verify-blob --bundle "${work}/release/${sums}.release-key.sigstore.json" --key "${work}/keys/release.pub" \
    "${work}/release/${sums}"
cp "${work}/release/${sums}" "${work}/keys/tampered-sums"
echo "0000000000000000000000000000000000000000000000000000000000000000  x" >> "${work}/keys/tampered-sums"
refused "" cosign verify-blob --bundle "${work}/release/${sums}.release-key.sigstore.json" \
    --key "${work}/keys/release.pub" "${work}/keys/tampered-sums"
if [ "${keyless}" = 1 ]; then
    pass "signed keyless and with a release key; both verify; a changed file does not"
else
    echo "NOTE: keyless signing not exercised (not a pull request, or no identity token: a fork?)"
    pass "signed with a release key; it verifies; a changed file does not"
fi

echo "== notes"
DRY_RUN=false SOURCE_RUN_URL="https://example.invalid/runs/100" TUR_DOWNLOADS_PUBLIC_URL="${s3}/tur-downloads" \
    python3 "${here}/notes.py" "${here}/../../docs/releases/TEMPLATE.md" "${here}/../../docs/releases/known-issues.md" \
    "${work}/release" "${work}/release/notes.md"
grep -qF "${s3}/tur-downloads/releases/${version}/${iso}" "${work}/release/notes.md" || fail "download link"
grep -qF "sha256sum -c --ignore-missing ${sums}" "${work}/release/notes.md" || fail "sha256sum command"
pass "the notes name the download, the checksum and the signature commands"

echo "== upload and read back (S3 stand-in)"
export TUR_DOWNLOADS_ENDPOINT="${s3}" TUR_DOWNLOADS_BUCKET=tur-downloads TUR_DOWNLOADS_ACCESS_KEY=test \
    TUR_DOWNLOADS_SECRET_KEY=test TUR_DOWNLOADS_PUBLIC_URL="${s3}/tur-downloads" TUR_DOWNLOADS_PROVIDER=Other \
    TUR_S3_UPLOAD_CUTOFF=8M TUR_S3_CHUNK_SIZE=5M
upload_files=("${iso}" "${sums}" "${manifest}")
for f in "${work}"/release/*.sigstore.json "${work}/release/${base}.release-key.pub"; do upload_files+=("$(basename "${f}")"); done
python3 "${here}/upload.py" "${work}/release" "releases/${version}" "${work}/uploads.json" "${upload_files[@]}"
etag="$(curl -sfI "${s3}/tur-downloads/releases/${version}/${iso}" | tr -d '\r' | sed -n 's/^[Ee][Tt]ag: *//p')"
case "${etag}" in *-5\"|*-5) ;; *) fail "the medium was not uploaded in five parts (ETag ${etag})" ;; esac
pass "multipart upload (ETag ${etag}), every object read back through its public address"
python3 "${here}/upload.py" "${work}/release" "releases/${version}" "${work}/uploads-again.json" "${upload_files[@]}" > /dev/null
pass "uploading the same files again is accepted (a stopped release can be repeated)"
mkdir -p "${work}/changed"
cp "${work}/release/${sums}" "${work}/changed/${sums}"
echo "# changed" >> "${work}/changed/${sums}"
refused "rclone copyto failed" python3 "${here}/upload.py" "${work}/changed" "releases/${version}" "${work}/x.json" "${sums}"
pass "an object is never replaced by different content"

readback() {  # remote-key local-file content-type
    python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import upload, hashlib, pathlib
p = pathlib.Path(sys.argv[3])
upload.read_back("turdl:tur-downloads/" + sys.argv[2], sys.argv[5] + "/" + sys.argv[2], p.stat().st_size,
                 hashlib.sha256(p.read_bytes()).hexdigest(), sys.argv[4])' "${here}" "$1" "$2" "$3" "${s3}/tur-downloads"
}
readback "releases/${version}/${sums}" "${work}/release/${sums}" "text/plain" > /dev/null
same_size="$(head -c "$(stat -c %s "${work}/release/${sums}")" /dev/zero | tr '\0' 'x')"
curl -sf -X PUT -H "Content-Type: text/plain" --data-binary "${same_size}" "${s3}/tur-downloads/probe/sums" > /dev/null
refused "SHA-256" readback "probe/sums" "${work}/release/${sums}" "text/plain"
curl -sf -X PUT -H "Content-Type: application/octet-stream" --data-binary @"${work}/release/${sums}" \
    "${s3}/tur-downloads/probe/type" > /dev/null
refused "served as" readback "probe/type" "${work}/release/${sums}" "text/plain"
pass "read back detects an object of the right size with the wrong content, and a wrong Content-Type"

echo "== the pre-release (GitHub stand-in)"
python3 "${here}/github_release.py" check --tag "v${version}"
assets=("${work}/release/${sums}" "${work}/release/${manifest}" "${work}"/release/*.sigstore.json
    "${work}/release/${base}.release-key.pub")
python3 "${here}/github_release.py" create --tag "v${version}" --sha "${rev}" \
    --name "TUR ${version} (pre-alpha preview)" --notes "${work}/release/notes.md" "${assets[@]}"
curl -sf "${api}/_state" > "${work}/state.json"
python3 - "${work}/state.json" "v${version}" "${rev}" "${#assets[@]}" "${work}/release/notes.md" <<'PY'
import json, sys
state, tag, rev, count, notes = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3], int(sys.argv[4]), open(sys.argv[5]).read()
releases = [r for r in state["releases"].values() if r["tag_name"] == tag]
assert len(releases) == 1, releases
r = releases[0]
assert r["prerelease"] is True and r["draft"] is False, r
assert r["_latest_allowed"] is False, "make_latest was not false"
assert r["target_commitish"] == rev and state["tags"][tag] == rev, (r["target_commitish"], state["tags"])
assert len(r["assets"]) == count and not any(a["name"].endswith(".iso") for a in r["assets"]), r["assets"]
assert r["body"].strip() == notes.strip()
assert all(a["size"] < 2 * 1024**3 for a in r["assets"])
print("release:", tag, "on", rev, "pre-release, not latest,", count, "assets:", ", ".join(a["name"] for a in r["assets"]))
PY
pass "pre-release created on the source commit, never latest, with the small files and the notes"
refused "the tag v${version} exists" python3 "${here}/github_release.py" check --tag "v${version}"
refused "a release is never replaced" python3 "${here}/github_release.py" create --tag "v${version}" --sha "${rev}" \
    --name x --notes "${work}/release/notes.md" "${assets[@]}"
echo "an upload GitHub refuses" > "${work}/reject-me.txt"
refused "deleting draft release" python3 "${here}/github_release.py" create --tag "v0.1.0-test.2" --sha "${rev}" \
    --name x --notes "${work}/release/notes.md" "${work}/release/${sums}" "${work}/reject-me.txt"
refused "missing:" python3 "${here}/github_release.py" create --tag "v0.1.0-test.2" --sha "${rev}" \
    --name x --notes "${work}/release/notes.md" "${work}/release/missing-file"
curl -sf "${api}/_state" | python3 -c '
import json, sys
s = json.load(sys.stdin)
assert "v0.1.0-test.2" not in s["tags"], s["tags"]
assert not [r for r in s["releases"].values() if r["tag_name"] == "v0.1.0-test.2"], "draft left behind"'
refused "not a pre-release tag" python3 "${here}/github_release.py" check --tag "v0.1.0"
pass "a release is never replaced; a failed one leaves no draft and no tag; only pre-release tags"

echo "ALL PASSED"

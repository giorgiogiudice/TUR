#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# windiff under Wine's master branch, before and after the fix queue, in a
# Fedora 45 container:
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/windiff/ci/wine-master.sh
#
#   1. builds the pinned master, unpatched (wine-build.sh; cached tree)
#   2. runs the check programs under it          -> results-master.json
#   3. applies the fix queue (wine-patches/series, in order) and rebuilds
#      what the patches touch
#   4. runs the check programs again             -> results-patched.json
#   5. runs the Wine conformance tests the queue adds to, under the patched
#      Wine, and puts the test programs in evidence/winetests so that the
#      Windows job can run the same binaries on real Windows
#   6. takes the patches out of the tree again, so that the cached tree is
#      the unpatched one
#
# The job is green when the baseline ran; a patch that does not apply or
# does not build is recorded, not fatal.
set -uo pipefail
. /src/tests/experiments/windiff/ci/wine-common.sh

ev=/work/evidence
w=/work/winebuild
patches="${windiff_src}/wine-patches"
mkdir -p "${ev}"
summary="${ev}/wine-master.md"
: >"${summary}"
log() { echo "$*" | tee -a "${summary}"; }

bash "${windiff_src}/ci/wine-build.sh" || { log "- Wine master ${WINE_MASTER:0:8}: **build failed**"; exit 1; }
windiff_user
wine="${w}/build/wine"
server="${w}/build/server/wineserver"
log "## Wine master ${WINE_MASTER:0:8} ($("${wine}" --version 2>&1 | head -n 1))"
log ""

run_checks master "${wine}" "${server}" "${ev}"
[ -s "${ev}/results-master.json" ] || { log "- baseline run wrote no results"; exit 1; }

mapfile -t series < <(grep -vE '^\s*(#|$)' "${patches}/series" 2>/dev/null)
if [ "${#series[@]}" -eq 0 ]; then
    log "- fix queue: empty"
    chmod -R a+rX "${ev}"
    exit 0
fi

# Does each patch also apply to the Wine Fedora 45 ships?
fedora_tree=/tmp/wine-fedora-check
rm -rf "${fedora_tree}"
git init -q "${fedora_tree}" && git -C "${fedora_tree}" remote add origin "${WINE_REPO}"
have_fedora=0
git -C "${fedora_tree}" fetch -q --depth 1 origin "${WINE_FEDORA_TAG}" 2>/dev/null &&
    git -C "${fedora_tree}" checkout -q FETCH_HEAD && have_fedora=1

applied=()
for p in "${series[@]}"; do
    if git -C "${w}/src" apply --check "${patches}/${p}" 2>"${ev}/apply-${p}.log"; then
        git -C "${w}/src" apply --whitespace=nowarn "${patches}/${p}"
        applied+=("${p}")
        note="applied to master"
    else
        note="**does NOT apply to master** ($(head -n 1 "${ev}/apply-${p}.log"))"
    fi
    if [ "${have_fedora}" = 1 ]; then
        if git -C "${fedora_tree}" apply --check "${patches}/${p}" 2>/dev/null; then
            git -C "${fedora_tree}" apply --whitespace=nowarn "${patches}/${p}"
            note="${note}; applies to wine-11.20"
        else
            note="${note}; does not apply to wine-11.20"
        fi
    fi
    log "- ${p}: ${note}"
done
rm -rf "${fedora_tree}"
git -C "${w}/src" diff --stat >"${ev}/patched-diffstat.txt"
git -C "${w}/src" diff -U0 -- 'dlls/*/tests/*.c' >"${ev}/patched-tests.diff"

start=$(date +%s)
if make -C "${w}/build" -j"$(nproc)" >"${ev}/wine-make-patched.log" 2>&1; then
    log "- patched build: ok ($(( $(date +%s) - start )) s, ${#applied[@]} of ${#series[@]} patches)"
    run_checks patched "${wine}" "${server}" "${ev}"

    # The conformance tests the queue adds to.
    mkdir -p "${ev}/winetests"
    cp "${ev}/patched-tests.diff" "${ev}/winetests/added.diff"
    : >"${ev}/winetests/tests.txt"
    while read -r file; do
        [ -n "${file}" ] || continue
        module="$(echo "${file}" | cut -d/ -f2)"
        name="$(basename "${file}" .c)"
        exe="${module}_test.exe"
        built="$(find "${w}/build/dlls/${module}/tests" -name "${exe}" | head -n 1)"
        if [ -z "${built}" ]; then
            log "- conformance test ${file}: test program ${exe} was not built"
            continue
        fi
        cp "${built}" "${ev}/winetests/${exe}"
        echo "${exe} ${name} ${file}" >>"${ev}/winetests/tests.txt"
    done < <(git -C "${w}/src" diff --name-only -- 'dlls/*/tests/*.c')
    prefix=/work/prefix-winetests
    rm -rf "${prefix}"; mkdir -p "${prefix}" /work/winetests-cwd
    chown -R tur "${prefix}" /work/winetests-cwd "${ev}/winetests"
    envs=(HOME=/home/tur WINEPREFIX="${prefix}" LC_ALL=C.UTF-8 "WINEDLLOVERRIDES=mscoree,mshtml=" WINEDEBUG=-all)
    runuser -u tur -- env "${envs[@]}" timeout 600 "${wine}" wineboot -u >/dev/null 2>&1
    while read -r exe name file; do
        (cd /work/winetests-cwd && runuser -u tur -- env "${envs[@]}" timeout 900 "${wine}" "${ev}/winetests/${exe}" "${name}" \
            >"${ev}/winetests/${exe}.${name}.wine-patched.txt" 2>&1)
        echo "exit status $?" >>"${ev}/winetests/${exe}.${name}.wine-patched.txt"
    done <"${ev}/winetests/tests.txt"
    runuser -u tur -- env "${envs[@]}" timeout 60 "${server}" -k || true
    python3 -I "${windiff_src}/winetest_report.py" "${ev}/winetests" wine-patched | tee -a "${summary}"
else
    log "- patched build: **FAILED** ($(( $(date +%s) - start )) s)"
    grep -B3 -A8 -E 'error:' "${ev}/wine-make-patched.log" | head -n 120 | tee "${ev}/wine-make-patched-errors.txt"
fi

# Back to the unpatched tree for the cache.
git -C "${w}/src" checkout -q -- . && git -C "${w}/src" clean -fdq
chmod -R a+rX "${ev}" "${w}"
exit 0

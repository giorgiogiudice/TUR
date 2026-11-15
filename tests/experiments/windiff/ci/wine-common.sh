# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Shared by the windiff container scripts (sourced, not run).

windiff_src=/src/tests/experiments/windiff

. /src/tests/experiments/windiff/ci/wine-pin.sh

windiff_user() {
    id tur >/dev/null 2>&1 || useradd -m -u 1001 tur
}

# run_checks LABEL WINE WINESERVER EVIDENCE_DIR
# Makes a fresh prefix and runs every check program in it as the user "tur"
# (root may write to read-only files, which would hide what Wine does).
run_checks() {
    local label="$1" wine="$2" server="$3" ev="$4"
    local prefix="/work/prefix-${label}"
    rm -rf "${prefix}"
    mkdir -p "${prefix}" "${ev}"
    chown -R tur "${prefix}" "${ev}"
    local envs=(HOME=/home/tur WINEPREFIX="${prefix}" LC_ALL=C.UTF-8 "WINEDLLOVERRIDES=mscoree,mshtml=" PYTHONUNBUFFERED=1)
    runuser -u tur -- env "${envs[@]}" WINEDEBUG=-all timeout 600 "${wine}" wineboot -u >"${ev}/wineboot-${label}.log" 2>&1 ||
        echo "wineboot (${label}) exited with $?"
    runuser -u tur -- env "${envs[@]}" timeout 120 "${server}" -w || true
    runuser -u tur -- env "${envs[@]}" "${wine}" --version >"${ev}/wine-version-${label}.txt" 2>&1 || true
    runuser -u tur -- env "${envs[@]}" python3 -I "${windiff_src}/run.py" --bin /work/bin --root "${prefix}/drive_c" \
        --out "${ev}/results-${label}.json" --label "${label}" --wine "${wine}" --wineserver "${server}" \
        ${WINDIFF_ONLY:+--only "${WINDIFF_ONLY}"} 2>&1 | tee "${ev}/run-${label}.log"
    runuser -u tur -- env "${envs[@]}" timeout 60 "${server}" -k || true
    # What Wine itself said it lacks while the checks ran.
    grep -rhoaE '(fixme|err):[a-z0-9_]+:[A-Za-z0-9_]+' "${ev}/results-${label}.json.logs" 2>/dev/null |
        sort | uniq -c | sort -rn >"${ev}/fixmes-${label}.txt" || true
    tar -C "${ev}" -czf "${ev}/logs-${label}.tar.gz" "results-${label}.json.logs" 2>/dev/null && rm -rf "${ev}/results-${label}.json.logs"
}

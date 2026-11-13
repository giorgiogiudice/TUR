#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The build half of the KMDF experiment, inside a Fedora 45 container (the
# workflow's kmdf-build job):
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/windows-drivers/kmdf/ci-build.sh
#
#   1. Wine 11.20 + third-party/wine-patches: ntoskrnl.exe, wdfldr.sys
#      (build-wine.sh)
#   2. Wdf01000.sys from Microsoft's MIT-licensed Windows-Driver-Frameworks
#      with clang/lld and mingw-w64's headers (build-wdf.sh), its imports
#      classified against Wine 11.20, Wine master and the patched tree
#   3. Microsoft's kmdf_fx2 sample driver and test program, and the KMDF
#      binding probe, against the same headers (build-fx2.sh)
#
# Writes /work/kmdf/ (build outputs) and /work/kmdf-evidence/ (logs and
# summaries). A step that fails is recorded and the next one still runs;
# the job is green when the record was written.
set -uo pipefail

src=/src
here="${src}/tests/windows-drivers/kmdf"
out=/work/kmdf
ev=/work/kmdf-evidence
mkdir -p "${out}" "${ev}"
summary="${ev}/summary.md"
: >"${summary}"
log() { echo "$*" | tee -a "${summary}"; }

dnf -y --setopt=install_weak_deps=False install \
    clang lld llvm mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    gcc make flex bison autoconf perl git-core python3 findutils diffutils \
    >"${ev}/dnf.log" 2>&1 || { tail -n 50 "${ev}/dnf.log"; exit 1; }
{
    echo "clang: $(clang --version | head -n 1)"
    rpm -q clang lld llvm mingw64-gcc mingw64-headers mingw64-crt autoconf | sed 's/^/  /'
} | tee "${ev}/toolchain.txt"

log "## KMDF experiment: build"
log ""

# 1. Wine with the patch set
if bash "${here}/build-wine.sh" "${out}" >"${ev}/build-wine.out" 2>&1; then
    log "- Wine 11.20 + TUR patches: built ($(ls "${out}/wine-overlay" | tr '\n' ' '))"
else
    log "- Wine 11.20 + TUR patches: **FAILED** (build-wine.out)"
fi
grep -E '^(applied|applies|does NOT|wine:|master)' "${ev}/build-wine.out" | sed 's/^/  - /' | tee -a "${summary}"
cp "${out}"/wine-*.log "${ev}/" 2>/dev/null || true

# Wine source trees the imports are classified against (only the module
# directories the classification reads).
trees="${out}/trees"
rm -rf "${trees}"
mkdir -p "${trees}"
sparse_tree() { # name ref
    local d="${trees}/$1"
    git init -q "${d}" && git -C "${d}" remote add origin https://gitlab.winehq.org/wine/wine.git &&
        git -C "${d}" sparse-checkout set dlls/ntoskrnl.exe dlls/hal dlls/usbd.sys dlls/wmilib.sys \
            dlls/ksecdd.sys dlls/ntdll dlls/wdfldr.sys &&
        git -C "${d}" fetch -q --depth 1 --filter=blob:none origin "$2" && git -C "${d}" checkout -q FETCH_HEAD
}
sparse_tree wine-11.20 7298a818b79446bae153b8d5aa27235ab3703bf9 || log "- could not fetch wine-11.20 for the import table"
sparse_tree wine-master cc5d1d86ee7d60649e6f858106db046c77c8b12c || log "- could not fetch wine master for the import table"
[ -d "${out}/wine-src" ] && ln -sfn "${out}/wine-src" "${trees}/wine-11.20+tur"

# 2. The framework
compare=()
for t in wine-11.20 wine-master wine-11.20+tur; do
    [ -d "${trees}/${t}/dlls" ] && compare+=("${trees}/${t}")
done
LINK_WINE_TREE="${trees}/wine-11.20+tur" bash "${here}/build-wdf.sh" "${out}" "${compare[@]}" \
    >"${ev}/build-wdf.out" 2>&1
log ""
log "### Wdf01000.sys"
log '```'
cat "${out}/wdf/summary.txt" | head -n 260 | tee -a "${summary}" >/dev/null
log '```'
mkdir -p "${ev}/wdf"
cp "${out}"/wdf/*.txt "${out}"/wdf/*.md "${out}"/wdf/*.json "${out}"/wdf/*.log "${ev}/wdf/" 2>/dev/null || true
tar -C "${out}/wdf" -czf "${ev}/wdf/compile-logs.tar.gz" logs 2>/dev/null || true
[ -f "${out}/wdf/imports.md" ] && cat "${out}/wdf/imports.md" >>"${summary}"

# 3. The sample driver, its test program and the probe
if [ -f "${here}/build-fx2.sh" ]; then
    bash "${here}/build-fx2.sh" "${out}" >"${ev}/build-fx2.out" 2>&1
    log ""
    log "### kmdf_fx2 and the probe"
    log '```'
    cat "${out}/fx2/summary.txt" 2>/dev/null | tee -a "${summary}" >/dev/null
    log '```'
    mkdir -p "${ev}/fx2"
    cp "${out}"/fx2/*.txt "${out}"/fx2/*.log "${ev}/fx2/" 2>/dev/null || true
fi

# What the experiment job takes from here.
mkdir -p /work/kmdf-artifact
cp "${out}"/Wdf01000.sys "${out}"/wine-overlay/* "${out}"/fx2/out/* /work/kmdf-artifact/ 2>/dev/null || true
ls -l /work/kmdf-artifact | tee -a "${ev}/artifact-files.txt"
chmod -R a+rX /work/kmdf-artifact "${ev}"
exit 0

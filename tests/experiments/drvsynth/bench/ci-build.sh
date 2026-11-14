#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The build half of the driver-synthesis test bench, in a Fedora 45
# container (workflow exp-drvsynth, job "capture"):
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work -v CACHE:/cache fedora:45@sha256:... \
#       bash /src/tests/experiments/drvsynth/bench/ci-build.sh
#
# 1. The KMDF runtime of the windows-drivers experiment, reused unchanged:
#    Wine 11.20's ntoskrnl.exe, wdfldr.sys and wineusb.sys with TUR's patch
#    set (tests/windows-drivers/kmdf/build-wine.sh) and Wdf01000.sys from
#    Microsoft's MIT-licensed source (build-wdf.sh). About 25 minutes, so
#    what the next steps need is kept in /cache (the workflow caches it,
#    keyed on those inputs) and rebuilt only when it is missing.
# 2. The invented device's Windows driver and program
#    (sealed/windows/build.sh), and the windows-drivers experiment's
#    installer program twdinst.exe (tests/windows-drivers/build.sh).
#
# Output: /work/artifact (everything the run needs), /work/build-evidence.
set -uo pipefail

src=/src
ev=/work/build-evidence
art=/work/artifact
runtime=/cache/runtime
mkdir -p "${ev}" "${art}"
summary="${ev}/summary.md"
: >"${summary}"
log() { echo "$*" | tee -a "${summary}"; }

dnf -y --setopt=install_weak_deps=False install \
    clang lld llvm mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    gcc make flex bison autoconf perl git-core python3 findutils diffutils libusb1-devel \
    >"${ev}/dnf.log" 2>&1 || { tail -n 50 "${ev}/dnf.log"; exit 1; }
rpm -q clang lld mingw64-gcc mingw64-headers | tee "${ev}/toolchain.txt"

log "## drvsynth: build"
if [ -f "${runtime}/Wdf01000.sys" ] && [ -f "${runtime}/wine-overlay/ntoskrnl.exe" ]; then
    log "- KMDF runtime and Wine overlay: from the cache ($(cat "${runtime}/built-by.txt" 2>/dev/null))"
else
    log "- KMDF runtime and Wine overlay: not cached; building"
    k=/work/kmdf
    mkdir -p "${k}"
    if ! bash "${src}/tests/windows-drivers/kmdf/build-wine.sh" "${k}" >"${ev}/build-wine.out" 2>&1; then
        log "- build-wine.sh: FAILED"
        tail -n 40 "${ev}/build-wine.out"
        exit 1
    fi
    LINK_WINE_TREE="${k}/wine-src" bash "${src}/tests/windows-drivers/kmdf/build-wdf.sh" "${k}" \
        >"${ev}/build-wdf.out" 2>&1
    if [ ! -f "${k}/Wdf01000.sys" ]; then
        log "- build-wdf.sh: Wdf01000.sys was not built"
        head -n 40 "${k}/wdf/summary.txt" 2>/dev/null
        exit 1
    fi
    rm -rf "${runtime}"
    mkdir -p "${runtime}/wine-overlay" "${runtime}/wine-tree/dlls"
    cp "${k}/Wdf01000.sys" "${runtime}/"
    cp "${k}"/wine-overlay/* "${runtime}/wine-overlay/"
    cp -a "${k}/wdf/src/src/publicinc/wdf/kmdf/1.33" "${runtime}/kmdf-inc"
    for m in ntoskrnl.exe hal usbd.sys wdfldr.sys; do
        mkdir -p "${runtime}/wine-tree/dlls/${m}"
        cp "${k}/wine-src/dlls/${m}/"*.spec "${runtime}/wine-tree/dlls/${m}/"
    done
    echo "${GITHUB_RUN_ID:-local} $(date -u +%FT%TZ)" >"${runtime}/built-by.txt"
    log "- KMDF runtime: built ($(stat -c %s "${runtime}/Wdf01000.sys") bytes) and cached"
fi

if bash "${src}/tests/experiments/drvsynth/sealed/windows/build.sh" /work/windows "${runtime}" \
    >"${ev}/build-windows.out" 2>&1; then
    log "- qfdesk.sys and gadget.exe: built"
else
    log "- qfdesk.sys / gadget.exe: **FAILED**"
fi
cat /work/windows/summary.txt >>"${summary}" 2>/dev/null
bash "${src}/tests/windows-drivers/build.sh" /work/twd >"${ev}/build-twd.out" 2>&1 ||
    log "- tests/windows-drivers/build.sh reported a failure (only twdinst.exe is needed)"

cp "${runtime}/Wdf01000.sys" "${runtime}"/wine-overlay/* /work/windows/qfdesk.sys /work/windows/qfdesk.inf \
    /work/windows/gadget.exe /work/twd/twdinst.exe "${art}/" 2>/dev/null
ls -l "${art}" | tee -a "${summary}"
chmod -R a+rX "${art}" "${ev}" /cache 2>/dev/null
for f in Wdf01000.sys ntoskrnl.exe wdfldr.sys wineusb.sys wineusb.so qfdesk.sys qfdesk.inf gadget.exe twdinst.exe; do
    [ -f "${art}/${f}" ] || { log "missing: ${f}"; exit 1; }
done
exit 0

#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The Linux candidates of the wincli experiment, inside a Fedora 45
# container with Fedora 45's own Wine (the Wine TUR would ship). Started by
# .github/workflows/exp-wincli.yml as
#
#   docker run --privileged --network host \
#       -v /run/systemd:/run/systemd -v /run/dbus:/run/dbus \
#       -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/wincli/fedora-run.sh
#
# WORK/dl holds the two PowerShell 7 archives (MIT, published by Microsoft),
# fetched and checked by the workflow; they are unpacked here, used and
# thrown away with the runner. Nothing is committed or redistributed.
#
# The container is privileged for two cheap tests only: registering
# binfmt_misc handlers and reading the runner's service manager. The
# runner's network is shared so that ipconfig and friends see a real one.
set -euo pipefail

src=/src/tests/experiments/wincli
work=/work
ev="${work}/evidence"
build="${work}/build"
mkdir -p "${ev}" "${build}" "${work}/p"
export PYTHONDONTWRITEBYTECODE=1 WINEDEBUG=-all LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 TZ=UTC
only="${WINCLI_ONLY:-}"

dnf -y --setopt=install_weak_deps=False install \
    wine mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    python3 iproute iputils traceroute bind-utils procps-ng findutils util-linux tar unzip curl \
    systemd glibc-langpack-en libicu openssl-libs krb5-libs zlib coreutils \
    >"${ev}/dnf.log" 2>&1 || { tail -n 60 "${ev}/dnf.log"; exit 1; }

echo "== PowerShell 7 (unpacked from the workflow's download)"
mkdir -p "${work}/pwsh-linux" "${work}/pwsh-win"
tar -xzf "${work}"/dl/powershell-*-linux-x64.tar.gz -C "${work}/pwsh-linux"
chmod +x "${work}/pwsh-linux/pwsh"
unzip -q -o "${work}"/dl/PowerShell-*-win-x64.zip -d "${work}/pwsh-win"

{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}"
    echo "kernel (the runner's): $(uname -r)"
    echo "wine: $(wine --version 2>&1)"
    echo "pwsh (Linux): $("${work}/pwsh-linux/pwsh" -NoLogo -NoProfile -Command '$PSVersionTable.PSVersion.ToString()' 2>&1 | tail -n 1)"
    echo "pwsh (Windows) file: $(ls -l "${work}/pwsh-win/pwsh.exe" | awk '{print $5}') bytes"
    echo "python: $(python3 --version)"
    echo "packages:"
    rpm -q wine wine-core mingw64-gcc iproute traceroute bind-utils systemd libicu | sed 's/^/  /' || true
    pe="$(dirname "$(find /usr/lib64 /usr/lib -name cmd.exe -path '*x86_64-windows*' 2>/dev/null | head -n 1)")"
    echo "wine PE directory: ${pe}"
    echo "wine programs shipped (*.exe, *.com):"
    find "${pe}" -maxdepth 1 \( -name '*.exe' -o -name '*.com' \) -printf '%f ' 2>/dev/null | fold -s -w 110 | sed 's/^/  /'
    echo
    echo "service manager reachable: $(systemctl is-system-running 2>&1 | head -n 1)"
    echo "network interfaces: $(ip -o link show | awk -F': ' '{print $2}' | tr '\n' ' ')"
} | tee "${ev}/versions.txt"

echo "== build the launcher, the bridge and the test program"
for name in turrun turshim hello; do
    x86_64-w64-mingw32-gcc -O2 -Wall -o "${build}/${name}.exe" "${src}/win32/${name}.c" 2>&1 | tee -a "${ev}/build.log"
done
ls -l "${build}" | tee -a "${ev}/build.log"

echo "== a Windows environment to copy"
template="${work}/prefix-template"
WINEPREFIX="${template}" WINEDLLOVERRIDES="mscoree=;mshtml=" wineboot -u >"${ev}/wineboot.log" 2>&1 || true
WINEPREFIX="${template}" wineserver -w || true
mkdir -p "${template}/drive_c/turrun" "${template}/drive_c/turshim"
cp "${build}/turrun.exe" "${template}/drive_c/turrun/"
# TUR's own commands inside the Windows environment: one bridge program
# under each name (see winshell/shims.py for what each does).
shim_names="ipconfig getmac route tracert nslookup tasklist taskkill systeminfo tree tar curl powershell pwsh winget shutdown sfc chkdsk dism format diskpart bcdedit bcdboot bootrec bootsect"
for name in ${shim_names}; do
    cp "${build}/turshim.exe" "${template}/drive_c/turshim/${name}.exe"
done
ls "${template}/drive_c" "${template}/dosdevices" | tee -a "${ev}/versions.txt"

prefix_for() {
    local dir="${work}/p/$1"
    cp -a "${template}" "${dir}"
    if [ "$2" = "plain" ]; then rm -rf "${dir}/drive_c/turshim"; fi
    WINEPREFIX="${dir}" wineserver -p >/dev/null 2>&1 || true
    echo "${dir}"
}

echo "== Windows command names for a Linux shell (candidate bash-shims)"
# Only names that mean nothing on Linux get one: a name Linux already has
# (ping, tar, find, sort, ...) is left alone, and listed.
mkdir -p "${work}/shimbin"
: >"${ev}/bash-shim-collisions.txt"
for name in dir copy del erase ren rename move md rd type cls ver vol title path assoc ftype set call start \
    pushd popd mklink echo cd if for rem ipconfig ping tracert nslookup tasklist taskkill systeminfo where \
    findstr find sort more fc tree xcopy robocopy attrib sc net reg certutil tar curl schtasks shutdown winget \
    getmac arp netstat route wmic setx icacls fsutil chcp subst powershell hostname whoami sfc chkdsk dism cmd \
    format diskpart bcdedit timeout date time exit mkdir rmdir; do
    if bash -c "type ${name}" >/dev/null 2>&1; then
        echo "${name}: $(bash -c "type ${name}" 2>&1 | head -n 1)" >>"${ev}/bash-shim-collisions.txt"
    else
        printf '#!/bin/sh\nexec python3 -I %s --argv %s "$@"\n' "${src}/winshell/tur-winshell" "${name}" >"${work}/shimbin/${name}"
        chmod +x "${work}/shimbin/${name}"
    fi
done
echo "names given a shim: $(ls "${work}/shimbin" | tr '\n' ' ')" | tee -a "${ev}/versions.txt"
echo "names Linux already has: $(cut -d: -f1 "${ev}/bash-shim-collisions.txt" | tr '\n' ' ')" | tee -a "${ev}/versions.txt"

export WINCLI_WINE=wine WINCLI_PWSH="${work}/pwsh-linux/pwsh" WINCLI_PWSH_WIN='Z:\work\pwsh-win\pwsh.exe'
export WINCLI_SHIMBIN="${work}/shimbin" WINCLI_BUILD="${build}"

candidate() {
    local name="$1" kind="$2"
    local prefix
    prefix="$(prefix_for "${name}" "${kind}")"
    WINCLI_PREFIX="${prefix}" python3 -I "${src}/run.py" --candidate "${name}" --out "${ev}" ${only:+--only "${only}"} \
        >"${ev}/${name}.log" 2>&1 || echo "candidate ${name} ended with $?" | tee -a "${ev}/${name}.log"
    WINEPREFIX="${prefix}" wineserver -k >/dev/null 2>&1 || true
    tail -n 3 "${ev}/${name}.log"
}

echo "== candidates"
candidate wine-cmd plain &
candidate wine-pwsh plain &
( candidate pwsh-native plain; candidate pwsh-compat shims ) &
candidate router shims &
wait
candidate bash-shims shims

echo "== integration and safety"
prefix="$(prefix_for integration shims)"
WINCLI_PREFIX="${prefix}" python3 -I "${src}/integration.py" --out "${ev}" >"${ev}/integration.log" 2>&1 \
    || echo "integration ended with $?" | tee -a "${ev}/integration.log"
WINEPREFIX="${prefix}" wineserver -k >/dev/null 2>&1 || true
tail -n 5 "${ev}/integration.log"

# What a Windows environment costs on disk, for the report.
{
    echo "prefix template: $(du -sh "${template}" | cut -f1)"
    echo "PowerShell for Linux, unpacked: $(du -sh "${work}/pwsh-linux" | cut -f1)"
    echo "PowerShell for Windows, unpacked: $(du -sh "${work}/pwsh-win" | cut -f1)"
} | tee -a "${ev}/versions.txt"
chmod -R a+rX "${ev}"

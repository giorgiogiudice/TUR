#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Where do TUR's real applications touch what Wine lacks? In a Fedora 45
# container with Fedora's Wine and a virtual display:
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/windiff/ci/harvest.sh
#
# The applications of tests/windows-runtime/apps.json (7-Zip, Notepad++,
# PuTTY, Inno Setup, VLC) are fetched from their publishers at the pinned
# hashes, installed or unpacked, and each is used briefly with Wine's
# default diagnostics on (tur-runtime runs them with fixme-all, so the
# runtime's own CI logs hold no such lines). harvest.py then counts
#   - the fixme:/err: lines and unimplemented-function aborts, by channel
#     and function;
#   - every function the applications' programs and libraries import, set
#     against the pinned Wine's .spec files: implemented, stub, or absent.
#
# Nothing downloaded is committed or kept; no Microsoft component is
# installed. Green when the summary was written.
set -uo pipefail
. /src/tests/experiments/windiff/ci/wine-common.sh

ev=/work/evidence
h=/work/harvest
mkdir -p "${ev}/harvest" "${h}/dl" "${h}/logs"
dnf -y --setopt=install_weak_deps=False install wine python3 util-linux findutils procps-ng tar gzip unzip curl git-core \
    xorg-x11-server-Xvfb mingw64-binutils >"${ev}/dnf-harvest.log" 2>&1 || { tail -n 40 "${ev}/dnf-harvest.log"; exit 1; }
windiff_user
chown -R tur "${h}" "${ev}/harvest"
wine --version | tee "${ev}/harvest/wine-version.txt"

python3 -I "${windiff_src}/harvest.py" fetch /src/tests/windows-runtime/apps.json "${h}/dl" | tee "${ev}/harvest/fetch.txt"

Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 &
sleep 2
prefix="${h}/prefix"
envs=(HOME=/home/tur WINEPREFIX="${prefix}" LC_ALL=C.UTF-8 DISPLAY=:77 "WINEDLLOVERRIDES=mscoree,mshtml=")
as_tur() { runuser -u tur -- env "${envs[@]}" "$@"; }
# use NAME SECONDS COMMAND...: runs a program for at most SECONDS with
# Wine's default diagnostics and keeps what it printed.
use() {
    local name="$1" secs="$2"; shift 2
    as_tur timeout "${secs}" "$@" >"${h}/logs/${name}.log" 2>&1
    echo "${name}: exit $? ($(grep -caE 'fixme:|err:' "${h}/logs/${name}.log") fixme/err lines)"
}
as_tur env WINEDEBUG=-all timeout 600 wine wineboot -u >/dev/null 2>&1
c="${prefix}/drive_c"
mkdir -p "${c}/work" && chown tur "${c}/work"
printf 'hello windiff\n' >"${c}/work/hello.txt"
python3 -I "${windiff_src}/harvest.py" wav "${c}/work/tone.wav"
chown -R tur "${c}/work"

{
    [ -f "${h}/dl/7zip-exe.exe" ] && {
        use 7zip-install 180 wine "${h}/dl/7zip-exe.exe" /S
        use 7zip-add 120 wine 'C:\Program Files\7-Zip\7z.exe' a 'C:\work\a.7z' 'C:\work\hello.txt'
        use 7zip-zip 120 wine 'C:\Program Files\7-Zip\7z.exe' a -tzip 'C:\work\a.zip' 'C:\work\hello.txt'
        use 7zip-extract 120 wine 'C:\Program Files\7-Zip\7z.exe' x -y '-oC:\work\out' 'C:\work\a.7z'
        use 7zip-list 120 wine 'C:\Program Files\7-Zip\7z.exe' l 'C:\work\a.zip'
        use 7zip-gui 25 wine 'C:\Program Files\7-Zip\7zFM.exe' 'C:\work'
    }
    [ -f "${h}/dl/notepadpp-zip.zip" ] && {
        mkdir -p "${c}/npp" && unzip -q -o "${h}/dl/notepadpp-zip.zip" -d "${c}/npp" && chown -R tur "${c}/npp"
        use notepadpp 30 wine 'C:\npp\notepad++.exe' -multiInst -nosession 'C:\work\hello.txt'
    }
    [ -f "${h}/dl/putty-exe.exe" ] && {
        cp "${h}/dl/putty-exe.exe" "${c}/work/putty.exe"
        use putty 20 wine 'C:\work\putty.exe'
    }
    [ -f "${h}/dl/innosetup.exe" ] && {
        use innosetup-install 300 wine "${h}/dl/innosetup.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-
        iscc="$(find "${c}" -iname ISCC.exe | head -n 1)"
        printf '[Setup]\nAppName=windiff\nAppVersion=1\nDefaultDirName={autopf}\\windiff\nOutputDir=C:\\work\\isout\n[Files]\nSource: "C:\\work\\hello.txt"; DestDir: "{app}"\n' >"${c}/work/t.iss"
        chown tur "${c}/work/t.iss"
        [ -n "${iscc}" ] && use innosetup-compile 240 wine "${iscc}" 'C:\work\t.iss'
        setup="$(find "${c}/work/isout" -name '*.exe' 2>/dev/null | head -n 1)"
        [ -n "${setup}" ] && use innosetup-made-installer 240 wine "${setup}" /VERYSILENT /SUPPRESSMSGBOXES /SP-
    }
    [ -f "${h}/dl/vlc-nsis.exe" ] && {
        use vlc-install 400 wine "${h}/dl/vlc-nsis.exe" /S
        vlc="$(find "${c}" -iname vlc.exe | head -n 1)"
        [ -n "${vlc}" ] && use vlc-play 40 wine "${vlc}" --intf dummy --no-video-title-show --play-and-exit 'C:\work\tone.wav'
        [ -n "${vlc}" ] && use vlc-gui 30 wine "${vlc}" --no-qt-privacy-ask
    }
} 2>&1 | tee "${ev}/harvest/uses.txt"
as_tur timeout 60 wineserver -k || true
pkill Xvfb || true

# Wine's .spec files at the pinned master, for the import census.
specs=/tmp/wine-specs
git init -q "${specs}" && git -C "${specs}" remote add origin "${WINE_REPO}" &&
    git -C "${specs}" sparse-checkout set --no-cone '/dlls/*/*.spec' &&
    git -C "${specs}" fetch -q --depth 1 --filter=blob:none origin "${WINE_MASTER}" && git -C "${specs}" checkout -q FETCH_HEAD ||
    echo "could not fetch Wine's spec files"

python3 -I "${windiff_src}/harvest.py" report "${h}/logs" "${c}" "${specs}" "${ev}/harvest" | tee "${ev}/harvest/summary.md"
tar -C "${h}" -czf "${ev}/harvest/logs.tar.gz" logs
chmod -R a+rX "${ev}"
[ -s "${ev}/harvest/summary.md" ]

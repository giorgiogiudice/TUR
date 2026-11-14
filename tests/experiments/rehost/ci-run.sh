#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# One shard of the re-hosting experiment inside a Fedora 45 container
# (.github/workflows/exp-rehost.yml):
#
#   docker run --privileged --network host --shm-size=2g \
#       -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/rehost/ci-run.sh SHARD
#
# Installs Fedora's packages (the compositor, the accessibility bus, the
# unpacking tools, the libraries Chromium needs, and for the shards that
# need them Fedora's .NET, OpenJDK and Python builds), runs the self-test,
# then harness/run.py as the unprivileged user "tur". Results go to
# /work/out. The container is privileged only so that an unprivileged user
# inside it may create user namespaces (Chromium's own sandbox).
#
# Never run on a development machine (AGENTS.md).
set -uo pipefail

shard=${1:?usage: ci-run.sh SHARD}
src=/src/tests/experiments/rehost
work=/work
out=${work}/out
mkdir -p "${out}" "${work}/cache"

base="python3 python3-gobject python3-pillow at-spi2-core at-spi2-atk dbus-daemon dbus-tools glib2
  sway grim xorg-x11-server-Xwayland mesa-dri-drivers mesa-vulkan-drivers mesa-libgbm mesa-libGL mesa-libEGL
  bsdtar 7zip p7zip p7zip-plugins innoextract msitools squashfs-tools util-linux procps-ng findutils which file
  nss atk cups-libs gtk3 libdrm alsa-lib libXcomposite libXdamage libXrandr libXScrnSaver libxkbcommon pango
  libnotify libsecret libxshmfence libXtst libXcursor libXinerama libXi libX11-xcb xdg-utils pulseaudio-libs
  liberation-fonts dejavu-sans-fonts google-noto-sans-fonts fontconfig ca-certificates"
case "${shard}" in
    n1) extra="dotnet-runtime-8.0 aspnetcore-runtime-8.0 dotnet-runtime-9.0 aspnetcore-runtime-9.0 dotnet-runtime-10.0 aspnetcore-runtime-10.0 libicu sqlite-libs git-core" ;;
    g1) extra="java-21-openjdk java-25-openjdk java-latest-openjdk java-atk-wrapper python3.9 python3.10 python3.11 python3.12 python3.13 python3.14 python3-pip openal-soft SDL2 libglvnd-glx libglvnd-opengl" ;;
    *) extra="" ;;
esac
echo "== packages"
# --skip-unavailable: a package Fedora 45 does not have is a finding (it is
# listed below as not installed), not a failed job.
if ! dnf -y --setopt=install_weak_deps=False --skip-unavailable install ${base} ${extra} > "${out}/dnf-${shard}.log" 2>&1; then
    tail -n 40 "${out}/dnf-${shard}.log"
    echo "::error::dnf could not install the packages"
    exit 1
fi
command -v 7z > /dev/null || { command -v 7zz > /dev/null && ln -s "$(command -v 7zz)" /usr/local/bin/7z; }
{
    . /etc/os-release
    echo "container: ${PRETTY_NAME}; kernel (the runner's): $(uname -r)"
    for name in ${base} ${extra}; do
        rpm -q "${name}" 2> /dev/null || echo "${name}: NOT INSTALLED (not in Fedora ${VERSION_ID}, or under another name)"
    done
    7z i 2> /dev/null | head -n 3
    dotnet --list-runtimes 2> /dev/null
    ls /usr/lib/jvm 2> /dev/null
} > "${out}/versions-${shard}.txt" 2>&1
grep -c "NOT INSTALLED" "${out}/versions-${shard}.txt" | sed 's/^/packages not installed: /'
grep "NOT INSTALLED" "${out}/versions-${shard}.txt" || true

id tur > /dev/null 2>&1 || useradd -m -u 1001 tur
chown -R tur:tur "${work}"
as_tur() { runuser -u tur -- env HOME=/home/tur "$@"; }

echo "== self-test"
if ! as_tur python3 -I "${src}/selftest.py" "${work}/selftest" > "${out}/selftest-${shard}.txt" 2>&1; then
    cat "${out}/selftest-${shard}.txt"
    echo "::warning::the detector's self-test failed (shard ${shard}); the corpus is measured all the same"
fi
tail -n 1 "${out}/selftest-${shard}.txt"
rm -rf "${work}/selftest"

if [ "${shard}" = g1 ]; then
    echo "== Ren'Py example games, packaged by the SDK's own distribute command"
    as_tur python3 -I - "${src}" "${work}" > "${out}/renpy-dist.log" 2>&1 << 'PY' || echo "::warning::the Ren'Py packages were not built (see renpy-dist.log)"
import os, subprocess, sys
src, work = sys.argv[1], sys.argv[2]
sys.path.insert(0, src)
from rehostlib import fetch, rehost
cache = os.path.join(work, "cache")
archive, record = fetch.renpy_sdk("8.5.3", cache)
sdk = os.path.join(cache, "runtimes", "renpy-8.5.3-sdk")
if not os.path.isdir(sdk):
    rehost.extract(archive, sdk + ".tmp")
    os.replace(os.path.join(sdk + ".tmp", os.listdir(sdk + ".tmp")[0]), sdk)
print(record)
env = dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
status = 0
for game in ("the_question", "tutorial"):
    done = subprocess.run([os.path.join(sdk, "renpy.sh"), os.path.join(sdk, "launcher"), "distribute", os.path.join(sdk, game), "--destination", os.path.join(work, "w", "renpy-dist"), "--package", "win", "--package", "mac", "--package", "linux", "--no-update"], env=env, capture_output=True, text=True, timeout=900)
    print(game, "exit", done.returncode, done.stdout[-1500:], done.stderr[-1500:])
    status = status or done.returncode
print(sorted(os.listdir(os.path.join(work, "w", "renpy-dist"))) if os.path.isdir(os.path.join(work, "w", "renpy-dist")) else "no output directory")
sys.exit(status)
PY
    tail -n 3 "${out}/renpy-dist.log"
fi

echo "== corpus, shard ${shard}"
status=0
as_tur env REHOST_BUDGET_SECONDS="${REHOST_BUDGET_SECONDS:-2700}" python3 -I "${src}/harness/run.py" \
    --corpus "${src}/corpus.json" --shard "${shard}" --only "${REHOST_ONLY:-}" \
    --work "${work}/w" --cache "${work}/cache" --out "${out}" ${REHOST_NO_PRETEND:+--no-pretend} \
    2>&1 | tee "${out}/harness-${shard}.log" | grep -E '^(== |  (linux|windows|macos): |    [a-z0-9-]+: |session|Traceback|[A-Za-z]+Error)' || status=$?
python3 -I "${src}/harness/summary.py" "${out}" > "${out}/results-${shard}.md" 2> "${out}/summary-${shard}.err" || cat "${out}/summary-${shard}.err"
rm -rf "${work}/w"
chmod -R a+rX "${out}"
ls "${out}" | head -n 80
exit 0

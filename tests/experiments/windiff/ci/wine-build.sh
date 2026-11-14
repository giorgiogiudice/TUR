#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds Wine's master branch (pinned in wine-common.sh), unpatched, in a
# Fedora 45 container, and the check programs' toolchain with it:
#
#   docker run -v CHECKOUT:/src:ro -v WORK:/work fedora:45@sha256:... \
#       bash /src/tests/experiments/windiff/ci/wine-build.sh
#
# /work/winebuild is kept between workflow runs (actions/cache): src (the
# checkout), build (the object tree), ccache. A tree built from the same
# commit with the same script is reused; make then rebuilds only what a
# patch touches. 64-bit only, no display, no optional libraries: enough to
# run console programs. Wine's own tests are built for the modules the
# patch queue adds tests to (kernel32, kernelbase, ntdll), not for the rest.
set -uo pipefail
. /src/tests/experiments/windiff/ci/wine-common.sh

ev=/work/evidence
w=/work/winebuild
mkdir -p "${ev}" "${w}"
dnf -y --setopt=install_weak_deps=False install \
    gcc make flex bison perl git-core python3 findutils diffutils util-linux procps-ng tar gzip ccache \
    mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt \
    >"${ev}/dnf-build.log" 2>&1 || { tail -n 40 "${ev}/dnf-build.log"; exit 1; }
{
    gcc --version | head -n 1
    x86_64-w64-mingw32-gcc --version | head -n 1
    rpm -q gcc mingw64-gcc mingw64-headers mingw64-crt ccache | sed 's/^/  /'
} | tee "${ev}/toolchain-wine.txt"

export CCACHE_DIR="${w}/ccache" CCACHE_MAXSIZE=2G CCACHE_COMPRESS=1 CCACHE_BASEDIR="${w}"
want="${WINE_MASTER} $(sha256sum /src/tests/experiments/windiff/ci/wine-build.sh | cut -d' ' -f1)"
git config --global --add safe.directory "${w}/src"

if [ "$(cat "${w}/stamp" 2>/dev/null)" = "${want}" ] && [ -x "${w}/build/wine" ]; then
    echo "wine: reusing the cached tree for ${WINE_MASTER}"
    git -C "${w}/src" checkout -q -- . && git -C "${w}/src" clean -fdq
else
    echo "wine: building ${WINE_MASTER} from scratch (ccache: $(ccache -s 2>/dev/null | grep -E 'Hits|hit' | head -n 1))"
    rm -rf "${w}/src" "${w}/build" "${w}/stamp"
    git init -q "${w}/src"
    git -C "${w}/src" remote add origin "${WINE_REPO}"
    git -C "${w}/src" fetch -q --depth 1 origin "${WINE_MASTER}" || exit 1
    git -C "${w}/src" checkout -q FETCH_HEAD
    # Tests only for the modules the patch queue touches.
    sed -i -E 's#^(wine_fn_config_makefile dlls/(kernel32|kernelbase|ntdll)/tests) enable_tests#\1 enable_windiff_tests#' \
        "${w}/src/configure"
    git -C "${w}/src" -c user.name=windiff -c user.email=windiff@invalid commit -q -am "windiff: tests for three modules only"
    mkdir -p "${w}/build"
    (cd "${w}/build" && CC="ccache gcc" x86_64_CC="ccache x86_64-w64-mingw32-gcc" CFLAGS="-O2 -g0" CROSSCFLAGS="-O2 -g0" \
        "${w}/src/configure" --enable-archs=x86_64 --disable-tests --disable-win16 \
        --without-x --without-freetype --without-wayland --without-vulkan --without-opengl --without-gstreamer \
        --without-pulse --without-alsa --without-cups --without-sane --without-gphoto --without-v4l2 --without-krb5 \
        --without-netapi --without-pcap --without-sdl --without-udev --without-dbus --without-capi --without-ffmpeg \
        --without-gssapi --without-inotify --without-fontconfig --without-gnutls --without-xinerama --without-xrandr \
        --without-xshape --without-xxf86vm --without-xinput2 --without-xcomposite --without-xcursor --without-xfixes \
        --without-xinput --without-xrender --without-xshm --without-osmesa --without-opencl --without-usb \
        --without-oss --without-coreaudio --without-pcsclite --without-hwloc \
        >"${ev}/wine-configure.log" 2>&1) || { tail -n 40 "${ev}/wine-configure.log"; exit 1; }
    echo "configure: ok"
fi

start=$(date +%s)
if ! make -C "${w}/build" -j"$(nproc)" >"${ev}/wine-make-baseline.log" 2>&1; then
    grep -E 'error|Error' "${ev}/wine-make-baseline.log" | head -n 60
    exit 1
fi
echo "make (baseline): $(( $(date +%s) - start )) s"
echo "${want}" >"${w}/stamp"
ccache -s 2>/dev/null | head -n 12 >"${ev}/ccache-stats.txt" || true
du -sh "${w}/src" "${w}/build" "${w}/ccache" 2>/dev/null | tee "${ev}/winebuild-size.txt"
"${w}/build/wine" --version 2>&1 | tee "${ev}/wine-version-build.txt" || true
chmod -R a+rX "${w}" "${ev}"

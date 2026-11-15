#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds TUR's desktop programs for the image, against the image's own Qt.
# Runs only in the build stage "desktop" of image/Containerfile, which starts
# from the pinned base image, never on a development machine (AGENTS.md).
#
#   build.sh fetch   with network: the compiler, CMake and the -devel
#                    packages of Qt and of KDE's two libraries, each asked
#                    for in exactly the build the base image has; and the
#                    two font packages of fonts.lock, checked against their
#                    SHA-256. Nothing is compiled.
#   build.sh build   without network: configure desktop/, build the four
#                    programs, install the component "image" under /out,
#                    and check what was built.
#
# What the image gets, under /out (desktop/CMakeLists.txt, "installation"):
#
#     /usr/bin/tur-centre, tur-settings,       the programs
#       tur-pax-ui, tur-pax-overlay
#     /usr/lib64/tur/libtur*.so                the libraries of their QML
#                                              modules (the QML is inside)
#     /usr/share/applications/                 launcher entries
#     /usr/share/dbus-1/services, interfaces   bus activation, interfaces
#     /usr/share/plasma/systemsettings/        TUR's entry in System Settings
#     /usr/share/icons/hicolor/                P@x's icon
#     /usr/lib/tur/desktop-programs.tsv        what was installed
#     /usr/lib/tur/desktop-built-against.tsv   the package builds of every
#                                              library the programs link
#
# and, beside /out, /rpms: the font packages, which the final stage installs.
#
# "The same Qt as the image" (issue #59). Fedora's repositories hold only the
# newest build of a package, and the base is pinned. So the -devel packages
# are asked for by the exact version and release of the base's own
# qt6-qtbase, qt6-qtdeclarative, qt6-qtwayland, layer-shell-qt and kwayland:
# when Fedora has moved on, that request cannot be met and the build stops
# here, saying so, instead of building against another Qt. After the
# installation every package of Qt and KDE that the base has must still be
# the build the base has. After the build, the package build of every
# library the programs link is written down, and the image's smoke test
# holds the image to that list.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
src=/src
build_root=/build
out=/out
rpms=/rpms
tab="$(printf '\t')"

# The packages whose -devel build must be the base's build of the library.
matched="qt6-qtbase qt6-qtdeclarative qt6-qtwayland layer-shell-qt kwayland"
# The programs of the component "image".
programs="tur-centre tur-settings tur-pax-ui tur-pax-overlay"

evr() { rpm -q --qf '%{VERSION}-%{RELEASE}' "$1"; }

fetch() {
    mkdir -p "${build_root}" "${rpms}"
    rpm -qa --qf '%{NAME}\t%{EVR}\n' | grep -v '^gpg-pubkey' | LC_ALL=C sort > "${build_root}/base-packages.tsv"
    echo "== the base's Qt and KDE libraries the programs are built against"
    wanted="cmake ninja-build gcc-c++ extra-cmake-modules"
    for package in ${matched}; do
        version="$(evr "${package}")"
        echo "${package} ${version}"
        wanted="${wanted} ${package}-devel-${version}"
    done
    wanted="${wanted} qt6-qtbase-private-devel-$(evr qt6-qtbase)"
    echo "== dnf install ${wanted}"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    if ! dnf -y --setopt=install_weak_deps=False install ${wanted}; then
        echo "The -devel packages of the base's own Qt and KDE libraries could not be installed." >&2
        echo "Fedora's repositories hold only the newest build of a package: when they have" >&2
        echo "moved past the pinned base (image/base.lock), the matching -devel build is gone." >&2
        echo "Refresh the base pin (scripts/update-base-pin.sh), in a pull request of its own." >&2
        exit 1
    fi
    rpm -qa --qf '%{NAME}\t%{EVR}\n' | grep -v '^gpg-pubkey' | LC_ALL=C sort > "${build_root}/stage-packages.tsv"
    # Every package the base has, as the base has it and as it is now.
    LC_ALL=C join -t "${tab}" "${build_root}/base-packages.tsv" "${build_root}/stage-packages.tsv" \
        | awk -F'\t' '$2 != $3' > "${build_root}/changed-packages.tsv"
    echo "== packages of the base that the installation replaced with another build"
    cat "${build_root}/changed-packages.tsv"
    if grep -E "^(qt6-|kf6-|layer-shell-qt|kwayland|kwin|plasma-|libplasma)" "${build_root}/changed-packages.tsv"; then
        echo "The installation replaced Qt or KDE packages of the base (above): the programs would" >&2
        echo "be built against another build than the image has. Refresh the base pin." >&2
        exit 1
    fi
    for package in ${matched}; do
        if [ "$(evr "${package}-devel")" != "$(evr "${package}")" ]; then
            echo "${package}-devel is $(evr "${package}-devel"), ${package} is $(evr "${package}")" >&2
            exit 1
        fi
    done
    echo "== build tools"
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' cmake ninja-build gcc-c++ extra-cmake-modules \
        qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtwayland-devel layer-shell-qt-devel kwayland-devel

    echo "== the fonts of image/desktop/fonts.lock"
    grep -v '^#' "${here}/fonts.lock" | while IFS="${tab}" read -r file sha256 licence path; do
        if [ -z "${file}" ]; then continue; fi
        got=""
        for mirror in https://dl.fedoraproject.org/pub/fedora/linux/releases/45 \
            https://download.fedoraproject.org/pub/fedora/linux/releases/45 \
            https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/45; do
            if curl --fail --silent --show-error --location --retry 3 --max-time 120 \
                -o "${rpms}/${file}" "${mirror}/${path}"; then
                got="${mirror}"
                break
            fi
        done
        if [ -z "${got}" ]; then
            echo "could not download ${file}" >&2
            exit 1
        fi
        echo "${sha256}  ${rpms}/${file}" | sha256sum -c -
        echo "${file} (${licence}) from ${got}"
    done
}

build() {
    # The time Qt's resource compiler writes into the QML it packs: the
    # commit's, so that two builds of one commit agree and a new image's
    # programs do not pass for the old one's (Qt's QML cache goes by it).
    if [ "${TUR_SOURCE_DATE_EPOCH:-0}" != 0 ]; then
        SOURCE_DATE_EPOCH="${TUR_SOURCE_DATE_EPOCH}"
        export SOURCE_DATE_EPOCH
    fi
    flags="-ffile-prefix-map=${src}/desktop=desktop -ffile-prefix-map=${build_root}/desktop=build"
    echo "== configure"
    cmake -S "${src}/desktop" -B "${build_root}/desktop" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=/usr \
        -DCMAKE_INSTALL_LIBDIR=lib64 \
        -DCMAKE_CXX_FLAGS="${flags}" < /dev/null
    echo "== build: ${programs}"
    started="$(date +%s)"
    # shellcheck disable=SC2086  # a list of targets, split on purpose
    cmake --build "${build_root}/desktop" --parallel "$(nproc)" --target ${programs} < /dev/null
    echo "== built in $(( $(date +%s) - started )) s"
    DESTDIR="${out}" cmake --install "${build_root}/desktop" --component image --strip < /dev/null

    echo "== installed"
    (cd "${out}" && find . \( -type f -o -type l \) | LC_ALL=C sort | sed 's|^\./|/|') > "${build_root}/installed.txt"
    cat "${build_root}/installed.txt"
    du -sh "${out}" | cut -f1
    for program in ${programs}; do
        test -x "${out}/usr/bin/${program}" || { echo "not installed: ${program}" >&2; exit 1; }
    done
    for file in applications/org.tur.Centre.desktop applications/org.tur.Settings.desktop \
        applications/org.tur.PaxUi.desktop dbus-1/services/org.tur.Settings1.service \
        dbus-1/services/org.tur.PaxUi1.service dbus-1/services/org.tur.PaxOverlay1.service \
        plasma/systemsettings/externalmodules/org.tur.Settings.desktop \
        icons/hicolor/scalable/apps/org.tur.Pax.svg; do
        test -s "${out}/usr/share/${file}" || { echo "not installed: /usr/share/${file}" >&2; exit 1; }
    done

    # Every program starts from where it was installed, loads its QML with no
    # warning and finds its modules in its own libraries, with no import path
    # set. The run path names /usr/lib64/tur, which does not exist in this
    # stage, so the loader is told where the libraries are.
    echo "== the installed programs start (offscreen)"
    mkdir -p "${build_root}/run"
    chmod 0700 "${build_root}/run"
    check() {
        echo "-- $*"
        env -i PATH=/usr/bin HOME="${build_root}/run" XDG_RUNTIME_DIR="${build_root}/run" LC_ALL=C.UTF-8 \
            QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software LD_LIBRARY_PATH="${out}/usr/lib64/tur" \
            "$@" < /dev/null
    }
    for program in ${programs}; do
        check "${out}/usr/bin/${program}" --version
    done
    check "${out}/usr/bin/tur-centre" --standalone --check --fixture "${src}/desktop/apps/tur-centre/fixtures/default"
    check "${out}/usr/bin/tur-settings" --standalone --check --fixture "${src}/desktop/apps/tur-settings/fixtures/default"
    check "${out}/usr/bin/tur-pax-ui" --standalone --check --no-dbus --mode ask
    check "${out}/usr/bin/tur-pax-ui" --standalone --check --no-dbus --mode setup
    check "${out}/usr/bin/tur-pax-overlay" --check --no-dbus

    # What was installed, for the smoke test and the SBOM: program, path,
    # version.
    mkdir -p "${out}/usr/lib/tur"
    {
        echo "# TUR's desktop programs in this image. Written by the image build"
        echo "# (image/desktop/build.sh). Fields, separated by tabs: program, path, version."
        for program in ${programs}; do
            version="$(check "${out}/usr/bin/${program}" --version | tail -n 1 | awk '{ print $NF }')"
            printf '%s\t%s\t%s\n' "${program}" "/usr/bin/${program}" "${version}"
        done
    } > "${out}/usr/lib/tur/desktop-programs.tsv"
    cat "${out}/usr/lib/tur/desktop-programs.tsv"

    # The package build of every library the programs and their libraries
    # link, in this stage. The image must have the same builds (the smoke
    # test compares). A library that is not a package's is one of TUR's own.
    echo "== what the programs link"
    find "${out}/usr/bin" "${out}/usr/lib64/tur" -type f | LC_ALL=C sort > "${build_root}/elf.txt"
    : > "${build_root}/libraries.txt"
    while read -r file; do
        env LD_LIBRARY_PATH="${out}/usr/lib64/tur" ldd "${file}" > "${build_root}/ldd.txt"
        if grep 'not found' "${build_root}/ldd.txt"; then
            echo "${file}: a library was not found" >&2
            exit 1
        fi
        awk '$3 ~ /^\// { print $3 }' "${build_root}/ldd.txt" >> "${build_root}/libraries.txt"
    done < "${build_root}/elf.txt"
    # The files themselves, behind any link (/lib64 is a link to /usr/lib64).
    grep -v "^${out}/" "${build_root}/libraries.txt" | xargs -r readlink -f | LC_ALL=C sort -u \
        > "${build_root}/system-libraries.txt"
    {
        echo "# The package builds of the libraries TUR's desktop programs were linked against,"
        echo "# in the build stage (image/desktop/build.sh). The image must hold the same builds."
        echo "# Fields, separated by tabs: package, version-release."
        xargs -r rpm -qf --qf '%{NAME}\t%{VERSION}-%{RELEASE}\n' < "${build_root}/system-libraries.txt" | LC_ALL=C sort -u
    } > "${out}/usr/lib/tur/desktop-built-against.tsv"
    cat "${out}/usr/lib/tur/desktop-built-against.tsv"
    if grep -v '^#' "${out}/usr/lib/tur/desktop-built-against.tsv" | grep -v "${tab}"; then
        echo "a library the programs link belongs to no package (above)" >&2
        exit 1
    fi
    for package in qt6-qtbase qt6-qtdeclarative layer-shell-qt kwayland; do
        grep -q "^${package}${tab}$(evr "${package}")\$" "${out}/usr/lib/tur/desktop-built-against.tsv" || {
            echo "the programs do not link ${package} $(evr "${package}")" >&2
            exit 1
        }
    done
    # The licence boundary, on what is installed: every library a program or
    # library needs must be allowed in desktop/ALLOWED_IMPORTS (none of the
    # GPL-only Qt modules, no widget library, whose QApplication would load
    # the GPL Breeze widget style). The same check the desktop workflow runs
    # on its own build.
    python3 -B "${src}/desktop/ci/boundary.py" --linked "${out}/usr"
    chmod -R a+rX "${out}"
}

case "${1:-}" in
    fetch) fetch ;;
    build) build ;;
    *)
        echo "usage: build.sh fetch|build" >&2
        exit 2
        ;;
esac

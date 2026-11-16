#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds TUR's Darling runtime archive from source, at the commit pinned in
# darling.lock, with TUR's patch queue (third-party/darling/patches) applied,
# on the OS image's pinned base. Never run on a development machine
# (AGENTS.md): CI runs it, in .github/workflows/darling-runtime.yml.
#
#   build-darling.sh image OUT     on the CI runner: podman build of
#                                  scripts/darling/Containerfile; OUT gets
#                                  darling-runtime.tar.xz and build-record.json
#   build-darling.sh fetch         inside the build, with network
#   build-darling.sh build         inside the build, without network
#
# What OUT holds:
#
#   darling-runtime.tar.xz   opt/tur-darling/...: Darling's launcher and
#                            server, its guest system under libexec/darling,
#                            and share/licenses/darling with every licence
#                            file of Darling and its submodules, SOURCE.txt
#                            (where the corresponding source is) and
#                            PATCHES.txt (TUR's queue as applied)
#   build-record.json        what was built from what: commits, the queue's
#                            SHA-256, the base, the archive's SHA-256 and
#                            sizes, what was removed and why, and the check of
#                            the host libraries it needs against the base
#
# Removed from the build before it is archived, because TUR never ships Apple
# binaries, SDK headers or Apple source (AGENTS.md):
#
#   - libexec/darling/usr/lib/swift: the Swift runtime, which Darling takes
#     unmodified from Apple's swift.org package (darling-swift README);
#   - headers, module maps, Swift interfaces, text stubs (.tbd), static
#     libraries and any Developer/ or *.sdk directory;
#   - applications in the guest's /Applications (Darling builds Apple's
#     TextEdit sample there).
#
# check-tree.py then refuses the archive if anything Apple-signed, any header
# or SDK, or the Swift runtime is still in it, if anything left links the
# Swift runtime, or if a host library the build needs is not in the base.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/darling/darling.lock
. "${here}/darling.lock"

source_dir=/work/darling
build_dir=/build
stage=/stage
out=/out
meta=/work/meta
queue_dir=/src/third-party/darling/patches

# The build tools, from the base's own repositories (Fedora 45). The list is
# Darling's own for Fedora 44 (build instructions, read 16 November 2026)
# without the 32-bit packages, with the FFmpeg libraries Fedora ships
# (ffmpeg-free), Ninja, ccache and the tools the checks use.
packages="make cmake ninja-build clang lld llvm llvm-devel gcc gcc-c++ bison flex git git-lfs
 python3 xz ccache file binutils glibc-devel libstdc++-devel dbus-devel fuse-devel systemd-devel
 elfutils-libelf-devel cairo-devel freetype-devel libjpeg-turbo-devel fontconfig-devel
 libglvnd-devel mesa-libGL-devel mesa-libEGL-devel mesa-libGLU-devel libtiff-devel libxml2-devel
 libbsd-devel libXcursor-devel libXrandr-devel libxkbfile-devel giflib-devel
 pulseaudio-libs-devel openssl-devel libcap-devel libcurl-devel libedit-devel
 libavcodec-free-devel libavformat-free-devel libavutil-free-devel libswresample-free-devel
 vulkan-loader-devel"

image() {
    local dest
    dest="$(mkdir -p "$1" && cd "$1" && pwd)"
    local root
    root="$(cd "${here}/../.." && pwd)"
    # shellcheck source=image/base.lock
    . "${root}/image/base.lock"
    local ccache="${DARLING_CCACHE:-${HOME}/.ccache-darling}"
    mkdir -p "${ccache}"
    local podman=(podman)
    [ -n "${DARLING_PODMAN_ROOT:-}" ] && podman=(sudo podman --root "${DARLING_PODMAN_ROOT}")
    "${podman[@]}" build \
        --file "${root}/scripts/darling/Containerfile" \
        --build-arg "BASE_IMAGE=${BASE_IMAGE}" \
        --build-arg "BASE_DIGEST=${BASE_DIGEST}" \
        --build-arg "DARLING_ALLOW_MISSING_HOST_LIBS=${DARLING_ALLOW_MISSING_HOST_LIBS:-0}" \
        --volume "${ccache}:/ccache:z" \
        --output "type=local,dest=${dest}" \
        "${root}"
    ls -l "${dest}"
}

fetch() {
    mkdir -p "${meta}"
    # What the base had before anything was added: the check of the host
    # libraries holds the build to these.
    rpm -qa --qf '%{NAME}\n' | sort -u > "${meta}/base-packages.txt"
    . /usr/lib/os-release
    echo "${VERSION_ID}" > "${meta}/fedora-release"

    echo "== build tools, from the base image's repositories"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False --skip-unavailable install ${packages} \
        2>&1 | tee "${meta}/dnf.log" | tail -n 5
    : > "${meta}/build-packages.txt"
    for package in ${packages}; do
        rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' "${package}" \
            >> "${meta}/build-packages.txt" 2>&1 || echo "${package} NOT INSTALLED" >> "${meta}/build-packages.txt"
    done
    cat "${meta}/build-packages.txt"

    echo "== Darling ${DARLING_COMMIT}"
    git lfs install --system
    export GIT_CLONE_PROTECTION_ACTIVE=false
    git init -q "${source_dir}"
    git -C "${source_dir}" remote add origin "${DARLING_REPO}"
    git -C "${source_dir}" fetch -q --depth 1 origin "${DARLING_COMMIT}"
    git -C "${source_dir}" checkout -q FETCH_HEAD
    test "$(git -C "${source_dir}" rev-parse HEAD)" = "${DARLING_COMMIT}"
    git -C "${source_dir}" submodule update --init --recursive --depth 1 --jobs 8
    git -C "${source_dir}" submodule foreach --quiet --recursive 'git lfs pull 2>/dev/null || true'
    git -C "${source_dir}" submodule status --recursive > "${meta}/submodules.txt"
    git -C "${source_dir}" log -1 --format=%ct > "${meta}/epoch"
    echo "submodules: $(wc -l < "${meta}/submodules.txt")"
}

# The SHA-256 of the queue as applied: the series file, then every patch it
# names, in order.
queue_digest() {
    {
        cat "${queue_dir}/series"
        while read -r sub patch; do
            case "${sub}" in ''|\#*) continue ;; esac
            cat "${queue_dir}/${patch}"
        done < "${queue_dir}/series"
    } | sha256sum | cut -d' ' -f1
}

apply_queue() {
    : > "${meta}/queue-apply.txt"
    while read -r sub patch; do
        case "${sub}" in ''|\#*) continue ;; esac
        if git -C "${source_dir}/${sub}" apply --index "${queue_dir}/${patch}" 2>>"${meta}/queue-apply.txt"; then
            echo "applied ${sub} ${patch}" >> "${meta}/queue-apply.txt"
        else
            echo "FAILED  ${sub} ${patch}" >> "${meta}/queue-apply.txt"
            cat "${meta}/queue-apply.txt"
            exit 1
        fi
    done < "${queue_dir}/series"
    cat "${meta}/queue-apply.txt"
}

# Licence files of Darling and of every submodule, and where the source is.
licences() {
    local dest="$1"
    mkdir -p "${dest}"
    cp /src/third-party/darling/README.md "${dest}/TUR-README.md"
    (
        cd "${source_dir}"
        {
            echo ./
            git submodule foreach --quiet --recursive 'echo "$displaypath"'
        } | while read -r path; do
            local name
            name="$(echo "${path%/}" | tr '/' '_')"
            [ "${name}" = "." ] && name=darling
            for file in "${path}"LICENSE* "${path}"LICENCE* "${path}"COPYING* "${path}"APPLE_LICENSE* \
                        "${path}"NOTICE* "${path}"/LICENSE* "${path}"/COPYING* "${path}"/APPLE_LICENSE*; do
                [ -f "${file}" ] || continue
                mkdir -p "${dest}/${name}"
                cp "${file}" "${dest}/${name}/"
            done
        done
    )
    {
        echo "TUR's build of Darling"
        echo
        echo "Darling: ${DARLING_REPO}"
        echo "Commit:  ${DARLING_COMMIT}"
        echo "TUR's patch queue (SHA-256 $(queue_digest)): third-party/darling/patches in"
        echo "         https://github.com/giorgiogiudice/TUR, applied in the order of its series file"
        echo
        echo "The corresponding source of this build is Darling's repository at the commit above,"
        echo "with the submodules at the commits listed below, and TUR's patch queue. Darling's"
        echo "licence is the GNU General Public License, version 3 or later; its components carry"
        echo "the licences in this directory (MIT, LGPL-2.1, APSL-2.0, BSD and others)."
        echo
        echo "Submodules:"
        cat "${meta}/submodules.txt"
    } > "${dest}/SOURCE.txt"
    cp "${queue_dir}/series" "${dest}/PATCHES.txt"
}

build() {
    local prefix="${DARLING_INSTALL_PREFIX}"
    local tree="${stage}${prefix}"
    mkdir -p "${out}"
    apply_queue

    export CCACHE_DIR=/ccache
    ccache --max-size=4G >/dev/null || true
    ccache --zero-stats >/dev/null || true
    local started
    started="$(date +%s)"
    # shellcheck disable=SC2086  # options, split on purpose
    cmake -S "${source_dir}" -B "${build_dir}" -G Ninja \
        -DCMAKE_INSTALL_PREFIX="${prefix}" \
        -DCOMPONENTS="${DARLING_COMPONENTS}" \
        ${DARLING_CMAKE_OPTIONS}
    ninja -C "${build_dir}" -j "$(nproc)"
    DESTDIR="${stage}" ninja -C "${build_dir}" install
    local finished
    finished="$(date +%s)"
    ccache --show-stats > "${meta}/ccache.txt" 2>&1 || true

    # What TUR does not ship (see the top of this file).
    : > "${meta}/removed.txt"
    remove() {
        local why="$1"
        shift
        for path in "$@"; do
            [ -e "${path}" ] || [ -L "${path}" ] || continue
            echo "${path#"${stage}"}	${why}" >> "${meta}/removed.txt"
            rm -rf "${path}"
        done
    }
    remove "Apple's Swift runtime from swift.org: an Apple binary" "${tree}/libexec/darling/usr/lib/swift"
    while IFS= read -r -d '' path; do
        remove "an application in the guest's /Applications (Apple sample code)" "${path}"
    done < <(find "${tree}/libexec/darling/Applications" -mindepth 1 -maxdepth 1 -print0 2>/dev/null)
    while IFS= read -r -d '' path; do
        remove "an SDK or developer directory" "${path}"
    done < <(find "${tree}" -depth \( -name Developer -o -name '*.sdk' \) -type d -print0)
    while IFS= read -r -d '' path; do
        remove "headers, interfaces and stubs are not needed to run programs" "${path}"
    done < <(find "${tree}" \( -name '*.h' -o -name '*.hpp' -o -name '*.modulemap' \
        -o -name '*.swiftinterface' -o -name '*.swiftmodule' -o -name '*.swiftdoc' \
        -o -name '*.tbd' -o -name '*.a' \) \( -type f -o -type l \) -print0)
    find "${tree}" -depth -type d -empty -delete
    # The launcher and the server run unprivileged: no set-user-ID bit.
    chmod -R u-s,g-s "${tree}"

    licences "${tree}/share/licenses/darling"

    local allow=()
    [ "${DARLING_ALLOW_MISSING_HOST_LIBS:-0}" = 1 ] && allow=(--allow-missing-host-libraries)
    python3 -I /src/scripts/darling/check-tree.py \
        --tree "${tree}" \
        --base-packages "${meta}/base-packages.txt" \
        --record "${meta}/check.json" "${allow[@]}"

    local epoch
    epoch="$(cat "${meta}/epoch")"
    tar --sort=name --mtime="@${epoch}" --owner=0 --group=0 --numeric-owner \
        --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
        -C "${stage}" -cf - "${prefix#/}" | xz -T1 -6 > "${out}/darling-runtime.tar.xz"

    local digest size unpacked queue
    digest="$(sha256sum "${out}/darling-runtime.tar.xz" | cut -d' ' -f1)"
    size="$(stat -c %s "${out}/darling-runtime.tar.xz")"
    unpacked="$(du -sb "${tree}" | cut -f1)"
    queue="$(queue_digest)"
    python3 - "${meta}" "${out}/build-record.json" <<EOF
import json, pathlib, sys
meta = pathlib.Path(sys.argv[1])
read = lambda name: (meta / name).read_text() if (meta / name).exists() else ""
release = read("fedora-release").strip()
record = {
    "schema_version": 1,
    "darling_repo": "${DARLING_REPO}",
    "darling_commit": "${DARLING_COMMIT}",
    "components": "${DARLING_COMPONENTS}",
    "cmake_options": "${DARLING_CMAKE_OPTIONS}",
    "install_prefix": "${prefix}",
    "queue_sha256": "${queue}",
    "queue_applied": read("queue-apply.txt").splitlines(),
    "fedora_release": release,
    "base_image": "${BASE_IMAGE:-}",
    "base_digest": "${BASE_DIGEST:-}",
    "sha256": "${digest}",
    "size": ${size},
    "unpacked_size": ${unpacked},
    "build_seconds": $((finished - started)),
    "submodules": read("submodules.txt").splitlines(),
    "build_packages": read("build-packages.txt").splitlines(),
    "removed": [line.split("\t") for line in read("removed.txt").splitlines()],
    "check": json.loads(read("check.json") or "{}"),
    "ccache": read("ccache.txt").splitlines()[-12:],
    "id": "darling-${DARLING_COMMIT:0:12}-q${queue:0:8}-fc" + release + "-x86_64",
    "tag": "${DARLING_COMMIT:0:12}-q${queue:0:8}-fc" + release + "-x86_64",
}
pathlib.Path(sys.argv[2]).write_text(json.dumps(record, indent=1) + "\n")
print(json.dumps({k: record[k] for k in ("id", "sha256", "size", "unpacked_size", "build_seconds")}))
EOF
    # Only the archive and its record leave this step.
    rm -rf "${build_dir}" "${stage}" "${source_dir}"
}

case "${1:-}" in
    image) image "${2:?OUT}" ;;
    fetch) fetch ;;
    build) build ;;
    *) echo "usage: $0 image OUT | fetch | build" >&2; exit 2 ;;
esac

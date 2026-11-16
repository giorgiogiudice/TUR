#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds speech recognition for P@x's voice, whisper.cpp's whisper-cli, from
# its source at the version pinned in whisper-cpp.lock. Runs only in the
# build stage "speech" of image/Containerfile, never on a development machine
# (AGENTS.md).
#
#   whisper-cpp.sh fetch   with network: the compilers and the Vulkan headers
#                          from the base image's own repositories, and the
#                          source archive, checked against its SHA-256.
#   whisper-cpp.sh build   without network: compile whisper-cli with every
#                          processor variant ggml has for x86-64, chosen at
#                          run time, and the Vulkan backend; no HIP/ROCm, no
#                          CUDA, no ffmpeg, no SDL. Put under /out:
#
#     /usr/libexec/tur/whisper.cpp/       whisper-cli, its libraries and the
#                                         backends it loads; every file finds
#                                         the others beside it ($ORIGIN)
#     /usr/bin/whisper-cli                a link to it
#     /usr/share/licenses/whisper.cpp/    the licence and notices
#     /usr/lib/tur/built-from-source.d/whisper.cpp.tsv
#                                         one line for built-from-source.tsv,
#                                         added to it by the final stage
#
# The same pattern as llama-cpp.sh (image/README.md, "The local AI engine").
# TUR runs whisper-cli once per utterance (tur-ai-local speech stt);
# whisper-server is not built: it has no Unix socket, no access key, and its
# /load takes any path (docs/architecture/pax-voice.md, section 3.1).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=image/engine/whisper-cpp.lock
. "${here}/whisper-cpp.lock"

build_root=/build
source_dir="${build_root}/whisper.cpp"
out=/out
prefix=/usr/libexec/tur/whisper.cpp
licences=/usr/share/licenses/whisper.cpp

packages="gcc gcc-c++ cmake ninja-build glslc vulkan-headers vulkan-loader-devel spirv-headers-devel"

fetch() {
    echo "== build tools, from the base image's repositories"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False install ${packages}
    # shellcheck disable=SC2086
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' ${packages} binutils glibc-devel libstdc++-devel \
        vulkan-loader glibc libstdc++ libgomp

    echo "== whisper.cpp ${WHISPER_CPP_VERSION}: ${WHISPER_CPP_URL}"
    mkdir -p "${source_dir}"
    curl -fsSL --retry 4 --retry-delay 5 -o "${build_root}/source.tar.gz" "${WHISPER_CPP_URL}"
    echo "${WHISPER_CPP_SHA256}  ${build_root}/source.tar.gz" | sha256sum -c -
    tar -xzf "${build_root}/source.tar.gz" -C "${source_dir}" --strip-components=1 --no-same-owner
    rm -f "${build_root}/source.tar.gz"
}

build() {
    cd "${source_dir}"
    jobs="$(nproc)"
    echo "== configure (${jobs} processors)"
    # As for llama.cpp: no native tuning, one processor backend per x86-64
    # level loaded at run time, the Vulkan backend, libraries found beside
    # the program. Examples are needed for whisper-cli; only that target is
    # built. No ffmpeg (tur-ai-local gives it 16 kHz mono WAVE files) and no
    # SDL (it never opens a microphone).
    # shellcheck disable=SC2016
    cmake -S . -B build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_FLAGS="-ffile-prefix-map=${source_dir}=whisper.cpp" \
        -DCMAKE_CXX_FLAGS="-ffile-prefix-map=${source_dir}=whisper.cpp" \
        -DBUILD_SHARED_LIBS=ON \
        -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
        -DCMAKE_INSTALL_RPATH='$ORIGIN' \
        -DGGML_NATIVE=OFF \
        -DGGML_BACKEND_DL=ON \
        -DGGML_CPU_ALL_VARIANTS=ON \
        -DGGML_BACKEND_DIR="${prefix}" \
        -DGGML_VULKAN=ON \
        -DGGML_OPENMP=ON \
        -DGGML_CCACHE=OFF \
        -DWHISPER_BUILD_TESTS=OFF \
        -DWHISPER_BUILD_EXAMPLES=ON \
        -DWHISPER_BUILD_SERVER=OFF \
        -DWHISPER_FFMPEG=OFF \
        -DWHISPER_SDL2=OFF < /dev/null
    echo "== build whisper-cli, its libraries and every backend"
    started="$(date +%s)"
    cmake --build build --target whisper-cli --parallel "${jobs}" < /dev/null
    echo "== built in $(( $(date +%s) - started )) s"

    mkdir -p "${out}${prefix}" "${out}/usr/bin" "${out}${licences}" "${out}/usr/lib/tur/built-from-source.d"
    cp -a build/bin/whisper-cli "${out}${prefix}/"
    find build/bin build/src build/ggml -maxdepth 2 -name '*.so*' \( -type f -o -type l \) \
        -exec cp -a {} "${out}${prefix}/" \;
    find "${out}${prefix}" -type f -exec strip --strip-unneeded {} \;
    chmod 0755 "${out}${prefix}"/*
    ln -s ../libexec/tur/whisper.cpp/whisper-cli "${out}/usr/bin/whisper-cli"

    echo "== what it is"
    ls -la "${out}${prefix}"
    du -sh "${out}${prefix}" | cut -f1
    missing="$(find "${out}${prefix}" -type f -exec ldd {} \; 2>&1 | grep 'not found' || true)"
    if [ -n "${missing}" ]; then
        echo "libraries not found: ${missing}" >&2
        exit 1
    fi
    for name in cpu-x64 cpu-haswell cpu-skylakex cpu-alderlake vulkan; do
        test -s "${out}${prefix}/libggml-${name}.so" || { echo "the build has no libggml-${name}.so" >&2; exit 1; }
    done
    test -s "${out}${prefix}/libwhisper.so" || ls "${out}${prefix}"/libwhisper.so.* >/dev/null

    licences_of
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        whisper.cpp "${WHISPER_CPP_VERSION}" "${WHISPER_CPP_LICENCE}" "${WHISPER_CPP_URL}" "${WHISPER_CPP_SHA256}" \
        "${prefix}" "${licences}" > "${out}/usr/lib/tur/built-from-source.d/whisper.cpp.tsv"
    cat "${out}/usr/lib/tur/built-from-source.d/whisper.cpp.tsv"
}

# The licence and the notices of what whisper-cli compiles in from the
# examples' common code, as the source carries them. A header that is not
# there in this version is not looked for further; the list says what was
# found.
licences_of() {
    dest="${out}${licences}"
    install -m 0644 LICENSE "${dest}/LICENSE"
    found=""
    for header in examples/miniaudio.h examples/dr_wav.h examples/stb_vorbis.c; do
        if [ -f "${header}" ]; then
            name="$(basename "${header}")"
            # The licence block sits at the end of these single-file
            # libraries, after "This software is available as a choice of
            # the following licenses" (miniaudio, dr_wav) or "This software
            # is available under 2 licenses" (stb). GNU sed's \| is meant.
            sed -n '/This software is available \(as a choice of the following licenses\|under 2 licenses\)/,$p' \
                "${header}" > "${dest}/LICENSE-${name}"
            if [ ! -s "${dest}/LICENSE-${name}" ]; then
                rm -f "${dest}/LICENSE-${name}"
                echo "no licence block found in ${header}" >&2
                exit 1
            fi
            found="${found} ${name}"
        fi
    done
    {
        echo "The licences of what is compiled into whisper.cpp's whisper-cli in TUR's image,"
        echo "built from ${WHISPER_CPP_URL}"
        echo "(SHA-256 ${WHISPER_CPP_SHA256}, commit ${WHISPER_CPP_COMMIT})."
        echo
        echo "LICENSE                  whisper.cpp and ggml (MIT)"
        for name in ${found}; do
            echo "LICENSE-${name}  examples/${name} (public domain or MIT/MIT-0, as the file says), from the file's end"
        done
    } > "${dest}/README"
    chmod 0644 "${dest}"/*
    ls -la "${dest}"
}

case "${1:-}" in
    fetch) fetch ;;
    build) build ;;
    *)
        echo "usage: whisper-cpp.sh fetch|build" >&2
        exit 2
        ;;
esac

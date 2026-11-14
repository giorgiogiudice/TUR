#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the engine of TUR's local AI, llama.cpp's llama-server, from its
# source at the version pinned in llama-cpp.lock. Runs only in the build stage
# "engine" of image/Containerfile, never on a development machine (AGENTS.md).
#
#   llama-cpp.sh fetch   with network: the compilers and the Vulkan headers
#                        from the base image's own package repositories, and
#                        the source archive, checked against its SHA-256.
#                        Nothing is compiled.
#   llama-cpp.sh build   without network: compile llama-server with every
#                        processor variant llama.cpp has for x86-64, chosen
#                        at run time, and the Vulkan backend; no HIP/ROCm and
#                        no CUDA. Put under /out what the image gets:
#
#     /usr/libexec/tur/llama.cpp/       llama-server, its libraries and the
#                                       backends it loads (libggml-cpu-*.so,
#                                       libggml-vulkan.so); every file finds
#                                       the others beside it ($ORIGIN)
#     /usr/bin/llama-server             a link to it: where tur-ai-local
#                                       starts the engine
#     /usr/share/licenses/llama.cpp/    the licence and notices of everything
#                                       compiled in
#     /usr/lib/tur/built-from-source.tsv  what was built, from what, under
#                                       which licence (the SBOM reads it)
#
# Why TUR builds it rather than installing Fedora's llama-cpp package: Fedora
# 45's build has no AVX2/FMA kernels, no Vulkan and requires ROCm (5.3 GB);
# measured 5.5 to 7.7 times slower generation (docs/architecture/local-ai.md,
# section 6; issue #74). See image/README.md, "The local AI engine".
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=image/engine/llama-cpp.lock
. "${here}/llama-cpp.lock"

build_root=/build
source_dir="${build_root}/llama.cpp"
out=/out
prefix=/usr/libexec/tur/llama.cpp
licences=/usr/share/licenses/llama.cpp
apache="${here}/../third-party-licensed/llama.cpp/Apache-2.0.txt"

# What the build needs from the base's repositories. The image itself gets
# none of it: only /out is copied out of this stage.
packages="gcc gcc-c++ cmake ninja-build glslc vulkan-headers vulkan-loader-devel spirv-headers-devel"

fetch() {
    echo "== build tools, from the base image's repositories"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False install ${packages}
    # shellcheck disable=SC2086
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' ${packages} binutils glibc-devel libstdc++-devel \
        vulkan-loader glibc libstdc++ libgomp

    echo "== llama.cpp ${LLAMA_CPP_VERSION}: ${LLAMA_CPP_URL}"
    mkdir -p "${source_dir}"
    curl -fsSL --retry 4 --retry-delay 5 -o "${build_root}/source.tar.gz" "${LLAMA_CPP_URL}"
    echo "${LLAMA_CPP_SHA256}  ${build_root}/source.tar.gz" | sha256sum -c -
    tar -xzf "${build_root}/source.tar.gz" -C "${source_dir}" --strip-components=1 --no-same-owner
    rm -f "${build_root}/source.tar.gz"
}

build() {
    cd "${source_dir}"
    short="$(printf '%s' "${LLAMA_CPP_COMMIT}" | cut -c1-7)"
    jobs="$(nproc)"
    echo "== configure (${jobs} processors)"
    # GGML_NATIVE is off: the build machine's processor says nothing about
    # the computer the image runs on. GGML_CPU_ALL_VARIANTS builds one
    # processor backend per x86-64 level and llama.cpp loads the best one
    # the computer supports when it starts. GGML_BACKEND_DIR is where it
    # looks for them; GGML_BACKEND_DL makes them loadable modules. The web
    # interface is not built (tur-ai-local starts the engine with
    # --no-webui) and nothing is downloaded: no prebuilt interface, no
    # OpenSSL for the engine's own downloads (tur-ai-local downloads models,
    # and starts the engine with --offline). The build number and commit
    # name the version; without git they would read 0 and "unknown".
    # $ORIGIN is meant literally: the libraries are found beside the program.
    # shellcheck disable=SC2016
    cmake -S . -B build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_FLAGS="-ffile-prefix-map=${source_dir}=llama.cpp" \
        -DCMAKE_CXX_FLAGS="-ffile-prefix-map=${source_dir}=llama.cpp" \
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
        -DLLAMA_BUILD_NUMBER="${LLAMA_CPP_BUILD_NUMBER}" \
        -DLLAMA_BUILD_COMMIT="${short}" \
        -DLLAMA_BUILD_TESTS=OFF \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_TOOLS=ON \
        -DLLAMA_BUILD_SERVER=ON \
        -DLLAMA_BUILD_APP=OFF \
        -DLLAMA_BUILD_UI=OFF \
        -DLLAMA_USE_PREBUILT_UI=OFF \
        -DLLAMA_OPENSSL=OFF \
        -DLLAMA_LLGUIDANCE=OFF < /dev/null
    echo "== build llama-server, its libraries and every backend"
    started="$(date +%s)"
    cmake --build build --target llama-server --parallel "${jobs}" < /dev/null
    echo "== built in $(( $(date +%s) - started )) s"

    mkdir -p "${out}${prefix}" "${out}/usr/bin" "${out}${licences}" "${out}/usr/lib/tur"
    cp -a build/bin/llama-server "${out}${prefix}/"
    # The shared libraries with their version links, and the backends.
    find build/bin -maxdepth 1 -name '*.so*' \( -type f -o -type l \) -exec cp -a {} "${out}${prefix}/" \;
    # No debug information is compiled in (Release); the symbol tables that
    # are not needed to link go too.
    find "${out}${prefix}" -type f -exec strip --strip-unneeded {} \;
    chmod 0755 "${out}${prefix}"/*
    ln -s ../libexec/tur/llama.cpp/llama-server "${out}/usr/bin/llama-server"

    echo "== what the engine is"
    ls -la "${out}${prefix}"
    du -sh "${out}${prefix}" | cut -f1
    for backend in "${out}${prefix}"/libggml-*.so; do
        printf '%s ' "$(basename "${backend}")"
    done
    echo
    # Every library a file needs must be beside it or in the base image.
    missing="$(find "${out}${prefix}" -type f -exec ldd {} \; 2>&1 | grep 'not found' || true)"
    if [ -n "${missing}" ]; then
        echo "libraries not found: ${missing}" >&2
        exit 1
    fi
    echo "== libraries the engine takes from the image"
    find "${out}${prefix}" -type f -exec ldd {} \; | awk '$3 ~ /^\// && $3 !~ /llama.cpp/ { print $1 }' | LC_ALL=C sort -u | tr '\n' ' '
    echo
    for name in cpu-x64 cpu-haswell cpu-skylakex cpu-alderlake vulkan; do
        test -s "${out}${prefix}/libggml-${name}.so" || { echo "the build has no libggml-${name}.so" >&2; exit 1; }
    done

    licences_of
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "# name" "version" "licence" "source" "source SHA-256" "installed at" "licence files" \
        > "${out}/usr/lib/tur/built-from-source.tsv"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        llama.cpp "${LLAMA_CPP_VERSION}" "${LLAMA_CPP_LICENCE}" "${LLAMA_CPP_URL}" "${LLAMA_CPP_SHA256}" \
        "${prefix}" "${licences}" >> "${out}/usr/lib/tur/built-from-source.tsv"
    cat "${out}/usr/lib/tur/built-from-source.tsv"
}

# The licence and notices of everything compiled into the engine, as its
# source carries them. Notices that live inside a source file are copied
# out of it, from the first line of the notice to its end.
licences_of() {
    dest="${out}${licences}"
    install -m 0644 LICENSE "${dest}/LICENSE"
    install -m 0644 licenses/LICENSE-jsonhpp "${dest}/LICENSE-nlohmann-json"
    install -m 0644 vendor/cpp-httplib/LICENSE "${dest}/LICENSE-cpp-httplib"
    install -m 0644 "${apache}" "${dest}/Apache-2.0.txt"
    sed -n '/^This software is available under 2 licenses/,$p' vendor/stb/stb_image.h > "${dest}/LICENSE-stb_image"
    sed -n '/^This software is available as a choice of the following licenses/,$p' vendor/miniaudio/miniaudio.h \
        > "${dest}/LICENSE-miniaudio"
    sed -n '/This is free and unencumbered software/,/unlicense.org/p' vendor/sheredom/subprocess.h \
        > "${dest}/LICENSE-subprocess.h"
    sed -n '1,/^\/\/ SOFTWARE\.$/p' ggml/src/ggml-cpu/llamafile/sgemm.cpp > "${dest}/LICENSE-llamafile-sgemm"
    {
        echo "The licences of what is compiled into llama.cpp's llama-server in TUR's image,"
        echo "built from ${LLAMA_CPP_URL}"
        echo "(SHA-256 ${LLAMA_CPP_SHA256}, commit ${LLAMA_CPP_COMMIT})."
        echo
        echo "LICENSE                  llama.cpp and ggml (MIT)"
        echo "LICENSE-nlohmann-json    vendor/nlohmann/json.hpp (MIT)"
        echo "Apache-2.0.txt           lines of Google's Abseil inside vendor/nlohmann/json.hpp (Apache-2.0)"
        echo "LICENSE-cpp-httplib      vendor/cpp-httplib (MIT)"
        echo "LICENSE-stb_image        vendor/stb/stb_image.h (MIT or public domain), from the file's end"
        echo "LICENSE-miniaudio        vendor/miniaudio/miniaudio.h (public domain or MIT-0), from the file's end"
        echo "LICENSE-subprocess.h     vendor/sheredom/subprocess.h (Unlicense), from the file's head"
        echo "LICENSE-llamafile-sgemm  ggml/src/ggml-cpu/llamafile/sgemm.cpp (MIT, Mozilla Foundation)"
        echo
        echo "Also in ggml/src/ggml-cpu/ops.cpp, under the MIT licence above:"
        grep -m1 'MIT licensed. Copyright' ggml/src/ggml-cpu/ops.cpp | sed 's|^// *||'
    } > "${dest}/README"
    for file in "${dest}"/*; do
        if [ ! -s "${file}" ]; then
            echo "an empty licence file: ${file} (did the source move a notice?)" >&2
            exit 1
        fi
    done
    grep -q 'Copyright (c) 2023 Jeffrey Quesnelle' "${dest}/README"
    grep -q 'ALTERNATIVE B - Public Domain' "${dest}/LICENSE-stb_image"
    grep -q 'MIT No Attribution' "${dest}/LICENSE-miniaudio"
    grep -q 'Mozilla Foundation' "${dest}/LICENSE-llamafile-sgemm"
    chmod 0644 "${dest}"/*
    ls -la "${dest}"
}

case "${1:-}" in
    fetch) fetch ;;
    build) build ;;
    *)
        echo "usage: llama-cpp.sh fetch|build" >&2
        exit 2
        ;;
esac

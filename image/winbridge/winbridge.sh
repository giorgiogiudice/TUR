#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds P@x's bridge into Windows applications (services/tur-winbridge) for
# the image, from its source, with mingw-w64 from the base image's own Fedora
# repositories. Runs only in the build stage "winbridge" of
# image/Containerfile, never on a development machine (AGENTS.md).
#
#   winbridge.sh fetch   with network: the cross compiler from the base's
#                        repositories. Nothing is compiled.
#   winbridge.sh build   without network: compile, and put under /out what
#                        the image gets:
#
#     /usr/lib/tur/winbridge/tur-winbridge.exe   the bridge (TUR's licence)
#     /usr/lib/tur/winbridge/tur-winhook.dll     the menu hook (MIT)
#     /usr/lib/tur/winbridge/build.tsv           what was built, with what
#     /usr/share/licenses/tur-winhook/LICENSE    the hook's MIT licence
#     /usr/share/licenses/tur-winbridge/         the licences of the mingw-w64
#                                                runtime linked into both files
#
# tur-runtime starts the bridge inside a Windows application's sandbox,
# under Wine, when tur-pax-a11y connects; nothing is copied into a prefix.
# The files are Windows programs of TUR's own: no Microsoft binary is built,
# fetched or shipped. See image/README.md, "P@x's bridge into Windows
# applications".
set -eu

src=/src/services/tur-winbridge
out=/out
dest=/usr/lib/tur/winbridge
work=/build/winbridge

# What the build needs from the base's repositories. The image gets none of
# it: only /out is copied out of this stage.
packages="mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt"

fetch() {
    echo "== the cross compiler, from the base image's repositories"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False --setopt=tsflags=nodocs install ${packages}
    # shellcheck disable=SC2086
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' ${packages}
}

build() {
    mkdir -p "${work}" "${out}${dest}" "${out}/usr/share/licenses/tur-winhook" "${out}/usr/share/licenses/tur-winbridge"
    /bin/sh "${src}/build.sh" "${work}"
    install -m 0644 "${work}/tur-winbridge.exe" "${out}${dest}/tur-winbridge.exe"
    install -m 0644 "${work}/tur-winhook.dll" "${out}${dest}/tur-winhook.dll"
    install -m 0644 "${src}/hook-mit/LICENSE" "${out}/usr/share/licenses/tur-winhook/LICENSE"

    # The mingw-w64 runtime (start-up code and helpers of its C library) is
    # linked into both files; its licence goes with them.
    copied=0
    for package in mingw64-crt mingw64-headers; do
        for file in /usr/share/licenses/"${package}"/*; do
            [ -f "${file}" ] || continue
            install -m 0644 "${file}" "${out}/usr/share/licenses/tur-winbridge/${package}-$(basename "${file}")"
            copied=$((copied + 1))
        done
    done
    if [ "${copied}" -eq 0 ]; then
        echo "no licence file of the mingw-w64 runtime was found" >&2
        exit 1
    fi
    {
        echo "tur-winbridge.exe is TUR's own program, under TUR's licence (/usr/share/licenses/tur/)."
        echo "tur-winhook.dll is under the MIT licence (/usr/share/licenses/tur-winhook/LICENSE)."
        echo "Both were compiled with $(x86_64-w64-mingw32-gcc --version | head -n 1) and link"
        echo "parts of the mingw-w64 runtime, whose licence files are beside this one:"
        rpm -q --qf '  %{NAME} %{VERSION}-%{RELEASE}: %{LICENSE}\n' mingw64-crt mingw64-headers
        echo "Parts of libgcc may be linked too, under the GCC Runtime Library Exception."
    } > "${out}/usr/share/licenses/tur-winbridge/README"

    tab="$(printf '\t')"
    {
        printf '# file\tSHA-256\tbytes\tlicence\tcompiler\truntime\n'
        for file in tur-winbridge.exe tur-winhook.dll; do
            case "${file}" in
                tur-winhook.dll) licence=MIT ;;
                *) licence=LicenseRef-TUR-Noncommercial-1.0.0 ;;
            esac
            printf '%s%s%s%s%s%s%s%s%s%s%s\n' "${file}" "${tab}" \
                "$(sha256sum "${out}${dest}/${file}" | cut -d' ' -f1)" "${tab}" \
                "$(wc -c < "${out}${dest}/${file}" | tr -d ' ')" "${tab}" "${licence}" "${tab}" \
                "$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}' mingw64-gcc)" "${tab}" \
                "$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}' mingw64-crt)"
        done
    } > "${out}${dest}/build.tsv"
    chmod 0644 "${out}${dest}/build.tsv" "${out}/usr/share/licenses/tur-winbridge"/*
    cat "${out}${dest}/build.tsv"
    ls -la "${out}${dest}" "${out}/usr/share/licenses/tur-winbridge" "${out}/usr/share/licenses/tur-winhook"
}

case "${1:-}" in
    fetch) fetch ;;
    build) build ;;
    *)
        echo "usage: winbridge.sh fetch | build" >&2
        exit 2
        ;;
esac

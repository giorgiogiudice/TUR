#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Fetches the crates of, and builds, the TUR programs that go into the image.
# Runs only in the build stage of image/Containerfile ("programs"), never on
# a development machine (AGENTS.md).
#
#   programs.sh fetch   with network: download exactly the crates the
#                       committed lockfiles name. Nothing is compiled.
#   programs.sh build   without network: compile every program of
#                       image/programs.tsv, and put under /out what the
#                       image gets:
#
#     the programs, where programs.tsv says
#     /usr/lib/tur/programs.tsv                  what was installed
#     /usr/share/tur/pax/tools/<component>/      each component's assistant
#                                                tools, where it ships them
#                                                as agent-tools.json
#     /usr/share/tur/catalogue/                  the application catalogue
#     /usr/share/licenses/tur/                   TUR's licence and notice
#     /usr/share/licenses/tur-third-party/       the licence files of every
#                                                third-party crate compiled
#                                                in, those of the Rust
#                                                standard library, and
#                                                crates.tsv: which program
#                                                holds which crate
#
# Everything is read from /src (the build context) and from the crates that
# `fetch` downloaded. See image/README.md, "Programs built into the image".
set -eu

src=/src
out=/out
table="${src}/image/programs.tsv"
work=/build/work
licences="${out}/usr/share/licenses/tur-third-party"
tab="$(printf '\t')"

mkdir -p "${work}"
grep -v '^#' "${table}" | grep -v '^[[:space:]]*$' > "${work}/programs.tsv"

check_toolchain() {
    want="$(sed -n 's/^channel = "\(.*\)"$/\1/p' /pin/rust-toolchain.toml)"
    have="$(rustc --version | cut -d' ' -f2)"
    if [ "${want}" != "${have}" ]; then
        echo "rust-toolchain.toml pins Rust ${want}, the build stage has ${have}" >&2
        exit 1
    fi
}

fetch() {
    while IFS="${tab}" read -r component program path; do
        echo "== cargo fetch --locked: ${component} (${program}, ${path})"
        (cd "${src}/${component}" && cargo fetch --locked < /dev/null)
    done < "${work}/programs.tsv"
    echo "== crates downloaded"
    find "${CARGO_HOME}/registry/cache" -name '*.crate' | wc -l
    du -sh "${CARGO_HOME}/registry" | cut -f1
}

# crate_licences NAME VERSION: copy the licence files of one crate, as the
# crate ships them, and print how many there were.
crate_licences() {
    name="$1"
    version="$2"
    dest="${licences}/${name}-${version}"
    if [ -d "${dest}" ]; then
        find "${dest}" -type f | wc -l
        return
    fi
    found=""
    for candidate in "${CARGO_HOME}"/registry/src/*/"${name}-${version}"; do
        if [ -d "${candidate}" ]; then found="${candidate}"; fi
    done
    if [ -z "${found}" ]; then
        echo "no source directory for crate ${name} ${version}" >&2
        exit 1
    fi
    mkdir -p "${dest}"
    (
        cd "${found}"
        find . -maxdepth 4 -type f \
            \( -iname 'licen[sc]e*' -o -iname 'copying*' -o -iname 'copyright*' \
            -o -iname 'notice*' -o -iname 'unlicense*' -o -iname 'patents*' \) \
            ! -name '*.rs' ! -name '*.c' ! -name '*.h' ! -name '*.py' ! -name '*.sh' \
            ! -name '*.toml' ! -name '*.json' ! -name '*.yml' ! -name '*.yaml' ! -name '*.js' \
            | LC_ALL=C sort
    ) > "${work}/licence-files.txt"
    while read -r file; do
        install -D -m 0644 "${found}/${file}" "${dest}/${file}"
    done < "${work}/licence-files.txt"
    wc -l < "${work}/licence-files.txt"
}

build() {
    check_toolchain
    mkdir -p "${out}/usr/lib/tur" "${licences}"
    installed="${out}/usr/lib/tur/programs.tsv"
    crates="${work}/crates.tsv"
    : > "${crates}"
    {
        echo "# The TUR programs compiled into this image. Written by the image build"
        echo "# (image/programs.sh). Fields, separated by tabs: program, path,"
        echo "# component, the directory of its assistant tools or \"-\"."
    } > "${installed}"

    # The application catalogue, where the catalogue program reads it when it
    # is not told otherwise and where its assistant tools name it.
    for entry in "${src}"/catalogue/*.toml; do
        install -D -m 0644 "${entry}" "${out}/usr/share/tur/catalogue/$(basename "${entry}")"
    done

    # The licence of TUR's own programs, and its required notice.
    install -D -m 0644 "${src}/LICENSE.md" "${out}/usr/share/licenses/tur/LICENSE.md"
    install -D -m 0644 "${src}/NOTICE" "${out}/usr/share/licenses/tur/NOTICE"

    # The Rust standard library is linked into every program.
    rust_version="$(rustc --version | cut -d' ' -f2)"
    sysroot="$(rustc --print sysroot)"
    rust_files=0
    echo "== licence files of the toolchain: ${sysroot}/share/doc"
    ls -la "${sysroot}/share/doc" "${sysroot}/share/doc/rust" 2>&1 || true
    for file in "${sysroot}"/share/doc/rust/COPYRIGHT* "${sysroot}"/share/doc/rust/LICENSE*; do
        if [ -f "${file}" ]; then
            install -D -m 0644 "${file}" "${licences}/rust-std-${rust_version}/$(basename "${file}")"
            rust_files=$((rust_files + 1))
        fi
    done

    while IFS="${tab}" read -r component program path; do
        echo "== cargo build: ${component} (${program})"
        (cd "${src}/${component}" && cargo build --release --locked --offline --bin "${program}" < /dev/null)
        install -D -m 0755 "${CARGO_TARGET_DIR}/release/${program}" "${out}${path}"

        tools="-"
        if [ -f "${src}/${component}/agent-tools.json" ]; then
            tools="/usr/share/tur/pax/tools/$(basename "${component}")"
            install -D -m 0644 "${src}/${component}/agent-tools.json" "${out}${tools}/agent-tools.json"
            install -D -m 0644 "${src}/${component}/agent-tools.adapter.json" \
                "${out}${tools}/agent-tools.adapter.json"
        fi
        printf '%s\t%s\t%s\t%s\n' "${program}" "${path}" "${component}" "${tools}" >> "${installed}"

        # What is linked in: the packages reached through ordinary
        # dependencies, on this platform. Build scripts' own dependencies
        # are not part of the program; procedural macros are listed, which
        # errs towards one notice too many.
        (cd "${src}/${component}" \
            && cargo tree --locked --offline --edges normal --prefix none --format '{p}|{l}' < /dev/null) \
            | sed 's/ (\*)$//' | LC_ALL=C sort -u > "${work}/tree.txt"
        # name, version and checksum of every package in the lockfile.
        awk '
            /^\[\[package\]\]/ { name = ""; version = "" }
            /^name = /        { name = $3; gsub(/"/, "", name) }
            /^version = /     { version = $3; gsub(/"/, "", version) }
            /^checksum = /    { sum = $3; gsub(/"/, "", sum); print name "\t" version "\t" sum }
        ' "${src}/${component}/Cargo.lock" > "${work}/lock.tsv"
        printf '%s\trust-std\t%s\tMIT OR Apache-2.0\t-\t%s\n' \
            "${program}" "${rust_version}" "${rust_files}" >> "${crates}"
        while IFS='|' read -r package licence; do
            if [ -z "${package}" ]; then continue; fi
            # shellcheck disable=SC2086  # split on purpose: name, version, source
            set -- ${package}
            name="$1"
            version="${2#v}"
            case "${package}" in
                *"(/"*) continue ;; # a crate of this repository
            esac
            checksum="$(awk -F'\t' -v n="${name}" -v v="${version}" '$1 == n && $2 == v { print $3 }' "${work}/lock.tsv")"
            if [ -z "${checksum}" ]; then
                echo "crate ${name} ${version} of ${component} has no checksum in Cargo.lock: ${package}" >&2
                exit 1
            fi
            files="$(crate_licences "${name}" "${version}")"
            printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
                "${program}" "${name}" "${version}" "${licence:-NOASSERTION}" "${checksum}" "${files}" >> "${crates}"
        done < "${work}/tree.txt"
    done < "${work}/programs.tsv"

    {
        echo "# The third-party crates compiled into the TUR programs of this image, and"
        echo "# the Rust standard library. Written by the image build"
        echo "# (image/programs.sh). Fields, separated by tabs: program, crate,"
        echo "# version, the licence the crate declares, the SHA-256 of the crate file"
        echo "# as Cargo.lock pins it, and how many licence files of the crate are in"
        echo "# <crate>-<version>/ beside this file."
        LC_ALL=C sort "${crates}"
    } > "${licences}/crates.tsv"

    echo "== programs"
    while IFS="${tab}" read -r _ _ path; do
        sha256sum "${out}${path}"
        ls -l "${out}${path}"
    done < "${work}/programs.tsv"
    echo "== third-party crates: $(grep -v '^#' "${licences}/crates.tsv" | cut -f2,3 | LC_ALL=C sort -u | wc -l) distinct"
    echo "== crates without a licence file of their own:"
    grep -v '^#' "${licences}/crates.tsv" | awk -F'\t' '$6 == 0 { print $2 " " $3 " (" $4 ")" }' | LC_ALL=C sort -u
    find "${out}" -type f | wc -l
    du -sh "${out}" | cut -f1
}

case "${1:-}" in
    fetch) fetch ;;
    build) build ;;
    *)
        echo "usage: programs.sh fetch|build" >&2
        exit 2
        ;;
esac

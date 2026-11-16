#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Puts speech synthesis for P@x's voice, Piper, into the image at the version
# pinned in piper.lock. Never run on a development machine (AGENTS.md).
#
#   piper.sh fetch     in the build stage "speech", with network: the wheel
#                      from PyPI, checked against its SHA-256, into /piper
#                      with this script and its lock.
#   piper.sh install   in the final stage, from /run/tur-piper (the stage's
#                      /piper bound in): Fedora's onnxruntime and
#                      pathvalidate, a virtual environment that uses them,
#                      the wheel unpacked into it, and a launcher:
#
#     /usr/libexec/tur/piper/             the environment; bin/piper runs
#                                         "python -m piper"
#     /usr/share/licenses/piper/          the GPL text from the wheel and a
#                                         README naming the source
#     /usr/lib/tur/built-from-source.tsv  one line added
#
# Piper is a GPL program: TUR starts it as a separate process (tur-ai-local
# speech tts) and links nothing of it (AGENTS.md, licence boundary). The
# virtual environment is made without pip (--without-pip): the wheel is a
# zip archive unpacked into its site-packages, which is what installing a
# wheel is, with no tool that could fetch anything else.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=image/engine/piper.lock
. "${here}/piper.lock"

prefix=/usr/libexec/tur/piper
licences=/usr/share/licenses/piper

fetch() {
    mkdir -p /piper
    echo "== piper-tts ${PIPER_VERSION}: ${PIPER_WHEEL_URL}"
    curl -fsSL --retry 4 --retry-delay 5 -o "/piper/${PIPER_WHEEL}" "${PIPER_WHEEL_URL}"
    echo "${PIPER_WHEEL_SHA256}  /piper/${PIPER_WHEEL}" | sha256sum -c -
    cp "${here}/piper.sh" "${here}/piper.lock" /piper/
}

install_piper() {
    wheel="${here}/${PIPER_WHEEL}"
    echo "${PIPER_WHEEL_SHA256}  ${wheel}" | sha256sum -c -
    echo "== Fedora's onnxruntime and pathvalidate"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False install ${PIPER_FEDORA_PACKAGES}
    # shellcheck disable=SC2086
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' ${PIPER_FEDORA_PACKAGES} onnxruntime
    dnf clean all

    echo "== the environment"
    python3 -m venv --system-site-packages --without-pip "${prefix}"
    site="$("${prefix}/bin/python3" -I -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
    "${prefix}/bin/python3" -I -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' \
        "${wheel}" "${site}"
    # The wheel's abi3 extension and data are now in site-packages; the
    # launcher is the one command tur-ai-local runs.
    printf '%s\n' '#!/bin/sh' \
        '# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0' \
        '# Piper (GPL-3.0-or-later), run as a separate program: /usr/share/licenses/piper.' \
        "exec ${prefix}/bin/python3 -I -m piper \"\$@\"" > "${prefix}/bin/piper"
    chmod 0755 "${prefix}/bin/piper"
    # It must start: the module and onnxruntime import, and --help answers.
    "${prefix}/bin/piper" --help > /dev/null

    echo "== licences"
    mkdir -p "${licences}"
    found="$(find "${site}" -path '*piper_tts-*.dist-info/*' \( -iname 'LICENSE*' -o -iname 'COPYING*' \) | head -n 1)"
    if [ -z "${found}" ]; then
        echo "the wheel carries no licence file" >&2
        exit 1
    fi
    install -m 0644 "${found}" "${licences}/LICENSE"
    {
        echo "Piper (piper1-gpl) ${PIPER_VERSION}, GPL-3.0-or-later, with espeak-ng (GPL-3.0-or-later) inside."
        echo "Installed from the wheel ${PIPER_WHEEL_URL}"
        echo "(SHA-256 ${PIPER_WHEEL_SHA256})."
        echo "Its source: ${PIPER_SOURCE_URL}"
        echo "(SHA-256 ${PIPER_SOURCE_SHA256}), and https://github.com/OHF-Voice/piper1-gpl."
        echo "TUR runs it as a separate program and links nothing of it."
    } > "${licences}/README"
    chmod 0644 "${licences}"/*
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        piper "${PIPER_VERSION}" "${PIPER_LICENCE}" "${PIPER_SOURCE_URL}" "${PIPER_SOURCE_SHA256}" \
        "${prefix}" "${licences}" >> /usr/lib/tur/built-from-source.tsv
}

case "${1:-}" in
    fetch) fetch ;;
    install) install_piper ;;
    *)
        echo "usage: piper.sh fetch|install" >&2
        exit 2
        ;;
esac

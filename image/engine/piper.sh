#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Piper's dependencies for P@x's voice. Piper itself is NOT in the image: the
# owner decided on 16 November 2026 that Piper and the espeak-ng inside it
# (both GPL-3.0-or-later) are downloaded on first use with the voices.
# tur-ai-local's speech catalogue holds the wheel, pinned by its size and
# SHA-256, and its source archive, pinned for the GPL's offer of source
# (services/tur-ai-local/speech/piper-tts-1-8-0.toml); `tur-ai-local speech
# ensure piper-tts-1-8-0` checks it and unpacks it into a private virtual
# environment that sees these system packages. piper.lock keeps the same pins
# for the record; a test holds the two together.
#
#   piper.sh deps   in the final stage: Fedora's onnxruntime and
#                   pathvalidate, which the wheel requires (requires_dist
#                   "onnxruntime<2,>=1", "pathvalidate<4,>=3"); neither is
#                   GPL. They must import.
#
# Never run on a development machine (AGENTS.md).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=image/engine/piper.lock
. "${here}/piper.lock"

deps() {
    echo "== Fedora's onnxruntime and pathvalidate, for Piper downloaded on first use"
    # shellcheck disable=SC2086  # a list of package names, split on purpose
    dnf -y --setopt=install_weak_deps=False install ${PIPER_FEDORA_PACKAGES}
    # shellcheck disable=SC2086
    rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' ${PIPER_FEDORA_PACKAGES} onnxruntime
    dnf clean all
    python3 -I -c 'import onnxruntime, pathvalidate'
}

case "${1:-}" in
    deps) deps ;;
    *)
        echo "usage: piper.sh deps" >&2
        exit 2
        ;;
esac

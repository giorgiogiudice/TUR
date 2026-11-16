#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Produce Darling's Debian packages for the pinned commit (ci/darling.lock),
# as upstream has it or with TUR's patch queue applied.
#
#     ci/darling-get.sh upstream OUT     Darling's own CI packages of the
#                                        pinned commit when they are still
#                                        kept, else a build from source
#     ci/darling-get.sh queue OUT        a build from source with every
#                                        patch of darling-patches/series
#
# OUT/*.deb, OUT/darling-get.txt (what was done), OUT/queue-apply.txt (for
# `queue`: each patch, the submodule it goes to, applied or not).
#
# The build is Darling's own (tools/debian/make-deb), as Darling's CI runs
# it on ubuntu-latest and ubuntu-24.04-arm, with ccache. Linux only; nothing
# here touches macOS or an Apple file.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
VARIANT=$1
OUT=$(mkdir -p "$2" && cd "$2" && pwd)
# shellcheck source=darling.lock
. "$HERE/ci/darling.lock"
ARCH=$(dpkg --print-architecture)
REPORT="$OUT/darling-get.txt"
echo "variant=$VARIANT arch=$ARCH commit=$DARLING_COMMIT" > "$REPORT"

if [ "$VARIANT" = upstream ] && [ -n "${GH_TOKEN:-}" ] && [ -n "${DARLING_CI_RUN:-}" ]; then
    name=debs
    [ "$ARCH" = arm64 ] && name=debs-arm64
    api=https://api.github.com/repos/darlinghq/darling
    run=$(curl -fsSL -H "Authorization: Bearer $GH_TOKEN" "$api/actions/runs/$DARLING_CI_RUN" || true)
    if [ "$(jq -r .head_sha <<<"$run")" = "$DARLING_COMMIT" ] && [ "$(jq -r .conclusion <<<"$run")" = success ]; then
        url=$(curl -fsSL -H "Authorization: Bearer $GH_TOKEN" "$api/actions/runs/$DARLING_CI_RUN/artifacts" |
              jq -r --arg n "$name" '.artifacts[] | select(.name == $n and .expired == false) | .archive_download_url' | head -1)
        if [ -n "$url" ] && curl -fsSL -H "Authorization: Bearer $GH_TOKEN" -o "$OUT/debs.zip" "$url"; then
            (cd "$OUT" && unzip -q -o debs.zip && rm debs.zip)
            sha256sum "$OUT"/*.deb >> "$REPORT"
            echo "source=darling-ci-run-$DARLING_CI_RUN" >> "$REPORT"
            exit 0
        fi
    fi
    echo "Darling's CI packages not usable (run not successful, not this commit, or expired): building" >> "$REPORT"
fi

SRC=$RUNNER_TEMP/darling
rm -rf "$SRC"
git init -q "$SRC"
git -C "$SRC" remote add origin "$DARLING_REPO"
git -C "$SRC" fetch -q --depth 1 origin "$DARLING_COMMIT"
git -C "$SRC" checkout -q FETCH_HEAD
git -C "$SRC" submodule update --init --recursive --depth 1 --jobs 8

if [ "$VARIANT" = queue ]; then
    : > "$OUT/queue-apply.txt"
    while read -r sub patch; do
        case "$sub" in ''|\#*) continue ;; esac
        if git -C "$SRC/$sub" apply --index "$HERE/darling-patches/$patch" 2>>"$OUT/queue-apply.txt"; then
            echo "applied $sub $patch" >> "$OUT/queue-apply.txt"
        else
            echo "FAILED  $sub $patch" >> "$OUT/queue-apply.txt"
            exit 1
        fi
    done < "$HERE/darling-patches/series"
    cat "$OUT/queue-apply.txt"
fi

sudo apt-get update -q
sudo apt-get install -y -q devscripts equivs debhelper ccache
(cd "$SRC" && sudo mk-build-deps -i -r -t "apt-get --no-install-recommends -y" debian/control)
sudo rm -rf /usr/share/dotnet /opt/ghc /usr/local/lib/android || true
sudo tee /etc/ccache.conf > /dev/null <<EOF
cache_dir = ${HOME}/.ccache
max_size = 2G
compiler_check = content
sloppiness = time_macros,include_file_mtime,include_file_ctime
EOF
ccache --zero-stats
(cd "$SRC" && ./tools/debian/make-deb)
ccache --show-stats | tee -a "$REPORT"
mv "$SRC"/../*.deb "$OUT/"
sha256sum "$OUT"/*.deb >> "$REPORT"
echo "source=built" >> "$REPORT"

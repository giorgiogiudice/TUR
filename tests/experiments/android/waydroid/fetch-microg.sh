#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Download microG Services, pinned by SHA-256:
#
#   fetch-microg.sh DIR      writes DIR/gms.apk
#
# microG is the open-source reimplementation of Google Play services
# (Apache-2.0, https://github.com/microg/GmsCore); release v0.3.18.252432
# of 12 November 2026, hash from the release's own asset record. It is not
# Google's software and contains none of it. Used in CI for one question:
# does Waydroid's image let it stand in for Play services. Never shipped.
set -euo pipefail

dir=$1
mkdir -p "$dir"
sha=1094fa60ace2367ac52bd08d1bd4f33fa8144ba930e355121b28234b8ebbf382
if ! echo "$sha  $dir/gms.apk" | sha256sum -c --status 2>/dev/null; then
	curl -fL --retry 3 --retry-delay 5 -sS -o "$dir/gms.apk" \
		https://github.com/microg/GmsCore/releases/download/v0.3.18.252432/com.google.android.gms-252432036.apk
fi
echo "$sha  $dir/gms.apk" | sha256sum -c

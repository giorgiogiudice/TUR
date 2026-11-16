#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Download Waydroid's Android images, pinned by SHA-256:
#
#   fetch-images.sh ZIP_DIR [IMG_DIR]
#
# The VANILLA system image (LineageOS 20, Android 13) contains no Google
# software; the GAPPS variant is never fetched here (AGENTS.md). File names
# and hashes are those of https://ota.waydro.id on 14 November 2026. With
# IMG_DIR, the archives are unpacked there (system.img, vendor.img).
set -euo pipefail

zips=$1
imgs=${2:-}
mkdir -p "$zips"
base=https://sourceforge.net/projects/waydroid/files/images

fetch() { # PATH FILE SHA256
	local dest="$zips/$2"
	if [ -f "$dest" ] && echo "$3  $dest" | sha256sum -c --status; then
		echo "$2: cached"
		return
	fi
	for attempt in 1 2 3; do
		curl -fL --retry 3 --retry-delay 5 -sS -o "$dest" "$base/$1/$2/download" && break
		echo "$2: download attempt $attempt failed" >&2
		sleep 10
	done
	echo "$3  $dest" | sha256sum -c
}

fetch system/lineage/waydroid_x86_64 lineage-20.0-20260927-VANILLA-waydroid_x86_64-system.zip \
	053552725bf4ae25e2d3f7e6f1947203294aa9d73dc17fe1f3abe646e5d3bd8a
fetch vendor/waydroid_x86_64 lineage-20.0-20261022-MAINLINE-waydroid_x86_64-vendor.zip \
	91663c337013145208b6c533d7e1fa2a915a9864ddc8fc56966f40cd759648ac

if [ -n "$imgs" ]; then
	mkdir -p "$imgs"
	for z in "$zips"/*.zip; do
		unzip -o -q "$z" -d "$imgs"
	done
	ls -l "$imgs"
fi

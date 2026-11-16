#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Put the pinned applications where a job wants them:
#
#   prepare.sh WORK NAME...        NAME is sample, random or arm
#
# For each NAME, WORK/locks/NAME.lock.tsv is the lock file kept in the
# repository (tests/experiments/android/locks/) if there is one; otherwise
# it is made from today's F-Droid index (WORK/index, fetched and verified
# here) and belongs in the repository afterwards, so that later runs use
# the same versions. The APKs of every lock are downloaded to WORK/apks and
# verified by SHA-256.
set -euo pipefail

work=$1
shift
exp=$(cd "$(dirname "$0")/.." && pwd)
tool="$exp/fdroid/fdroid_index.py"
mkdir -p "$work/locks" "$work/apks" "$work/index" "$work/new-locks"

need_index() {
	if [ ! -f "$work/index/index-meta.json" ]; then
		python3 "$tool" fetch "$work/index"
	fi
}

for name in "$@"; do
	lock="$work/locks/$name.lock.tsv"
	if [ -f "$exp/locks/$name.lock.tsv" ]; then
		cp "$exp/locks/$name.lock.tsv" "$lock"
		echo "$name: lock file from the repository"
	else
		need_index
		case "$name" in
		sample) python3 "$tool" resolve "$work/index/index-v2.json" "$exp/sample.txt" "$lock" ;;
		arm) python3 "$tool" resolve "$work/index/index-v2.json" "$exp/arm-sample.txt" "$lock" --prefer-abi arm64-v8a ;;
		random) python3 "$tool" random "$work/index/index-v2.json" 30 tur-android-1 "$lock" ;;
		*)
			echo "unknown lock: $name" >&2
			exit 2
			;;
		esac
		cp "$lock" "$work/new-locks/"
		echo "$name: lock file made from the index"
	fi
	python3 "$tool" download "$lock" "$work/apks"
done

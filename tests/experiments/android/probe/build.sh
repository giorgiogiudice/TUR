#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build the probe application with the Android SDK and NDK that GitHub's
# runner image carries, without Gradle:
#
#   build.sh OUT_DIR
#
# writes OUT_DIR/tur-probe.apk (native code for x86_64 and arm64-v8a) and
# OUT_DIR/tur-probe-arm64.apk (native code for arm64-v8a only: what an
# ARM-only application looks like to an x86-64 computer). Signed with a key
# made on the spot; nothing here is published.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
out=$1
mkdir -p "$out"
work=$(mktemp -d)

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/usr/local/lib/android/sdk}}
bt="$sdk/build-tools/$(ls "$sdk/build-tools" | sort -V | tail -n 1)"
platform="$sdk/platforms/$(ls "$sdk/platforms" | sort -V | tail -n 1)/android.jar"
ndk=${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_HOME:-$sdk/ndk/$(ls "$sdk/ndk" | sort -V | tail -n 1)}}
tc="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
{
	echo "build-tools: $bt"
	echo "platform:    $platform"
	echo "ndk:         $ndk"
	javac -version 2>&1
} | tee "$out/toolchain.txt"

# Java to dex.
mkdir -p "$work/classes" "$work/dex"
javac -source 8 -target 8 -bootclasspath "$platform" -Xlint:-options -d "$work/classes" \
	"$here"/src/org/tur/probe/*.java
"$bt/d8" --lib "$platform" --min-api 21 --output "$work/dex" $(find "$work/classes" -name '*.class')

# Native code. -ffp-contract=off: no fused multiply-add on ARM, so that the
# floating-point checksum is the same on every ABI and in Java.
for abi in x86_64:x86_64-linux-android21 arm64-v8a:aarch64-linux-android21; do
	dir=${abi%%:*}
	triple=${abi##*:}
	mkdir -p "$work/lib/$dir"
	"$tc/${triple}-clang" -O2 -fPIC -shared -ffp-contract=off -Wall -Wl,-soname,libturprobe.so \
		-o "$work/lib/$dir/libturprobe.so" "$here/jni/probe.c"
done

keytool -genkeypair -keystore "$work/probe.jks" -storepass android -keypass android -alias probe \
	-keyalg RSA -keysize 2048 -validity 3650 -dname "CN=TUR experiment probe" >/dev/null 2>&1

package() { # NAME ABI...
	local name=$1
	shift
	local stage="$work/stage-$name"
	mkdir -p "$stage/lib"
	# The application has no resources of its own: the manifest is all
	# aapt2 has to link. The older aapt is the fallback.
	"$bt/aapt2" link -o "$stage/base.apk" -I "$platform" --manifest "$here/AndroidManifest.xml" \
		--min-sdk-version 21 --target-sdk-version 33 --version-code 1 --version-name 1.0 ||
		"$bt/aapt" package -f -M "$here/AndroidManifest.xml" -I "$platform" -F "$stage/base.apk" \
			--min-sdk-version 21 --target-sdk-version 33 --version-code 1 --version-name 1.0
	cp "$work/dex/classes.dex" "$stage/"
	for abi in "$@"; do
		cp -r "$work/lib/$abi" "$stage/lib/"
	done
	(cd "$stage" && zip -q -r base.apk classes.dex lib)
	"$bt/zipalign" -f 4 "$stage/base.apk" "$stage/aligned.apk"
	"$bt/apksigner" sign --ks "$work/probe.jks" --ks-pass pass:android --key-pass pass:android \
		--out "$out/$name.apk" "$stage/aligned.apk"
	rm -f "$out/$name.apk.idsig"
	unzip -l "$out/$name.apk" | tee "$out/$name.contents.txt"
	sha256sum "$out/$name.apk" | tee -a "$out/toolchain.txt"
}

package tur-probe x86_64 arm64-v8a
package tur-probe-arm64 arm64-v8a
rm -rf "$work"

#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Adds the Android runtime to the image (ADR-0006; android-runtime.md,
# sections 3.2, 8.3 and 10.2). Runs only in the build of image/Containerfile.
#
#   waydroid.sh PACKAGES_LOCK
#
# - Installs the packages PACKAGES_LOCK names, at exactly those versions, from
#   the Fedora 45 release tree, without weak dependencies, and records what
#   came with them (/usr/share/tur/android/added-packages.tsv).
# - Checks that the SELinux module was installed with them, that the
#   container service is not enabled and that TUR's drop-in, which keeps it
#   from starting until Android is switched on, applies to it.
# - Hides Waydroid's three launcher entries and takes away their claim on APK
#   files and market: links. They start Waydroid directly: with Android off
#   they cannot work, and an APK opened from the file manager would be
#   installed without TUR's inspection. `tur-android` is the way in.
# - Leaves nothing under /var: Waydroid's state belongs to the installed
#   system, and is made when Android is switched on.
set -euo pipefail

lock="${1:?the packages lock is required}"
say() { printf 'android: %s\n' "$*"; }
die() { printf 'android: %s\n' "$*" >&2; exit 1; }

mapfile -t want < <(grep -v '^#' "${lock}" | grep -v '^[[:space:]]*$' | cut -f1)
[ "${#want[@]}" -gt 0 ] || die "${lock} names no package"

work="$(mktemp -d)"
rpm -qa --qf '%{NAME}\n' | LC_ALL=C sort -u > "${work}/before"
dnf -y --repo=fedora --setopt=install_weak_deps=False install "${want[@]}"
dnf clean all
rpm -qa --qf '%{NAME}\n' | LC_ALL=C sort -u > "${work}/after"

for nevra in "${want[@]}"; do
    rpm -q "${nevra}" > /dev/null || die "${nevra} is not installed at that version"
done

mkdir -p /usr/share/tur/android
total=0
{
    printf '# What image/android/waydroid.sh added for the Android runtime.\n'
    printf '# name\tversion\tinstalled size in bytes\tlicence\n'
    while read -r name; do
        line="$(rpm -q --qf '%{NAME}\t%{EVR}.%{ARCH}\t%{SIZE}\t%{LICENSE}\n' "${name}" | head -n1)"
        printf '%s\n' "${line}"
        total=$(( total + $(printf '%s' "${line}" | cut -f3) ))
    done < <(LC_ALL=C comm -13 "${work}/before" "${work}/after")
    printf '# total\t\t%s\n' "${total}"
} > /usr/share/tur/android/added-packages.tsv
cat /usr/share/tur/android/added-packages.tsv
say "$(grep -c -v '^#' /usr/share/tur/android/added-packages.tsv) packages added, $(tail -n1 /usr/share/tur/android/added-packages.tsv | cut -f3) bytes installed"

# Fedora's SELinux module for Waydroid, installed by the package's scriptlet.
semodule -l | grep -qx waydroid || die "the waydroid SELinux module is not installed"
say "SELinux module: $(semodule -l | grep -x waydroid)"

# The container service: present, not enabled, and held by TUR's drop-in.
unit=/usr/lib/systemd/system/waydroid-container.service
test -s "${unit}" || die "${unit} is missing"
systemctl disable waydroid-container.service 2> /dev/null || true
state="$(systemctl is-enabled waydroid-container.service 2>&1 || true)"
say "waydroid-container.service: ${state}"
case "${state}" in
    enabled*) die "waydroid-container.service is enabled" ;;
esac
grep -qx 'ConditionPathExists=/var/lib/tur/android/enabled' \
    /usr/lib/systemd/system/waydroid-container.service.d/50-tur-android.conf \
    || die "TUR's drop-in for the container service is missing"
test -s /usr/lib/systemd/system/dev-binderfs.mount || die "dev-binderfs.mount is missing"

# Waydroid's launcher entries: hidden, and no claim on files or links.
for entry in Waydroid.desktop waydroid.market.desktop waydroid.app.install.desktop; do
    file="/usr/share/applications/${entry}"
    test -f "${file}" || die "${file} is missing"
    sed -i -e '/^MimeType=/d' -e '/^NoDisplay=/d' "${file}"
    sed -i '0,/^\[Desktop Entry\]$/s//[Desktop Entry]\nNoDisplay=true/' "${file}"
    grep -qx 'NoDisplay=true' "${file}" || die "${file} could not be hidden"
done
if command -v update-desktop-database > /dev/null; then
    update-desktop-database /usr/share/applications
fi
if grep -rqs 'vnd.android.package-archive' /usr/share/applications/mimeinfo.cache; then
    die "a launcher entry still claims APK files"
fi

# Nothing under /var: the package's scriptlet may have started Waydroid's
# state directory.
rm -rf /var/lib/waydroid
rm -rf "${work}"
say "done"

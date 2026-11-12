#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Container-level checks on a built TUR image. These prove the image content,
# not that it boots; boot tests live in tests/boot.
#
#   tests/image/smoke.sh IMAGE EXPECTED_BUILD_ID
set -euo pipefail

cd "$(dirname "$0")/../.."
image="${1:?image reference required}"
expected_build_id="${2:?expected build id required}"
read -r -a podman <<<"${PODMAN:-podman}"

# shellcheck source=image/base.lock
. image/base.lock

"${podman[@]}" run --rm -i --network none \
    -e "EXPECT_BUILD_ID=${expected_build_id}" \
    -e "EXPECT_BASE_DIGEST=${BASE_DIGEST}" \
    -e "EXPECT_FEDORA=${BASE_TAG}" \
    "${image}" /bin/bash -s <<'INNER'
set -u
fail=0
check() { # check DESCRIPTION COMMAND...
    local what="$1" said; shift
    # No command may read this script, which arrives on standard input. What
    # a failing command printed is shown: a FAIL alone says too little.
    if said="$("$@" 2>&1 </dev/null)"; then
        echo "PASS  ${what}"
    else
        echo "FAIL  ${what}"
        printf '%s\n' "${said}" | head -n 12 | sed 's/^/      | /'
        fail=1
    fi
}
osr() { ( . /usr/lib/os-release; eval "printf '%s' \"\${$1:-}\"" ); }
turr() { ( . /usr/lib/tur/release; eval "printf '%s' \"\${$1:-}\"" ); }

check "os-release IMAGE_ID is tur"            test "$(osr IMAGE_ID)" = "tur"
check "os-release BUILD_ID matches the build" test "$(osr BUILD_ID)" = "${EXPECT_BUILD_ID}"
check "os-release PRETTY_NAME names TUR"      bash -c '[[ "$(. /usr/lib/os-release; echo "$PRETTY_NAME")" == TUR\ * ]]'
check "distribution ID is tur"                test "$(osr ID)" = "tur"
check "the family is fedora (ID_LIKE)"        test "$(osr ID_LIKE)" = "fedora"
check "VERSION_ID is TUR's version"           test "$(osr VERSION_ID)" = "$(turr TUR_VERSION)"
check "os-release names TUR and TUR's addresses" bash -c \
    '. /usr/lib/os-release; [ "$NAME" = TUR ] && [ "$PRETTY_NAME" = "TUR $VERSION_ID" ] && [ "$LOGO" = tur-logo-icon ] &&
     [ "$DEFAULT_HOSTNAME" = tur ] && [ "$HOME_URL" = https://github.com/giorgiogiudice/TUR ] &&
     [[ "$SUPPORT_URL" == https://github.com/giorgiogiudice/TUR/* ]] && [[ "$BUG_REPORT_URL" == https://github.com/giorgiogiudice/TUR/* ]]'
check "no Fedora or Red Hat in os-release but ID_LIKE" bash -c \
    '! grep -v -x "ID_LIKE=fedora" /usr/lib/os-release | grep -i -E "fedora|red ?hat"'
check "the base is Fedora ${EXPECT_FEDORA}, as recorded" bash -c \
    '[ "$(. /usr/lib/tur/release; echo "$TUR_BASE_OS_ID $TUR_BASE_OS_VERSION_ID")" = "fedora $1" ]' _ "${EXPECT_FEDORA}"
check "system-release names TUR"              bash -c '[ "$(cat /etc/system-release)" = "TUR release $(. /usr/lib/os-release; echo "$VERSION_ID")" ]'
check "release package is Fedora's generic-release" bash -c \
    '[ "$(rpm -q --whatprovides system-release --qf "%{NAME}")" = generic-release ]'
check "logo package is Fedora's generic-logos" bash -c \
    '[ "$(rpm -q --whatprovides system-logos --qf "%{NAME}")" = generic-logos ]'
for pkg in fedora-logos fedora-release fedora-release-common fedora-release-kinoite fedora-release-identity-kinoite \
           fedora-release-ostree-desktop fedora-release-notes fedora-bookmarks plasma-welcome-fedora \
           fedora-workstation-backgrounds NetworkManager-config-connectivity-fedora; do
    check "package ${pkg} is not installed"   bash -c '! rpm -q "$1"' _ "${pkg}"
done
check "the rpm-ostree polkit rule of the release package is kept" \
    grep -q 'org.projectatomic.rpmostree1.upgrade' /usr/share/polkit-1/rules.d/org.projectatomic.rpmostree1.rules
check "the systemd presets are kept"          test -s /usr/lib/systemd/system-preset/90-default.preset
check "the statement of origin is in the image" grep -qx \
    'TUR is built from software packaged by the Fedora Project and many other projects. TUR is not provided, endorsed or supported by the Fedora Project or by Red Hat.' \
    /usr/share/tur/statement-of-origin.txt
check "TUR's logo is the system logo"        bash -c 'test -s /usr/share/icons/hicolor/scalable/apps/tur-logo-icon.svg &&
     cmp -s /usr/share/icons/hicolor/scalable/apps/tur-logo-icon.svg /usr/share/icons/hicolor/scalable/places/start-here.svg &&
     cmp -s /usr/share/pixmaps/system-logo-white.png /usr/share/icons/hicolor/256x256/apps/tur-logo-icon.png'
check "Plymouth's default theme is tur"       bash -c '[ "$(plymouth-set-default-theme)" = tur ]'
check "the initramfs carries the TUR splash and names TUR" bash -c \
    'i=$(ls -1 /usr/lib/modules/*/initramfs.img); lsinitrd "$i" | grep -q usr/share/plymouth/themes/tur/watermark.png &&
     lsinitrd -f usr/lib/initrd-release "$i" | grep -qx "NAME=\"TUR\""'
check "the default global theme is TUR's"     grep -qx 'LookAndFeelPackage=org.tur.dark' /usr/share/kde-settings/kde-profile/default/xdg/kdeglobals
check "no Fedora global theme"                bash -c '! ls -d /usr/share/plasma/look-and-feel/org.fedoraproject.* 2>/dev/null'
check "the default wallpaper is TUR's"        bash -c '[ "$(readlink -f /usr/share/wallpapers/Default)" = /usr/share/wallpapers/Tur ] &&
     test -s /usr/share/wallpapers/Tur/contents/images/1920x1080.png'
check "login and lock screens show TUR's wallpaper" bash -c \
    'grep -q "Image=file:///usr/share/wallpapers/Tur/" /usr/lib/plasmalogin/plasmalogin.conf.d/10-tur.conf &&
     grep -q "Image=file:///usr/share/wallpapers/Tur/" /usr/lib/plasmalogin/defaults.conf &&
     grep -q "Image=file:///usr/share/wallpapers/Tur/" /usr/share/kde-settings/kde-profile/default/xdg/kscreenlockerrc'
check "Firefox does not open Fedora's start page" bash -c \
    'grep -q "browser.startup.homepage\", \"about:home\"" /usr/lib64/firefox/browser/defaults/preferences/tur-default-prefs.js &&
     ! grep -i fedora /usr/lib64/firefox/distribution/distribution.ini'
check "installation counting is off"          bash -c '[ "$(readlink /usr/lib/systemd/system/rpm-ostree-countme.timer)" = /dev/null ]'
check "the connectivity check does not ask Fedora's server" bash -c \
    '! grep -r -s -l fedoraproject /usr/lib/NetworkManager/conf.d /etc/NetworkManager/conf.d'
check "release file records the build ID"     test "$(turr TUR_BUILD_ID)" = "${EXPECT_BUILD_ID}"
check "release file records the base digest"  test "$(turr TUR_BASE_DIGEST)" = "${EXPECT_BASE_DIGEST}"
check "source revision is a full commit hash" bash -c '[[ "$(. /usr/lib/tur/release; echo "$TUR_SOURCE_REVISION")" =~ ^[0-9a-f]{40}$ ]]'

for pkg in systemd bootc ostree kernel dracut selinux-policy-targeted \
           plasma-workspace plasma-desktop kwin \
           NetworkManager pipewire wireplumber udisks2 fwupd bluez \
           xdg-desktop-portal xdg-desktop-portal-kde; do
    check "package ${pkg} is installed" rpm -q "${pkg}"
done

kernels=$(find /usr/lib/modules -mindepth 1 -maxdepth 1 -type d | wc -l)
check "exactly one kernel is installed"       test "${kernels}" = "1"
k=$(ls -1 /usr/lib/modules | head -n1)
check "kernel image is present"               test -s "/usr/lib/modules/${k}/vmlinuz"
check "initramfs is present"                  test -s "/usr/lib/modules/${k}/initramfs.img"
check "default target is graphical"           test "$(systemctl get-default)" = "graphical.target"
check "SELinux is configured enforcing"       grep -qx 'SELINUX=enforcing' /etc/selinux/config
check "bootc container lint passes"           bootc container lint

# Boot-attempt counting, GRUB half (ADR-0014; design sections 5.3, 5.4 and 10).
configs=/usr/lib/bootupd/grub2-static/configs.d
fragment="${configs}/08_tur_boot_count.cfg"
check "GRUB counting fragment is installed"   test -s "${fragment}"
check "fragment declares contract version 1"  grep -q 'tur.boot_contract=1' "${fragment}"
check "fragment ends with a newline"          bash -c '[ -z "$(tail -c1 "$1")" ]' _ "${fragment}"
check "fragment sorts before the menu builder" bash -c \
    'a=$(ls -1 "$1" | grep -n -x 08_tur_boot_count.cfg | cut -d: -f1);
     b=$(ls -1 "$1" | grep -n -x 10_blscfg.cfg | cut -d: -f1);
     [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ]' _ "${configs}"
check "fragment's licence is in the image"    grep -q 'BSD 3-Clause' /usr/share/licenses/tur-grub-boot-count/LICENSE
check "one kernel argument, a GRUB variable"  grep -qxF 'kargs = ["$tur_boot_args"]' /usr/lib/bootc/kargs.d/10-tur-boot.toml
check "no other kernel-argument file"         bash -c '[ "$(ls -1 /usr/lib/bootc/kargs.d | wc -l)" = 1 ]'
check "GRUB tools the design relies on"       bash -c 'command -v grub2-editenv && command -v bootupctl'
check "greenboot is not installed"            bash -c '! rpm -q greenboot'

# Boot health, userspace half (design sections 6.1 and 10).
units=/usr/lib/systemd/system
check "tur-boot-health runs in the image"     /usr/libexec/tur/tur-boot-health --version
check "tur-boot-health is on the PATH"        bash -c '[ "$(readlink -f /usr/bin/tur-boot-health)" = /usr/libexec/tur/tur-boot-health ]'
check "tur-boot-health lists its error codes" bash -c 'tur-boot-health status --codes | grep -q "^TUR-BOOT-TRIAL-003 "'
for unit in tur-boot-state.service tur-boot-health.service tur-boot-mark-good.service \
            tur-boot-record-failure.service tur-boot-retry.service \
            tur-boot-emergency-retry.service tur-boot-arm.service \
            tur-boot-evidence.service tur-boot-failed.target; do
    check "unit ${unit} is in the image"      test -s "${units}/${unit}"
done
for unit in tur-boot-state tur-boot-health tur-boot-mark-good tur-boot-arm tur-boot-evidence; do
    check "${unit} is started on every boot"  test -L "${units}/multi-user.target.wants/${unit}.service"
done
check "emergency mode restarts in a trial"    test -L "${units}/emergency.target.wants/tur-boot-emergency-retry.service"
check "the gate is required by boot-complete.target" \
    grep -qx 'Requires=tur-boot-health.service' "${units}/boot-complete.target.d/10-tur.conf"
check "the restart after a failed gate runs no TUR code" bash -c \
    '[ "$(grep -c "^ExecStart=" "$1")" = 1 ] && grep -qx "ExecStart=/usr/bin/systemctl --no-block reboot" "$1"' \
    _ "${units}/tur-boot-retry.service"
check "the restart is limited to a trial"     grep -qx 'ConditionKernelCommandLine=|tur.boot=trial' "${units}/tur-boot-retry.service"
check "the arming unit does not require /boot" bash -c '! grep -q "^RequiresMountsFor=.*/boot" "$1"' _ "${units}/tur-boot-arm.service"
check "no bootloader update during a trial"   grep -qx 'ConditionKernelCommandLine=!tur.boot=trial' "${units}/bootloader-update.service.d/10-tur.conf"
check "the unit that drop-in applies to exists" test -e "${units}/bootloader-update.service"
check "bootc's update timer is masked"        bash -c '[ "$(readlink /usr/lib/systemd/system/bootc-fetch-apply-updates.timer)" = /dev/null ]'
check "GRUB's boot-success timer is masked"   bash -c '[ "$(readlink /usr/lib/systemd/user/grub-boot-success.timer)" = /dev/null ]'
check "the journal is persistent by decision" grep -qx 'Storage=persistent' /usr/lib/systemd/journald.conf.d/10-tur.conf
check "state directories are declared"        grep -q '^d /var/lib/tur/boot-health ' /usr/lib/tmpfiles.d/tur-boot-health.conf
check "the gate's configuration is in the image" grep -qx 'attempts = 3' /usr/lib/tur/boot-health/gate.toml
check "no TUR file under /etc"                bash -c '! ls -d /etc/tur* /usr/etc/tur* 2>/dev/null | grep -q .'

# TUR's own programs (image/programs.tsv; image/README.md, "Programs built
# into the image"). The list the build wrote is in the image.
programs=/usr/lib/tur/programs.tsv
check "the list of TUR programs is in the image" test -s "${programs}"
check "eleven programs are listed"            bash -c '[ "$(grep -c -v "^#" "$1")" = 11 ]' _ "${programs}"
echo "--- TUR programs"
while IFS=$'\t' read -r program path component tools; do
    case "${program}" in "#"* | "") continue ;; esac
    check "${program} is installed at ${path}" test -x "${path}"
    version="$("${path}" --version 2>&1 < /dev/null | head -n1)"
    echo "      ${version}  ($(stat -c %s "${path}") bytes, from ${component})"
    check "${program} --version names the program and a version" \
        bash -c '[[ "$1" =~ ^"$2"\ [0-9]+\.[0-9]+\.[0-9]+ ]]' _ "${version}" "${program}"
    if [ "${tools}" != "-" ]; then
        check "${program}: its assistant tools are in ${tools}" \
            test -s "${tools}/agent-tools.json" -a -s "${tools}/agent-tools.adapter.json"
        check "${program}: tur-pax loads its tool manifest" \
            tur-pax manifest validate "${tools}/agent-tools.json"
        # The adapters say which program performs each tool. Each adapter
        # must be for a tool the manifest declares and name a program that is
        # in the image. A tool without an adapter is not offered to a model
        # (pax-orchestrator.md); which those are is printed.
        check "${program}: every adapter is for a declared tool and names a program in the image" \
            python3 -c '
import json, os, sys
manifest = json.load(open(sys.argv[1] + "/agent-tools.json"))
adapters = json.load(open(sys.argv[1] + "/agent-tools.adapter.json"))
assert manifest["service"] == adapters["service"], "the two files name different services"
declared = {tool["name"] for tool in manifest["tools"]}
performed = {adapter["tool"]: adapter["program"] for adapter in adapters["adapters"]}
assert performed, "no adapter at all"
unknown = sorted(set(performed) - declared)
assert not unknown, f"adapters for tools the manifest does not declare: {unknown}"
missing = sorted({name for name in performed.values() if not os.access(name, os.X_OK)})
assert not missing, f"not in the image: {missing}"
' "${tools}"
        echo "      offered: $(jq -r '[.adapters[].tool] | join(" ")' "${tools}/agent-tools.adapter.json")"
        echo "      declared, with no adapter: $(jq -r --slurpfile a "${tools}/agent-tools.adapter.json" \
            '[.tools[].name] - [$a[0].adapters[].tool] | join(" ")' "${tools}/agent-tools.json")"
    fi
done < "${programs}"
check "programs a person or an adapter starts are in /usr/bin" bash -c \
    'for p in tur-diag tur-device tur-storage tur-catalogue tur-runtime tur-update tur-pax tur-pax-memory tur-boot-health; do
         [ -x "/usr/bin/$p" ] || exit 1; done'
check "five components have their assistant tools installed" bash -c \
    '[ "$(ls -1 /usr/share/tur/pax/tools | tr "\n" " ")" = "tur-catalogue tur-device-manager tur-diagnostics tur-runtime tur-storage " ]'

# The application catalogue, where tur-catalogue reads it when it is not told
# otherwise, and where its assistant tools name it.
catalogue=/usr/share/tur/catalogue
check "the catalogue is installed"            bash -c '[ "$(ls -1 "$1"/*.toml | wc -l)" -ge 30 ]' _ "${catalogue}"
check "tur-catalogue validates the installed catalogue, found without an option" tur-catalogue validate
check "tur-catalogue finds an application in it" bash -c \
    'cd / && tur-catalogue search discord --json | jq -e ".hits[] | select(.id == \"com.discord.discord\")"'
check "the catalogue its assistant tools name is that directory" \
    grep -q "\"${catalogue}\"" /usr/share/tur/pax/tools/tur-catalogue/agent-tools.adapter.json

# What the programs start (specification section 12; pax-orchestrator.md).
# Nothing is added to the image for them: every one comes with the base.
for tool in /usr/bin/bwrap /usr/bin/prlimit /usr/bin/lsblk /usr/bin/udevadm /usr/bin/journalctl \
            /usr/bin/fwupdmgr /usr/bin/systemctl /usr/bin/tar /usr/bin/bootc /bin/sh; do
    check "${tool}, which a TUR program starts by path, is in the image" test -x "${tool}"
done
for tool in uname systemd-analyze dmesg lspci lsusb vulkaninfo wpctl nmcli bluetoothctl blkid ls; do
    check "${tool}, which tur-diag collects with, is on the PATH" bash -c 'command -v "$1"' _ "${tool}"
done
check "bubblewrap and util-linux are packages of the base" rpm -q bubblewrap util-linux
check "bubblewrap is not set-user-ID"         bash -c '[ ! -u /usr/bin/bwrap ]'
check "no Windows, Android or macOS runtime is in the image" bash -c \
    'for name in wine wine64 wineserver umu-run waydroid darling; do
         if command -v "${name}"; then exit 1; fi
     done
     if rpm -qa | grep -i -E "^(wine|waydroid|darling|umu-launcher)"; then exit 1; fi'

# A program runs, not only prints its version.
check "tur-diag status works without a running system" bash -c \
    'tur-diag status --json | jq -e ".tur.build_id == \"${EXPECT_BUILD_ID}\" and .architecture == \"x86_64\""'
check "tur-runtime recognises a Linux program" bash -c \
    'tur-runtime inspect --json /usr/bin/true | jq -e ".recommendation.runtime == \"native-linux\""'
check "tur-update lists the commands it names" bash -c 'tur-update commands | grep -q bootc'
check "tur-boot-health is still on the PATH"  tur-boot-health --version
# What a program needs from the image at run time: the C library and the
# compiler's support library, which every image has. Everything else a program
# contains is linked in (docs/dependencies/licence-review.md, item 13).
echo "--- shared libraries the programs need"
grep -v '^#' "${programs}" | cut -f2 | while read -r path; do
    echo "      ${path}: $(ldd "${path}" 2>&1 | awk '{print $1}' | sort | tr '\n' ' ')"
done
check "the programs need only the C library and libgcc at run time" bash -c \
    'for path in $(grep -v "^#" "$1" | cut -f2); do
         ldd "${path}" | awk "{print \$1}" | grep -v -E "^(linux-vdso\.so\.1|libc\.so\.6|libm\.so\.6|libgcc_s\.so\.1|/lib64/ld-linux-x86-64\.so\.2)\$" \
             && { echo "${path}"; exit 1; }
     done; exit 0' _ "${programs}"

# Licences (THIRD_PARTY_NOTICES.md).
third=/usr/share/licenses/tur-third-party
check "TUR's licence and notice are in the image" bash -c \
    'grep -q "TUR Noncommercial License" /usr/share/licenses/tur/LICENSE.md && grep -q "Required Notice" /usr/share/licenses/tur/NOTICE'
check "the list of third-party crates is in the image" test -s "${third}/crates.tsv"
check "every listed crate has its directory of licence files" bash -c \
    'grep -v "^#" "$1/crates.tsv" | while IFS=$'"'"'\t'"'"' read -r program crate version licence sum files; do
         [ "${files}" -gt 0 ] || continue
         [ -d "$1/${crate}-${version}" ] || { echo "${crate}-${version}"; exit 1; }
     done' _ "${third}"
check "the notices of the Rust standard library are in the image" bash -c \
    'grep -q -i "apache license" "$1"/rust-std-*/COPYRIGHT-library.html' _ "${third}"
check "every third-party licence directory belongs to a listed crate" bash -c \
    'for dir in "$1"/*/; do
         name="$(basename "${dir}")"
         grep -v "^#" "$1/crates.tsv" | awk -F"\t" -v n="${name}" "\$2 \"-\" \$3 == n { found = 1 } END { exit !found }" \
             || { echo "${name}"; exit 1; }
     done' _ "${third}"
echo "--- third-party crates"
echo "      $(grep -v '^#' "${third}/crates.tsv" | cut -f2,3 | sort -u | wc -l) distinct, in $(grep -v '^#' "${third}/crates.tsv" | cut -f1 | sort -u | wc -l) programs; $(find "${third}" -type f | wc -l) files, $(du -sk "${third}" | cut -f1) KiB"
echo "      without a licence file of their own: $(grep -v '^#' "${third}/crates.tsv" | awk -F'\t' '$6 == 0 { print $2 "-" $3 }' | sort -u | tr '\n' ' ')"
echo "      the largest files:"
find "${third}" -type f -printf '%s %P\n' | sort -rn | head -n 12 | sed 's/^/        /'
# Everything TUR adds must be readable by an ordinary user: the checks above
# run as root, which would not notice.
check "what TUR adds is readable by every user" bash -c \
    '[ -z "$(find /usr/share/tur /usr/share/licenses/tur /usr/share/licenses/tur-third-party /usr/share/licenses/tur-grub-boot-count /usr/lib/tur /usr/libexec/tur \( -type d ! -perm -0555 \) -o \( -type f ! -perm -0444 \) | head -n 5)" ]'

# The unit graph (design section 6.1). Printed in full: a warning here is
# worth reading even when the check passes.
echo "--- systemd-analyze verify"
if systemd-analyze verify --recursive-errors=no \
    "${units}"/tur-boot-*.service "${units}/tur-boot-failed.target" 2>&1; then
    echo "PASS  systemd-analyze verify accepts the boot-health units"
else
    echo "FAIL  systemd-analyze verify accepts the boot-health units"
    fail=1
fi

exit "${fail}"
INNER

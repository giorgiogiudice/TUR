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
    local what="$1"; shift
    if "$@" >/dev/null 2>&1; then echo "PASS  ${what}"; else echo "FAIL  ${what}"; fail=1; fi
}
osr() { ( . /usr/lib/os-release; eval "printf '%s' \"\${$1:-}\"" ); }
turr() { ( . /usr/lib/tur/release; eval "printf '%s' \"\${$1:-}\"" ); }

check "os-release IMAGE_ID is tur"            test "$(osr IMAGE_ID)" = "tur"
check "os-release BUILD_ID matches the build" test "$(osr BUILD_ID)" = "${EXPECT_BUILD_ID}"
check "os-release PRETTY_NAME names TUR"      bash -c '[[ "$(. /usr/lib/os-release; echo "$PRETTY_NAME")" == TUR\ * ]]'
check "distribution ID is still fedora"       test "$(osr ID)" = "fedora"
check "Fedora release is ${EXPECT_FEDORA}"    test "$(osr VERSION_ID)" = "${EXPECT_FEDORA}"
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

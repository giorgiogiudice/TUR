#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#
# Container-level checks on a built UOS image. These prove the image content,
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
uosr() { ( . /usr/lib/uos/release; eval "printf '%s' \"\${$1:-}\"" ); }

check "os-release IMAGE_ID is uos"            test "$(osr IMAGE_ID)" = "uos"
check "os-release BUILD_ID matches the build" test "$(osr BUILD_ID)" = "${EXPECT_BUILD_ID}"
check "os-release PRETTY_NAME names UOS"      bash -c '[[ "$(. /usr/lib/os-release; echo "$PRETTY_NAME")" == UOS\ * ]]'
check "distribution ID is still fedora"       test "$(osr ID)" = "fedora"
check "Fedora release is ${EXPECT_FEDORA}"    test "$(osr VERSION_ID)" = "${EXPECT_FEDORA}"
check "release file records the build ID"     test "$(uosr UOS_BUILD_ID)" = "${EXPECT_BUILD_ID}"
check "release file records the base digest"  test "$(uosr UOS_BASE_DIGEST)" = "${EXPECT_BASE_DIGEST}"
check "source revision is a full commit hash" bash -c '[[ "$(. /usr/lib/uos/release; echo "$UOS_SOURCE_REVISION")" =~ ^[0-9a-f]{40}$ ]]'

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

exit "${fail}"
INNER

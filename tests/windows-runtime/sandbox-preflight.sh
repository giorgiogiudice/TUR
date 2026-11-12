#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#
# Finds out whether bubblewrap can build the sandbox uos-runtime uses on this
# machine, and if it cannot, why, and applies the narrowest documented remedy.
#
#   tests/windows-runtime/sandbox-preflight.sh EVIDENCE_FILE
#
# Ubuntu 24.04 restricts unprivileged user namespaces through AppArmor
# (kernel.apparmor_restrict_unprivileged_userns=1): a program may create one
# only if an AppArmor profile allows it. bubblewrap has no such profile there,
# so it fails with "setting up uid map: Permission denied". The remedies, in
# the order tried:
#
#   1. An AppArmor profile that allows /usr/bin/bwrap to create user
#      namespaces. This is the mechanism Ubuntu itself uses for applications
#      that need them, and it changes nothing for any other program.
#   2. kernel.apparmor_restrict_unprivileged_userns=0, which lifts the
#      restriction for every program on the machine. Acceptable on a
#      throw-away CI runner; not something to do to a user's system.
#
# If neither works the script fails: the sandbox is not optional, and the
# end-to-end test must not run without it.
#
# This is for the CI runner only. It uses sudo and changes system policy.
set -uo pipefail

evidence="${1:?evidence file required}"
: >"${evidence}"
note() { echo "$*" | tee -a "${evidence}"; }

# The same namespace options uos-runtime uses, around a command that does
# nothing.
probe() {
    bwrap --unshare-user --unshare-ipc --unshare-pid --unshare-uts --unshare-cgroup-try \
        --disable-userns --die-with-parent --new-session \
        --ro-bind /usr /usr --ro-bind /etc /etc \
        --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib64 /lib64 \
        --proc /proc --dev /dev --tmpfs /tmp --remount-ro / \
        -- /usr/bin/true
}

note "kernel: $(uname -r)"
note "bubblewrap: $(bwrap --version 2>&1)"
note "setuid bit on bwrap: $(stat -c '%A' "$(command -v bwrap)")"
for key in kernel.apparmor_restrict_unprivileged_userns kernel.unprivileged_userns_clone user.max_user_namespaces; do
    note "sysctl ${key} = $(sysctl -n "${key}" 2>/dev/null || echo '(not present)')"
done
note "AppArmor profiles naming bwrap: $(sudo aa-status 2>/dev/null | grep -i bwrap | tr -s ' \n' ' ' || true)"

if output="$(probe 2>&1)"; then
    note "result: bubblewrap builds the sandbox with no change to the machine"
    exit 0
fi
note "first attempt failed: ${output}"

note "remedy 1: an AppArmor profile that allows /usr/bin/bwrap to create user namespaces"
profile=/etc/apparmor.d/uos-ci-bwrap
sudo tee "${profile}" >/dev/null <<'PROFILE'
# Installed by tests/windows-runtime/sandbox-preflight.sh on a CI runner.
abi <abi/4.0>,
include <tunables/global>

profile uos-ci-bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,
}
PROFILE
if sudo apparmor_parser -r "${profile}" 2>&1 | tee -a "${evidence}" && output="$(probe 2>&1)"; then
    note "result: bubblewrap builds the sandbox after remedy 1 (AppArmor profile for bwrap only)"
    exit 0
fi
note "remedy 1 did not work: ${output}"
sudo apparmor_parser -R "${profile}" 2>/dev/null || true
sudo rm -f "${profile}"

note "remedy 2: kernel.apparmor_restrict_unprivileged_userns=0 (lifts the restriction machine-wide)"
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 2>&1 | tee -a "${evidence}"
if output="$(probe 2>&1)"; then
    note "result: bubblewrap builds the sandbox after remedy 2 (restriction lifted machine-wide)"
    exit 0
fi

note "remedy 2 did not work: ${output}"
note "result: NOT RUN. bubblewrap cannot create namespaces on this runner; the sandboxed tests cannot run."
exit 1

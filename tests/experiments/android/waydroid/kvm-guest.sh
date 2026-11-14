#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Run waydroid/run.sh inside a KVM guest that runs Fedora 45, the kernel
# TUR ships: the Rust binder driver built in, SELinux enforcing, Fedora's
# own Waydroid package.
#
#   kvm-guest.sh WORK MAX_APPS [LOCK.tsv ...]
#
# WORK holds fedora.qcow2 (Fedora's Cloud Base image, verified by the
# caller), image-zips/ (Waydroid's system and vendor archives), apks/ (the
# pinned APKs and probe/). Results come back to WORK/evidence. On the
# runner only; the guest is thrown away.
set -u

WORK=$1
MAX=$2
shift 2
LOCKS=("$@")
EXP=$(cd "$(dirname "$0")/.." && pwd)
OUT="$WORK/evidence"
mkdir -p "$OUT"

log() { echo "[$(date +%T)] $*" | tee -a "$OUT/guest.log"; }

rm -f "$WORK/key" "$WORK/key.pub"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/key"
cat > "$WORK/user-data" <<EOF
#cloud-config
users:
  - name: tur
    groups: wheel
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    shell: /bin/bash
    ssh_authorized_keys:
      - $(cat "$WORK/key.pub")
EOF
printf 'instance-id: tur-android\nlocal-hostname: tur-android\n' > "$WORK/meta-data"
cloud-localds "$WORK/seed.iso" "$WORK/user-data" "$WORK/meta-data"
qemu-img create -q -f qcow2 -b "$WORK/fedora.qcow2" -F qcow2 "$WORK/disk.qcow2" 40G

cpus=$(nproc)
mem=$(( $(free -m | awk '/^Mem:/ {print $2}') * 2 / 3 ))
log "guest: $cpus CPUs, $mem MiB"
qemu-system-x86_64 -enable-kvm -cpu host -smp "$cpus" -m "$mem" -machine q35 \
	-drive "file=$WORK/disk.qcow2,if=virtio,cache=unsafe" \
	-drive "file=$WORK/seed.iso,if=virtio,format=raw,readonly=on" \
	-netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 -device virtio-net-pci,netdev=n0 \
	-device virtio-rng-pci -display none -serial "file:$OUT/serial.log" \
	-daemonize -pidfile "$WORK/qemu.pid"
if [ ! -s "$WORK/qemu.pid" ]; then
	log "QEMU did not start"
	exit 0
fi

SSH_OPTS=(-i "$WORK/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=8 -o LogLevel=ERROR -o ServerAliveInterval=30)
gssh() { ssh "${SSH_OPTS[@]}" -p 2222 tur@127.0.0.1 "$@"; }
gscp() { scp "${SSH_OPTS[@]}" -P 2222 "$@"; }

up=
for _ in $(seq 1 90); do
	if gssh true 2>/dev/null; then
		up=1
		break
	fi
	sleep 4
done
if [ -z "$up" ]; then
	log "the guest did not answer on SSH"
	tail -n 60 "$OUT/serial.log"
	kill "$(cat "$WORK/qemu.pid")" 2>/dev/null
	exit 0
fi
log "guest up: $(gssh uname -r)"
gssh 'cloud-init status --wait' > "$OUT/cloud-init.txt" 2>&1

log "installing Waydroid and a compositor from Fedora's repositories"
gssh 'sudo dnf install -y waydroid sway grim wtype wl-clipboard unzip' > "$OUT/dnf.log" 2>&1
log "dnf status $?"
gssh 'rpm -q kernel-core waydroid waydroid-selinux lxc sway wlroots python3-gbinder libgbinder selinux-policy-targeted 2>&1; uname -r; getenforce' | tee "$OUT/guest-packages.txt"

log "copying images and applications"
gssh 'sudo mkdir -p /var/tmp/tur && sudo chown tur /var/tmp/tur && mkdir -p /var/tmp/tur/locks /var/tmp/tur/out'
gscp -r "$WORK/image-zips" "$WORK/apks" tur@127.0.0.1:/var/tmp/tur/ >> "$OUT/guest.log" 2>&1
gscp -r "$EXP" tur@127.0.0.1:/var/tmp/tur/exp >> "$OUT/guest.log" 2>&1
remote_locks=()
for lock in "${LOCKS[@]}"; do
	[ -f "$lock" ] || continue
	gscp "$lock" tur@127.0.0.1:/var/tmp/tur/locks/ >> "$OUT/guest.log" 2>&1
	remote_locks+=("/var/tmp/tur/locks/$(basename "$lock")")
done
gssh 'mkdir -p /var/tmp/tur/images && cd /var/tmp/tur/images && for z in ../image-zips/*.zip; do unzip -o -q "$z"; done && rm -rf ../image-zips && ls -l' | tee -a "$OUT/guest.log"

log "running the Waydroid script in the guest"
timeout 3000 ssh "${SSH_OPTS[@]}" -p 2222 tur@127.0.0.1 \
	"sudo SETTLE=${SETTLE:-10} bash /var/tmp/tur/exp/waydroid/run.sh /var/tmp/tur/out /var/tmp/tur/images /var/tmp/tur/apks tur $MAX ${remote_locks[*]}" \
	> "$OUT/run-in-guest.log" 2>&1
log "script status $?"
tail -n 40 "$OUT/run-in-guest.log"

gssh 'sudo journalctl -b --no-pager | tail -n 3000' > "$OUT/journal-tail.txt" 2>&1
gssh 'sudo chmod -R a+rX /var/tmp/tur/out'
gscp -r tur@127.0.0.1:/var/tmp/tur/out/. "$OUT/" >> "$OUT/guest.log" 2>&1
gssh 'sudo poweroff' 2>/dev/null
sleep 5
kill "$(cat "$WORK/qemu.pid")" 2>/dev/null
log "done"
exit 0

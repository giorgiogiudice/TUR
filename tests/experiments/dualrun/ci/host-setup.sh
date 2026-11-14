#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Prepare a GitHub-hosted Ubuntu runner for the dualrun experiment.
#
#   host-setup.sh light   packages for the synthetic tests (no KVM)
#   host-setup.sh heavy   QEMU/KVM, firmware, TPM, media tools, display tools
#
# Prints the work directory it chose (the place with the most free space) on
# the last line as WORK=<path>, and appends it to $GITHUB_ENV when set.
set -euo pipefail
mode="${1:-light}"

sudo apt-get update -qq
light=(dmsetup gdisk dosfstools mtools ntfs-3g e2fsprogs util-linux python3)
heavy=(qemu-system-x86 qemu-utils ovmf swtpm xorriso wimtools tesseract-ocr
       python3-pil python3-pil.imagetk python3-tk xvfb openbox xdotool wmctrl
       x11-utils x11-apps xclip smbclient netpbm)
if [ "$mode" = heavy ]; then
  sudo apt-get install -y --no-install-recommends "${light[@]}" "${heavy[@]}"
  # FreeRDP: version 3 where the release has it, else version 2.
  sudo apt-get install -y --no-install-recommends freerdp3-x11 \
    || sudo apt-get install -y --no-install-recommends freerdp2-x11 || true
else
  sudo apt-get install -y --no-install-recommends "${light[@]}"
fi

# Device-mapper targets the launcher may use. Absent ones are a finding.
for m in dm-mod dm-flakey dm-zero loop ntfs3; do
  if sudo modprobe "$m" 2>/dev/null; then echo "module $m: loaded"; else echo "module $m: NOT available"; fi
done
sudo dmsetup targets || true

if [ "$mode" = heavy ]; then
  # Hosted runners expose /dev/kvm but not to the runner user (see boot.yml).
  echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS+="static_node=kvm"' \
    | sudo tee /etc/udev/rules.d/99-kvm4all.rules >/dev/null
  sudo udevadm control --reload-rules
  sudo udevadm trigger --name-match=kvm || true
  ls -la /dev/kvm || echo "/dev/kvm is absent"
  # Windows reads model-specific registers KVM does not know; without this
  # some processors make it stop with an error screen.
  echo 1 | sudo tee /sys/module/kvm/parameters/ignore_msrs >/dev/null || true
  qemu-system-x86_64 --version | head -1
  qemu-system-x86_64 -device help 2>/dev/null | grep -E 'hv-balloon|vmbus-bridge|"nvme"|tpm-crb' || true
  ls -la /usr/share/OVMF/ || true
fi

# The roomiest place: /mnt (the runner's temporary disk) or the runner temp.
best=""; best_kb=0
for d in /mnt "${RUNNER_TEMP:-/tmp}"; do
  [ -d "$d" ] || continue
  kb=$(df -Pk "$d" | awk 'NR==2 {print $4}')
  echo "free space: $d: $((kb / 1024 / 1024)) GiB"
  if [ "$kb" -gt "$best_kb" ]; then best="$d"; best_kb="$kb"; fi
done
if [ "$mode" = heavy ] && [ "$best_kb" -lt $((50 * 1024 * 1024)) ]; then
  echo "less than 50 GiB free: removing preinstalled tool trees from the runner"
  sudo rm -rf /usr/local/lib/android /usr/share/dotnet /opt/ghc /opt/hostedtoolcache/CodeQL || true
  best="${RUNNER_TEMP:-/tmp}"
  df -h "$best" | tail -n 1
fi
work="$best/dualrun"
sudo mkdir -p "$work"
sudo chown "$(id -u):$(id -g)" "$work"
mkdir -p "$work/out"
if [ -n "${GITHUB_ENV:-}" ]; then echo "WORK=$work" >> "$GITHUB_ENV"; fi
echo "WORK=$work"

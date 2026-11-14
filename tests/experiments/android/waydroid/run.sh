#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Start Waydroid on this machine without a GPU or a screen, install the
# pinned applications, and record each one's first screen:
#
#   sudo run.sh OUT_DIR IMAGES_DIR APKS_DIR USER [MAX_APPS] [LOCK.tsv ...]
#
# IMAGES_DIR holds system.img and vendor.img (Waydroid's VANILLA image: no
# Google software). APKS_DIR holds the pinned APKs by SHA-256 and, in
# probe/, the probe application. USER is the ordinary user whose session
# Waydroid's windows belong to.
#
# The same script runs on GitHub's runner (Ubuntu, its own kernel) and in a
# KVM guest running Fedora 45 (the kernel TUR ships, with the Rust binder
# driver). Waydroid and a headless Wayland compositor (sway, grim) must be
# installed already. Every step writes to OUT_DIR; a step that fails is
# recorded and the script goes on, because how far it gets is the finding.
set -u

OUT=$1
IMAGES=$2
APKS=$3
USR=$4
MAX=${5:-0}
shift 5 2>/dev/null || shift $#
LOCKS=("$@")

HERE=$(cd "$(dirname "$0")" && pwd)
UIDN=$(id -u "$USR")
UHOME=$(getent passwd "$USR" | cut -d: -f6)
RUNDIR=/run/user/$UIDN
mkdir -p "$OUT/shots" "$OUT/logs"
T0=$(date +%s)

log() { echo "[$(( $(date +%s) - T0 ))s] $*" | tee -a "$OUT/run.log"; }
fact() { echo "$1=$2" | tee -a "$OUT/facts.txt"; }
# A command inside Android, through Waydroid's shell (lxc-attach), fed on
# standard input so that no option parsing gets in the way.
ash() { timeout "${ASH_TIMEOUT:-60}" waydroid shell <<<"$1" 2>&1; }
# A command as the session user, inside the session's environment.
asuser() {
	sudo -u "$USR" env XDG_RUNTIME_DIR="$RUNDIR" WAYLAND_DISPLAY="${WL:-wayland-1}" SWAYSOCK="$RUNDIR/sway.sock" \
		DBUS_SESSION_BUS_ADDRESS="unix:path=$RUNDIR/bus" HOME="$UHOME" XDG_SESSION_TYPE=wayland "$@"
}
shot() { asuser timeout 30 grim "$OUT/shots/$1.png" 2>> "$OUT/logs/grim.log" && chmod a+r "$OUT/shots/$1.png"; }

# ---------------------------------------------------------------- kernel
{
	echo "kernel: $(uname -r)"
	echo "os: $(. /etc/os-release && echo "$PRETTY_NAME")"
	echo "binder in /proc/filesystems: $(grep -c binder /proc/filesystems)"
	echo "kernel configuration:"
	for cfg in "/boot/config-$(uname -r)" "/lib/modules/$(uname -r)/config"; do
		[ -f "$cfg" ] && grep -E 'CONFIG_ANDROID|CONFIG_ASHMEM|CONFIG_RUST=|CONFIG_MEMFD' "$cfg"
	done
	echo "binder devices:"
	ls -l /dev/binder* /dev/binderfs 2>&1
	echo "selinux: $(getenforce 2>/dev/null || echo 'not present')"
	echo "apparmor: $(cat /sys/module/apparmor/parameters/enabled 2>/dev/null || echo 'not present')"
	echo "cpus: $(nproc); memory: $(free -m | awk '/^Mem:/ {print $2}') MiB"
	echo "waydroid: $(waydroid --version 2>&1)"
	echo "lxc: $(lxc-info --version 2>&1)"
} > "$OUT/kernel.txt" 2>&1
cat "$OUT/kernel.txt"
mem_before=$(free -m | awk '/^Mem:/ {print $3}')

# ---------------------------------------------------------------- init
if [ ! -f "$IMAGES/system.img" ] || [ ! -f "$IMAGES/vendor.img" ]; then
	log "no system.img and vendor.img in $IMAGES"
	fact init "NOT RUN: images missing"
	exit 0
fi
# Waydroid uses images it finds in one of two fixed directories without
# asking its servers for anything.
if command -v getenforce >/dev/null && [ "$(getenforce)" != "Disabled" ]; then
	# Under SELinux the images need a label the container service may
	# read; /usr/share gives them one.
	mkdir -p /usr/share/waydroid-extra/images
	mv "$IMAGES/system.img" "$IMAGES/vendor.img" /usr/share/waydroid-extra/images/
	restorecon -R /usr/share/waydroid-extra
	ls -lZ /usr/share/waydroid-extra/images | tee -a "$OUT/kernel.txt"
else
	mkdir -p /etc/waydroid-extra
	rm -rf /etc/waydroid-extra/images
	ln -s "$IMAGES" /etc/waydroid-extra/images
fi

log "waydroid init"
waydroid init -f > "$OUT/logs/init.log" 2>&1
fact init_status $?
tail -n 15 "$OUT/logs/init.log"
# No GPU here: Android draws with its software renderer (SwiftShader) and
# plain buffers. On a computer with a GPU these two lines are absent.
if [ -f /var/lib/waydroid/waydroid.cfg ]; then
	if ! grep -q swiftshader /var/lib/waydroid/waydroid.cfg; then
		sed -i '/^\[properties\]/a ro.hardware.gralloc=default\nro.hardware.egl=swiftshader' /var/lib/waydroid/waydroid.cfg
		grep -q '^\[properties\]' /var/lib/waydroid/waydroid.cfg ||
			printf '\n[properties]\nro.hardware.gralloc=default\nro.hardware.egl=swiftshader\n' >> /var/lib/waydroid/waydroid.cfg
	fi
	waydroid upgrade --offline >> "$OUT/logs/init.log" 2>&1
	cp /var/lib/waydroid/waydroid.cfg "$OUT/waydroid.cfg"
	cp /var/lib/waydroid/waydroid_base.prop "$OUT/waydroid_base.prop" 2>/dev/null
fi

log "container service"
systemctl restart waydroid-container > "$OUT/logs/container-service.log" 2>&1
sleep 3
systemctl status waydroid-container --no-pager >> "$OUT/logs/container-service.log" 2>&1
fact container_service "$(systemctl is-active waydroid-container)"
ls -l /dev/binder* /dev/binderfs /dev/anbox-* 2>&1 | tee "$OUT/binder-devices.txt" | head -n 20

# ---------------------------------------------------------------- session
mkdir -p "$RUNDIR" && chown "$USR" "$RUNDIR" && chmod 700 "$RUNDIR"
chown -R "$USR" "$OUT" 2>/dev/null
if [ ! -S "$RUNDIR/bus" ]; then
	sudo -u "$USR" env XDG_RUNTIME_DIR="$RUNDIR" dbus-daemon --session --address="unix:path=$RUNDIR/bus" --fork --nopidfile
fi
sudo -u "$USR" env XDG_RUNTIME_DIR="$RUNDIR" DBUS_SESSION_BUS_ADDRESS="unix:path=$RUNDIR/bus" HOME="$UHOME" \
	WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman SWAYSOCK="$RUNDIR/sway.sock" \
	sway -c "$HERE/sway.conf" > "$OUT/logs/sway.log" 2>&1 &
WL=
for _ in $(seq 1 100); do
	WL=$(ls "$RUNDIR" 2>/dev/null | grep -E '^wayland-[0-9]+$' | head -n 1)
	[ -n "$WL" ] && break
	sleep 0.2
done
if [ -z "$WL" ]; then
	log "sway did not start"
	tail -n 30 "$OUT/logs/sway.log"
	fact compositor "failed"
	exit 0
fi
fact compositor "sway headless on $WL"
# A headless compositor has no input devices; hold a virtual keyboard and a
# virtual pointer for the session so that applications listen for input.
asuser wtype -s 100000000 -k F24 > /dev/null 2>&1 &
asuser python3 "$HERE/../atl/vpointer.py" "$RUNDIR/vpointer.cmd" 1280 800 > "$OUT/logs/vpointer.log" 2>&1 &
sleep 1.5
asuser swaymsg -t get_seats > "$OUT/sway-seats.json" 2>&1

start_session() {
	log "waydroid session start"
	SESSION_T=$(date +%s)
	asuser waydroid session start >> "$OUT/logs/session.log" 2>&1 &
	local spid=$! booted=
	for _ in $(seq 1 "${BOOT_TRIES:-150}"); do
		sleep 2
		if [ "$(ASH_TIMEOUT=10 ash 'getprop sys.boot_completed' | tr -d '\r' | tail -n 1)" = "1" ]; then
			booted=1
			break
		fi
		# "waydroid session start" stays in the foreground while the
		# session lives; if it has gone, the session failed.
		kill -0 "$spid" 2>/dev/null || break
	done
	BOOT_SECONDS=$(( $(date +%s) - SESSION_T ))
	[ -n "$booted" ]
}

if start_session; then
	fact android_booted yes
	fact boot_seconds "$BOOT_SECONDS"
else
	fact android_booted no
	waydroid status > "$OUT/logs/status-after-failed-boot.txt" 2>&1
	shot boot-failed
	# On Fedora the container runs under SELinux; if the policy stopped it,
	# say so and try once more in permissive mode, recorded as such.
	if command -v getenforce >/dev/null && [ "$(getenforce)" = "Enforcing" ]; then
		ausearch -m avc -ts boot > "$OUT/logs/avc-enforcing.txt" 2>&1
		fact selinux_denials_enforcing "$(grep -c 'denied' "$OUT/logs/avc-enforcing.txt")"
		log "retrying with SELinux permissive"
		asuser waydroid session stop >> "$OUT/logs/session.log" 2>&1
		setenforce 0
		systemctl restart waydroid-container
		sleep 3
		if start_session; then
			fact android_booted "yes, only with SELinux permissive"
			fact boot_seconds "$BOOT_SECONDS"
		fi
	fi
fi
waydroid status 2>&1 | tee "$OUT/status.txt"

collect() {
	log "collecting logs"
	ASH_TIMEOUT=60 ash 'logcat -d -t 4000' > "$OUT/logs/logcat-tail.txt" 2>&1
	cp /var/lib/waydroid/waydroid.log "$OUT/logs/waydroid.log" 2>/dev/null
	dmesg | grep -iE 'binder|lxc|waydroid|apparmor="DENIED"|avc:' | tail -n 400 > "$OUT/logs/dmesg.txt" 2>&1
	command -v ausearch >/dev/null && ausearch -m avc -ts boot > "$OUT/logs/avc.txt" 2>&1
	command -v getenforce >/dev/null && fact selinux_at_end "$(getenforce)"
	[ -f "$OUT/logs/avc.txt" ] && fact selinux_denials "$(grep -c 'denied' "$OUT/logs/avc.txt")"
	chmod -R a+rX "$OUT" 2>/dev/null
}

if ! grep -q '^android_booted=yes' "$OUT/facts.txt"; then
	log "Android did not finish booting"
	tail -n 40 "$OUT/logs/session.log"
	collect
	exit 0
fi

sleep 5
shot home
fact android_release "$(ash 'getprop ro.build.version.release' | tr -d '\r' | tail -n 1)"
fact android_sdk "$(ash 'getprop ro.build.version.sdk' | tr -d '\r' | tail -n 1)"
fact android_abilist "$(ash 'getprop ro.product.cpu.abilist' | tr -d '\r' | tail -n 1)"
fact native_bridge "$(ash 'getprop ro.dalvik.vm.native.bridge' | tr -d '\r' | tail -n 1)"
fact egl "$(ash 'getprop ro.hardware.egl' | tr -d '\r' | tail -n 1)"
fact gms_installed "$(ash 'pm list packages com.google.android.gms' | tr -d '\r' | grep -c gms)"
fact packages_installed "$(ash 'pm list packages' | grep -c '^package:')"
mem_after=$(free -m | awk '/^Mem:/ {print $3}')
fact host_memory_used_before_mib "$mem_before"
fact host_memory_used_after_boot_mib "$mem_after"
for cg in $(find /sys/fs/cgroup -maxdepth 3 -type d -name 'lxc.payload.waydroid*' 2>/dev/null | head -n 1); do
	[ -f "$cg/memory.current" ] && fact container_cgroup_memory_bytes "$(cat "$cg/memory.current")"
done
fact android_processes "$(ash 'ps -A' | wc -l)"

# ---------------------------------------------------------------- applications
DATA="$UHOME/.local/share/waydroid/data"
install_apk() { # FILE -> prints pm's answer
	mkdir -p "$DATA/local/tmp"
	cp "$1" "$DATA/local/tmp/tur-install.apk"
	chmod 644 "$DATA/local/tmp/tur-install.apk"
	ASH_TIMEOUT=240 ash 'pm install -r -g /data/local/tmp/tur-install.apk' | tr -d '\r' | tail -n 3 | tr '\n' ' '
	rm -f "$DATA/local/tmp/tur-install.apk"
}

echo -e "package\tgroup\tversion\tnativecode\tinstall\tlaunch_status\ttotal_time_ms\twait_time_ms\tresumed\tprocess_alive\tpss_kb\tcrash" > "$OUT/apps.tsv"
run_app() { # PACKAGE GROUP VERSION NATIVECODE APKFILE
	local pkg=$1 group=$2 ver=$3 nat=$4 apk=$5
	local inst comp start status total wait resumed alive pss crash
	log "app $pkg"
	ash 'logcat -c' >/dev/null
	inst=$(install_apk "$apk")
	if ! grep -q Success <<<"$inst"; then
		echo -e "$pkg\t$group\t$ver\t$nat\t${inst:0:160}\t-\t-\t-\t-\t-\t-\t-" >> "$OUT/apps.tsv"
		return
	fi
	comp=$(ash "cmd package resolve-activity --brief -c android.intent.category.LAUNCHER $pkg" | tr -d '\r' | tail -n 1)
	if ! grep -q / <<<"$comp"; then
		echo -e "$pkg\t$group\t$ver\t$nat\tSuccess\tno launcher activity\t-\t-\t-\t-\t-\t-" >> "$OUT/apps.tsv"
		ash "pm uninstall $pkg" >/dev/null
		return
	fi
	start=$(ASH_TIMEOUT=90 ash "am start -W -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -n $comp" | tr -d '\r')
	echo "$start" > "$OUT/logs/start-$pkg.txt"
	status=$(awk -F': ' '/^Status:/ {print $2}' <<<"$start" | head -n 1)
	total=$(awk -F': ' '/^TotalTime:/ {print $2}' <<<"$start" | head -n 1)
	wait=$(awk -F': ' '/^WaitTime:/ {print $2}' <<<"$start" | head -n 1)
	sleep "${SETTLE:-10}"
	shot "$pkg"
	resumed=$(ash 'dumpsys activity activities' | tr -d '\r' | grep -E 'ResumedActivity' | head -n 1 | sed -E 's/.* u0 ([^ ]+).*/\1/')
	alive=$(ash "pidof $pkg" | tr -d '\r' | tail -n 1)
	pss=$(ash "dumpsys meminfo $pkg" | tr -d '\r' | awk '/TOTAL PSS:/ {print $3; exit}')
	crash=$(ash 'logcat -d -b crash' | grep -c "Process: $pkg")
	echo -e "$pkg\t$group\t$ver\t$nat\tSuccess\t${status:--}\t${total:--}\t${wait:--}\t${resumed:--}\t${alive:+yes}\t${pss:--}\t$crash" >> "$OUT/apps.tsv"
	ash "logcat -d -t 400" > "$OUT/logs/logcat-$pkg.txt" 2>&1
	ash "am force-stop $pkg; input keyevent KEYCODE_HOME" >/dev/null
	# Removed again: the image's data partition stays small and the next
	# application starts from the same state.
	ash "pm uninstall $pkg" >/dev/null
}

# The probe first.
PROBE="$APKS/probe/tur-probe.apk"
if [ -f "$PROBE" ]; then
	log "probe"
	inst=$(install_apk "$PROBE")
	fact probe_install "$inst"
	if [ -f "$APKS/probe/tur-probe-arm64.apk" ]; then
		ash 'pm uninstall org.tur.probe' >/dev/null
		fact probe_arm64_only_install "$(install_apk "$APKS/probe/tur-probe-arm64.apk")"
		ash 'pm uninstall org.tur.probe' >/dev/null
		inst=$(install_apk "$PROBE")
	fi
	if grep -q Success <<<"$inst"; then
		ash 'logcat -c' >/dev/null
		asuser dbus-monitor --session > "$OUT/logs/probe-dbus-monitor.log" 2>/dev/null &
		MON=$!
		ash 'am start -W -n org.tur.probe/.ProbeActivity --es phase basic' > "$OUT/logs/start-probe-basic.txt" 2>&1
		sleep 4
		fact probe_clipboard_android_to_host "$(asuser timeout 5 wl-paste -n 2>&1 | head -c 100)"
		asuser wl-copy tur-host-clip-123 2>> "$OUT/logs/grim.log" &
		shot probe-basic-1
		sleep 12
		# Input from the host: the pointer, a key, the Escape key.
		for y in 90 150 210 270 400 600; do
			echo "click 640 $y" >> "$RUNDIR/vpointer.cmd"
			sleep 1
			asuser timeout 5 wtype q 2>> "$OUT/logs/grim.log"
			sleep 0.5
		done
		asuser timeout 5 wtype -k Escape 2>> "$OUT/logs/grim.log"
		sleep 2
		shot probe-basic-2
		ash 'logcat -d -s TURPROBE' | tr -d '\r' | sed -E 's/.*TURPROBE: //' > "$OUT/probe-basic.txt"
		kill "$MON" 2>/dev/null
		fact probe_clipboard_host_to_android "$(grep '^clipboard_get=' "$OUT/probe-basic.txt" | tr '\n' ' ')"
		fact probe_notification_on_host_bus "$(grep -c 'tur-probe-notification' "$OUT/logs/probe-dbus-monitor.log")"
		fact probe_touch_from_host "$(grep -cE '^(touch|tap)=' "$OUT/probe-basic.txt")"
		fact probe_key_from_host "$(grep -E '^(key_down|text)=' "$OUT/probe-basic.txt" | tr '\n' ' ')"
		fact probe_back_from_escape "$(grep -c '^back_pressed=' "$OUT/probe-basic.txt")"
		fact probe_pss_kb "$(ash 'dumpsys meminfo org.tur.probe' | tr -d '\r' | awk '/TOTAL PSS:/ {print $3; exit}')"
		ash 'am force-stop org.tur.probe; logcat -c' >/dev/null
		ash 'am start -W -n org.tur.probe/.ProbeActivity --es phase bench' > "$OUT/logs/start-probe-bench.txt" 2>&1
		for _ in $(seq 1 60); do
			sleep 2
			ash 'logcat -d -s TURPROBE' | grep -q bench_done && break
		done
		ash 'logcat -d -s TURPROBE' | tr -d '\r' | sed -E 's/.*TURPROBE: //' > "$OUT/probe-bench.txt"
		shot probe-bench
		ash 'am force-stop org.tur.probe; logcat -c' >/dev/null
		for phase in open view webview; do
			ash "am start -W -n org.tur.probe/.ProbeActivity --es phase $phase" > "$OUT/logs/start-probe-$phase.txt" 2>&1
			sleep 8
			shot "probe-$phase"
			ash 'logcat -d -s TURPROBE' | tr -d '\r' | sed -E 's/.*TURPROBE: //' > "$OUT/probe-$phase.txt"
			fact "probe_${phase}_resumed" "$(ash 'dumpsys activity activities' | tr -d '\r' | grep -E 'ResumedActivity' | head -n 1 | sed -E 's/.* u0 ([^ ]+).*/\1/')"
			ash 'am force-stop org.tur.probe; input keyevent KEYCODE_HOME; logcat -c' >/dev/null
		done
	fi
fi

n=0
for lock in "${LOCKS[@]}"; do
	[ -f "$lock" ] || continue
	while IFS=$'\t' read -r pkg group vname vcode sha size file nat abi minsdk tgtsdk name; do
		case "$pkg" in "#"* | package | "") continue ;; esac
		if [ "$MAX" != "0" ] && [ "$n" -ge "$MAX" ]; then
			break
		fi
		n=$((n + 1))
		if [ ! -f "$APKS/$sha.apk" ]; then
			echo -e "$pkg\t$group\t$vname\t$nat\tNOT RUN: APK not downloaded\t-\t-\t-\t-\t-\t-\t-" >> "$OUT/apps.tsv"
			continue
		fi
		run_app "$pkg" "$group" "$vname" "$nat" "$APKS/$sha.apk" < /dev/null
	done < "$lock"
done
fact applications_run "$n"

# ---------------------------------------------------------------- microG
# microG is the open-source (Apache-2.0) reimplementation of Google Play
# services; it is not Google's software. It answers under Google's package
# name only if the Android image lets it present Google's signature
# ("signature spoofing"), which LineageOS allows for microG alone. Does
# Waydroid's image? The probe reports what an application sees.
fact signature_spoofing_permission_in_image "$(ash 'pm list permissions' | grep -c FAKE_PACKAGE_SIGNATURE)"
MICROG="$APKS/microg/gms.apk"
if [ -f "$MICROG" ] && [ -f "$PROBE" ]; then
	log "microG"
	fact microg_install "$(install_apk "$MICROG")"
	install_apk "$PROBE" >/dev/null
	ash 'logcat -c' >/dev/null
	ash 'am start -W -n org.tur.probe/.ProbeActivity --es phase none' > "$OUT/logs/start-probe-microg.txt" 2>&1
	sleep 5
	ash 'logcat -d -s TURPROBE' | tr -d '\r' | sed -E 's/.*TURPROBE: //' > "$OUT/probe-microg.txt"
	fact microg_seen_by_application "$(grep -E '^google_play_services' "$OUT/probe-microg.txt" | tr '\n' ' ')"
	ash 'dumpsys package com.google.android.gms' | tr -d '\r' | grep -iE 'FAKE_PACKAGE_SIGNATURE|versionName|signatures' | head -n 8 > "$OUT/microg-package.txt"
	shot microg-probe
	ash 'am force-stop org.tur.probe; pm uninstall org.tur.probe; pm uninstall com.google.android.gms' >/dev/null
fi

# ---------------------------------------------------------------- one window per application
# Waydroid's multi-window mode gives each Android task its own Wayland
# window, which is what TUR's design relies on. Best effort, at the end.
if [ "${MULTI_WINDOW:-1}" = "1" ] && [ -f "$PROBE" ]; then
	log "multi-window mode"
	# Waydroid shares the clipboard only when the Python module pyclip is
	# present; neither Fedora's nor upstream's package asks for it. The
	# first session ran as packaged; this one has the module.
	if command -v dnf >/dev/null; then
		dnf install -y python3-pyclip > "$OUT/logs/pyclip.log" 2>&1
	else
		apt-get install -y python3-pyclip > "$OUT/logs/pyclip.log" 2>&1
	fi
	fact pyclip_present "$(python3 -c 'import pyclip; print("yes")' 2>&1 | tail -n 1)"
	asuser waydroid prop set persist.waydroid.multi_windows true >> "$OUT/logs/session.log" 2>&1
	asuser waydroid session stop >> "$OUT/logs/session.log" 2>&1
	sleep 5
	systemctl restart waydroid-container
	sleep 3
	if BOOT_TRIES=120 start_session; then
		fact multi_window_boot_seconds "$BOOT_SECONDS"
		sleep 5
		install_apk "$PROBE" >/dev/null
		ash 'logcat -c' >/dev/null
		ash 'am start -W -n org.tur.probe/.ProbeActivity --es phase basic' > "$OUT/logs/start-probe-multiwindow.txt" 2>&1
		sleep 4
		fact pyclip_clipboard_android_to_host "$(asuser timeout 5 wl-paste -n 2>&1 | head -c 100)"
		asuser wl-copy tur-host-clip-456 2>> "$OUT/logs/grim.log" &
		sleep 13
		ash 'logcat -d -s TURPROBE' | tr -d '\r' | sed -E 's/.*TURPROBE: //' > "$OUT/probe-multiwindow.txt"
		fact pyclip_clipboard_host_to_android "$(grep '^clipboard_get=' "$OUT/probe-multiwindow.txt" | tr '\n' ' ')"
		fact multi_window_probe_layout "$(grep '^layout=' "$OUT/probe-multiwindow.txt" | tr '\n' ' ')"
		ash 'am start -W -n com.android.settings/.Settings' >> "$OUT/logs/start-probe-multiwindow.txt" 2>&1
		sleep 8
		asuser swaymsg -t get_tree > "$OUT/multi-window-tree.json" 2>&1
		fact multi_window_host_windows "$(grep -c '"app_id"' "$OUT/multi-window-tree.json")"
		fact multi_window_app_ids "$(grep -oE '"app_id": *"[^"]*"' "$OUT/multi-window-tree.json" | sort -u | tr '\n' ' ')"
		shot multi-window
	else
		fact multi_window_boot "failed"
	fi
fi

collect
log "done"
exit 0

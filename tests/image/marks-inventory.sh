#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Where Fedora's name and marks appear in an image: an inventory for a person
# to read (docs/reports/TUR-008.md). It decides nothing; tests/image/marks.py
# is the check that fails a build.
#
#   tests/image/marks-inventory.sh IMAGE OUT_DIR
#
# Writes OUT_DIR/marks-inventory.txt. PODMAN names the container tool.
set -euo pipefail

image="${1:?image reference required}"
out="${2:?output directory required}"
read -r -a podman <<<"${PODMAN:-podman}"
mkdir -p "${out}"

"${podman[@]}" run --rm -i --network none "${image}" /bin/bash -s <<'INNER' > "${out}/marks-inventory.txt" 2>&1
set -u
section() { printf '\n======== %s\n' "$*"; }
owner() { # owner FILE...: file, then the package that owns it
    local f
    for f in "$@"; do
        printf '%s\t%s\n' "$f" "$(rpm -qf --qf '%{NAME}\n' "$f" 2>/dev/null | head -n1 || echo '-')"
    done
}

section "os-release and release files"
for f in /usr/lib/os-release /etc/os-release /etc/system-release /etc/redhat-release /etc/fedora-release \
         /etc/system-release-cpe /usr/lib/system-release-cpe /etc/issue /etc/issue.net /usr/lib/issue /usr/lib/issue.net; do
    if [ -e "$f" ]; then echo "--- $f -> $(readlink -f "$f") ($(rpm -qf "$f" 2>/dev/null | head -n1))"; cat "$f"; fi
done

section "tools present"
for t in dracut lsinitrd plymouth-set-default-theme rsvg-convert magick convert python3 rpmbuild rpm2cpio cpio \
         gtk-update-icon-cache kbuildsycoca6 dnf dnf5 rpm-ostree bootc bootupctl grub2-mkconfig; do
    printf '%s\t%s\n' "$t" "$(command -v "$t" || echo absent)"
done
python3 -c 'import PIL; print("python3 PIL", PIL.__version__)' 2>&1 | tail -n1

section "packages: release, logos, backgrounds, branding (name, version, licence, vendor)"
rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{LICENSE}\t%{VENDOR}\n' \
    | grep -i -E 'fedora|generic|logos|backgrounds|bookmark|release|lookandfeel|welcome|kde-settings|plymouth|anaconda|firefox|flathub|third-party|wallpaper' \
    | sort

section "who provides and who requires system-release, system-logos, fedora-logos"
for cap in system-release 'system-release(releasever)' system-logos fedora-logos redhat-release fedora-release \
           fedora-release-common fedora-release-identity fedora-release-variant system-backgrounds desktop-backgrounds-compat \
           fedora-bookmarks system-bookmarks plasma-lookandfeel-fedora; do
    echo "--- ${cap}"
    echo "provided by: $(rpm -q --whatprovides "${cap}" 2>&1 | tr '\n' ' ')"
    echo "required by: $(rpm -q --whatrequires "${cap}" 2>&1 | tr '\n' ' ')"
done

section "files, requirements and provides of the branding packages"
for p in $(rpm -qa --qf '%{NAME}\n' | grep -E '^(fedora-logos|fedora-release.*|fedora-bookmarks|fedora-.*backgrounds.*|f[0-9]+-backgrounds.*|desktop-backgrounds.*|plasma-lookandfeel-fedora|plasma-welcome-fedora|kde-settings.*|fedora-flathub-remote|fedora-third-party|fedora-chromium-config|fedora-workstation-repositories|fedora-repos.*|fedora-gpg-keys|generic-.*)$' | sort); do
    echo "--- package ${p}: $(rpm -q --qf '%{VERSION}-%{RELEASE} %{LICENSE}' "$p")"
    echo "requires: $(rpm -q --requires "$p" | tr '\n' ';')"
    echo "provides: $(rpm -q --provides "$p" | tr '\n' ';')"
    echo "conflicts/obsoletes: $(rpm -q --conflicts --obsoletes "$p" | tr '\n' ';')"
    rpm -ql "$p" | head -n 400
    if [ "$(rpm -ql "$p" | wc -l)" -gt 400 ]; then echo "... $(rpm -ql "$p" | wc -l) files in all"; fi
done

section "files of the Firefox package that set defaults"
rpm -ql firefox 2>/dev/null | grep -E 'distribution|defaults|pref|policies|\.ini$|\.cfg$|\.desktop$' | head -n 100
for f in $(rpm -ql firefox 2>/dev/null | grep -E '(distribution/.*|defaults/pref/.*|browser/defaults/preferences/.*|\.cfg)$'); do
    [ -f "$f" ] || continue
    echo "--- $f"; head -c 6000 "$f"; echo
done

section "file names containing fedora (any case), with owner"
mapfile -t named < <(find / -xdev \( -path /proc -o -path /sys -o -path /sysroot -o -path /ostree \) -prune -o -iname '*fedora*' -print 2>/dev/null | sort)
echo "count: ${#named[@]}"
owner "${named[@]}"

section "icons named start-here, distributor-logo, system-logo, or named after a logos package"
find /usr/share/icons /usr/share/pixmaps -xdev \( -iname 'start-here*' -o -iname '*distributor-logo*' -o -iname '*system-logo*' -o -iname '*-logo-icon*' -o -iname 'poweredby*' \) 2>/dev/null \
    | sort | while read -r f; do owner "$f"; done

section "text files containing 'Fedora' (case-sensitive), outside documentation, licences, man pages and translations"
mapfile -t texts < <(grep -rIl --exclude-dir=doc --exclude-dir=licenses --exclude-dir=man --exclude-dir=locale \
    --exclude-dir=help --exclude-dir=gtk-doc --exclude-dir=info \
    -e 'Fedora' /usr/share /usr/lib /usr/libexec /etc 2>/dev/null | grep -v -E '^/usr/lib/(modules|firmware|python3)' | sort)
echo "count: ${#texts[@]}"
owner "${texts[@]}" | awk -F'\t' '{ print $2 "\t" $1 }' | sort

section "the same files, by package"
owner "${texts[@]}" | cut -f2 | sort | uniq -c | sort -rn

section "user-visible lines with Fedora: desktop files, metainfo, plasma, kde, plymouth, wallpapers, xdg, polkit, systemd"
grep -rIn -e 'Fedora' /usr/share/applications /usr/share/metainfo /usr/share/plasma /usr/share/kde-settings \
    /usr/share/plymouth /usr/share/wallpapers /usr/share/sddm /usr/share/plasmalogin /usr/lib/plasmalogin /etc/xdg /usr/share/xdg \
    /usr/share/kservices6 /usr/share/kxmlgui5 /usr/share/plasma-setup /usr/share/plasma-welcome /usr/share/desktop-directories \
    /usr/share/xsessions /usr/share/wayland-sessions /usr/share/flatpak /etc/flatpak /usr/lib/systemd /usr/share/polkit-1 \
    /usr/share/backgrounds /usr/share/kf6 /usr/share/color-schemes /usr/share/desktop-base /usr/share/kservicetypes6 2>/dev/null \
    | cut -c1-300 | head -n 600

section "binaries that contain 'Fedora' or 'Powered by' (strings), in /usr/bin and /usr/libexec and Plasma's plugins"
for f in /usr/bin/* /usr/libexec/* /usr/libexec/*/*; do
    [ -f "$f" ] && [ ! -L "$f" ] || continue
    if grep -a -q -e 'Fedora' "$f" 2>/dev/null; then
        printf '%s\t%s\t%s\n' "$f" "$(rpm -qf --qf '%{NAME}' "$f" 2>/dev/null)" "$(grep -a -o -e '[[:print:]]\{0,40\}Fedora[[:print:]]\{0,40\}' "$f" | head -n 3 | tr '\n' '|')"
    fi
done 2>/dev/null | head -n 200
grep -a -l -r 'Powered by' /usr/bin /usr/libexec /usr/lib64/qt6/plugins /usr/lib64/qt6/qml 2>/dev/null | head -n 20

section "Plymouth"
cat /etc/plymouth/plymouthd.conf 2>/dev/null; echo "--- defaults"; cat /usr/share/plymouth/plymouthd.defaults 2>/dev/null
plymouth-set-default-theme 2>&1; echo "--- themes"; ls -la /usr/share/plymouth/themes/ /usr/share/plymouth/themes/* 2>&1
echo "--- plymouth packages: $(rpm -qa 'plymouth*' | tr '\n' ' ')"
for f in /usr/share/plymouth/themes/*/*.plymouth; do echo "--- $f"; cat "$f"; done

section "initramfs"
k=$(ls -1 /usr/lib/modules | head -n1)
echo "kernel: ${k}"
ls -la "/usr/lib/modules/${k}/"
lsinitrd "/usr/lib/modules/${k}/initramfs.img" 2>&1 | head -n 40
echo "--- initramfs files matching plymouth, fedora, logo, os-release, release"
lsinitrd "/usr/lib/modules/${k}/initramfs.img" 2>/dev/null | grep -i -E 'plymouth|fedora|logo|release|watermark|\.png|\.svg|font' | head -n 200
echo "--- os-release in the initramfs"
lsinitrd -f usr/lib/os-release "/usr/lib/modules/${k}/initramfs.img" 2>&1 | head -n 30
lsinitrd -f etc/initrd-release "/usr/lib/modules/${k}/initramfs.img" 2>&1 | head -n 30
echo "--- dracut configuration"
for f in /usr/lib/dracut/dracut.conf.d/* /etc/dracut.conf.d/* /etc/dracut.conf; do [ -f "$f" ] && { echo "# $f ($(rpm -qf "$f" 2>/dev/null))"; cat "$f"; }; done
echo "--- kernel arguments the image sets"
cat /usr/lib/bootc/kargs.d/* 2>/dev/null
ls -la /usr/lib/bootc/install 2>/dev/null; cat /usr/lib/bootc/install/* 2>/dev/null

section "GRUB, bootupd and the ESP payload"
find /usr/lib/bootupd -maxdepth 6 2>/dev/null | head -n 80
for f in /usr/lib/bootupd/grub2-static/*.cfg /usr/lib/bootupd/grub2-static/configs.d/*.cfg; do echo "# $f"; cat "$f"; done 2>/dev/null | head -n 200
ls -la /usr/share/grub /usr/share/pixmaps/bootloader 2>&1 | head -n 40
cat /etc/default/grub 2>/dev/null

section "KDE and Plasma defaults"
ls -la /usr/share/plasma/look-and-feel/ /usr/share/wallpapers/ /usr/share/backgrounds/ 2>&1
for f in /etc/xdg/kdeglobals /usr/share/kde-settings/kde-profile/default/xdg/kdeglobals /etc/xdg/kcm-about-distrorc /etc/xdg/kscreenlockerrc \
         /etc/xdg/plasmarc /etc/xdg/ksplashrc /etc/xdg/plasma-welcomerc /etc/xdg/plasma-org.kde.plasma.desktop-appletsrc \
         /usr/share/kde-settings/kde-profile/default/xdg/*; do
    [ -f "$f" ] && { echo "--- $f ($(rpm -qf "$f" 2>/dev/null))"; head -n 80 "$f"; }
done
echo "--- XDG_CONFIG_DIRS candidates"; ls -la /etc/xdg /usr/share/kde-settings 2>&1 | head -n 80
echo "--- plasmalogin"; ls -laR /usr/lib/plasmalogin /etc/plasmalogin.conf* /usr/share/plasmalogin 2>&1 | head -n 60
for f in /usr/lib/plasmalogin/*.conf /usr/lib/plasmalogin/*/*.conf /etc/plasmalogin.conf /etc/plasmalogin.conf.d/*; do
    [ -f "$f" ] && { echo "# $f ($(rpm -qf "$f" 2>/dev/null))"; cat "$f"; }
done
for d in /usr/share/plasma/look-and-feel/*; do
    echo "--- $d"; cat "$d/metadata.json" 2>/dev/null | head -n 40; cat "$d/contents/defaults" 2>/dev/null
done
for d in /usr/share/wallpapers/*; do echo "--- $d ($(rpm -qf "$d" 2>/dev/null | head -n1))"; cat "$d/metadata.json" "$d/metadata.desktop" 2>/dev/null | head -n 30; ls "$d" "$d/contents" 2>/dev/null | head -n 20; done
echo "--- plasma-setup"; rpm -ql plasma-setup 2>/dev/null | head -n 80
echo "--- plasma-welcome"; rpm -qa 'plasma-welcome*'

section "Flatpak remotes and what adds them"
ls -la /usr/share/flatpak/remotes.d /etc/flatpak/remotes.d /usr/lib/flatpak 2>&1
for f in /usr/share/flatpak/remotes.d/* /etc/flatpak/remotes.d/*; do [ -f "$f" ] && { echo "# $f ($(rpm -qf "$f" 2>/dev/null))"; cat "$f"; }; done
grep -rl -i 'flatpak remote-add\|flatpak-add\|fedora-flathub\|fedora-third-party' /usr/lib/systemd /usr/libexec 2>/dev/null | while read -r f; do owner "$f"; done
systemctl list-unit-files 2>/dev/null | grep -i -E 'flatpak|fedora|countme|third' | head -n 40

section "network contact: connectivity check, time, countme"
for f in /usr/lib/NetworkManager/conf.d/* /etc/NetworkManager/conf.d/* /etc/NetworkManager/NetworkManager.conf; do [ -f "$f" ] && { echo "# $f ($(rpm -qf "$f" 2>/dev/null))"; cat "$f"; }; done
echo "--- chrony"; grep -v '^#' /etc/chrony.conf 2>/dev/null | grep . ; ls -la /usr/lib/chrony* /etc/chrony* 2>/dev/null
grep -rn -i 'pool\|server' /usr/lib/systemd/timesyncd.conf.d /etc/systemd/timesyncd.conf 2>/dev/null | head
echo "--- countme"
grep -rn -i countme /etc/dnf /usr/share/dnf5 /etc/yum.repos.d /usr/lib/systemd/system 2>/dev/null | head -n 40
systemctl list-unit-files 2>/dev/null | grep -i -E 'countme|rpm-ostree|bootc|update' | head -n 20
echo "--- user agent and vendor in package tools"; grep -rn -i 'user_agent\|useragent' /etc/dnf /usr/share/dnf5 2>/dev/null | head

section "systemd presets and polkit rules, with owners"
for f in /usr/lib/systemd/system-preset/* /usr/lib/systemd/user-preset/* /usr/share/polkit-1/rules.d/* /etc/polkit-1/rules.d/*; do
    [ -e "$f" ] && owner "$f"
done
echo "--- rpm macros naming the distribution"
for f in /usr/lib/rpm/macros.d/macros.dist; do [ -f "$f" ] && { echo "# $f ($(rpm -qf "$f"))"; cat "$f"; }; done

section "scripts and configuration that test ID=fedora or name the distribution ID"
grep -rIl -E '(^|[^A-Z_])ID(_LIKE)?[[:space:]]*(=|==|!=)[[:space:]]*"?fedora|"\$ID"[[:space:]]*=+[[:space:]]*"?fedora|os_release.*fedora' \
    /usr/lib /usr/libexec /usr/bin /usr/sbin /usr/share/dnf5 /etc 2>/dev/null | grep -v -E '^/usr/lib/(modules|firmware)' | head -n 60 | while read -r f; do owner "$f"; done
echo "--- binaries that read os-release and contain the word fedora"
for f in /usr/bin/bootc /usr/bin/bootupctl /usr/libexec/bootupd /usr/bin/rpm-ostree /usr/bin/flatpak /usr/bin/dnf5 /usr/bin/ostree /usr/bin/fwupdmgr /usr/libexec/fwupd/fwupd; do
    [ -f "$f" ] || continue
    printf '%s: %s\n' "$f" "$(grep -a -o -E '[[:print:]]{0,50}fedora[[:print:]]{0,50}' "$f" 2>/dev/null | sort -u | head -n 12 | tr '\n' '|')"
done

section "all packages (name, version, licence)"
rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{LICENSE}\n' | sort
INNER
echo "inventory: ${out}/marks-inventory.txt ($(wc -l < "${out}/marks-inventory.txt") lines)"

#!/bin/bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Replaces Fedora's marks in the TUR image (task D-03, report TUR-008;
# desktop-experience.md, section 12). Runs once, as root, in the final stage
# of image/Containerfile, after TUR's files are in place and before nothing
# else but the lint. It needs the network only to download the three pinned
# packages of packages.lock.
#
#   branding.sh ART_DIR HERE_DIR TUR_VERSION TUR_BUILD_ID
#
# ART_DIR is branding/ of the repository (the flat mark, drawn by
# branding/render.py), HERE_DIR is image/branding/. Every step says what it
# did; anything unexpected stops the build.
set -euo pipefail

art="${1:?branding directory required}"
here="${2:?image/branding directory required}"
version="${3:?TUR version required}"
build_id="${4:?build ID required}"

say() { printf 'branding: %s\n' "$*"; }
die() { printf 'branding: ERROR: %s\n' "$*" >&2; exit 1; }

# os-release allows lower-case letters, digits, ".", "_" and "-" in VERSION_ID.
[[ "${version}" =~ ^[a-z0-9._-]+$ ]] || die "TUR version '${version}' is not a valid VERSION_ID"

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

# One fixed time for everything below that records one (the package
# database's install times, the initramfs): the day of the build ID, which
# comes from the commit, so that two builds of a commit give the same bytes.
SOURCE_DATE_EPOCH="$(date -u -d "${build_id:0:8}" +%s)" || die "build ID '${build_id}' does not start with a date"
export SOURCE_DATE_EPOCH

# Dependencies the package database cannot satisfy, before and after: the swap
# must not add one (desktop-experience.md, 12.1: "the package database stays
# consistent").
unmet() { rpm -Va --nofiles --noscripts --nodigest --nosignature 2>&1 | grep -i 'unsatisfied\|requires' | LC_ALL=C sort || true; }
unmet > "${work}/unmet.before"
say "unmet dependencies before: $(wc -l < "${work}/unmet.before")"

# ---- 1. Release and logo packages: Fedora's own mark-free replacements ---------
# Fedora's guidelines require fedora-logos, fedora-release and
# fedora-release-notes to be removed from a system that mixes or modifies
# Fedora's packages; generic-release and generic-logos are Fedora's
# replacements for exactly that case. They are pinned (packages.lock).
mirrors=(
    https://dl.fedoraproject.org/pub/fedora/linux/releases/45
    https://download.fedoraproject.org/pub/fedora/linux/releases/45
    https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/45
)
grep -v '^#' "${here}/packages.lock" | while IFS=$'\t' read -r file sha256 licence path; do
    [ -n "${file}" ] || continue
    got=""
    for mirror in "${mirrors[@]}"; do
        if curl --fail --silent --show-error --location --retry 3 --max-time 120 \
                -o "${work}/${file}" "${mirror}/${path}"; then
            got="${mirror}"
            break
        fi
    done
    [ -n "${got}" ] || die "could not download ${file}"
    echo "${sha256}  ${work}/${file}" | sha256sum --check --quiet - || die "${file}: checksum differs from packages.lock"
    say "downloaded ${file} (${licence}) from ${got}, checksum as pinned"
done

# What the removed release packages carried that a Kinoite system needs, kept:
# the rpm-ostree polkit rule (MIT, Fedora's text) and its licence. The systemd
# presets are in the redhat-systemd-presets-* packages, which stay.
rule=/usr/share/polkit-1/rules.d/org.projectatomic.rpmostree1.rules
cp -p "${rule}" "${work}/rpmostree1.rules"
cp -p /usr/share/licenses/fedora-release-common/LICENSE "${work}/fedora-release-LICENSE"
# The base's preset file from the identity package holds no setting; say so
# here so that a base that changes it stops the build.
if grep -v '^#' /usr/lib/systemd/system-preset/80-kde-desktop.preset | grep -q .; then
    die "80-kde-desktop.preset of the base now holds presets: carry them over before removing it"
fi

swap_out=(fedora-logos fedora-release-kinoite fedora-release-identity-kinoite fedora-release-common
          fedora-release-ostree-desktop)
for pkg in "${swap_out[@]}"; do rpm -q "${pkg}" > /dev/null || die "${pkg} is not in the base"; done
rpm -e --nodeps "${swap_out[@]}"
rpm -i --nosignature "${work}/generic-release-common-45-0.4.noarch.rpm" \
    "${work}/generic-release-45-0.4.noarch.rpm" "${work}/generic-logos-18.0.0-29.fc45.noarch.rpm"
say "replaced ${swap_out[*]} with generic-release, generic-release-common and generic-logos"

install -m 0644 "${work}/rpmostree1.rules" "${rule}"
install -D -m 0644 "${work}/fedora-release-LICENSE" /usr/share/licenses/tur-fedora-release-files/LICENSE
cat > /usr/share/licenses/tur-fedora-release-files/README <<'EOF'
/usr/share/polkit-1/rules.d/org.projectatomic.rpmostree1.rules is Fedora's
file from the fedora-release-ostree-desktop package, kept unchanged when TUR
replaced Fedora's release packages. It is under the MIT licence in LICENSE.
EOF

test "$(rpm -q --whatprovides system-release --qf '%{NAME}')" = generic-release || die "system-release is not generic-release"
test "$(rpm -q --whatprovides system-logos --qf '%{NAME}')" = generic-logos || die "system-logos is not generic-logos"

# ---- 2. Packages whose only purpose is Fedora's presentation ---------------------
# Removed when nothing needs them. When another package requires one, its
# files are deleted and its record is kept, so that no dependency breaks
# (desktop-experience.md, 12.4, B1: "the package cannot be"); its licence
# files stay.
strip_files() {
    local pkg="$1" f
    rpm -ql "${pkg}" | LC_ALL=C sort -r | while read -r f; do
        case "${f}" in /usr/share/licenses/* | /usr/share/doc/*) continue ;; esac
        if [ -L "${f}" ] || [ -f "${f}" ]; then
            rm -f "${f}"
        elif [ -d "${f}" ]; then
            rmdir --ignore-fail-on-non-empty "${f}"
        fi
    done
}
presentation=(fedora-bookmarks plasma-welcome-fedora plasma-lookandfeel-fedora f45-backgrounds-kde
              f45-backgrounds-base fedora-workstation-backgrounds fedora-appstream-metadata
              NetworkManager-config-connectivity-fedora)
for pkg in "${presentation[@]}"; do
    rpm -q "${pkg}" > /dev/null || { say "${pkg}: not in the base"; continue; }
    if rpm -e --test "${pkg}" > /dev/null 2>&1; then
        rpm -e "${pkg}"
        say "${pkg}: removed"
    else
        strip_files "${pkg}"
        say "${pkg}: files deleted, record kept; rpm says: $(rpm -e --test "${pkg}" 2>&1 | tr '\n' ' ' | cut -c1-300)"
    fi
done
# The Fedora global themes also hold files of kde-settings-plasma (scripts that
# set Fedora's logo on the launcher); the directories go as a whole.
rm -rf /usr/share/plasma/look-and-feel/org.fedoraproject.*
# Directories no package owns, left empty by the removals.
for d in /usr/share/backgrounds/fedora-workstation /usr/share/backgrounds/f45 /usr/share/wallpapers/F45; do
    if [ -d "${d}" ]; then rmdir --ignore-fail-on-non-empty "${d}"; fi
done

# ---- 3. os-release and the release files --------------------------------------
# ID=tur, ID_LIKE=fedora: the change Fedora intends for a derived system
# (desktop-experience.md, 12.1). What depends on ID is recorded in TUR-008.
cat > /usr/lib/os-release <<EOF
NAME="TUR"
VERSION="${version}"
ID=tur
ID_LIKE=fedora
VERSION_ID=${version}
PRETTY_NAME="TUR ${version}"
ANSI_COLOR="0;38;2;42;169;248"
LOGO=tur-logo-icon
CPE_NAME="cpe:/o:tur:tur:${version}"
DEFAULT_HOSTNAME="tur"
HOME_URL="https://github.com/giorgiogiudice/TUR"
DOCUMENTATION_URL="https://github.com/giorgiogiudice/TUR/tree/main/docs"
SUPPORT_URL="https://github.com/giorgiogiudice/TUR/issues"
BUG_REPORT_URL="https://github.com/giorgiogiudice/TUR/issues"
IMAGE_ID=tur
IMAGE_VERSION=${version}
BUILD_ID=${build_id}
EOF
# /etc/system-release and its aliases point here. The firmware boot entry that
# bootupd creates is named from the text before " release".
printf 'TUR release %s\n' "${version}" > /usr/lib/fedora-release
printf 'cpe:/o:tur:tur:%s\n' "${version}" > /usr/lib/system-release-cpe
test "$(readlink -f /etc/os-release)" = /usr/lib/os-release || die "/etc/os-release does not point at /usr/lib/os-release"
test "$(cat /etc/system-release)" = "TUR release ${version}" || die "/etc/system-release does not name TUR"
say "os-release: $(. /usr/lib/os-release; echo "${PRETTY_NAME} (ID=${ID}, ID_LIKE=${ID_LIKE})")"

# ---- 4. Logos: TUR's flat mark under the names the system looks for -------------
icons=/usr/share/icons/hicolor
for px in 16 22 24 32 48 64 96 128 256 512; do
    install -D -m 0644 "${art}/png/tur-icon-${px}.png" "${icons}/${px}x${px}/apps/tur-logo-icon.png"
done
for px in 16 22 24 32 48 96 256; do
    install -D -m 0644 "${art}/png/tur-icon-${px}.png" "${icons}/${px}x${px}/places/start-here.png"
done
install -D -m 0644 "${art}/tur-icon.svg" "${icons}/scalable/apps/tur-logo-icon.svg"
install -D -m 0644 "${art}/tur-mark-symbolic.svg" "${icons}/symbolic/apps/tur-logo-icon-symbolic.svg"
# generic-logos ships its neutral artwork under these names; TUR's is put in
# their place, so that what looks for the distribution's logo finds TUR's.
install -m 0644 "${art}/tur-icon.svg" "${icons}/scalable/apps/start-here.svg"
install -m 0644 "${art}/tur-icon.svg" "${icons}/scalable/places/start-here.svg"
install -m 0644 "${art}/png/tur-icon-256.png" /usr/share/pixmaps/system-logo-white.png
install -m 0644 "${art}/png/tur-icon-256.png" /usr/share/pixmaps/tur-logo.png
install -D -m 0644 "${art}/tur-wordmark.svg" /usr/share/tur/branding/tur-wordmark.svg
install -m 0644 "${art}/tur-mark.svg" "${art}/tur-icon.svg" /usr/share/tur/branding/
if [ -e "${icons}/icon-theme.cache" ]; then
    gtk-update-icon-cache --force --quiet "${icons}"
fi
say "logos: tur-logo-icon in ${icons}; start-here and system-logo-white are TUR's"

# ---- 5. Boot splash (desktop-experience.md, 6.1) ----------------------------------
theme=/usr/share/plymouth/themes/tur
test -s "${theme}/tur.plymouth" || die "the TUR Plymouth theme file is missing"
install -m 0644 "${art}/plymouth/watermark.png" "${art}"/plymouth/throbber-*.png "${theme}/"
# The passphrase dialog's images are Plymouth's own (plymouth-theme-spinner).
for f in bullet.png capslock.png entry.png keyboard.png keymap-render.png lock.png; do
    install -m 0644 "/usr/share/plymouth/themes/spinner/${f}" "${theme}/${f}"
done
sed -i -E 's/^Theme=.*/Theme=tur/' /usr/share/plymouth/plymouthd.defaults
grep -qx 'Theme=tur' /usr/share/plymouth/plymouthd.defaults || die "plymouthd.defaults does not name the TUR theme"
test "$(plymouth-set-default-theme)" = tur || die "Plymouth's default theme is not tur"
say "Plymouth: default theme tur"

# ---- 6. Wallpaper, login and lock screen, global theme -----------------------------
wp=/usr/share/wallpapers/Tur
install -D -m 0644 "${art}/wallpaper/1920x1080.png" "${wp}/contents/images/1920x1080.png"
install -m 0644 "${art}/wallpaper/2560x1440.png" "${wp}/contents/images/2560x1440.png"
install -m 0644 "${art}/wallpaper/screenshot.png" "${wp}/contents/screenshot.png"
# Plasma's first-run setup loads its background from Default/contents/images
# (and images_dark), by these two names in JPEG XL (Fedora's patch to it).
formats="$(magick -list format 2> /dev/null || true)"
if grep -Eq '^ *JXL\*? +JXL +rw' <<< "${formats}"; then
    magick "${art}/wallpaper/2560x1440.png" -resize 5120x2880 -quality 100 "${wp}/contents/images/5120x2880.jxl"
    magick "${art}/wallpaper/2560x1440.png" -resize 1440x2960^ -gravity center -extent 1440x2960 -quality 100 \
        "${wp}/contents/images/1440x2960.jxl"
    say "wallpaper: JPEG XL copies for the first-run setup made with $(magick -version | head -n1 | cut -d' ' -f2-3)"
else
    die "ImageMagick in the base cannot write JPEG XL"
fi
ln -s images "${wp}/contents/images_dark"
# Fedora's names for the default wallpaper (Default, and Fedora -> Default)
# now lead to TUR's.
ln -sfn Tur /usr/share/wallpapers/Default
test "$(readlink -f /usr/share/wallpapers/Fedora)" = "${wp}" || die "/usr/share/wallpapers/Fedora does not lead to TUR's wallpaper"

profile=/usr/share/kde-settings/kde-profile/default/xdg
sed -i -E 's/^LookAndFeelPackage=.*/LookAndFeelPackage=org.tur.dark/; s/^ColorScheme=.*/ColorScheme=BreezeDark/' "${profile}/kdeglobals"
sed -i -E '/^\[Icons\]/,/^\[/ s/^Theme=.*/Theme=breeze-dark/' "${profile}/kdeglobals"
sed -i 's|/usr/share/wallpapers/Fedora/|/usr/share/wallpapers/Tur/|g' "${profile}/kscreenlockerrc" /usr/lib/plasmalogin/defaults.conf
# Fedora's chat room offered to NeoChat, and Fedora's bookmarks for Konqueror.
rm -f "${profile}/neochatrc"
cat > /usr/share/kde-settings/kde-profile/default/share/apps/konqueror/bookmarks.xml <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xbel>
<xbel/>
EOF
grep -qx 'LookAndFeelPackage=org.tur.dark' "${profile}/kdeglobals" || die "kdeglobals does not name TUR's global theme"
say "desktop: global theme org.tur.dark, wallpaper Tur on the desktop, login and lock screens"

# ---- 7. Firefox (desktop-experience.md, 12.4, B6) ----------------------------------
# The start page and pinned tab are replaced by tur-default-prefs.js and the
# build's name by distribution.ini (both from image/rootfs). Fedora's build
# compiles its bookmarks into the browser; if they are there, a policy keeps
# them out of new profiles.
omni=/usr/lib64/firefox/browser/omni.ja
# omni.ja is a zip file in Mozilla's own layout (Python's zipfile refuses it;
# unzip reads it with a warning). If it cannot be read, the policy is
# installed anyway: keeping Fedora's bookmarks out matters more than the note
# Firefox then shows about a managed browser.
names="$(unzip -Z1 "${omni}" 2> /dev/null || true)"
fedora_bookmarks=""
while read -r name; do
    case "${name##*/}" in *bookmarks*) ;; *) continue ;; esac
    if { unzip -p "${omni}" "${name}" 2> /dev/null || true; } | grep -q Fedora; then
        fedora_bookmarks="${fedora_bookmarks} ${name}"
    fi
done <<< "${names}"
if [ -z "${names}" ]; then
    install -m 0644 "${here}/firefox-policies.json" /usr/lib64/firefox/distribution/policies.json
    say "Firefox: could not list ${omni}; policy NoDefaultBookmarks installed regardless"
elif [ -n "${fedora_bookmarks}" ]; then
    install -m 0644 "${here}/firefox-policies.json" /usr/lib64/firefox/distribution/policies.json
    say "Firefox: Fedora's bookmarks are built in (${fedora_bookmarks# }); policy NoDefaultBookmarks installed"
else
    say "Firefox: $(grep -c . <<< "${names}") files in omni.ja, no built-in bookmarks name Fedora; no policy installed"
fi
grep -q 'browser.startup.homepage", "about:home"' /usr/lib64/firefox/browser/defaults/preferences/tur-default-prefs.js \
    || die "TUR's Firefox defaults are missing"

# ---- 8. Contact with Fedora's servers nobody asked for (desktop-experience.md, 12.3)
# Counting of installations: off. (The connectivity check went with its
# package above; the time servers and Flatpak remotes are recorded in TUR-008.)
ln -sf /dev/null /usr/lib/systemd/system/rpm-ostree-countme.timer
say "rpm-ostree-countme.timer masked"

unmet > "${work}/unmet.after"
if ! LC_ALL=C comm -13 "${work}/unmet.before" "${work}/unmet.after" > "${work}/unmet.new" || [ -s "${work}/unmet.new" ]; then
    cat "${work}/unmet.new" >&2
    die "the changes left dependencies unmet"
fi
say "no dependency left unmet"

# ---- 9. The initramfs: it carries os-release, the release file and the splash ---------
# Regenerated with the arguments the base used (lsinitrd shows them), at a
# fixed time so that two builds give the same bytes.
kver="$(ls -1 /usr/lib/modules)"
[ "$(printf '%s\n' "${kver}" | wc -l)" = 1 ] || die "expected one kernel"
dracut --reproducible --add ostree --no-hostonly --kver "${kver}" --force "/usr/lib/modules/${kver}/initramfs.img" \
    > "${work}/dracut.log" 2>&1 || { tail -n 60 "${work}/dracut.log" >&2; die "dracut failed"; }
chmod 0644 "/usr/lib/modules/${kver}/initramfs.img"
listing="$(lsinitrd "/usr/lib/modules/${kver}/initramfs.img")"
grep -q 'usr/share/plymouth/themes/tur/tur.plymouth' <<< "${listing}" || die "the initramfs lacks the TUR splash"
grep -q 'usr/share/plymouth/themes/tur/watermark.png' <<< "${listing}" || die "the initramfs lacks the TUR mark"
release="$(lsinitrd -f usr/lib/initrd-release "/usr/lib/modules/${kver}/initramfs.img")"
grep -qx 'NAME="TUR"' <<< "${release}" || die "the initramfs's os-release does not name TUR: ${release}"
say "initramfs regenerated for ${kver} ($(stat -c %s "/usr/lib/modules/${kver}/initramfs.img") bytes, SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH})"

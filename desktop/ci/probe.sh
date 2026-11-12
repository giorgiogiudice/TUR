#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Settle, with evidence, the assumptions that desktop-experience.md (sections
# 5.7, 15.4 and 16) leaves to task D-01. Runs tur-probe inside the builder in
# several ways and writes what it reports, and a picture of each, to
# $OUT/probe. Nothing here fails the job: the point is the record. The
# answers are written up in docs/architecture/desktop-kit.md.
#
#   probe.sh PRESET
set -u

preset=${1:?usage: probe.sh PRESET}
build=$PWD/desktop/build/$preset
out=$PWD/${OUT:-out/desktop}/probe
probe=$build/ci/probe/tur-probe
rm -rf "$out"
mkdir -p "$out/config-tur" "$out/config-empty"

# A colour scheme in TUR's dark colours, written from the format's key names
# (no file of KDE's was copied). If Kirigami reports these colours, it takes
# the system's scheme.
cat > "$out/config-tur/kdeglobals" <<'EOF'
[General]
ColorScheme=TurProbe

[KDE]
AnimationDurationFactor=0

[Colors:Window]
BackgroundNormal=11,15,23
BackgroundAlternate=19,26,39
ForegroundNormal=238,241,246
ForegroundInactive=169,180,198
DecorationFocus=91,190,255
DecorationHover=91,190,255

[Colors:View]
BackgroundNormal=19,26,39
BackgroundAlternate=27,36,52
ForegroundNormal=238,241,246
ForegroundInactive=169,180,198
DecorationFocus=91,190,255
DecorationHover=91,190,255

[Colors:Button]
BackgroundNormal=19,26,39
BackgroundAlternate=27,36,52
ForegroundNormal=238,241,246
ForegroundInactive=169,180,198
DecorationFocus=91,190,255
DecorationHover=91,190,255

[Colors:Selection]
BackgroundNormal=18,103,196
BackgroundAlternate=18,103,196
ForegroundNormal=255,255,255
ForegroundInactive=255,255,255
DecorationFocus=91,190,255
DecorationHover=91,190,255

[Colors:Tooltip]
BackgroundNormal=27,36,52
ForegroundNormal=238,241,246

[Colors:Complementary]
BackgroundNormal=7,10,16
ForegroundNormal=238,241,246

[Colors:Header]
BackgroundNormal=11,15,23
ForegroundNormal=238,241,246
EOF

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_FONT_DPI=96 LC_ALL=C.UTF-8
export QML_IMPORT_PATH="$build/qml"

# run NAME CONFIG-DIR [VAR=VALUE...] -- ARGUMENTS
run() {
    name=$1
    config=$2
    shift 2
    settings=""
    while [ "$1" != "--" ]; do
        settings="$settings $1"
        shift
    done
    shift
    echo "== $name:$settings"
    # shellcheck disable=SC2086  # the settings are VAR=VALUE words
    env XDG_CONFIG_HOME="$out/$config" XDG_CONFIG_DIRS="$out/$config" $settings \
        timeout 120 "$probe" "$@" --grab "$out/$name.png" > "$out/$name.log" 2>&1
    status=$?
    line=$(grep '^TUR-PROBE ' "$out/$name.log" | tail -n 1)
    if [ -n "$line" ]; then
        echo "$line"
        printf '{"scenario":"%s","exit":%s,"facts":%s}\n' "$name" "$status" "${line#TUR-PROBE }" >> "$out/report.jsonl"
    else
        echo "no report (exit status $status); the last lines of its output:"
        tail -n 15 "$out/$name.log"
        printf '{"scenario":"%s","exit":%s,"facts":null}\n' "$name" "$status" >> "$out/report.jsonl"
    fi
    grep -v '^TUR-PROBE ' "$out/$name.log" | sort | uniq -c | sort -rn | head -n 12
}

# 1. The proof page as TUR's programs outside a session draw it.
run standalone config-empty -- --standalone --page Probe

# 2. What style does a QGuiApplication get from Plasma's platform theme?
#    (styleAtStart in the report; expected org.kde.breeze.)
run session-theme config-tur QT_QPA_PLATFORMTHEME=kde XDG_CURRENT_DESKTOP=KDE KDE_FULL_SESSION=true KDE_SESSION_VERSION=6 -- --page BreezeProbe

# 3. org.kde.breeze asked for by name, with TUR's colours as the system scheme:
#    does Kirigami report them, and do stock controls draw in software?
run breeze-tur-scheme config-tur QT_QUICK_CONTROLS_STYLE=org.kde.breeze -- --page BreezeProbe

# 4. The same with no system scheme, for comparison.
run breeze-no-scheme config-empty QT_QUICK_CONTROLS_STYLE=org.kde.breeze -- --page BreezeProbe

# 5. The fallback: the Basic style with TUR's colours as the system scheme.
#    Does Kirigami still report them without org.kde.breeze?
run basic-tur-scheme config-tur QT_QUICK_CONTROLS_STYLE=Basic -- --page BreezeProbe

echo "== is the style installed?"
rpm -q qqc2-breeze-style kf6-kirigami kf6-kcolorscheme plasma-integration
ls -d /usr/lib64/qt6/qml/org/kde/breeze /usr/lib64/qt6/qml/org/kde/kirigami 2>&1
ls /usr/lib64/qt6/plugins/kf6/kirigami/platform/ 2>&1 || true
ls /usr/lib64/qt6/plugins/platformthemes/ 2>&1 || true

echo "== report"
cat "$out/report.jsonl"
exit 0

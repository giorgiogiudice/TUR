#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Print what the builder image contains, and compare the packages a rendered
# picture depends on with desktop/ci/builder.packages. Runs inside the
# builder. A difference is a warning, not a failure: the screenshot
# comparison is the judge of whether it matters.
set -eu

here=$(dirname "$0")

echo "== tools"
cmake --version | head -1
echo "ninja $(ninja --version)"
g++ --version | head -1
echo "Qt $(qmake6 -query QT_VERSION 2>/dev/null || echo '(qmake6 not found)')"
python3 --version
python3 -c 'import PIL; print("Pillow", PIL.__version__)'
for tool in qmllint qmlformat lupdate; do
    echo "$tool: $(command -v "$tool" || echo 'not on the path')"
done

echo "== Qt, KDE and font packages, with the licence each RPM declares"
rpm -qa --qf '%{NAME}  %{VERSION}-%{RELEASE}  [%{LICENSE}]\n' \
    | grep -E '^(qt6-|kf6-|qqc2-|plasma-integration|kwin |layer-shell-qt|breeze-icon-theme|rsms-inter|jetbrains-mono|google-noto|freetype|harfbuzz|fontconfig|gcc-c\+\+|cmake |ninja-build)' \
    | sort || true

echo "== fonts"
fc-match Inter
fc-match 'JetBrains Mono'
fc-match monospace
fc-match sans-serif
fc-list : family | grep -E -i 'inter|jetbrains|hebrew' | sort -u || true

echo "== packages a picture depends on, against builder.packages"
changed=0
while read -r name version; do
    case "$name" in '' | '#'*) continue ;; esac
    have=$(rpm -q --qf '%{VERSION}-%{RELEASE}' "$name" 2>/dev/null || echo missing)
    if [ "$have" = "$version" ]; then
        echo "  ok       $name $have"
    else
        echo "  CHANGED  $name: builder.packages says $version, the image has $have"
        echo "::warning::builder image drift: $name is $have, desktop/ci/builder.packages says $version"
        changed=1
    fi
done < "$here/builder.packages"
if [ "$changed" -ne 0 ]; then
    echo "The image was built with other packages than the reference images were made with."
    echo "If the screenshots still match, update desktop/ci/builder.packages; if not, see"
    echo "desktop/README.md, \"Reference images\"."
fi

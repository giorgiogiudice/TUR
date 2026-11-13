#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Waits for the window of an application the centre opened, takes two
# pictures of the whole screen (two seconds after it appears, and eight
# seconds later: a window can be on screen before anything is drawn in it,
# docs/reports/apps-1.md, finding 5), and closes the application.
#
#   watch.sh FLATPAK_ID TITLE_WORD
set -u

app=${1:?}
word=${2:?}
shots=${OUT:?}/shots

for _ in $(seq 900); do
    line=$(xwininfo -root -tree 2>/dev/null | grep -i "\"[^\"]*${word}[^\"]*\"" | grep -v -i 'tst_centre' | head -n 1)
    if [ -n "$line" ]; then
        echo "found $line"
        sleep 2
        import -window root "$shots/e2e-opened-window.png"
        sleep 8
        import -window root "$shots/e2e-opened-window-later.png"
        flatpak kill "$app" 2>/dev/null || pkill -f "$app" || true
        exit 0
    fi
    sleep 1
done
echo "no window with \"$word\" in its title within 15 minutes"
exit 1

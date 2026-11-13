#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# A stand-in for tur-apps in tst_centre_apps: it behaves as the subject (the
# last argument, after "--") says, so that the tests can show what
# AppsProcess does about each. It is not shipped, and installs nothing.
#
#   lines         three events and a result, one line at a time
#   wait-cancel   a plan event, then waits for the line "cancel" on its
#                 input and ends as tur-apps ends a cancelled installation
#   no-input      reports whether its standard input is empty
#   sleep         sleeps for a minute
#   flood         prints without end
#   crash         ends by a signal
#   arguments     prints its arguments, one per line, as a JSON document
set -u
for last; do :; done
subject=${last:-}
case "$subject" in
lines)
    echo '{"event":"plan","plan":{"route":"flatpak"},"schema_version":1}'
    sleep 0.1
    echo '{"event":"step-started","step":"install","text":"Installing","schema_version":1}'
    echo '{"event":"progress","step":"install","done":10,"total":40,"schema_version":1}'
    printf '%s' '{"event":"result","operation":"install","result":{"id":"lines"},"schema_version":1}'
    ;;
wait-cancel)
    echo '{"event":"plan","plan":{"route":"flatpak"},"schema_version":1}'
    echo '{"event":"step-started","step":"install","text":"Installing","schema_version":1}'
    while read -r line; do
        if [ "$line" = cancel ]; then
            echo '{"event":"step-finished","step":"install","ok":false,"schema_version":1}'
            echo '{"event":"error","error":{"code":"TUR-DESKTOP-APPS-013","title":"Nothing was done without your confirmation","message":"Cancelled. Nothing was installed."},"schema_version":1}'
            exit 3
        fi
    done
    echo '{"event":"result","operation":"install","result":{},"schema_version":1}'
    ;;
no-input)
    if read -r _; then echo '{"input":"some"}'; else echo '{"input":"none"}'; fi
    ;;
sleep)
    sleep 60
    ;;
flood)
    while :; do echo '{"event":"activity","step":"install","text":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}'; done
    ;;
crash)
    kill -9 $$
    ;;
arguments)
    printf '{"arguments":['
    first=1
    for argument; do
        [ "$first" = 1 ] || printf ','
        first=0
        printf '"%s"' "$argument"
    done
    printf ']}\n'
    ;;
*)
    echo '{"error":{"code":"TUR-DESKTOP-APPS-002","title":"No application has this identifier","message":"no such thing"}}'
    exit 6
    ;;
esac

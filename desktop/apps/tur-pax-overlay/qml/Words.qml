// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick
import Tur.Kit.Pax as Pax

// What the overlay says, in one place (design, 9.3 and 5.6). P@x's name is a
// name and is not translated.
QtObject {
    id: words

    readonly property string name: "P@x"

    // The pill's words for a state. The words differ for every state, so
    // that colour is never the only difference.
    function activity(state: string, scope: string): string {
        switch (state) {
        case "idle":
            return qsTr("P@x is on")
        case "listening":
            return qsTr("Listening…")
        case "working":
            return qsTr("P@x is working")
        case "observing":
            return scope === "screen" ? qsTr("P@x can see this screen") : qsTr("P@x can see this window")
        case "acting":
            return qsTr("P@x is controlling")
        case "waiting":
            return qsTr("P@x is waiting for you")
        case "paused":
            return qsTr("P@x is paused")
        }
        return ""
    }

    // What the pill says: the state, then, at all times while P@x is on, the
    // stop combination and the key that lists P@x's other keys (the owner's
    // answer of 12 November 2026, desktop-experience.md 17.1, answer 2). The
    // keys come from Tur.Kit.Pax's PaxCommands, which is generated from the
    // one file the bindings come from (desktop/kit/pax/commands.json).
    function pill(state: string, scope: string): string {
        //: %1 is what P@x is doing; %2 and %3 are key combinations, such as Meta+Esc.
        return qsTr("%1 · Stop: %2 · Keys: %3").arg(words.activity(state, scope)).arg(Pax.PaxCommands.stopKeys)
            .arg(Pax.PaxCommands.listKeys)
    }

    readonly property string commandsTitle: qsTr("P@x's keys")
}

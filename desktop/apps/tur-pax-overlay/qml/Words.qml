// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// What the overlay says, in one place (design, 9.3 and 5.6). P@x's name is a
// name and is not translated.
QtObject {
    readonly property string name: "P@x"

    // The pill's words for a state. The words differ for every state, so
    // that colour is never the only difference.
    function activity(state: string, scope: string): string {
        switch (state) {
        case "listening":
            return qsTr("Listening…")
        case "working":
            return qsTr("P@x is working")
        case "observing":
            return scope === "screen" ? qsTr("P@x can see this screen") : qsTr("P@x can see this window")
        case "acting":
            return qsTr("P@x is controlling · Stop: Meta+Esc")
        case "waiting":
            return qsTr("P@x is waiting for you")
        case "paused":
            return qsTr("P@x is paused")
        }
        return ""
    }
}

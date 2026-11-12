// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// GENERATED from pax/commands.json by pax/gen_commands.py. Do not edit.
// Change commands.json, then run: python3 desktop/kit/pax/gen_commands.py --write
pragma Singleton

import QtQuick

// P@x's keyboard commands. Only P@x's surfaces import Tur.Kit.Pax.
QtObject {
    // Shown by P@x's indicator at all times while P@x is on.
    readonly property string stopKeys: "Meta+Esc"
    // The command that shows the full list.
    readonly property string listKeys: "Meta+/"

    // Every command, in the order of commands.json.
    readonly property var commands: [
        { id: "stop", keys: "Meta+Esc", action: qsTranslate("PaxCommands", "Stop P@x"), bound: false },
        { id: "ask", keys: "Meta+Space", action: qsTranslate("PaxCommands", "Ask P@x"), bound: false },
        { id: "talk", keys: "Hold Meta+Space", action: qsTranslate("PaxCommands", "Talk to P@x while held"), bound: false },
        { id: "keys", keys: "Meta+/", action: qsTranslate("PaxCommands", "Show P@x's keys"), bound: false }
    ]
}

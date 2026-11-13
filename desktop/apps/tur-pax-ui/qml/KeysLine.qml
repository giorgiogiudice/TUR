// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// P@x's keyboard commands, shown wherever P@x's window is while P@x is on
// (the owner's answer of 12 November 2026, desktop-experience.md 17.1,
// answer 2). The keys and their words come from Tur.Kit.Pax's PaxCommands,
// generated from desktop/kit/pax/commands.json, the file the bindings in
// data/org.tur.PaxUi.desktop come from, so the two cannot disagree.
Flow {
    id: keys

    // Only the commands that something binds; the rest are not yet keys.
    property bool boundOnly: true

    objectName: "paxKeys"
    spacing: Kit.Theme.spaceGroup

    Accessible.role: Accessible.List
    Accessible.name: Words.keysTitle

    Repeater {
        model: Pax.PaxCommands.commands.filter(command => !keys.boundOnly || command.bound)

        delegate: Kit.Label {
            id: command

            required property var modelData

            objectName: "key-" + command.modelData.id
            width: Math.min(command.implicitWidth, keys.width)
            //: One of P@x's keys: %1 is the key combination, which is not translated; %2 what it does.
            text: qsTr("%1 · %2").arg(command.modelData.keys).arg(command.modelData.action)
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }
    }
}

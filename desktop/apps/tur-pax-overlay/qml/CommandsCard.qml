// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The full list of P@x's keyboard commands, under the pill, when it is asked
// for (ShowCommands, or the key the pill names). The owner's answer of 12
// November 2026 (desktop-experience.md 17.1, answer 2): the list is one action
// away from the indicator. Its keys and words come from Tur.Kit.Pax's
// PaxCommands, generated from desktop/kit/pax/commands.json, the file the
// bindings come from. Like everything on the overlay it takes no input.
Rectangle {
    id: card

    // The widest it may be.
    property real maxWidth: 10000

    readonly property real padding: Kit.Theme.spaceGroup
    // The longest key combination, so that the keys line up in a column.
    readonly property string widestKeys: {
        let widest = ""
        for (const command of Pax.PaxCommands.commands) {
            if (command.keys.length > widest.length)
                widest = command.keys
        }
        return widest
    }
    readonly property var layers: [Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayOutline]

    width: Math.min(card.maxWidth, column.implicitWidth + 2 * card.padding)
    height: column.implicitHeight + 2 * card.padding
    radius: Kit.Theme.radiusSheet
    color: Pax.PaxTheme.overlayOutline
    border.width: 1.5
    border.color: Pax.PaxTheme.overlayFill

    Accessible.role: Accessible.List
    Accessible.name: Words.commandsTitle

    // Not shown: it gives the keys' font to the measure below.
    Kit.Label {
        id: keysSample

        visible: false
        kind: Kit.Label.Strong
        text: card.widestKeys
    }

    TextMetrics {
        id: keysMetrics

        font: keysSample.font
        text: card.widestKeys
    }

    ColumnLayout {
        id: column

        x: card.padding
        y: card.padding
        width: card.width - 2 * card.padding
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            text: Words.commandsTitle
            kind: Kit.Label.Strong
            color: Pax.PaxTheme.overlayQuietText
            wrapMode: Text.Wrap
        }

        Repeater {
            model: Pax.PaxCommands.commands

            delegate: RowLayout {
                id: line

                required property var modelData

                objectName: "command-" + line.modelData.id
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGroup

                Kit.Label {
                    objectName: "commandKeys"
                    Layout.preferredWidth: Math.ceil(keysMetrics.advanceWidth) + 1
                    Layout.maximumWidth: column.width / 2
                    text: line.modelData.keys
                    kind: Kit.Label.Strong
                    color: Pax.PaxTheme.overlayFill
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    id: action

                    objectName: "commandAction"
                    Layout.fillWidth: true
                    Layout.preferredWidth: Math.ceil(actionMetrics.advanceWidth) + 1
                    text: line.modelData.action
                    color: Pax.PaxTheme.overlayQuietText
                    wrapMode: Text.Wrap

                    TextMetrics {
                        id: actionMetrics

                        font: action.font
                        text: line.modelData.action
                    }
                }
            }
        }
    }
}

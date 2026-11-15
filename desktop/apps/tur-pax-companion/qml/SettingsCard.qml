// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What the character may tell the person (tur-pax-companion.md, section 5):
// each kind of notice with its own switch, suggestions (off unless switched
// on, with exactly what they are made from), and whether the character
// stays over a full-screen window. Every switch takes effect at once.
Leaf {
    id: card

    required property CompanionSettings settings

    signal backRequested()

    Accessible.role: Accessible.Grouping
    Accessible.name: Words.settingsTitle

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            objectName: "settingsTitle"
            Layout.fillWidth: true
            text: Words.settingsTitle
            kind: Kit.Label.Heading
        }

        Kit.Label {
            objectName: "settingsNote"
            Layout.fillWidth: true
            Layout.bottomMargin: Kit.Theme.spaceGrid
            text: Words.settingsNote
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Repeater {
            model: ["repair", "fault", "approval", "task", "update"]

            delegate: Kit.Switch {
                id: kindSwitch

                required property string modelData

                objectName: "kind-" + kindSwitch.modelData
                Layout.fillWidth: true
                text: Words.kind(kindSwitch.modelData)
                checked: card.settings.kinds[kindSwitch.modelData] === true
                onToggled: card.settings.setKind(kindSwitch.modelData, kindSwitch.checked)
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        Kit.Switch {
            id: suggestionsSwitch

            objectName: "suggestionsSwitch"
            Layout.fillWidth: true
            text: Words.suggestionsSwitch
            checked: card.settings.suggestions
            onToggled: card.settings.suggestions = suggestionsSwitch.checked
        }

        Kit.Label {
            objectName: "suggestionsNote"
            Layout.fillWidth: true
            text: Words.suggestionsNote.arg(card.settings.suggestionsPerHour)
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        Kit.Switch {
            id: staySwitch

            objectName: "staySwitch"
            Layout.fillWidth: true
            text: Words.stayOver
            checked: card.settings.overFullScreen
            onToggled: card.settings.overFullScreen = staySwitch.checked
        }

        Kit.Label {
            objectName: "stayNote"
            Layout.fillWidth: true
            text: Words.stayOverNote
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Kit.Switch {
            id: asleepSwitch

            objectName: "asleepSwitch"
            Layout.fillWidth: true
            text: Words.stayAsleep
            checked: card.settings.presence === "always"
            onToggled: card.settings.presence = asleepSwitch.checked ? "always" : "with-pax"
        }

        Kit.Button {
            objectName: "backButton"
            Layout.topMargin: Kit.Theme.spaceRelated
            text: Words.back
            kind: Kit.Button.Quiet
            onClicked: card.backRequested()
        }
    }
}

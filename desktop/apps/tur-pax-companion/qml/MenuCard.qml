// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What a long press, a right click or the menu key on the character opens
// (tur-pax-companion.md, section 5): quiet in one press, "What is P@x
// watching?", what it may tell, hide, and stop while P@x works. Stop is the
// person's control over P@x and is the system's, not P@x's: it is not gold.
Leaf {
    id: menu

    // The character is quiet now.
    property bool quiet: false
    // P@x is off: the menu offers its setup instead of stop.
    property bool asleep: false
    // P@x has a task that can be stopped.
    property bool stoppable: false

    signal quietRequested(string mode)
    signal watchingRequested()
    signal settingsRequested()
    signal hideRequested()
    signal stopRequested()
    signal setupRequested()
    signal closeRequested()

    Accessible.role: Accessible.Grouping
    Accessible.name: Words.menuTitle

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            objectName: "menuTitle"
            Layout.fillWidth: true
            text: Words.menuTitle
            kind: Kit.Label.Heading
        }

        Kit.Button {
            objectName: "stopButton"
            Layout.fillWidth: true
            visible: menu.stoppable
            text: Words.stop
            kind: Kit.Button.Primary
            onClicked: menu.stopRequested()
        }

        Kit.Label {
            objectName: "quietTitle"
            Layout.fillWidth: true
            text: menu.quiet ? Words.quietNow : Words.quietTitle
            kind: menu.quiet ? Kit.Label.Body : Kit.Label.Small
            tone: menu.quiet ? Kit.Label.Primary : Kit.Label.Secondary
            paragraph: menu.quiet
        }

        Flow {
            objectName: "quietChoices"
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Kit.Button {
                objectName: "quietOff"
                visible: menu.quiet
                text: Words.quietOff
                onClicked: menu.quietRequested("off")
            }

            Kit.Button {
                objectName: "quietHour"
                visible: !menu.quiet
                text: Words.quietHour
                onClicked: menu.quietRequested("hour")
            }

            Kit.Button {
                objectName: "quietTomorrow"
                visible: !menu.quiet
                text: Words.quietTomorrow
                onClicked: menu.quietRequested("tomorrow")
            }

            Kit.Button {
                objectName: "quietAlways"
                visible: !menu.quiet
                text: Words.quietAlways
                onClicked: menu.quietRequested("always")
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        Kit.ListRow {
            objectName: "watchingRow"
            Layout.fillWidth: true
            onOverlay: true
            text: Words.watching
            onClicked: menu.watchingRequested()
        }

        Kit.ListRow {
            objectName: "settingsRow"
            Layout.fillWidth: true
            onOverlay: true
            text: Words.settings
            onClicked: menu.settingsRequested()
        }

        Kit.ListRow {
            objectName: "setupRow"
            Layout.fillWidth: true
            visible: menu.asleep
            onOverlay: true
            text: Words.setUp
            onClicked: menu.setupRequested()
        }

        Kit.ListRow {
            objectName: "hideRow"
            Layout.fillWidth: true
            onOverlay: true
            text: Words.hide
            subtitle: Words.hideNote
            onClicked: menu.hideRequested()
        }

        Kit.Button {
            objectName: "closeButton"
            text: Words.close
            kind: Kit.Button.Quiet
            onClicked: menu.closeRequested()
        }
    }
}

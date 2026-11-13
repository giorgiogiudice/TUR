// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 3: where to install. Every disk, with what is on it; the medium the
// installer runs from is shown and cannot be chosen. Then how: "Erase this
// disk and install TUR", or, on a disk that holds Windows with free space
// beside it, "Use the free space beside Windows" (issue 64, step 1;
// ADR-0028). When that cannot be done on the chosen disk, the screen says why
// and what to do.
WizardScreen {
    id: screen

    required property Engine engine
    // The kernel name of the disk chosen so far, to show it chosen again.
    property string chosenName
    // How: "erase" or "beside".
    property string mode: "erase"

    readonly property var chosenDisk: {
        for (const disk of screen.engine.disks) {
            if (disk.name === screen.chosenName && Words.choosable(disk))
                return disk
        }
        return null
    }

    signal diskChosen(var disk)
    signal modeChosen(string mode)
    signal backRequested()
    signal continueRequested()

    function focusFirstDisk(): void {
        for (let i = 0; i < cards.count; i++) {
            const card = cards.itemAt(i) as DiskCard
            if (card !== null && card.choosable) {
                if (screen.chosenName.length === 0 || card.selected) {
                    screen.focusOn(card)
                    return
                }
            }
        }
        screen.focusOn(lookAgain)
    }

    initialFocus: lookAgain

    Component.onCompleted: {
        screen.engine.runDisks()
        // A list that was already there when the screen opened.
        if (screen.engine.disksState === "done")
            Qt.callLater(screen.focusFirstDisk)
    }

    Connections {
        target: screen.engine

        function onDisksChanged(): void {
            if (screen.engine.disksState !== "done")
                return
            // The chosen disk as listed now: a second look may have found
            // it changed, and the engine compares with what it lists.
            if (screen.chosenDisk !== null)
                screen.diskChosen(screen.chosenDisk)
            Qt.callLater(screen.focusFirstDisk)
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Where to install TUR")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Choose the disk TUR will use, then how. Erasing a disk loses everything on it. On a disk that holds Windows, TUR can use free space beside Windows instead and keep it. The other disks are not touched.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.BusyIndicator {
        visible: screen.engine.disksState === "running"
        label: qsTr("Finding disks")
    }

    Repeater {
        id: cards

        model: screen.engine.disksState === "done" ? screen.engine.disks : []

        DiskCard {
            required property var modelData

            objectName: "disk-" + modelData.name
            Layout.fillWidth: true
            disk: modelData
            selected: screen.chosenDisk !== null && screen.chosenDisk.name === modelData.name
            onClicked: {
                screen.diskChosen(modelData)
                Transcript.write("disk-chosen name=" + modelData.name + " serial=\"" + modelData.serial + "\" windows="
                                 + (modelData.windows ? "yes" : "no"))
            }
        }
    }

    Kit.Message {
        Layout.fillWidth: true
        visible: screen.engine.disksState === "done" && screen.engine.selectable === 0
        severity: Kit.Message.Error
        code: "TUR-INSTALL-MEDIUM-003"
        title: qsTr("There is no disk TUR can be installed on.")
        reason: qsTr("Nothing was changed. If the disk you expect is missing, the installer has no driver for its controller, or the firmware hides it.")
        whatToDo: qsTr("Look for a RAID or Intel RST setting in the firmware and set the disk controller to AHCI or NVMe. Then start from this medium again.")
        supportBundleAvailable: false
    }

    Kit.Message {
        Layout.fillWidth: true
        visible: screen.engine.disksState === "failed"
        severity: Kit.Message.Error
        code: String(screen.engine.stepFailure.code || "")
        title: qsTr("The installer could not list the disks.")
        whatToDo: qsTr("Look again. If it happens again, save the log to a USB stick and report it.")
        technicalDetail: String(screen.engine.stepFailure.detail || "")
        supportBundleAvailable: false
    }

    // --- how -------------------------------------------------------------------
    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        visible: screen.chosenDisk !== null
        text: qsTr("How to install")
        kind: Kit.Label.Heading
    }

    Kit.Card {
        Layout.fillWidth: true
        visible: screen.chosenDisk !== null
        padding: Kit.Theme.spaceRelated

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGrid

            Kit.ListRow {
                objectName: "eraseOption"
                Layout.fillWidth: true
                text: qsTr("Erase this disk and install TUR")
                subtitle: screen.chosenDisk !== null && screen.chosenDisk.windows
                          ? qsTr("Everything on the disk, Windows too, is replaced.")
                          : qsTr("Everything on the disk is replaced by TUR.")
                selected: screen.mode !== "beside"
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    screen.modeChosen("erase")
                    Transcript.write("mode-chosen mode=erase")
                }
            }

            Kit.ListRow {
                objectName: "besideWindowsOption"
                Layout.fillWidth: true
                text: qsTr("Use the free space beside Windows")
                subtitle: Words.besidePossible(screen.chosenDisk)
                          ? qsTr("Keeps Windows. Uses %1 of free space.").arg(Words.size(screen.chosenDisk.beside.free_size || 0))
                          : qsTr("Not available on this disk.")
                enabled: Words.besidePossible(screen.chosenDisk)
                selected: screen.mode === "beside" && Words.besidePossible(screen.chosenDisk)
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    screen.modeChosen("beside")
                    Transcript.write("mode-chosen mode=beside")
                }
            }
        }
    }

    Kit.Label {
        objectName: "besideExplanation"
        Layout.fillWidth: true
        visible: screen.chosenDisk !== null
        text: Words.besidePossible(screen.chosenDisk)
              ? qsTr("Beside Windows, TUR goes into the free space you made in Windows. Windows' partitions and files stay as they are; TUR adds its start-up files to Windows' EFI system partition. When the computer starts, TUR's menu offers TUR and Windows.")
              : Words.besideWhyNot(screen.chosenDisk)
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "backButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Back")
            onClicked: screen.backRequested()
        }

        Kit.Button {
            id: lookAgain

            objectName: "lookAgainButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Look again")
            enabled: screen.engine.disksState !== "running"
            onClicked: screen.engine.runDisks()
        }

        Item {
            Layout.fillWidth: true
        }

        Kit.Button {
            id: continueButton

            objectName: "continueButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Continue")
            kind: Kit.Button.Primary
            enabled: screen.chosenDisk !== null && screen.engine.disksState === "done"
            Keys.onReturnPressed: continueButton.clicked()
            Keys.onEnterPressed: continueButton.clicked()
            onClicked: screen.continueRequested()
        }
    }
}

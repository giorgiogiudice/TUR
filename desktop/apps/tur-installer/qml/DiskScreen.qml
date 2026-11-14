// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 3: where to install. Every disk, with what is on it; the medium the
// installer runs from is shown and cannot be chosen. Then how: "Erase this
// disk and install TUR", or, on a disk that holds Windows with free space
// beside it, "Use the free space beside Windows" (issue 64, step 1;
// ADR-0029). When that cannot be done on the chosen disk, the screen says why
// and what to do.
WizardScreen {
    id: screen

    required property Engine engine
    // The kernel name of the disk chosen so far, to show it chosen again.
    property string chosenName
    // How: "erase" or "beside".
    property string mode: "erase"
    // Beside Windows: which system starts by default, "tur" or "windows".
    property string defaultSystem: "tur"
    // Shrinking Windows: the room for TUR in GiB.
    property int turGiB: 0

    readonly property int turMinGiB: Words.shrinkPossible(screen.chosenDisk)
                                     ? Math.ceil(Number(screen.chosenDisk.shrink.tur_min || 0) / 1073741824) : 0
    readonly property int turMaxGiB: Words.shrinkPossible(screen.chosenDisk)
                                     ? Math.floor(Number(screen.chosenDisk.shrink.tur_max || 0) / 1073741824) : 0
    readonly property bool sizeOk: screen.mode !== "shrink"
                                   || (screen.turGiB >= screen.turMinGiB && screen.turGiB <= screen.turMaxGiB && screen.turMinGiB > 0)
    readonly property bool keepsWindows: (screen.mode === "beside" && Words.besidePossible(screen.chosenDisk))
                                         || (screen.mode === "shrink" && Words.shrinkPossible(screen.chosenDisk))

    readonly property var chosenDisk: {
        for (const disk of screen.engine.disks) {
            if (disk.name === screen.chosenName && Words.choosable(disk))
                return disk
        }
        return null
    }

    signal diskChosen(var disk)
    signal modeChosen(string mode)
    signal defaultChosen(string system)
    signal turSizeChosen(int gib)
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

            Kit.ListRow {
                objectName: "shrinkWindowsOption"
                Layout.fillWidth: true
                text: qsTr("Make room by shrinking Windows")
                subtitle: Words.shrinkPossible(screen.chosenDisk)
                          ? qsTr("Keeps Windows' files. Back up first.")
                          : qsTr("Not available on this disk.")
                enabled: Words.shrinkPossible(screen.chosenDisk)
                selected: screen.mode === "shrink" && Words.shrinkPossible(screen.chosenDisk)
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    screen.modeChosen("shrink")
                    if (screen.turGiB < screen.turMinGiB || screen.turGiB > screen.turMaxGiB)
                        screen.turSizeChosen(screen.turMinGiB)
                    Transcript.write("mode-chosen mode=shrink")
                }
            }
        }
    }

    // --- shrinking Windows: how much room ------------------------------------------
    Kit.Card {
        Layout.fillWidth: true
        visible: screen.mode === "shrink" && Words.shrinkPossible(screen.chosenDisk)
        padding: Kit.Theme.spaceRelated

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGrid

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Windows' partition is %1. It can give TUR between %2 GiB and %3 GiB; %4 stay free for Windows.")
                    .arg(Words.size(Number(screen.chosenDisk !== null && screen.chosenDisk.shrink ? screen.chosenDisk.shrink.size || 0 : 0)))
                    .arg(screen.turMinGiB).arg(screen.turMaxGiB).arg(Words.size(17179869184))
                wrapMode: Text.Wrap
            }

            Kit.TextField {
                id: sizeField

                objectName: "turSizeField"
                Layout.fillWidth: true
                Layout.maximumWidth: 440
                label: qsTr("Room for TUR, in GiB")
                text: screen.turGiB > 0 ? String(screen.turGiB) : ""
                inputMethodHints: Qt.ImhDigitsOnly
                validator: IntValidator {
                    bottom: 1
                    top: 100000
                }
                helperText: qsTr("From %1 to %2.").arg(screen.turMinGiB).arg(screen.turMaxGiB)
                errorText: screen.sizeOk ? "" : qsTr("Type a whole number from %1 to %2.").arg(screen.turMinGiB).arg(screen.turMaxGiB)
                onTextEdited: {
                    screen.turSizeChosen(parseInt(sizeField.text) || 0)
                    Transcript.write("size-chosen gib=" + (parseInt(sizeField.text) || 0))
                }
            }

            Kit.Label {
                objectName: "shrinkBackupNote"
                Layout.fillWidth: true
                text: qsTr("A backup of everything on this disk is strongly recommended before Windows is shrunk: if the power fails while Windows' data is moved, Windows' partition can be damaged.")
                kind: Kit.Label.Strong
                wrapMode: Text.Wrap
            }
        }
    }

    // --- beside Windows: which system starts -----------------------------------------
    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        visible: screen.keepsWindows
        text: qsTr("When you switch the computer on")
        kind: Kit.Label.Heading
    }

    Kit.Card {
        Layout.fillWidth: true
        visible: screen.keepsWindows
        padding: Kit.Theme.spaceRelated

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGrid

            Kit.ListRow {
                objectName: "defaultTurOption"
                Layout.fillWidth: true
                text: qsTr("Start TUR")
                subtitle: qsTr("TUR's start menu also offers Windows.")
                selected: screen.defaultSystem !== "windows"
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    screen.defaultChosen("tur")
                    Transcript.write("default-chosen system=tur")
                }
            }

            Kit.ListRow {
                objectName: "defaultWindowsOption"
                Layout.fillWidth: true
                text: qsTr("Start Windows")
                subtitle: qsTr("Start TUR from the computer's boot menu.")
                selected: screen.defaultSystem === "windows"
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    screen.defaultChosen("windows")
                    Transcript.write("default-chosen system=windows")
                }
            }
        }
    }

    Kit.Label {
        objectName: "defaultExplanation"
        Layout.fillWidth: true
        visible: screen.keepsWindows
        text: qsTr("You can change this later in TUR. To start Windows from TUR once, use “Restart into Windows”: it starts Windows the way the computer itself does, so BitLocker does not ask for its recovery key.")
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        objectName: "besideExplanation"
        Layout.fillWidth: true
        visible: screen.chosenDisk !== null
        text: screen.mode === "shrink"
              ? qsTr("The installer shrinks Windows' partition with ntfsresize, then puts TUR into the space it frees, as beside Windows. Windows checks its partition the next time it starts.")
              : Words.besidePossible(screen.chosenDisk)
                ? qsTr("Beside Windows, TUR goes into the free space you made in Windows. Windows' partitions and files stay as they are; TUR adds its start-up files to Windows' EFI system partition. When the computer starts, TUR's menu offers TUR and Windows.")
                : Words.besideWhyNot(screen.chosenDisk)
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    // Why Windows cannot be shrunk from here, and what to do, when it cannot.
    Kit.Label {
        objectName: "shrinkExplanation"
        Layout.fillWidth: true
        visible: screen.chosenDisk !== null && screen.chosenDisk.windows === true && !Words.shrinkPossible(screen.chosenDisk)
        text: screen.chosenDisk !== null && screen.chosenDisk.windows === true
              ? qsTr("Shrinking Windows from here is not possible. %1").arg(Words.shrinkWhyNot(screen.chosenDisk)) : ""
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
            enabled: screen.chosenDisk !== null && screen.engine.disksState === "done" && screen.sizeOk
            Keys.onReturnPressed: continueButton.clicked()
            Keys.onEnterPressed: continueButton.clicked()
            onClicked: screen.continueRequested()
        }
    }
}

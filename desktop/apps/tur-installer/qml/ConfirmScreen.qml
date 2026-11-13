// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 5: the destructive confirmation (desktop-experience.md, 11.5;
// specification 28.5; AGENTS.md: never a destructive storage operation
// without explicit confirmation that shows the exact target and its stable
// identity).
//
// The only screen with a red frame. It shows the disk by model, capacity,
// connection, its whole serial number (with a privacy note) and its stable
// path; what is on it now; what will be on it; what will be destroyed; a
// separate warning when it holds Windows or BitLocker. Then the phrase the
// engine derived from the disk's identity, "ERASE" and the end of its serial
// number. Installing beside Windows (ADR-0029) is confirmed on the same
// screen, which then says that everything on the disk is kept, where TUR goes
// and what is added to Windows' EFI system partition, with its own phrase,
// "BESIDE" and the same end. The button is enabled only when the typed text matches exactly;
// pasting is refused; Enter does nothing; nothing has the keyboard but the
// field when the screen opens: there is no default button. The engine
// compares the phrase again itself before it writes.
WizardScreen {
    id: screen

    required property var disk
    // "erase" the whole disk, or "beside" Windows.
    property string mode: "erase"
    // Beside Windows: "tur" or "windows", the system started by default.
    property string defaultSystem: "tur"
    // Shrinking Windows: the room for TUR in GiB.
    property int turGiB: 0
    readonly property bool shrink: screen.mode === "shrink" && Words.shrinkPossible(screen.disk)

    readonly property bool beside: (screen.mode === "beside" && Words.besidePossible(screen.disk))
                                   || (screen.mode === "shrink" && Words.shrinkPossible(screen.disk))
    readonly property var room: screen.mode === "beside" && screen.beside ? screen.disk.beside : ({
        free_size: screen.turGiB * 1073741824,
        before: screen.disk.shrink ? String(screen.disk.shrink.partition || "") : "",
        esp: screen.disk.beside ? String(screen.disk.beside.esp || "") : "",
        esp_needed: screen.disk.beside ? Number(screen.disk.beside.esp_needed || 0) : 0,
        esp_free: screen.disk.beside ? Number(screen.disk.beside.esp_free || 0) : 0
    })
    readonly property string phrase: screen.shrink ? String(screen.disk.shrink.phrase || "")
                                     : screen.beside ? String(screen.room.phrase || "") : String(screen.disk.phrase || "")
    readonly property bool matches: screen.phrase.length > 0 && field.text === screen.phrase
    readonly property bool windows: screen.beside ? screen.disk.bitlocker === true
                                                  : screen.disk.windows === true || screen.disk.bitlocker === true
    readonly property real diskSize: Number(screen.disk.size || 0)

    signal backRequested()
    signal eraseRequested(string phrase)

    function writePhraseState(): void {
        Transcript.write("phrase-typed length=" + field.text.length + " matches="
                         + (field.text === screen.phrase && screen.phrase.length > 0 ? "yes" : "no")
                         + " erase_enabled=" + (eraseButton.enabled ? "yes" : "no"))
    }

    initialFocus: field

    Component.onCompleted: {
        Transcript.write("confirm-shown name=" + screen.disk.name + " by_id=" + screen.disk.by_id + " phrase=\""
                         + screen.phrase + "\" windows_warning=" + (screen.windows ? "yes" : "no")
                         + " holds_data=" + (Words.holdsData(screen.disk) ? "yes" : "no")
                         + " mode=" + (screen.shrink ? "shrink" : screen.beside ? "beside" : "erase")
                         + (screen.beside ? " default=" + screen.defaultSystem : ""))
    }

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: frame.implicitHeight + 2 * Kit.Theme.spaceMargin
        radius: Kit.Theme.radiusCard
        color: Kit.Theme.surfaceRaised
        border.width: Kit.Theme.lineFocusWidth
        border.color: Kit.Theme.negative

        ColumnLayout {
            id: frame

            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: Kit.Theme.spaceMargin
            spacing: Kit.Theme.spaceGroup

            Kit.Label {
                Layout.fillWidth: true
                text: screen.shrink ? qsTr("Shrink Windows and install TUR beside it")
                      : screen.beside ? qsTr("Install TUR beside Windows") : qsTr("Erase this disk and install TUR")
                kind: Kit.Label.Title
                tone: Kit.Label.Negative
                wrapMode: Text.Wrap
            }

            // --- the disk --------------------------------------------------------
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("The disk")
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.diskLine(screen.disk)
                    kind: Kit.Label.Strong
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "confirmSerial"
                    Layout.fillWidth: true
                    text: qsTr("Serial number: %1").arg(String(screen.disk.serial || screen.disk.wwn || qsTr("none reported")))
                    wrapMode: Text.WrapAnywhere
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("A serial number identifies this disk. Think before you share a picture of this screen.")
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "confirmPath"
                    Layout.fillWidth: true
                    text: qsTr("Stable path: %1").arg(String(screen.disk.by_id || ""))
                    kind: Kit.Label.Mono
                    wrapMode: Text.WrapAnywhere
                }
            }

            // --- now and after ----------------------------------------------------
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: screen.beside ? qsTr("On it now, all of it kept") : qsTr("On it now")
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                }

                PartitionBar {
                    Layout.fillWidth: true
                    diskSize: Math.max(1, screen.diskSize)
                    partitions: screen.disk.partitions || []
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !Words.holdsData(screen.disk)
                    text: qsTr("No partitions. Anything on the disk is still erased.")
                    wrapMode: Text.Wrap
                }

                Repeater {
                    model: screen.disk.partitions || []

                    Kit.Label {
                        required property var modelData

                        Layout.fillWidth: true
                        text: Words.partitionLine(modelData)
                        wrapMode: Text.Wrap
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("After installing")
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                }

                PartitionBar {
                    Layout.fillWidth: true
                    diskSize: Math.max(1, screen.diskSize)
                    after: true
                    partitions: screen.beside && !screen.shrink ? (screen.disk.partitions || []) : []
                    besideRegion: screen.beside && !screen.shrink ? screen.room : null
                    visible: !screen.shrink
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !screen.beside
                    text: qsTr("1 · BIOS boot · 1.0 MiB")
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !screen.beside
                    text: qsTr("2 · EFI system · 512.0 MiB · FAT")
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !screen.beside
                    text: qsTr("3 · TUR · %1 · ext4").arg(Words.size(Math.max(0, screen.diskSize - 513 * 1048576)))
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !screen.beside
                    text: qsTr("A new partition table, written by bootc, the installer TUR's image carries. Not encrypted: encryption is not available in this version.")
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "shrinkWindowsLine"
                    Layout.fillWidth: true
                    visible: screen.shrink
                    text: qsTr("%1 · Windows' partition, shrunk from %2 to about %3 with ntfsresize. Its files are moved inside it, not deleted. Windows checks it the next time it starts.")
                        .arg(String(screen.room.before || ""))
                        .arg(Words.size(screen.disk.shrink ? Number(screen.disk.shrink.size || 0) : 0))
                        .arg(Words.size(Math.max(0, (screen.disk.shrink ? Number(screen.disk.shrink.size || 0) : 0) - screen.turGiB * 1073741824)))
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "besideNewPartition"
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: String(screen.room.after || "").length > 0
                          ? qsTr("New · TUR · %1 · ext4, in the free space between %2 and %3")
                            .arg(Words.size(screen.room.free_size || 0)).arg(String(screen.room.before || "")).arg(String(screen.room.after || ""))
                          : qsTr("New · TUR · %1 · ext4, in the free space after %2")
                            .arg(Words.size(screen.room.free_size || 0)).arg(String(screen.room.before || ""))
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: qsTr("%1 · EFI system, shared with Windows: TUR's start-up files are added (%2 of its %3 free). Windows' files there are kept.")
                        .arg(String(screen.room.esp || "")).arg(Words.size(screen.room.esp_needed || 0)).arg(Words.size(screen.room.esp_free || 0))
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "besideDefaultSystem"
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: screen.defaultSystem === "windows"
                          ? qsTr("The computer starts Windows by default. TUR is started from the computer's boot menu, as “TUR”.")
                          : qsTr("The computer starts TUR by default; TUR's menu also offers Windows. Windows stays in the computer's own boot menu.")
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: qsTr("The new partition is made by the installer and filled by bootc, the installer TUR's image carries. Not encrypted: encryption is not available in this version.")
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }
            }

            // --- what is lost ------------------------------------------------------
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("What will be destroyed")
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                }

                Kit.Message {
                    objectName: "shrinkBackupWarning"
                    Layout.fillWidth: true
                    visible: screen.shrink
                    severity: Kit.Message.Warning
                    title: qsTr("Back up first. A backup of everything on this disk is strongly recommended.")
                    reason: qsTr("Shrinking moves Windows' data inside its partition. If the power fails or the disk fails while it moves, Windows' partition can be damaged. The installer refuses a volume that Windows has not shut down fully, but it was tested only on volumes made by Linux tools, not on a real Windows volume.")
                    whatToDo: qsTr("Copy what you cannot lose to another disk before you go on. Keep the computer on mains power.")
                    supportBundleAvailable: false
                }

                Kit.Label {
                    objectName: "besideNothingDestroyed"
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: screen.shrink
                          ? qsTr("No file is deleted. Windows' partition is made smaller; nothing else of Windows is written to.")
                          : qsTr("Nothing that is on the disk now. No partition is deleted, moved or shrunk, and Windows' partitions are not written to.")
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: screen.beside
                    text: qsTr("Still, back up what matters to you first: a power cut or a faulty disk while installing can go wrong.")
                    kind: Kit.Label.Strong
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: !screen.beside
                    text: !Words.holdsData(screen.disk) ? qsTr("Everything on this disk. No data on this disk is kept.")
                          : screen.disk.partition_count === 1
                            ? qsTr("Everything on this disk: its one partition and every file on it. No data on this disk is kept.")
                            : qsTr("Everything on this disk: all %1 partitions and every file on them. No data on this disk is kept.")
                              .arg(screen.disk.partition_count)
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Your other disks are not touched.")
                    wrapMode: Text.Wrap
                }
            }

            Kit.Message {
                objectName: "windowsWarning"
                Layout.fillWidth: true
                visible: screen.windows
                severity: Kit.Message.Warning
                title: screen.beside ? qsTr("Windows on this disk uses BitLocker. Have its recovery key ready.")
                       : screen.disk.windows ? qsTr("This disk holds Windows. Erasing it removes Windows and everything on it.")
                       : qsTr("This disk holds data encrypted with BitLocker. Erasing it destroys that data.")
                reason: screen.beside ? qsTr("Starting Windows from TUR's start menu changes what BitLocker checks when the computer starts, so Windows may ask for its recovery key there. “Restart into Windows” in TUR, and the computer's own boot menu, start Windows without that.")
                        : screen.disk.bitlocker ? qsTr("It also holds a volume encrypted with BitLocker, which is lost with it.") : ""
                whatToDo: screen.beside ? qsTr("Find the recovery key before you go on: on your Microsoft account's devices page, or with “manage-bde -protectors -get C:” in Windows.")
                          : Words.besidePossible(screen.disk) ? qsTr("To keep Windows, go back and choose “Use the free space beside Windows”.")
                          : qsTr("To keep Windows, go back and choose another disk.")
                supportBundleAvailable: false
            }

            // --- the confirmation --------------------------------------------------
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceRelated

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("To confirm, type")
                    kind: Kit.Label.Strong
                }

                Kit.Label {
                    objectName: "confirmPhrase"
                    Layout.fillWidth: true
                    text: screen.phrase
                    kind: Kit.Label.Mono
                    wrapMode: Text.WrapAnywhere
                }

                Kit.TextField {
                    id: field

                    objectName: "phraseField"
                    Layout.fillWidth: true
                    Layout.maximumWidth: 440
                    label: qsTr("Confirmation phrase")
                    helperText: qsTr("Capital letters and one space, exactly as shown. Pasting is not accepted.")
                    inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase | Qt.ImhPreferUppercase
                    // Typed, not pasted; and Enter confirms nothing.
                    Keys.onPressed: event => {
                        if (event.matches(StandardKey.Paste)) {
                            event.accepted = true
                            Transcript.write("phrase-paste refused=yes")
                        }
                    }
                    Keys.onReturnPressed: event => event.accepted = true
                    Keys.onEnterPressed: event => event.accepted = true
                    // Written once the bindings have followed the text, so
                    // that the line says what the screen then shows.
                    onTextChanged: Qt.callLater(screen.writePhraseState)

                    // The middle button pastes the selection on Linux: not here.
                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.MiddleButton
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    objectName: "backButton"
                    Layout.minimumWidth: implicitWidth
                    text: qsTr("Back")
                    onClicked: screen.backRequested()
                }

                Item {
                    Layout.fillWidth: true
                }

                Kit.Button {
                    id: eraseButton

                    objectName: "eraseButton"
                    Layout.minimumWidth: implicitWidth
                    text: screen.shrink ? qsTr("Shrink Windows and install")
                          : screen.beside ? qsTr("Install beside Windows") : qsTr("Erase disk and install")
                    kind: Kit.Button.Destructive
                    enabled: screen.matches
                    onEnabledChanged: Transcript.write("erase-button enabled=" + (eraseButton.enabled ? "yes" : "no"))
                    onClicked: {
                        if (!screen.matches)
                            return
                        Transcript.write("erase-confirmed name=" + screen.disk.name + " mode="
                                         + (screen.shrink ? "shrink" : screen.beside ? "beside" : "erase"))
                        screen.eraseRequested(field.text)
                    }
                }
            }
        }
    }
}

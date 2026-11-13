// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 6: installing (desktop-experience.md, 11.5). The engine's stages as
// they really happen, read from bootc's own output: no percentage that is
// not measured. While copying, the bytes written to the disk so far, which
// the engine reads from the kernel. "Cancel" is offered while cancelling
// changes nothing, and replaced by a sentence once the disk is written.
WizardScreen {
    id: screen

    required property Engine engine
    required property var disk
    // "erase" or "beside" (Windows; ADR-0028).
    property string mode: "erase"

    // When each stage was first seen, for the time it took.
    property var startedAt: ({})
    property real now: Date.now()

    // bootc says "layers needed: 70 (3.2 GB)"; the engine passes on what is
    // after the colon. The size is of the compressed image.
    readonly property string compressed: {
        const found = /\(([^)]+)\)/.exec(screen.engine.copyTotal)
        return found !== null ? found[1] : ""
    }

    readonly property var stages: screen.mode === "beside"
                                  ? ["revalidate", "prepare", "copy", "bootloader", "finish", "boot-menu", "check", "record"]
                                  : ["revalidate", "prepare", "copy", "bootloader", "finish", "check", "record"]

    function stateOf(stage: string): string {
        if (screen.engine.stage === stage && screen.engine.installState === "running")
            return "now"
        if (screen.engine.stagesSeen.indexOf(stage) >= 0)
            return "done"
        return "waiting"
    }

    function seconds(stage: string): string {
        const index = screen.stages.indexOf(stage)
        const begun = screen.startedAt[stage]
        if (begun === undefined)
            return ""
        let ended = screen.now
        for (let i = index + 1; i < screen.stages.length; i++) {
            if (screen.startedAt[screen.stages[i]] !== undefined) {
                ended = screen.startedAt[screen.stages[i]]
                break
            }
        }
        return qsTr("%1 s").arg(Math.max(0, Math.round((ended - begun) / 1000)))
    }

    initialFocus: cancelButton

    Connections {
        target: screen.engine

        function onInstallChanged(): void {
            const stage = screen.engine.stage
            if (stage.length > 0 && screen.startedAt[stage] === undefined) {
                const next = Object.assign({}, screen.startedAt)
                next[stage] = Date.now()
                screen.startedAt = next
            }
        }
    }

    Timer {
        interval: 1000
        repeat: true
        running: screen.engine.installState === "running"
        onTriggered: screen.now = Date.now()
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Installing TUR")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: (screen.mode === "beside"
               ? qsTr("Beside Windows, on %1. This takes a few minutes. Keep the computer switched on and the installation medium in place.")
               : qsTr("On %1. This takes a few minutes. Keep the computer switched on and the installation medium in place."))
              .arg(Words.diskLine(screen.disk))
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Card {
        Layout.fillWidth: true

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceRelated

            Repeater {
                model: screen.stages

                ColumnLayout {
                    id: stageRow

                    required property string modelData

                    readonly property string stageState: screen.stateOf(stageRow.modelData)

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceGrid

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kit.Theme.spaceRelated

                        Item {
                            Layout.preferredWidth: 24
                            Layout.preferredHeight: 24

                            Kit.Glyph {
                                anchors.centerIn: parent
                                visible: stageRow.stageState === "done"
                                kind: Kit.Glyph.Check
                                tint: Kit.Theme.positive
                            }

                            Kit.BusyIndicator {
                                anchors.centerIn: parent
                                visible: stageRow.stageState === "now"
                                running: stageRow.stageState === "now"
                                label: screen.engine.stageTitle(stageRow.modelData)
                            }
                        }

                        Kit.Label {
                            Layout.fillWidth: true
                            text: screen.engine.stageTitle(stageRow.modelData)
                            kind: stageRow.stageState === "now" ? Kit.Label.Strong : Kit.Label.Body
                            tone: stageRow.stageState === "waiting" ? Kit.Label.Secondary : Kit.Label.Primary
                            wrapMode: Text.Wrap
                        }

                        Kit.Label {
                            text: stageRow.stageState === "done" ? qsTr("Done · %1").arg(screen.seconds(stageRow.modelData))
                                  : stageRow.stageState === "now" ? qsTr("Now · %1").arg(screen.seconds(stageRow.modelData))
                                  : qsTr("Waiting")
                            kind: Kit.Label.Small
                            tone: Kit.Label.Secondary
                            tabular: true
                        }
                    }

                    Kit.ProgressBar {
                        Layout.fillWidth: true
                        Layout.leftMargin: 24 + Kit.Theme.spaceRelated
                        visible: stageRow.modelData === "copy" && stageRow.stageState === "now"
                        indeterminate: true
                        label: qsTr("Writing the system to the disk")
                        detail: screen.compressed.length > 0
                                ? qsTr("%1 written to the disk · the image is %2 compressed").arg(Words.size(screen.engine.bytesWritten)).arg(screen.compressed)
                                : qsTr("%1 written to the disk").arg(Words.size(screen.engine.bytesWritten))
                    }
                }
            }
        }
    }

    Kit.Label {
        objectName: "lastOutput"
        Layout.fillWidth: true
        visible: screen.engine.outputLines.length > 0
        text: screen.engine.outputLines.length > 0 ? screen.engine.outputLines[screen.engine.outputLines.length - 1] : ""
        kind: Kit.Label.Mono
        tone: Kit.Label.Secondary
        wrapMode: Text.WrapAnywhere
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            id: cancelButton

            objectName: "cancelButton"
            Layout.minimumWidth: implicitWidth
            visible: screen.engine.cancellable
            text: qsTr("Cancel")
            onClicked: screen.engine.cancel()
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: !screen.engine.cancellable && screen.engine.installState === "running"
            spacing: Kit.Theme.spaceGrid

            Kit.Label {
                objectName: "cancelUnavailable"
                Layout.fillWidth: true
                text: qsTr("Cancel is not available while the disk is being written.")
                wrapMode: Text.Wrap
            }

            Kit.Disclosure {
                id: why

                objectName: "whyButton"
                text: qsTr("Why?")
            }

            Kit.Label {
                Layout.fillWidth: true
                visible: why.expanded
                text: screen.mode === "beside"
                      ? qsTr("Stopping now would leave an unfinished TUR partition in the free space. Windows' partitions are not written to, and Windows can still be started from the computer's boot menu. Let the installation finish; it takes a few minutes.")
                      : qsTr("Stopping now would leave the disk erased and without a working system, and it would not start this computer. Your other disks are not touched. Let the installation finish; it takes a few minutes.")
                tone: Kit.Label.Secondary
                paragraph: true
                wrapMode: Text.Wrap
            }
        }
    }
}

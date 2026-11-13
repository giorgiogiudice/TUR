// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 2: this computer. The checks the engine makes before anything else:
// the TUR image on the medium, every file of it against its SHA-256; how the
// computer was started; its processor and memory; Secure Boot. A check that
// stops the installation says so with its code and what to do, and
// "Continue" stays unavailable (desktop-experience.md, 11.5).
WizardScreen {
    id: screen

    required property Engine engine

    readonly property bool running: screen.engine.checkState === "running" || screen.engine.checkState === "idle"
    readonly property bool blocked: screen.engine.blocking.length > 0 || screen.engine.checkState === "failed"

    signal backRequested()
    signal continueRequested()

    initialFocus: screen.running || screen.blocked ? backButton : continueButton

    Component.onCompleted: {
        if (screen.engine.checkState !== "done")
            screen.engine.runCheck()
    }

    Connections {
        target: screen.engine

        function onCheckChanged(): void {
            if (screen.engine.checkState === "done" && !screen.blocked)
                screen.focusOn(continueButton)
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("This computer")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Before anything else, the installer checks the TUR image on this medium and what TUR needs from the computer. Nothing is changed.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.ProgressBar {
        objectName: "payloadProgress"
        Layout.fillWidth: true
        visible: screen.engine.checkState === "running"
        label: qsTr("Checking the TUR image on this medium")
        detail: screen.engine.payloadTotal > 0
                ? qsTr("File %1 of %2").arg(screen.engine.payloadDone).arg(screen.engine.payloadTotal)
                : qsTr("Starting")
        indeterminate: screen.engine.payloadTotal <= 0
        from: 0
        to: Math.max(1, screen.engine.payloadTotal)
        value: screen.engine.payloadDone
    }

    Kit.Card {
        Layout.fillWidth: true
        visible: screen.engine.checks.length > 0

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceRelated

            Repeater {
                model: screen.engine.checks

                RowLayout {
                    id: checkRow

                    required property var modelData

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceRelated

                    Kit.Glyph {
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 2
                        kind: checkRow.modelData.state === "ok" ? Kit.Glyph.Positive
                              : checkRow.modelData.state === "stop" ? Kit.Glyph.Negative : Kit.Glyph.Info
                        tint: checkRow.modelData.state === "ok" ? Kit.Theme.positive
                              : checkRow.modelData.state === "stop" ? Kit.Theme.negative : Kit.Theme.textSecondary
                    }

                    Kit.Label {
                        Layout.fillWidth: true
                        text: (checkRow.modelData.state === "ok" ? qsTr("Passed: ")
                               : checkRow.modelData.state === "stop" ? qsTr("Stops the installation: ") : qsTr("Note: "))
                              + Words.check(checkRow.modelData, screen.engine.facts)
                        wrapMode: Text.Wrap
                    }

                    Kit.CodeLabel {
                        visible: String(checkRow.modelData.code || "").length > 0
                        code: String(checkRow.modelData.code || "")
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                visible: screen.engine.checkState === "done"
                spacing: Kit.Theme.spaceRelated

                Kit.Glyph {
                    Layout.alignment: Qt.AlignTop
                    Layout.topMargin: 2
                    kind: Kit.Glyph.Info
                    tint: Kit.Theme.textSecondary
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Note: no network is needed. The installation uses none.")
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    Repeater {
        model: screen.engine.blocking

        Kit.Message {
            id: block

            required property var modelData

            Layout.fillWidth: true
            severity: Kit.Message.Error
            code: String(block.modelData.code || "")
            title: qsTr("TUR cannot be installed on this computer from here.")
            whatToDo: String(block.modelData.text || "")
            supportBundleAvailable: false
        }
    }

    Kit.Message {
        Layout.fillWidth: true
        visible: screen.engine.checkState === "failed"
        severity: Kit.Message.Error
        code: String(screen.engine.stepFailure.code || "")
        title: qsTr("The installer could not examine this computer.")
        reason: screen.engine.stepFailure.refused ? qsTr("The system did not allow the installer to look at the disks.")
                                                  : qsTr("The part of the installer that examines the computer stopped.")
        whatToDo: qsTr("Try again. If it happens again, save the log to a USB stick and report it.")
        technicalDetail: String(screen.engine.stepFailure.detail || "")
        actions: [qsTr("Try again")]
        supportBundleAvailable: false
        onActionTriggered: screen.engine.runCheck()
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            id: backButton

            objectName: "backButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Back")
            onClicked: screen.backRequested()
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
            enabled: screen.engine.checkState === "done" && !screen.blocked
            Keys.onReturnPressed: continueButton.clicked()
            Keys.onEnterPressed: continueButton.clicked()
            onClicked: screen.continueRequested()
        }
    }
}

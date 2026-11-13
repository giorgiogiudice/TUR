// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The result (desktop-experience.md, 11.5, screen 6). Installed: remove the
// medium and restart. Not installed: a two-layer message with the code, the
// state the disk is in now, and a way to save the installer's log to a USB
// stick. Cancelled before anything was written: nothing changed.
WizardScreen {
    id: screen

    required property Engine engine
    required property var disk

    readonly property string outcome: screen.engine.installState
    readonly property bool success: screen.outcome === "success"
    readonly property string diskState: String(screen.engine.result.disk_state || "")

    signal restartRequested()
    signal powerOffRequested()
    signal saveLogRequested()
    signal backToDisksRequested()

    // What a failure means for the disk, in words.
    function whatNow(): string {
        switch (screen.diskState) {
        case "untouched":
            return qsTr("Nothing was changed on any disk. Go back and look at the disks again.")
        case "erased":
            return qsTr("The disk has been partly written. It will not start this computer. Your other disks were not touched. Save the log to a USB stick and report the problem, then start the installation again.")
        default:
            return qsTr("The disk may have been partly written; do not rely on it. Your other disks were not touched. Save the log to a USB stick and report the problem.")
        }
    }

    function title(): string {
        switch (String(screen.engine.result.code || "")) {
        case "TUR-INSTALL-CONFIRM-001":
            return qsTr("The confirmation did not name this disk.")
        case "TUR-INSTALL-CONFIRM-004":
            return qsTr("This disk has changed since you were shown it.")
        case "TUR-INSTALL-IDENTITY-001":
        case "TUR-INSTALL-IDENTITY-002":
        case "TUR-INSTALL-IDENTITY-003":
            return qsTr("The disk is not the one you were shown.")
        case "TUR-INSTALL-MEDIUM-001":
            return qsTr("The TUR image on this medium cannot be used.")
        case "TUR-INSTALL-MEDIUM-004":
            return qsTr("The installer was not allowed to install.")
        default:
            return qsTr("The installation did not complete.")
        }
    }

    initialFocus: screen.success ? restartButton : (screen.outcome === "failed" ? saveButton : backButton)

    Component.onCompleted: Transcript.write("result-shown outcome=" + screen.outcome + " code="
                                            + String(screen.engine.result.code || "") + " disk_state=" + screen.diskState)

    Kit.Label {
        Layout.fillWidth: true
        text: screen.success ? qsTr("TUR is installed")
              : screen.outcome === "cancelled" ? qsTr("The installation was cancelled")
              : qsTr("TUR was not installed")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    // --- installed --------------------------------------------------------------
    Kit.Banner {
        objectName: "removeMedium"
        Layout.fillWidth: true
        visible: screen.success
        severity: Kit.Banner.Positive
        text: qsTr("Remove the installation medium (the USB stick or disc), then restart.")
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.success
        text: qsTr("TUR was installed on %1. Writing it took %2 seconds.")
              .arg(Words.diskLine(screen.disk)).arg(Number(screen.engine.result.seconds || 0))
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.success
        text: qsTr("When it starts for the first time, TUR asks for your language and keyboard and creates your account.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.success && String(screen.engine.result.log_on_disk || "").length > 0
        text: qsTr("The record of this installation is kept on the new system, in %1.")
              .arg(String(screen.engine.result.log_on_disk || ""))
        tone: Kit.Label.Secondary
        wrapMode: Text.WrapAnywhere
    }

    // --- cancelled ----------------------------------------------------------------
    Kit.Label {
        Layout.fillWidth: true
        visible: screen.outcome === "cancelled"
        text: qsTr("It was cancelled before anything was written. Nothing was changed on any disk.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    // --- not installed --------------------------------------------------------------
    Kit.Message {
        objectName: "failureMessage"
        Layout.fillWidth: true
        visible: screen.outcome === "failed" || screen.outcome === "refused"
        severity: Kit.Message.Error
        code: String(screen.engine.result.code || "TUR-INSTALL-MEDIUM-002")
        title: screen.title()
        reason: String(screen.engine.result.message || "")
        whatToDo: screen.whatNow()
        technicalDetail: {
            const lines = screen.engine.outputLines.slice(-20)
            const detail = String(screen.engine.result.detail || "")
            return (detail.length > 0 ? [detail].concat(lines) : lines).join("\n")
        }
        supportBundleAvailable: false
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.engine.saveState === "done"
        text: qsTr("The log was saved to the USB stick, in the folder %1.").arg(String(screen.engine.saveResult.folder || ""))
        tone: Kit.Label.Positive
        wrapMode: Text.WrapAnywhere
    }

    Flow {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            id: backButton

            objectName: "backToDisksButton"
            visible: !screen.success && screen.diskState === "untouched"
            text: qsTr("Back to the disks")
            onClicked: screen.backToDisksRequested()
        }

        Kit.Button {
            id: restartButton

            objectName: "restartButton"
            text: qsTr("Restart now")
            kind: screen.success ? Kit.Button.Primary : Kit.Button.Secondary
            onClicked: screen.restartRequested()
        }

        Kit.Button {
            objectName: "powerOffButton"
            text: qsTr("Shut down")
            onClicked: screen.powerOffRequested()
        }

        Kit.Button {
            id: saveButton

            objectName: "saveLogButton"
            text: screen.success ? qsTr("Save the installation log") : qsTr("Save the log to a USB stick")
            kind: Kit.Button.Secondary
            onClicked: screen.saveLogRequested()
        }
    }
}

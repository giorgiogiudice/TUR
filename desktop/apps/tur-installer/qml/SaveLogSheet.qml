// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Saves the installer's log to a USB stick, for a report. The engine lists
// the filesystems on USB and removable disks that are not the installation
// medium, mounts the chosen one where nothing else sees it, adds one folder,
// and unmounts it again. Nothing else on the stick is changed.
Kit.Sheet {
    id: sheet

    required property Engine engine

    title: qsTr("Save the log to a USB stick")

    onAboutToShow: sheet.engine.findSticks()

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Plug in a USB stick, then choose it. One folder is added to it; nothing on it is changed or deleted.")
        wrapMode: Text.Wrap
    }

    Kit.BusyIndicator {
        visible: sheet.engine.sticksState === "running" || sheet.engine.saveState === "running"
        label: sheet.engine.saveState === "running" ? qsTr("Saving") : qsTr("Looking for USB sticks")
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: sheet.engine.sticksState === "done" && sheet.engine.sticks.length === 0
        text: qsTr("No USB stick was found. Plug one in and look again.")
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Repeater {
        model: sheet.engine.sticksState === "done" ? sheet.engine.sticks : []

        Kit.ListRow {
            required property var modelData

            Layout.fillWidth: true
            onOverlay: true
            focusPolicy: Qt.StrongFocus
            enabled: sheet.engine.saveState !== "running"
            text: String(modelData.label || modelData.model || modelData.name)
            subtitle: [String(modelData.vendor || "").trim(), String(modelData.model || "").trim(), Words.size(modelData.size || 0),
                       String(modelData.fstype || "")].filter(part => part.length > 0).join(" · ")
            onClicked: sheet.engine.saveLog(String(modelData.device))
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: sheet.engine.saveState === "done"
        text: qsTr("Saved, in the folder %1. You can remove the stick.").arg(String(sheet.engine.saveResult.folder || ""))
        tone: Kit.Label.Positive
        wrapMode: Text.WrapAnywhere
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: sheet.engine.saveState === "failed"
        text: String(sheet.engine.saveResult.message || qsTr("The log could not be saved."))
        tone: Kit.Label.Negative
        wrapMode: Text.Wrap
    }

    buttons: [
        Kit.Button {
            text: qsTr("Close")
            focus: true
            onClicked: sheet.close()
        },
        Kit.Button {
            text: qsTr("Look again")
            enabled: sheet.engine.sticksState !== "running"
            onClicked: sheet.engine.findSticks()
        }
    ]
}

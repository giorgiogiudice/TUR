// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The diagnostics drawer (desktop-experience.md, 11.5; specification 28.4),
// there from the first screen to the last: what is happening now, what is
// done, warnings, the last command or line of output, the detailed log, and
// saving the log to a USB stick. A person never needs it to install; it is
// there for when something goes wrong.
ColumnLayout {
    id: drawer

    required property Engine engine
    required property string phase
    required property int stepIndex
    required property var steps

    // Things a person should know before confirming.
    readonly property var warnings: {
        const out = []
        for (const disk of drawer.engine.disks) {
            if (disk.windows)
                out.push(qsTr("Windows is on %1.").arg(Words.diskName(disk)))
            else if (disk.bitlocker)
                out.push(qsTr("BitLocker-encrypted data is on %1.").arg(Words.diskName(disk)))
        }
        if (drawer.engine.facts.secure_boot === "unknown")
            out.push(qsTr("The Secure Boot state could not be read."))
        return out
    }

    // The room the drawer's buttons need, whatever the text size: a
    // button's label does not wrap.
    readonly property real buttonsWidth: Math.max(logButton.implicitWidth, saveButton.implicitWidth)

    signal logRequested()
    signal saveRequested()

    spacing: Kit.Theme.spaceRelated

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Diagnostics")
        kind: Kit.Label.Heading
    }

    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        text: qsTr("Now")
        kind: Kit.Label.Caps
        tone: Kit.Label.Secondary
    }

    Kit.Label {
        objectName: "drawerPhase"
        Layout.fillWidth: true
        text: drawer.phase
        kind: Kit.Label.Strong
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        visible: drawer.stepIndex > 0
        text: qsTr("Done")
        kind: Kit.Label.Caps
        tone: Kit.Label.Secondary
    }

    Repeater {
        model: drawer.steps.slice(0, Math.min(drawer.stepIndex, drawer.steps.length))

        RowLayout {
            id: doneRow

            required property var modelData

            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Kit.Glyph {
                kind: Kit.Glyph.Check
                tint: Kit.Theme.positive
                size: 12
            }

            Kit.Label {
                Layout.fillWidth: true
                text: doneRow.modelData.title
                kind: Kit.Label.Small
                wrapMode: Text.Wrap
            }
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        text: qsTr("Warnings: %1").arg(drawer.warnings.length)
        kind: Kit.Label.Caps
        tone: Kit.Label.Secondary
    }

    Repeater {
        model: drawer.warnings

        RowLayout {
            id: warningRow

            required property string modelData

            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Kit.Glyph {
                Layout.alignment: Qt.AlignTop
                kind: Kit.Glyph.Warning
                tint: Kit.Theme.warning
                size: 12
            }

            Kit.Label {
                Layout.fillWidth: true
                text: warningRow.modelData
                kind: Kit.Label.Small
                wrapMode: Text.Wrap
            }
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        visible: drawer.engine.lastAction.length > 0
        text: qsTr("Last action")
        kind: Kit.Label.Caps
        tone: Kit.Label.Secondary
    }

    Kit.Label {
        objectName: "drawerLastAction"
        Layout.fillWidth: true
        visible: drawer.engine.lastAction.length > 0
        text: drawer.engine.lastAction
        kind: Kit.Label.Mono
        tone: Kit.Label.Secondary
        wrapMode: Text.WrapAnywhere
    }

    Kit.Button {
        id: logButton

        objectName: "logButton"
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceGroup
        text: qsTr("Detailed log")
        onClicked: drawer.logRequested()
    }

    Kit.Button {
        id: saveButton

        objectName: "drawerSaveButton"
        Layout.fillWidth: true
        text: qsTr("Save the log…")
        onClicked: drawer.saveRequested()
    }
}

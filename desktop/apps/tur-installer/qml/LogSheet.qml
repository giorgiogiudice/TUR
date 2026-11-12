// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// The detailed log, without leaving the installer (desktop-experience.md,
// 11.5): the engine's log on the medium and what the installer received from
// it, read again each time the sheet opens.
Kit.Sheet {
    id: sheet

    required property Engine engine
    property string text

    title: qsTr("Installer log")
    width: Math.min(880, parent ? parent.width - 2 * Kit.Theme.spaceMargin : 880)

    onAboutToShow: sheet.text = sheet.engine.logText()

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Everything the installer and its engine wrote, newest last. It names your disks and their serial numbers, and nothing else about you.")
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 360
        color: Kit.Theme.surfaceSunken
        radius: Kit.Theme.radiusControl

        Flickable {
            id: logView

            anchors.fill: parent
            anchors.margins: Kit.Theme.spaceRelated
            clip: true
            contentWidth: width
            contentHeight: logText.implicitHeight
            boundsBehavior: Flickable.StopAtBounds
            onContentHeightChanged: logView.contentY = Math.max(0, logView.contentHeight - logView.height)

            Controls.ScrollBar.vertical: Controls.ScrollBar {
            }

            Kit.Label {
                id: logText

                width: logView.width
                text: sheet.text
                kind: Kit.Label.Mono
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
            }
        }
    }

    buttons: [
        Kit.Button {
            text: qsTr("Close")
            focus: true
            onClicked: sheet.close()
        }
    ]
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, what will be sent (desktop-experience.md, 9.12, screen 3): the
// consent text, as the service gives it for the chosen provider
// (GetConsentText), word for word, with its data terms in it. "Switch P@x
// on" stays unavailable until the box is ticked; it calls Enable with the
// version of the text that was shown, and the service records that version,
// the provider and the time. Nothing is switched on before this.
SetupScreen {
    id: screen

    required property AgentClient client
    required property string provider
    required property string providerName

    property string version
    property string text
    property bool working: false

    signal done()
    signal backRequested()

    function switchOn(): void {
        screen.clearProblem()
        screen.working = true
        screen.client.call("Enable", [screen.version], function (reply) {
            screen.working = false
            if (reply.ok)
                screen.done()
            else
                screen.refused(reply)
        })
    }

    title: Words.consentTitle
    initialFocus: understood

    Component.onCompleted: {
        screen.client.call("GetConsentText", [screen.provider], function (reply) {
            if (!reply.ok) {
                screen.refused(reply)
                return
            }
            screen.version = String(reply.values[0])
            screen.text = String(reply.values[1])
        })
    }

    Rectangle {
        objectName: "consentBox"
        Layout.fillWidth: true
        implicitHeight: consentText.implicitHeight + 2 * Kit.Theme.spaceGroup
        radius: Kit.Theme.radiusCard
        color: Kit.Theme.surfaceRaised
        border.width: Kit.Theme.lineWidth
        border.color: Kit.Theme.lineSubtle

        Kit.Label {
            id: consentText

            objectName: "consentText"
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: Kit.Theme.spaceGroup
            text: screen.text
            paragraph: true
            wrapMode: Text.Wrap
        }
    }

    Kit.Label {
        objectName: "consentVersion"
        Layout.fillWidth: true
        visible: screen.version.length > 0
        text: qsTr("Version %1 of this text. If it changes in an update, P@x asks again before its next task.").arg(screen.version)
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Kit.CheckBox {
        id: understood

        objectName: "consentCheck"
        Layout.fillWidth: true
        enabled: screen.version.length > 0
        text: Words.consentBox(screen.providerName.length > 0 ? screen.providerName : screen.provider)
    }

    Flow {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "backButton"
            text: Words.back
            onClicked: screen.backRequested()
        }

        Kit.Button {
            objectName: "switchOnButton"
            text: Words.switchOn
            kind: Kit.Button.Primary
            enabled: understood.checked && !screen.working && screen.version.length > 0
            onClicked: screen.switchOn()
        }
    }
}

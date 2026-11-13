// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The ask bar (desktop-experience.md, 9.2): one line for one request, opened
// by P@x's key (Meta+Space, from desktop/kit/pax/commands.json), closed by
// Esc. Enter sends the request; the conversation panel then opens with the
// task. Voice is not in this version, and the bar says so.
//
// When P@x cannot take a task, the bar says why instead (PaxStateNotice).
Kit.Page {
    id: bar

    required property AgentClient client
    required property Conversation conversation

    // The height at which nothing is cut off, for the window and the tests.
    readonly property real naturalHeight: column.implicitHeight + bar.topPadding + bar.bottomPadding
    readonly property bool ready: !stateNotice.blocking
    property alias field: askField

    signal setupRequested()
    signal closeRequested()

    function send(): void {
        const request = askField.text.trim()
        if (request.length === 0 || !bar.ready || bar.conversation.running || bar.conversation.starting)
            return
        bar.conversation.ask(request)
        askField.text = ""
    }

    implicitWidth: 640
    implicitHeight: bar.naturalHeight
    padding: Kit.Theme.spaceGroup

    // The bar floats over whatever is on the screen; its fill sets it
    // apart, and the hairline is decoration (5.4).
    background: Rectangle {
        color: Kit.Theme.surfaceOverlay
        radius: Kit.Theme.radiusSheet
        border.width: Kit.Theme.lineWidth
        border.color: Kit.Theme.lineSubtle
    }

    Keys.onEscapePressed: bar.closeRequested()

    ColumnLayout {
        id: column

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Kit.Theme.spaceRelated

        RowLayout {
            Layout.fillWidth: true
            visible: bar.ready
            spacing: Kit.Theme.spaceRelated

            Pax.PaxMark {
                objectName: "paxMark"
                Layout.alignment: Qt.AlignVCenter
                size: 20
            }

            Kit.TextField {
                id: askField

                objectName: "askField"
                Layout.fillWidth: true
                placeholderText: Words.askPlaceholder
                focus: true
                Accessible.name: Words.askField
                Keys.onReturnPressed: bar.send()
                Keys.onEnterPressed: bar.send()
                Keys.onEscapePressed: bar.closeRequested()
            }

            Kit.Button {
                objectName: "sendButton"
                Layout.alignment: Qt.AlignVCenter
                text: Words.send
                kind: Kit.Button.Primary
                enabled: askField.text.trim().length > 0 && !bar.conversation.running && !bar.conversation.starting
                onClicked: bar.send()
            }
        }

        // Voice is designed (9.1) and not built: the bar says so in words,
        // with the control there and unavailable.
        RowLayout {
            Layout.fillWidth: true
            visible: bar.ready
            spacing: Kit.Theme.spaceRelated

            Kit.Button {
                objectName: "speakButton"
                text: Words.speak
                kind: Kit.Button.Quiet
                enabled: false
            }

            Kit.Label {
                objectName: "voiceLater"
                Layout.fillWidth: true
                text: Words.voiceLater
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }
        }

        KeysLine {
            Layout.fillWidth: true
            visible: bar.ready
        }

        PaxStateNotice {
            id: stateNotice

            Layout.fillWidth: true
            client: bar.client
            onSetupRequested: bar.setupRequested()
            onDismissed: bar.closeRequested()
        }
    }
}

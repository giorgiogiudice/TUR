// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The ask bar (desktop-experience.md, 9.2): one line for one request, opened
// by P@x's key (Meta+Space, from desktop/kit/pax/commands.json), closed by
// Esc. Enter sends the request; the conversation panel then opens with the
// task.
//
// Voice (docs/architecture/pax-voice.md, section 7): the microphone under
// the field (VoiceControl). Held, P@x's key speaks instead of opening the
// bar for typing (keyOpened). What was heard comes into the field; Enter
// sends it when it waits, Esc drops it. "Talk with P@x" starts a live
// conversation, which the panel shows.
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
    property alias voice: voiceControl

    signal setupRequested()
    signal closeRequested()

    function send(): void {
        if (voiceControl.send())
            return
        const request = askField.text.trim()
        if (request.length === 0 || !bar.ready || bar.conversation.running || bar.conversation.starting)
            return
        bar.conversation.ask(request)
        askField.text = ""
    }

    function escape(): void {
        if (voiceControl.cancel())
            return
        bar.closeRequested()
    }

    // The bar was opened by P@x's key, which may still be held.
    function keyOpened(): void {
        voiceControl.keyOpened()
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

    Keys.onEscapePressed: bar.escape()

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
                placeholderText: voiceControl.phase === "listening" ? Words.listening : Words.askPlaceholder
                readOnly: voiceControl.active
                focus: true
                Accessible.name: Words.askField
                Keys.onReturnPressed: bar.send()
                Keys.onEnterPressed: bar.send()
                Keys.onEscapePressed: bar.escape()
                Keys.onPressed: event => voiceControl.keyPressed(event)
                Keys.onReleased: event => voiceControl.keyReleased(event)
            }

            Kit.Button {
                objectName: "sendButton"
                Layout.alignment: Qt.AlignVCenter
                text: Words.send
                kind: Kit.Button.Primary
                enabled: askField.text.trim().length > 0 && !voiceControl.active
                         && !bar.conversation.running && !bar.conversation.starting
                onClicked: bar.send()
            }
        }

        VoiceControl {
            id: voiceControl

            objectName: "voiceControl"
            Layout.fillWidth: true
            visible: bar.ready
            client: bar.client
            conversation: bar.conversation
            field: askField
            allowed: bar.ready && !bar.conversation.running && !bar.conversation.starting
            onSetupRequested: bar.setupRequested()
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

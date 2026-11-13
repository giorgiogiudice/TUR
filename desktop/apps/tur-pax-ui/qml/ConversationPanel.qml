// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The conversation panel (desktop-experience.md, 9.2): 420 px along the
// right edge of the screen. The header names the AI in use and what the task
// and the day have cost; the body is the conversation, each step as it
// happens; the foot has the text field and, while a task runs, Stop and
// Pause with their keys written on them. The panel also shows P@x's record
// of what it did (Activity).
Kit.Page {
    id: panel

    required property AgentClient client
    required property Conversation conversation

    // conversation or activity
    property string page: "conversation"

    readonly property bool ready: !stateNotice.blocking
    readonly property var spend: panel.conversation.spend
    // The height at which nothing scrolls, for the tests and the pictures.
    readonly property real naturalHeight: header.implicitHeight + foot.implicitHeight
                                          + (panel.page === "activity" ? activity.naturalHeight
                                                                       : body.implicitHeight + 2 * Kit.Theme.spaceGroup)
                                          + 2 * Kit.Theme.lineWidth
    property alias field: input

    signal setupRequested()
    signal closeRequested()

    function send(): void {
        const request = input.text.trim()
        if (request.length === 0)
            return
        // While a task runs the field still takes words (9.2). "Stop" stops
        // it; anything else is kept until the task has ended.
        if (panel.conversation.running) {
            if (/^stop[.!]?$/i.test(request)) {
                panel.conversation.stop()
                input.text = ""
            } else {
                busyNote.visible = true
            }
            return
        }
        if (!panel.ready || panel.conversation.starting)
            return
        panel.conversation.ask(request)
        input.text = ""
    }

    function scrollToEnd(): void {
        flick.contentY = Math.max(0, flick.contentHeight - flick.height)
    }

    implicitWidth: 420
    padding: 0

    Connections {
        target: panel.conversation

        function onCountChanged(): void {
            Qt.callLater(panel.scrollToEnd)
        }

        function onTaskChanged(): void {
            if (!panel.conversation.running)
                busyNote.visible = false
        }
    }

    // The stop key works inside the window too; the global binding is in
    // the desktop file (data/org.tur.PaxUi.desktop).
    Shortcut {
        sequence: Pax.PaxCommands.stopKeys
        enabled: panel.conversation.running
        onActivated: panel.conversation.stop()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // --- header ------------------------------------------------------------
        Rectangle {
            id: header

            objectName: "panelHeader"
            Layout.fillWidth: true
            implicitHeight: headerColumn.implicitHeight + 2 * Kit.Theme.spaceGroup
            color: Kit.Theme.surfaceRaised

            ColumnLayout {
                id: headerColumn

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Kit.Theme.spaceGroup
                spacing: Kit.Theme.spaceGrid

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceRelated

                    Pax.PaxName {
                        objectName: "paxName"
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Item {
                        Layout.fillWidth: true
                    }
                }

                Kit.Label {
                    objectName: "providerLine"
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: {
                        const status = panel.client.status
                        if (!status.provider_name)
                            return ""
                        return status.model ? qsTr("%1 · %2").arg(status.provider_name).arg(status.model) : String(status.provider_name)
                    }
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "spendLine"
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: {
                        const parts = [Words.taskSpend(panel.spend), Words.daySpend(panel.spend)].filter(part => part.length > 0)
                        return parts.join(" · ")
                    }
                    kind: Kit.Label.Small
                    tone: panel.spend.dayLimitReached ? Kit.Label.Warning : Kit.Label.Secondary
                    tabular: true
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    objectName: "noPriceLine"
                    Layout.fillWidth: true
                    visible: panel.ready && panel.spend.known === true && panel.spend.priceKnown !== true
                    text: Words.noPrice
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: Kit.Theme.spaceGrid
                    spacing: Kit.Theme.spaceRelated

                    Kit.Button {
                        objectName: "activityButton"
                        visible: panel.client.presence === AgentClient.Present
                        text: panel.page === "activity" ? Words.conversation : Words.activity
                        kind: Kit.Button.Quiet
                        onClicked: panel.page = panel.page === "activity" ? "conversation" : "activity"
                    }

                    Kit.Button {
                        objectName: "panelSetupButton"
                        visible: panel.client.presence === AgentClient.Present && panel.ready
                        text: Words.setup
                        kind: Kit.Button.Quiet
                        onClicked: panel.setupRequested()
                    }

                    Kit.Button {
                        objectName: "closeButton"
                        text: Words.close
                        kind: Kit.Button.Quiet
                        onClicked: panel.closeRequested()
                    }
                }
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
            Layout.preferredHeight: Kit.Theme.lineWidth
        }

        // --- the conversation ------------------------------------------------------
        Flickable {
            id: flick

            objectName: "conversationView"
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: panel.page === "conversation"
            clip: true
            contentWidth: width
            contentHeight: body.implicitHeight + 2 * Kit.Theme.spaceGroup
            boundsBehavior: Flickable.StopAtBounds

            Controls.ScrollBar.vertical: Controls.ScrollBar {
            }

            ColumnLayout {
                id: body

                x: Kit.Theme.spaceGroup
                y: Kit.Theme.spaceGroup
                width: flick.width - 2 * Kit.Theme.spaceGroup
                spacing: Kit.Theme.spaceGroup

                PaxStateNotice {
                    id: stateNotice

                    Layout.fillWidth: true
                    client: panel.client
                    onSetupRequested: panel.setupRequested()
                    onDismissed: panel.closeRequested()
                }

                Kit.Label {
                    objectName: "emptyConversation"
                    Layout.fillWidth: true
                    visible: panel.ready && panel.conversation.count === 0
                    text: Words.emptyConversation
                    tone: Kit.Label.Secondary
                    paragraph: true
                    wrapMode: Text.Wrap
                }

                Repeater {
                    objectName: "entries"
                    model: panel.conversation

                    delegate: ConversationEntry {
                        Layout.fillWidth: true
                        onSetupRequested: panel.setupRequested()
                    }
                }

                SystemCard {
                    id: busyNote

                    objectName: "busyNote"
                    Layout.fillWidth: true
                    visible: false
                    phase: "notice"
                    text: Words.busyNote
                }
            }
        }

        ActivityView {
            id: activity

            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: panel.page === "activity"
            client: panel.client
            active: panel.page === "activity"
        }

        Kit.Separator {
            Layout.fillWidth: true
            Layout.preferredHeight: Kit.Theme.lineWidth
        }

        // --- the foot ------------------------------------------------------------------
        Rectangle {
            id: foot

            objectName: "panelFoot"
            Layout.fillWidth: true
            implicitHeight: footColumn.implicitHeight + 2 * Kit.Theme.spaceGroup
            color: Kit.Theme.surfaceRaised

            ColumnLayout {
                id: footColumn

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Kit.Theme.spaceGroup
                spacing: Kit.Theme.spaceRelated

                // Stop and Pause, always in the same place while a task runs,
                // with the key on the button (9.2).
                Flow {
                    objectName: "stopBar"
                    Layout.fillWidth: true
                    visible: panel.conversation.running
                    spacing: Kit.Theme.spaceRelated

                    Kit.Button {
                        id: stopButton

                        objectName: "stopButton"
                        //: The Stop button. %1 is the stop key, such as Meta+Esc; it is not translated.
                        text: qsTr("%1  %2").arg(Words.stop).arg(Pax.PaxCommands.stopKeys)
                        kind: Kit.Button.Primary
                        Accessible.name: qsTr("Stop P@x (%1)").arg(Pax.PaxCommands.stopKeys)
                        onClicked: panel.conversation.stop()
                    }

                    Kit.Button {
                        objectName: "pauseButton"
                        text: panel.conversation.paused ? Words.resume : Words.pause
                        onClicked: {
                            if (panel.conversation.paused)
                                panel.conversation.resume()
                            else
                                panel.conversation.pause()
                        }
                    }
                }

                Kit.TextField {
                    id: input

                    objectName: "panelField"
                    Layout.fillWidth: true
                    visible: panel.ready
                    placeholderText: Words.askPlaceholder
                    Accessible.name: Words.askField
                    Keys.onReturnPressed: panel.send()
                    Keys.onEnterPressed: panel.send()
                }

                Flow {
                    Layout.fillWidth: true
                    visible: panel.ready
                    spacing: Kit.Theme.spaceRelated

                    Kit.Button {
                        objectName: "panelSendButton"
                        text: Words.send
                        enabled: input.text.trim().length > 0 && !panel.conversation.starting
                        onClicked: panel.send()
                    }

                    Kit.Button {
                        objectName: "panelSpeakButton"
                        text: Words.speak
                        kind: Kit.Button.Quiet
                        enabled: false
                    }
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: panel.ready
                    text: Words.voiceLater
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                KeysLine {
                    Layout.fillWidth: true
                    visible: panel.client.presence === AgentClient.Present && panel.client.status.enabled === true
                }
            }
        }
    }
}

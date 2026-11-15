// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The whole of a notice of TUR's, which "Show me" opens from the bubble
// (tur-pax-companion.md, section 4): what happened, its code, the service's
// own explanation, and the commands the service offers, as text a person
// can select and copy. The character runs none of them.
Leaf {
    id: card

    property var notice: ({})

    readonly property string title: card.notice.title !== undefined && String(card.notice.title).length > 0
                                    ? String(card.notice.title) : String(card.notice.sentence || "")
    readonly property string detail: card.notice.detail !== undefined ? String(card.notice.detail) : ""
    readonly property string code: card.notice.code !== undefined ? String(card.notice.code) : ""
    readonly property var commands: card.notice.commands !== undefined ? Array.from(card.notice.commands) : []

    // P@x is on: the card offers to ask P@x about the notice.
    property bool paxOn: false

    signal closeRequested()
    signal explainRequested(string request)

    Accessible.role: Accessible.Grouping
    Accessible.name: card.title

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            objectName: "senderName"
            text: Words.tur
            kind: Kit.Label.Strong
            wrapMode: Text.NoWrap
        }

        Kit.Label {
            objectName: "noticeTitle"
            Layout.fillWidth: true
            text: card.title
            kind: Kit.Label.Heading
        }

        Kit.CodeLabel {
            objectName: "code"
            visible: card.code.length > 0
            code: card.code
        }

        Kit.Label {
            objectName: "noticeDetail"
            Layout.fillWidth: true
            visible: card.detail.length > 0 && card.detail !== card.title
            text: card.detail
            paragraph: true
        }

        Kit.Label {
            objectName: "commandsTitle"
            Layout.fillWidth: true
            Layout.topMargin: Kit.Theme.spaceRelated
            visible: card.commands.length > 0
            text: Words.commandsTitle
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Repeater {
            model: card.commands

            delegate: Rectangle {
                id: line

                required property string modelData

                Layout.fillWidth: true
                implicitHeight: command.implicitHeight + 2 * Kit.Theme.spaceRelated
                radius: Kit.Theme.radiusControl
                color: Kit.Theme.surfaceSunken

                TextInput {
                    id: command

                    objectName: "command"
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: Kit.Theme.spaceRelated
                    text: line.modelData
                    readOnly: true
                    selectByMouse: true
                    activeFocusOnTab: true
                    wrapMode: TextInput.WrapAnywhere
                    font.family: Kit.Theme.monoFamily
                    font.pointSize: Kit.Theme.typeMonoSize * Kit.Theme.fontScale
                    color: Kit.Theme.textPrimary
                    selectionColor: Kit.Theme.selection
                    selectedTextColor: Kit.Theme.selectionOn
                    onActiveFocusChanged: {
                        if (command.activeFocus)
                            command.selectAll()
                        else
                            command.deselect()
                    }

                    Accessible.role: Accessible.StaticText
                    Accessible.name: line.modelData
                    Accessible.readOnly: true

                    Kit.FocusRing {
                        shown: command.activeFocus
                    }
                }
            }
        }

        Kit.Label {
            objectName: "commandsNote"
            Layout.fillWidth: true
            visible: card.commands.length > 0
            text: Words.commandsNote
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Flow {
            Layout.fillWidth: true
            Layout.topMargin: Kit.Theme.spaceRelated
            spacing: Kit.Theme.spaceRelated

            // Only while P@x is on, and only when pressed: the code and the
            // title go to P@x as a request, under its rules.
            Kit.Button {
                objectName: "explainButton"
                visible: card.paxOn && card.code.length > 0
                text: Words.explain
                onClicked: card.explainRequested(Words.explainRequest.arg(card.code).arg(card.title))
            }

            Kit.Button {
                objectName: "closeButton"
                text: Words.close
                kind: Kit.Button.Quiet
                onClicked: card.closeRequested()
            }
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The speech bubble (docs/architecture/tur-pax-companion.md, section 4):
// who says it, one sentence, a code if there is one, and at most two
// actions. Who says it is always shown, because the system and P@x are not
// the same speaker (desktop-experience.md, 9.2): a repair, a refusal or a
// limit is TUR speaking, in a neutral name; only P@x's own words carry
// P@x's name in gold.
Leaf {
    id: bubble

    // The notice, as Companion.bubble gives it.
    property var notice: ({})

    readonly property string sender: bubble.notice.sender !== undefined ? String(bubble.notice.sender) : "tur"
    readonly property string sentence: bubble.notice.sentence !== undefined ? String(bubble.notice.sentence) : ""
    readonly property string code: bubble.notice.code !== undefined ? String(bubble.notice.code) : ""
    readonly property bool suggestion: bubble.notice.kind === "suggestion"
    // Never more than two (Companion keeps to that too).
    readonly property var actions: bubble.notice.actions !== undefined ? Array.from(bubble.notice.actions).slice(0, 2) : []

    signal actionTriggered(string action)

    Accessible.role: Accessible.AlertMessage
    Accessible.name: Words.says.arg(bubble.sender === "pax" ? Words.pax : Words.tur)
    Accessible.description: bubble.sentence

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceRelated

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Pax.PaxName {
                objectName: "paxName"
                visible: bubble.sender === "pax"
            }

            Kit.Label {
                objectName: "senderName"
                visible: bubble.sender !== "pax"
                text: Words.tur
                kind: Kit.Label.Strong
                wrapMode: Text.NoWrap
            }

            Kit.Label {
                objectName: "suggestionTag"
                Layout.fillWidth: true
                visible: bubble.suggestion
                text: Words.suggestionTag
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
            }

            Item {
                Layout.fillWidth: true
                visible: !bubble.suggestion
            }
        }

        Kit.Label {
            objectName: "sentence"
            Layout.fillWidth: true
            text: bubble.sentence
            paragraph: true
        }

        Kit.CodeLabel {
            objectName: "code"
            visible: bubble.code.length > 0
            code: bubble.code
        }

        Flow {
            objectName: "actions"
            Layout.fillWidth: true
            visible: bubble.actions.length > 0
            spacing: Kit.Theme.spaceRelated

            Repeater {
                model: bubble.actions

                delegate: Kit.Button {
                    id: button

                    required property string modelData
                    required property int index

                    objectName: "action-" + button.modelData
                    text: Words.action(button.modelData)
                    kind: button.modelData === "dismiss" ? Kit.Button.Quiet
                          : button.index === 0 ? Kit.Button.Primary : Kit.Button.Secondary
                    onClicked: bubble.actionTriggered(button.modelData)
                }
            }
        }
    }
}

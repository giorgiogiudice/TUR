// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What the system says about P@x, never P@x's words (desktop-experience.md,
// 9.2): in the third person, in a bordered card that carries TUR's name, so
// that a person can tell what the model said from what the system decided.
// A refusal before a task started, a notice of the service (a limit that was
// reached), a task that was stopped or failed, a task a schedule started.
Rectangle {
    id: card

    // refused, notice, cancelled, spend_limit, scheduled, failed, ...
    property string phase
    // The service's own sentence, as it said it.
    property string text
    // The TUR-AI-* code, if there is one.
    property string code
    // The label of the one thing to do about it, or empty.
    property string actionText

    signal actionTriggered()

    readonly property color tone: {
        switch (card.phase) {
        case "refused":
        case "setting":
        case "failed":
            return Kit.Theme.negative
        case "notice":
        case "spend_limit":
        case "step_limit":
        case "time_limit":
            return Kit.Theme.warning
        case "scheduled":
            return Kit.Theme.accentText
        default:
            return Kit.Theme.lineControl
        }
    }
    // The sign beside the title: the card's tone, or the secondary text
    // colour where the line is the neutral one.
    readonly property color signTone: {
        switch (card.phase) {
        case "refused":
        case "setting":
        case "failed":
        case "notice":
        case "spend_limit":
        case "step_limit":
        case "time_limit":
        case "scheduled":
            return card.tone
        default:
            return Kit.Theme.textSecondary
        }
    }
    readonly property string title: Words.cardTitle(card.phase)
    readonly property string note: Words.cardNote(card.phase)

    implicitWidth: column.implicitWidth + 2 * Kit.Theme.spaceGroup
    implicitHeight: column.implicitHeight + 2 * Kit.Theme.spaceGroup
    radius: Kit.Theme.radiusCard
    color: Kit.Theme.surfaceRaised
    border.width: Kit.Theme.lineWidth
    border.color: card.tone

    Accessible.role: Accessible.AlertMessage
    Accessible.name: card.title
    Accessible.description: card.text

    ColumnLayout {
        id: column

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Kit.Theme.spaceGroup
        spacing: Kit.Theme.spaceRelated

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Kit.Glyph {
                Layout.alignment: Qt.AlignTop
                Layout.topMargin: 1
                tint: card.signTone
                size: 16
                kind: {
                    switch (card.phase) {
                    case "refused":
                    case "setting":
                    case "failed":
                        return Kit.Glyph.Negative
                    case "notice":
                    case "spend_limit":
                    case "step_limit":
                    case "time_limit":
                        return Kit.Glyph.Warning
                    default:
                        return Kit.Glyph.Info
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                // The system's name: this card is TUR speaking, not P@x.
                Kit.Label {
                    Layout.fillWidth: true
                    text: "TUR"
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                    Accessible.ignored: true
                }

                Kit.Label {
                    objectName: "cardTitle"
                    Layout.fillWidth: true
                    text: card.title
                    kind: Kit.Label.Strong
                    wrapMode: Text.Wrap
                }
            }
        }

        Kit.Label {
            objectName: "cardText"
            Layout.fillWidth: true
            visible: card.text.length > 0
            text: card.text
            paragraph: true
            wrapMode: Text.Wrap
        }

        Kit.Label {
            objectName: "cardNote"
            Layout.fillWidth: true
            visible: card.note.length > 0
            text: card.note
            tone: Kit.Label.Secondary
            paragraph: true
            wrapMode: Text.Wrap
        }

        Kit.CodeLabel {
            objectName: "cardCode"
            visible: card.code.length > 0
            code: card.code
        }

        Kit.Button {
            objectName: "cardAction"
            visible: card.actionText.length > 0
            text: card.actionText
            onClicked: card.actionTriggered()
        }
    }
}

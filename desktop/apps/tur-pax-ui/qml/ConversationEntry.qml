// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// One entry of the conversation (Conversation's rows; desktop-experience.md,
// 9.2). The person's words, P@x's words, a step, the system's card and the
// end of a task each look different, so that nobody mistakes one for
// another: the person's words sit at the far side in a bubble; P@x's words
// carry its name in its colour and nothing else does; the system's cards are
// bordered and carry TUR's name.
ColumnLayout {
    id: entry

    // The row's roles (Conversation::roleNames).
    required property string kind
    required property string text
    required property string task
    required property var step
    required property string phase
    required property string code
    required property real time
    required property var detail

    // A card offers setup when the refusal is one that setup cures.
    readonly property bool setupCures: entry.kind === "system" && entry.phase === "refused"
                                       && ["TUR-AI-SERVICE-001", "TUR-AI-SERVICE-002", "TUR-AI-SERVICE-003",
                                           "TUR-AI-KEY-002"].indexOf(entry.code) >= 0

    signal setupRequested()

    objectName: "entry-" + entry.kind
    spacing: 0

    // --- the person's words ---------------------------------------------------
    RowLayout {
        Layout.fillWidth: true
        visible: entry.kind === "you"
        spacing: 0

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: Kit.Theme.spaceMargin * 2
        }

        Rectangle {
            objectName: "youWords"
            Layout.maximumWidth: entry.width - Kit.Theme.spaceMargin * 2
            Layout.preferredWidth: Math.max(youCaption.implicitWidth, youText.implicitWidth) + 2 * Kit.Theme.spaceGroup
            implicitHeight: youColumn.implicitHeight + 2 * Kit.Theme.spaceRelated
            radius: Kit.Theme.radiusCard
            color: Kit.Theme.surfaceRaised
            border.width: Kit.Theme.lineWidth
            border.color: Kit.Theme.lineSubtle

            ColumnLayout {
                id: youColumn

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Kit.Theme.spaceGroup
                anchors.rightMargin: Kit.Theme.spaceGroup
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    id: youCaption

                    Layout.fillWidth: true
                    text: {
                        if (entry.phase === "elsewhere")
                            return qsTr("You, from another program")
                        const at = Words.time(entry.time)
                        return at.length > 0 ? qsTr("%1, %2").arg(Words.you).arg(at) : Words.you
                    }
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    horizontalAlignment: Text.AlignRight
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    id: youText

                    Layout.fillWidth: true
                    text: entry.text
                    paragraph: true
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    // --- P@x's words ------------------------------------------------------------
    ColumnLayout {
        objectName: "paxWords"
        Layout.fillWidth: true
        visible: entry.kind === "pax"
        spacing: Kit.Theme.spaceGrid

        Pax.PaxName {
            objectName: "paxName"
        }

        Kit.Label {
            objectName: "paxText"
            Layout.fillWidth: true
            text: entry.text
            tone: entry.phase === "interim" ? Kit.Label.Secondary : Kit.Label.Primary
            paragraph: true
            wrapMode: Text.Wrap
        }
    }

    // --- a step -------------------------------------------------------------------
    StepLine {
        Layout.fillWidth: true
        visible: entry.kind === "step"
        step: entry.kind === "step" ? entry.step : ({})
        phase: entry.phase
    }

    // --- the system speaking ---------------------------------------------------------
    SystemCard {
        Layout.fillWidth: true
        visible: entry.kind === "system"
        phase: entry.phase
        text: entry.phase === "scheduled" ? qsTr("Asked by a schedule: %1").arg(entry.text) : entry.text
        code: entry.code
        actionText: entry.setupCures ? Words.setup : ""
        onActionTriggered: entry.setupRequested()
    }

    // --- a task that completed ----------------------------------------------------------
    RowLayout {
        objectName: "summary"
        Layout.fillWidth: true
        visible: entry.kind === "summary"
        spacing: Kit.Theme.spaceRelated

        Kit.Glyph {
            Layout.alignment: Qt.AlignTop
            Layout.topMargin: 1
            kind: Kit.Glyph.Positive
            tint: Kit.Theme.positive
            size: 16
        }

        Kit.Label {
            Layout.fillWidth: true
            text: {
                const detail = entry.detail || {}
                let out = Words.summary(detail)
                if (detail.cost !== null && detail.cost !== undefined)
                    out += " " + qsTr("It cost %1.").arg(Words.money(detail.cost, String(detail.currency || "")))
                else if (Number(detail.tokens || 0) > 0)
                    out += " " + qsTr("It used %1.").arg(Words.tokens(detail.tokens))
                return out
            }
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }
    }
}

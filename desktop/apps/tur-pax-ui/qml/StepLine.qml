// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One action of P@x's, as it happens (desktop-experience.md, 9.2): one line
// in plain words with its state, the policy's decision in plain words, and,
// when the action returned something, that it went to P@x as data and not
// as instructions (ai-control-layer.md, section 7 and 21.5). "Details" opens
// the technical layer: the tool, the exact action (a command in full), the
// action class, the tier and the audit record's identifier.
//
// The service does not send what a tool returned, only how much (the step
// document of pax-service.md, 2.4); so the size is what is shown.
ColumnLayout {
    id: line

    // The service's step document.
    property var step: ({})
    // running, done, failed, refused, rejected, needs_you, stopped.
    property string phase

    readonly property string returned: Words.returned(line.step)
    readonly property bool refused: line.phase === "refused" || line.phase === "rejected"
                                    || line.phase === "failed"

    spacing: Kit.Theme.spaceGrid

    RowLayout {
        Layout.fillWidth: true
        spacing: Kit.Theme.spaceRelated

        Item {
            Layout.alignment: Qt.AlignTop
            Layout.topMargin: 1
            Layout.preferredWidth: 16
            Layout.preferredHeight: 16

            Kit.Glyph {
                anchors.centerIn: parent
                size: 14
                kind: {
                    switch (line.phase) {
                    case "done":
                        return Kit.Glyph.Check
                    case "refused":
                    case "rejected":
                    case "failed":
                        return Kit.Glyph.Negative
                    case "stopped":
                    case "needs_you":
                        return Kit.Glyph.Warning
                    default:
                        return Kit.Glyph.ChevronForward
                    }
                }
                tint: {
                    switch (line.phase) {
                    case "done":
                        return Kit.Theme.positive
                    case "refused":
                    case "rejected":
                    case "failed":
                        return Kit.Theme.negative
                    case "stopped":
                    case "needs_you":
                        return Kit.Theme.warning
                    default:
                        return Kit.Theme.accentText
                    }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGrid

            Kit.Label {
                objectName: "stepTitle"
                Layout.fillWidth: true
                text: String(line.step.title || line.step.tool || "")
                kind: line.phase === "running" ? Kit.Label.Strong : Kit.Label.Body
                wrapMode: Text.Wrap
            }

            Kit.Label {
                objectName: "stepDecision"
                Layout.fillWidth: true
                text: Words.stepState(line.phase) + " · " + Words.decision(line.step)
                kind: Kit.Label.Small
                tone: line.refused ? Kit.Label.Negative : Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            // What the action returned is data for P@x, never instructions
            // to it: marked so, on the line itself.
            Rectangle {
                objectName: "stepData"
                Layout.fillWidth: true
                visible: line.returned.length > 0
                implicitHeight: dataRow.implicitHeight + 2 * Kit.Theme.spaceRelated
                radius: Kit.Theme.radiusControl
                color: Kit.Theme.surfaceSunken
                border.width: Kit.Theme.lineWidth
                border.color: Kit.Theme.lineControl

                Accessible.role: Accessible.StaticText
                Accessible.name: Words.dataTag + ": " + line.returned + " " + Words.dataNotInstructions

                RowLayout {
                    id: dataRow

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: Kit.Theme.spaceRelated
                    spacing: Kit.Theme.spaceRelated

                    Rectangle {
                        Layout.alignment: Qt.AlignTop
                        implicitWidth: tag.implicitWidth + 2 * Kit.Theme.spaceGrid
                        implicitHeight: tag.implicitHeight + Kit.Theme.spaceGrid
                        radius: Kit.Theme.radiusControl / 2
                        color: Kit.Theme.surfaceSunken
                        border.width: Kit.Theme.lineWidth
                        border.color: Kit.Theme.lineControl

                        Kit.Label {
                            id: tag

                            anchors.centerIn: parent
                            text: Words.dataTag
                            kind: Kit.Label.Caps
                            tone: Kit.Label.Secondary
                            Accessible.ignored: true
                        }
                    }

                    Kit.Label {
                        Layout.fillWidth: true
                        text: line.returned + " " + Words.dataNotInstructions
                            + (Number(line.step.withheld || 0) > 0 ? " " + qsTr("Part of it was withheld before it left this computer.") : "")
                            + (line.step.truncated === true ? " " + qsTr("It was cut short.") : "")
                        kind: Kit.Label.Small
                        tone: Kit.Label.Secondary
                        wrapMode: Text.Wrap
                        Accessible.ignored: true
                    }
                }
            }

            Kit.Disclosure {
                id: disclosure

                objectName: "stepDetails"
                text: Words.details
            }

            GridLayout {
                objectName: "stepTechnical"
                Layout.fillWidth: true
                visible: disclosure.expanded
                columns: 1
                rowSpacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Tool: %1").arg(String(line.step.service || "") + " · " + String(line.step.tool || ""))
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.WrapAnywhere
                }

                // The exact action; a command is always shown in full.
                Kit.Label {
                    objectName: "stepExact"
                    Layout.fillWidth: true
                    text: String(line.step.detail || "")
                    kind: Kit.Label.Mono
                    wrapMode: Text.WrapAnywhere
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.actionClass(line.step["class"]) + " · " + Words.tier(line.step.tier)
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: line.step.result !== null && line.step.result !== undefined
                    text: qsTr("Result: %1").arg(String(line.step.result || ""))
                          + (line.step.exit_code !== null && line.step.exit_code !== undefined
                             ? " · " + qsTr("exit code %1").arg(line.step.exit_code) : "")
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.CodeLabel {
                    visible: String(line.step.error_code || "").length > 0
                    code: String(line.step.error_code || "")
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Record: %1").arg(String(line.step.audit_id || ""))
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.WrapAnywhere
                }
            }
        }
    }
}

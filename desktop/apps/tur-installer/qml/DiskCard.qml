// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import Tur.Kit as Kit

// One disk on the disk screen: what it is, a picture of its partitions, and
// what is on it in plain words. A disk that cannot be chosen (the medium
// itself, one that is too small, in use or without a serial number) is shown
// with the reason, and the keyboard passes it by. No device path names a
// disk here (desktop-experience.md, 11.5, screen 3).
T.ItemDelegate {
    id: card

    required property var disk
    property bool selected: false
    readonly property bool choosable: Words.choosable(card.disk)

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    padding: Kit.Theme.spaceGroup
    enabled: card.choosable
    focusPolicy: Qt.StrongFocus
    highlighted: card.selected

    Accessible.role: Accessible.RadioButton
    Accessible.name: Words.diskLine(card.disk) + ". " + Words.contents(card.disk)
    Accessible.description: Words.serialLine(card.disk)
    Accessible.checkable: true
    Accessible.checked: card.selected

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceRelated

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            // The choice, as a sign and not only a colour: a filled ring
            // when chosen.
            Rectangle {
                Layout.alignment: Qt.AlignTop
                Layout.topMargin: 2
                Layout.preferredWidth: 18
                Layout.preferredHeight: 18
                radius: 9
                color: "transparent"
                border.width: Kit.Theme.lineFocusWidth
                border.color: card.choosable ? Kit.Theme.lineControl : Kit.Theme.lineSubtle

                Rectangle {
                    anchors.centerIn: parent
                    width: 8
                    height: 8
                    radius: 4
                    visible: card.selected
                    color: Kit.Theme.accent
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.diskLine(card.disk)
                    kind: Kit.Label.Strong
                    tone: card.choosable ? Kit.Label.Primary : Kit.Label.Secondary
                    wrapMode: Text.Wrap
                    Accessible.ignored: true
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.serialLine(card.disk)
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                    Accessible.ignored: true
                }
            }
        }

        PartitionBar {
            Layout.fillWidth: true
            diskSize: card.disk.size || 1
            partitions: card.disk.partitions || []
        }

        Kit.Label {
            Layout.fillWidth: true
            text: Words.contents(card.disk)
            tone: card.disk.windows || card.disk.bitlocker ? Kit.Label.Warning : Kit.Label.Primary
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }

        Repeater {
            model: card.disk.partitions || []

            Kit.Label {
                required property var modelData

                Layout.fillWidth: true
                text: Words.partitionLine(modelData)
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
                Accessible.ignored: true
            }
        }

        Kit.Label {
            Layout.fillWidth: true
            visible: !card.choosable && card.disk.status !== "installer-medium"
            text: qsTr("Cannot be chosen: %1").arg(Words.refusal(card.disk))
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }
    }

    background: Rectangle {
        radius: Kit.Theme.radiusCard
        color: Kit.Theme.surfaceRaised
        border.width: card.selected ? Kit.Theme.lineFocusWidth : Kit.Theme.lineWidth
        border.color: card.selected ? Kit.Theme.accent : Kit.Theme.lineSubtle

        Kit.StateLayer {
            hovered: card.hovered && card.choosable
            pressed: card.down
            cornerRadius: Kit.Theme.radiusCard
        }

        Kit.FocusRing {
            shown: card.visualFocus
            cornerRadius: Kit.Theme.radiusCard
        }
    }
}

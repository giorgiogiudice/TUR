// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import Tur.Kit as Kit

// One choice among several, as a card: where P@x's AI runs, a provider, a
// model. A radio button to a screen reader, with its title as its name;
// whatever is declared inside goes under the title (a provider's data
// terms, a model's size and licence). Space or a click chooses it.
T.AbstractButton {
    id: card

    property string title
    property string subtitle
    property bool selected: false
    // A choice that cannot be made, with the reason in `subtitle`.
    property bool unavailable: false
    default property alias content: extra.data

    focusPolicy: Qt.StrongFocus
    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    padding: Kit.Theme.spaceGroup

    Accessible.role: Accessible.RadioButton
    Accessible.name: card.title
    Accessible.description: card.subtitle
    Accessible.checkable: true
    Accessible.checked: card.selected
    Accessible.onPressAction: card.clicked()

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceGrid

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            // The radio mark: an outline, filled when chosen. Not colour
            // alone: the filled dot is the difference.
            Rectangle {
                Layout.alignment: Qt.AlignTop
                Layout.topMargin: 1
                implicitWidth: 18
                implicitHeight: 18
                radius: 9
                color: Kit.Theme.surfaceRaised
                border.width: card.selected ? 2 : Kit.Theme.lineWidth
                border.color: card.unavailable ? Kit.Theme.lineSubtle : card.selected ? Kit.Theme.accent : Kit.Theme.lineControl

                Rectangle {
                    anchors.centerIn: parent
                    visible: card.selected
                    width: 8
                    height: 8
                    radius: 4
                    color: Kit.Theme.accent
                }
            }

            Kit.Label {
                Layout.fillWidth: true
                text: card.title
                kind: Kit.Label.Strong
                color: card.unavailable ? Kit.Theme.textSecondary : Kit.Theme.textPrimary
                wrapMode: Text.Wrap
                Accessible.ignored: true
            }
        }

        Kit.Label {
            Layout.fillWidth: true
            visible: card.subtitle.length > 0
            text: card.subtitle
            tone: Kit.Label.Secondary
            paragraph: true
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }

        ColumnLayout {
            id: extra

            Layout.fillWidth: true
            visible: children.length > 0
            spacing: Kit.Theme.spaceGrid
        }
    }

    background: Rectangle {
        radius: Kit.Theme.radiusCard
        color: Kit.Theme.surfaceRaised
        border.width: card.selected ? 2 : Kit.Theme.lineWidth
        border.color: card.selected ? Kit.Theme.accent : Kit.Theme.lineControl

        Kit.StateLayer {
            hovered: card.hovered && !card.unavailable
            pressed: card.down
            cornerRadius: Kit.Theme.radiusCard
        }

        Kit.FocusRing {
            shown: card.visualFocus
            cornerRadius: Kit.Theme.radiusCard
        }
    }
}

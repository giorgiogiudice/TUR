// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T

// One row of a list: a title, an optional second line, and a selected state.
// Put rows in a Card, with a Separator between them where that helps.
T.ItemDelegate {
    id: control

    // A supporting second line.
    property string subtitle

    // The current row of its list.
    property bool selected: false

    // Set on a row inside a menu or a sheet (surface.overlay). The selection
    // colour is too close to that surface in the dark variant (2.8:1), so
    // such a row is marked with the accent instead (desktop-experience.md,
    // 5.2).
    property bool onOverlay: false

    readonly property color titleColor: !control.enabled ? Theme.textDisabled
                                        : !control.selected ? Theme.textPrimary
                                        : control.onOverlay ? Theme.accentOn : Theme.selectionOn
    readonly property color subtitleColor: control.enabled && !control.selected
                                           ? Theme.textSecondary : control.titleColor

    highlighted: control.selected

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(control.subtitle.length > 0 ? Theme.targetTouch : Theme.targetPointer,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Theme.spaceGroup
    verticalPadding: Theme.spaceGrid + Theme.spaceGrid / 2

    Accessible.role: Accessible.ListItem
    Accessible.name: control.text
    Accessible.description: control.subtitle
    Accessible.selectable: true
    Accessible.selected: control.selected

    contentItem: Item {
        implicitWidth: lines.implicitWidth
        implicitHeight: lines.implicitHeight

        ColumnLayout {
            id: lines

            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.spaceGrid / 2

            Label {
                Layout.fillWidth: true
                text: control.text
                color: control.titleColor
                Accessible.ignored: true
            }

            Label {
                Layout.fillWidth: true
                visible: control.subtitle.length > 0
                text: control.subtitle
                kind: Label.Small
                color: control.subtitleColor
                Accessible.ignored: true
            }
        }
    }

    background: Rectangle {
        implicitWidth: 200
        implicitHeight: Theme.targetPointer
        radius: Theme.radiusControl
        color: !control.selected ? "transparent"
               : control.onOverlay ? Theme.accent : Theme.selection

        StateLayer {
            hovered: control.hovered
            pressed: control.down
            tint: control.titleColor
        }

        FocusRing {
            shown: control.visualFocus
            inside: true
            tint: control.selected ? control.titleColor : Theme.accentText
        }
    }
}

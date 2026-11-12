// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// A check box with its label. Checked is a filled box with a mark, so the
// state does not rest on colour.
T.CheckBox {
    id: control

    readonly property real boxSize: 20

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Theme.targetPointer,
                             implicitContentHeight + topPadding + bottomPadding,
                             implicitIndicatorHeight + topPadding + bottomPadding)
    verticalPadding: Theme.spaceRelated
    spacing: Theme.spaceRelated

    Accessible.role: Accessible.CheckBox
    Accessible.name: control.text
    Accessible.checkable: true
    Accessible.checked: control.checked

    indicator: Rectangle {
        id: box

        implicitWidth: control.boxSize
        implicitHeight: control.boxSize
        // The box is at the start of the line: the left, or the right in a
        // right-to-left layout.
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: Theme.spaceGrid
        color: control.checked && control.enabled ? Theme.accent : Theme.surfaceRaised
        border.width: control.checked && control.enabled ? 0 : Theme.lineWidth
        border.color: control.enabled ? Theme.lineControl : Theme.lineSubtle

        Glyph {
            anchors.centerIn: parent
            visible: control.checked
            kind: Glyph.Check
            tint: control.enabled ? Theme.accentOn : Theme.textDisabled
            size: 14
        }

        StateLayer {
            cornerRadius: box.radius
            hovered: control.hovered
            pressed: control.down
            tint: control.checked ? Theme.accentOn : Theme.textPrimary
        }

        FocusRing {
            shown: control.visualFocus
            cornerRadius: box.radius
        }
    }

    contentItem: Label {
        leftPadding: control.mirrored ? 0 : control.boxSize + control.spacing
        rightPadding: control.mirrored ? control.boxSize + control.spacing : 0
        text: control.text
        verticalAlignment: Text.AlignVCenter
        Accessible.ignored: true
    }
}

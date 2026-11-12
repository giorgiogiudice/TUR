// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T

// The control that opens and closes a section in place, such as a message's
// technical detail. The arrow points forward when closed and down when open.
T.Button {
    id: control

    property alias expanded: control.checked

    checkable: true

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Theme.spaceRelated
    verticalPadding: Theme.spaceGrid
    spacing: Theme.spaceGrid

    Accessible.role: Accessible.Button
    Accessible.name: control.text
    Accessible.checkable: true
    Accessible.checked: control.checked

    contentItem: RowLayout {
        spacing: control.spacing

        Glyph {
            Layout.alignment: Qt.AlignVCenter
            kind: control.checked ? Glyph.ChevronDown : Glyph.ChevronForward
            tint: Theme.textSecondary
            size: 14
        }

        Label {
            Layout.alignment: Qt.AlignVCenter
            text: control.text
            tone: Label.Secondary
            wrapMode: Text.NoWrap
            Accessible.ignored: true
        }

        Item {
            Layout.fillWidth: true
        }
    }

    background: Rectangle {
        implicitHeight: Theme.targetPointer
        radius: Theme.radiusControl
        color: "transparent"

        StateLayer {
            hovered: control.hovered
            pressed: control.down
        }

        FocusRing {
            shown: control.visualFocus
            inside: true
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import Tur.Kit as Kit

// The way back from an application's page: an arrow that points to the start
// of the line and the name of where it leads, "Results" or "Home"
// (desktop-experience.md, section 8.4). The arrow turns round in a
// right-to-left layout.
T.Button {
    id: control

    // What a screen reader says: where the button leads, as a sentence.
    property string spokenName: control.text

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Kit.Theme.spaceRelated
    verticalPadding: Kit.Theme.spaceGrid
    spacing: Kit.Theme.spaceGrid

    Accessible.role: Accessible.Button
    Accessible.name: control.spokenName

    contentItem: RowLayout {
        spacing: control.spacing

        Kit.Glyph {
            Layout.alignment: Qt.AlignVCenter
            // The kit's forward arrow, turned to point back. It mirrors
            // itself, so this is right in both directions.
            kind: Kit.Glyph.ChevronForward
            rotation: 180
            size: 14
        }

        Kit.Label {
            Layout.alignment: Qt.AlignVCenter
            text: control.text
            wrapMode: Text.NoWrap
            Accessible.ignored: true
        }
    }

    background: Rectangle {
        implicitHeight: Kit.Theme.targetPointer
        radius: Kit.Theme.radiusControl
        color: "transparent"

        Kit.StateLayer {
            hovered: control.hovered
            pressed: control.down
        }

        Kit.FocusRing {
            shown: control.visualFocus
        }
    }
}

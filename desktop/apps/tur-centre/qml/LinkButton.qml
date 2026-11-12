// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T
import Tur.Kit as Kit

// An address that opens in the browser when pressed. It is shown in full, as
// it is: a person can see where it leads before going there. Nothing is
// fetched by the application centre itself.
T.Button {
    id: control

    // The address.
    property string url

    text: control.url

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Kit.Theme.spaceGrid
    verticalPadding: Kit.Theme.spaceGrid / 2

    Accessible.role: Accessible.Link
    Accessible.name: control.url

    contentItem: Kit.Label {
        text: control.text
        kind: Kit.Label.Small
        tone: Kit.Label.Accent
        // An address has no spaces to wrap at.
        wrapMode: Text.WrapAnywhere
        verticalAlignment: Text.AlignVCenter
        Accessible.ignored: true
    }

    background: Rectangle {
        implicitHeight: Kit.Theme.targetMinimum
        radius: Kit.Theme.radiusControl
        color: "transparent"

        Kit.StateLayer {
            hovered: control.hovered
            pressed: control.down
            tint: Kit.Theme.accentText
        }

        Kit.FocusRing {
            shown: control.visualFocus
            inside: true
        }
    }
}

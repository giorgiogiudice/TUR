// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T
import Tur.Kit as Kit

// One answer of a question with exclusive answers: a radio button and its
// label (the removal question of desktop-experience.md, 8.5). Chosen is a
// ring with a filled dot, not chosen an empty ring: the state does not rest
// on colour. Drawn in the kit's tokens; the kit has no radio button yet, and
// this one lives here so as not to change the kit's shared files.
T.RadioButton {
    id: control

    readonly property real ringSize: 20

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Kit.Theme.targetPointer,
                             implicitContentHeight + topPadding + bottomPadding,
                             implicitIndicatorHeight + topPadding + bottomPadding)
    verticalPadding: Kit.Theme.spaceRelated
    spacing: Kit.Theme.spaceRelated

    Accessible.role: Accessible.RadioButton
    Accessible.name: control.text
    Accessible.checkable: true
    Accessible.checked: control.checked

    indicator: Rectangle {
        id: ring

        implicitWidth: control.ringSize
        implicitHeight: control.ringSize
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: width / 2
        color: Kit.Theme.surfaceRaised
        border.width: control.checked ? 2 * Kit.Theme.lineWidth : Kit.Theme.lineWidth
        border.color: control.checked && control.enabled ? Kit.Theme.accent : Kit.Theme.lineControl

        Rectangle {
            anchors.centerIn: parent
            visible: control.checked
            width: 10
            height: 10
            radius: 5
            color: control.enabled ? Kit.Theme.accent : Kit.Theme.textDisabled
        }

        Kit.StateLayer {
            cornerRadius: ring.radius
            hovered: control.hovered
            pressed: control.down
            tint: Kit.Theme.textPrimary
        }

        Kit.FocusRing {
            shown: control.visualFocus
            cornerRadius: ring.radius
        }
    }

    contentItem: Kit.Label {
        leftPadding: control.mirrored ? 0 : control.ringSize + control.spacing
        rightPadding: control.mirrored ? control.ringSize + control.spacing : 0
        text: control.text
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.Wrap
        Accessible.ignored: true
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T
import Tur.Kit as Kit

// One of the few answers to a choice: a ring with its label, filled in the
// middle when it is the one chosen, so that the state does not rest on
// colour. The kit has no such control yet; this one is drawn from the kit's
// tokens in the manner of its check box.
T.RadioButton {
    id: control

    readonly property real ringSize: 20
    readonly property real dotSize: 10

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Kit.Theme.targetPointer,
                             implicitContentHeight + topPadding + bottomPadding,
                             implicitIndicatorHeight + topPadding + bottomPadding)
    verticalPadding: Kit.Theme.spaceGrid
    spacing: Kit.Theme.spaceRelated

    Accessible.role: Accessible.RadioButton
    Accessible.name: control.text
    Accessible.checkable: true
    Accessible.checked: control.checked

    indicator: Rectangle {
        id: ring

        implicitWidth: control.ringSize
        implicitHeight: control.ringSize
        // At the start of the line: the left, or the right in a
        // right-to-left layout.
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: height / 2
        color: Kit.Theme.surfaceRaised
        border.width: Kit.Theme.lineWidth
        border.color: control.enabled ? Kit.Theme.lineControl : Kit.Theme.lineSubtle

        Rectangle {
            anchors.centerIn: parent
            width: control.dotSize
            height: control.dotSize
            radius: height / 2
            visible: control.checked
            color: control.enabled ? Kit.Theme.accent : Kit.Theme.textDisabled
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
        Accessible.ignored: true
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// A switch with its label, for a setting that takes effect at once. On is a
// filled track with the handle at the end, off an outlined track with the
// handle at the start: position and fill, not colour alone.
T.Switch {
    id: control

    readonly property real trackWidth: 40
    readonly property real trackHeight: 22
    readonly property real handleSize: 16
    readonly property real handleMargin: (control.trackHeight - control.handleSize) / 2

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Theme.targetPointer,
                             implicitContentHeight + topPadding + bottomPadding,
                             implicitIndicatorHeight + topPadding + bottomPadding)
    verticalPadding: Theme.spaceRelated
    spacing: Theme.spaceRelated

    Accessible.role: Accessible.Button
    Accessible.name: control.text
    Accessible.checkable: true
    Accessible.checked: control.checked

    indicator: Rectangle {
        id: track

        implicitWidth: control.trackWidth
        implicitHeight: control.trackHeight
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: height / 2
        color: control.checked && control.enabled ? Theme.accent : Theme.surfaceRaised
        border.width: control.checked && control.enabled ? 0 : Theme.lineWidth
        border.color: control.enabled ? Theme.lineControl : Theme.lineSubtle

        Rectangle {
            // visualPosition already runs the other way in a right-to-left
            // layout.
            x: control.handleMargin
               + control.visualPosition * (track.width - width - 2 * control.handleMargin)
            anchors.verticalCenter: parent.verticalCenter
            width: control.handleSize
            height: control.handleSize
            radius: height / 2
            color: !control.enabled ? Theme.textDisabled
                   : control.checked ? Theme.accentOn : Theme.lineControl

            Behavior on x {
                enabled: !control.down
                NumberAnimation {
                    duration: Motion.control
                    easing.type: Motion.easing
                }
            }
        }

        FocusRing {
            shown: control.visualFocus
            cornerRadius: track.radius
        }
    }

    contentItem: Label {
        leftPadding: control.mirrored ? 0 : control.trackWidth + control.spacing
        rightPadding: control.mirrored ? control.trackWidth + control.spacing : 0
        text: control.text
        verticalAlignment: Text.AlignVCenter
        Accessible.ignored: true
    }
}

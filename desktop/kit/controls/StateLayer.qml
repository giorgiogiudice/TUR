// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick

// Shows that a control is under the pointer or pressed, by laying a faint
// wash of the control's own text colour over it. The palette has no hover or
// pressed colours; this stays inside it.
Rectangle {
    id: layer

    property bool hovered: false
    property bool pressed: false
    property color tint: Theme.textPrimary
    property real cornerRadius: Theme.radiusControl

    anchors.fill: parent
    radius: layer.cornerRadius
    color: layer.tint
    opacity: layer.pressed ? 0.16 : layer.hovered ? 0.08 : 0

    Behavior on opacity {
        NumberAnimation {
            duration: Motion.fadeControl
            easing.type: Motion.easing
        }
    }
}

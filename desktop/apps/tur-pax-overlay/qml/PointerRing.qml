// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// A click by P@x, shown and not performed (design, 9.4): a gold ring that
// grows from the pointer's tip and fades. With a dark line outside it, as
// every mark of the overlay has, so that it shows on white.
//
// The item's origin is the centre of the ring.
Item {
    id: ring

    // Each time this changes, the ring plays once.
    property int pulses: 0
    // How far it has grown: 0 is just begun, 1 is gone.
    property real progress: 1

    readonly property real radius: 12 + 20 * ring.progress
    readonly property var layers: [Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]

    width: 0
    height: 0
    visible: ring.progress < 1

    Accessible.ignored: true

    onPulsesChanged: grow.restart()

    NumberAnimation {
        id: grow

        target: ring
        property: "progress"
        from: 0
        to: 1
        duration: Kit.Motion.frozen ? 0 : 2 * Math.max(Kit.Motion.fadeAppear, 1)
        easing.type: Easing.OutCubic
    }

    Rectangle {
        x: -ring.radius - 1
        y: -ring.radius - 1
        width: 2 * (ring.radius + 1)
        height: width
        radius: width / 2
        color: "transparent"
        border.width: 1
        border.color: Pax.PaxTheme.overlayOutline
        opacity: 1 - ring.progress
    }

    Rectangle {
        x: -ring.radius
        y: -ring.radius
        width: 2 * ring.radius
        height: width
        radius: width / 2
        color: "transparent"
        border.width: 2.5
        border.color: Pax.PaxTheme.overlayFill
        opacity: 1 - ring.progress
    }
}

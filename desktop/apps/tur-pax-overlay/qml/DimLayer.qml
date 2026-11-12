// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// Dims an area except a hole, by a fifth (design, 9.5: "for a single row in
// a long list, the rest of the window is dimmed by 20 %"), so that the row
// stands out. Four bands round the hole.
Item {
    id: dim

    // What is dimmed, and what is left clear, in the surface's coordinates.
    property rect area
    property rect hole
    property bool shown: true

    readonly property real strength: 0.2

    // The hole, kept inside the area.
    readonly property real holeLeft: Math.max(dim.area.x, Math.min(dim.hole.x, dim.area.x + dim.area.width))
    readonly property real holeTop: Math.max(dim.area.y, Math.min(dim.hole.y, dim.area.y + dim.area.height))
    readonly property real holeRight: Math.max(dim.holeLeft, Math.min(dim.hole.x + dim.hole.width, dim.area.x + dim.area.width))
    readonly property real holeBottom: Math.max(dim.holeTop, Math.min(dim.hole.y + dim.hole.height, dim.area.y + dim.area.height))

    anchors.fill: parent
    opacity: dim.shown ? dim.strength : 0

    Accessible.ignored: true

    Behavior on opacity {
        NumberAnimation {
            duration: Kit.Motion.fadeAppear
        }
    }

    Rectangle {
        x: dim.area.x
        y: dim.area.y
        width: dim.area.width
        height: dim.holeTop - dim.area.y
        color: Pax.PaxTheme.overlayOutline
    }
    Rectangle {
        x: dim.area.x
        y: dim.holeBottom
        width: dim.area.width
        height: dim.area.y + dim.area.height - dim.holeBottom
        color: Pax.PaxTheme.overlayOutline
    }
    Rectangle {
        x: dim.area.x
        y: dim.holeTop
        width: dim.holeLeft - dim.area.x
        height: dim.holeBottom - dim.holeTop
        color: Pax.PaxTheme.overlayOutline
    }
    Rectangle {
        x: dim.holeRight
        y: dim.holeTop
        width: dim.area.x + dim.area.width - dim.holeRight
        height: dim.holeBottom - dim.holeTop
        color: Pax.PaxTheme.overlayOutline
    }
}

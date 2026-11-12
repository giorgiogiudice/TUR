// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// A highlight (design, 9.5): a rounded rectangle round the target, a 3 px
// gold line with a 1 px dark line outside it, 6 px corner radius, 4 px clear
// of the target. The same measures as the highlight tur-settings draws in its
// own window (RowHighlight.qml), with the dark line added: gold alone is
// 1.9:1 on white.
Item {
    id: mark

    // The target, in the surface's coordinates, without the clearance.
    property rect target
    // False while the pointer is still travelling to it.
    property bool shown: true

    readonly property real clearance: 4
    readonly property real line: 3
    readonly property real edge: 1
    readonly property real reach: mark.clearance + mark.line + mark.edge
    readonly property var layers: [Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]

    x: mark.target.x - mark.reach
    y: mark.target.y - mark.reach
    width: mark.target.width + 2 * mark.reach
    height: mark.target.height + 2 * mark.reach
    opacity: mark.shown ? 1 : 0

    Accessible.ignored: true

    Behavior on opacity {
        NumberAnimation {
            duration: Kit.Motion.fadeAppear
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: Kit.Theme.radiusControl + mark.edge
        color: "transparent"
        border.width: mark.edge
        border.color: Pax.PaxTheme.overlayOutline
    }

    Rectangle {
        anchors.fill: parent
        anchors.margins: mark.edge
        radius: Kit.Theme.radiusControl
        color: "transparent"
        border.width: mark.line
        border.color: Pax.PaxTheme.overlayFill
    }
}

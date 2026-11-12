// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// P@x's pointer (design, 9.4; desktop-assets/pax-pointer.svg). Not an arrow:
// the flame, tilted so that its tip is the hot spot, gold with a dark
// outline 1.5 px wide and a white hairline outside that, so that it is
// visible on any background. One and a half times the user's cursor. A tag
// reading "P@x" trails it while it is at rest.
//
// The item's origin is the tip: place it with x and y at the point.
Item {
    id: pointer

    // The height of the flame, in logical pixels.
    property real size: 36
    property bool atRest: true

    // The drawing's grid: the flame is 34 units high.
    readonly property real unit: pointer.size / 34

    // The colours from the outermost line inwards, for the contrast test.
    readonly property var layers: [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]

    width: 0
    height: 0

    Accessible.ignored: true

    Shape {
        width: 34 * pointer.unit
        height: 34 * pointer.unit
        antialiasing: true

        // The white hairline: a wider white line under the outline.
        ShapePath {
            strokeColor: Pax.PaxTheme.overlayHairline
            strokeWidth: 3.5
            fillColor: "transparent"
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(pointer.unit, pointer.unit)

            PathSvg {
                path: "M 0 0 L 24.86 11.03 A 12 12 0 1 1 8.62 25.8 Z"
            }
        }

        ShapePath {
            strokeColor: Pax.PaxTheme.overlayOutline
            strokeWidth: 1.5
            fillColor: Pax.PaxTheme.overlayFill
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(pointer.unit, pointer.unit)

            PathSvg {
                path: "M 0 0 L 24.86 11.03 A 12 12 0 1 1 8.62 25.8 Z"
            }
        }
    }

    // The tag, beside the flame's round end.
    Rectangle {
        objectName: "pointerTag"
        visible: pointer.atRest
        x: 35 * pointer.unit
        y: 27 * pointer.unit
        width: Math.max(30 * pointer.unit, tag.implicitWidth + 10 * pointer.unit)
        height: 14 * pointer.unit
        radius: height / 2
        color: Pax.PaxTheme.overlayFill
        border.width: 1
        border.color: Pax.PaxTheme.overlayOutline

        Text {
            id: tag

            anchors.centerIn: parent
            text: Words.name
            color: Pax.PaxTheme.overlayText
            font.family: Kit.Theme.interfaceFamily
            font.pixelSize: Math.round(8.5 * pointer.unit)
            font.weight: Font.DemiBold
        }
    }
}

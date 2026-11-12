// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// The icon of a state of the pill (design, 9.3): a microphone, the flame, an
// eye, the pointer's flame, a hand, a pause sign. Drawn on a 16 by 16 grid.
// Decoration: the pill's words carry the meaning.
Item {
    id: glyph

    property string kind
    property color tint
    property real size: 16

    implicitWidth: glyph.size
    implicitHeight: glyph.size

    Accessible.ignored: true

    // Filled, and stroked, parts of each icon, as SVG paths on the grid.
    readonly property string filled: {
        switch (glyph.kind) {
        case "listening":
            return "M 8 1 A 3 3 0 0 1 11 4 V 8 A 3 3 0 0 1 5 8 V 4 A 3 3 0 0 1 8 1 Z"
        case "working":
        case "acting":
            // The flame of PaxMark, fitted to the grid; its tip at the top
            // left.
            return "M 1 1 L 12.2 6 A 5.6 5.6 0 1 1 4.9 12.6 Z"
        case "observing":
            return "M 8 5.6 A 2.4 2.4 0 1 1 7.99 5.6 Z"
        case "waiting":
            return "M 5 14.5 L 2.4 9.8 A 1 1 0 0 1 4.1 8.8 L 5 10 V 3.5 A 1 1 0 0 1 7 3.5 V 8 V 2.5 A 1 1 0 0 1 9 2.5 "
                   + "V 8 V 3.5 A 1 1 0 0 1 11 3.5 V 8.5 V 5 A 1 1 0 0 1 13 5 V 10.5 A 4 4 0 0 1 9 14.5 Z"
        case "paused":
            return "M 4 3 H 7 V 13 H 4 Z M 9 3 H 12 V 13 H 9 Z"
        }
        return ""
    }
    readonly property string stroked: {
        switch (glyph.kind) {
        case "listening":
            return "M 3.5 8 A 4.5 4.5 0 0 0 12.5 8 M 8 12.5 V 15"
        case "observing":
            return "M 1 8 C 3.5 3.5 12.5 3.5 15 8 C 12.5 12.5 3.5 12.5 1 8 Z"
        }
        return ""
    }

    Shape {
        anchors.fill: parent
        antialiasing: true

        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: glyph.tint
            scale: Qt.size(glyph.size / 16, glyph.size / 16)

            PathSvg {
                path: glyph.filled
            }
        }

        ShapePath {
            strokeColor: glyph.stroked.length > 0 ? glyph.tint : "transparent"
            strokeWidth: 1.5 * glyph.size / 16
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.size / 16, glyph.size / 16)

            PathSvg {
                path: glyph.stroked.length > 0 ? glyph.stroked : "M 0 0"
            }
        }
    }
}

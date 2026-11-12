// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit.Pax as Pax

// The frame of the activity indicator (design, 9.3): round the window P@x is
// reading, or at the edge of the screen. Observing: 2 px gold, solid.
// Acting: 3 px gold, solid. Waiting for you: 2 px gold, dashed. Paused: 2 px
// grey, dashed. No frame in the other states.
//
// Every line of gold has a 1 px dark line beside it, on the side away from
// the window or towards the inside of the screen: gold alone is 1.9:1 on
// white, below the 3:1 a line that carries meaning needs. The design names
// the gold line only; the dark one is this program's addition.
Item {
    id: frame

    property string activity: "idle"
    // The window, in the surface's coordinates; an empty rectangle means the
    // screen's edges.
    property rect target

    readonly property bool drawn: frame.activity === "observing" || frame.activity === "acting"
                                  || frame.activity === "waiting" || frame.activity === "paused"
    readonly property bool dashed: frame.activity === "waiting" || frame.activity === "paused"
    readonly property real line: frame.activity === "acting" ? 3 : 2
    readonly property color colour: frame.activity === "paused" ? Pax.PaxTheme.overlayPaused : Pax.PaxTheme.overlayFill
    readonly property bool atEdge: frame.target.width <= 0 || frame.target.height <= 0
    readonly property var layers: [Pax.PaxTheme.overlayOutline, frame.colour]

    // The centre lines of the two strokes. At the screen's edge the coloured
    // line is outermost and the dark line inside it; round a window the dark
    // line is outermost.
    readonly property rect colourPath: frame.atEdge
                                       ? Qt.rect(frame.line / 2, frame.line / 2, frame.width - frame.line,
                                                 frame.height - frame.line)
                                       : Qt.rect(frame.target.x - frame.line / 2, frame.target.y - frame.line / 2,
                                                 frame.target.width + frame.line, frame.target.height + frame.line)
    readonly property rect darkPath: frame.atEdge
                                     ? Qt.rect(frame.line + 0.5, frame.line + 0.5, frame.width - 2 * frame.line - 1,
                                               frame.height - 2 * frame.line - 1)
                                     : Qt.rect(frame.target.x - frame.line - 0.5, frame.target.y - frame.line - 0.5,
                                               frame.target.width + 2 * frame.line + 1,
                                               frame.target.height + 2 * frame.line + 1)

    function outline(r: rect): var {
        return [Qt.point(r.x, r.y), Qt.point(r.x + r.width, r.y), Qt.point(r.x + r.width, r.y + r.height),
                Qt.point(r.x, r.y + r.height), Qt.point(r.x, r.y)]
    }

    visible: frame.drawn

    Accessible.ignored: true

    Shape {
        anchors.fill: parent

        ShapePath {
            strokeColor: Pax.PaxTheme.overlayOutline
            strokeWidth: 1
            fillColor: "transparent"
            joinStyle: ShapePath.MiterJoin
            capStyle: ShapePath.SquareCap

            PathPolyline {
                path: frame.outline(frame.darkPath)
            }
        }

        ShapePath {
            strokeColor: frame.colour
            strokeWidth: frame.line
            fillColor: "transparent"
            joinStyle: ShapePath.MiterJoin
            capStyle: frame.dashed ? ShapePath.FlatCap : ShapePath.SquareCap
            strokeStyle: frame.dashed ? ShapePath.DashLine : ShapePath.SolidLine
            dashPattern: [3, 2]

            PathPolyline {
                path: frame.outline(frame.colourPath)
            }
        }
    }
}

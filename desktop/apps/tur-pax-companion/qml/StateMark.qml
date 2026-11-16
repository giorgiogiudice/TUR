// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// The small sign beside the character for a state (tur-pax-companion.md,
// section 1): a spark, two arcs, three dots, an eye, a raised hand, a pause
// sign, a speech bubble, a barred circle, an exclamation mark. Drawn on a 16
// by 16 grid. Where the indicator at the top of the screen has a sign for
// the same state (the eye, the hand, the pause sign) it is the same drawing,
// so that the two never look like different things. Decoration: the
// character's words carry the meaning for a screen reader.
Item {
    id: mark

    property string kind
    property color tint
    property real size: 12

    readonly property string filled: {
        switch (mark.kind) {
        case "noticing":
            return "M 8 1.5 L 9.7 6.3 L 14.5 8 L 9.7 9.7 L 8 14.5 L 6.3 9.7 L 1.5 8 L 6.3 6.3 Z"
        case "thinking":
            return "M 1.6 8 A 1.6 1.6 0 1 1 4.8 8 A 1.6 1.6 0 1 1 1.6 8 Z "
                   + "M 6.4 8 A 1.6 1.6 0 1 1 9.6 8 A 1.6 1.6 0 1 1 6.4 8 Z "
                   + "M 11.2 8 A 1.6 1.6 0 1 1 14.4 8 A 1.6 1.6 0 1 1 11.2 8 Z"
        case "observing":
            return "M 8 5.6 A 2.4 2.4 0 1 1 7.99 5.6 Z"
        case "waiting":
            return "M 5 14.5 L 2.4 9.8 A 1 1 0 0 1 4.1 8.8 L 5 10 V 3.5 A 1 1 0 0 1 7 3.5 V 8 V 2.5 A 1 1 0 0 1 9 2.5 "
                   + "V 8 V 3.5 A 1 1 0 0 1 11 3.5 V 8.5 V 5 A 1 1 0 0 1 13 5 V 10.5 A 4 4 0 0 1 9 14.5 Z"
        case "paused":
            return "M 4 3 H 7 V 13 H 4 Z M 9 3 H 12 V 13 H 9 Z"
        case "telling":
            return "M 3 2.5 H 13 A 1.5 1.5 0 0 1 14.5 4 V 9.5 A 1.5 1.5 0 0 1 13 11 H 8.5 L 5 14.5 V 11 H 3 "
                   + "A 1.5 1.5 0 0 1 1.5 9.5 V 4 A 1.5 1.5 0 0 1 3 2.5 Z"
        case "listening":
            return "M 2 8 A 1.5 1.5 0 1 1 5 8 A 1.5 1.5 0 1 1 2 8 Z"
        }
        return ""
    }
    readonly property string stroked: {
        switch (mark.kind) {
        case "listening":
            return "M 7.5 4.5 A 5 5 0 0 1 7.5 11.5 M 10.5 2 A 8.5 8.5 0 0 1 10.5 14"
        case "observing":
            return "M 1 8 C 3.5 3.5 12.5 3.5 15 8 C 12.5 12.5 3.5 12.5 1 8 Z"
        case "refused":
            return "M 2.5 8 A 5.5 5.5 0 1 1 13.5 8 A 5.5 5.5 0 1 1 2.5 8 Z M 4.2 4.2 L 11.8 11.8"
        case "error":
            return "M 8 2.5 V 9 M 8 12.6 V 12.7"
        }
        return ""
    }

    implicitWidth: mark.size
    implicitHeight: mark.size

    Accessible.ignored: true

    Shape {
        anchors.fill: parent
        antialiasing: true

        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: mark.filled.length > 0 ? mark.tint : "transparent"
            scale: Qt.size(mark.size / 16, mark.size / 16)

            PathSvg {
                path: mark.filled.length > 0 ? mark.filled : "M 0 0"
            }
        }

        ShapePath {
            strokeColor: mark.stroked.length > 0 ? mark.tint : "transparent"
            strokeWidth: Math.max(1.5, 2 * mark.size / 16)
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(mark.size / 16, mark.size / 16)

            PathSvg {
                path: mark.stroked.length > 0 ? mark.stroked : "M 0 0"
            }
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// The few line glyphs the kit's own controls need: the four status signs, a
// check mark, the disclosure arrows and a cross. Drawn here, in the control's
// colour, so that the kit needs no icon theme and no library from KDE (the
// trusted screen and the installer use the kit with neither). A glyph is
// decoration: the control it sits in carries the word for a screen reader.
Item {
    id: glyph

    enum Kind {
        Info,
        Positive,
        Warning,
        Negative,
        Check,
        ChevronForward,
        ChevronDown,
        Close
    }

    property int kind: Glyph.Info
    property color tint: Theme.textPrimary
    property real size: 16

    // "Forward" points to the end of the line, which is the left in a
    // right-to-left layout.
    readonly property bool flipped: glyph.kind === Glyph.ChevronForward
                                    && Application.layoutDirection === Qt.RightToLeft

    // Paths on a 16 by 16 grid, stroked.
    readonly property string circle: "M 1.75 8 A 6.25 6.25 0 0 1 14.25 8 A 6.25 6.25 0 0 1 1.75 8 Z "
    readonly property string outline: {
        switch (glyph.kind) {
        case Glyph.Positive:
            return glyph.circle + "M 4.9 8.3 L 7.1 10.4 L 11.1 5.8"
        case Glyph.Warning:
            return "M 8 2 L 14.5 13.5 L 1.5 13.5 Z M 8 6.5 L 8 9.6 M 8 11.6 L 8 11.7"
        case Glyph.Negative:
            return glyph.circle + "M 5.75 5.75 L 10.25 10.25 M 10.25 5.75 L 5.75 10.25"
        case Glyph.Check:
            return "M 3.5 8.5 L 6.6 11.5 L 12.5 4.75"
        case Glyph.ChevronForward:
            return "M 6 3.5 L 10.5 8 L 6 12.5"
        case Glyph.ChevronDown:
            return "M 3.5 6 L 8 10.5 L 12.5 6"
        case Glyph.Close:
            return "M 4 4 L 12 12 M 12 4 L 4 12"
        default:
            return glyph.circle + "M 8 7.25 L 8 11.25 M 8 4.75 L 8 4.85"
        }
    }

    implicitWidth: glyph.size
    implicitHeight: glyph.size

    Accessible.ignored: true

    Shape {
        anchors.fill: parent
        antialiasing: true
        transform: Scale {
            origin.x: glyph.width / 2
            xScale: glyph.flipped ? -1 : 1
        }

        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: Math.max(1.5, glyph.width / 16 * 1.5)
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: glyph.outline
            }
        }
    }
}

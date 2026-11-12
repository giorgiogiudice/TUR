// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit as Kit

// The five rating icons of desktop-experience.md, section 8.2. No stars, no
// percentages, no medals. Each rating has a shape of its own, so that it is
// never told by colour alone: a tick in a circle, a tick with a small
// exclamation mark, an exclamation mark in a triangle, a cross in a circle,
// and a question mark in a dashed circle.
//
// The icon is decoration. The rating's words stand beside it and are what a
// screen reader reads.
Item {
    id: glyph

    // The catalogue's rating: good, usable, poor, broken or unknown.
    property string rating: "unknown"
    property real size: 16

    readonly property color tint: {
        switch (glyph.rating) {
        case "good":
        case "usable":
            return Kit.Theme.positive
        case "poor":
            return Kit.Theme.warning
        case "broken":
            return Kit.Theme.negative
        default:
            return Kit.Theme.textSecondary
        }
    }

    // Three of the five are signs the kit already draws.
    readonly property bool fromKit: glyph.rating === "good" || glyph.rating === "poor" || glyph.rating === "broken"
    readonly property real strokeWidth: Math.max(1.5, glyph.width / 16 * 1.5)

    implicitWidth: glyph.size
    implicitHeight: glyph.size

    Accessible.ignored: true

    Kit.Glyph {
        anchors.fill: parent
        visible: glyph.fromKit
        size: glyph.size
        tint: glyph.tint
        kind: glyph.rating === "good" ? Kit.Glyph.Positive
              : glyph.rating === "poor" ? Kit.Glyph.Warning : Kit.Glyph.Negative
    }

    // Works, with minor problems: a tick with a small exclamation mark.
    Shape {
        anchors.fill: parent
        visible: glyph.rating === "usable"
        antialiasing: true

        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: glyph.strokeWidth
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: "M 1.75 8.75 L 4.75 11.75 L 10 5.25 M 13.5 4 L 13.5 9.25 M 13.5 11.9 L 13.5 12"
            }
        }
    }

    // No reports: a question mark in a dashed circle.
    Shape {
        anchors.fill: parent
        visible: !glyph.fromKit && glyph.rating !== "usable"
        antialiasing: true

        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: glyph.strokeWidth
            fillColor: "transparent"
            strokeStyle: ShapePath.DashLine
            dashPattern: [1.6, 1.4]
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: "M 1.75 8 A 6.25 6.25 0 0 1 14.25 8 A 6.25 6.25 0 0 1 1.75 8 Z"
            }
        }

        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: glyph.strokeWidth
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: "M 6.1 6.4 C 6.1 4.3 9.9 4.3 9.9 6.4 C 9.9 7.7 8 7.8 8 9.3 M 8 11.5 L 8 11.6"
            }
        }
    }
}

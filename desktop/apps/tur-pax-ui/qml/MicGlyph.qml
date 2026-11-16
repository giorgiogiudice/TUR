// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit as Kit

// A microphone, drawn in lines like the kit's glyphs (Kit.Glyph has none).
// Decoration: the control or the line it sits beside carries the words for a
// screen reader. `lit`: P@x's microphone is open, and the capsule is filled
// in the system's blue; never gold, which is P@x's (desktop-experience.md,
// 5.1): the microphone is the person's, like Stop.
Item {
    id: glyph

    property bool lit: false
    property color tint: glyph.lit ? Kit.Theme.accent : Kit.Theme.textSecondary
    property real size: 16

    implicitWidth: glyph.size
    implicitHeight: glyph.size
    Accessible.ignored: true

    Shape {
        anchors.fill: parent
        antialiasing: true

        // The capsule.
        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: Math.max(1.5, glyph.width / 16 * 1.5)
            fillColor: glyph.lit ? glyph.tint : "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: "M 5.5 4 A 2.5 2.5 0 0 1 10.5 4 L 10.5 8 A 2.5 2.5 0 0 1 5.5 8 Z"
            }
        }

        // The stand.
        ShapePath {
            strokeColor: glyph.tint
            strokeWidth: Math.max(1.5, glyph.width / 16 * 1.5)
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(glyph.width / 16, glyph.height / 16)

            PathSvg {
                path: "M 3.25 7.5 A 4.75 4.75 0 0 0 12.75 7.5 M 8 12.25 L 8 14.5 M 5.5 14.5 L 10.5 14.5"
            }
        }
    }
}

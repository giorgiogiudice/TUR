// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// P@x's flame: the teardrop of the pointer sketch
// (docs/architecture/desktop-assets/pax-pointer.svg), as a small mark beside
// P@x's name. A placeholder shape until the artwork exists. Decoration: the
// thing it sits in carries the name.
Item {
    id: mark

    property real size: 16

    // On a surface the mark is in P@x's line colour; on P@x's own gold fill
    // the pill passes the colour of its text.
    property color tint: PaxTheme.pax

    implicitWidth: mark.size
    implicitHeight: mark.size

    Accessible.ignored: true

    Shape {
        anchors.fill: parent
        antialiasing: true

        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: mark.tint
            // The sketch's path is 32 wide and 34 high; one unit of margin
            // at the left centres it in a 34 by 34 box.
            scale: Qt.size(mark.width / 34, mark.height / 34)

            PathSvg {
                path: "M 1 0 L 25.86 11.03 A 12 12 0 1 1 9.62 25.8 Z"
            }
        }
    }
}

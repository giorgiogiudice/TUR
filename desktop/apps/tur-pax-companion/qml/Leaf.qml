// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The ground of everything that opens above the character: the bubble, the
// whole notice, the menu and its pages. It floats over whatever is on the
// screen, so its fill sets it apart and its line is one that can be seen
// (line.control, 3:1), not decoration. It follows the desktop's light or
// dark variant, as P@x's window does.
Kit.Page {
    id: leaf

    // The height at which nothing is cut off.
    readonly property real naturalHeight: leaf.implicitContentHeight + leaf.topPadding + leaf.bottomPadding

    padding: Kit.Theme.spaceGroup

    background: Rectangle {
        color: Kit.Theme.surfaceOverlay
        radius: Kit.Theme.radiusSheet
        border.width: Kit.Theme.lineWidth
        border.color: Kit.Theme.lineControl
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// A raised container for a group of related things, or for the rows of a
// list. Depth is the surface tone and a hairline; there is no shadow inside a
// window (desktop-experience.md, 5.4).
T.Pane {
    id: control

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            contentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             contentHeight + topPadding + bottomPadding)
    padding: Theme.spaceGroup

    background: Rectangle {
        color: Theme.surfaceRaised
        radius: Theme.radiusCard
        border.width: Theme.lineWidth
        border.color: Theme.lineSubtle
    }
}

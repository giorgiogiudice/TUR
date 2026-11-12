// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The highlight around a row that was asked for by its address
// (desktop-experience.md, 9.5): a rounded line, 3 px wide, 4 px clear of the
// row, with a 6 px corner. It is a child of the row, so it stays on the row
// while the page scrolls or the window changes size.
//
// Gold is P@x and nothing else (5.1). The line is gold only when the
// assistant asked for the row; a row opened by a person, from the command
// line, a link or the search, is marked in the system's blue. This is the
// one file of this program that may use gold (desktop/ALLOWED_IMPORTS).
Rectangle {
    id: mark

    // The assistant asked for the row.
    property bool pax: false

    // How far the line stands from the row, and how wide it is.
    readonly property real clearance: 4
    readonly property real line: 3
    readonly property real reach: mark.clearance + mark.line

    anchors.fill: parent
    anchors.margins: -mark.reach
    radius: Kit.Theme.radiusControl
    color: "transparent"
    border.width: mark.line
    border.color: mark.pax ? Pax.PaxTheme.pax : Kit.Theme.accentText

    Accessible.ignored: true
}

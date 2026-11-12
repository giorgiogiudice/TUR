// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick

// The focus ring of desktop-experience.md, 5.4: 2 px, in accent.text, 2 px
// clear of the control, on every focusable thing, never removed. Put it in a
// control's background; it fills its parent and reaches outside it.
Rectangle {
    id: ring

    // Whether the control has keyboard focus.
    property bool shown: false

    // The radius of the control the ring goes around.
    property real cornerRadius: Theme.radiusControl

    // Draw the ring inside the control's edge. For rows that touch their
    // neighbours, where there is no room outside.
    property bool inside: false

    // The ring's colour. accent.text everywhere, except on a filled row,
    // where the kit passes the colour of the row's text.
    property color tint: Theme.accentText

    readonly property real reach: Theme.lineFocusGap + Theme.lineFocusWidth

    anchors.fill: parent
    anchors.margins: ring.inside ? Theme.lineFocusGap : -ring.reach
    radius: ring.inside ? Math.max(0, ring.cornerRadius - Theme.lineFocusGap) : ring.cornerRadius + ring.reach
    color: "transparent"
    border.width: Theme.lineFocusWidth
    border.color: ring.tint
    visible: ring.shown
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The solid form of P@x's pill: gold, dark text, a dark outline and a white
// hairline, so that it reads on any background (desktop-experience.md, 5.2
// "P@x's overlay" and 9.3). This is the shape and the colours only; the
// activity indicator, with its states and its lease, is tur-pax-overlay's.
Rectangle {
    id: pill

    // What P@x is doing, as words. Also the pill's name for a screen reader.
    property string text

    readonly property real outlineWidth: 2

    implicitWidth: row.implicitWidth + 2 * Kit.Theme.spaceGroup + 2
    implicitHeight: Math.max(Kit.Theme.targetPointer, row.implicitHeight + 2 * Kit.Theme.spaceGrid) + 2
    radius: Kit.Theme.radiusSheet + 1
    color: PaxTheme.overlayHairline

    Accessible.role: Accessible.Indicator
    Accessible.name: pill.text

    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        radius: Kit.Theme.radiusSheet
        color: PaxTheme.fill
        border.width: pill.outlineWidth
        border.color: PaxTheme.overlayOutline

        RowLayout {
            id: row

            anchors.centerIn: parent
            spacing: Kit.Theme.spaceRelated

            PaxMark {
                Layout.alignment: Qt.AlignVCenter
                tint: PaxTheme.fillText
            }

            Kit.Label {
                Layout.alignment: Qt.AlignVCenter
                text: pill.text
                kind: Kit.Label.Strong
                color: PaxTheme.fillText
                wrapMode: Text.NoWrap
                Accessible.ignored: true
            }
        }
    }
}

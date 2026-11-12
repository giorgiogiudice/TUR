// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// P@x's pointer (design, 9.4). Not an arrow: P@x's flame (Tur.Kit.Pax's
// PaxFlame, the first draft of the flat mark of the owner's answer 17.1.5),
// tilted so that its tip is the hot spot, gold with a dark outline 1.5 px
// wide and a white hairline outside that, so that it is visible on any
// background; its inner flame and core as in the logo. One and a half times
// the user's cursor. A tag reading "P@x" trails it while it is at rest.
//
// The item's origin is the tip: place it with x and y at the point.
Item {
    id: pointer

    // The length of the flame from its tip, in logical pixels.
    property real size: 36
    property bool atRest: true

    // The colours from the outermost line inwards, for the contrast test.
    readonly property var layers: [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]

    width: 0
    height: 0

    Accessible.ignored: true

    // The flame stands upright with its tip at the top centre of its box;
    // turned 45 degrees about the tip, its body lies to the lower right.
    Pax.PaxFlame {
        x: -pointer.size / 2
        y: 0
        size: pointer.size
        tint: Pax.PaxTheme.overlayFill
        inner: Pax.PaxTheme.overlayText
        core: Pax.PaxTheme.overlayFill
        outline: Pax.PaxTheme.overlayOutline
        outlineWidth: 1.5
        hairline: Pax.PaxTheme.overlayHairline
        hairlineWidth: 3.5
        transform: Rotation {
            origin.x: pointer.size / 2
            origin.y: 0
            angle: -45
        }
    }

    // The tag, beside the flame's round end.
    Rectangle {
        objectName: "pointerTag"
        visible: pointer.atRest
        x: 0.95 * pointer.size
        y: 0.8 * pointer.size
        width: Math.max(0.85 * pointer.size, tag.implicitWidth + 0.3 * pointer.size)
        height: 0.4 * pointer.size
        radius: height / 2
        color: Pax.PaxTheme.overlayFill
        border.width: 1
        border.color: Pax.PaxTheme.overlayOutline

        Text {
            id: tag

            anchors.centerIn: parent
            text: Words.name
            color: Pax.PaxTheme.overlayText
            font.family: Kit.Theme.interfaceFamily
            font.pixelSize: Math.round(0.24 * pointer.size)
            font.weight: Font.DemiBold
        }
    }
}

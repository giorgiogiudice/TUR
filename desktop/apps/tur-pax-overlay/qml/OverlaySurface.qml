// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// What one screen shows of P@x (design, 9.3 to 9.6): the dimming, the frame,
// the highlights and their captions, the pill and P@x's pointer, in that
// order from the bottom. The overlay's state is in global coordinates; this
// surface draws the part that falls on its screen.
//
// Nothing here takes input: the window it is in lets every event through
// (surfaces.cpp), and no item asks for focus.
Item {
    id: surface

    required property OverlayState overlay
    required property string screenName
    // The screen, in global logical pixels.
    required property rect screenGeometry

    readonly property real originX: surface.screenGeometry.x
    readonly property real originY: surface.screenGeometry.y

    // A rectangle of the overlay's state, in this surface's coordinates.
    function local(x: real, y: real, width: real, height: real): rect {
        return Qt.rect(x - surface.originX, y - surface.originY, width, height)
    }

    function followMotion() {
        surface.overlay.motionFactor = Kit.Motion.factor
        surface.overlay.frozen = Kit.Motion.frozen
    }

    width: surface.screenGeometry.width
    height: surface.screenGeometry.height

    Component.onCompleted: surface.followMotion()

    Connections {
        target: Kit.Motion

        function onFactorChanged() {
            surface.followMotion()
        }

        function onFrozenChanged() {
            surface.followMotion()
        }
    }

    Repeater {
        model: surface.overlay.highlights

        delegate: DimLayer {
            required property var modelData

            visible: modelData.dim
            area: modelData.dimWidth > 0
                  ? surface.local(modelData.dimX, modelData.dimY, modelData.dimWidth, modelData.dimHeight)
                  : Qt.rect(0, 0, surface.width, surface.height)
            // The hole is what is inside the highlight's line (4 px clear of
            // the target); the line itself is drawn over the dimming, so its
            // rounded corners leave nothing undimmed outside them.
            hole: surface.local(modelData.x - 4, modelData.y - 4, modelData.width + 8, modelData.height + 8)
            shown: modelData.shown
        }
    }

    ScreenFrame {
        objectName: "frame"
        anchors.fill: parent
        activity: surface.overlay.activity
        target: surface.overlay.frameRect.width > 0
                ? surface.local(surface.overlay.frameRect.x, surface.overlay.frameRect.y,
                                surface.overlay.frameRect.width, surface.overlay.frameRect.height)
                : Qt.rect(0, 0, 0, 0)
    }

    Repeater {
        model: surface.overlay.highlights

        delegate: HighlightMark {
            required property var modelData

            objectName: "highlight-" + modelData.id
            target: surface.local(modelData.x, modelData.y, modelData.width, modelData.height)
            shown: modelData.shown
        }
    }

    Repeater {
        model: surface.overlay.highlights

        delegate: CaptionBubble {
            id: bubble

            required property var modelData

            // Only on the screen the highlight is on.
            readonly property bool here: bubble.beside.x + bubble.beside.width / 2 >= 0
                                         && bubble.beside.x + bubble.beside.width / 2 < surface.width
                                         && bubble.beside.y + bubble.beside.height / 2 >= 0
                                         && bubble.beside.y + bubble.beside.height / 2 < surface.height

            objectName: "caption-" + modelData.id
            beside: surface.local(modelData.x - 8, modelData.y - 8, modelData.width + 16, modelData.height + 16)
            area: Qt.rect(0, 0, surface.width, surface.height)
            step: modelData.step
            caption: modelData.caption
            shown: modelData.shown && bubble.here
        }
    }

    ActivityPill {
        objectName: "pill"
        anchors.horizontalCenter: parent.horizontalCenter
        y: 12
        activity: surface.overlay.activity
        scope: surface.overlay.scope
    }

    PointerRing {
        objectName: "ring"
        x: surface.overlay.pulsePoint.x - surface.originX
        y: surface.overlay.pulsePoint.y - surface.originY
        pulses: surface.overlay.pulses
    }

    PaxPointer {
        objectName: "pointer"
        x: surface.overlay.pointer.x - surface.originX
        y: surface.overlay.pointer.y - surface.originY
        visible: surface.overlay.pointerVisible
        opacity: surface.overlay.pointerOpacity
        atRest: !surface.overlay.pointerMoving
        size: 1.5 * surface.overlay.cursorSize
    }
}

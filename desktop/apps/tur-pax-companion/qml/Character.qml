// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// P@x's character: a small flame, the flame at the heart of TUR's mark
// (Tur.Kit.Pax's PaxFlame is the same drawing), that lives on the bar
// (docs/architecture/tur-pax-companion.md, section 1). It has no face. What
// it has is a body, an inner flame, and a core that is its eye; it speaks
// with its posture, with the core, and with one small mark beside it.
//
// Every state differs from every other in shape and in its mark, never in
// colour alone, and has its own words (Words.state). Nothing here moves
// unless something is true: the core sways only while P@x is working, the
// ring beats only while P@x is controlling, and with reduced motion, with
// motion frozen for a picture, or on battery nothing loops at all.
//
// Honest signals, not decoration:
//  - `fuel`: how much of today's spending limit is left is how high the
//    inner flame burns;
//  - `saving`: on battery the character keeps still and dims;
//  - `attentive`: it leans a little when the pointer is on it, which says
//    "this can be pressed".
//
// Its palette is fixed, like the overlay's: it sits on a bar and over
// windows that TUR does not draw. Gold with a dark line and a white
// hairline reads on any of them (tests/tst_character.qml).
Item {
    id: character

    // One of Companion.states().
    property string phase: "idle"
    // The side of the box the character is drawn in.
    property real size: 40
    property real fuel: 1
    property bool saving: false
    property bool attentive: false

    readonly property bool asleep: character.phase === "asleep"
    readonly property bool loud: character.phase === "acting"
    readonly property bool banked: character.phase === "paused"
    // The mark beside the flame; none while asleep, idle or acting.
    readonly property string mark: {
        switch (character.phase) {
        case "asleep":
        case "idle":
        case "acting":
            return ""
        }
        return character.phase
    }
    readonly property bool hasInner: !character.asleep && !character.banked && character.phase !== "error"
    readonly property bool hasCore: character.hasInner
    // Whether anything is moving on a loop right now.
    readonly property bool looping: Kit.Motion.loops && !character.saving
                                    && (character.phase === "thinking" || character.phase === "acting")

    // The colours from the outermost line inwards, for the contrast test.
    readonly property var layers: character.asleep
                                  ? [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayPaused, Pax.PaxTheme.overlayOutline]
                                  : character.banked
                                    ? [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayOutline]
                                    : [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]
    readonly property var markLayers: [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayOutline]
    readonly property color markGlyph: Pax.PaxTheme.overlayFill
    readonly property color markGround: Pax.PaxTheme.overlayOutline

    readonly property color bodyFill: character.asleep || character.banked ? Pax.PaxTheme.overlayOutline
                                                                           : Pax.PaxTheme.overlayFill
    readonly property color bodyLine: character.asleep ? Pax.PaxTheme.overlayPaused
                                      : character.banked ? Pax.PaxTheme.overlayFill : Pax.PaxTheme.overlayOutline
    // Inside the flame: dark on gold. While it is loud the flame itself is
    // dark, on a gold disc, as the indicator's pill is while P@x controls.
    readonly property color innerFill: character.loud ? Pax.PaxTheme.overlayFill : Pax.PaxTheme.overlayOutline
    readonly property color coreFill: character.loud ? Pax.PaxTheme.overlayOutline : Pax.PaxTheme.overlayFill

    // Posture, in the flame's own 32 by 32 grid and in degrees.
    readonly property real lean: {
        if (character.phase === "listening")
            return -9
        if (character.phase === "telling")
            return 6
        return character.attentive && !character.asleep ? -4 : 0
    }
    readonly property real stretch: {
        switch (character.phase) {
        case "asleep":
            return 0.78
        case "refused":
            return 0.8
        case "error":
            return 0.88
        case "noticing":
        case "telling":
            return 1.05
        }
        return 1
    }
    readonly property real spread: character.asleep ? 0.86 : character.phase === "refused" ? 1.06 : 1
    readonly property real coreScale: {
        switch (character.phase) {
        case "listening":
            return 1.3
        case "observing":
            return 1.55
        case "waiting":
            return 0.8
        }
        return 1
    }
    readonly property real coreRise: character.phase === "noticing" || character.phase === "telling" ? -2 : 0
    // Sideways, while P@x works: the core sways. Zero when nothing loops.
    property real sway: 0
    // The ring's beat while P@x controls: 0 to 1.
    property real beat: 0

    // The flame and its parts on the 32 grid (Tur.Kit.Pax's PaxFlame).
    readonly property string outerPath: "M 16 0 C 19 7 27 13 27 21 A 11 11 0 0 1 5 21 C 5 13 13 7 16 0 Z"
    readonly property string innerPath: "M 16 14 C 17.5 16.5 20 19 20 22.5 A 4 4 0 0 1 12 22.5 C 12 19 14.5 16.5 16 14 Z"
    readonly property string corePath: "M 16 20.5 A 2.5 2.5 0 1 1 15.99 20.5 Z"

    // The flame is drawn a little smaller than the box, so that its lines
    // and its mark stay inside it.
    readonly property real flameSize: character.size * 0.8
    readonly property real unit: character.flameSize / 32

    implicitWidth: character.size
    implicitHeight: character.size
    opacity: character.saving ? 0.78 : 1

    Accessible.ignored: true

    SequentialAnimation on sway {
        running: character.looping && character.phase === "thinking"
        loops: Animation.Infinite
        alwaysRunToEnd: false

        NumberAnimation {
            to: 1.6
            duration: Kit.Motion.panel * 2
            easing.type: Easing.InOutSine
        }
        NumberAnimation {
            to: -1.6
            duration: Kit.Motion.panel * 4
            easing.type: Easing.InOutSine
        }
        NumberAnimation {
            to: 0
            duration: Kit.Motion.panel * 2
            easing.type: Easing.InOutSine
        }
    }

    SequentialAnimation on beat {
        running: character.looping && character.loud
        loops: Animation.Infinite
        alwaysRunToEnd: false

        NumberAnimation {
            to: 1
            duration: Kit.Motion.panel * 3
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            to: 0
            duration: Kit.Motion.panel * 3
            easing.type: Easing.InCubic
        }
    }

    onLoopingChanged: {
        if (!character.looping) {
            character.sway = 0
            character.beat = 0
        }
    }

    // Loud: a gold disc with a dark line and a white hairline, as the
    // indicator's pill is while P@x controls. The ring beats inside it.
    Rectangle {
        objectName: "paxDiscHairline"
        visible: character.loud
        anchors.centerIn: parent
        width: character.size
        height: character.size
        radius: character.size / 2
        color: Pax.PaxTheme.overlayHairline
    }

    Rectangle {
        objectName: "paxDisc"
        visible: character.loud
        anchors.centerIn: parent
        width: character.size - 2
        height: character.size - 2
        radius: width / 2
        color: Pax.PaxTheme.overlayFill
        border.width: 2 + character.beat
        border.color: Pax.PaxTheme.overlayOutline
    }

    Item {
        id: figure

        objectName: "paxFigure"
        width: character.flameSize
        height: character.flameSize
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: character.loud ? (character.size - character.flameSize * 0.72) / 2
                                             : (character.size - character.flameSize) / 2
        scale: character.loud ? 0.72 : 1
        transformOrigin: Item.Bottom

        transform: [
            Scale {
                origin.x: figure.width / 2
                origin.y: figure.height
                xScale: character.spread
                yScale: character.stretch

                Behavior on yScale {
                    NumberAnimation {
                        duration: Kit.Motion.control
                        easing.type: Kit.Motion.easing
                    }
                }
            },
            Rotation {
                origin.x: figure.width / 2
                origin.y: figure.height
                angle: character.lean

                Behavior on angle {
                    NumberAnimation {
                        duration: Kit.Motion.control
                        easing.type: Kit.Motion.easing
                    }
                }
            }
        ]

        Shape {
            anchors.fill: parent
            antialiasing: true

            // The white hairline, outside everything; not while loud, where
            // the disc carries it.
            ShapePath {
                strokeColor: character.loud ? "transparent" : Pax.PaxTheme.overlayHairline
                strokeWidth: character.loud ? 0 : 5
                fillColor: "transparent"
                joinStyle: ShapePath.RoundJoin
                scale: Qt.size(character.unit, character.unit)

                PathSvg {
                    path: character.outerPath
                }
            }

            // The body: its line, then its fill.
            ShapePath {
                strokeColor: character.loud ? "transparent" : character.bodyLine
                strokeWidth: character.loud ? 0 : 3
                fillColor: character.loud ? Pax.PaxTheme.overlayOutline : character.bodyFill
                joinStyle: ShapePath.RoundJoin
                scale: Qt.size(character.unit, character.unit)

                PathSvg {
                    path: character.outerPath
                }
            }
        }

        // The inner flame: as high as there is spending limit left today.
        Shape {
            objectName: "paxInner"
            anchors.fill: parent
            antialiasing: true
            visible: character.hasInner

            transform: Scale {
                origin.x: 16 * character.unit
                origin.y: 26.5 * character.unit
                xScale: 0.7 + 0.3 * character.fuel
                yScale: 0.45 + 0.55 * character.fuel
            }

            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: character.innerFill
                scale: Qt.size(character.unit, character.unit)

                PathSvg {
                    path: character.innerPath
                }
            }
        }

        // The core: its eye.
        Shape {
            objectName: "paxCore"
            anchors.fill: parent
            antialiasing: true
            visible: character.hasCore

            transform: [
                Scale {
                    origin.x: 16 * character.unit
                    origin.y: 23 * character.unit
                    xScale: character.coreScale
                    yScale: character.coreScale
                },
                Translate {
                    x: character.sway * character.unit
                    y: character.coreRise * character.unit
                },
                // When the inner flame burns low the core shrinks with it and
                // stays inside it: left as it is, a full core over a low
                // inner flame read as a smile (the low-fuel picture of run
                // 43774347681), and the character has no face.
                Scale {
                    id: coreFuel

                    readonly property real factor: Math.min(1, (0.45 + 0.55 * character.fuel) / 0.725)

                    origin.x: 16 * character.unit
                    origin.y: 26.5 * character.unit
                    xScale: coreFuel.factor
                    yScale: coreFuel.factor
                }
            ]

            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: character.coreFill
                scale: Qt.size(character.unit, character.unit)

                PathSvg {
                    path: character.corePath
                }
            }
        }
    }

    // The mark: a small dark disc with a white hairline, the sign in gold.
    Rectangle {
        id: badge

        objectName: "paxMarkGround"
        visible: character.mark.length > 0
        width: Math.max(12, Math.round(character.size * 0.42))
        height: badge.width
        radius: badge.width / 2
        anchors.top: parent.top
        anchors.right: parent.right
        color: character.markGround
        border.width: 1
        border.color: Pax.PaxTheme.overlayHairline

        StateMark {
            objectName: "paxMark"
            anchors.centerIn: parent
            kind: character.mark
            size: badge.width - 5
            tint: character.markGlyph
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The caption beside a highlight (design, 9.5 and 9.6): the step ("Step 2 of
// 3") and one sentence. Beside the highlight, never over it: to its right if
// there is room on the screen, else to its left, else below or above it.
//
// The design's guide controls (Back, Repeat, Do it for me, Stop) are not
// drawn: the overlay takes no input, and a button that cannot be pressed
// must not be shown (tur-pax-overlay.md, 7).
Rectangle {
    id: bubble

    // The highlight's outer edge and the screen, in the surface's
    // coordinates.
    property rect beside
    property rect area
    property string step
    property string caption
    property bool shown: true

    readonly property real gap: Kit.Theme.spaceRelated + 4
    readonly property real margin: Kit.Theme.spaceRelated
    readonly property real padding: Kit.Theme.spaceGroup - 2
    readonly property real widest: 320

    readonly property bool fitsRight: bubble.beside.x + bubble.beside.width + bubble.gap + bubble.width
                                      <= bubble.area.x + bubble.area.width - bubble.margin
    readonly property bool fitsLeft: bubble.beside.x - bubble.gap - bubble.width >= bubble.area.x + bubble.margin
    readonly property bool fitsBelow: bubble.beside.y + bubble.beside.height + bubble.gap + bubble.height
                                      <= bubble.area.y + bubble.area.height - bubble.margin
    readonly property bool sideways: bubble.fitsRight || bubble.fitsLeft

    readonly property var layers: [Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayOutline]

    function clamp(value: real, low: real, high: real): real {
        return Math.max(low, Math.min(value, high))
    }

    // As wide as the longer line needs, up to `widest`; measured apart from
    // the labels, whose width depends on this one.
    width: Math.min(bubble.widest, Math.ceil(Math.max(stepMetrics.advanceWidth, sentenceMetrics.advanceWidth))
                    + 2 * bubble.padding)
    height: column.implicitHeight + 2 * bubble.padding
    x: bubble.fitsRight ? bubble.beside.x + bubble.beside.width + bubble.gap
       : bubble.fitsLeft ? bubble.beside.x - bubble.gap - bubble.width
       : bubble.clamp(bubble.beside.x, bubble.area.x + bubble.margin,
                      bubble.area.x + bubble.area.width - bubble.width - bubble.margin)
    y: bubble.sideways ? bubble.clamp(bubble.beside.y + bubble.beside.height / 2 - bubble.height / 2,
                                      bubble.area.y + bubble.margin,
                                      bubble.area.y + bubble.area.height - bubble.height - bubble.margin)
       : bubble.fitsBelow ? bubble.beside.y + bubble.beside.height + bubble.gap
       : bubble.beside.y - bubble.gap - bubble.height
    radius: Kit.Theme.radiusSheet
    color: Pax.PaxTheme.overlayOutline
    border.width: 1.5
    border.color: Pax.PaxTheme.overlayFill
    visible: bubble.caption.length > 0 || bubble.step.length > 0
    opacity: bubble.shown ? 1 : 0

    Accessible.role: Accessible.StaticText
    Accessible.name: bubble.step.length > 0 ? bubble.step + ". " + bubble.caption : bubble.caption

    Behavior on opacity {
        NumberAnimation {
            duration: Kit.Motion.fadeAppear
        }
    }

    TextMetrics {
        id: stepMetrics

        font: stepLabel.font
        text: bubble.step
    }

    TextMetrics {
        id: sentenceMetrics

        font: sentence.font
        text: bubble.caption
    }

    Column {
        id: column

        x: bubble.padding
        y: bubble.padding
        width: bubble.width - 2 * bubble.padding
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            id: stepLabel

            objectName: "captionStep"
            visible: bubble.step.length > 0
            width: parent.width
            text: bubble.step
            kind: Kit.Label.Small
            color: Pax.PaxTheme.overlayPaused
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }

        Kit.Label {
            id: sentence

            objectName: "captionText"
            visible: bubble.caption.length > 0
            width: parent.width
            text: bubble.caption
            color: Pax.PaxTheme.overlayQuietText
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }
    }
}

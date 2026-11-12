// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The pill of the activity indicator (design, 9.3): top centre of every
// screen, 36 px high, an icon and words that differ for every state. Quiet
// when P@x is quiet, loud when it is not: dark with a gold outline while it
// listens, works, observes or waits; solid gold with dark text while it acts;
// grey and dashed while paused.
//
// It takes no input. The design's Stop, Pause and Resume buttons need a
// surface that accepts a click, which this one must not; they belong to the
// conversation panel's pill (tur-pax-ui) until then (tur-pax-overlay.md, 7).
Item {
    id: pill

    property string activity: "idle"
    property string scope: "window"
    // P@x is switched on: the pill is drawn, idle included.
    property bool on: false
    // The widest the pill may be: its words wrap beyond it.
    property real maxWidth: 10000

    readonly property string text: pill.on || pill.activity !== "idle" ? Words.pill(pill.activity, pill.scope) : ""
    readonly property bool loud: pill.activity === "acting"
    readonly property bool dashed: pill.activity === "paused"

    // The colours from the outermost line inwards, and the colours of the
    // words and of what is behind them, for the contrast test.
    readonly property var layers: pill.loud
                                  ? [Pax.PaxTheme.overlayHairline, Pax.PaxTheme.overlayOutline, Pax.PaxTheme.overlayFill]
                                  : pill.dashed ? [Pax.PaxTheme.overlayPaused, Pax.PaxTheme.overlayOutline]
                                                : [Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayOutline]
    readonly property color textColour: pill.loud ? Pax.PaxTheme.overlayText
                                                  : pill.dashed ? Pax.PaxTheme.overlayQuietText
                                                                : Pax.PaxTheme.overlayFill
    readonly property color textBackground: pill.loud ? Pax.PaxTheme.overlayFill : Pax.PaxTheme.overlayOutline
    readonly property color glyphColour: pill.dashed ? Pax.PaxTheme.overlayPaused : pill.textColour

    readonly property real padding: Kit.Theme.spaceGroup

    implicitWidth: Math.min(pill.maxWidth, glyph.implicitWidth + row.spacing + Math.ceil(metrics.advanceWidth)
                                           + 2 * pill.padding)
    implicitHeight: Math.max(Kit.Theme.targetPointer, words.implicitHeight + 2 * Kit.Theme.spaceRelated)
    width: implicitWidth
    height: implicitHeight
    visible: pill.text.length > 0

    Accessible.role: Accessible.Indicator
    Accessible.name: pill.text

    // Loud: a white hairline, then the dark outline, then gold.
    Rectangle {
        visible: pill.loud
        anchors.fill: parent
        anchors.margins: -1
        radius: Math.min(height / 2, Kit.Theme.targetPointer / 2 + 1)
        color: Pax.PaxTheme.overlayHairline
    }

    Rectangle {
        anchors.fill: parent
        radius: Math.min(height / 2, Kit.Theme.targetPointer / 2)
        color: pill.loud ? Pax.PaxTheme.overlayFill : Pax.PaxTheme.overlayOutline
        border.width: pill.dashed ? 0 : pill.loud ? 1.5 : 2
        border.color: pill.loud ? Pax.PaxTheme.overlayOutline : Pax.PaxTheme.overlayFill
    }

    // Paused: a dashed grey outline.
    Shape {
        visible: pill.dashed
        anchors.fill: parent
        antialiasing: true

        ShapePath {
            id: dash

            readonly property real r: Math.min(pill.height, Kit.Theme.targetPointer) / 2 - 1
            readonly property real w: pill.width - 1
            readonly property real h: pill.height - 1

            strokeColor: Pax.PaxTheme.overlayPaused
            strokeWidth: 2
            fillColor: "transparent"
            strokeStyle: ShapePath.DashLine
            dashPattern: [3, 2]

            PathSvg {
                path: "M " + (1 + dash.r) + " 1 H " + (dash.w - dash.r) + " A " + dash.r + " " + dash.r
                      + " 0 0 1 " + (dash.w - dash.r) + " " + dash.h + " H " + (1 + dash.r) + " A "
                      + dash.r + " " + dash.r + " 0 0 1 " + (1 + dash.r) + " 1 Z"
            }
        }
    }

    TextMetrics {
        id: metrics

        font: words.font
        text: pill.text
    }

    Row {
        id: row

        anchors.centerIn: parent
        spacing: Kit.Theme.spaceRelated

        StateGlyph {
            id: glyph

            anchors.verticalCenter: parent.verticalCenter
            kind: pill.activity
            tint: pill.glyphColour
        }

        Kit.Label {
            id: words

            objectName: "pillText"
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(Math.ceil(metrics.advanceWidth), pill.width - 2 * pill.padding - glyph.width - row.spacing)
            text: pill.text
            kind: Kit.Label.Strong
            color: pill.textColour
            wrapMode: Text.Wrap
            Accessible.ignored: true
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// GENERATED from tokens/tokens.json by tokens/gen_tokens.py. Do not edit.
// Change tokens.json, then run: python3 desktop/kit/tokens/gen_tokens.py --write
pragma Singleton

import QtQuick

QtObject {
    id: theme

    // Light or dark. It follows the desktop's preference until a program
    // assigns it; followSystem() restores that. TUR is dark unless the
    // desktop says light.
    property bool light: SystemAppearance.prefersLight

    function followSystem(): void {
        theme.light = Qt.binding(() => SystemAppearance.prefersLight)
    }

    // Multiplies every text size. 1 is the user's own font size; the
    // accessibility tests run at 2.
    property real textScale: 1.0

    // Sizes are given for a 10 pt interface font and follow the user's
    // font size from there.
    readonly property real fontScale: theme.textScale
        * (Application.font.pointSize > 0 ? Application.font.pointSize : 10) / 10

    // Colours. Blue is the system. Gold is not here: it belongs to P@x
    // and lives in the module Tur.Kit.Pax.
    // surface.base: Window background
    readonly property color surfaceBase:    theme.light ? "#F3F5F9" : "#0B0F17"
    // surface.raised: Lists, cards, text fields
    readonly property color surfaceRaised:  theme.light ? "#FFFFFF" : "#131A27"
    // surface.overlay: Menus, dialogs, tooltips, sheets
    readonly property color surfaceOverlay: theme.light ? "#FFFFFF" : "#1B2434"
    // surface.sunken: Technical detail, command text
    readonly property color surfaceSunken:  theme.light ? "#E7EBF2" : "#070A10"
    // line.subtle: Separators (decoration only)
    readonly property color lineSubtle:     theme.light ? "#D3D9E4" : "#27324A"
    // line.control: Borders of fields, switches, check boxes
    readonly property color lineControl:    theme.light ? "#6F7C93" : "#6B7A95"
    // text.primary: Body text, titles
    readonly property color textPrimary:    theme.light ? "#10151F" : "#EEF1F6"
    // text.secondary: Supporting text
    readonly property color textSecondary:  theme.light ? "#465368" : "#A9B4C6"
    // text.disabled: Unavailable controls
    readonly property color textDisabled:   theme.light ? "#8A94A6" : "#657086"
    // accent: Primary button fill, switch on, progress
    readonly property color accent:         theme.light ? "#0A68C4" : "#2AA9F8"
    // accent.on: Text and icons on accent
    readonly property color accentOn:       theme.light ? "#FFFFFF" : "#04111E"
    // accent.text: Links, focus ring
    readonly property color accentText:     theme.light ? "#0A5CB0" : "#5BBEFF"
    // selection: Selected row or text
    readonly property color selection:      theme.light ? "#0A68C4" : "#1267C4"
    // selection.on: Text on selection
    readonly property color selectionOn:    theme.light ? "#FFFFFF" : "#FFFFFF"
    // positive: Works, done
    readonly property color positive:       theme.light ? "#157347" : "#4CCB8F"
    // warning: Needs attention; a repair session
    readonly property color warning:        theme.light ? "#A34A00" : "#FF9A52"
    // negative: Failed; destroys data
    readonly property color negative:       theme.light ? "#B3261E" : "#FF7A7A"

    // Type
    readonly property string interfaceFamily: "Inter"
    readonly property string monoFamily: "JetBrains Mono"
    readonly property real lineHeight: 1.4
    readonly property int measure: 72
    readonly property real capsSpacing: 0.08
    // display: Setup, installer and trusted-screen titles
    readonly property real typeDisplaySize: 22
    readonly property int typeDisplayWeight: 600
    // title: Window and page titles
    readonly property real typeTitleSize: 15
    readonly property int typeTitleWeight: 600
    // heading: Section headings, the title of a sheet or message
    readonly property real typeHeadingSize: 12
    readonly property int typeHeadingWeight: 600
    // body: Everything else
    readonly property real typeBodySize: 10
    readonly property int typeBodyWeight: 400
    // strong: The one thing in a paragraph that must be seen
    readonly property real typeStrongSize: 10
    readonly property int typeStrongWeight: 600
    // small: Supporting lines, captions. Nothing is smaller
    readonly property real typeSmallSize: 9
    readonly property int typeSmallWeight: 400
    // caps: Group labels on the trusted screen and the destructive confirmation only (the design's Label style)
    readonly property real typeCapsSize: 9
    readonly property int typeCapsWeight: 600
    // mono: Technical detail
    readonly property real typeMonoSize: 10
    readonly property int typeMonoWeight: 400

    // Space: a 4 px grid
    readonly property int spaceGrid: 4
    readonly property int spaceRelated: 8
    readonly property int spaceGroup: 16
    readonly property int spaceMargin: 24

    // Radius
    readonly property int radiusControl: 6
    readonly property int radiusCard: 10
    readonly property int radiusSheet: 14

    // Target sizes
    readonly property int targetPointer: 36
    readonly property int targetTouch: 44
    readonly property int targetMinimum: 24

    // Lines and the focus ring
    readonly property int lineWidth: 1
    readonly property int lineFocusWidth: 2
    readonly property int lineFocusGap: 2

    // Motion: base durations in milliseconds; use the Motion singleton
    readonly property int motionControl: 120
    readonly property int motionAppear: 200
    readonly property int motionPanel: 320
    readonly property int motionReducedFade: 100
}

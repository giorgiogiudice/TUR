// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// The ground every TUR view stands on. It does three things for whatever is
// put in it:
//
//  - paints the window background (surface.base);
//  - mirrors the layout in a right-to-left language, for everything inside;
//  - gives Qt Quick Controls' own controls (a scroll bar, a combo box) TUR's
//    colours through the palette, so that a stock control under the Basic
//    style does not turn up in Qt's default grey.
T.Pane {
    id: control

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            contentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             contentHeight + topPadding + bottomPadding)
    padding: Theme.spaceMargin

    LayoutMirroring.enabled: Application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true

    palette.window: Theme.surfaceBase
    palette.windowText: Theme.textPrimary
    palette.base: Theme.surfaceRaised
    palette.alternateBase: Theme.surfaceSunken
    palette.text: Theme.textPrimary
    palette.button: Theme.surfaceRaised
    palette.buttonText: Theme.textPrimary
    palette.highlight: Theme.selection
    palette.highlightedText: Theme.selectionOn
    palette.placeholderText: Theme.textSecondary
    palette.toolTipBase: Theme.surfaceOverlay
    palette.toolTipText: Theme.textPrimary
    palette.link: Theme.accentText
    palette.mid: Theme.lineControl
    palette.midlight: Theme.lineSubtle
    palette.dark: Theme.lineControl
    palette.light: Theme.surfaceOverlay
    palette.shadow: Theme.surfaceSunken

    background: Rectangle {
        color: Theme.surfaceBase
    }
}

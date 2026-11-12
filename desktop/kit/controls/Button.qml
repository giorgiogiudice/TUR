// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// A button. Its label is a verb that completes the title above it
// (desktop-experience.md, 5.6): "Erase disk and install", not "OK".
//
// The kind chooses the look; a button has no colour property. Primary is the
// system's blue. Nothing here is gold: that is P@x's.
T.Button {
    id: control

    enum Kind {
        Secondary,
        Primary,
        Quiet,
        Destructive
    }

    // Secondary: outlined, the default. Primary: filled, one per view, the
    // action the view exists for. Quiet: no outline, for minor actions.
    // Destructive: for an action that destroys data.
    property int kind: Button.Secondary

    readonly property bool filled: control.kind === Button.Primary && control.enabled
    readonly property color foreground: {
        if (!control.enabled)
            return Theme.textDisabled
        switch (control.kind) {
        case Button.Primary:
            return Theme.accentOn
        case Button.Destructive:
            return Theme.negative
        default:
            return Theme.textPrimary
        }
    }

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Theme.spaceGroup
    verticalPadding: Theme.spaceRelated

    Accessible.role: Accessible.Button
    Accessible.name: control.text

    contentItem: Label {
        text: control.text
        color: control.foreground
        wrapMode: Text.NoWrap
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Accessible.ignored: true
    }

    background: Rectangle {
        implicitWidth: 2 * Theme.targetPointer
        implicitHeight: Theme.targetPointer
        radius: Theme.radiusControl
        color: control.filled ? Theme.accent
               : control.kind === Button.Quiet ? "transparent" : Theme.surfaceRaised
        border.width: control.filled || control.kind === Button.Quiet ? 0 : Theme.lineWidth
        border.color: !control.enabled ? Theme.lineSubtle
                      : control.kind === Button.Destructive ? Theme.negative : Theme.lineControl

        StateLayer {
            hovered: control.hovered
            pressed: control.down
            tint: control.foreground
        }

        FocusRing {
            shown: control.visualFocus
        }
    }
}

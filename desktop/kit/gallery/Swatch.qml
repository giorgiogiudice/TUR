// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One colour token: a chip, its name and its value in the current variant.
// Token names and colour values are not translated.
ColumnLayout {
    id: swatch

    required property string token
    required property color tint

    spacing: Kit.Theme.spaceGrid

    Rectangle {
        Layout.preferredWidth: 168
        Layout.preferredHeight: 40
        radius: Kit.Theme.radiusControl
        color: swatch.tint
        border.width: Kit.Theme.lineWidth
        border.color: Kit.Theme.lineControl
    }

    Kit.Label {
        text: swatch.token
        kind: Kit.Label.Small
        wrapMode: Text.NoWrap
    }

    Kit.Label {
        text: swatch.tint.toString().toUpperCase()
        kind: Kit.Label.Mono
        tone: Kit.Label.Secondary
        wrapMode: Text.NoWrap
    }
}

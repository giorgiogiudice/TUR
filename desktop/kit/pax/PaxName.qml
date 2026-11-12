// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// P@x's name with its mark, in P@x's colour: how a surface says "this is
// P@x". The name is a name; it is not translated.
RowLayout {
    spacing: Kit.Theme.spaceGrid

    Accessible.role: Accessible.StaticText
    Accessible.name: label.text

    PaxMark {
        Layout.alignment: Qt.AlignVCenter
        size: 14
    }

    Kit.Label {
        id: label

        Layout.alignment: Qt.AlignVCenter
        text: "P@x"
        kind: Kit.Label.Strong
        color: PaxTheme.pax
        wrapMode: Text.NoWrap
        Accessible.ignored: true
    }
}

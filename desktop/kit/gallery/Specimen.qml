// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// A captioned group of examples. Small things flow side by side and wrap;
// give a wide thing `width: parent.width` to put it on a line of its own.
ColumnLayout {
    id: specimen

    property string caption
    default property alias examples: flow.data

    Layout.fillWidth: true
    spacing: Kit.Theme.spaceRelated

    Kit.Label {
        Layout.fillWidth: true
        text: specimen.caption
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
    }

    Flow {
        id: flow

        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGroup
    }
}

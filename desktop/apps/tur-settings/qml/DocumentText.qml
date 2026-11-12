// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// A document shown word for word, on the sunken surface the kit keeps for
// text that is quoted and not written by the program: a licence, a notice.
// Its text is not translated and not rendered: it is the file as shipped.
Rectangle {
    id: box

    property string text

    Layout.fillWidth: true
    implicitHeight: words.implicitHeight + 2 * Kit.Theme.spaceGroup
    radius: Kit.Theme.radiusControl
    color: Kit.Theme.surfaceSunken

    Kit.Label {
        id: words

        objectName: "documentText"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Kit.Theme.spaceGroup
        text: box.text
        kind: Kit.Label.Small
        paragraph: true
    }
}

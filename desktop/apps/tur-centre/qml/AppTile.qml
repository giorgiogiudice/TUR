// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// Where an application's icon goes. The catalogue holds no icons, so the
// tile shows the first letter of the application's name and nothing that
// could be taken for a publisher's mark (desktop-experience.md, 5.4: no
// vendor's logo). Decoration: the name stands beside it.
Rectangle {
    id: tile

    // The application's name.
    property string name
    property real size: 40

    readonly property string initial: {
        const letters = Array.from(tile.name.trim())
        return letters.length > 0 ? letters[0].toUpperCase() : ""
    }

    implicitWidth: tile.size
    implicitHeight: tile.size
    radius: Kit.Theme.radiusCard
    color: Kit.Theme.surfaceSunken
    border.width: Kit.Theme.lineWidth
    border.color: Kit.Theme.lineSubtle

    Accessible.ignored: true

    Kit.Label {
        anchors.centerIn: parent
        text: tile.initial
        kind: tile.size >= 48 ? Kit.Label.Title : Kit.Label.Heading
        tone: Kit.Label.Secondary
        wrapMode: Text.NoWrap
        Accessible.ignored: true
    }
}

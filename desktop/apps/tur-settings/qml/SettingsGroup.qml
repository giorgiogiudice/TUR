// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// A titled group of rows: one section of a page, as the registry names it.
// The rows stand on a card.
//
// A group that is `collapsible` is closed until asked for: the second of a
// page's two layers, "plain words on top, technical detail for the rest"
// (desktop-experience.md, 10.2). A row in a closed group that is asked for by
// its address opens the group (expose()).
ColumnLayout {
    id: group

    // The page the group is on, and which of its sections it is.
    required property string page
    required property string section

    readonly property bool isSettingsGroup: true
    readonly property string title: qsTranslate("SettingsRegistry",
                                                Registry.sectionTitle(group.page, group.section, Registry.revision))

    property bool collapsible: false
    property alias expanded: opener.checked
    readonly property bool open: !group.collapsible || group.expanded

    default property alias rows: body.data

    // Makes the rows visible.
    function expose(): void {
        if (group.collapsible)
            group.expanded = true
    }

    Layout.fillWidth: true
    spacing: Kit.Theme.spaceRelated

    Kit.Label {
        objectName: "groupTitle"
        Layout.fillWidth: true
        visible: !group.collapsible
        text: group.title
        kind: Kit.Label.Heading
    }

    Kit.Disclosure {
        id: opener

        objectName: "groupOpener"
        Layout.fillWidth: true
        visible: group.collapsible
        text: group.title
    }

    Kit.Card {
        objectName: "groupCard"
        Layout.fillWidth: true
        visible: group.open

        ColumnLayout {
            id: body

            anchors.fill: parent
            spacing: 0
        }
    }
}

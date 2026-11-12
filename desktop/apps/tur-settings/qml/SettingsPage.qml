// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One page of settings: its title, as the registry has it, an optional
// sentence under it, and its groups of rows. A page cannot be written
// without its address (desktop-experience.md, 10.3).
ColumnLayout {
    id: page

    // "tur.appearance".
    required property string address

    readonly property bool isSettingsPage: true
    readonly property var entry: Registry.entry(page.address, Registry.revision)
    readonly property string title: page.entry.title !== undefined ? qsTranslate("SettingsRegistry", page.entry.title)
                                                                   : ""

    // One sentence about the page, under its title.
    property string summary

    default property alias groups: body.data

    // A row of the page leads to another address: the window goes there.
    signal addressRequested(string target)

    spacing: Kit.Theme.spaceGroup

    Kit.Label {
        objectName: "pageTitle"
        Layout.fillWidth: true
        text: page.title
        kind: Kit.Label.Title
    }

    Kit.Label {
        objectName: "pageSummary"
        Layout.fillWidth: true
        visible: page.summary.length > 0
        text: page.summary
        tone: Kit.Label.Secondary
        paragraph: true
    }

    ColumnLayout {
        id: body

        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGroup
    }
}

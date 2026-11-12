// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// A row that leads to a page of KDE System Settings: where it leads is the
// address its registry entry names (source.opens), so the page that shows
// the row and the registry cannot disagree about it. The button says where
// it goes before anything is opened.
SettingRow {
    id: row

    // The person asked to go where the row leads.
    signal followed(string target)

    focusTarget: button

    Kit.Button {
        id: button

        objectName: "rowOpen"
        text: qsTr("Open in System Settings")
        //: Read by a screen reader. %1 is the title of a page of KDE System Settings, such as "Colours".
        Accessible.name: qsTr("Open %1 in System Settings").arg(row.title)
        onClicked: row.followed(row.entry.opens !== undefined ? row.entry.opens : "")
    }
}

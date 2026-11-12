// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What the registries hold for the words in the search field
// (desktop-experience.md, 10.1 and 10.3): a row for each setting or page that
// matches, best first, with where it is. Choosing one goes there and
// highlights it. A page of KDE System Settings is found too, and says that
// it is there.
//
// The search is over the registries only. Most of KDE's settings are not in
// one yet (task D-20), so when nothing matches, the view says where the rest
// of the settings are and does not say that there is no such setting.
ColumnLayout {
    id: view

    // What was searched for, and what matched: address, title, score, path,
    // section, kind, program.
    required property string query
    required property var hits

    signal chosen(string address)
    signal systemSettingsRequested()

    spacing: Kit.Theme.spaceGroup

    Kit.Label {
        objectName: "searchTitle"
        Layout.fillWidth: true
        //: The title of the search results. %1 is what was typed.
        text: qsTr("Settings that match “%1”").arg(view.query.trim())
        kind: Kit.Label.Title
    }

    Kit.Label {
        objectName: "searchNothing"
        Layout.fillWidth: true
        visible: view.hits.length === 0
        text: qsTr("None of TUR's own settings matches. Displays, sound, network, printers and most other settings are in System Settings.")
        paragraph: true
    }

    Kit.Button {
        objectName: "searchOpenSystemSettings"
        visible: view.hits.length === 0
        text: qsTr("Open System Settings")
        onClicked: view.systemSettingsRequested()
    }

    Kit.Card {
        objectName: "searchHits"
        Layout.fillWidth: true
        visible: view.hits.length > 0
        padding: Kit.Theme.spaceGrid

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            Repeater {
                model: view.hits

                Kit.ListRow {
                    id: hit

                    required property var modelData

                    objectName: "searchHit-" + hit.modelData.address
                    Layout.fillWidth: true
                    text: qsTranslate("SettingsRegistry", hit.modelData.title)
                    subtitle: Words.where(hit.modelData.path, hit.modelData.section)
                    // A row of a list does not take the keyboard by itself;
                    // these are what the view is for.
                    focusPolicy: Qt.StrongFocus
                    onClicked: view.chosen(hit.modelData.address)
                }
            }
        }
    }
}

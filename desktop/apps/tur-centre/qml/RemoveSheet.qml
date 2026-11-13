// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// "Remove" asks one question (desktop-experience.md, 8.5):
//
//     Remove Adobe Photoshop?
//     ◉ Keep its settings and documents
//     ○ Remove its settings and data too
//     [Cancel]   [Remove]
//
// Keeping is chosen when the sheet opens, as tur-apps keeps them by default.
// Neither button has the keyboard when it opens: the first answer has it.
Kit.Sheet {
    id: sheet

    property string name
    // Where the application's data is, as tur-apps lists it.
    property var dataPlaces: []

    readonly property bool deleteData: removeData.checked

    signal confirmed(bool deleteData)

    parent: Controls.Overlay.overlay
    //: %1 is the application's name.
    title: qsTr("Remove %1?").arg(sheet.name)
    closePolicy: Controls.Popup.CloseOnEscape

    onAboutToShow: keepData.checked = true

    Controls.ButtonGroup {
        id: answers
    }

    ChoiceRow {
        id: keepData

        objectName: "removeKeep"
        Layout.fillWidth: true
        text: qsTr("Keep its settings and documents")
        checked: true
        focus: true
        Controls.ButtonGroup.group: answers
    }

    ChoiceRow {
        id: removeData

        objectName: "removeDelete"
        Layout.fillWidth: true
        text: qsTr("Remove its settings and data too")
        Controls.ButtonGroup.group: answers
    }

    Kit.Label {
        objectName: "removeWhere"
        Layout.fillWidth: true
        visible: sheet.dataPlaces.length > 0
        //: %1 is where an application keeps its data, for example ~/.var/app/org.example.App.
        text: qsTr("Its data is in %1.").arg(sheet.dataPlaces.join(", "))
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        paragraph: true
    }

    buttons: [
        Kit.Button {
            objectName: "removeCancel"
            text: qsTr("Cancel")
            onClicked: sheet.reject()
        },
        Kit.Button {
            objectName: "removeConfirm"
            text: qsTr("Remove")
            kind: Kit.Button.Destructive
            onClicked: {
                const deleteData = sheet.deleteData
                sheet.close()
                sheet.confirmed(deleteData)
            }
        }
    ]
}

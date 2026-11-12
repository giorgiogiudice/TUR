// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// A dialog sheet, open over a page. The page behind is dimmed and cannot be
// reached until the sheet is answered or left with Escape.
GalleryPage {
    id: page

    title: qsTr("Dialog sheet")
    focusTarget: back
    keyboardScope: sheet.sheetItem
    implicitHeight: 560

    function retire(): void {
        sheet.close()
    }

    Component.onCompleted: sheet.open()

    Specimen {
        caption: qsTr("What is behind the sheet stays visible, dimmed")

        Kit.Card {
            width: parent.width

            ColumnLayout {
                anchors.fill: parent
                spacing: Kit.Theme.spaceRelated

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Install TUR")
                    kind: Kit.Label.Heading
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("TUR will be installed on the disk you chose. Nothing has been changed yet.")
                    tone: Kit.Label.Secondary
                    paragraph: true
                }

                Kit.Button {
                    text: qsTr("Choose another disk")
                }
            }
        }
    }

    Item {
        Layout.fillHeight: true
    }

    Kit.Sheet {
        id: sheet

        title: qsTr("Erase this disk and install TUR?")

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("Everything on this disk will be removed, including Windows and the files stored on it. This cannot be undone.")
            paragraph: true
        }

        Kit.Label {
            Layout.fillWidth: true
            text: "Samsung SSD 980 PRO 1TB · nvme-eui.002538b211b0a1f4"
            kind: Kit.Label.Mono
            tone: Kit.Label.Secondary
        }

        Kit.CheckBox {
            text: qsTr("I have a copy of the files I need")
        }

        buttons: [
            Kit.Button {
                id: back

                text: qsTr("Back")
                focus: true
            },
            Kit.Button {
                text: qsTr("Erase disk and install")
                kind: Kit.Button.Destructive
            }
        ]
    }
}

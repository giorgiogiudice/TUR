// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The gallery's window: the list of pages on one side, the chosen page on
// the other. For people; the tests load the pages directly.
Window {
    id: window

    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant

    property int current: 0

    readonly property var entries: [
        { title: qsTr("Foundations"), page: foundations },
        { title: qsTr("Buttons"), page: buttons },
        { title: qsTr("Fields and choices"), page: inputs },
        { title: qsTr("Cards, lists, banners and progress"), page: containers },
        { title: qsTr("The two-layer message"), page: messages },
        { title: qsTr("Dialog sheet"), page: sheets }
    ]

    width: 1120
    height: 800
    visible: true
    title: qsTr("TUR kit gallery")
    color: Kit.Theme.surfaceBase

    Component.onCompleted: {
        if (window.variant === "light")
            Kit.Theme.light = true
        else if (window.variant === "dark")
            Kit.Theme.light = false
    }

    Component {
        id: foundations

        FoundationsPage {
        }
    }

    Component {
        id: buttons

        ButtonsPage {
        }
    }

    Component {
        id: inputs

        InputsPage {
        }
    }

    Component {
        id: containers

        ContainersPage {
        }
    }

    Component {
        id: messages

        MessagePage {
        }
    }

    Component {
        id: sheets

        SheetPage {
        }
    }

    Kit.Page {
        anchors.fill: parent
        padding: 0

        RowLayout {
            anchors.fill: parent
            spacing: 0

            ColumnLayout {
                Layout.preferredWidth: 280
                Layout.fillHeight: true
                Layout.margins: Kit.Theme.spaceRelated
                spacing: Kit.Theme.spaceGrid

                Repeater {
                    model: window.entries

                    Kit.ListRow {
                        required property var modelData
                        required property int index

                        Layout.fillWidth: true
                        text: modelData.title
                        selected: index === window.current
                        onClicked: window.current = index
                    }
                }

                Item {
                    Layout.fillHeight: true
                }

                Kit.Switch {
                    Layout.fillWidth: true
                    text: qsTr("Light")
                    checked: Kit.Theme.light
                    onToggled: Kit.Theme.light = checked
                }
            }

            Kit.Separator {
                Layout.preferredWidth: Kit.Theme.lineWidth
                Layout.fillHeight: true
            }

            Flickable {
                id: view

                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: width
                contentHeight: loader.implicitHeight

                Loader {
                    id: loader

                    width: view.width
                    sourceComponent: window.entries[window.current].page
                }
            }
        }
    }
}

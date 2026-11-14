// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A plain, well-labelled Qt Quick dialog for the vision test of the winsee
// experiment (docs/reports/exp-winsee-1.md): fields, check boxes, radio
// buttons, a drop-down list, tabs and buttons, each with a name the
// accessibility tree reports. It does nothing; linux_labels.py photographs
// it and reads where its controls are. Qt Quick and Qt Quick Controls only
// (LGPL-3.0); the Basic style, so that no GPL widget style is loaded.

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 560
    height: 440
    title: "Printer options"

    header: TabBar {
        id: tabs
        TabButton { text: "General" }
        TabButton { text: "Layout" }
        TabButton { text: "Quality" }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 18
        spacing: 10

        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 10
            Layout.fillWidth: true

            Label { text: "Printer name" }
            TextField {
                text: "Office laser"
                Layout.fillWidth: true
                Accessible.name: "Printer name"
            }
            Label { text: "Copies" }
            TextField {
                text: "1"
                Layout.preferredWidth: 80
                Accessible.name: "Copies"
            }
            Label { text: "Paper size" }
            ComboBox {
                model: ["A4", "A5", "Letter"]
                Accessible.name: "Paper size"
            }
        }

        CheckBox { text: "Print in colour"; checked: true }
        CheckBox { text: "Double sided" }

        RowLayout {
            spacing: 12
            Label { text: "Orientation" }
            RadioButton { text: "Portrait"; checked: true }
            RadioButton { text: "Landscape" }
        }

        Item { Layout.fillHeight: true }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 10
            Button { text: "Preview" }
            Button { text: "Print test page" }
            Button { text: "Help" }
            Button { text: "Close" }
        }
    }
}

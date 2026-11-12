// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Evidence for two assumptions of desktop-experience.md, section 5.7, that
// task D-01 was to settle: what stock Qt Quick controls look like under the
// style a TUR program gets inside the session (org.kde.breeze), and whether
// Kirigami takes the system's colour scheme with it. desktop/ci/probe.sh
// runs this page under several styles and colour schemes; it has no
// reference image, because what it shows is KDE's work and not TUR's.
Rectangle {
    id: page

    // Read by tur-probe and printed.
    readonly property var facts: ({
        "kirigamiBackground": Kirigami.Theme.backgroundColor.toString(),
        "kirigamiText": Kirigami.Theme.textColor.toString(),
        "kirigamiHighlight": Kirigami.Theme.highlightColor.toString(),
        "kirigamiFont": Kirigami.Theme.defaultFont.family,
        "kirigamiGridUnit": Kirigami.Units.gridUnit,
        "kirigamiLongDuration": Kirigami.Units.longDuration
    })

    width: 640
    height: 360
    color: Kirigami.Theme.backgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing * 2
        spacing: Kirigami.Units.largeSpacing

        Controls.Label {
            text: "Stock Qt Quick controls, as the session styles them"
            font.bold: true
        }

        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Controls.Button {
                text: "Button"
            }

            Controls.Button {
                text: "Highlighted"
                highlighted: true
            }

            Controls.CheckBox {
                text: "Check box"
                checked: true
            }

            Controls.Switch {
                text: "Switch"
                checked: true
            }
        }

        Controls.TextField {
            Layout.preferredWidth: 280
            placeholderText: "Text field"
        }

        Controls.ProgressBar {
            Layout.preferredWidth: 280
            value: 0.4
        }

        Item {
            Layout.fillHeight: true
        }
    }
}

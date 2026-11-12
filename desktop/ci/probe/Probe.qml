// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Shapes
import Tur.Kit as Kit

// The one page of the toolchain proof (task D-01). It uses one of each thing
// the chain must get right: the tokens, both fonts, a script that needs a
// fallback font, a vector shape, and a stock Qt Quick control. If its picture
// matches the reference, the build, the software renderer, the fonts and the
// comparison all work.
Rectangle {
    id: page

    // How many times the stock button was pressed; the test presses it.
    property int presses: 0

    width: 640
    height: 360
    color: Kit.Theme.surfaceBase

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kit.Theme.spaceMargin
        spacing: Kit.Theme.spaceGroup

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("TUR toolchain proof")
            kind: Kit.Label.Title
        }

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("If this page matches its reference image, the build, the software renderer and the fonts are the ones intended.")
            tone: Kit.Label.Secondary
        }

        RowLayout {
            spacing: Kit.Theme.spaceGroup

            Rectangle {
                objectName: "accentSwatch"
                implicitWidth: 72
                implicitHeight: 40
                radius: Kit.Theme.radiusControl
                color: Kit.Theme.accent
            }

            Rectangle {
                implicitWidth: 72
                implicitHeight: 40
                radius: Kit.Theme.radiusControl
                color: Kit.Theme.surfaceRaised
                border.width: Kit.Theme.lineWidth
                border.color: Kit.Theme.lineControl
            }

            Shape {
                implicitWidth: 40
                implicitHeight: 40
                antialiasing: true

                ShapePath {
                    strokeColor: Kit.Theme.positive
                    strokeWidth: 3
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap

                    PathAngleArc {
                        centerX: 20
                        centerY: 20
                        radiusX: 16
                        radiusY: 16
                        startAngle: -90
                        sweepAngle: 270
                    }
                }
            }

            Kit.Glyph {
                kind: Kit.Glyph.Positive
                tint: Kit.Theme.positive
                size: 24
            }
        }

        Kit.Label {
            text: "TUR-DESKTOP-PROOF-001  /dev/disk/by-uuid/0f3a-91c2"
            kind: Kit.Label.Mono
        }

        Kit.Label {
            // A script the interface font does not have: drawn by a fallback
            // font, right to left. Not for translation.
            text: "שלום עולם"
        }

        Controls.Button {
            objectName: "stockButton"
            text: qsTr("A stock control")
            onClicked: page.presses += 1
        }

        Item {
            Layout.fillHeight: true
        }
    }
}

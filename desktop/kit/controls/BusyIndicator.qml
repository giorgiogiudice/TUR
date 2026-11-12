// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import QtQuick.Templates as T

// A spinner, for real activity of unknown length and nothing else
// (desktop-experience.md, 5.5). With motion reduced it is a still ring with
// a gap: it still says "working", and nothing turns.
T.BusyIndicator {
    id: control

    // What is being waited for. The spinner's name for a screen reader.
    property string label: qsTr("Working")

    readonly property real ringSize: 24
    readonly property real ringWidth: 2.5

    implicitWidth: implicitContentWidth + leftPadding + rightPadding
    implicitHeight: implicitContentHeight + topPadding + bottomPadding

    Accessible.role: Accessible.Indicator
    Accessible.name: control.label

    contentItem: Item {
        implicitWidth: control.ringSize
        implicitHeight: control.ringSize
        visible: control.running

        Shape {
            id: ring

            anchors.fill: parent
            antialiasing: true

            ShapePath {
                strokeColor: Theme.lineSubtle
                strokeWidth: control.ringWidth
                fillColor: "transparent"

                PathAngleArc {
                    centerX: ring.width / 2
                    centerY: ring.height / 2
                    radiusX: ring.width / 2 - control.ringWidth
                    radiusY: ring.height / 2 - control.ringWidth
                    startAngle: 0
                    sweepAngle: 360
                }
            }

            ShapePath {
                strokeColor: Theme.accent
                strokeWidth: control.ringWidth
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap

                PathAngleArc {
                    centerX: ring.width / 2
                    centerY: ring.height / 2
                    radiusX: ring.width / 2 - control.ringWidth
                    radiusY: ring.height / 2 - control.ringWidth
                    startAngle: -90
                    sweepAngle: 250
                }
            }

            RotationAnimation on rotation {
                running: control.running && control.visible && Motion.loops
                from: 0
                to: 360
                duration: 900
                loops: Animation.Infinite
            }
        }
    }
}

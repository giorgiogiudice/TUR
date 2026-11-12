// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// Progress, with what is progressing written above it. Numbers appear only
// when they are real: "Copying the system image · 2.1 GiB of 5.4 GiB", never
// a percentage the program cannot compute (desktop-experience.md, 5.6). When
// the amount is not known the bar is indeterminate and says nothing about how
// far along it is.
T.ProgressBar {
    id: control

    // What is progressing. Also the bar's name for a screen reader.
    property string label

    // Real figures, or empty.
    property string detail

    readonly property real barHeight: 6

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding,
                            caption.implicitWidth)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    topPadding: caption.text.length > 0 ? caption.implicitHeight + Theme.spaceGrid : 0
    topInset: control.topPadding

    Accessible.role: Accessible.ProgressBar
    Accessible.name: control.label
    Accessible.description: control.detail

    Label {
        id: caption

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        visible: text.length > 0
        //: A progress caption. %1 is what is progressing, %2 the real figures, for example "2.1 GiB of 5.4 GiB".
        text: control.detail.length > 0 ? qsTr("%1 · %2").arg(control.label).arg(control.detail) : control.label
        tabular: true
        Accessible.ignored: true
    }

    contentItem: Item {
        id: track

        // Where the moving block of an indeterminate bar is, from 0 to 1.
        property real phase: 0

        implicitWidth: 200
        implicitHeight: control.barHeight

        // A known amount: filled from the start of the line.
        Rectangle {
            anchors.left: parent.left
            visible: !control.indeterminate
            width: control.position * track.width
            height: track.height
            radius: height / 2
            color: Theme.accent
        }

        // An unknown amount: one block that travels to and fro.
        Rectangle {
            x: (track.width - width) * track.phase
            visible: control.indeterminate && Motion.loops
            width: track.width * 0.3
            height: track.height
            radius: height / 2
            color: Theme.accent
        }

        // An unknown amount with motion reduced: three still blocks, which
        // cannot be mistaken for a bar that stopped part of the way.
        Repeater {
            model: control.indeterminate && !Motion.loops ? 3 : 0

            Rectangle {
                required property int index

                x: track.width * (0.1 + 0.3 * index)
                width: track.width * 0.2
                height: track.height
                radius: height / 2
                color: Theme.accent
            }
        }

        SequentialAnimation {
            running: control.indeterminate && control.visible && Motion.loops
            loops: Animation.Infinite

            NumberAnimation {
                target: track
                property: "phase"
                from: 0
                to: 1
                duration: 1100
                easing.type: Easing.InOutSine
            }

            NumberAnimation {
                target: track
                property: "phase"
                from: 1
                to: 0
                duration: 1100
                easing.type: Easing.InOutSine
            }
        }
    }

    background: Rectangle {
        implicitWidth: 200
        implicitHeight: control.barHeight
        radius: height / 2
        color: Theme.lineSubtle
    }
}

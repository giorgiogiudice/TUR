// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T

// A dialog sheet: a question or a short task that must be finished or left
// before anything else. It is modal on purpose, and that is the only place
// the keyboard is held (desktop-experience.md, 13.1, X1). Escape leaves it.
//
//     Sheet {
//         title: qsTr("Erase this disk?")
//         Label { text: ... }
//         buttons: [
//             Button { text: qsTr("Back"); focus: true; onClicked: ... },
//             Button { text: qsTr("Erase disk and install"); kind: Button.Destructive; onClicked: ... }
//         ]
//     }
//
// Give the safe button `focus: true`. The sheet is centred in the item it is
// declared in; for the whole window, declare it at the top.
T.Dialog {
    id: control

    default property alias content: body.data

    // The sheet's buttons, the safe one first.
    property alias buttons: buttonRow.data

    // Everything the sheet draws, for tests that walk it.
    readonly property Item sheetItem: frame.parent

    anchors.centerIn: parent
    width: Math.min(480, parent ? parent.width - 2 * Theme.spaceMargin : 480)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             contentHeight + topPadding + bottomPadding)
    padding: Theme.spaceMargin
    modal: true
    focus: true
    closePolicy: T.Popup.CloseOnEscape
    popupType: T.Popup.Item

    enter: Transition {
        NumberAnimation {
            property: "opacity"
            from: 0
            to: 1
            duration: Motion.fadePanel
            easing.type: Motion.easing
        }
    }

    exit: Transition {
        NumberAnimation {
            property: "opacity"
            from: 1
            to: 0
            duration: Motion.fadeAppear
            easing.type: Motion.easing
        }
    }

    contentItem: ColumnLayout {
        spacing: Theme.spaceGroup

        // A popup lives in the window's overlay, not in the page that
        // declares it, so it does not inherit the page's mirroring.
        LayoutMirroring.enabled: Application.layoutDirection === Qt.RightToLeft
        LayoutMirroring.childrenInherit: true

        Accessible.role: Accessible.Dialog
        Accessible.name: control.title

        Label {
            Layout.fillWidth: true
            visible: control.title.length > 0
            text: control.title
            kind: Label.Heading
        }

        ColumnLayout {
            id: body

            Layout.fillWidth: true
            spacing: Theme.spaceRelated
        }

        Flow {
            id: buttonRow

            Layout.fillWidth: true
            spacing: Theme.spaceRelated
        }
    }

    background: Rectangle {
        id: frame

        color: Theme.surfaceOverlay
        radius: Theme.radiusSheet
        border.width: Theme.lineWidth
        border.color: Theme.lineSubtle
    }

    // What is behind the sheet is dimmed, not hidden. There is no token for
    // this: it is the darkest tone of the variant, mostly transparent.
    T.Overlay.modal: Rectangle {
        readonly property color shade: Theme.light ? Theme.textPrimary : Theme.surfaceSunken

        color: Qt.rgba(shade.r, shade.g, shade.b, 0.55)
    }
}

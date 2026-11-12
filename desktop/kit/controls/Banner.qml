// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T

// One line of news at the top of a page or a group: something is done,
// needs attention or went wrong, with at most one thing to do about it. For
// an error with a code, a reason and technical detail, use Message.
//
// The severity is a sign and a word for a screen reader, not only a colour.
T.Control {
    id: control

    enum Severity {
        Info,
        Positive,
        Warning,
        Negative
    }

    property int severity: Banner.Info

    // What to tell the person: a full sentence.
    property string text

    // The label of the one action a banner may offer; empty for none.
    property string actionText

    signal triggered()

    readonly property color tone: {
        switch (control.severity) {
        case Banner.Positive:
            return Theme.positive
        case Banner.Warning:
            return Theme.warning
        case Banner.Negative:
            return Theme.negative
        default:
            return Theme.accentText
        }
    }

    readonly property string spokenText: {
        switch (control.severity) {
        case Banner.Positive:
            //: Read by a screen reader before a banner that reports success. %1 is the banner's text.
            return qsTr("Done: %1").arg(control.text)
        case Banner.Warning:
            //: Read by a screen reader before a banner that needs attention. %1 is the banner's text.
            return qsTr("Warning: %1").arg(control.text)
        case Banner.Negative:
            //: Read by a screen reader before a banner that reports a failure. %1 is the banner's text.
            return qsTr("Error: %1").arg(control.text)
        default:
            //: Read by a screen reader before a banner that only informs. %1 is the banner's text.
            return qsTr("Note: %1").arg(control.text)
        }
    }

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    horizontalPadding: Theme.spaceGroup
    verticalPadding: Theme.spaceRelated

    Accessible.role: Accessible.AlertMessage
    Accessible.name: control.spokenText

    contentItem: RowLayout {
        spacing: Theme.spaceRelated + Theme.spaceGrid

        Glyph {
            Layout.alignment: Qt.AlignVCenter
            tint: control.tone
            kind: {
                switch (control.severity) {
                case Banner.Positive:
                    return Glyph.Positive
                case Banner.Warning:
                    return Glyph.Warning
                case Banner.Negative:
                    return Glyph.Negative
                default:
                    return Glyph.Info
                }
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            text: control.text
            Accessible.ignored: true
        }

        Button {
            objectName: "bannerAction"
            Layout.alignment: Qt.AlignVCenter
            visible: control.actionText.length > 0
            text: control.actionText
            onClicked: control.triggered()
        }
    }

    background: Rectangle {
        implicitHeight: Theme.targetPointer + 2 * Theme.spaceRelated
        color: Theme.surfaceRaised
        radius: Theme.radiusCard
        border.width: Theme.lineWidth
        border.color: control.tone
    }
}

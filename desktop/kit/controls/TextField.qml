// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Templates as T

// A single-line text field with its label above it and, below it, either a
// supporting line or the reason the entry is wrong. The label is the field's
// name for a screen reader, so a field without one is a mistake.
T.TextField {
    id: control

    // The field's name: shown above it and read by a screen reader.
    property string label

    // A supporting line under the field.
    property string helperText

    // When not empty, the field is marked as wrong and this says why. It is
    // marked by a sign and by words, not by colour alone.
    property string errorText

    readonly property bool invalid: control.errorText.length > 0
    readonly property bool captioned: control.label.length > 0
    readonly property bool supported: control.invalid || control.helperText.length > 0
    readonly property real fieldPadding: Theme.spaceRelated + Theme.spaceGrid

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            placeholder.implicitWidth + leftPadding + rightPadding,
                            caption.implicitWidth)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             contentHeight + topPadding + bottomPadding)

    // Room for the focus ring between the field and the lines around it.
    readonly property real ringRoom: Theme.lineFocusGap + 2 * Theme.lineFocusWidth

    topInset: control.captioned ? caption.implicitHeight + control.ringRoom : 0
    bottomInset: control.supported ? support.implicitHeight + control.ringRoom : 0
    leftPadding: control.fieldPadding
    rightPadding: control.fieldPadding
    topPadding: control.topInset + Theme.spaceRelated
    bottomPadding: control.bottomInset + Theme.spaceRelated

    font.pointSize: Theme.typeBodySize * Theme.fontScale
    color: control.enabled ? Theme.textPrimary : Theme.textDisabled
    selectionColor: Theme.selection
    selectedTextColor: Theme.selectionOn
    placeholderTextColor: Theme.textSecondary
    verticalAlignment: TextInput.AlignVCenter
    selectByMouse: true

    Accessible.role: Accessible.EditableText
    Accessible.name: control.label.length > 0 ? control.label : control.placeholderText
    Accessible.description: control.invalid ? control.errorText : control.helperText

    Label {
        id: caption

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        visible: control.captioned
        text: control.label
        kind: Label.Small
        tone: Label.Secondary
        Accessible.ignored: true
    }

    Label {
        id: placeholder

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.leftMargin: control.leftPadding
        anchors.rightMargin: control.rightPadding
        anchors.topMargin: control.topPadding
        anchors.bottomMargin: control.bottomPadding
        // On the line the typed text will be on.
        verticalAlignment: Text.AlignVCenter
        visible: control.length === 0 && control.preeditText.length === 0
        text: control.placeholderText
        color: control.enabled ? control.placeholderTextColor : Theme.textDisabled
        wrapMode: Text.NoWrap
        Accessible.ignored: true
    }

    Item {
        id: support

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        visible: control.supported
        implicitHeight: supportLabel.implicitHeight
        height: implicitHeight

        Glyph {
            id: supportSign

            anchors.left: parent.left
            anchors.top: parent.top
            anchors.topMargin: supportLabel.halfLeading
            visible: control.invalid
            kind: Glyph.Negative
            tint: Theme.negative
            size: 14
        }

        Label {
            id: supportLabel

            anchors.left: control.invalid ? supportSign.right : parent.left
            anchors.leftMargin: control.invalid ? Theme.spaceGrid : 0
            anchors.right: parent.right
            anchors.top: parent.top
            text: control.invalid ? control.errorText : control.helperText
            kind: Label.Small
            tone: control.invalid ? Label.Negative : Label.Secondary
            paragraph: true
            Accessible.ignored: true
        }
    }

    background: Rectangle {
        implicitWidth: 240
        implicitHeight: Theme.targetPointer
        radius: Theme.radiusControl
        color: Theme.surfaceRaised
        border.width: control.invalid ? Theme.lineFocusWidth : Theme.lineWidth
        border.color: !control.enabled ? Theme.lineSubtle
                      : control.invalid ? Theme.negative : Theme.lineControl

        FocusRing {
            shown: control.activeFocus
        }
    }
}

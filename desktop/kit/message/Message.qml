// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T

// The two-layer message (specification 6.3; desktop-experience.md, 5.6).
// Every error and every notable event in every TUR program uses this one
// component, so that they read alike everywhere.
//
// First layer, always shown: what happened, why, what to do, the actions, and
// the stable code. Second layer, closed until asked for: the technical
// detail, in the fixed-width face, with a Copy button.
//
// The text comes from the code's registry entry (specification 29.3). The
// program that shows a message does not write its words.
//
//     Message {
//         code: "TUR-STORAGE-NTFS-004"
//         title: entry.title
//         reason: entry.reason
//         whatToDo: entry.action
//         technicalDetail: entry.detail
//         actions: [qsTr("Try again")]
//         onActionTriggered: index => ...
//     }
T.Control {
    id: control

    enum Severity {
        Error,
        Warning,
        Notice,
        Done
    }

    property int severity: Message.Error

    // The stable code. Never translated; always visible; can be copied.
    property string code

    // What happened, in one sentence.
    property string title

    // Why. May be empty.
    property string reason

    // What the person can do about it. May be empty. Never the unsafe thing.
    property string whatToDo

    // Command or component, exit status, device identifiers, evidence. One
    // item to a line. Shown only when the person opens it.
    property string technicalDetail

    // The labels of this message's own actions, the safe one first.
    property list<string> actions

    // Offer "Ask P@x to look into it". Set it only while P@x is on.
    property bool paxAvailable: false

    // Whether the technical detail is open.
    property alias expanded: disclosure.checked

    // Whether this program can make a support bundle. A program that cannot
    // (the tool that makes one is not there, or the program runs where there
    // is none) sets this to false, so that no button is shown that would do
    // nothing.
    property bool supportBundleAvailable: true

    // "Create a support bundle" is offered whenever the message is an error
    // and a bundle can be made.
    readonly property bool offersSupportBundle: control.severity === Message.Error && control.supportBundleAvailable

    signal actionTriggered(int index)
    signal supportBundleRequested()
    signal paxRequested()

    readonly property color tone: {
        switch (control.severity) {
        case Message.Warning:
            return Theme.warning
        case Message.Notice:
            return Theme.accentText
        case Message.Done:
            return Theme.positive
        default:
            return Theme.negative
        }
    }

    // The severity as a word, for a screen reader: it is a sign on screen,
    // and must not be a colour alone.
    readonly property string spokenTitle: {
        switch (control.severity) {
        case Message.Warning:
            //: Read by a screen reader. %1 is what happened, in one sentence.
            return qsTr("Warning: %1").arg(control.title)
        case Message.Notice:
            //: Read by a screen reader. %1 is what happened, in one sentence.
            return qsTr("Note: %1").arg(control.title)
        case Message.Done:
            //: Read by a screen reader. %1 is what happened, in one sentence.
            return qsTr("Done: %1").arg(control.title)
        default:
            //: Read by a screen reader. %1 is what happened, in one sentence.
            return qsTr("Error: %1").arg(control.title)
        }
    }

    // A message is at most 72 characters wide.
    readonly property real measure: metrics.averageCharacterWidth * Theme.measure

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(implicitBackgroundHeight + topInset + bottomInset,
                             implicitContentHeight + topPadding + bottomPadding)
    padding: Theme.spaceGroup

    Accessible.role: Accessible.AlertMessage
    Accessible.name: control.spokenTitle
    Accessible.description: control.whatToDo.length > 0 ? control.whatToDo : control.reason

    FontMetrics {
        id: metrics

        font.family: Application.font.family
        font.pointSize: Theme.typeBodySize * Theme.fontScale
    }

    contentItem: RowLayout {
        spacing: Theme.spaceRelated + Theme.spaceGrid

        Glyph {
            Layout.alignment: Qt.AlignTop
            size: 20
            tint: control.tone
            kind: {
                switch (control.severity) {
                case Message.Warning:
                    return Glyph.Warning
                case Message.Notice:
                    return Glyph.Info
                case Message.Done:
                    return Glyph.Positive
                default:
                    return Glyph.Negative
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Theme.spaceRelated + Theme.spaceGrid

            Label {
                objectName: "messageTitle"
                Layout.fillWidth: true
                Layout.maximumWidth: control.measure
                text: control.title
                kind: Label.Heading
                Accessible.ignored: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: control.reason.length > 0
                spacing: Theme.spaceGrid / 2

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Reason")
                    kind: Label.Small
                    tone: Label.Secondary
                }

                Label {
                    objectName: "messageReason"
                    Layout.fillWidth: true
                    Layout.maximumWidth: control.measure
                    text: control.reason
                    paragraph: true
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: control.whatToDo.length > 0
                spacing: Theme.spaceGrid / 2

                Label {
                    Layout.fillWidth: true
                    text: qsTr("What to do")
                    kind: Label.Small
                    tone: Label.Secondary
                }

                Label {
                    objectName: "messageWhatToDo"
                    Layout.fillWidth: true
                    Layout.maximumWidth: control.measure
                    text: control.whatToDo
                    paragraph: true
                }
            }

            Flow {
                objectName: "messageActions"
                Layout.fillWidth: true
                visible: control.actions.length > 0 || control.offersSupportBundle || control.paxAvailable
                spacing: Theme.spaceRelated

                Repeater {
                    model: control.actions

                    Button {
                        required property string modelData
                        required property int index

                        objectName: "messageAction" + index
                        text: modelData
                        kind: index === 0 ? Button.Primary : Button.Secondary
                        onClicked: control.actionTriggered(index)
                    }
                }

                Button {
                    objectName: "messageSupportBundle"
                    visible: control.offersSupportBundle
                    text: qsTr("Create a support bundle")
                    onClicked: control.supportBundleRequested()
                }

                Button {
                    objectName: "messageAskPax"
                    visible: control.paxAvailable
                    text: qsTr("Ask P@x to look into it")
                    onClicked: control.paxRequested()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                visible: control.technicalDetail.length > 0 || control.code.length > 0
                spacing: Theme.spaceRelated

                Disclosure {
                    id: disclosure

                    objectName: "messageDisclosure"
                    visible: control.technicalDetail.length > 0
                    text: qsTr("Technical detail")
                }

                Item {
                    Layout.fillWidth: true
                }

                CodeLabel {
                    id: codeLabel

                    objectName: "messageCode"
                    Layout.alignment: Qt.AlignVCenter
                    visible: control.code.length > 0
                    code: control.code
                }
            }

            Rectangle {
                objectName: "messageDetail"
                Layout.fillWidth: true
                visible: disclosure.checked && control.technicalDetail.length > 0
                implicitHeight: detailRow.implicitHeight + 2 * Theme.spaceRelated
                radius: Theme.radiusControl
                color: Theme.surfaceSunken

                RowLayout {
                    id: detailRow

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: Theme.spaceRelated
                    spacing: Theme.spaceRelated

                    TextEdit {
                        id: detailText

                        objectName: "messageDetailText"
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: Theme.spaceRelated
                        Layout.leftMargin: Theme.spaceGrid
                        text: control.technicalDetail
                        readOnly: true
                        selectByMouse: true
                        activeFocusOnTab: false
                        textFormat: TextEdit.PlainText
                        wrapMode: TextEdit.Wrap
                        font.family: Theme.monoFamily
                        font.pointSize: Theme.typeMonoSize * Theme.fontScale
                        color: Theme.textPrimary
                        selectionColor: Theme.selection
                        selectedTextColor: Theme.selectionOn

                        Accessible.role: Accessible.StaticText
                        Accessible.name: detailText.text
                        Accessible.readOnly: true
                        Accessible.multiLine: true
                    }

                    Button {
                        objectName: "messageCopy"
                        Layout.alignment: Qt.AlignTop
                        kind: Button.Quiet
                        text: qsTr("Copy")
                        //: Read by a screen reader for the Copy button beside a message's technical detail.
                        Accessible.name: qsTr("Copy the technical detail")
                        onClicked: {
                            detailText.selectAll()
                            detailText.copy()
                            detailText.deselect()
                        }
                    }
                }
            }
        }
    }

    background: Rectangle {
        color: Theme.surfaceRaised
        radius: Theme.radiusCard
        border.width: Theme.lineWidth
        border.color: Theme.lineSubtle
    }
}

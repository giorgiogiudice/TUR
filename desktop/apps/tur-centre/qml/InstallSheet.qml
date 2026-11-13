// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// The plan, before anything is done (desktop-experience.md, 8.5; task D-11):
// where the application comes from and how that is checked, what is
// downloaded and how large it is, what it may reach, how it is removed, and
// its licence. Everything in it is tur-apps' plan (`tur-apps plan --json`)
// or the catalogue's entry; the headings are the application centre's.
// Nothing is installed until "Install" is pressed.
Kit.Sheet {
    id: sheet

    // The operation whose plan is shown.
    property AppsOperation operation: null
    // The catalogue's entry and the router's option, for the licence and
    // whether the way is doubtful.
    property var entry: null
    property var option: null

    readonly property var plan: sheet.operation ? sheet.operation.plan : ({})
    readonly property var edition: Words.editionOf(sheet.option, sheet.entry)
    readonly property var downloads: sheet.plan.downloads ?? []
    readonly property var access: sheet.plan.access ?? []
    readonly property bool doubtful: !!sheet.option && sheet.option.outlook === "doubtful"
    readonly property bool chrome: !!sheet.entry && sheet.entry.id === Words.chromeId

    signal confirmed()
    signal declined()

    parent: Controls.Overlay.overlay
    width: Math.min(560, parent ? parent.width - 2 * Kit.Theme.spaceMargin : 560)
    //: The title of the sheet that shows the plan of an installation. %1 is the application's name.
    title: qsTr("Install %1?").arg(sheet.plan.application ? sheet.plan.application.name : "")
    closePolicy: Controls.Popup.CloseOnEscape
    onRejected: sheet.declined()

    component Fact: ColumnLayout {
        id: fact

        property string heading
        default property alias body: lines.data

        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGrid / 2

        Kit.Label {
            Layout.fillWidth: true
            text: fact.heading
            kind: Kit.Label.Strong
        }

        ColumnLayout {
            id: lines

            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGrid / 2
        }
    }

    Flickable {
        id: scroller

        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(facts.implicitHeight,
                                         (sheet.parent ? sheet.parent.height : 600) - 220)
        contentWidth: width
        contentHeight: facts.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Controls.ScrollBar.vertical: Controls.ScrollBar {
        }

        ColumnLayout {
            id: facts

            width: scroller.width
            spacing: Kit.Theme.spaceGroup

            Fact {
                objectName: "planSource"
                heading: qsTr("Where it comes from")

                Kit.Label {
                    objectName: "planSourceName"
                    Layout.fillWidth: true
                    text: sheet.plan.source ? sheet.plan.source.name : ""
                    paragraph: true
                }

                // tur-apps' sentence: how what is installed is checked.
                Kit.Label {
                    objectName: "planVerification"
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: sheet.plan.source && sheet.plan.source.verification ? sheet.plan.source.verification : ""
                    tone: Kit.Label.Secondary
                    paragraph: true
                }

                Kit.Label {
                    objectName: "planChromeOrigin"
                    Layout.fillWidth: true
                    visible: sheet.chrome
                    text: Words.chromeOrigin
                    paragraph: true
                }
            }

            Fact {
                objectName: "planDownloads"
                heading: qsTr("What is downloaded")

                Repeater {
                    model: sheet.downloads

                    Kit.Label {
                        id: download

                        required property var modelData

                        objectName: "planDownload"
                        Layout.fillWidth: true
                        text: download.modelData.size !== null && download.modelData.size !== undefined
                              //: %1 is what is downloaded, in tur-apps' words; %2 its size, for example 5.6 MB.
                              ? qsTr("%1 · %2").arg(download.modelData.what).arg(Words.size(download.modelData.size))
                              //: %1 is what is downloaded, in tur-apps' words.
                              : qsTr("%1 · size not known").arg(download.modelData.what)
                        paragraph: true
                    }
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: sheet.downloads.length === 0
                    text: qsTr("Nothing is downloaded: the file you chose is used.")
                    paragraph: true
                }
            }

            Fact {
                objectName: "planAccess"
                heading: qsTr("What it may reach on this computer")

                Repeater {
                    model: sheet.access

                    Kit.Label {
                        id: reach

                        required property var modelData

                        objectName: "planAccessItem"
                        Layout.fillWidth: true
                        //: %1 is a kind of thing an application can reach, for example Files; %2 tur-apps' description of it.
                        text: qsTr("%1: %2").arg(Words.accessKind(reach.modelData.kind)).arg(reach.modelData.text)
                        paragraph: true
                    }
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: sheet.plan.access_known === false
                    text: qsTr("Not known before it is installed: the source was not asked.")
                    tone: Kit.Label.Secondary
                    paragraph: true
                }
            }

            Fact {
                objectName: "planRemoval"
                heading: qsTr("How it is removed")

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("With Remove, in this application centre. Its settings and documents are kept unless you choose otherwise.")
                    paragraph: true
                }

                Kit.Label {
                    objectName: "planRemovalText"
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: sheet.plan.removal && sheet.plan.removal.text ? sheet.plan.removal.text : ""
                    tone: Kit.Label.Secondary
                    paragraph: true
                }
            }

            Fact {
                objectName: "planLicence"
                heading: qsTr("Licence")

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.licenceSentence(sheet.edition)
                    paragraph: true
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: Words.packagingSentence(sheet.edition)
                    paragraph: true
                }
            }

            Kit.Label {
                objectName: "planDoubtful"
                Layout.fillWidth: true
                visible: sheet.doubtful
                text: qsTr("If it does not work you can remove it without affecting anything else.")
                paragraph: true
            }

            // Anything else tur-apps wants said: extensions whose size is not
            // known in advance, for example.
            Repeater {
                model: sheet.plan.notes ?? []

                Kit.Label {
                    required property var modelData

                    Layout.fillWidth: true
                    text: modelData
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    paragraph: true
                }
            }
        }
    }

    buttons: [
        Kit.Button {
            objectName: "planCancel"
            text: qsTr("Cancel")
            focus: true
            onClicked: sheet.reject()
        },
        Kit.Button {
            objectName: "planInstall"
            text: sheet.doubtful ? qsTr("Install anyway") : qsTr("Install")
            kind: Kit.Button.Primary
            onClicked: {
                sheet.close()
                sheet.confirmed()
            }
        }
    ]
}

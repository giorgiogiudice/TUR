// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Sources (desktop-experience.md, 8.6), as far as tur-apps manages them: the
// Flatpak sources of this person, with a switch for the two TUR knows,
// Flathub and Fedora's own. Everything is per person: switching a source on
// or off here changes nothing for anyone else, and what was installed from a
// source stays installed when it is switched off.
//
// The design's default is stated, not assumed: Flathub is on for every
// person (8.6; ADR-0011 makes only proprietary stores opt-in). Until the
// image or the first-run setup switches it on (app-install-backend.md,
// "What TUR's default should be"), a fresh account shows it "Not added",
// and the switch here, or the plan's "Switch on Flathub", turns it on.
ColumnLayout {
    id: view

    required property Apps apps

    spacing: Kit.Theme.spaceGroup

    Component.onCompleted: view.apps.refreshSources()

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Sources")
        kind: Kit.Label.Title
    }

    Kit.Label {
        objectName: "sourcesDefault"
        Layout.fillWidth: true
        text: qsTr("TUR's choice is that Flathub is on for everyone, so that Linux applications can be installed without an administrator. Sources are switched on and off for you alone, and switching one off keeps what you installed from it.")
        paragraph: true
    }

    Kit.BusyIndicator {
        objectName: "sourcesBusy"
        visible: view.apps.sourcesStatus === Apps.Loading && view.apps.sources.length === 0
        running: visible
        label: qsTr("Listing your sources")
    }

    Loader {
        objectName: "sourcesFailure"
        Layout.fillWidth: true
        active: view.apps.sourcesFailure.code !== undefined
        visible: active

        sourceComponent: FailureMessage {
            failure: view.apps.sourcesFailure
            onRetryRequested: view.apps.refreshSources()
        }
    }

    Kit.Card {
        Layout.fillWidth: true
        visible: view.apps.sources.length > 0

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGroup

            Repeater {
                model: view.apps.sources

                ColumnLayout {
                    id: source

                    required property var modelData
                    required property int index

                    // TUR knows it: tur-apps can switch it.
                    readonly property bool known: source.modelData.carries !== undefined
                    readonly property bool changing: view.apps.sourceChanging === source.modelData.name

                    objectName: "source-" + source.modelData.name
                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceGrid

                    Kit.Separator {
                        Layout.fillWidth: true
                        visible: source.index > 0
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kit.Theme.spaceGroup

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: Kit.Theme.spaceGrid / 2

                            Kit.Label {
                                objectName: "sourceTitle"
                                Layout.fillWidth: true
                                text: source.modelData.title ?? source.modelData.name
                                kind: Kit.Label.Strong
                            }

                            Kit.Label {
                                objectName: "sourceCarries"
                                Layout.fillWidth: true
                                visible: text.length > 0
                                // tur-apps' words: what the source carries.
                                text: source.modelData.carries ?? ""
                                paragraph: true
                            }

                            Kit.Label {
                                objectName: "sourceState"
                                Layout.fillWidth: true
                                text: source.changing ? qsTr("Switching…")
                                      : source.known ? Words.sourceState(source.modelData.state)
                                      //: %1 is the state of a source: On or Off.
                                      : qsTr("%1 · added outside the application centre").arg(Words.sourceState(source.modelData.state))
                                kind: Kit.Label.Small
                                tone: Kit.Label.Secondary
                            }

                            Kit.Label {
                                objectName: "sourceLocation"
                                Layout.fillWidth: true
                                visible: text.length > 0
                                text: source.modelData.location ?? ""
                                kind: Kit.Label.Mono
                                tone: Kit.Label.Secondary
                                wrapMode: Text.WrapAnywhere
                            }
                        }

                        Kit.Switch {
                            objectName: "sourceSwitch"
                            Layout.alignment: Qt.AlignTop
                            visible: source.known
                            enabled: view.apps.sourceChanging.length === 0
                            checked: source.modelData.state === "on"
                            text: ""
                            // The switch's state is read out with it.
                            Accessible.name: source.modelData.title ?? source.modelData.name
                            onToggled: view.apps.setSource(source.modelData.name, checked)
                        }
                    }
                }
            }
        }
    }

    Kit.Label {
        objectName: "sourcesStores"
        Layout.fillWidth: true
        text: qsTr("Stores such as Steam and Google Play, and setting up Windows and Android, are not here yet.")
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        paragraph: true
    }
}

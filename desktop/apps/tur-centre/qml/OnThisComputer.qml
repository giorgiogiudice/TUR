// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// "On this computer" (desktop-experience.md, 8.4): what tur-apps lists for an
// installed application: the version, how it runs, where it came from, the
// space it takes, where its data is, whether an update is known; with Update
// and Remove. An application installed with flatpak or tur-runtime directly
// is listed, and is not tur-apps' to update or remove.
Kit.Card {
    id: card

    // One application of `tur-apps list --json`.
    required property var installed
    // Something is under way for it: the buttons wait.
    property bool busy: false

    readonly property bool ours: card.installed.installed_by === "tur-apps"
    readonly property var update: card.installed.update ?? ({})

    signal updateRequested()
    signal removeRequested()

    ColumnLayout {
        anchors.fill: parent
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("On this computer")
            kind: Kit.Label.Heading
        }

        Kit.Label {
            objectName: "installedMissing"
            Layout.fillWidth: true
            visible: card.installed.state === "missing"
            text: qsTr("TUR recorded this application as installed, and it is not there any more.")
            tone: Kit.Label.Warning
            paragraph: true
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Kit.Theme.spaceGroup
            rowSpacing: Kit.Theme.spaceGrid

            Kit.Label {
                text: qsTr("Runs as")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedRoute"
                Layout.fillWidth: true
                text: Words.routeLabel(card.installed.route ?? "", "")
            }

            Kit.Label {
                text: qsTr("Version")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedVersion"
                Layout.fillWidth: true
                text: card.installed.version ?? qsTr("Not known")
            }

            Kit.Label {
                text: qsTr("From")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedFrom"
                Layout.fillWidth: true
                // tur-apps' words: "Flatpak remote flathub".
                text: card.installed.from ?? ""
                paragraph: true
            }

            Kit.Label {
                text: qsTr("Space")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedSize"
                Layout.fillWidth: true
                text: card.installed.size !== null && card.installed.size !== undefined
                      ? Words.size(card.installed.size) : qsTr("Not known")
            }

            Kit.Label {
                text: qsTr("Its data")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedData"
                Layout.fillWidth: true
                text: (card.installed.data ?? []).join("\n")
                kind: Kit.Label.Mono
                wrapMode: Text.WrapAnywhere
            }

            Kit.Label {
                text: qsTr("Updates")
                tone: Kit.Label.Secondary
            }
            Kit.Label {
                objectName: "installedUpdate"
                Layout.fillWidth: true
                text: Words.updateState(card.update)
                paragraph: true
            }
        }

        Kit.Label {
            objectName: "installedElsewhere"
            Layout.fillWidth: true
            visible: !card.ours
            text: qsTr("Installed outside the application centre. It is listed here; it is updated and removed with the program that installed it.")
            tone: Kit.Label.Secondary
            paragraph: true
        }

        Flow {
            Layout.fillWidth: true
            visible: card.ours
            spacing: Kit.Theme.spaceRelated

            Kit.Button {
                objectName: "installedUpdateButton"
                visible: card.update.available === true
                enabled: !card.busy
                text: qsTr("Update")
                onClicked: card.updateRequested()
            }

            Kit.Button {
                objectName: "installedRemoveButton"
                enabled: !card.busy
                text: qsTr("Remove…")
                onClicked: card.removeRequested()
            }
        }
    }
}

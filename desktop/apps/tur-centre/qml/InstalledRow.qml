// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One installed application, in Installed and in Updates
// (desktop-experience.md, 8.1 and 8.5): its name, how it runs, where it came
// from, its version and size, whether an update is known, and what is under
// way for it.
ColumnLayout {
    id: row

    // One application of `tur-apps list --json`.
    required property var application
    property AppsOperation operation: null
    // Updates: the button is Update.
    property bool offerUpdate: false

    readonly property bool ours: row.application.installed_by === "tur-apps"
    readonly property bool working: row.operation !== null
                                    && (row.operation.phase === AppsOperation.Running
                                        || row.operation.phase === AppsOperation.Planning)

    signal detailsRequested()
    signal openRequested()
    signal updateRequested()

    spacing: Kit.Theme.spaceGrid

    RowLayout {
        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGroup

        AppTile {
            Layout.alignment: Qt.AlignTop
            name: row.application.name ?? ""
            size: 40
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGrid / 2

            Kit.Label {
                objectName: "installedRowName"
                Layout.fillWidth: true
                text: row.application.name ?? ""
                kind: Kit.Label.Strong
            }

            Kit.Label {
                objectName: "installedRowRoute"
                Layout.fillWidth: true
                text: Words.routeLabel(row.application.route ?? "", "")
            }

            Kit.Label {
                objectName: "installedRowFacts"
                Layout.fillWidth: true
                text: {
                    const parts = [row.application.from ?? ""]
                    if (row.application.version)
                        //: %1 is a version of an application.
                        parts.push(qsTr("version %1").arg(row.application.version))
                    if (row.application.size !== null && row.application.size !== undefined)
                        parts.push(Words.size(row.application.size))
                    return parts.filter(part => part.length > 0).join(" · ")
                }
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                paragraph: true
            }

            Kit.Label {
                objectName: "installedRowUpdate"
                Layout.fillWidth: true
                text: row.application.state === "missing"
                      ? qsTr("Recorded as installed, and not there any more")
                      : Words.updateState(row.application.update)
                kind: Kit.Label.Small
                tone: row.application.state === "missing" ? Kit.Label.Warning
                      : row.application.update && row.application.update.available === true
                        ? Kit.Label.Accent : Kit.Label.Secondary
            }

            Kit.Label {
                objectName: "installedRowElsewhere"
                Layout.fillWidth: true
                visible: !row.ours
                text: qsTr("Installed outside the application centre")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
            }
        }

        Kit.Button {
            objectName: "installedRowUpdateButton"
            Layout.alignment: Qt.AlignTop
            visible: row.offerUpdate && row.ours && !row.working && !!row.application.update
                     && row.application.update.available === true
            text: qsTr("Update")
            onClicked: row.updateRequested()
        }

        Kit.Button {
            objectName: "installedRowOpen"
            Layout.alignment: Qt.AlignTop
            visible: !row.offerUpdate && row.ours && row.application.state === "installed"
            text: qsTr("Open")
            onClicked: row.openRequested()
        }

        Kit.Button {
            objectName: "installedRowDetails"
            Layout.alignment: Qt.AlignTop
            visible: row.ours
            text: qsTr("Details")
            kind: Kit.Button.Quiet
            Accessible.name: qsTr("Details of %1").arg(row.application.name ?? "")
            onClicked: row.detailsRequested()
        }
    }

    OperationBlock {
        objectName: "installedRowOperation"
        Layout.fillWidth: true
        operation: row.operation
        name: row.application.name ?? ""
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Installed (desktop-experience.md, 8.1): everything installed for this
// person, whatever it runs as: tur-apps' one list across routes, with how
// each runs, its update state and its size.
ColumnLayout {
    id: view

    required property Apps apps

    signal applicationRequested(string applicationId)

    spacing: Kit.Theme.spaceGroup

    Component.onCompleted: view.apps.refresh(view.apps.updatesChecked)

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Installed")
        kind: Kit.Label.Title
    }

    Kit.BusyIndicator {
        objectName: "installedBusy"
        visible: view.apps.installedStatus === Apps.Loading && view.apps.installed.length === 0
        running: visible
        label: qsTr("Listing what is installed")
    }

    Loader {
        objectName: "installedFailure"
        Layout.fillWidth: true
        active: view.apps.installedStatus === Apps.Failed
        visible: active

        sourceComponent: FailureMessage {
            failure: view.apps.installedFailure
            onRetryRequested: view.apps.refresh(view.apps.updatesChecked)
        }
    }

    Kit.Label {
        objectName: "installedNothing"
        Layout.fillWidth: true
        visible: view.apps.installedStatus === Apps.Ready && view.apps.installed.length === 0
        text: qsTr("Nothing is installed for you yet. Applications you install here are listed here, whatever they run as.")
        paragraph: true
    }

    Kit.Card {
        Layout.fillWidth: true
        visible: view.apps.installed.length > 0

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGroup

            Repeater {
                model: view.apps.installed

                ColumnLayout {
                    id: entry

                    required property var modelData
                    required property int index

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceGroup

                    Kit.Separator {
                        Layout.fillWidth: true
                        visible: entry.index > 0
                    }

                    InstalledRow {
                        objectName: "installedRow-" + entry.modelData.id
                        Layout.fillWidth: true
                        application: entry.modelData
                        operation: view.apps.operations[entry.modelData.id] ?? null
                        onDetailsRequested: view.applicationRequested(entry.modelData.id)
                        onOpenRequested: view.apps.launch(entry.modelData.id)
                    }
                }
            }
        }
    }

    // What tur-apps could not find out, in its words.
    Repeater {
        model: view.apps.installedProblems

        Kit.Label {
            required property var modelData

            Layout.fillWidth: true
            text: modelData
            kind: Kit.Label.Small
            tone: Kit.Label.Warning
            paragraph: true
        }
    }
}

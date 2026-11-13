// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Updates (desktop-experience.md, 8.5): the application updates the sources
// know of, grouped by where each application came from, with "Update" for
// one and "Update all". The system itself is updated in Settings, which is
// its one owner (11.1); the view says so and does not manage it.
ColumnLayout {
    id: view

    required property Apps apps

    signal applicationRequested(string applicationId)

    // The applications the list says have an update, and those being
    // updated.
    readonly property var pending: {
        const out = []
        for (const application of view.apps.installed) {
            const operation = view.apps.operations[application.id]
            const updating = operation && operation.kind === AppsOperation.Update
            if ((application.update && application.update.available === true) || updating)
                out.push(application)
        }
        return out
    }
    // Where they came from, in the order of the list.
    readonly property var groups: {
        const out = []
        for (const application of view.pending) {
            if (out.indexOf(application.from) < 0)
                out.push(application.from)
        }
        return out
    }
    readonly property bool checking: view.apps.installedStatus === Apps.Loading

    spacing: Kit.Theme.spaceGroup

    // The sources are asked when the view opens.
    Component.onCompleted: view.apps.refresh(true)

    RowLayout {
        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGroup

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("Updates")
            kind: Kit.Label.Title
        }

        Kit.Button {
            objectName: "updatesAll"
            visible: view.pending.length > 1
            enabled: !view.apps.busy
            text: qsTr("Update all")
            kind: Kit.Button.Primary
            onClicked: view.apps.updateAll()
        }
    }

    Kit.Banner {
        objectName: "updatesSystem"
        Layout.fillWidth: true
        severity: Kit.Banner.Info
        text: qsTr("TUR itself is updated in Settings, not here.")
    }

    Kit.BusyIndicator {
        objectName: "updatesChecking"
        visible: view.checking && view.pending.length === 0
        running: visible
        label: qsTr("Asking the sources for updates")
    }

    Loader {
        objectName: "updatesFailure"
        Layout.fillWidth: true
        active: view.apps.installedStatus === Apps.Failed
        visible: active

        sourceComponent: FailureMessage {
            failure: view.apps.installedFailure
            onRetryRequested: view.apps.refresh(true)
        }
    }

    Kit.Label {
        objectName: "updatesNone"
        Layout.fillWidth: true
        visible: view.apps.installedStatus === Apps.Ready && view.apps.updatesChecked && view.pending.length === 0
        text: view.apps.installed.length > 0 ? qsTr("Everything is up to date.")
                                             : qsTr("Nothing is installed for you, so there is nothing to update.")
        paragraph: true
    }

    Repeater {
        model: view.groups

        ColumnLayout {
            id: group

            required property var modelData

            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated

            Kit.Label {
                objectName: "updatesGroup"
                Layout.fillWidth: true
                // tur-apps' words for where they came from.
                text: group.modelData
                kind: Kit.Label.Heading
            }

            Kit.Card {
                Layout.fillWidth: true

                ColumnLayout {
                    anchors.fill: parent
                    spacing: Kit.Theme.spaceGroup

                    Repeater {
                        model: view.pending.filter(application => application.from === group.modelData)

                        InstalledRow {
                            required property var modelData

                            objectName: "updatesRow-" + modelData.id
                            Layout.fillWidth: true
                            application: modelData
                            offerUpdate: true
                            operation: view.apps.operations[modelData.id] ?? null
                            onUpdateRequested: view.apps.update(modelData.id)
                            onDetailsRequested: view.applicationRequested(modelData.id)
                        }
                    }
                }
            }
        }
    }

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

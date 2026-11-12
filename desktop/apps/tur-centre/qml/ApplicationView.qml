// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One application's page (desktop-experience.md, sections 8.4 and 8.8).
//
// States: being fetched; could not be fetched (the two-layer message, with
// the catalogue's code); a page with "How it will run"; and, when the router
// states that nothing runs the application on this machine, the "no route"
// page.
ColumnLayout {
    id: view

    required property ApplicationSession application

    // Opens a page in the browser.
    signal urlRequested(string url)

    readonly property bool ready: view.application.status === ApplicationSession.Ready
    readonly property var entry: view.application.entry
    readonly property var recommended: view.application.recommended
    readonly property string primaryLabel: view.ready
                                           ? Words.primaryActionLabel(view.recommended, view.entry) : ""

    spacing: Kit.Theme.spaceGroup

    Kit.BusyIndicator {
        objectName: "applicationBusy"
        visible: view.application.status === ApplicationSession.Loading
        running: visible
        label: qsTr("Asking the catalogue about this application")
    }

    Loader {
        objectName: "applicationFailure"
        Layout.fillWidth: true
        active: view.application.status === ApplicationSession.Failed
        visible: active

        sourceComponent: FailureMessage {
            failure: view.application.failure
            onRetryRequested: view.application.retry()
        }
    }

    // The application: its name, who publishes it, what kind it is, and the
    // button that says what will happen next.
    Loader {
        Layout.fillWidth: true
        active: view.ready
        visible: active

        sourceComponent: RowLayout {
            spacing: Kit.Theme.spaceGroup

            AppTile {
                Layout.alignment: Qt.AlignTop
                name: view.entry.name
                size: 56
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    objectName: "applicationName"
                    Layout.fillWidth: true
                    text: view.entry.name
                    kind: Kit.Label.Title
                }

                Kit.Label {
                    objectName: "applicationPublisher"
                    Layout.fillWidth: true
                    //: Under an application's name. %1 is its publisher, %2 its category, for example Productivity.
                    text: qsTr("%1 · %2").arg(view.entry.publisher).arg(Words.categoryLabel(view.entry.category))
                    tone: Kit.Label.Secondary
                }
            }

            // It says what will happen next, and is not there at all when
            // there is no route. Part 1 installs nothing, so it is not
            // available yet; the line under the header says so.
            Kit.Button {
                objectName: "applicationPrimary"
                Layout.alignment: Qt.AlignTop
                visible: view.primaryLabel.length > 0
                text: view.primaryLabel
                kind: Kit.Button.Primary
                enabled: false
            }
        }
    }

    Kit.Label {
        objectName: "applicationNotYet"
        Layout.fillWidth: true
        visible: view.primaryLabel.length > 0
        text: qsTr("The application centre cannot install applications yet.")
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
    }

    // The router was not told what this computer has.
    Kit.Banner {
        objectName: "applicationHostAssumed"
        Layout.fillWidth: true
        visible: view.ready && view.application.hostAssumed
        severity: Kit.Banner.Warning
        text: qsTr("TUR has not checked what this computer has. How this application will run here is an assumption, and may turn out differently.")
    }

    Loader {
        objectName: "applicationBody"
        Layout.fillWidth: true
        active: view.ready
        visible: active

        sourceComponent: view.application.noRoute ? noRoute : routes
    }

    Component {
        id: routes

        ApplicationRoutes {
            application: view.application
        }
    }

    Component {
        id: noRoute

        NoRoute {
            application: view.application
            onUrlRequested: url => view.urlRequested(url)
        }
    }

    // About this application: what the catalogue says it is.
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.ready && !!view.entry.summary
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("About this application")
            kind: Kit.Label.Heading
        }

        Kit.Label {
            objectName: "applicationSummary"
            Layout.fillWidth: true
            text: view.ready && view.entry.summary ? view.entry.summary : ""
            paragraph: true
        }
    }

    Loader {
        Layout.fillWidth: true
        // The "no route" page keeps its evidence in its own last row.
        active: view.ready && !view.application.noRoute
        visible: active

        sourceComponent: ColumnLayout {
            spacing: Kit.Theme.spaceRelated

            Kit.Disclosure {
                id: details

                objectName: "applicationDetails"
                text: qsTr("Details")
            }

            Facts {
                Layout.fillWidth: true
                Layout.leftMargin: Kit.Theme.spaceGroup
                visible: details.expanded
                entry: view.entry
                onUrlRequested: url => view.urlRequested(url)
            }
        }
    }
}

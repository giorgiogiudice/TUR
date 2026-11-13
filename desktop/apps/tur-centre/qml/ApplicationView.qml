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

    // Installing, updating, removing (task D-11): tur-apps. Without it the
    // page reads and does nothing.
    property Apps apps: null

    // Opens a page in the browser.
    signal urlRequested(string url)

    readonly property bool ready: view.application.status === ApplicationSession.Ready
    readonly property var entry: view.application.entry
    readonly property var recommended: view.application.recommended
    readonly property string applicationId: view.application.applicationId

    // What tur-apps lists for this application, or an empty map.
    readonly property var installedEntry: view.apps !== null && view.apps.installed.length >= 0
                                          ? view.apps.installedEntry(view.applicationId) : ({})
    readonly property bool installed: view.installedEntry.state === "installed"
    // What is under way, or has just ended, for this application.
    readonly property AppsOperation operation: view.apps !== null && view.apps.operations[view.applicationId]
                                               ? view.apps.operations[view.applicationId] : null
    readonly property bool working: view.operation !== null
                                    && (view.operation.phase === AppsOperation.Planning
                                        || view.operation.phase === AppsOperation.Running)

    readonly property string action: view.ready ? Words.primaryAction(view.recommended, view.entry) : ""
    // tur-apps installs by two routes; the button offers no other.
    readonly property bool canInstall: view.apps !== null && view.recommended
                                       && (view.action === "install" || view.action === "install-anyway")
                                       && Words.installable(view.recommended.route)
    readonly property string primaryLabel: !view.ready ? ""
                                           : view.installed ? qsTr("Open")
                                           : Words.primaryActionLabel(view.recommended, view.entry)
    // Why the button cannot be pressed, when it cannot.
    readonly property string notYet: {
        if (!view.ready || view.installed || view.primaryLabel.length === 0)
            return ""
        switch (view.action) {
        case "set-up":
            return qsTr("The application centre cannot set up what is needed first yet.")
        case "open-store":
            return qsTr("The application centre cannot open stores yet.")
        case "publisher-page":
            return ""
        }
        if (view.apps === null)
            return qsTr("The application centre cannot install applications here.")
        return view.canInstall ? "" : qsTr("The application centre cannot yet install applications that run this way.")
    }
    // A Windows application: the person may have its installer already.
    readonly property bool takesAFile: view.ready && !view.installed && view.apps !== null && !!view.recommended
                                       && view.recommended.route === "wine"

    // The failure of the last attempt to open the application.
    property var launchFailure: ({})

    // Starts the plan of an installation; its sheet opens when it is made.
    function install(file: string): void {
        view.launchFailure = {}
        view.apps.plan(view.applicationId, file)
    }

    // "I have the installer file": the same flow, from the hash check on.
    function chooseInstaller(path: string): void {
        if (path.length > 0)
            view.install(path)
    }

    function primaryClicked(): void {
        if (view.installed) {
            view.launchFailure = view.apps.launch(view.applicationId)
            return
        }
        if (view.action === "publisher-page") {
            const edition = Words.editionOf(view.recommended, view.entry)
            if (edition && edition.download_page)
                view.urlRequested(edition.download_page)
            return
        }
        if (view.canInstall)
            view.install("")
    }

    // The two sheets, for the tests and the pictures.
    property alias planSheet: installSheet
    property alias removalSheet: removeSheet

    function openRemoveQuestion(): void {
        removeSheet.open()
    }

    onApplicationIdChanged: view.launchFailure = {}
    // A plan made before the page was shown (Home's "Install Google
    // Chrome…") opens its sheet as soon as both are there.
    onOperationChanged: Qt.callLater(view.showPlan)
    Component.onCompleted: Qt.callLater(view.showPlan)

    function showPlan(): void {
        if (view.operation !== null && view.operation.phase === AppsOperation.Planned && !installSheet.opened)
            installSheet.open()
    }

    spacing: Kit.Theme.spaceGroup

    Connections {
        target: view.operation
        ignoreUnknownSignals: true

        function onChanged() {
            view.showPlan()
        }
    }

    InstallSheet {
        id: installSheet

        objectName: "installSheet"
        operation: view.operation
        entry: view.entry
        option: view.recommended
        onConfirmed: view.operation.confirm()
        onDeclined: {
            if (view.operation && view.operation.phase === AppsOperation.Planned)
                view.operation.dismiss()
        }
    }

    RemoveSheet {
        id: removeSheet

        objectName: "removeSheet"
        name: view.ready ? view.entry.name : ""
        dataPlaces: view.installedEntry.data ?? []
        onConfirmed: deleteData => view.apps.remove(view.applicationId, deleteData)
    }

    // The file chooser, made only when it is asked for.
    Loader {
        id: chooser

        // It takes no room in the page: what it loads is a dialog.
        visible: false
        active: false
        source: Qt.resolvedUrl("InstallerDialog.qml")
        onLoaded: {
            const dialog = chooser.item as InstallerDialog
            dialog.chosen.connect(view.chooseInstaller)
            dialog.open()
        }
    }

    // "I have the installer file", or "Choose the installer…".
    function openChooser(): void {
        if (chooser.active)
            (chooser.item as InstallerDialog).open()
        else
            chooser.active = true
    }

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
            // there is no route. Progress replaces it while something is
            // under way (8.5); once installed it is "Open".
            Kit.Button {
                objectName: "applicationPrimary"
                Layout.alignment: Qt.AlignTop
                visible: view.primaryLabel.length > 0 && !view.working
                text: view.primaryLabel
                kind: Kit.Button.Primary
                enabled: view.installed || view.action === "publisher-page" || view.canInstall
                onClicked: view.primaryClicked()
            }
        }
    }

    Kit.Label {
        objectName: "applicationNotYet"
        Layout.fillWidth: true
        visible: view.notYet.length > 0
        text: view.notYet
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
    }

    // Chrome, offered to people coming from Windows: where it comes from,
    // said plainly before anything is pressed (17.1, answer 4).
    Kit.Banner {
        objectName: "applicationChromeOrigin"
        Layout.fillWidth: true
        visible: view.ready && view.applicationId === Words.chromeId && !view.installed
        severity: Kit.Banner.Info
        text: Words.chromeOrigin
    }

    Kit.Button {
        objectName: "applicationInstallerFile"
        visible: view.takesAFile && !view.working
        text: qsTr("I have the installer file")
        kind: Kit.Button.Quiet
        onClicked: view.openChooser()
    }

    OperationBlock {
        objectName: "applicationOperation"
        Layout.fillWidth: true
        operation: view.operation
        name: view.ready ? view.entry.name : ""
        // Switched on, then the plan is made again.
        onSwitchOnRequested: source => view.apps.setSource(source, true, view.applicationId,
                                                           view.operation ? view.operation.installerFile : "")
        onInstallerRequested: view.openChooser()
    }

    Loader {
        objectName: "applicationLaunchFailure"
        Layout.fillWidth: true
        active: view.launchFailure.code !== undefined
        visible: active

        sourceComponent: FailureMessage {
            failure: view.launchFailure
            onRetryRequested: view.primaryClicked()
        }
    }

    // On this computer (8.4): what tur-apps lists, with Update and Remove.
    OnThisComputer {
        objectName: "applicationOnThisComputer"
        Layout.fillWidth: true
        visible: view.ready && view.installedEntry.id !== undefined
        installed: view.installedEntry
        busy: view.working
        onUpdateRequested: view.apps.update(view.applicationId)
        onRemoveRequested: removeSheet.open()
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

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import Tur.Centre
import Tur.Kit.Testing as Testing

// Puts the application centre's shell on a stage for the tests: a Catalogue
// that answers from a set of fixtures (recorded answers of tur-catalogue), or
// from a catalogue program that is not there; an Apps that answers from
// recorded runs of tur-apps (fixtures/apps/<scenario>); and a Shell on top.
//
// `centreFixtures` and `centreHelper` are set by tst_main.cpp: the fixtures
// directory of the source tree, and the stand-in program of unit/helper.cpp.
Item {
    id: host

    // The states every walk goes through and every picture is taken of.
    //
    //   name     the name of the state, and of its pictures
    //   set      the catalogue's fixture set; empty for "no catalogue program"
    //   apps     the recorded runs of tur-apps (fixtures/apps/<apps>);
    //            "install" when not given
    //   address  where the shell is sent
    //   open     the sections to open in place, by objectName
    //   act      what is done on the page once it is there (perform())
    //   sheet    the sheet the state shows, by objectName: its picture is
    //            the sheet's
    readonly property var states: [
        { name: "home", set: "default", address: "tur-centre:home", open: [] },
        { name: "search-results", set: "default", address: "tur-centre:search?q=office", open: [] },
        { name: "search-nothing-found", set: "default", address: "tur-centre:search?q=photothing", open: [] },
        { name: "search-failed", set: "", address: "tur-centre:search?q=office", open: [] },
        { name: "application-microsoft-365", set: "default", address: "tur-centre:app/com.microsoft.office",
          open: ["applicationDetails"] },
        { name: "application-krita", set: "default", address: "tur-centre:app/org.kde.krita", open: [] },
        { name: "application-elden-ring", set: "default", address: "tur-centre:app/com.bandainamco.eldenring",
          open: ["wayAllReports"] },
        { name: "application-netflix-assumed-host", set: "assumed-host", address: "tur-centre:app/com.netflix.netflix",
          open: [] },
        { name: "application-unknown", set: "default", address: "tur-centre:app/org.example.missing", open: [] },
        { name: "no-route-valorant", set: "default", address: "tur-centre:app/com.riotgames.valorant",
          open: ["noRouteChecked", "noRouteDetail"] },
        { name: "no-route-banking", set: "default", address: "tur-centre:app/tur.class.android-banking", open: [] },
        // Installing (D-11), on recorded runs of the real tur-apps.
        { name: "application-chrome", set: "default", address: "tur-centre:app/com.google.chrome", open: [] },
        { name: "install-plan", set: "default", apps: "install", address: "tur-centre:app/net.sourceforge.chromium-bsu",
          open: [], act: "plan", sheet: "installSheet" },
        { name: "install-progress", set: "default", apps: "install",
          address: "tur-centre:app/net.sourceforge.chromium-bsu", open: [], act: "installing" },
        { name: "install-done", set: "default", apps: "install", address: "tur-centre:app/net.sourceforge.chromium-bsu",
          open: [], act: "install" },
        { name: "install-failed", set: "default", apps: "failure", address: "tur-centre:app/net.sourceforge.chromium-bsu",
          open: [], act: "install" },
        { name: "install-cancelled", set: "default", apps: "cancel",
          address: "tur-centre:app/net.sourceforge.chromium-bsu", open: [], act: "cancel" },
        { name: "install-source-off", set: "default", apps: "sources",
          address: "tur-centre:app/net.sourceforge.chromium-bsu", open: [], act: "plan" },
        { name: "remove-question", set: "default", apps: "remove", address: "tur-centre:app/net.sourceforge.chromium-bsu",
          open: [], act: "remove-question", sheet: "removeSheet" },
        { name: "installed", set: "default", apps: "remove", address: "tur-centre:installed", open: [] },
        { name: "updates", set: "default", apps: "update", address: "tur-centre:updates", open: [] },
        { name: "sources", set: "default", apps: "sources", address: "tur-centre:sources", open: [] }
    ]

    property Item stage: null
    readonly property Shell shell: host.stage !== null ? host.stage.shell : null
    readonly property Catalogue catalogue: host.stage !== null ? host.stage.catalogue : null
    readonly property Apps apps: host.stage !== null ? host.stage.apps : null

    // The state entered, and how far its act has come.
    property var current: null
    property bool actDone: true

    // Nothing is awaited: every answer that was asked for has arrived, and
    // the state's act is done.
    readonly property bool settled: host.catalogue !== null && host.catalogue.search.settled
                                    && host.catalogue.application.status !== ApplicationSession.Loading
                                    && !host.catalogue.checkingStatistics
                                    && host.apps !== null && host.apps.installedStatus !== Apps.Loading
                                    && host.apps.sourcesStatus !== Apps.Loading
                                    && host.apps.sourceChanging === ""
                                    && host.actDone

    Component {
        id: stageComponent

        Item {
            id: stageRoot

            property string fixture
            property string appsFixture
            property string program: "tur-catalogue"
            property int timeout: 5000
            property alias catalogue: stageCatalogue
            property alias apps: stageApps
            property alias shell: stageShell

            anchors.fill: parent

            Catalogue {
                id: stageCatalogue

                fixture: stageRoot.fixture
                program: stageRoot.program
                timeout: stageRoot.timeout
            }

            Apps {
                id: stageApps

                fixture: stageRoot.appsFixture
            }

            Shell {
                id: stageShell

                anchors.fill: parent
                catalogue: stageCatalogue
                apps: stageApps
                // The tests do not wait for a pause in the typing unless
                // the pause is what they test.
                debounce: 0
            }
        }
    }

    function appsDirectory(scenario) {
        return centreFixtures + "/apps/" + (scenario && scenario.length > 0 ? scenario : "install")
    }

    // Starts a shell on a fixture set and sends it to an address. With
    // `set` empty there is no catalogue program at all. `apps` names the
    // recorded runs of tur-apps. Returns the shell.
    function start(set, address, apps) {
        const properties = set.length > 0 ? { fixture: centreFixtures + "/" + set }
                                          : { program: "/nonexistent/tur-catalogue" }
        properties.appsFixture = host.appsDirectory(apps)
        return host.startWith(properties, address)
    }

    // Starts a shell on the stand-in program, which misbehaves on request.
    function startOnHelper(address, timeout) {
        return host.startWith({ program: centreHelper, timeout: timeout, appsFixture: host.appsDirectory("") }, address)
    }

    function startWith(properties, address) {
        host.stop()
        if (properties.appsFixture === undefined)
            properties.appsFixture = host.appsDirectory("")
        host.stage = stageComponent.createObject(host, properties)
        if (host.stage === null)
            throw new Error("the stage could not be created")
        if (address.length > 0 && !host.shell.open(address))
            throw new Error(address + " is not an address of the application centre")
        return host.shell
    }

    // Starts the shell in one of `states`, and does its act.
    function enter(state) {
        const shell = host.start(state.set, state.address, state.apps ?? "")
        host.current = state
        host.actDone = !state.act
        host.phase = 0
        if (state.act === "installing")
            host.apps.holdAfter(host.linesBeforeActivity(state.apps))
        if (state.act)
            stepper.start()
        return shell
    }

    // The caller lets the event loop run afterwards (wait(0)), so that the
    // old shell is really gone before the next is looked at.
    function stop() {
        stepper.stop()
        host.current = null
        host.actDone = true
        if (host.stage === null)
            return
        host.stage.visible = false
        host.stage.destroy()
        host.stage = null
    }

    // --- Acts ------------------------------------------------------------

    // Every item under `item`, depth first; the first named `name`.
    function find(item, name) {
        if (item === null || item === undefined)
            return null
        if (item.objectName === name)
            return item
        const children = item.children ?? []
        for (let i = 0; i < children.length; i++) {
            const found = host.find(children[i], name)
            if (found !== null)
                return found
        }
        return null
    }

    function page() {
        const loader = host.find(host.shell, "shellView")
        return loader ? loader.item : null
    }

    // The sheet a state shows, opened on the page; null for none.
    function sheetOf(state) {
        const view = host.page()
        if (view === null || !state || !state.sheet)
            return null
        return state.sheet === "installSheet" ? view.planSheet : view.removalSheet
    }

    // How many lines of the recorded installation come before flatpak's
    // first activity: the progress is held there for its picture.
    function linesBeforeActivity(scenario) {
        const runs = Testing.Harness.readJson(host.appsDirectory(scenario) + "/runs.json")
        if (!runs)
            return 3
        for (const invocation of runs.invocations) {
            if (invocation.arguments[0] !== "install")
                continue
            for (let i = 0; i < invocation.stdout.length; i++) {
                if (JSON.parse(invocation.stdout[i]).event === "activity")
                    return i + 1
            }
        }
        return 3
    }

    property int phase: 0

    Timer {
        id: stepper

        interval: 20
        repeat: true
        onTriggered: host.advance()
    }

    // One step of the state's act, each time it can be taken.
    function advance() {
        const state = host.current
        const view = host.page()
        if (state === null || view === null || host.apps === null)
            return
        if (host.catalogue.application.status !== ApplicationSession.Ready
                || host.apps.installedStatus === Apps.Loading)
            return
        const id = host.catalogue.application.applicationId
        const operation = host.apps.operation(id)
        const finish = () => {
            // A sheet that is closing is gone before the state is looked at.
            if (!state.sheet && view.planSheet.visible)
                return
            stepper.stop()
            host.actDone = true
        }
        switch (state.act) {
        case "plan":
            if (host.phase === 0) {
                view.primaryClicked()
                host.phase = 1
            } else if (operation && operation.phase !== AppsOperation.Planning) {
                if (operation.phase !== AppsOperation.Planned || host.sheetOpened(view))
                    finish()
            }
            break
        case "installing":
        case "install":
        case "cancel":
            if (host.phase === 0) {
                view.primaryClicked()
                host.phase = 1
            } else if (host.phase === 1 && operation && operation.phase === AppsOperation.Planned
                       && view.planSheet.opened) {
                // As the sheet's Install does: it closes, and the plan is
                // carried out.
                view.planSheet.close()
                operation.confirm()
                host.phase = 2
            } else if (host.phase === 1 && operation && operation.phase === AppsOperation.Failed) {
                finish()
            } else if (host.phase === 2 && operation) {
                if (state.act === "installing" && operation.phase === AppsOperation.Running
                        && operation.activity.length > 0) {
                    finish()
                } else if (state.act === "cancel" && operation.phase === AppsOperation.Running
                           && operation.cancellable && operation.step === "install") {
                    operation.cancel()
                    host.phase = 3
                } else if (operation.phase !== AppsOperation.Running) {
                    finish()
                }
            } else if (host.phase === 3 && operation && operation.phase !== AppsOperation.Running) {
                finish()
            }
            break
        case "remove-question":
            if (host.phase === 0 && view.installed) {
                view.openRemoveQuestion()
                host.phase = 1
            } else if (host.phase === 1 && host.sheetOpened(view)) {
                finish()
            }
            break
        default:
            finish()
        }
    }

    function sheetOpened(view) {
        const state = host.current
        if (!state || !state.sheet)
            return true
        const sheet = host.sheetOf(state)
        return sheet !== undefined && sheet !== null && sheet.opened
    }
}

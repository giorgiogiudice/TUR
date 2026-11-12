// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import Tur.Centre

// Puts the application centre's shell on a stage for the tests: a Catalogue
// that answers from a set of fixtures (recorded answers of tur-catalogue), or
// from a catalogue program that is not there, and a Shell on top of it.
//
// `centreFixtures` and `centreHelper` are set by tst_main.cpp: the fixtures
// directory of the source tree, and the stand-in program of unit/helper.cpp.
Item {
    id: host

    // The states every walk goes through and every picture is taken of.
    //
    //   name     the name of the state, and of its pictures
    //   set      the fixture set; empty for "no catalogue program"
    //   address  where the shell is sent
    //   open     the sections to open in place, by objectName
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
        { name: "no-route-banking", set: "default", address: "tur-centre:app/tur.class.android-banking", open: [] }
    ]

    property Item stage: null
    readonly property Shell shell: host.stage !== null ? host.stage.shell : null
    readonly property Catalogue catalogue: host.stage !== null ? host.stage.catalogue : null

    // Nothing is awaited: every answer that was asked for has arrived.
    readonly property bool settled: host.catalogue !== null && host.catalogue.search.settled
                                    && host.catalogue.application.status !== ApplicationSession.Loading

    Component {
        id: stageComponent

        Item {
            id: stageRoot

            property string fixture
            property string program: "tur-catalogue"
            property int timeout: 5000
            property alias catalogue: stageCatalogue
            property alias shell: stageShell

            anchors.fill: parent

            Catalogue {
                id: stageCatalogue

                fixture: stageRoot.fixture
                program: stageRoot.program
                timeout: stageRoot.timeout
            }

            Shell {
                id: stageShell

                anchors.fill: parent
                catalogue: stageCatalogue
                // The tests do not wait for a pause in the typing unless
                // the pause is what they test.
                debounce: 0
            }
        }
    }

    // Starts a shell on a fixture set and sends it to an address. With
    // `set` empty there is no catalogue program at all. Returns the shell.
    function start(set, address) {
        return host.startWith(set.length > 0 ? { fixture: centreFixtures + "/" + set }
                                             : { program: "/nonexistent/tur-catalogue" }, address)
    }

    // Starts a shell on the stand-in program, which misbehaves on request.
    function startOnHelper(address, timeout) {
        return host.startWith({ program: centreHelper, timeout: timeout }, address)
    }

    function startWith(properties, address) {
        host.stop()
        host.stage = stageComponent.createObject(host, properties)
        if (host.stage === null)
            throw new Error("the stage could not be created")
        if (address.length > 0 && !host.shell.open(address))
            throw new Error(address + " is not an address of the application centre")
        return host.shell
    }

    // Starts the shell in one of `states`.
    function enter(state) {
        return host.start(state.set, state.address)
    }

    // The caller lets the event loop run afterwards (wait(0)), so that the
    // old shell is really gone before the next is looked at.
    function stop() {
        if (host.stage === null)
            return
        host.stage.visible = false
        host.stage.destroy()
        host.stage = null
    }
}

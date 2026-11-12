// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import Tur.Settings

// Puts the settings shell on a stage for the tests: what the window is made
// of (SystemFacts, SettingsHost, Shell) without the window, reading "what
// this system is" from a fixture set.
//
// `settingsFixtures` and `settingsHelper` are set by tst_main.cpp: the
// fixtures directory of the source tree, and the stand-in for KDE System
// Settings (unit/helper.cpp). The stage is not on the session bus: what the
// bus adds is tested in unit/tst_bus.cpp.
Item {
    id: host

    // The states every walk goes through and every picture is taken of.
    //
    //   name       the name of the state, and of its pictures
    //   set        the fixture set
    //   go         an address to go to, as the command line does: a page is
    //              opened, a row is revealed and highlighted
    //   reveal     an address to reveal, with `options`
    //   follow     an address a row leads to, followed as its button does
    //   query      words to search for
    //   open       sections to open in place, by objectName
    //   developer  developer mode: the address is shown
    readonly property var states: [
        { name: "overview", set: "default", go: "tur.overview" },
        { name: "overview-not-tur", set: "not-tur", go: "tur.overview" },
        { name: "appearance", set: "default", go: "tur.appearance" },
        { name: "appearance-row-revealed", set: "default", go: "tur.appearance#variant", developer: true },
        { name: "appearance-system-settings-missing", set: "default", go: "tur.appearance",
          follow: "kde.kcm_colors" },
        { name: "about", set: "default", go: "tur.about" },
        { name: "about-row-revealed-by-pax", set: "rebranded", reveal: "tur.about#source-revision",
          options: { pax: true, dim: true }, developer: true },
        { name: "about-licence", set: "default", go: "tur.about", open: ["licenceOpener"] },
        { name: "search-results", set: "default", query: "dark" },
        { name: "search-nothing", set: "default", query: "printer" }
    ]

    property Item stage: null
    readonly property Shell shell: host.stage !== null ? host.stage.shell : null
    readonly property SettingsHost service: host.stage !== null ? host.stage.service : null
    readonly property SystemFacts facts: host.stage !== null ? host.stage.facts : null

    Component {
        id: stageComponent

        Item {
            id: stageRoot

            property string fixture
            // No KDE System Settings unless a test names a program.
            property string systemSettingsProgram: "/nonexistent/systemsettings"
            property bool developer: false
            property alias shell: stageShell
            property alias service: stageService
            property alias facts: stageFacts

            anchors.fill: parent

            SystemFacts {
                id: stageFacts

                fixture: stageRoot.fixture
            }

            SettingsHost {
                id: stageService

                registry: Registry
                systemSettingsProgram: stageRoot.systemSettingsProgram
                currentAddress: stageShell.address
            }

            Shell {
                id: stageShell

                anchors.fill: parent
                host: stageService
                facts: stageFacts
                developer: stageRoot.developer
                // The tests do not wait for a pause in the typing unless the
                // pause is what they test.
                debounce: 0
            }
        }
    }

    // Starts a shell on a fixture set. `properties` may name
    // systemSettingsProgram and developer. Returns the shell.
    function start(set, properties) {
        host.stop()
        const all = Object.assign({ fixture: settingsFixtures + "/" + set }, properties !== undefined ? properties : {})
        host.stage = stageComponent.createObject(host, all)
        if (host.stage === null)
            throw new Error("the stage could not be created")
        return host.shell
    }

    // Starts the shell in one of `states`. The sections the state opens in
    // place are for the caller, which can find them.
    function enter(state) {
        const shell = host.start(state.set, { developer: state.developer === true })
        if (state.go !== undefined) {
            const outcome = host.service.go(state.go)
            if (outcome.ok !== true)
                throw new Error(state.go + " could not be gone to: " + outcome.code)
        }
        if (state.reveal !== undefined) {
            const outcome = host.service.reveal(state.reveal, state.options !== undefined ? state.options : {})
            if (outcome.ok !== true)
                throw new Error(state.reveal + " could not be revealed: " + outcome.code)
        }
        if (state.follow !== undefined)
            shell.follow(state.follow, false)
        if (state.query !== undefined)
            shell.search(state.query)
        return shell
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

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Installer

// Puts the installer on a stage for the tests: an Engine that replays a set
// of the engine's recorded answers (fixtures/), the keyboard layouts writing
// to a scratch directory, and the Wizard on top.
//
// `installerFixtures` and `installerScratch` are set by tst_main.cpp.
Item {
    id: host

    // The states every walk goes through and every picture is taken of.
    //
    //   name     the name of the state, and of its pictures
    //   fixture  the fixture set (default beside-windows)
    //   step     the screen
    //   chosen   the disk chosen before it, by kernel name
    //   install  the recorded installation to run
    //   mode     how: "erase" (default) or "beside" Windows (ADR-0028)
    readonly property var states: [
        { name: "welcome", step: "welcome" },
        { name: "computer", step: "computer" },
        { name: "computer-blocked", fixture: "blocked", step: "computer" },
        { name: "disks", step: "disk" },
        { name: "disks-chosen", step: "disk", chosen: "vda" },
        { name: "disks-none", fixture: "no-disk", step: "disk" },
        { name: "account", step: "account", chosen: "vda" },
        { name: "confirm-blank", step: "confirm", chosen: "vda" },
        { name: "confirm-windows", step: "confirm", chosen: "vdb" },
        { name: "installing-cancellable", step: "install", chosen: "vda", install: "install-held" },
        { name: "installing", step: "install", chosen: "vda", install: "install-paused" },
        { name: "done-success", step: "done", chosen: "vda", install: "install" },
        { name: "done-failed", step: "done", chosen: "vda", install: "install-failed" },
        { name: "done-refused", step: "done", chosen: "vda", install: "install-refused" },
        { name: "disks-windows-chosen", fixture: "same-disk", step: "disk", chosen: "vda" },
        { name: "disks-beside-chosen", fixture: "same-disk", step: "disk", chosen: "vda", mode: "beside" },
        { name: "disks-beside-not-possible", fixture: "same-disk", step: "disk", chosen: "vdb" },
        { name: "confirm-beside", fixture: "same-disk", step: "confirm", chosen: "vda", mode: "beside" },
        { name: "installing-beside", fixture: "same-disk", step: "install", chosen: "vda", mode: "beside",
          install: "install-beside-paused" },
        { name: "done-beside", fixture: "same-disk", step: "done", chosen: "vda", mode: "beside", install: "install-beside" },
        { name: "done-beside-failed", fixture: "same-disk", step: "done", chosen: "vda", mode: "beside",
          install: "install-beside-failed" }
    ]

    property Item stage: null
    property var pending: null
    readonly property Wizard wizard: host.stage !== null ? host.stage.wizard : null
    readonly property Engine engine: host.stage !== null ? host.stage.engine : null

    // The state asked for has been reached, and nothing is awaited.
    readonly property bool ready: {
        if (host.stage === null || host.pending === null)
            return false
        const state = host.pending
        const engine = host.engine
        const wizard = host.wizard
        if (wizard.step !== state.step)
            return false
        switch (state.step) {
        case "computer":
            return engine.checkState === "done" || engine.checkState === "failed"
        case "disk":
            return engine.disksState === "done"
        case "install":
            return state.install === "install-held" ? engine.stage === "revalidate" : engine.stage === "copy"
        case "done":
            return engine.installState !== "running"
        default:
            return true
        }
    }

    Component {
        id: stageComponent

        Item {
            id: stageRoot

            property string fixture
            property string installFixture: "install"
            property alias engine: stageEngine
            property alias wizard: stageWizard

            anchors.fill: parent

            Engine {
                id: stageEngine

                fixture: stageRoot.fixture
                installFixture: stageRoot.installFixture
            }

            KeyboardLayouts {
                id: stageLayouts

                configDirectory: installerScratch
            }

            Wizard {
                id: stageWizard

                anchors.fill: parent
                engine: stageEngine
                keyboard: stageLayouts
            }
        }
    }

    function disk(name: string): var {
        for (const item of host.engine.disks) {
            if (item.name === name)
                return item
        }
        return null
    }

    // Starts the installer in one of `states`. The caller waits for `ready`.
    function enter(state: var): Wizard {
        host.stop()
        host.pending = state
        host.stage = stageComponent.createObject(host, {
            fixture: installerFixtures + "/" + (state.fixture || "beside-windows"),
            installFixture: state.install || "install"
        })
        if (host.stage === null)
            throw new Error("the stage could not be created")
        if (state.chosen)
            host.engine.runDisks()
        else
            host.wizard.goTo(state.step)
        return host.wizard
    }

    // The caller lets the event loop run afterwards (wait(0)), so that the
    // old installer is really gone before the next is looked at.
    function stop(): void {
        host.pending = null
        if (host.stage === null)
            return
        host.stage.visible = false
        host.stage.destroy()
        host.stage = null
    }

    Connections {
        target: host.engine

        // A state after the disk screen: choose the disk the engine listed,
        // then go on, as a person would.
        function onDisksChanged(): void {
            const state = host.pending
            if (state === null || !state.chosen || host.engine.disksState !== "done")
                return
            if (host.wizard.chosenDisk.name === state.chosen)
                return
            const chosen = host.disk(state.chosen)
            if (chosen === null)
                throw new Error("the fixture has no disk " + state.chosen)
            host.wizard.chosenDisk = chosen
            host.wizard.mode = state.mode || "erase"
            if (state.install) {
                host.wizard.goTo("confirm")
                host.engine.install(chosen, state.mode === "beside" ? chosen.beside.phrase : chosen.phrase,
                                    state.mode || "erase")
                host.wizard.goTo("install")
            } else {
                host.wizard.goTo(state.step)
            }
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.PaxCompanion
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// Puts P@x's character on a stage for the tests and the pictures: a stand-in
// for the desktop (a window, and a bar along the bottom edge, drawn with the
// kit's colours in the variant being looked at), and over it the strip the
// character lives in (Stage), with its rules (Companion), the person's
// choices (CompanionSettings, kept nowhere) and its placement. The stand-in
// bar is not Plasma's: it is a rectangle as thick as Plasma's default panel.
//
// `scenes` is every scene the walks go through and the pictures are taken
// of; reach() gets there by telling the rules what the services would:
// P@x's state, P@x's indicator, and the healing service's notices as its
// program prints them (fixtures/heal/notices.json, `healNoticesFile`).
Item {
    id: host

    // The stand-in bar's thickness: Plasma's default panel.
    property real barHeight: 44
    // The screen is taller than the stage, so that nothing has to scroll.
    readonly property real screenHeight: 2400
    // Noon on 14 November 2026, UTC, for the rules' clock.
    readonly property real noon: 1794657600000

    // What the healing service's program prints.
    readonly property var printed: Testing.Harness.readJson(healNoticesFile).printed

    // A notice as AgentSource makes it from a task of P@x's that ended with
    // a refused step, and from one the day's limit stopped (the sentences
    // are the recorded service's: fixtures of tur-pax-ui).
    readonly property var refusedNotice: ({
        id: "task:t-1794571291-2", kind: "task", sender: "tur",
        sentence: "P@x finished, but one step was refused.",
        detail: "The assistant cannot ask for your approval in a way that is safe on this system, so it does only things that can be undone.",
        actions: ["show", "dismiss"], opens: "panel"
    })
    readonly property var limitNotice: ({
        id: "task:t-1794571291-5", kind: "task", sender: "tur",
        sentence: "P@x stopped: today's spending limit was reached.",
        code: "TUR-AI-SPEND-001", actions: ["show", "dismiss"], opens: "panel"
    })
    readonly property var limitHint: ({
        id: "suggest:limit:2026-11-14", sender: "tur",
        sentence: "It looks like P@x has used most of today's spending limit. You can change the limit in P@x's setup.",
        actions: ["show", "dismiss"], opens: "setup"
    })

    // Each scene:
    //   name       its name, and its pictures'
    //   off        P@x is off, and the person keeps the character on the bar
    //   service    org.tur.Agent1's state: idle (the default), working, paused
    //   indicator  what P@x's indicator shows
    //   ask        the ask bar is open
    //   waiting    how many proposals wait
    //   problem    P@x is on and cannot take a task
    //   step       the step P@x is on
    //   spent      the share of the day's limit used (0.2 by default)
    //   battery    on battery
    //   quiet      hour, tomorrow or always
    //   heal       indexes into `printed`: the notices the healing service has
    //   offer      a notice offered to the rules
    //   suggest    a suggestion (suggestions are switched on for it)
    //   fold       the bubble was left alone
    //   mood       refused or error
    //   act        an action of the bubble pressed
    //   page       what is opened from the menu
    //   along      where along the bar (1, its end, by default)
    readonly property var scenes: [
        { name: "asleep", off: true },
        { name: "idle" },
        { name: "noticing", heal: [0], fold: true },
        { name: "listening", ask: true },
        { name: "thinking", service: "working" },
        { name: "observing", service: "working", indicator: "observing" },
        { name: "acting", service: "working", indicator: "acting" },
        { name: "waiting", waiting: 1 },
        { name: "paused", service: "paused" },
        { name: "telling", heal: [0] },
        { name: "refused", mood: "refused" },
        { name: "error", problem: { ready: false, code: "", message: "" } },
        { name: "low-fuel", spent: 0.92 },
        { name: "on-battery", service: "working", battery: true },
        { name: "bubble-fault", heal: [1] },
        { name: "bubble-refused", offer: host.refusedNotice, mood: "refused" },
        { name: "bubble-limit", offer: host.limitNotice, mood: "refused", spent: 1 },
        { name: "bubble-suggestion", spent: 0.95, suggest: host.limitHint },
        { name: "bubble-at-the-start", heal: [0], along: 0 },
        { name: "notice", heal: [1], act: "show" },
        { name: "menu", page: "menu" },
        { name: "menu-quiet", quiet: "hour", heal: [0], page: "menu" },
        { name: "menu-working", service: "working", page: "menu" },
        { name: "menu-asleep", off: true, page: "menu" },
        { name: "watching", page: "watching" },
        { name: "watching-working", service: "working", battery: true, page: "watching",
          step: { title: "Read the test value", reason: "observe_permitted" } },
        { name: "settings", page: "settings" }
    ]

    property Item rig: null
    readonly property Stage stage: host.rig !== null ? host.rig.stageItem : null
    readonly property Companion companion: host.rig !== null ? host.rig.companionObject : null
    readonly property CompanionSettings settings: host.rig !== null ? host.rig.settingsObject : null
    readonly property Placement placement: host.rig !== null ? host.rig.placementObject : null
    // The height the stage needs for all that is shown.
    readonly property real wantedHeight: host.stage !== null ? host.stage.wantedHeight : host.barHeight

    function scene(name: string): var {
        for (const entry of host.scenes) {
            if (entry.name === name)
                return entry
        }
        throw new Error("no scene " + name)
    }

    function stop(): void {
        if (host.rig !== null) {
            host.rig.destroy()
            host.rig = null
        }
    }

    // A fresh character, told what the scene says.
    function reach(scene: var): void {
        host.stop()
        host.rig = rigComponent.createObject(host)
        const rules = host.companion
        if (scene.off === true)
            host.settings.presence = "always"
        if (scene.along !== undefined)
            host.settings.along = scene.along
        rules.paxOn = scene.off !== true
        rules.serviceState = scene.off === true ? "" : (scene.service !== undefined ? scene.service : "idle")
        rules.overlayActivity = scene.indicator !== undefined ? scene.indicator : ""
        rules.waiting = scene.waiting !== undefined ? scene.waiting : 0
        rules.spent = scene.spent !== undefined ? scene.spent : 0.2
        rules.onBattery = scene.battery === true
        if (scene.problem !== undefined)
            rules.problem = scene.problem
        if (scene.step !== undefined)
            rules.step = scene.step
        if (scene.quiet !== undefined)
            rules.setQuiet(scene.quiet)
        if (scene.suggest !== undefined) {
            host.settings.suggestions = true
            rules.suggest(scene.suggest)
        }
        if (scene.heal !== undefined)
            host.rig.healObject.ingest(JSON.stringify(scene.heal.map(index => host.printed[index])))
        if (scene.offer !== undefined)
            rules.offer(scene.offer)
        if (scene.fold === true)
            rules.fold()
        if (scene.mood !== undefined)
            rules.flag(scene.mood)
        if (scene.ask === true)
            rules.askOpen = true
        if (scene.act !== undefined)
            host.stage.act(scene.act)
        if (scene.page !== undefined)
            host.stage.page = scene.page
    }

    // The desktop under the strip, in the variant being drawn.
    Rectangle {
        anchors.fill: parent
        color: Kit.Theme.surfaceSunken
    }

    // A window on it.
    Rectangle {
        anchors.fill: parent
        anchors.margins: 16
        anchors.bottomMargin: host.barHeight + 16
        color: Kit.Theme.surfaceBase
        border.width: 1
        border.color: Kit.Theme.lineSubtle

        Kit.Label {
            x: 16
            y: 12
            text: "A window"
            tone: Kit.Label.Secondary
        }
    }

    // The stand-in for the bar.
    Rectangle {
        id: bar

        objectName: "standInBar"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: host.barHeight
        color: Kit.Theme.surfaceRaised

        Row {
            x: 12
            anchors.verticalCenter: parent.verticalCenter
            spacing: 10

            Repeater {
                model: 4

                delegate: Rectangle {
                    width: 24
                    height: 24
                    radius: 6
                    color: Kit.Theme.surfaceSunken
                }
            }
        }

        Kit.Label {
            // Beside the character's seat at the end of the bar.
            x: Application.layoutDirection === Qt.RightToLeft ? 64 : bar.width - 64 - implicitWidth
            anchors.verticalCenter: parent.verticalCenter
            text: "10:42"
            wrapMode: Text.NoWrap
        }
    }

    Component {
        id: rigComponent

        Item {
            id: rigRoot

            readonly property Stage stageItem: rigStage
            readonly property Companion companionObject: rigCompanion
            readonly property CompanionSettings settingsObject: rigSettings
            readonly property Placement placementObject: rigPlacement
            readonly property HealSource healObject: rigHeal

            anchors.fill: parent

            CompanionSettings {
                id: rigSettings
            }

            Companion {
                id: rigCompanion

                settings: rigSettings
                clock: host.noon
                // Nothing closes or passes by itself during a test.
                bubbleSeconds: 0
                moodSeconds: 0
            }

            HealSource {
                id: rigHeal

                companion: rigCompanion
            }

            Placement {
                id: rigPlacement

                screenWidth: host.width
                screenHeight: host.screenHeight
                usableHeight: host.screenHeight - host.barHeight
                along: rigSettings.along
                rightToLeft: Application.layoutDirection === Qt.RightToLeft
            }

            Stage {
                id: rigStage

                anchors.fill: parent
                companion: rigCompanion
                settings: rigSettings
                placement: rigPlacement
                idleCloseSeconds: 0
            }
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// tur-pax-companion's two surfaces and what feeds them (docs/architecture/
// tur-pax-companion.md, sections 2 and 6).
//
//  - `strip`: the strip of the screen the character lives in (Stage). Under
//    Wayland the program makes it a layer-shell surface anchored to the
//    bottom edge and both sides, on the overlay layer, ignoring the bar's
//    exclusive zone so that it reaches the screen's edge, with no keyboard
//    and an input region that is only the character and what is open.
//  - `probe`: one transparent pixel column that takes no input and does not
//    ignore the bar's exclusive zone. Its height is what the compositor
//    leaves beside the bar; the screen's height less that is the bar's
//    thickness. Nothing of Plasma's is asked or loaded.
//
// The tests and the pictures load Stage without these windows.
QtObject {
    id: main

    // The bus P@x's service, its window and its indicator are on; empty for
    // the session bus.
    property string busAddress
    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant
    // Where the person's choices are kept; empty to keep nothing.
    property string settingsPath
    // The healing service's program and arguments; empty for tur-diag's.
    property list<string> healCommand
    // The healing service's state file; empty for the system's.
    property string healStateFile
    // The bar's thickness where the compositor cannot be asked (no layer
    // shell): for trying the program on another platform.
    property real standInInset: 0
    // Set by the program: the windows are layer-shell surfaces.
    property bool layered: false
    // Set by the program once the surfaces are configured; nothing is shown
    // before.
    property bool ready: false

    readonly property CompanionSettings settings: CompanionSettings {
        path: main.settingsPath
    }

    readonly property Companion companion: Companion {
        settings: main.settings
        overlayActivity: main.indicator.present ? String(main.indicator.value) : ""
        askOpen: main.paxWindow.present && String(main.paxWindow.value) === "ask"
        onBattery: main.battery.present && main.battery.value === true
    }

    readonly property AgentSource agent: AgentSource {
        busAddress: main.busAddress
        companion: main.companion
    }

    readonly property HealSource heal: HealSource {
        companion: main.companion
        command: main.healCommand
    }

    // What P@x's indicator shows: the character says the same.
    readonly property BusProperty indicator: BusProperty {
        busAddress: main.busAddress
        service: "org.tur.PaxOverlay1"
        path: "/org/tur/PaxOverlay1"
        iface: "org.tur.PaxOverlay1"
        name: "Activity"
    }

    // What P@x's window shows: while the ask bar is open the character
    // listens and says nothing.
    readonly property BusProperty paxWindow: BusProperty {
        busAddress: main.busAddress
        service: "org.tur.PaxUi1"
        path: "/org/tur/PaxUi1"
        iface: "org.tur.PaxUi1"
        name: "Mode"
    }

    readonly property BusProperty battery: BusProperty {
        system: true
        service: "org.freedesktop.UPower"
        path: "/org/freedesktop/UPower"
        iface: "org.freedesktop.UPower"
        name: "OnBattery"
    }

    readonly property Placement placement: Placement {
        screenWidth: ground.screenWidth
        screenHeight: ground.screenHeight
        usableHeight: main.layered ? probeWindow.height : ground.screenHeight - main.standInInset
        along: main.settings.along
        rightToLeft: Application.layoutDirection === Qt.RightToLeft
    }

    readonly property Window probe: Window {
        id: probeWindow

        objectName: "paxProbe"
        title: "P@x"
        width: 1
        height: 1
        color: "transparent"
        visible: main.ready && main.layered
    }

    readonly property Window strip: Window {
        id: stripWindow

        objectName: "paxStrip"
        title: "P@x"
        width: ground.screenWidth
        height: stage.wantedHeight
        color: "transparent"
        visible: main.ready && main.companion.shown

        Item {
            id: ground

            readonly property real screenWidth: Screen.width
            readonly property real screenHeight: Screen.height
            readonly property string screenName: Screen.name

            anchors.fill: parent

            Stage {
                id: stage

                anchors.fill: parent
                companion: main.companion
                settings: main.settings
                placement: main.placement
                onAskRequested: main.agent.showWindow("ask")
                onWindowRequested: mode => main.agent.showWindow(mode)
                onStopRequested: main.agent.stop()
                onAskPaxRequested: request => main.agent.ask(request)
            }
        }
    }

    // For the program: the only places of the strip that take a click,
    // whether the person asked for the keyboard, whether the character
    // belongs over a full-screen window, and where it is.
    readonly property var hitRects: stage.hitRects
    readonly property bool attended: stage.attended
    readonly property bool raised: main.companion.raised
    readonly property var anchor: ({
        "screen": ground.screenName,
        "x": stage.anchorX,
        "lift": main.placement.popupLift - main.placement.inset,
        "shown": main.companion.shown
    })

    // The person asked for the keyboard (org.tur.PaxCompanion1.Attend).
    function attend(): void {
        stage.attend()
    }

    Component.onCompleted: {
        if (main.variant === "light")
            Kit.Theme.light = true
        else if (main.variant === "dark")
            Kit.Theme.light = false
        if (main.healStateFile.length > 0)
            main.heal.stateFile = main.healStateFile
        main.agent.start()
        main.heal.start()
        main.indicator.start()
        main.paxWindow.start()
        main.battery.start()
    }
}

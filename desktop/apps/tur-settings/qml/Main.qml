// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The settings window. Everything in it is Shell; the tests and the
// screenshot job load Shell without the window.
Window {
    id: window

    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant

    // Where "what this system is" is read from: see src/main.cpp and
    // SystemFacts.
    property string fixture

    // What to open: a page or a row, as an address or as a link.
    property string address

    // Developer mode: show the address of what is on screen.
    property bool developer: false

    // Answer on the session bus as org.tur.Settings1.
    property bool dbus: false

    property string systemSettingsProgram: "systemsettings"

    // Why the program is not on the bus, when it was asked to be; and the
    // address given at the start, if it could not be opened. For src/main.cpp.
    readonly property string busProblem: service.busProblem
    property string refused

    width: 1080
    height: 760
    minimumWidth: 640
    minimumHeight: 480
    visible: true
    title: qsTr("Settings")
    color: Kit.Theme.surfaceBase

    Component.onCompleted: {
        if (window.variant === "light")
            Kit.Theme.light = true
        else if (window.variant === "dark")
            Kit.Theme.light = false
        if (window.dbus)
            service.startBus()
        if (window.address.length > 0) {
            const outcome = service.go(window.address)
            if (outcome.ok !== true) {
                shell.failure = outcome
                window.refused = outcome.code + ": " + window.address
            }
        } else {
            // The search field has the keyboard when the window opens.
            shell.focusSearch()
        }
    }

    SystemFacts {
        id: system

        fixture: window.fixture
    }

    SettingsHost {
        id: service

        registry: Registry
        systemSettingsProgram: window.systemSettingsProgram
        currentAddress: shell.address
        onPresentRequested: {
            window.show()
            window.raise()
            window.requestActivate()
        }
    }

    Shell {
        id: shell

        anchors.fill: parent
        host: service
        facts: system
        developer: window.developer
    }
}

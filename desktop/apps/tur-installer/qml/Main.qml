// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The installer's window. Everything in it is Wizard; the tests and the
// pictures load Wizard without the window.
//
// The window writes the installer's state lines (Transcript): which screen
// is shown, which control has the keyboard and whether the window is active.
// tests/iso drives the installer with the keyboard in a virtual machine and
// follows these lines; a person never needs them.
Window {
    id: window

    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant
    // The engine's recorded answers, for tests; empty to run the engine.
    property string fixture
    property string installFixture: "install"
    property string engineProgram: "/usr/libexec/tur-installer/tur-install-engine"
    property string engineLauncher: "pkexec"

    // The nearest name of the item that has the keyboard.
    function nameOf(item: Item): string {
        for (let at = item; at !== null; at = at.parent) {
            if (at.objectName.length > 0)
                return at.objectName
        }
        return "none"
    }

    width: 1120
    height: 740
    minimumWidth: 800
    minimumHeight: 560
    visible: true
    title: qsTr("Install TUR")
    color: Kit.Theme.surfaceBase

    Component.onCompleted: {
        if (window.variant === "light")
            Kit.Theme.light = true
        else if (window.variant === "dark")
            Kit.Theme.light = false
        Transcript.write("window shown variant=" + (Kit.Theme.light ? "light" : "dark") + " width=" + window.width
                         + " height=" + window.height)
    }

    onActiveChanged: Transcript.write("window active=" + (window.active ? "true" : "false"))
    onActiveFocusItemChanged: {
        const item = window.activeFocusItem
        Transcript.write("focus item=" + window.nameOf(item) + " screen=" + wizard.step)
    }

    // While the disk is written the window does not close: closing it would
    // not stop the installation, and the person would lose sight of it.
    onClosing: closeEvent => {
        if (engine.installState === "running") {
            closeEvent.accepted = false
            wizard.explainBusy()
            Transcript.write("close refused reason=installing")
            return
        }
        Transcript.write("closed")
    }

    Engine {
        id: engine

        fixture: window.fixture
        installFixture: window.installFixture
        program: window.engineProgram
        launcher: window.engineLauncher
    }

    KeyboardLayouts {
        id: layouts
    }

    Wizard {
        id: wizard

        anchors.fill: parent
        engine: engine
        keyboard: layouts
        onCloseRequested: window.close()
    }
}

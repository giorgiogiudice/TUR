// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// tur-pax-ui's windows (docs/architecture/tur-pax-ui.md, section 2). One
// surface in two sizes, the ask bar and the conversation panel
// (desktop-experience.md, 9.2), which the program makes a layer-shell
// surface under Wayland; and the setup window (9.12), an ordinary window.
// The tests and the pictures load AskBar, ConversationPanel and SetupFlow
// without these windows.
QtObject {
    id: main

    // What is shown: "" (nothing), ask, panel, activity or setup ("ask-held"
    // is taken as ask, by P@x's key).
    property string mode: "ask"
    // The bus P@x's service is on; empty for the session bus.
    property string busAddress
    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant
    // Set by the program once the surface is configured; nothing is shown
    // before, so that a layer-shell surface is never mapped unconfigured.
    property bool ready: false

    readonly property AgentClient client: AgentClient {
        busAddress: main.busAddress
    }

    readonly property Conversation conversation: Conversation {
        client: main.client
        // A task started or ended: the bar becomes the panel (9.2).
        onAttentionWanted: {
            if (main.mode === "ask")
                main.show("panel")
        }
    }

    readonly property Window surface: Window {
        id: surfaceWindow

        objectName: "paxSurface"
        title: "P@x"
        width: main.mode === "ask" ? 640 : 420
        height: main.mode === "ask" ? Math.max(64, Math.ceil(bar.naturalHeight)) : 760
        minimumWidth: main.mode === "ask" ? 480 : 360
        color: "transparent"
        visible: main.ready && (main.mode === "ask" || main.mode === "panel" || main.mode === "activity")

        // Out of the way when it loses the keyboard while empty (9.2).
        onActiveChanged: {
            if (!surfaceWindow.active && main.mode === "ask" && bar.field.text.length === 0)
                main.show("")
        }

        AskBar {
            id: bar

            anchors.fill: parent
            visible: main.mode === "ask"
            client: main.client
            conversation: main.conversation
            onSetupRequested: main.show("setup")
            onCloseRequested: main.show("")
        }

        ConversationPanel {
            id: panel

            anchors.fill: parent
            visible: main.mode === "panel" || main.mode === "activity"
            client: main.client
            conversation: main.conversation
            onSetupRequested: main.show("setup")
            onCloseRequested: main.show("")
        }
    }

    readonly property Window setupWindow: Window {
        objectName: "paxSetup"
        title: qsTr("Set up P@x")
        width: 600
        height: 780
        minimumWidth: 420
        minimumHeight: 480
        color: Kit.Theme.surfaceBase
        visible: main.ready && main.mode === "setup"

        onClosing: main.show("")

        Loader {
            anchors.fill: parent
            active: main.mode === "setup"
            sourceComponent: SetupFlow {
                client: main.client
                onFinished: main.show(main.client.status.ready === true ? "ask" : "")
            }
        }
    }

    // Shows what is asked for, and gives it the keyboard. "ask-held" is the
    // bar opened by P@x's key (the desktop file's action "ask"): if the key
    // is still held, the bar listens (docs/architecture/pax-voice.md,
    // section 7).
    function show(asked: string): void {
        const held = asked === "ask-held"
        const mode = held ? "ask" : asked
        main.mode = mode
        if (mode === "ask") {
            if (held)
                bar.keyOpened()
            main.surface.requestActivate()
            bar.field.forceActiveFocus()
        } else if (mode === "panel" || mode === "activity") {
            panel.page = mode === "activity" ? "activity" : "conversation"
            main.surface.requestActivate()
        } else if (mode === "setup") {
            main.setupWindow.requestActivate()
        }
    }

    Component.onCompleted: {
        if (main.mode === "ask-held") {
            main.mode = "ask"
            // Once the voice status is known; the key may still be held.
            Qt.callLater(bar.keyOpened)
        }
        if (main.variant === "light")
            Kit.Theme.light = true
        else if (main.variant === "dark")
            Kit.Theme.light = false
        main.client.start()
    }
}

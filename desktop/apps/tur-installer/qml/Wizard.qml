// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// The installer, without its window (desktop-experience.md, 11.5). Three
// regions: the steps on the left, the current screen in the middle, and the
// diagnostics drawer on the right, which is there from the first screen to
// the last (specification 28.4).
//
// The screens, in order: welcome (language, keyboard, what will happen),
// this computer (the checks), where to install (the disks), your account
// (made at the first start), confirm (the destructive confirmation), and
// installing; then the result. The engine does every step that looks at or
// writes to a disk; the wizard decides only what to ask it next.
Kit.Page {
    id: wizard

    required property Engine engine
    required property KeyboardLayouts keyboard

    // The screen shown: welcome, computer, disk, account, confirm, install, done.
    property string step: "welcome"
    // The disk chosen on the disk screen, as the engine listed it.
    property var chosenDisk: ({})
    // How to install on it: "erase" the whole disk, or "beside" Windows, into
    // the free space the engine found there (ADR-0029).
    property string mode: "erase"
    // Beside Windows: the system the computer starts by default, "tur" or
    // "windows" (the owner's decision of 14 November 2026: the person chooses).
    property string defaultSystem: "tur"

    readonly property var steps: [
        { name: "welcome", title: qsTr("Welcome") },
        { name: "computer", title: qsTr("This computer") },
        { name: "disk", title: qsTr("Where to install") },
        { name: "account", title: qsTr("Your account") },
        { name: "confirm", title: qsTr("Confirm") },
        { name: "install", title: qsTr("Installing") }
    ]
    readonly property int stepIndex: {
        for (let i = 0; i < wizard.steps.length; i++) {
            if (wizard.steps[i].name === wizard.step)
                return i
        }
        return wizard.steps.length
    }

    readonly property real railWidth: 220
    readonly property real drawerWidth: 272

    // The height at which nothing has to scroll, for the tests and pictures.
    readonly property real naturalHeight: Math.max(rail.implicitHeight + 2 * Kit.Theme.spaceMargin,
                                                   pane.implicitHeight,
                                                   drawer.implicitHeight + 2 * Kit.Theme.spaceMargin)

    // What has the keyboard, so that it can be kept in sight.
    readonly property Item focusItem: wizard.Window.activeFocusItem

    // The person chose to try TUR first, or to close the installer.
    signal closeRequested()

    function goTo(name: string): void {
        wizard.step = name
        flick.contentY = 0
        Transcript.write("screen name=" + name + " step=" + (wizard.stepIndex + 1))
    }

    // What the drawer says is happening now.
    function phase(): string {
        switch (wizard.step) {
        case "welcome":
            return qsTr("Starting")
        case "computer":
            return wizard.engine.checkState === "running" ? qsTr("Checking this computer") : qsTr("Checks finished")
        case "disk":
            return wizard.engine.disksState === "running" ? qsTr("Finding disks") : qsTr("Choosing a disk")
        case "account":
            return qsTr("Your account")
        case "confirm":
            return qsTr("Waiting for your confirmation")
        case "install":
            return wizard.engine.stage.length > 0 ? wizard.engine.stageTitle(wizard.engine.stage) : qsTr("Starting the installation")
        default:
            switch (wizard.engine.installState) {
            case "success":
                return qsTr("TUR is installed")
            case "cancelled":
                return qsTr("Cancelled")
            default:
                return qsTr("The installation did not complete")
            }
        }
    }

    function explainBusy(): void {
        busySheet.open()
    }

    function openLog(): void {
        logSheet.open()
    }

    function openSaveLog(): void {
        saveSheet.open()
    }

    // Keeps whatever has the keyboard in sight (13.1, X2).
    function reveal(item: Item): void {
        if (item === null)
            return
        let inside = false
        for (let at = item; at !== null; at = at.parent) {
            if (at === flick.contentItem) {
                inside = true
                break
            }
        }
        if (!inside)
            return
        const top = item.mapToItem(flick.contentItem, 0, 0).y
        const bottom = top + item.height
        const room = Kit.Theme.spaceGroup
        const furthest = Math.max(0, flick.contentHeight - flick.height)
        if (top - room < flick.contentY)
            flick.contentY = Math.max(0, top - room)
        else if (bottom + room > flick.contentY + flick.height)
            flick.contentY = Math.min(furthest, bottom + room - flick.height)
    }

    function revealFocus(): void {
        wizard.reveal(wizard.focusItem)
    }

    padding: 0

    onFocusItemChanged: wizard.reveal(wizard.focusItem)

    // A screen is laid out in the frames after it is loaded.
    Timer {
        id: revealLater

        interval: 100
        onTriggered: wizard.revealFocus()
    }

    Connections {
        target: wizard.engine

        function onInstallChanged(): void {
            const state = wizard.engine.installState
            if (wizard.step === "install" && state !== "running" && state !== "idle")
                wizard.goTo("done")
        }
    }

    Component {
        id: welcomeScreen

        WelcomeScreen {
            keyboard: wizard.keyboard
            onContinueRequested: wizard.goTo("computer")
            onTryRequested: wizard.closeRequested()
        }
    }

    Component {
        id: computerScreen

        ComputerScreen {
            engine: wizard.engine
            onBackRequested: wizard.goTo("welcome")
            onContinueRequested: wizard.goTo("disk")
        }
    }

    Component {
        id: diskScreen

        DiskScreen {
            engine: wizard.engine
            chosenName: String(wizard.chosenDisk.name || "")
            mode: wizard.mode
            defaultSystem: wizard.defaultSystem
            onDefaultChosen: chosen => wizard.defaultSystem = chosen
            onDiskChosen: disk => {
                // Another disk, or the same disk listed again: beside
                // Windows only while the engine still offers it there.
                if (String(disk.name || "") !== String(wizard.chosenDisk.name || "") || !Words.besidePossible(disk))
                    wizard.mode = "erase"
                wizard.chosenDisk = disk
            }
            onModeChosen: chosen => wizard.mode = chosen
            onBackRequested: wizard.goTo("computer")
            onContinueRequested: wizard.goTo("account")
        }
    }

    Component {
        id: accountScreen

        AccountScreen {
            onBackRequested: wizard.goTo("disk")
            onContinueRequested: wizard.goTo("confirm")
        }
    }

    Component {
        id: confirmScreen

        ConfirmScreen {
            disk: wizard.chosenDisk
            mode: wizard.mode
            defaultSystem: wizard.defaultSystem
            onBackRequested: wizard.goTo("disk")
            onEraseRequested: phrase => {
                wizard.engine.install(wizard.chosenDisk, phrase, wizard.mode, wizard.defaultSystem)
                wizard.goTo("install")
            }
        }
    }

    Component {
        id: progressScreen

        ProgressScreen {
            engine: wizard.engine
            disk: wizard.chosenDisk
            mode: wizard.mode
        }
    }

    Component {
        id: doneScreen

        DoneScreen {
            engine: wizard.engine
            disk: wizard.chosenDisk
            mode: wizard.mode
            defaultSystem: wizard.defaultSystem
            onRestartRequested: wizard.engine.restart()
            onPowerOffRequested: wizard.engine.powerOff()
            onSaveLogRequested: wizard.openSaveLog()
            onBackToDisksRequested: {
                wizard.engine.resetInstall()
                wizard.goTo("disk")
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // --- the steps ---------------------------------------------------------
        Rectangle {
            Layout.preferredWidth: wizard.railWidth
            Layout.fillHeight: true
            color: Kit.Theme.surfaceSunken

            ColumnLayout {
                id: rail

                objectName: "wizardSteps"
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Kit.Theme.spaceMargin
                spacing: Kit.Theme.spaceRelated

                Kit.Label {
                    Layout.fillWidth: true
                    text: "TUR"
                    kind: Kit.Label.Display
                }

                Kit.Label {
                    Layout.fillWidth: true
                    Layout.bottomMargin: Kit.Theme.spaceGroup
                    text: qsTr("Installer")
                    kind: Kit.Label.Caps
                    tone: Kit.Label.Secondary
                }

                // For the person on the other end of a support request
                // (desktop-experience.md, 6.2): which build this is.
                Kit.Label {
                    objectName: "buildLine"
                    Layout.fillWidth: true
                    Layout.topMargin: -Kit.Theme.spaceGroup + Kit.Theme.spaceGrid
                    Layout.bottomMargin: Kit.Theme.spaceGroup
                    visible: String(wizard.engine.facts.build_id || "").length > 0
                    text: qsTr("TUR %1 · build %2").arg(String(wizard.engine.facts.version || ""))
                              .arg(String(wizard.engine.facts.build_id || ""))
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.WrapAnywhere
                }

                Repeater {
                    model: wizard.steps

                    RowLayout {
                        id: stepRow

                        required property var modelData
                        required property int index

                        readonly property bool current: stepRow.index === wizard.stepIndex
                        readonly property bool done: stepRow.index < wizard.stepIndex

                        Layout.fillWidth: true
                        spacing: Kit.Theme.spaceRelated

                        Rectangle {
                            Layout.preferredWidth: 3
                            Layout.preferredHeight: stepTitle.implicitHeight
                            radius: 1
                            color: stepRow.current ? Kit.Theme.accent : Kit.Theme.surfaceSunken
                        }

                        Item {
                            Layout.preferredWidth: 16
                            Layout.preferredHeight: 16

                            // The last step after an installation that did
                            // not complete is marked as that, not as done.
                            Kit.Glyph {
                                id: stepSign

                                readonly property bool failed: stepRow.index === wizard.steps.length - 1
                                                               && wizard.step === "done"
                                                               && wizard.engine.installState !== "success"

                                anchors.centerIn: parent
                                visible: stepRow.done
                                kind: stepSign.failed ? Kit.Glyph.Negative : Kit.Glyph.Check
                                tint: stepSign.failed ? Kit.Theme.negative : Kit.Theme.positive
                                size: 14
                            }
                        }

                        Kit.Label {
                            id: stepTitle

                            Layout.fillWidth: true
                            text: stepRow.modelData.title
                            kind: stepRow.current ? Kit.Label.Strong : Kit.Label.Body
                            tone: stepRow.current ? Kit.Label.Accent : (stepRow.done ? Kit.Label.Primary : Kit.Label.Secondary)
                            wrapMode: Text.Wrap
                        }
                    }
                }
            }
        }

        Kit.Separator {
            Layout.preferredWidth: Kit.Theme.lineWidth
            Layout.fillHeight: true
        }

        // --- the current screen ---------------------------------------------------
        Flickable {
            id: flick

            objectName: "wizardContent"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: width
            contentHeight: pane.implicitHeight
            boundsBehavior: Flickable.StopAtBounds

            Controls.ScrollBar.vertical: Controls.ScrollBar {
            }

            Item {
                id: pane

                width: flick.width
                implicitHeight: column.implicitHeight + 2 * Kit.Theme.spaceMargin
                height: implicitHeight

                ColumnLayout {
                    id: column

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: Kit.Theme.spaceMargin
                    spacing: Kit.Theme.spaceRelated

                    Kit.Label {
                        Layout.fillWidth: true
                        visible: wizard.stepIndex < wizard.steps.length
                        text: qsTr("Step %1 of %2").arg(wizard.stepIndex + 1).arg(wizard.steps.length)
                        kind: Kit.Label.Caps
                        tone: Kit.Label.Secondary
                    }

                    Loader {
                        id: screen

                        objectName: "wizardScreen"
                        Layout.fillWidth: true
                        sourceComponent: {
                            switch (wizard.step) {
                            case "computer":
                                return computerScreen
                            case "disk":
                                return diskScreen
                            case "account":
                                return accountScreen
                            case "confirm":
                                return confirmScreen
                            case "install":
                                return progressScreen
                            case "done":
                                return doneScreen
                            default:
                                return welcomeScreen
                            }
                        }
                        onLoaded: {
                            const loaded = screen.item as WizardScreen
                            if (loaded !== null && loaded.initialFocus !== null) {
                                loaded.initialFocus.forceActiveFocus(Qt.TabFocusReason)
                                // Once the screen is laid out: the control
                                // with the keyboard is in sight (the
                                // confirmation's field is below the fold).
                                revealLater.restart()
                            }
                            Transcript.write("screen-ready name=" + wizard.step)
                        }
                    }
                }
            }
        }

        Kit.Separator {
            Layout.preferredWidth: Kit.Theme.lineWidth
            Layout.fillHeight: true
        }

        // --- the diagnostics drawer -----------------------------------------------
        Rectangle {
            Layout.preferredWidth: Math.max(wizard.drawerWidth, drawer.buttonsWidth + 2 * Kit.Theme.spaceMargin)
            Layout.fillHeight: true
            color: Kit.Theme.surfaceRaised

            DiagnosticsDrawer {
                id: drawer

                objectName: "wizardDrawer"
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Kit.Theme.spaceMargin
                engine: wizard.engine
                phase: wizard.phase()
                stepIndex: wizard.stepIndex
                steps: wizard.steps
                onLogRequested: wizard.openLog()
                onSaveRequested: wizard.openSaveLog()
            }
        }
    }

    LogSheet {
        id: logSheet

        engine: wizard.engine
    }

    SaveLogSheet {
        id: saveSheet

        engine: wizard.engine
    }

    Kit.Sheet {
        id: busySheet

        title: qsTr("TUR is being installed")

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("The installer stays open until the installation has finished. Closing it would not stop the installation, and you would not see how it ends.")
            paragraph: true
            wrapMode: Text.Wrap
        }

        buttons: [
            Kit.Button {
                text: qsTr("Keep watching")
                kind: Kit.Button.Primary
                focus: true
                onClicked: busySheet.close()
            }
        ]
    }
}

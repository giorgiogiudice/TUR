// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The strip of the screen P@x's character lives in (docs/architecture/
// tur-pax-companion.md, sections 2 to 5): as wide as the screen, its bottom
// edge the screen's, as high as what is shown. The character sits inside
// the bar; the bubble, the whole notice and the menu open above the bar,
// beside the character. Everything else of the strip is empty, and the
// program gives the compositor `hitRects` as the only places that take a
// click: a click anywhere else goes to whatever is under the strip.
//
// What the pointer and the keyboard do:
//  - a click on the character: what waits is said, if something waits;
//    otherwise the ask bar is asked for (askRequested);
//  - a right click, a long press or the menu key: the menu;
//  - a drag: the character moves along the bar and stays where it is left;
//  - the keyboard only after the person asked for it (attend()): Tab goes
//    from the character through what is open, Esc gives the keyboard back.
// The stage never takes the keyboard by itself.
Item {
    id: stage

    required property Companion companion
    required property CompanionSettings settings
    required property Placement placement

    // What is open above the character: "", bubble, notice, menu, watching
    // or settings.
    property string page: ""
    // The notice "Show me" opened.
    property var opened: ({})
    // The person asked for the keyboard.
    property bool attended: false
    // The menu and its pages close by themselves after this long without
    // the pointer on them; 0: never.
    property int idleCloseSeconds: 20

    // Being dragged along the bar, and where its box's left edge is then.
    property bool dragging: false
    property real dragX: 0
    readonly property real dragThreshold: 8

    // The row the character needs, and the height of all that is shown.
    readonly property real rowHeight: Math.ceil(stage.placement.characterLift + stage.placement.size + 4)
    readonly property real wantedHeight: stage.page === "" ? stage.rowHeight
                                         : Math.ceil(stage.placement.popupLift + leaf.height + stage.margin)
    // The only places that take a click, in the stage's own coordinates.
    readonly property var hitRects: stage.page === ""
                                    ? [Qt.rect(seat.x, seat.y, seat.width, seat.height)]
                                    : [Qt.rect(seat.x, seat.y, seat.width, seat.height),
                                       Qt.rect(leaf.x, leaf.y, leaf.width, leaf.height)]
    // The middle of the character, from the screen's left edge.
    readonly property real anchorX: seat.x + seat.width / 2
    readonly property Item characterItem: figure
    readonly property Item seatItem: seat
    readonly property Item leafItem: leaf
    readonly property var content: holder.item
    // Kept clear between the bar and what opens above it, and at the screen's edges.
    readonly property real margin: 8

    // The ask bar is wanted, beside the character.
    signal askRequested()
    // P@x's window is wanted: panel, activity or setup.
    signal windowRequested(string mode)
    signal stopRequested()
    // The person asked P@x something with a button: the words to send.
    signal askPaxRequested(string request)

    function press(): void {
        if (stage.page !== "" && stage.page !== "bubble") {
            stage.close()
            return
        }
        if (!stage.companion.speaking && stage.companion.heldCount > 0 && stage.companion.state !== "asleep") {
            stage.companion.unfold()
            return
        }
        if (stage.companion.speaking)
            stage.companion.fold()
        stage.askRequested()
    }

    function toggleMenu(): void {
        if (stage.page === "menu")
            stage.close()
        else
            stage.page = "menu"
    }

    // Back to the bubble if one is being said, else to nothing.
    function close(): void {
        stage.page = stage.companion.speaking ? "bubble" : ""
    }

    function act(action: string): void {
        const notice = stage.companion.bubble
        if (action === "show") {
            if (notice.opens === "notice") {
                // The whole notice takes the bubble's place before the
                // bubble goes, so that whoever has the keyboard keeps it.
                stage.opened = notice
                stage.page = "notice"
                stage.companion.dismiss()
            } else {
                stage.companion.dismiss()
                stage.windowRequested(notice.opens !== undefined ? String(notice.opens) : "panel")
            }
        } else if (action === "ask") {
            stage.companion.dismiss()
            stage.askPaxRequested(notice.request !== undefined ? String(notice.request) : "")
        } else {
            stage.companion.dismiss()
        }
    }

    // The person asked for the keyboard: something to go to is opened, and
    // the character has the focus first.
    function attend(): void {
        stage.attended = true
        if (stage.page === "")
            stage.page = stage.companion.speaking ? "bubble" : "menu"
        seat.forceActiveFocus(Qt.TabFocusReason)
    }

    function leave(): void {
        if (stage.page === "bubble")
            stage.companion.fold()
        stage.page = ""
        stage.attended = false
    }

    function sync(): void {
        if (stage.companion.speaking) {
            if (stage.page === "" || stage.page === "bubble")
                stage.page = "bubble"
        } else if (stage.page === "bubble") {
            stage.page = ""
        }
    }

    onPageChanged: {
        stage.companion.attending = stage.page !== "" && stage.page !== "bubble"
        if (stage.page === "" && stage.attended)
            stage.attended = false
    }

    Keys.onEscapePressed: stage.leave()

    Connections {
        target: stage.companion

        function onBubbleChanged(): void {
            stage.sync()
        }

        function onShownChanged(): void {
            if (!stage.companion.shown)
                stage.leave()
        }
    }

    Component.onCompleted: stage.sync()

    Timer {
        interval: stage.idleCloseSeconds * 1000
        running: stage.idleCloseSeconds > 0 && stage.page !== "" && stage.page !== "bubble" && !stage.attended
                 && !leafHover.hovered
        onTriggered: stage.close()
    }

    // The character's seat on the bar.
    Item {
        id: seat

        objectName: "paxSeat"
        x: stage.dragging ? stage.dragX : stage.placement.characterX
        y: stage.height - stage.placement.characterLift - stage.placement.size
        width: stage.placement.size
        height: stage.placement.size
        activeFocusOnTab: true

        Accessible.role: Accessible.Button
        Accessible.name: Words.state(stage.companion.state)
        Accessible.description: Words.press(stage.companion.state, stage.companion.heldCount) + ". "
                                + Words.characterMenu
        Accessible.focusable: true
        Accessible.onPressAction: stage.press()

        Keys.onReturnPressed: stage.press()
        Keys.onEnterPressed: stage.press()
        Keys.onSpacePressed: stage.press()
        Keys.onMenuPressed: stage.toggleMenu()

        Character {
            id: figure

            objectName: "paxCharacter"
            anchors.fill: parent
            size: seat.width
            phase: stage.companion.state
            fuel: stage.companion.fuel
            saving: stage.companion.saving
            attentive: pointer.containsMouse && !stage.dragging
        }

        Kit.FocusRing {
            shown: seat.activeFocus && stage.attended
            cornerRadius: seat.width / 2
        }

        MouseArea {
            id: pointer

            property real pressX: 0
            property bool moved: false

            objectName: "paxPointer"
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            pressAndHoldInterval: 700

            onPressed: mouse => {
                pointer.pressX = mouse.x
                pointer.moved = false
            }
            onPositionChanged: mouse => {
                if (!pointer.pressed || !(pointer.pressedButtons & Qt.LeftButton))
                    return
                const delta = mouse.x - pointer.pressX
                if (!pointer.moved && Math.abs(delta) > stage.dragThreshold) {
                    pointer.moved = true
                    stage.dragX = seat.x
                    stage.dragging = true
                }
                if (pointer.moved) {
                    const most = Math.max(stage.margin, stage.width - seat.width - stage.margin)
                    stage.dragX = Math.min(most, Math.max(stage.margin, stage.dragX + delta))
                }
            }
            onReleased: {
                if (!pointer.moved)
                    return
                // It stays where it was left, to the nearer end if close.
                stage.settings.along = stage.placement.alongAt(stage.dragX)
                stage.dragging = false
            }
            onCanceled: stage.dragging = false
            onClicked: mouse => {
                if (pointer.moved)
                    return
                if (mouse.button === Qt.RightButton)
                    stage.toggleMenu()
                else
                    stage.press()
            }
            onPressAndHold: {
                if (!pointer.moved)
                    stage.toggleMenu()
            }
        }
    }

    // What is open above the bar, beside the character. Higher than the
    // screen leaves room for, it scrolls.
    Flickable {
        id: leaf

        // Wider with larger text, so that a button keeps to one line.
        readonly property real wide: (stage.page === "bubble" ? 320 : stage.page === "menu" ? 300 : 380)
                                     * Math.min(1.6, Math.max(1, Kit.Theme.fontScale))
        readonly property real room: Math.max(120, stage.placement.screenHeight - stage.placement.popupLift
                                                   - 2 * stage.margin)

        objectName: "leaf"
        visible: stage.page !== ""
        width: Math.max(160, Math.min(leaf.wide, stage.width - 2 * stage.margin))
        height: holder.item !== null ? Math.min(leaf.room, Math.ceil(holder.height)) : 0
        x: Math.round(stage.placement.popupX(leaf.width))
        y: stage.height - stage.placement.popupLift - leaf.height
        contentWidth: leaf.width
        contentHeight: holder.height
        clip: leaf.contentHeight > leaf.height
        boundsBehavior: Flickable.StopAtBounds

        HoverHandler {
            id: leafHover
        }

        Loader {
            id: holder

            width: leaf.width
            height: holder.item !== null ? Math.ceil(holder.implicitHeight) : 0
            sourceComponent: {
                switch (stage.page) {
                case "bubble":
                    return bubblePage
                case "notice":
                    return noticePage
                case "menu":
                    return menuPage
                case "watching":
                    return watchingPage
                case "settings":
                    return settingsPage
                }
                return null
            }
        }
    }

    Component {
        id: bubblePage

        Bubble {
            objectName: "bubble"
            notice: stage.companion.bubble
            onActionTriggered: action => stage.act(action)
        }
    }

    Component {
        id: noticePage

        NoticeCard {
            objectName: "notice"
            notice: stage.opened
            paxOn: stage.companion.paxOn && stage.companion.state !== "error"
            onCloseRequested: stage.close()
            onExplainRequested: request => {
                stage.close()
                stage.askPaxRequested(request)
            }
        }
    }

    Component {
        id: menuPage

        MenuCard {
            objectName: "menu"
            quiet: stage.companion.quiet
            asleep: stage.companion.state === "asleep"
            stoppable: stage.companion.serviceState === "working" || stage.companion.serviceState === "paused"
            onQuietRequested: mode => {
                stage.companion.setQuiet(mode)
                stage.close()
            }
            onWatchingRequested: stage.page = "watching"
            onSettingsRequested: stage.page = "settings"
            onHideRequested: {
                stage.close()
                stage.settings.hidden = true
            }
            onStopRequested: {
                stage.stopRequested()
                stage.close()
            }
            onSetupRequested: {
                stage.close()
                stage.windowRequested("setup")
            }
            onCloseRequested: stage.close()
        }
    }

    Component {
        id: watchingPage

        WatchingCard {
            objectName: "watching"
            phase: stage.companion.state
            step: stage.companion.step
            problem: stage.companion.problem
            fuel: stage.companion.fuel
            fuelKnown: stage.companion.spent >= 0
            saving: stage.companion.saving
            onActivityRequested: {
                stage.close()
                stage.windowRequested("activity")
            }
            onBackRequested: stage.page = "menu"
        }
    }

    Component {
        id: settingsPage

        SettingsCard {
            objectName: "settings"
            settings: stage.settings
            onBackRequested: stage.page = "menu"
        }
    }
}

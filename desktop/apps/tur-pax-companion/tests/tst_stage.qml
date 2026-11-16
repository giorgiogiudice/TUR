// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxCompanion
import Tur.Kit as Kit

// What the pointer and the keyboard do to P@x's character, and what it never
// does by itself (docs/architecture/tur-pax-companion.md, section 5; each
// test names its rule):
//
//  R1  it never takes the keyboard by itself;
//  R2  only the character and what is open above it take a click, and what
//      is open is above the bar, never over it or over the character;
//  R6  one press opens the ask bar; what waits is said first if something
//      waits;
//  R7  a right click, a long press or the menu key opens quiet, "What is
//      P@x watching?", what it may tell, hide and stop; quiet is one press;
//  R8  it can be dragged along the bar, hidden, and brought back;
//  R9  everything is reachable with the keyboard once the person asks;
//  R10 a notice's commands are text to copy: nothing on the character runs
//      one.
Item {
    id: root

    width: 700
    height: 900

    CompanionHost {
        id: host

        anchors.left: parent.left
        anchors.top: parent.top
        width: 640
        // As high as all that is shown, with the window above it.
        height: Math.max(240, Math.ceil(host.wantedHeight) + 80)
    }

    SignalSpy {
        id: asked

        signalName: "askRequested"
    }

    SignalSpy {
        id: windows

        signalName: "windowRequested"
    }

    SignalSpy {
        id: stops

        signalName: "stopRequested"
    }

    SignalSpy {
        id: sent

        signalName: "askPaxRequested"
    }

    TestCase {
        name: "CompanionStage"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
        }

        function reach(name) {
            host.reach(host.scene(name))
            for (const spy of [asked, windows, stops, sent]) {
                spy.target = host.stage
                spy.clear()
            }
            wait(50)
            return host.stage
        }

        // The item of that objectName under `item`, through loaders, panes
        // and what scrolls.
        function find(item, name) {
            if (!item)
                return null
            if (item.objectName === name)
                return item
            const below = []
            for (const child of item.children)
                below.push(child)
            if (item.contentItem && below.indexOf(item.contentItem) < 0)
                below.push(item.contentItem)
            for (const child of below) {
                const found = find(child, name)
                if (found !== null)
                    return found
            }
            return null
        }

        function same(first, second) {
            return Math.abs(first.x - second.x) < 0.01 && Math.abs(first.y - second.y) < 0.01
                   && Math.abs(first.width - second.width) < 0.01 && Math.abs(first.height - second.height) < 0.01
        }

        function inside(item, ancestor) {
            for (let at = item; at; at = at.parent) {
                if (at === ancestor)
                    return true
            }
            return false
        }

        function press(stage, name) {
            // What was just opened is laid out on the next frame: until then
            // a button of a second row is not where it will be drawn.
            wait(50)
            const button = find(stage.leafItem, name)
            verify(button !== null, "there is a " + name)
            verify(button.visible, name + " is shown")
            mouseClick(button)
            wait(50)
        }

        // --- R6: one press ---------------------------------------------------

        function test_one_press_opens_the_ask_bar() {
            const stage = reach("idle")
            mouseClick(stage.seatItem)
            compare(asked.count, 1)
            // The bar is P@x's window's; nothing opens here.
            compare(stage.page, "")
            compare(windows.count, 0)
        }

        function test_what_waits_is_said_before_the_ask_bar() {
            const stage = reach("noticing")
            compare(host.companion.state, "noticing")
            compare(stage.page, "")
            mouseClick(stage.seatItem)
            compare(asked.count, 0)
            verify(host.companion.speaking)
            compare(stage.page, "bubble")
            // Pressed again, the bubble makes room for the ask bar and waits.
            mouseClick(stage.seatItem)
            compare(asked.count, 1)
            verify(!host.companion.speaking)
            compare(host.companion.heldCount, 1)
            compare(stage.page, "")
        }

        function test_asleep_a_press_still_reaches_pax_window() {
            const stage = reach("asleep")
            mouseClick(stage.seatItem)
            // P@x's window says that P@x is off and offers its setup.
            compare(asked.count, 1)
        }

        function test_the_bubble_makes_room_when_the_ask_bar_opens() {
            const stage = reach("telling")
            compare(stage.page, "bubble")
            // P@x's key was pressed: org.tur.PaxUi1.Mode became "ask".
            host.companion.askOpen = true
            compare(stage.page, "")
            compare(host.companion.state, "listening")
            compare(host.companion.heldCount, 1)
        }

        // --- The bubble --------------------------------------------------------

        function test_the_bubble_has_one_sentence_and_at_most_two_actions() {
            for (const name of ["telling", "bubble-fault", "bubble-refused", "bubble-limit", "bubble-suggestion"]) {
                const stage = reach(name)
                compare(stage.page, "bubble", name)
                const bubble = stage.content
                verify(bubble.sentence.length > 0 && bubble.sentence.length <= 280, name + ": one sentence")
                verify(bubble.actions.length <= 2, name + ": at most two actions")
                const actions = find(stage.leafItem, "actions")
                let buttons = 0
                for (const child of actions.children) {
                    if (child.text !== undefined && child.visible)
                        buttons++
                }
                compare(buttons, bubble.actions.length, name + ": the buttons shown")
                // Who says it is always there, and TUR is not P@x.
                const pax = find(stage.leafItem, "paxName")
                const tur = find(stage.leafItem, "senderName")
                verify(pax.visible !== tur.visible, name + ": one speaker")
                verify(tur.visible, name + ": the system speaks as TUR")
            }
        }

        function test_not_now_is_the_end_of_it() {
            const stage = reach("telling")
            press(stage, "action-dismiss")
            verify(!host.companion.speaking)
            compare(host.companion.heldCount, 0)
            compare(stage.page, "")
            compare(host.companion.state, "idle")
            compare(windows.count, 0)
        }

        function test_show_me_opens_the_whole_notice_and_runs_nothing() {
            const stage = reach("bubble-fault")
            press(stage, "action-show")
            compare(stage.page, "notice")
            verify(!host.companion.speaking)
            const title = find(stage.leafItem, "noticeTitle")
            compare(title.text, "Name lookups bypass the system resolver")
            compare(find(stage.leafItem, "code").text, "TUR-NETWORK-RESOLVER-001")
            // R10: the command is there as text that can be selected.
            const command = find(stage.leafItem, "command")
            compare(command.text, "tur-diag heal apply TUR-NETWORK-RESOLVER-001 restore-resolver-link")
            verify(command.readOnly)
            verify(find(stage.leafItem, "commandsNote").visible)
            // And no control on the card carries a command.
            const walk = item => {
                if (item.text !== undefined && item.clicked !== undefined)
                    verify(String(item.text).indexOf("tur-diag") < 0, "a button would run " + item.text)
                for (const child of item.children)
                    walk(child)
            }
            walk(stage.content)
            // Asking P@x about it is the person's press, with fixed words.
            press(stage, "explainButton")
            compare(sent.count, 1)
            verify(String(sent.signalArguments[0][0]).indexOf("TUR-NETWORK-RESOLVER-001") >= 0)
            compare(stage.page, "")
        }

        function test_show_me_on_a_task_opens_pax_window() {
            const stage = reach("bubble-refused")
            compare(host.companion.state, "refused")
            press(stage, "action-show")
            compare(windows.count, 1)
            compare(windows.signalArguments[0][0], "panel")
            verify(!host.companion.speaking)
            compare(stage.page, "")
            // Answered, the refusal is acknowledged.
            compare(host.companion.state, "idle")
        }

        // --- R7: the menu ------------------------------------------------------

        function test_a_right_click_or_a_long_press_opens_the_menu() {
            const stage = reach("idle")
            mouseClick(stage.seatItem, stage.seatItem.width / 2, stage.seatItem.height / 2, Qt.RightButton)
            compare(stage.page, "menu")
            compare(asked.count, 0)
            mouseClick(stage.seatItem, stage.seatItem.width / 2, stage.seatItem.height / 2, Qt.RightButton)
            compare(stage.page, "")

            mousePress(stage.seatItem)
            wait(900)
            mouseRelease(stage.seatItem)
            compare(stage.page, "menu")
            // A long press is not also a press.
            compare(asked.count, 0)
            // A press on the character closes the menu.
            mouseClick(stage.seatItem)
            compare(stage.page, "")
            compare(asked.count, 0)
        }

        function test_quiet_is_one_press_away_data() {
            return [
                { tag: "for an hour", button: "quietHour", until: host.noon + 3600000 },
                { tag: "until I say", button: "quietAlways", until: -1 }
            ]
        }

        function test_quiet_is_one_press_away(data) {
            const stage = reach("idle")
            stage.toggleMenu()
            press(stage, data.button)
            verify(host.companion.quiet)
            compare(host.settings.quietUntil, data.until)
            compare(stage.page, "")
            // The menu then says so, and offers to speak again.
            stage.toggleMenu()
            verify(!find(stage.leafItem, "quietHour").visible)
            press(stage, "quietOff")
            verify(!host.companion.quiet)
        }

        function test_quiet_until_tomorrow() {
            const stage = reach("idle")
            stage.toggleMenu()
            press(stage, "quietTomorrow")
            verify(host.companion.quiet)
            const until = new Date(host.settings.quietUntil)
            compare(until.getHours(), 7)
            verify(host.settings.quietUntil > host.noon)
            verify(host.settings.quietUntil - host.noon < 36 * 3600000)
        }

        function test_stop_is_there_while_pax_works_and_only_then() {
            let stage = reach("menu")
            verify(!find(stage.leafItem, "stopButton").visible)
            stage = reach("menu-working")
            press(stage, "stopButton")
            compare(stops.count, 1)
            compare(stage.page, "")
        }

        function test_what_is_pax_watching_says_what_is_read() {
            let stage = reach("idle")
            stage.toggleMenu()
            press(stage, "watchingRow")
            compare(stage.page, "watching")
            verify(find(stage.leafItem, "paxNow").text.length > 0)
            verify(find(stage.leafItem, "neverReads").text.indexOf("screen") >= 0)
            verify(!find(stage.leafItem, "paxStep").visible)
            press(stage, "activityButton")
            compare(windows.signalArguments[0][0], "activity")

            // While P@x works: the step, in the service's words.
            stage = reach("watching-working")
            verify(find(stage.leafItem, "paxStep").text.indexOf("Read the test value") >= 0)
            verify(find(stage.leafItem, "savingLine").visible)
            press(stage, "backButton")
            compare(stage.page, "menu")
        }

        function test_each_kind_of_notice_has_its_switch() {
            const stage = reach("idle")
            stage.toggleMenu()
            press(stage, "settingsRow")
            compare(stage.page, "settings")
            for (const kind of ["repair", "fault", "approval", "task", "update"]) {
                const kindSwitch = find(stage.leafItem, "kind-" + kind)
                verify(kindSwitch !== null, "a switch for " + kind)
                // On unless the person switches it off.
                verify(kindSwitch.checked, kind + " is on by default")
                mouseClick(kindSwitch)
                verify(!host.settings.kinds[kind], kind + " was switched off")
                compare(host.companion.offer({ id: "t:" + kind, kind: kind, sender: "tur", sentence: "Something." }),
                        "off")
            }
            // Suggestions are off unless the person switches them on.
            const suggestions = find(stage.leafItem, "suggestionsSwitch")
            verify(!suggestions.checked)
            compare(host.companion.suggest(host.limitHint), "off")
            mouseClick(suggestions)
            verify(host.settings.suggestions)
            // What they are made from is said beside the switch.
            verify(find(stage.leafItem, "suggestionsNote").text.indexOf("2") >= 0)
            const stay = find(stage.leafItem, "staySwitch")
            // Stepping aside is the default; staying is one switch.
            verify(!stay.checked)
            mouseClick(stay)
            verify(host.settings.overFullScreen)
            press(stage, "backButton")
            compare(stage.page, "menu")
        }

        // --- R8: dragged, hidden, brought back ---------------------------------

        function test_it_can_be_dragged_along_the_bar() {
            const stage = reach("idle")
            const seat = stage.seatItem
            const start = seat.x
            const x = start + seat.width / 2
            const y = seat.y + seat.height / 2
            compare(host.settings.along, 1)
            mousePress(stage, x, y)
            mouseMove(stage, x - 12, y)
            verify(stage.dragging)
            mouseMove(stage, x - 100, y)
            mouseMove(stage, x - 200, y)
            verify(Math.abs(seat.x - (start - 200)) <= 1, "it follows the pointer: " + seat.x)
            mouseRelease(stage, x - 200, y)
            verify(!stage.dragging)
            // It stays where it was left, and a drag is not a press.
            verify(Math.abs(seat.x - (start - 200)) <= 1, "it stays: " + seat.x)
            verify(host.settings.along < 1 && host.settings.along > 0.5)
            compare(asked.count, 0)
            compare(stage.page, "")

            // Dragged past the start of the bar: at the start, on the bar.
            const again = seat.x + seat.width / 2
            mousePress(stage, again, y)
            mouseMove(stage, again - 20, y)
            mouseMove(stage, 30, y)
            mouseMove(stage, -300, y)
            mouseRelease(stage, -300, y)
            compare(host.settings.along, 0)
            compare(seat.x, 8)
            // Never off the bar's row.
            compare(seat.y + seat.height, host.height - 2)
        }

        function test_it_can_be_hidden_and_brought_back() {
            const stage = reach("idle")
            stage.toggleMenu()
            press(stage, "hideRow")
            verify(host.settings.hidden)
            verify(!host.companion.shown)
            compare(stage.page, "")
            // org.tur.PaxCompanion1.Show, or the launcher's entry.
            host.settings.hidden = false
            verify(host.companion.shown)
        }

        // --- R2: where a click is taken ---------------------------------------

        function test_only_the_character_and_what_is_open_take_a_click() {
            let stage = reach("idle")
            compare(stage.hitRects.length, 1)
            const seat = stage.seatItem
            verify(same(stage.hitRects[0], seat), "the character's own box")
            // The row is as high as the character, not more.
            verify(stage.wantedHeight <= host.barHeight + 4)

            for (const name of ["telling", "notice", "menu", "watching", "settings"]) {
                stage = reach(name)
                compare(stage.hitRects.length, 2, name)
                const leaf = stage.leafItem
                verify(same(stage.hitRects[0], stage.seatItem), name + ": the character's own box")
                verify(same(stage.hitRects[1], leaf), name + ": what is open")
                // What is open is on the screen, above the bar, and clear of
                // the character.
                verify(leaf.x >= 8 && leaf.x + leaf.width <= host.width - 8, name + " is on the screen")
                verify(leaf.y + leaf.height <= host.height - host.barHeight - 8 + 0.5, name + " is above the bar")
                verify(leaf.y + leaf.height <= stage.seatItem.y, name + " is clear of the character")
                verify(leaf.y >= 0, name + " fits the strip")
                verify(leaf.height > 40, name + " is drawn")
            }
        }

        function test_the_character_sits_inside_the_bar() {
            for (const name of ["idle", "telling", "bubble-at-the-start"]) {
                const stage = reach(name)
                const seat = stage.seatItem
                verify(seat.y >= host.height - host.barHeight, name + ": below the bar's top edge")
                verify(seat.y + seat.height <= host.height, name + ": above the screen's edge")
                verify(seat.x >= 8 && seat.x + seat.width <= host.width - 8)
            }
            // At the start of the bar, what opens stays on the screen.
            const stage = reach("bubble-at-the-start")
            compare(stage.seatItem.x, 8)
            compare(stage.leafItem.x, 8)
        }

        // --- R1 and R9: the keyboard ------------------------------------------

        function test_it_never_takes_the_keyboard_by_itself() {
            for (const name of ["idle", "telling", "bubble-fault", "bubble-suggestion", "acting", "waiting"]) {
                const stage = reach(name)
                wait(50)
                verify(!stage.attended, name)
                const focus = root.Window.activeFocusItem
                verify(!inside(focus, stage), name + " took the keyboard")
            }
        }

        function test_asked_for_everything_is_reachable_with_the_keyboard() {
            let stage = reach("bubble-fault")
            stage.attend()
            verify(stage.attended)
            verify(stage.seatItem.activeFocus)
            // Tab goes from the character into the bubble.
            keyClick(Qt.Key_Tab)
            let focus = root.Window.activeFocusItem
            verify(inside(focus, stage.leafItem), "Tab went into the bubble")
            // Esc gives the keyboard back; the bubble waits as a mark.
            keyClick(Qt.Key_Escape)
            verify(!stage.attended)
            compare(stage.page, "")
            compare(host.companion.heldCount, 1)

            // With nothing being said, the menu is what the keyboard gets.
            stage = reach("idle")
            stage.attend()
            compare(stage.page, "menu")
            verify(stage.seatItem.activeFocus)
            keyClick(Qt.Key_Tab)
            verify(inside(root.Window.activeFocusItem, stage.leafItem))
            keyClick(Qt.Key_Escape)
            compare(stage.page, "")
            verify(!stage.attended)

            // On the character itself: Return presses it, the menu key opens
            // the menu.
            stage.attend()
            stage.page = ""
            stage.attended = true
            stage.seatItem.forceActiveFocus()
            keyClick(Qt.Key_Menu)
            compare(stage.page, "menu")
            stage.close()
            stage.seatItem.forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(asked.count, 1)
        }

        // --- Honest signals ------------------------------------------------------

        function test_the_flame_shows_the_days_limit_and_the_battery() {
            let stage = reach("low-fuel")
            verify(Math.abs(stage.characterItem.fuel - 0.08) < 0.001)
            stage = reach("on-battery")
            verify(stage.characterItem.saving)
            verify(!stage.characterItem.looping)
            stage = reach("acting")
            compare(stage.characterItem.phase, "acting")
            verify(stage.characterItem.loud)
        }
    }
}

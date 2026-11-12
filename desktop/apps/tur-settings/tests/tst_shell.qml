// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Settings
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The shell (desktop-experience.md, 10.1 and 10.2): the list of pages as the
// registry has it, the search field, one page at a time, the way into KDE
// System Settings, and the address shown in developer mode only.
Item {
    id: root

    width: 1080
    height: 1000

    SettingsStage {
        id: host

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: root.height
    }

    SignalSpy {
        id: asked

        signalName: "showRequested"
    }

    TestCase {
        name: "Shell"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            asked.clear()
            asked.target = null
            host.stop()
            host.height = root.height
            settingsHelperRecord.forget()
            wait(0)
        }

        function view(shell) {
            return findChild(shell, "shellView").item
        }

        function type(text) {
            for (const letter of text)
                keyClick(letter)
        }

        // What the stand-in for KDE System Settings was started with, once
        // it has been.
        function helperArguments() {
            let seen
            tryVerify(() => {
                seen = Testing.Harness.readJson(settingsHelperOut)
                return seen !== undefined
            }, 10000, "the stand-in for System Settings was started")
            return seen
        }

        function test_it_opens_at_overview() {
            const shell = host.start("default")
            compare(shell.page, "tur.overview")
            compare(shell.address, "tur.overview")
            verify(view(shell).isSettingsPage)
            compare(view(shell).address, "tur.overview")
            compare(findChild(view(shell), "pageTitle").text, "Overview")
            verify(findChild(shell, "shellPage-tur.overview").selected)
            // Nothing is highlighted and nothing is refused.
            compare(shell.marked, null)
            verify(!findChild(shell, "shellFailure").visible)
            // The address is not shown: this is not developer mode.
            verify(!findChild(shell, "shellAddress").visible)
        }

        // The list is the design's (10.2), in its order. What this task
        // builds can be opened; the rest is listed and cannot.
        function test_the_side_list_is_the_design_s() {
            const shell = host.start("default")
            const expected = [
                ["tur.overview", "Overview", true, ""], ["tur.updates", "Updates", false, ""],
                ["tur.recovery", "Recovery", false, ""], ["tur.apps", "Applications", false, ""],
                ["tur.windows", "Windows applications", false, ""], ["tur.android", "Android", false, ""],
                ["tur.storage", "Storage", false, ""], ["tur.devices", "Devices", false, ""],
                ["tur.pax.general", "General", false, "P@x"], ["tur.pax.voice", "Voice", false, "P@x"],
                ["tur.pax.privacy", "Privacy", false, "P@x"], ["tur.pax.approvals", "Approvals", false, "P@x"],
                ["tur.pax.memory", "Memory", false, "P@x"], ["tur.pax.schedules", "Schedules", false, "P@x"],
                ["tur.pax.spending", "Spending", false, "P@x"], ["tur.pax.activity", "Activity", false, "P@x"],
                ["tur.diagnostics", "Diagnostics", false, ""], ["tur.appearance", "Appearance", true, ""],
                ["tur.about", "About", true, ""]
            ]
            const listed = Registry.navigation.filter(item => !item.header)
            compare(listed.length, expected.length)

            // P@x's pages stand under their heading, which is not a page and
            // is closed until asked for: the assistant is not put forward.
            const heading = findChild(shell, "shellGroup-P@x")
            verify(heading !== null && heading.visible)
            compare(heading.text, "P@x")
            verify(!heading.expanded)
            for (const [address, title, built, group] of expected) {
                const row = findChild(shell, "shellPage-" + address)
                verify(row !== null, "the list has " + title)
                compare(row.visible, group.length === 0, title)
            }

            mouseClick(heading)
            verify(heading.expanded)
            // The rows take their places before the next frame.
            wait(50)
            let previous = -1
            for (let i = 0; i < expected.length; i++) {
                const [address, title, built, group] = expected[i]
                compare(listed[i].address, address)
                compare(listed[i].group, group)
                const row = findChild(shell, "shellPage-" + address)
                verify(row.visible, title + " is shown")
                compare(row.text, title)
                compare(row.enabled, built, title)
                // In the order of the list, down the window.
                const top = Testing.Harness.sceneRect(row).y
                verify(top > previous, title + " is below the page before it")
                previous = top
            }
            const headingTop = Testing.Harness.sceneRect(heading).y
            verify(headingTop < Testing.Harness.sceneRect(findChild(shell, "shellPage-tur.pax.general")).y)
            verify(headingTop > Testing.Harness.sceneRect(findChild(shell, "shellPage-tur.devices")).y)

            mouseClick(heading)
            verify(!heading.expanded)
            verify(!findChild(shell, "shellPage-tur.pax.privacy").visible)
            wait(50)

            // A page that is shown only while its feature is on is not listed.
            compare(findChild(shell, "shellPage-tur.macos"), null)
            // And at the end, under the list, the way into KDE's settings.
            const kde = findChild(shell, "shellSystemSettings")
            compare(kde.text, "Displays, sound, network and more")
            verify(Testing.Harness.sceneRect(kde).y > Testing.Harness.sceneRect(findChild(shell, "shellPage-tur.about")).y)
        }

        // In a window too low for it the list scrolls, and what the keyboard
        // reaches is brought into sight.
        function test_the_list_scrolls_in_a_low_window() {
            host.height = 300
            const shell = host.start("default")
            wait(50)
            const list = findChild(shell, "shellList")
            verify(list.contentHeight > list.height, "the list is longer than its room")
            compare(list.contentY, 0)
            const about = findChild(shell, "shellPage-tur.about")
            about.forceActiveFocus(Qt.TabFocusReason)
            tryVerify(() => list.contentY > 0, 2000, "the list scrolled to the page that has the keyboard")
            wait(50)
            const rect = Testing.Harness.sceneRect(about)
            const room = Testing.Harness.sceneRect(list)
            verify(rect.y >= room.y && rect.y + rect.height <= room.y + room.height, "About is in sight")
            // The way into System Settings did not move out of the window.
            const kde = Testing.Harness.sceneRect(findChild(shell, "shellSystemSettings"))
            verify(kde.y + kde.height <= host.height)
        }

        function test_choosing_a_page_shows_it() {
            const shell = host.start("default")
            mouseClick(findChild(shell, "shellPage-tur.appearance"))
            compare(shell.page, "tur.appearance")
            compare(shell.address, "tur.appearance")
            compare(host.service.currentAddress, "tur.appearance")
            compare(view(shell).address, "tur.appearance")
            verify(findChild(shell, "shellPage-tur.appearance").selected)
            verify(!findChild(shell, "shellPage-tur.overview").selected)

            // A page that is not built cannot be gone to.
            mouseClick(findChild(shell, "shellPage-tur.updates"))
            compare(shell.page, "tur.appearance")

            mouseClick(findChild(shell, "shellPage-tur.about"))
            compare(view(shell).address, "tur.about")
        }

        function test_the_arrows_move_between_the_pages_that_can_be_opened() {
            const shell = host.start("default")
            const overview = findChild(shell, "shellPage-tur.overview")
            overview.forceActiveFocus(Qt.TabFocusReason)
            // Down skips what cannot be opened, and stops at the heading of
            // a group, which can be.
            keyClick(Qt.Key_Down)
            verify(findChild(shell, "shellGroup-P@x").activeFocus)
            keyClick(Qt.Key_Down)
            verify(findChild(shell, "shellPage-tur.appearance").activeFocus)
            keyClick(Qt.Key_Space)
            compare(shell.page, "tur.appearance")
            keyClick(Qt.Key_Down)
            verify(findChild(shell, "shellPage-tur.about").activeFocus)
            // Not out of the list of pages.
            keyClick(Qt.Key_Down)
            verify(findChild(shell, "shellPage-tur.about").activeFocus)
            keyClick(Qt.Key_Up)
            keyClick(Qt.Key_Up)
            keyClick(Qt.Key_Up)
            verify(overview.activeFocus)
            keyClick(Qt.Key_Up)
            verify(overview.activeFocus)
        }

        function test_the_last_row_opens_system_settings() {
            const shell = host.start("default", { systemSettingsProgram: settingsHelper })
            mouseClick(findChild(shell, "shellSystemSettings"))
            // System Settings where it last was: no module is named.
            compare(helperArguments(), [])
            verify(!findChild(shell, "shellFailure").visible)
        }

        function test_it_says_when_system_settings_cannot_be_started() {
            const shell = host.start("default")
            mouseClick(findChild(shell, "shellSystemSettings"))
            const message = findChild(shell, "shellFailure")
            verify(message.visible)
            compare(message.code, "TUR-DESKTOP-SETTINGS-005")
            compare(message.title, "The program that holds this setting could not be opened.")
            compare(message.reason, "KDE System Settings could not be started.")
            verify(message.technicalDetail.indexOf("/nonexistent/systemsettings") >= 0)
            // The page is still there, under the message.
            compare(view(shell).address, "tur.overview")

            // The message can be put away.
            mouseClick(findChild(message, "messageAction0"))
            verify(!message.visible)
        }

        function test_the_address_is_shown_in_developer_mode_only() {
            const shell = host.start("default", { developer: true })
            const foot = findChild(shell, "shellAddress")
            verify(foot.visible)
            compare(foot.text, "tur.overview")
            verify(host.service.go("tur.appearance#variant").ok)
            compare(foot.text, "tur.appearance#variant")
            shell.clearHighlight()
            compare(foot.text, "tur.appearance")
        }

        function test_typing_searches_once_the_keys_pause() {
            const shell = host.start("default")
            shell.debounce = 300
            shell.focusSearch()
            verify(findChild(shell, "shellSearch").activeFocus)

            type("dark")
            // Nothing happens until the keys pause.
            compare(shell.query, "")
            verify(view(shell).isSettingsPage)
            tryCompare(shell, "query", "dark", 5000)
            compare(shell.searching, true)
            // The matches stand where the page was; no page is selected and
            // the shell has no address.
            compare(view(shell).isSettingsPage, undefined)
            compare(findChild(shell, "searchTitle").text, "Settings that match “dark”")
            verify(!findChild(shell, "shellPage-tur.overview").selected)
            compare(shell.address, "")
            verify(shell.hits.length > 0)
            compare(shell.hits[0].address, "tur.appearance#variant")
        }

        function test_enter_searches_without_waiting() {
            const shell = host.start("default")
            shell.debounce = 60000
            shell.focusSearch()
            type("fonts")
            compare(shell.query, "")
            keyClick(Qt.Key_Return)
            compare(shell.query, "fonts")
            verify(findChild(shell, "searchHits").visible)
        }

        function test_a_match_says_where_it_is_and_leads_there() {
            const shell = host.start("default")
            shell.search("version")
            const hit = findChild(shell, "searchHit-tur.about#version")
            verify(hit !== null, "the version on About is found")
            compare(hit.text, "Version")
            compare(hit.subtitle, "About › This system")
            // The same title on Overview is another match, with its own place.
            compare(findChild(shell, "searchHit-tur.overview#version").subtitle, "Overview › This system")

            asked.target = host.service
            mouseClick(hit)
            // The window was asked once, for the match that was pressed.
            compare(asked.count, 1)
            compare(asked.signalArguments[0][0] + "#" + asked.signalArguments[0][1], "tur.about#version")
            compare(shell.page, "tur.about")
            // The search is over; the page is shown and the row highlighted,
            // in the system's blue: it was a person who asked.
            compare(shell.searching, false)
            compare(findChild(shell, "shellSearch").text, "")
            compare(view(shell).address, "tur.about")
            compare(shell.address, "tur.about#version")
            verify(shell.marked !== null)
            compare(shell.marked.address, "tur.about#version")
            verify(shell.marked.marked)
            verify(!shell.marked.markedByPax)
            verify(findChild(shell, "shellPage-tur.about").selected)
        }

        function test_a_match_works_from_the_keyboard() {
            const shell = host.start("default")
            shell.search("night light")
            const hit = findChild(shell, "searchHit-tur.appearance#night-light")
            verify(hit !== null)
            hit.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(shell.address, "tur.appearance#night-light")
        }

        function test_a_page_of_system_settings_is_found_and_opened_there() {
            const shell = host.start("default", { systemSettingsProgram: settingsHelper })
            shell.search("fonts")
            // TUR's own row first, then the page of KDE's that it leads to.
            compare(shell.hits[0].address, "tur.appearance#fonts")
            const kde = findChild(shell, "searchHit-kde.kcm_fonts")
            verify(kde !== null, "the Fonts page of System Settings is found")
            compare(kde.subtitle, "System Settings › Fonts")
            mouseClick(kde)
            compare(helperArguments(), ["kcm_fonts"])
            verify(!findChild(shell, "shellFailure").visible)
        }

        function test_nothing_found_says_where_the_rest_is() {
            const shell = host.start("default", { systemSettingsProgram: settingsHelper })
            shell.search("printer")
            compare(shell.hits.length, 0)
            verify(!findChild(shell, "searchHits").visible)
            const nothing = findChild(shell, "searchNothing")
            verify(nothing.visible)
            // It does not say that there is no such setting: most of KDE's
            // are not in a registry.
            compare(nothing.text, "None of TUR's own settings matches. Displays, sound, network, printers and most other "
                    + "settings are in System Settings.")
            mouseClick(findChild(shell, "searchOpenSystemSettings"))
            compare(helperArguments(), [])
        }

        function test_clearing_the_field_brings_the_page_back() {
            const shell = host.start("default")
            verify(host.service.go("tur.about").ok)
            shell.focusSearch()
            type("dark")
            tryCompare(shell, "searching", true, 5000)
            for (let i = 0; i < 4; i++)
                keyClick(Qt.Key_Backspace)
            tryCompare(shell, "searching", false, 5000)
            compare(view(shell).address, "tur.about")
            compare(shell.address, "tur.about")
        }

        function test_searching_puts_a_highlight_away() {
            const shell = host.start("default")
            verify(host.service.go("tur.about#version").ok)
            verify(shell.marked !== null)
            shell.search("dark")
            compare(shell.marked, null)
            compare(shell.address, "")
        }

        function test_escape_puts_a_highlight_away() {
            const shell = host.start("default")
            verify(host.service.go("tur.appearance#current").ok)
            const row = shell.marked
            verify(row !== null && row.marked)
            keyClick(Qt.Key_Escape)
            compare(shell.marked, null)
            verify(!row.marked)
            compare(shell.address, "tur.appearance")
        }

        function test_what_the_registry_cannot_load_is_reported() {
            host.start("default")
            // The registries the program carries load without a complaint.
            compare(Registry.problems, [])
        }
    }
}

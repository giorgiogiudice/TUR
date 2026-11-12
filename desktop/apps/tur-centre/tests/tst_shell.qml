// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The shell (desktop-experience.md, section 8.1): the side list of seven
// views, the search field in the header, one content area, the addresses,
// and the way from a search to an application's page and back.
Item {
    id: root

    width: 1080
    height: 900

    CentreHost {
        id: host

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: root.height
    }

    SignalSpy {
        id: spy
    }

    TestCase {
        name: "Shell"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            host.height = root.height
            spy.clear()
            spy.target = null
            wait(0)
        }

        function settle() {
            tryVerify(() => host.settled, 15000, "every answer that was asked for has arrived")
            wait(0)
        }

        function view(shell) {
            return findChild(shell, "shellView").item
        }

        function type(text) {
            for (const letter of text)
                keyClick(letter)
        }

        function test_it_opens_at_home_with_the_search_field_focused() {
            const shell = host.start("default", "")
            shell.focusSearch()
            compare(shell.view, "home")
            compare(shell.address, "tur-centre:home")
            const field = findChild(shell, "shellSearch")
            verify(field.activeFocus, "the search field has the keyboard when the window opens")
            compare(findChild(shell, "shellTitle").text, "Application centre")

            // Home carries its standing sentence, word for word.
            const sentence = findChild(shell, "homeStandingSentence")
            verify(sentence !== null && sentence.visible)
            compare(sentence.text,
                    "TUR has not tested applications itself yet. Every rating here is what the named source reports.")
            // Nothing was asked of the catalogue to show Home.
            compare(host.catalogue.search.status, SearchSession.Idle)
            compare(host.catalogue.application.status, ApplicationSession.Idle)
        }

        function test_the_side_list_has_the_design_s_seven_views() {
            const shell = host.start("default", "")
            const expected = [
                ["home", "Home", true], ["search", "Search", true], ["categories", "Categories", false],
                ["installed", "Installed", false], ["updates", "Updates", false], ["sources", "Sources", false],
                ["open-a-file", "Open a file", false]
            ]
            compare(shell.sections.length, 7)
            for (const [key, title, built] of expected) {
                const row = findChild(shell, "shellSection-" + key)
                verify(row !== null, "the side list has " + title)
                compare(row.text, title)
                // What part 1 does not build is listed, and not available.
                compare(row.enabled, built, title)
                compare(row.selected, key === "home")
            }

            mouseClick(findChild(shell, "shellSection-search"))
            compare(shell.view, "search")
            verify(findChild(shell, "shellSection-search").selected)
            verify(!findChild(shell, "shellSection-home").selected)
            compare(shell.address, "tur-centre:search")

            // A view that is not built cannot be gone to.
            mouseClick(findChild(shell, "shellSection-sources"))
            compare(shell.view, "search")

            mouseClick(findChild(shell, "shellSection-home"))
            compare(shell.view, "home")
        }

        function test_typing_searches_once_the_keys_pause() {
            const shell = host.start("default", "")
            shell.debounce = 300
            shell.focusSearch()
            spy.target = host.catalogue.search
            spy.signalName = "queryChanged"

            type("office")
            // Typing goes to the search view at once, and asks nothing yet.
            compare(shell.view, "search")
            compare(findChild(shell, "shellSearch").text, "office")
            compare(host.catalogue.search.query, "")
            compare(host.catalogue.search.status, SearchSession.Idle)

            // One question for the six keys, not six.
            tryCompare(host.catalogue.search, "query", "office", 5000)
            compare(spy.count, 1)
            settle()
            compare(host.catalogue.search.status, SearchSession.Results)
            compare(shell.address, "tur-centre:search?q=office")
        }

        function test_enter_searches_without_waiting() {
            const shell = host.start("default", "")
            shell.debounce = 60000
            shell.focusSearch()
            type("krita")
            compare(host.catalogue.search.query, "")
            keyClick(Qt.Key_Return)
            compare(host.catalogue.search.query, "krita")
            settle()
            compare(host.catalogue.search.hits.count, 1)
        }

        function test_clearing_the_field_clears_the_results() {
            const shell = host.start("default", "tur-centre:search?q=office")
            settle()
            compare(host.catalogue.search.hits.count, 2)
            shell.focusSearch()
            const field = findChild(shell, "shellSearch")
            compare(field.text, "office")
            for (let i = 0; i < 6; i++)
                keyClick(Qt.Key_Backspace)
            compare(field.text, "")
            tryCompare(host.catalogue.search, "status", SearchSession.Idle, 5000)
            compare(host.catalogue.search.hits.count, 0)
            // Still the search view: the person is searching.
            compare(shell.view, "search")
        }

        function test_every_view_has_an_address_data() {
            return [
                { tag: "home", address: "tur-centre:home", view: "home", back: "tur-centre:home" },
                { tag: "search", address: "tur-centre:search", view: "search", back: "tur-centre:search" },
                { tag: "search for words", address: "tur-centre:search?q=ms%20office", view: "search",
                  query: "ms office", back: "tur-centre:search?q=ms%20office" },
                { tag: "search, plus for a space", address: "tur-centre:search?q=ms+office", view: "search",
                  query: "ms office", back: "tur-centre:search?q=ms%20office" },
                { tag: "an application", address: "tur-centre:app/com.microsoft.office", view: "application",
                  id: "com.microsoft.office", back: "tur-centre:app/com.microsoft.office" },
                { tag: "a class of applications", address: "tur-centre:app/tur.class.android-banking",
                  view: "application", id: "tur.class.android-banking",
                  back: "tur-centre:app/tur.class.android-banking" }
            ]
        }

        function test_every_view_has_an_address(data) {
            const shell = host.start("default", "")
            verify(shell.open(data.address), data.address + " is an address")
            compare(shell.view, data.view)
            if (data.query !== undefined) {
                compare(host.catalogue.search.query, data.query)
                compare(findChild(shell, "shellSearch").text, data.query)
            }
            if (data.id !== undefined)
                compare(host.catalogue.application.applicationId, data.id)
            // And the shell says where it is, in the same form.
            compare(shell.address, data.back)
            settle()
        }

        function test_what_is_not_an_address_is_refused_data() {
            return [
                { tag: "empty", address: "" },
                { tag: "a web address", address: "https://example.org/" },
                { tag: "a file", address: "/usr/bin/true" },
                { tag: "an application with no identifier", address: "tur-centre:app/" },
                { tag: "an application with a path", address: "tur-centre:app/../../etc/passwd" },
                { tag: "badly encoded", address: "tur-centre:search?q=%E0%A4%A" },
                { tag: "a view that is not built", address: "tur-centre:sources/google-play" },
                { tag: "another program", address: "tur-settings:tur.pax.privacy" }
            ]
        }

        function test_what_is_not_an_address_is_refused(data) {
            const shell = host.start("default", "tur-centre:search?q=krita")
            settle()
            verify(!shell.open(data.address), "\"" + data.address + "\" is not an address")
            // The shell stays where it was, and asks nothing.
            compare(shell.view, "search")
            compare(host.catalogue.search.query, "krita")
            compare(host.catalogue.application.applicationId, "")
        }

        function test_details_opens_the_page_and_back_returns_to_the_results() {
            const shell = host.start("default", "tur-centre:search?q=office")
            settle()
            verify(findChild(shell, "shellSide").visible)
            verify(!findChild(shell, "shellBack").visible)

            const details = findChild(view(shell), "rowDetails")
            verify(details !== null)
            mouseClick(details)
            compare(shell.view, "application")
            compare(host.catalogue.application.applicationId, "com.microsoft.office")
            settle()

            // The page takes the whole width; the way back is where the
            // title was, and says where it leads.
            verify(!findChild(shell, "shellSide").visible)
            verify(!findChild(shell, "shellTitle").visible)
            const back = findChild(shell, "shellBack")
            verify(back.visible)
            compare(back.text, "Results")
            // The search field stays, with what was searched for.
            const field = findChild(shell, "shellSearch")
            verify(field.visible)
            compare(field.text, "office")
            compare(findChild(view(shell), "applicationName").text, "Microsoft 365")

            mouseClick(back)
            compare(shell.view, "search")
            compare(host.catalogue.search.status, SearchSession.Results)
            compare(host.catalogue.search.hits.count, 2)
            verify(findChild(shell, "shellSide").visible)
        }

        function test_back_leads_home_when_there_are_no_results() {
            const shell = host.start("default", "tur-centre:app/org.kde.krita")
            settle()
            const back = findChild(shell, "shellBack")
            compare(back.text, "Home")
            mouseClick(back)
            compare(shell.view, "home")
        }

        function test_back_and_details_work_from_the_keyboard() {
            const shell = host.start("default", "tur-centre:search?q=krita")
            settle()
            const details = findChild(view(shell), "rowDetails")
            details.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(shell.view, "application")
            settle()
            const back = findChild(shell, "shellBack")
            back.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(shell.view, "search")
        }

        function test_typing_on_a_page_leads_to_the_search() {
            const shell = host.start("default", "tur-centre:app/org.kde.krita")
            settle()
            shell.focusSearch()
            type("valorant")
            compare(shell.view, "search")
            tryCompare(host.catalogue.search, "query", "valorant", 5000)
            settle()
            compare(host.catalogue.search.hits.count, 1)
        }

        function test_what_has_the_keyboard_is_kept_in_sight() {
            // A window too short for the page (13.1, X2).
            host.height = 420
            const shell = host.start("default", "tur-centre:app/com.microsoft.office")
            settle()
            findChild(shell, "applicationDetails").expanded = true
            wait(50)

            const content = findChild(shell, "shellContent")
            verify(content.contentHeight > content.height, "the page is longer than the window")
            compare(content.contentY, 0)

            const stops = Testing.Harness.tabStops(content)
            verify(stops.length >= 3)
            const frame = Testing.Harness.sceneRect(content)
            let scrolled = false
            for (const stop of stops) {
                stop.forceActiveFocus(Qt.TabFocusReason)
                wait(0)
                const rect = Testing.Harness.sceneRect(stop)
                verify(rect.y >= frame.y - 1 && rect.y + rect.height <= frame.y + frame.height + 1,
                       Testing.Harness.describe(stop) + " has the keyboard and is in sight (from " + rect.y + " to "
                       + (rect.y + rect.height) + " in a view from " + frame.y + " to " + (frame.y + frame.height) + ")")
                scrolled = scrolled || content.contentY > 0
            }
            verify(scrolled, "the page was scrolled to do it")

            // Going to another view starts at its top.
            shell.showHome()
            compare(content.contentY, 0)
        }

        function test_a_link_is_handed_to_the_browser_and_nothing_is_fetched() {
            const shell = host.start("default", "tur-centre:search?q=photothing")
            settle()
            spy.target = shell
            spy.signalName = "urlRequested"
            mouseClick(findChild(view(shell), "searchTellProject"))
            compare(spy.count, 1)
            compare(spy.signalArguments[0][0], "https://github.com/giorgiogiudice/TUR/issues")
        }
    }
}

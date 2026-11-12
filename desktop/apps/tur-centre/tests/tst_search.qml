// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// Search (desktop-experience.md, section 8.3), in each of its states, on the
// router's recorded answers for the seed catalogue. Every word asserted here
// is the design's fixed vocabulary or the router's own text.
Item {
    id: root

    width: 1080
    height: 1400

    CentreHost {
        id: host

        anchors.fill: parent
    }

    SignalSpy {
        id: spy
    }

    TestCase {
        name: "Search"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
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

        // Every item under `item`, itself included, in the order of the tree.
        function everything(item) {
            const out = [item]
            for (let i = 0; i < item.children.length; i++)
                out.push(...everything(item.children[i]))
            return out
        }

        // The rows of the result list, top to bottom.
        function rows(shell) {
            return everything(view(shell)).filter(item => item.objectName === "rowName")
                                          .map(item => item.parent.parent.parent)
        }

        // The text of everything under `item` that shows text.
        function texts(item) {
            return everything(item).filter(part => part.text !== undefined && part.visible).map(part => part.text)
        }

        function shown(row, name) {
            const item = findChild(row, name)
            return item !== null && item.visible
        }

        function test_nothing_asked_shows_nothing() {
            const shell = host.start("default", "tur-centre:search")
            compare(host.catalogue.search.status, SearchSession.Idle)
            const page = view(shell)
            verify(!findChild(page, "searchResults").visible)
            verify(!findChild(page, "searchNothing").visible)
            verify(!findChild(page, "searchBusy").visible)
            verify(!findChild(page, "searchFailure").visible)
        }

        function test_results_are_the_catalogue_s_rows_in_its_order() {
            const shell = host.start("default", "tur-centre:search?q=office")
            settle()
            const page = view(shell)
            verify(findChild(page, "searchResults").visible)
            verify(!findChild(page, "searchNothing").visible)
            compare(findChild(page, "searchCount").text, "2 applications in TUR's catalogue for \"office\"")

            const found = rows(shell)
            compare(found.length, 2)

            // Microsoft 365 as the seed catalogue records it and the router
            // routes it: one row, the route the router recommends, and that
            // route's rating. Nobody has reported on it, and the row says so.
            const office = found[0]
            compare(findChild(office, "rowName").text, "Microsoft 365")
            compare(findChild(office, "rowPublisher").text,
                    "Microsoft · Office suite: Word, Excel, PowerPoint and others.")
            compare(findChild(office, "rowRoute").text, "Windows application, in Windows")
            verify(shown(office, "rowRating"))
            compare(findChild(findChild(office, "rowRating"), "ratingWords").text, "No reports")
            verify(!shown(office, "rowNoRoute"))
            // What is needed first, in the router's own sentences.
            verify(shown(office, "rowNeeded"))
            compare(texts(findChild(office, "rowNeeded")),
                    ["Needed first", "The Windows guest must be set up.",
                     "You must supply a Windows licence: TUR does not include one."])
            compare(findChild(office, "rowDetails").text, "Details")

            const libre = found[1]
            compare(findChild(libre, "rowName").text, "LibreOffice")
            compare(findChild(libre, "rowRoute").text, "Linux application")
            compare(findChild(findChild(libre, "rowRating"), "ratingWords").text, "No reports")
            verify(!shown(libre, "rowNeeded"))
        }

        function test_one_result_is_counted_as_one() {
            const shell = host.start("default", "tur-centre:search?q=krita")
            settle()
            compare(findChild(view(shell), "searchCount").text, "1 application in TUR's catalogue for \"krita\"")
            compare(rows(shell).length, 1)
        }

        function test_other_names_and_typing_mistakes_find_it() {
            // "MS Office" finds Microsoft 365: the catalogue's search knows
            // other names and forgives typing mistakes.
            const shell = host.start("default", "tur-centre:search?q=ms%20office")
            settle()
            compare(rows(shell).length, 1)
            compare(findChild(rows(shell)[0], "rowName").text, "Microsoft 365")

            shell.showSearch("ofice")
            settle()
            compare(rows(shell).length, 1)
            compare(findChild(rows(shell)[0], "rowName").text, "Microsoft 365")
            compare(findChild(view(shell), "searchCount").text, "1 application in TUR's catalogue for \"ofice\"")
        }

        function test_an_application_that_cannot_run_is_listed_and_says_so() {
            const shell = host.start("default", "tur-centre:search?q=valorant")
            settle()
            const found = rows(shell)
            compare(found.length, 1)
            const valorant = found[0]
            compare(findChild(valorant, "rowName").text, "Valorant")
            verify(shown(valorant, "rowNoRoute"))
            // Not a route, and not a rating: "no reports" beside "cannot
            // run" would say less than is known.
            verify(!shown(valorant, "rowRoute"))
            verify(!shown(valorant, "rowRating"))
            verify(!shown(valorant, "rowNeeded"))
            // It still has a page, which says why.
            mouseClick(findChild(valorant, "rowDetails"))
            compare(shell.view, "application")
            compare(host.catalogue.application.applicationId, "com.riotgames.valorant")
            settle()
        }

        function test_a_class_of_applications_is_found_by_what_people_call_it() {
            const shell = host.start("default", "tur-centre:search?q=banking%20app")
            settle()
            const found = rows(shell)
            compare(found.length, 1)
            compare(findChild(found[0], "rowName").text, "Banking and payment applications for Android")
            verify(shown(found[0], "rowNoRoute"))
        }

        function test_nothing_found_says_so_and_offers_only_what_exists() {
            const shell = host.start("default", "tur-centre:search?q=photothing")
            settle()
            compare(host.catalogue.search.status, SearchSession.NothingFound)
            const page = view(shell)
            verify(findChild(page, "searchNothing").visible)
            verify(!findChild(page, "searchResults").visible)
            verify(!findChild(page, "searchFailure").visible)
            compare(findChild(page, "searchNothingTitle").text, "Nothing found for \"photothing\".")
            const tell = findChild(page, "searchTellProject")
            verify(tell.visible && tell.enabled)
            compare(tell.text, "Tell the project you need it")
            // "Open a file" is not offered: TUR cannot look at a file yet.
            compare(texts(findChild(page, "searchNothing")).filter(text => text.indexOf("file") >= 0), [])
        }

        function test_a_search_that_is_under_way_shows_that_it_is() {
            // The stand-in program never answers a search for "sleep".
            const shell = host.startOnHelper("tur-centre:search?q=sleep", 60000)
            compare(host.catalogue.search.status, SearchSession.Searching)
            wait(100)
            const page = view(shell)
            const busy = findChild(page, "searchBusy")
            verify(busy.visible)
            compare(busy.label, "Searching the catalogue")
            verify(!findChild(page, "searchResults").visible)
            verify(!findChild(page, "searchNothing").visible)
            // The window is not held up by it: another view can be gone to.
            shell.showHome()
            compare(shell.view, "home")
        }

        function test_the_last_answer_stays_while_the_next_is_awaited() {
            const shell = host.startOnHelper("", 60000)
            // The stand-in echoes its arguments for anything but its special
            // words, which is not a search's answer; so use a fixture for
            // the first answer and the stand-in only for the wait.
            host.catalogue.fixture = centreFixtures + "/default"
            shell.showSearch("office")
            settle()
            compare(rows(shell).length, 2)

            host.catalogue.fixture = ""
            shell.showSearch("sleep")
            compare(host.catalogue.search.status, SearchSession.Searching)
            wait(100)
            const page = view(shell)
            // The rows that are there still answer "office", and say so.
            verify(findChild(page, "searchResults").visible)
            compare(findChild(page, "searchCount").text, "2 applications in TUR's catalogue for \"office\"")
            verify(!findChild(page, "searchBusy").visible)
        }

        function test_a_failure_is_the_two_layer_message_with_its_code() {
            // No catalogue program at all.
            const shell = host.start("", "tur-centre:search?q=office")
            settle()
            compare(host.catalogue.search.status, SearchSession.Failed)
            const page = view(shell)
            verify(!findChild(page, "searchResults").visible)
            verify(!findChild(page, "searchNothing").visible)
            const message = findChild(page, "searchFailure").item
            verify(message !== null)
            compare(message.code, "TUR-DESKTOP-CENTRE-001")
            compare(findChild(message, "messageCode").text, "TUR-DESKTOP-CENTRE-001")
            compare(message.title, "The application catalogue could not be read.")
            compare(message.reason, "The program that reads it could not be started.")
            // The technical layer is closed until asked for, and names the
            // command.
            verify(!message.expanded)
            verify(message.technicalDetail.indexOf("Command: /nonexistent/tur-catalogue search --json -- office") === 0)
            // No button that would do nothing: nothing makes a support
            // bundle yet, and P@x is not there.
            verify(!findChild(message, "messageSupportBundle").visible)
            verify(!findChild(message, "messageAskPax").visible)

            // "Try again" asks again.
            const again = findChild(message, "messageAction0")
            compare(again.text, "Try again")
            spy.target = host.catalogue.search
            spy.signalName = "statusChanged"
            mouseClick(again)
            tryCompare(host.catalogue.search, "status", SearchSession.Failed, 15000)
            verify(spy.count >= 2, "it went to searching and failed again")
        }

        function test_the_catalogue_s_own_failure_keeps_the_catalogue_s_code() {
            // The stand-in reports a catalogue it cannot load, as
            // tur-catalogue does: on standard error, with its code.
            const shell = host.startOnHelper("tur-centre:search?q=refused", 60000)
            settle()
            compare(host.catalogue.search.status, SearchSession.Failed)
            const message = findChild(view(shell), "searchFailure").item
            compare(message.code, "TUR-DESKTOP-CATALOGUE-001")
            // The reason is the catalogue's own sentence.
            compare(message.reason, "cannot read the catalogue directory /nowhere: it does not exist")
            verify(message.technicalDetail.indexOf("Exit status: 1") > 0)
        }

        function test_a_program_that_never_answers_is_given_up_on() {
            const shell = host.startOnHelper("tur-centre:search?q=sleep", 400)
            tryCompare(host.catalogue.search, "status", SearchSession.Failed, 20000)
            compare(findChild(view(shell), "searchFailure").item.code, "TUR-DESKTOP-CENTRE-002")
        }
    }
}

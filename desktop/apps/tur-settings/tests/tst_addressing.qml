// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Settings
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// Addressing (desktop-experience.md, 9.5 and 10.3): the program is opened at
// an address; a row that is asked for is brought into view, takes the
// keyboard and is highlighted; and the rectangle the program reports for it
// is where the row is drawn.
//
// The window here is low (480 px), so that most rows are out of sight until
// they are asked for. The same things over D-Bus are in unit/tst_bus.cpp.
Item {
    id: root

    width: 1080
    height: 480

    SettingsStage {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "Addressing"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            Kit.Theme.light = false
            Registry.directory = ""
            wait(0)
        }

        function view(shell) {
            return findChild(shell, "shellView").item
        }

        // Lets the window draw, so that what is read afterwards is what a
        // frame shows and not what was only asked for.
        function drawn(shell) {
            wait(50)
            grabImage(shell)
            wait(50)
        }

        function everyRow() {
            const rows = []
            for (const page of Registry.ownPages()) {
                for (const address of Registry.rowsOf(page))
                    rows.push({ tag: address, address: address, page: page })
            }
            return rows
        }

        // --- Every row can be asked for ---------------------------------------

        function test_a_row_is_shown_where_the_program_says_it_is_data() {
            return everyRow()
        }

        function test_a_row_is_shown_where_the_program_says_it_is(data) {
            const shell = host.start("default")
            // Asked for at once, before the window has drawn the page.
            const result = host.service.reveal(data.address, {})
            verify(result.ok, data.address + " is revealed")
            compare(result.address, data.address)
            compare(result.page, data.page)
            compare(result.row, data.address.split("#")[1])
            compare(result.program, "org.tur.Settings")
            compare(result.located, true)
            compare(result.highlighted, true)
            compare(result.focused, true)

            // The page is the row's, and the shell and the host say so.
            compare(view(shell).address, data.page)
            compare(shell.address, data.address)
            compare(host.service.currentAddress, data.address)

            const row = host.service.findRow(view(shell), data.address)
            verify(row !== null && row.visible)
            verify(row.marked)
            compare(shell.marked, row)

            // Where the program said the row is, is where the row is once
            // the window has drawn: nothing moved after the answer.
            drawn(shell)
            const rect = Testing.Harness.sceneRect(row)
            compare(result.x, rect.x, "x")
            compare(result.y, rect.y, "y")
            compare(result.width, rect.width, "width")
            compare(result.height, rect.height, "height")
            verify(rect.width > 100 && rect.height >= Kit.Theme.targetPointer)

            // And the row is in sight: inside the part of the window the
            // page is shown in. A row taller than that part shows its top.
            const room = Testing.Harness.sceneRect(findChild(shell, "shellContent"))
            verify(rect.x >= room.x && rect.x + rect.width <= room.x + room.width, "within the page's width")
            verify(rect.y >= room.y, "its top is in sight")
            if (rect.height <= room.height)
                verify(rect.y + rect.height <= room.y + room.height, "all of it is in sight")

            // The keyboard is on the row: on its control, if it has one that
            // can be used, and on the row itself otherwise.
            const holder = row.focusTarget !== null && row.focusTarget.enabled ? row.focusTarget : row
            verify(holder.activeFocus, "the keyboard is on " + Testing.Harness.describe(holder))
        }

        // --- The highlight ---------------------------------------------------

        // The line is drawn 4 px clear of the row and 3 px wide, in the
        // system's blue, or in P@x's gold when the assistant asked.
        function test_the_highlight_is_drawn_round_the_row_data() {
            return [
                { tag: "by a person, dark", light: false, pax: false },
                { tag: "by a person, light", light: true, pax: false },
                { tag: "by the assistant, dark", light: false, pax: true },
                { tag: "by the assistant, light", light: true, pax: true }
            ]
        }

        function test_the_highlight_is_drawn_round_the_row(data) {
            if (Testing.Harness.devicePixelRatio !== 1)
                skip("the picture is read pixel by pixel, at a scale of 1")
            Kit.Theme.light = data.light
            const shell = host.start("default")
            const result = host.service.reveal("tur.appearance#current", { pax: data.pax })
            verify(result.ok)
            const row = shell.marked
            compare(row.markedByPax, data.pax)
            drawn(shell)

            const line = data.pax ? Pax.PaxTheme.pax : Kit.Theme.accentText
            const ground = Kit.Theme.surfaceRaised
            verify(Testing.Harness.contrast(line, ground) >= 3, "the line can be seen on the card")
            const image = grabImage(shell)
            const left = Math.round(result.x)
            const top = Math.round(result.y)
            const right = Math.round(result.x + result.width)
            const bottom = Math.round(result.y + result.height)
            const middleX = Math.round(result.x + result.width / 2)
            const middleY = Math.round(result.y + result.height / 2)

            // The line: the middle one of its three pixels, on each side.
            compare(image.pixel(left - 6, middleY), line, "left of the row")
            compare(image.pixel(right + 5, middleY), line, "right of the row")
            compare(image.pixel(middleX, top - 6), line, "above the row")
            compare(image.pixel(middleX, bottom + 5), line, "below the row")
            // The 4 px between the line and the row, and what is outside
            // the line, are the card.
            compare(image.pixel(left - 2, middleY), ground, "between the line and the row")
            compare(image.pixel(right + 1, middleY), ground, "between the row and the line")
            compare(image.pixel(left - 9, middleY), ground, "outside the line")
            compare(image.pixel(right - 2, bottom + 8), ground, "under the line")

            // Put away, the line is gone.
            host.service.clearHighlight()
            compare(shell.marked, null)
            verify(!row.marked)
            drawn(shell)
            compare(grabImage(shell).pixel(left - 6, middleY), ground)
        }

        // Asked for a second time, a row that is already in sight, with the
        // keyboard, stays where it is: the page does not scroll.
        function test_a_row_asked_for_again_does_not_move_data() {
            return everyRow()
        }

        function test_a_row_asked_for_again_does_not_move(data) {
            const shell = host.start("default")
            const first = host.service.reveal(data.address, {})
            verify(first.ok)
            drawn(shell)
            for (const options of [{}, { pax: true }, { highlight: false }]) {
                const again = host.service.reveal(data.address, options)
                verify(again.ok)
                compare(again.y, first.y)
                compare(again.x, first.x)
                compare(again.focused, true)
                drawn(shell)
                const rect = Testing.Harness.sceneRect(host.service.findRow(view(shell), data.address))
                compare(rect.y, first.y, "where it is after the window has drawn")
            }
        }

        function test_asking_for_another_row_moves_the_highlight() {
            const shell = host.start("default")
            verify(host.service.reveal("tur.appearance#current", {}).ok)
            const first = shell.marked
            verify(host.service.reveal("tur.appearance#fonts", {}).ok)
            verify(!first.marked)
            compare(shell.marked.address, "tur.appearance#fonts")
            // And to another page: the row that was marked is gone with its
            // page, and nothing is left behind.
            verify(host.service.reveal("tur.about#base", {}).ok)
            compare(shell.marked.address, "tur.about#base")
        }

        function test_choosing_a_page_puts_the_highlight_away() {
            const shell = host.start("default")
            verify(host.service.reveal("tur.appearance#current", {}).ok)
            mouseClick(findChild(shell, "shellPage-tur.overview"))
            compare(shell.marked, null)
            compare(shell.address, "tur.overview")
            // Coming back, the row is not marked again by itself. (In a
            // window this low the page's place in the list is out of sight,
            // so the page is gone to as the list does it.)
            shell.go("tur.appearance")
            compare(view(shell).address, "tur.appearance")
            verify(!host.service.findRow(view(shell), "tur.appearance#current").marked)
            compare(shell.marked, null)
        }

        function test_without_a_highlight_the_row_is_still_brought_and_focused() {
            const shell = host.start("default")
            const result = host.service.reveal("tur.about#third-party", { highlight: false })
            verify(result.ok)
            compare(result.highlighted, false)
            compare(result.focused, true)
            compare(shell.marked, null)
            // Without a highlight the shell is at the page.
            compare(shell.address, "tur.about")
            const row = host.service.findRow(view(shell), "tur.about#third-party")
            verify(!row.marked)
            // The row's control has the keyboard, and shows it.
            verify(findChild(row, "noticesOpener").activeFocus)
            drawn(shell)
            const rect = Testing.Harness.sceneRect(row)
            compare(result.y, rect.y)
            verify(rect.y >= 0 && rect.y < root.height)
        }

        function test_a_highlight_with_a_duration_goes_away_by_itself() {
            const shell = host.start("default")
            const result = host.service.reveal("tur.overview#build", { duration: 0.3 })
            verify(result.ok && result.highlighted)
            const row = shell.marked
            verify(row.marked)
            tryVerify(() => !row.marked, 5000, "the highlight went away")
            compare(shell.marked, null)

            // Without a duration it stays.
            verify(host.service.reveal("tur.overview#build", {}).ok)
            wait(600)
            verify(shell.marked !== null && shell.marked.marked)
        }

        // Dimming: the rest of the window is darkened by a fifth, and the
        // row and its line are left as they are.
        function test_dimming_leaves_the_row_clear() {
            if (Testing.Harness.devicePixelRatio !== 1)
                skip("the picture is read pixel by pixel, at a scale of 1")
            const shell = host.start("default")
            verify(host.service.reveal("tur.appearance#current", {}).ok)
            drawn(shell)
            const plain = grabImage(shell)

            const result = host.service.reveal("tur.appearance#current", { dim: true })
            verify(result.ok)
            verify(shell.dimmed)
            verify(findChild(shell, "shellShade").visible)
            drawn(shell)
            const dimmed = grabImage(shell)

            // What is left clear is the row with its line: 7 px round it.
            compare(shell.hole.x, result.x - 7)
            compare(shell.hole.y, result.y - 7)
            compare(shell.hole.width, result.width + 14)
            compare(shell.hole.height, result.height + 14)

            const insideX = Math.round(result.x + 4)
            const insideY = Math.round(result.y + result.height / 2)
            compare(dimmed.pixel(insideX, insideY), plain.pixel(insideX, insideY), "the row is as it was")
            compare(dimmed.pixel(Math.round(result.x) - 6, insideY), Kit.Theme.accentText, "its line is as it was")
            // The side list, the page's title and the foot of the page are
            // darker than they were.
            for (const [x, y] of [[20, 200], [Math.round(result.x) + 4, Math.round(result.y) - 40],
                                  [root.width - 40, root.height - 10]]) {
                const before = plain.pixel(x, y)
                const after = dimmed.pixel(x, y)
                verify(after.r + after.g + after.b < before.r + before.g + before.b - 0.001,
                       "the window is darker at " + x + ", " + y + ": " + before + " became " + after)
            }

            // Scrolling the page moves what is left clear with the row.
            const flick = findChild(shell, "shellContent")
            verify(flick.contentHeight > flick.height, "the page is longer than the window")
            const row = shell.marked
            flick.contentY = 40
            drawn(shell)
            const moved = Testing.Harness.sceneRect(row)
            compare(moved.y, result.y - 40)
            compare(shell.hole.y, moved.y - 7)

            host.service.clearHighlight()
            verify(!shell.dimmed)
            verify(!findChild(shell, "shellShade").visible)
        }

        // A row in a group that is closed: the group opens.
        function test_a_row_in_a_closed_group_opens_the_group() {
            const shell = host.start("default")
            verify(host.service.open("tur.about").ok)
            const group = findChild(view(shell), "technicalGroup")
            verify(!group.expanded)
            const result = host.service.reveal("tur.about#base-image", {})
            verify(result.ok)
            verify(group.expanded)
            const row = shell.marked
            verify(row.visible)
            drawn(shell)
            const rect = Testing.Harness.sceneRect(row)
            compare(result.y, rect.y)
            compare(result.height, rect.height)
            verify(rect.height > 0 && rect.y + rect.height <= root.height)
        }

        // --- A page ----------------------------------------------------------

        function test_a_page_is_opened_without_a_highlight_data() {
            const rows = []
            for (const page of Registry.ownPages())
                rows.push({ tag: page, page: page })
            return rows
        }

        function test_a_page_is_opened_without_a_highlight(data) {
            const shell = host.start("default")
            // From somewhere else, scrolled down, with a row marked.
            verify(host.service.reveal("tur.about#settings-version", {}).ok)
            const result = host.service.open(data.page)
            verify(result.ok)
            compare(view(shell).address, data.page)
            compare(shell.address, data.page)
            compare(shell.marked, null)
            compare(findChild(shell, "shellContent").contentY, 0)

            // Revealing a page says where the page is shown.
            const where = host.service.reveal(data.page, {})
            verify(where.ok)
            compare(where.row, "")
            compare(where.highlighted, false)
            drawn(shell)
            const room = Testing.Harness.sceneRect(findChild(shell, "shellContent"))
            compare(where.x, room.x)
            compare(where.y, room.y)
            compare(where.width, room.width)
            compare(where.height, room.height)
        }

        // --- The command line and the link -------------------------------------

        function test_the_command_line_and_the_link_data() {
            return [
                { tag: "a page", text: "tur.appearance", address: "tur.appearance", marked: false },
                { tag: "a row", text: "tur.appearance#variant", address: "tur.appearance#variant", marked: true },
                { tag: "a link to a page", text: "tur-settings:tur.about", address: "tur.about", marked: false },
                { tag: "a link to a row", text: "tur-settings:tur.about#licence", address: "tur.about#licence",
                  marked: true },
                { tag: "a link with its fragment sign encoded", text: "tur-settings:tur.about%23licence",
                  address: "tur.about#licence", marked: true }
            ]
        }

        function test_the_command_line_and_the_link(data) {
            const shell = host.start("default")
            const outcome = host.service.go(data.text)
            verify(outcome.ok, data.text + " is gone to")
            compare(shell.address, data.address)
            compare(shell.marked !== null, data.marked)
            compare(view(shell).address, data.address.split("#")[0])
        }

        // --- What is refused --------------------------------------------------

        function test_what_is_not_an_address_is_refused_data() {
            return [
                { tag: "empty", text: "" },
                { tag: "upper case", text: "tur.Appearance" },
                { tag: "another namespace", text: "gnome.appearance" },
                { tag: "no page", text: "tur" },
                { tag: "an empty segment", text: "tur..about" },
                { tag: "an empty row", text: "tur.about#" },
                { tag: "a row with a dot", text: "tur.about#ver.sion" },
                { tag: "two rows", text: "tur.about#version#build" },
                { tag: "spaces round it", text: " tur.about " },
                { tag: "a path", text: "tur.about/../../etc/passwd" },
                { tag: "a web address", text: "https://example.org/" },
                { tag: "another program's link", text: "tur-centre:home" },
                { tag: "a link with a path", text: "tur-settings://tur.about" },
                { tag: "an option", text: "--help" }
            ]
        }

        function test_what_is_not_an_address_is_refused(data) {
            const shell = host.start("default")
            verify(host.service.open("tur.appearance").ok)
            for (const outcome of [host.service.reveal(data.text, {}), host.service.go(data.text)]) {
                // go() with nothing only brings the window forward.
                if (data.text === "" && outcome.ok)
                    continue
                compare(outcome.ok, false)
                compare(outcome.code, "TUR-DESKTOP-SETTINGS-004")
                compare(outcome.title, "This is not the address of a setting.")
            }
            // The shell stays where it was.
            compare(shell.address, "tur.appearance")
            compare(shell.marked, null)
        }

        function test_what_is_not_there_is_refused_with_its_code_data() {
            return [
                { tag: "a page nobody has", text: "tur.nothing", code: "TUR-DESKTOP-SETTINGS-001",
                  title: "No setting has this address.",
                  reason: "tur.nothing is not the address of a settings page or of a row on one." },
                { tag: "a row its page does not have", text: "tur.about#nothing", code: "TUR-DESKTOP-SETTINGS-001",
                  title: "No setting has this address.",
                  reason: "tur.about#nothing is not the address of a settings page or of a row on one." },
                { tag: "a page that is reserved", text: "tur.pax.privacy", code: "TUR-DESKTOP-SETTINGS-003",
                  title: "This page is not part of the system yet.",
                  reason: "tur.pax.privacy is reserved for a page that has not been built." },
                { tag: "a row of a page that is reserved", text: "tur.pax.privacy#private-mode",
                  code: "TUR-DESKTOP-SETTINGS-003", title: "This page is not part of the system yet.",
                  reason: "tur.pax.privacy#private-mode is reserved for a page that has not been built." },
                { tag: "a page shown only with its feature", text: "tur.macos", code: "TUR-DESKTOP-SETTINGS-003",
                  title: "This page is not part of the system yet.",
                  reason: "tur.macos is reserved for a page that has not been built." },
                { tag: "a row that was removed", text: "tur.example#old-thing", code: "TUR-DESKTOP-SETTINGS-002",
                  title: "This setting is no longer there.",
                  reason: "tur.example#old-thing was removed. It was replaced by tur.example#thing." },
                { tag: "a page that was removed", text: "tur.example.gone", code: "TUR-DESKTOP-SETTINGS-002",
                  title: "This setting is no longer there.",
                  reason: "tur.example.gone was removed and nothing replaced it." },
                // Another program's row: this stage is on no bus, so there
                // is nowhere to pass the call on to, and it says so.
                { tag: "another program's row, with no bus", text: "tur.example#thing",
                  code: "TUR-DESKTOP-SETTINGS-005", title: "The program that holds this setting could not be opened.",
                  reason: "tur.example#thing belongs to the program org.tur.Example, and there is no session bus to "
                          + "reach it on." }
            ]
        }

        function test_what_is_not_there_is_refused_with_its_code(data) {
            // The registry of a program that does not exist is read too.
            Registry.directory = settingsOtherRegistries
            compare(Registry.problems, [])
            const shell = host.start("default")
            verify(host.service.reveal("tur.appearance#current", {}).ok)
            for (const outcome of [host.service.reveal(data.text, {}), host.service.open(data.text),
                                   host.service.go(data.text)]) {
                compare(outcome.ok, false)
                compare(outcome.code, data.code)
                compare(outcome.title, data.title)
                compare(outcome.reason, data.reason)
            }
            // The shell stays where it was, with what was marked.
            compare(shell.address, "tur.appearance#current")
            verify(shell.marked !== null && shell.marked.marked)
        }

        function test_the_window_says_what_it_could_not_open() {
            // As the window does with the address it is started with.
            const shell = host.start("default")
            shell.follow("tur.pax.privacy#private-mode", true)
            const message = findChild(shell, "shellFailure")
            verify(message.visible)
            compare(message.code, "TUR-DESKTOP-SETTINGS-003")
            compare(message.title, "This page is not part of the system yet.")
            compare(view(shell).address, "tur.overview")
            // Going somewhere that exists puts the message away.
            verify(host.service.go("tur.about").ok)
            verify(!message.visible)
        }

        // Another program's registry is searched with this program's own.
        function test_search_crosses_programs() {
            Registry.directory = settingsOtherRegistries
            host.start("default")
            const hits = Registry.search("widget", 10)
            compare(hits.length, 1)
            compare(hits[0].address, "tur.example#thing")
            compare(hits[0].program, "org.tur.Example")
            // What was removed is not found.
            compare(Registry.search("taken away", 10).length, 0)
        }
    }
}

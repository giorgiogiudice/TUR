// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Settings
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The registry and the pages, held to each other in both directions
// (desktop-experience.md, 10.3, "In the code"): every row a page shows is in
// the registry under the page's address, and every row the registry has for
// a page is shown by it. A row with no address cannot be written at all (the
// property is required); this is the test that an address is the right one.
Item {
    id: root

    width: 1080
    height: 2400

    SettingsStage {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "RegistryAndPages"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        // The group a row stands in, or null.
        function groupOf(row) {
            for (let at = row.parent; at; at = at.parent) {
                if (at.isSettingsGroup === true)
                    return at
            }
            return null
        }

        function test_the_program_has_pages() {
            host.start("default")
            compare(Registry.problems, [])
            // What this task builds.
            compare(Registry.ownPages(), ["tur.overview", "tur.appearance", "tur.about"])
        }

        function test_each_page_shows_exactly_its_registered_rows_data() {
            const rows = []
            for (const address of Registry.ownPages())
                rows.push({ tag: address, page: address })
            return rows
        }

        function test_each_page_shows_exactly_its_registered_rows(data) {
            const shell = host.start("default")
            // Every page of the registry that can be opened has a view, and
            // the view is the page it says it is.
            shell.go(data.page)
            const view = findChild(shell, "shellView").item
            verify(view.isSettingsPage === true, data.page + " is shown by a page")
            compare(view.address, data.page)
            compare(view.title, Registry.entry(data.page).title)

            const registered = Registry.rowsOf(data.page)
            verify(registered.length > 0, "the registry has rows for " + data.page)
            const shown = []
            for (const row of host.service.rows(view)) {
                verify(shown.indexOf(row.address) < 0, row.address + " is on the page twice")
                shown.push(row.address)
            }
            // Both directions, and in the registry's order.
            compare(shown, registered)

            for (const row of host.service.rows(view)) {
                const entry = Registry.entry(row.address)
                verify(entry.address !== undefined, row.address + " is registered")
                compare(entry.page, data.page, row.address + " is on its own page")
                compare(entry.status, "available")
                // What it is called on screen is what the registry calls it.
                compare(row.title, entry.title)
                verify(row.title.length > 0)
                compare(row.kind, entry.kind)
                // A row whose source is not part of the system says so, and
                // one whose source is there does not.
                compare(row.available, entry.sourceAvailable, row.address)
                if (!row.available)
                    verify(row.unavailableReason.length > 0, row.address + " says why it is not available")
                // It stands in the section the registry puts it in.
                const group = groupOf(row)
                verify(group !== null, row.address + " is in a group")
                compare(group.page, data.page)
                compare(group.section, entry.section, row.address)
                compare(group.title, entry.sectionTitle)
                // A row that leads somewhere has a button for it; one that
                // is only read has nothing to press that changes anything.
                if (entry.kind === "action") {
                    verify(findChild(row, "rowOpen") !== null, row.address + " has its button")
                    verify(Registry.entry(entry.opens).address !== undefined, entry.opens + " is registered")
                }
                // Changing it is declared for everything but information.
                compare(entry.changeClass >= 0, entry.kind !== "information", row.address)
            }
        }

        function test_no_address_is_shown_on_screen() {
            // Addresses are never shown to the user except in developer mode
            // (10.3, rule 2).
            const shell = host.start("default")
            for (const page of Registry.ownPages()) {
                shell.go(page)
                wait(0)
                const texts = Testing.Harness.contrastFindings(shell, []).filter(finding => finding.kind === "text")
                verify(texts.length > 0)
                for (const finding of texts)
                    verify(finding.where.indexOf("\"tur.") < 0, "an address is on screen: " + finding.where)
            }
        }
    }
}

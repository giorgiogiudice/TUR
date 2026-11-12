// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Gallery
import Tur.Kit.Testing as Testing

// The accessibility walk (desktop-experience.md, 13.1, X1 and X3), over every
// page of the gallery, left-to-right and right-to-left:
//
//  - Tab alone reaches every enabled control once, then comes round again;
//  - in reading order: down the page, and along each line in the direction
//    of the language;
//  - and each control tells the accessibility layer a name and a role.
Item {
    id: root

    width: 800
    height: 2400

    PageHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "AccessibilityWalk"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.unload()
            Testing.Harness.rightToLeft = false
            wait(0)
        }

        function test_walk_data() {
            const rows = []
            for (const name of host.pageNames) {
                rows.push({ tag: host.slug(name) + "-ltr", page: name, rtl: false })
                rows.push({ tag: host.slug(name) + "-rtl", page: name, rtl: true })
            }
            return rows
        }

        // The tab stop that an item is, or is inside.
        function stopOf(item, stops) {
            for (let at = item; at; at = at.parent) {
                if (stops.indexOf(at) >= 0)
                    return at
            }
            return null
        }

        function test_walk(data) {
            Testing.Harness.rightToLeft = data.rtl
            const page = host.load(data.page)
            wait(50)

            const stops = Testing.Harness.tabStops(page.keyboardScope)
            console.info("TUR-A11Y " + data.tag + ": " + stops.length + " tab stops")

            // X3: a name and a role, as the accessibility layer reports them.
            for (const stop of stops) {
                const where = Testing.Harness.describe(stop)
                const seen = Testing.Harness.accessible(stop)
                verify(seen.valid, where + " is not exposed to the accessibility layer")
                verify(seen.name.trim().length > 0, where + " has no accessible name")
                verify(seen.role !== Accessible.NoRole, where + " has no accessible role")
            }

            if (stops.length === 0)
                return

            // X1: Tab alone, from wherever the page starts.
            if (page.keyboardScope === page)
                root.Window.contentItem.forceActiveFocus()
            const visited = []
            for (let press = 0; press < stops.length; press++) {
                keyClick(Qt.Key_Tab)
                const focus = root.Window.activeFocusItem
                const stop = stopOf(focus, stops)
                verify(stop !== null, "Tab number " + (press + 1) + " went to " + Testing.Harness.describe(focus)
                       + ", which is not one of this page's controls")
                verify(visited.indexOf(stop) < 0, "Tab reached " + Testing.Harness.describe(stop)
                       + " a second time before it had reached every control")
                visited.push(stop)
            }
            compare(visited.length, stops.length, "Tab reached every control")

            // One more Tab comes round to the first again: no dead end.
            keyClick(Qt.Key_Tab)
            compare(stopOf(root.Window.activeFocusItem, stops), visited[0], "Tab comes round again")

            // Reading order, starting from the first control of the page.
            const start = visited.indexOf(stops[0])
            const order = visited.slice(start).concat(visited.slice(0, start))
            for (let i = 1; i < order.length; i++) {
                const a = Testing.Harness.sceneRect(order[i - 1])
                const b = Testing.Harness.sceneRect(order[i])
                const below = b.y >= a.y + a.height - 1
                const above = b.y + b.height <= a.y + 1
                const onward = data.rtl ? b.x + b.width <= a.x + 1 : b.x >= a.x + a.width - 1
                verify(below || (!above && onward),
                       Testing.Harness.describe(order[i]) + " comes after " + Testing.Harness.describe(order[i - 1])
                       + " but is not below it or further along its line")
            }
        }
    }
}

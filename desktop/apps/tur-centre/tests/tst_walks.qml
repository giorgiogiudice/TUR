// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The kit's walks (desktop-experience.md, section 13) over every state of the
// application centre, each on the whole shell: header, side list and content.
//
//  - The contrast walk, in both variants: every text and every control
//    boundary against the colour really painted behind it (X5).
//  - The accessibility walk, left-to-right and right-to-left: Tab alone
//    reaches every enabled control once and comes round; each has a name and
//    a role (X1, X3); the order is the reading order.
//  - Text at 200 %, strings 40 % longer in a right-to-left script, and both:
//    nothing is elided or cut off (X8, L5, L6).
//
// The states are the ones the pictures are taken of: CentreHost.states.
Item {
    id: root

    width: 1080
    height: 6000

    CentreHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "CentreWalks"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            Kit.Theme.light = false
            Kit.Theme.textScale = 1
            Testing.Harness.pseudo = Testing.Harness.None
            Testing.Harness.rightToLeft = false
            wait(0)
        }

        // Enters a state, waits for every answer, and opens the sections the
        // state opens in place.
        function enter(state) {
            const shell = host.enter(state)
            tryVerify(() => host.settled, 15000, "every answer that was asked for has arrived")
            wait(0)
            for (const name of state.open) {
                const section = findChild(shell, name)
                verify(section !== null, "the state " + state.name + " has a section named " + name)
                section.expanded = true
            }
            wait(50)
            return shell
        }

        function rows(variants) {
            const out = []
            for (const state of host.states) {
                for (const variant of variants)
                    out.push(Object.assign({ tag: state.name + "-" + variant.name, state: state }, variant))
            }
            return out
        }

        // --- Contrast (X5) -------------------------------------------------

        function test_contrast_data() {
            return rows([{ name: "dark", light: false }, { name: "light", light: true }])
        }

        function test_contrast(data) {
            Kit.Theme.light = data.light
            const shell = enter(data.state)
            // A state that shows a sheet is judged on the sheet, which is
            // what the person sees and can use; otherwise the search field
            // has the keyboard, as when the window opens, so that the focus
            // ring is judged too.
            const sheet = host.sheetOf(data.state)
            if (sheet === null)
                shell.focusSearch()
            wait(50)
            const target = sheet !== null ? sheet.sheetItem : shell
            // A grab makes the scene lay itself out as it will be painted.
            grabImage(target)

            // line.subtle is decoration: nothing is understood through it.
            const findings = Testing.Harness.contrastFindings(target, [Kit.Theme.lineSubtle])
            const failures = []
            let texts = 0
            let boundaries = 0
            let exempt = 0
            for (const finding of findings) {
                if (finding.exempt) {
                    exempt++
                    continue
                }
                if (finding.problem.length > 0) {
                    failures.push(finding.kind + ": " + finding.problem + " — " + finding.where)
                    continue
                }
                const need = finding.kind === "text" ? 4.5 : 3.0
                if (finding.kind === "text")
                    texts++
                else
                    boundaries++
                if (finding.ratio < need) {
                    failures.push(finding.kind + " " + finding.foreground + " on " + finding.background + " is "
                                  + finding.ratio.toFixed(2) + ":1, needs " + need + ":1 — " + finding.where)
                }
            }
            console.info("TUR-CONTRAST centre-" + data.tag + ": " + texts + " texts, " + boundaries + " boundaries, "
                         + exempt + " exempt (unavailable), " + failures.length + " failures")
            compare(failures.join("\n"), "", "every text and boundary of the state meets its ratio")
            verify(texts > 0, "the walk found text to check")
        }

        // --- Keyboard and accessible names (X1, X3) --------------------------

        function test_keyboard_data() {
            return rows([{ name: "ltr", rtl: false }, { name: "rtl", rtl: true }])
        }

        // The tab stop that an item is, or is inside.
        function stopOf(item, stops) {
            for (let at = item; at; at = at.parent) {
                if (stops.indexOf(at) >= 0)
                    return at
            }
            return null
        }

        // The part of the window an item is in: the header, the side list or
        // the content. Reading order is within a part; the parts follow one
        // another in that order.
        function partOf(item) {
            const parts = ["shellHeader", "shellSide", "shellContent"]
            for (let at = item; at; at = at.parent) {
                const index = parts.indexOf(at.objectName)
                if (index >= 0)
                    return index
            }
            return -1
        }

        function test_keyboard(data) {
            Testing.Harness.rightToLeft = data.rtl
            const shell = enter(data.state)

            // A sheet holds the keyboard (13.1, X1): its controls have names
            // and roles, and Tab goes round them without leaving it.
            const sheet = host.sheetOf(data.state)
            if (sheet !== null) {
                verify(sheet.opened)
                const inside = Testing.Harness.tabStops(sheet.sheetItem)
                console.info("TUR-A11Y centre-" + data.tag + ": " + inside.length + " tab stops in the sheet")
                verify(inside.length >= 2)
                for (const stop of inside) {
                    const seen = Testing.Harness.accessible(stop)
                    verify(seen.valid && seen.name.trim().length > 0 && seen.role !== Accessible.NoRole,
                           Testing.Harness.describe(stop) + " has a name and a role")
                }
                for (let press = 0; press < inside.length + 1; press++) {
                    keyClick(Qt.Key_Tab)
                    verify(stopOf(root.Window.activeFocusItem, inside) !== null,
                           "Tab stays in the sheet: " + Testing.Harness.describe(root.Window.activeFocusItem))
                }
                return
            }

            const stops = Testing.Harness.tabStops(shell)
            console.info("TUR-A11Y centre-" + data.tag + ": " + stops.length + " tab stops")
            // The search field at least, in every state.
            verify(stops.length >= 1)

            // X3: a name and a role, as the accessibility layer reports them.
            for (const stop of stops) {
                const where = Testing.Harness.describe(stop)
                const seen = Testing.Harness.accessible(stop)
                verify(seen.valid, where + " is not exposed to the accessibility layer")
                verify(seen.name.trim().length > 0, where + " has no accessible name")
                verify(seen.role !== Accessible.NoRole, where + " has no accessible role")
                verify(partOf(stop) >= 0, where + " is in the header, the side list or the content")
            }

            // X1: Tab alone, from wherever the window starts.
            root.Window.contentItem.forceActiveFocus()
            const visited = []
            for (let press = 0; press < stops.length; press++) {
                keyClick(Qt.Key_Tab)
                const focus = root.Window.activeFocusItem
                const stop = stopOf(focus, stops)
                verify(stop !== null, "Tab number " + (press + 1) + " went to " + Testing.Harness.describe(focus)
                       + ", which is not one of this state's controls")
                verify(visited.indexOf(stop) < 0, "Tab reached " + Testing.Harness.describe(stop)
                       + " a second time before it had reached every control")
                visited.push(stop)
            }
            compare(visited.length, stops.length, "Tab reached every control")

            // One more Tab comes round to the first again: no dead end.
            keyClick(Qt.Key_Tab)
            compare(stopOf(root.Window.activeFocusItem, stops), visited[0], "Tab comes round again")

            // Reading order, starting from the first control of the window:
            // the header, then the side list, then the content; and within
            // each, down the page and along each line in the direction of
            // the language.
            const start = visited.indexOf(stops[0])
            const order = visited.slice(start).concat(visited.slice(0, start))
            for (let i = 1; i < order.length; i++) {
                const before = partOf(order[i - 1])
                const after = partOf(order[i])
                verify(after >= before, Testing.Harness.describe(order[i]) + " comes after "
                       + Testing.Harness.describe(order[i - 1]) + " but belongs to an earlier part of the window")
                if (after !== before)
                    continue
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

        // --- Text size and length (X8, L5, L6) -------------------------------

        function test_no_clipped_text_data() {
            return rows([
                { name: "200-percent", scale: 2, rtl: false },
                { name: "rtl-long", scale: 1, rtl: true },
                { name: "200-percent-rtl-long", scale: 2, rtl: true }
            ])
        }

        function test_no_clipped_text(data) {
            Kit.Theme.textScale = data.scale
            Testing.Harness.rightToLeft = data.rtl
            Testing.Harness.pseudo = data.rtl ? Testing.Harness.RtlLong : Testing.Harness.None
            const shell = enter(data.state)
            const sheet = host.sheetOf(data.state)
            grabImage(shell)
            wait(0)

            const clipped = Testing.Harness.clippedText(shell)
            if (sheet !== null)
                clipped.push(...Testing.Harness.clippedText(sheet.sheetItem))
            compare(clipped.join("\n"), "", "no text is elided or cut off")
            verify(shell.naturalHeight <= root.height,
                   "the state (" + shell.naturalHeight + " px) fits in the test window, so all of it was looked at")
        }
    }
}

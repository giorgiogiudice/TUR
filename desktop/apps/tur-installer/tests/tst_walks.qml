// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Installer
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The kit's walks (desktop-experience.md, section 13) over every state of the
// installer, each on the whole wizard: the steps, the screen and the
// diagnostics drawer.
//
//  - The contrast walk, in both variants: every text and every control
//    boundary against the colour really painted behind it (X5).
//  - The accessibility walk, left-to-right and right-to-left: Tab alone
//    reaches every enabled control once and comes round; each has a name and
//    a role (X1, X3); the order is the reading order.
//  - Text at 200 %, strings 40 % longer in a right-to-left script, and both:
//    nothing is elided or cut off (X8, L5, L6).
//
// The states are the ones the pictures are taken of: WizardHost.states.
Item {
    id: root

    width: 1400
    height: 4200

    WizardHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "InstallerWalks"
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

        function enter(state) {
            const wizard = host.enter(state)
            tryVerify(() => host.ready, 10000, "the state " + state.name + " was reached")
            wait(50)
            return wizard
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
            const wizard = enter(data.state)
            grabImage(wizard)

            // line.subtle is decoration: nothing is understood through it.
            const findings = Testing.Harness.contrastFindings(wizard, [Kit.Theme.lineSubtle])
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
            console.info("TUR-CONTRAST installer-" + data.tag + ": " + texts + " texts, " + boundaries + " boundaries, "
                         + exempt + " exempt (unavailable), " + failures.length + " failures")
            compare(failures.join("\n"), "", "every text and boundary of the state meets its ratio")
            verify(texts > 0, "the walk found text to check")
        }

        // --- Keyboard and accessible names (X1, X3) --------------------------

        function test_keyboard_data() {
            return rows([{ name: "ltr", rtl: false }, { name: "rtl", rtl: true }])
        }

        function stopOf(item, stops) {
            for (let at = item; at; at = at.parent) {
                if (stops.indexOf(at) >= 0)
                    return at
            }
            return null
        }

        // The part of the window an item is in: the steps, the screen or the
        // drawer. Reading order is within a part; the parts follow one
        // another in that order.
        function partOf(item) {
            const parts = ["wizardSteps", "wizardContent", "wizardDrawer"]
            for (let at = item; at; at = at.parent) {
                const index = parts.indexOf(at.objectName)
                if (index >= 0)
                    return index
            }
            return -1
        }

        function test_keyboard(data) {
            Testing.Harness.rightToLeft = data.rtl
            const wizard = enter(data.state)

            const stops = Testing.Harness.tabStops(wizard)
            console.info("TUR-A11Y installer-" + data.tag + ": " + stops.length + " tab stops")
            // The drawer's two buttons at least, on every screen.
            verify(stops.length >= 2)

            for (const stop of stops) {
                const where = Testing.Harness.describe(stop)
                const seen = Testing.Harness.accessible(stop)
                verify(seen.valid, where + " is not exposed to the accessibility layer")
                verify(seen.name.trim().length > 0, where + " has no accessible name")
                verify(seen.role !== Accessible.NoRole, where + " has no accessible role")
                verify(partOf(stop) >= 0, where + " is in the steps, the screen or the drawer")
            }

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
            keyClick(Qt.Key_Tab)
            compare(stopOf(root.Window.activeFocusItem, stops), visited[0], "Tab comes round again")

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
            const wizard = enter(data.state)
            grabImage(wizard)
            wait(0)

            const clipped = Testing.Harness.clippedText(wizard)
            compare(clipped.join("\n"), "", "no text is elided or cut off")
            verify(wizard.naturalHeight <= root.height,
                   "the state (" + wizard.naturalHeight + " px) fits in the test window, so all of it was looked at")
        }
    }
}

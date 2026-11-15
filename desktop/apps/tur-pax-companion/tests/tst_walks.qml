// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxCompanion
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// The kit's walks (desktop-experience.md, section 13) over every scene of
// P@x's character, the ones the pictures are taken of (CompanionHost.scenes):
//
//  - contrast, in both variants: every text and every control boundary
//    against the colour really painted behind it (X5). The character's own
//    layered shapes are judged in tst_character.qml, over every background;
//  - gold for P@x and nothing else (5.1): every item drawn in P@x's gold is
//    part of the character or of P@x's name. What TUR says in the bubble,
//    and every button, is not gold;
//  - keyboard, left-to-right and right-to-left: Tab alone reaches the
//    character and every enabled control of what is open, once, and comes
//    round; each has a name and a role (X1, X3); inside what is open the
//    order is the reading order;
//  - text at 200 %, strings 40 % longer in a right-to-left script, and both:
//    nothing is elided or cut off (X8, L5, L6).
Item {
    id: root

    width: 1100
    height: 3200

    CompanionHost {
        id: host

        anchors.left: parent.left
        anchors.top: parent.top
        width: 640
        height: Math.max(240, Math.ceil(host.wantedHeight) + 80)
    }

    TestCase {
        name: "CompanionWalks"
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

        function rows(variants) {
            const out = []
            for (const scene of host.scenes) {
                for (const variant of variants)
                    out.push(Object.assign({ tag: scene.name + "-" + variant.name, scene: scene }, variant))
            }
            return out
        }

        function reach(scene) {
            host.reach(scene)
            wait(50)
            return host
        }

        // --- Contrast (X5) ----------------------------------------------------

        function test_contrast_data() {
            return rows([{ name: "dark", light: false }, { name: "light", light: true }])
        }

        function test_contrast(data) {
            Kit.Theme.light = data.light
            const view = reach(data.scene)
            grabImage(view)
            // Decoration: the kit's hairline, and the white hairline of the
            // character's mark, which is one of its layers and is judged
            // with them (tst_character.qml).
            const findings = Testing.Harness.contrastFindings(view, [Kit.Theme.lineSubtle, Pax.PaxTheme.overlayHairline])
            const failures = []
            let texts = 0
            let boundaries = 0
            for (const finding of findings) {
                if (finding.exempt)
                    continue
                if (finding.problem.length > 0) {
                    failures.push(finding.kind + ": " + finding.problem + " — " + finding.where)
                    continue
                }
                const need = finding.kind === "text" ? 4.5 : 3.0
                if (finding.kind === "text")
                    texts++
                else
                    boundaries++
                if (finding.ratio < need)
                    failures.push(finding.kind + " " + finding.foreground + " on " + finding.background + " is "
                                  + finding.ratio.toFixed(2) + ":1, needs " + need + ":1 — " + finding.where)
            }
            console.info("TUR-CONTRAST companion-" + data.tag + ": " + texts + " texts, " + boundaries
                         + " boundaries, " + failures.length + " failures")
            compare(failures.join("\n"), "", "every text and boundary meets its ratio")
            verify(texts > 0, "the walk found text to check")
        }

        // --- Gold is P@x's (5.1) ----------------------------------------------

        function test_gold_only_for_pax_data() {
            return rows([{ name: "dark", light: false }, { name: "light", light: true }])
        }

        function isGold(colour) {
            return colour !== undefined && colour !== null
                   && (Qt.colorEqual(colour, Pax.PaxTheme.pax) || Qt.colorEqual(colour, Pax.PaxTheme.fill)
                       || Qt.colorEqual(colour, Pax.PaxTheme.overlayFill))
        }

        function insidePax(item) {
            for (let at = item; at !== null && at !== undefined; at = at.parent) {
                if (String(at.objectName).indexOf("pax") === 0)
                    return true
            }
            return false
        }

        function test_gold_only_for_pax(data) {
            Kit.Theme.light = data.light
            const view = reach(data.scene)
            const strays = []
            let golden = 0
            const visit = item => {
                if (!item.visible)
                    return
                const colours = []
                if (item.color !== undefined)
                    colours.push(item.color)
                if (item.border !== undefined && item.border.width > 0)
                    colours.push(item.border.color)
                if (item.tint !== undefined)
                    colours.push(item.tint)
                if (colours.some(isGold)) {
                    golden++
                    if (!insidePax(item))
                        strays.push(Testing.Harness.describe(item))
                }
                const below = []
                for (const child of item.children)
                    below.push(child)
                if (item.contentItem && below.indexOf(item.contentItem) < 0)
                    below.push(item.contentItem)
                for (const child of below)
                    visit(child)
            }
            visit(view)
            console.info("TUR-GOLD companion-" + data.tag + ": " + golden + " gold items, " + strays.length
                         + " outside the character and P@x's name")
            compare(strays.join("\n"), "", "gold is drawn only in the character and in P@x's name")
        }

        // --- Keyboard and accessible names (X1, X3) -----------------------------

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

        // The character first, then what is open above it.
        function partOf(item) {
            for (let at = item; at; at = at.parent) {
                if (at.objectName === "paxSeat")
                    return 0
                if (at.objectName === "leaf")
                    return 1
            }
            return 2
        }

        function test_keyboard(data) {
            Testing.Harness.rightToLeft = data.rtl
            const view = reach(data.scene)
            const stops = Testing.Harness.tabStops(view)
            console.info("TUR-A11Y companion-" + data.tag + ": " + stops.length + " tab stops")
            // The character itself is always one of them.
            verify(stops.indexOf(host.stage.seatItem) >= 0, "the character can be reached")
            for (const stop of stops) {
                const where = Testing.Harness.describe(stop)
                const seen = Testing.Harness.accessible(stop)
                verify(seen.valid, where + " is not exposed to the accessibility layer")
                verify(seen.name.trim().length > 0, where + " has no accessible name")
                verify(seen.role !== Accessible.NoRole, where + " has no accessible role")
            }
            // What the character says of itself is its state, in words.
            compare(Testing.Harness.accessible(host.stage.seatItem).name, Words.state(host.companion.state))

            root.Window.contentItem.forceActiveFocus()
            const visited = []
            for (let press = 0; press < stops.length; press++) {
                keyClick(Qt.Key_Tab)
                const focus = root.Window.activeFocusItem
                const stop = stopOf(focus, stops)
                verify(stop !== null, "Tab number " + (press + 1) + " went to " + Testing.Harness.describe(focus)
                       + ", which is not one of this scene's controls")
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
                       + Testing.Harness.describe(order[i - 1]) + " but belongs to an earlier part")
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

        // --- Text size and length (X8, L5, L6) ----------------------------------

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
            const view = reach(data.scene)
            grabImage(view)
            wait(0)
            const clipped = Testing.Harness.clippedText(view)
            compare(clipped.join("\n"), "", "no text is elided or cut off")
            verify(host.height <= root.height,
                   "the scene (" + host.height + " px) fits in the test window, so all of it was looked at")
            // What is open is on the screen and above the bar at any size.
            if (host.stage.page !== "") {
                const leaf = host.stage.leafItem
                verify(leaf.x >= 0 && leaf.x + leaf.width <= host.width, "what is open is on the screen")
                verify(leaf.y >= 0, "what is open fits above the bar")
                verify(leaf.contentHeight <= leaf.height + 1, "nothing has to scroll")
            }
        }
    }
}

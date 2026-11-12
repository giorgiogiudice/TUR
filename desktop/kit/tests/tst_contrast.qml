// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Gallery
import Tur.Kit.Testing as Testing

// The contrast walk (desktop-experience.md, 13.1, X5), over every component
// in every state, in BOTH variants. It does not trust a table: for each piece
// of text and each control boundary actually on the page it finds the colour
// actually painted behind it and computes the WCAG ratio. Text needs 4.5:1, a
// boundary 3:1. White on white in one variant fails here.
Item {
    id: root

    width: 800
    height: 2400

    PageHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "ContrastWalk"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.unload()
            Kit.Theme.light = false
            wait(0)
        }

        function test_walk_data() {
            const rows = []
            for (const name of host.pageNames) {
                rows.push({ tag: host.slug(name) + "-dark", page: name, light: false })
                rows.push({ tag: host.slug(name) + "-light", page: name, light: true })
            }
            return rows
        }

        function test_walk(data) {
            Kit.Theme.light = data.light
            const page = host.load(data.page)
            if (page.focusTarget !== null)
                page.focusTarget.forceActiveFocus(Qt.TabFocusReason)
            else if (page.focusName.length > 0)
                findChild(page, page.focusName).forceActiveFocus(Qt.TabFocusReason)
            wait(50)
            // A grab makes the scene lay itself out as it will be painted.
            grabImage(page)

            // line.subtle is decoration: nothing is understood through it.
            const findings = Testing.Harness.contrastFindings(page, [Kit.Theme.lineSubtle])
            const failures = []
            let texts = 0
            let boundaries = 0
            let exempt = 0
            let lowest = 21
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
                lowest = Math.min(lowest, finding.ratio / need)
                if (finding.ratio < need) {
                    failures.push(finding.kind + " " + finding.foreground + " on " + finding.background + " is "
                                  + finding.ratio.toFixed(2) + ":1, needs " + need + ":1 — " + finding.where)
                }
            }
            console.info("TUR-CONTRAST " + data.tag + ": " + texts + " texts, " + boundaries + " boundaries, "
                         + exempt + " exempt (unavailable), " + failures.length + " failures")
            compare(failures.join("\n"), "", "every text and boundary on the page meets its ratio")
            verify(texts > 0, "the walk found text to check")
        }
    }
}

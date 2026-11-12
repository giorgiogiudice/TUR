// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Gallery
import Tur.Kit.Testing as Testing
import ".."

// Renders every page of the gallery to PNG files in TUR_SCREENSHOT_DIR, for
// the screenshot job to compare with the references in
// desktop/tests/screenshots (desktop-experience.md, 15.4).
//
// One process renders at one scale (QT_SCALE_FACTOR). At 1x: dark and light,
// each left-to-right and right-to-left with lengthened strings. At a
// fractional scale: dark and light, left-to-right.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 800
    height: 2400

    PageHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "GalleryScreenshots"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            host.unload()
            Kit.Theme.light = false
            Testing.Harness.pseudo = Testing.Harness.None
            Testing.Harness.rightToLeft = false
            wait(0)
        }

        function test_render_data() {
            const scale = Testing.Harness.scaleLabel
            const combinations = [
                { name: "dark-ltr", light: false, rtl: false },
                { name: "light-ltr", light: true, rtl: false }
            ]
            if (scale === "1x") {
                combinations.push({ name: "dark-rtl-long", light: false, rtl: true })
                combinations.push({ name: "light-rtl-long", light: true, rtl: true })
            }
            const rows = []
            for (const page of host.pageNames) {
                for (const combination of combinations) {
                    rows.push({
                        tag: host.slug(page) + "-" + combination.name + "-" + scale,
                        page: page,
                        light: combination.light,
                        rtl: combination.rtl
                    })
                }
            }
            return rows
        }

        function test_render(data) {
            Kit.Theme.light = data.light
            Testing.Harness.rightToLeft = data.rtl
            Testing.Harness.pseudo = data.rtl ? Testing.Harness.RtlLong : Testing.Harness.None

            const page = host.load(data.page)
            host.showKeyboardFocus(page.focusTarget !== null ? page.focusTarget
                                   : page.focusName.length > 0 ? findChild(page, page.focusName) : null)

            // The first grab makes the scene lay itself out; the page's
            // height is final only after it.
            wait(50)
            Testing.Harness.grab(page, "")
            wait(50)
            verify(page.height > 0)
            verify(page.height <= root.height, "the page (" + page.height + " px) fits in the test window")

            const file = Testing.Harness.outputDir + "/" + data.tag + ".png"
            const shot = Testing.Harness.grab(page, file)
            verify(shot.saved, "the picture was saved to " + file)
            fuzzyCompare(shot.ratio, Testing.Harness.devicePixelRatio, 0.01,
                         "the window was drawn at the scale asked for (window " + shot.windowWidth + " px wide)")
            compare(shot.width, Math.round(page.width * shot.ratio))
        }
    }
}

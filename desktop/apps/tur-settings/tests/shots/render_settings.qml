// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Settings
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing
import ".."

// Renders every state of the settings to PNG files in TUR_SCREENSHOT_DIR, for
// the screenshot job to compare with the references in
// desktop/tests/screenshots (desktop-experience.md, 15.4).
//
// What is in the pictures: the titles are the registry's; the version, build
// and base are those of the fixture sets (fixtures/README.md), which were
// written by hand after image/Containerfile; the licence is the file at the
// top of the repository. Nothing in a picture stands for a service that does
// not exist.
//
// The kit's matrix: one process renders at one scale (QT_SCALE_FACTOR). At
// 1x: dark and light, each left-to-right and right-to-left with lengthened
// strings. At a fractional scale: dark and light, left-to-right. Each picture
// is the whole shell at the height at which nothing has to scroll.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 1080
    height: 4200

    SettingsStage {
        id: host

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 760
    }

    TestCase {
        name: "SettingsScreenshots"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            host.stop()
            host.height = 760
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
            for (const state of host.states) {
                for (const combination of combinations) {
                    rows.push({
                        tag: "settings-" + state.name + "-" + combination.name + "-" + scale,
                        state: state,
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

            const shell = host.enter(data.state)
            wait(0)
            for (const name of data.state.open !== undefined ? data.state.open : []) {
                const section = findChild(shell, name)
                verify(section !== null, "the state has a section named " + name)
                section.expanded = true
            }
            // A state that did not put the keyboard on a row has it in the
            // search field, as when the window opens.
            if (shell.marked === null)
                shell.focusSearch()

            // The first grab makes the scene lay itself out; the height at
            // which nothing scrolls is final only after it.
            wait(50)
            Testing.Harness.grab(shell, "")
            wait(50)
            const natural = Math.ceil(shell.naturalHeight)
            verify(natural > 0)
            verify(natural <= root.height, "the state (" + natural + " px) fits in the test window")
            // At least the height of the program's window, so that a short
            // page is shown as it is in the window: with room under it.
            host.height = Math.max(760, natural)
            wait(50)
            // A row that was asked for was scrolled to in the lower window;
            // in a window as high as the page nothing is scrolled.
            findChild(shell, "shellContent").contentY = 0
            findChild(shell, "shellList").contentY = 0
            Testing.Harness.grab(shell, "")
            wait(50)
            Testing.Harness.grab(shell, "")
            wait(50)
            compare(findChild(shell, "shellContent").contentY, 0, "nothing is scrolled out of the picture")

            const file = Testing.Harness.outputDir + "/" + data.tag + ".png"
            const shot = Testing.Harness.grab(shell, file)
            verify(shot.saved, "the picture was saved to " + file)
            fuzzyCompare(shot.ratio, Testing.Harness.devicePixelRatio, 0.01,
                         "the window was drawn at the scale asked for (window " + shot.windowWidth + " px wide)")
            compare(shot.width, Math.round(shell.width * shot.ratio))
        }
    }
}

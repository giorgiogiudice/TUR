// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Installer
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing
import ".."

// Renders every state of the installer to PNG files in TUR_SCREENSHOT_DIR,
// dark and light, at the size of its window (1120 by 740) and, when a state
// is taller, at the height at which nothing has to scroll. The pictures go
// into the desktop workflow's artifact; docs/reports/TUR-009.md shows them.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; the test installer-pictures passes it with -input.
Item {
    id: root

    width: 1120
    height: 3200

    WizardHost {
        id: host

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 740
    }

    TestCase {
        name: "InstallerPictures"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            host.stop()
            host.height = 740
            Kit.Theme.light = false
            wait(0)
        }

        function test_render_data() {
            const rows = []
            for (const state of host.states) {
                for (const variant of [{ name: "dark", light: false }, { name: "light", light: true }])
                    rows.push({ tag: "installer-" + state.name + "-" + variant.name, state: state, light: variant.light })
            }
            return rows
        }

        function test_render(data) {
            Kit.Theme.light = data.light
            const wizard = host.enter(data.state)
            tryVerify(() => host.ready, 10000, "the state was reached")
            wait(100)
            Testing.Harness.grab(wizard, "")
            wait(50)
            host.height = Math.max(740, Math.ceil(wizard.naturalHeight))
            wait(100)
            Testing.Harness.grab(wizard, "")
            wait(50)
            const file = Testing.Harness.outputDir + "/" + data.tag + ".png"
            const shot = Testing.Harness.grab(wizard, file)
            verify(shot.saved, "the picture was saved to " + file)
        }
    }
}

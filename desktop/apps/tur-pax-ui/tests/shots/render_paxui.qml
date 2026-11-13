// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing
import ".."

// Renders every state of P@x's window (PaxHost.states: the ask bar, the
// conversation panel, the activity view and each screen of the setup) to PNG
// files in TUR_SCREENSHOT_DIR, dark and light, left-to-right, for the
// screenshot job to compare with the references in desktop/tests/screenshots
// (desktop/ci/screenshots.sh). Each view at its own width (640, 420 and 600)
// and at the height at which nothing has to scroll. The stand-in for
// org.tur.Agent1 answers from the real service's recorded replies, so the
// words in the pictures are the service's.
//
// Names: paxui-<state>-<variant>-ltr-<scale>.png.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 700
    height: 5200

    PaxHost {
        id: host

        anchors.left: parent.left
        anchors.top: parent.top
        width: 700
        height: 780
    }

    TestCase {
        name: "PaxUiPictures"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            host.stop()
            host.height = 780
            Kit.Theme.light = false
            fixture.startAgent()
            wait(0)
        }

        function test_render_data() {
            const rows = []
            for (const state of host.states) {
                for (const variant of [{ name: "dark", light: false }, { name: "light", light: true }])
                    rows.push({ tag: "paxui-" + state.name + "-" + variant.name, state: state, light: variant.light })
            }
            return rows
        }

        function test_render(data) {
            Kit.Theme.light = data.light
            // Tall enough while the state is reached that nothing scrolls:
            // the software renderer can leave a picture of a scrolled item
            // behind (seen in the first pictures of the panel).
            host.height = 5000
            wait(0)
            host.reach(data.state, this)
            const minimum = data.state.view === "bar" ? 1 : 780
            host.height = Math.max(minimum, Math.ceil(host.naturalHeight))
            wait(100)
            Testing.Harness.grab(host.view, "")
            wait(50)
            const file = Testing.Harness.outputDir + "/" + data.tag + "-ltr-" + Testing.Harness.scaleLabel + ".png"
            const shot = Testing.Harness.grab(host.view, file)
            verify(shot.saved, "the picture was saved to " + file)
        }
    }
}

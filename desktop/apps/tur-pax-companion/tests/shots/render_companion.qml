// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxCompanion
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing
import ".."

// Renders P@x's character to PNG files in TUR_SCREENSHOT_DIR, dark and
// light, for the screenshot job to compare with the references in
// desktop/tests/screenshots (desktop/ci/screenshots.sh):
//
//  - every scene of CompanionHost.scenes: the character in its seat on a
//    stand-in bar, in each state, with its bubble, the whole notice, the
//    menu and its pages, over a stand-in window. The character's own
//    palette is fixed; what is under it and what opens above it follow the
//    variant, and the pictures show both;
//  - one sheet of the character alone in every state, larger, each with its
//    words, for the design document.
//
// The healing service's words in the pictures are fixtures/heal/notices.json;
// the sentences about P@x's tasks are the recorded service's.
//
// Names: companion-<scene>-<variant>-ltr-<scale>.png.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 700
    height: 1400

    CompanionHost {
        id: host

        anchors.left: parent.left
        anchors.top: parent.top
        width: 640
        height: Math.max(200, Math.ceil(host.wantedHeight) + 56)
    }

    // The sheet: the character alone, in every state, on the bar's colour.
    Rectangle {
        id: sheet

        readonly property var states: ["asleep", "idle", "noticing", "listening", "thinking", "observing", "acting",
                                       "waiting", "paused", "telling", "refused", "error"]

        visible: false
        anchors.left: parent.left
        anchors.top: parent.top
        width: 640
        height: grid.implicitHeight + 32
        color: Kit.Theme.surfaceBase

        Grid {
            id: grid

            x: 16
            y: 16
            columns: 4
            spacing: 8

            Repeater {
                model: sheet.states

                delegate: Rectangle {
                    id: tile

                    required property string modelData

                    width: 146
                    height: 132
                    radius: Kit.Theme.radiusCard
                    color: Kit.Theme.surfaceRaised

                    Character {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: 14
                        size: 64
                        phase: tile.modelData
                    }

                    Kit.Label {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: 8
                        horizontalAlignment: Text.AlignHCenter
                        text: Words.state(tile.modelData)
                        kind: Kit.Label.Small
                    }
                }
            }
        }
    }

    TestCase {
        name: "CompanionPictures"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            host.stop()
            host.visible = true
            sheet.visible = false
            Kit.Theme.light = false
            wait(0)
        }

        function save(item, tag) {
            wait(100)
            Testing.Harness.grab(item, "")
            wait(50)
            const file = Testing.Harness.outputDir + "/" + tag + "-ltr-" + Testing.Harness.scaleLabel + ".png"
            const shot = Testing.Harness.grab(item, file)
            verify(shot.saved, "the picture was saved to " + file)
        }

        function test_render_data() {
            const rows = []
            for (const scene of host.scenes) {
                for (const variant of [{ name: "dark", light: false }, { name: "light", light: true }])
                    rows.push({ tag: "companion-" + scene.name + "-" + variant.name, scene: scene, light: variant.light })
            }
            return rows
        }

        function test_render(data) {
            Kit.Theme.light = data.light
            host.reach(data.scene)
            save(host, data.tag)
        }

        function test_sheet_data() {
            return [{ tag: "companion-states-dark", light: false }, { tag: "companion-states-light", light: true }]
        }

        function test_sheet(data) {
            Kit.Theme.light = data.light
            host.visible = false
            sheet.visible = true
            save(sheet, data.tag)
        }
    }
}

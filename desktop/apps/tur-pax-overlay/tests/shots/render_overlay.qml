// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxOverlay
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// Renders the overlay's states to PNG files in TUR_SCREENSHOT_DIR, for the
// screenshot job to compare with the references in desktop/tests/screenshots
// (desktop-experience.md, 15.4): the pill and frame of every state, a guided
// step (highlight, dimming, caption and pointer at rest) and a click, each
// over a stand-in window drawn in TUR's dark and light variants. The overlay
// itself does not change with the variant (its palette is fixed); what is
// under it does, and the pictures show both.
//
// The stand-in window is made of the kit's controls and labels; its words
// are the design's example (9.6) and stand for no real page.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 760
    height: 400

    Item {
        id: stage

        width: 720
        height: 320

        // The desktop under the window, in the variant being drawn.
        Rectangle {
            anchors.fill: parent
            color: Kit.Theme.surfaceSunken
        }

        // A window under the overlay, in the variant being drawn.
        Rectangle {
            id: window

            x: 20
            y: 56
            width: 680
            height: 248
            color: Kit.Theme.surfaceBase
            border.width: 1
            border.color: Kit.Theme.lineSubtle

            Kit.Label {
                x: 24
                y: 18
                text: "Night Light"
                kind: Kit.Label.Heading
            }

            Repeater {
                model: ["Switching times", "Day colour temperature", "Night colour temperature"]

                delegate: Rectangle {
                    id: line

                    required property string modelData
                    required property int index

                    x: 16
                    y: 64 + line.index * 56
                    width: 648
                    height: 48
                    radius: Kit.Theme.radiusControl
                    color: Kit.Theme.surfaceRaised

                    Kit.Label {
                        x: 16
                        anchors.verticalCenter: parent.verticalCenter
                        text: line.modelData
                    }

                    Rectangle {
                        x: 440
                        anchors.verticalCenter: parent.verticalCenter
                        width: 192
                        height: 30
                        radius: Kit.Theme.radiusControl
                        color: Kit.Theme.surfaceSunken
                        border.width: 1
                        border.color: Kit.Theme.lineControl

                        Kit.Label {
                            x: 10
                            anchors.verticalCenter: parent.verticalCenter
                            text: line.index === 0 ? "Always off" : "4500 K"
                        }
                    }
                }
            }
        }

        OverlayState {
            id: overlay
        }

        OverlaySurface {
            id: surface

            overlay: overlay
            screenName: "picture"
            screenGeometry: Qt.rect(0, 0, 720, 320)
        }
    }

    TestCase {
        name: "OverlayScreenshots"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            Kit.Motion.frozen = true
            overlay.frozen = true
            overlay.setScreenRects([Qt.rect(0, 0, 720, 320)])
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function cleanup() {
            overlay.clear()
            Kit.Theme.light = false
            wait(0)
        }

        // The first row of the stand-in window, in the stage's coordinates.
        function row(index) {
            return Qt.rect(window.x + 16, window.y + 64 + index * 56, 648, 48)
        }

        function scenes() {
            const out = []
            for (const state of ["listening", "working", "observing", "acting", "waiting", "paused"])
                out.push({ name: "state-" + state, state: state })
            out.push({ name: "guide", state: "acting" })
            out.push({ name: "click", state: "acting" })
            return out
        }

        function test_render_data() {
            const scale = Testing.Harness.scaleLabel
            const rows = []
            for (const scene of scenes()) {
                for (const light of [false, true]) {
                    rows.push({
                        tag: "overlay-" + scene.name + "-" + (light ? "light" : "dark") + "-ltr-" + scale,
                        scene: scene,
                        light: light
                    })
                }
            }
            return rows
        }

        function test_render(data) {
            Kit.Theme.light = data.light
            const scene = data.scene
            if (scene.name === "guide") {
                overlay.setActivity("acting", "window", Qt.rect(window.x, window.y, window.width, window.height))
                const target = row(0)
                overlay.addHighlight(target, {
                    dim: true,
                    dimRect: Qt.rect(window.x, window.y, window.width, window.height),
                    step: "Step 2 of 3",
                    caption: "Open this list."
                })
                overlay.movePointer(Qt.point(target.x + 536, target.y + target.height / 2), false)
            } else if (scene.name === "click") {
                overlay.setActivity("acting", "screen", Qt.rect(0, 0, 0, 0))
                const target = row(1)
                overlay.movePointer(Qt.point(target.x + 536, target.y + target.height / 2), false)
                overlay.pulse()
                wait(0)
                // The ring part of the way out, as the eye sees it.
                findChild(surface, "ring").progress = 0.4
            } else {
                overlay.setActivity(scene.state, "screen", Qt.rect(0, 0, 0, 0))
            }
            wait(50)
            Testing.Harness.grab(stage, "")
            wait(50)
            const file = Testing.Harness.outputDir + "/" + data.tag + ".png"
            const shot = Testing.Harness.grab(stage, file)
            verify(shot.saved, "the picture was saved to " + file)
            fuzzyCompare(shot.ratio, Testing.Harness.devicePixelRatio, 0.01, "drawn at the scale asked for")
        }
    }
}

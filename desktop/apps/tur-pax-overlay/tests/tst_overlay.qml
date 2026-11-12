// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxOverlay
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// What one screen's surface draws, under the offscreen platform: the pill's
// words and icon for every state, the frame, a highlight at the measures of
// the design with its caption beside it and never over it, the dimming, and
// P@x's pointer at its tip; two screens side by side each draw their part of
// the same pointer; and nothing on a surface can take the keyboard or a
// click.
Item {
    id: root

    width: 1400
    height: 900

    OverlayState {
        id: overlay
    }

    // Two screens side by side, as the compositor test has them: 640 by 400
    // each, the second at x 640.
    OverlaySurface {
        id: left

        overlay: overlay
        screenName: "Virtual-1"
        screenGeometry: Qt.rect(0, 0, 640, 400)
    }

    OverlaySurface {
        id: right

        y: 450
        overlay: overlay
        screenName: "Virtual-2"
        screenGeometry: Qt.rect(640, 0, 640, 400)
    }

    TestCase {
        name: "Overlay"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
            overlay.frozen = true
            overlay.setScreenRects([Qt.rect(0, 0, 640, 400), Qt.rect(640, 0, 640, 400)])
        }

        function cleanup() {
            overlay.clear()
            wait(0)
        }

        function rectText(r) {
            return [r.x, r.y, r.width, r.height].join(",")
        }

        function pointText(item) {
            return item.x + "," + item.y
        }

        // Every item of a surface, depth first.
        function items(item) {
            let out = [item]
            for (let i = 0; i < item.children.length; ++i)
                out = out.concat(items(item.children[i]))
            return out
        }

        function test_states_have_their_own_words_and_icon() {
            const pill = findChild(left, "pill")
            const seen = {}
            for (const state of ["listening", "working", "observing", "acting", "waiting", "paused"]) {
                overlay.setActivity(state, "window", Qt.rect(0, 0, 0, 0))
                wait(0)
                verify(pill.visible, state + ": the pill is shown")
                verify(pill.text.length > 0, state + ": the pill has words")
                verify(seen[pill.text] === undefined, state + ": its words are its own, not " + seen[pill.text])
                seen[pill.text] = state
                compare(pill.height, 36, "the pill is 36 px high (design, 9.3)")
                compare(findChild(right, "pill").text, pill.text, "every screen shows the same pill")
            }
            overlay.setActivity("observing", "screen", Qt.rect(0, 0, 0, 0))
            compare(pill.text, "P@x can see this screen")
            overlay.setActivity("idle", "window", Qt.rect(0, 0, 0, 0))
            wait(0)
            verify(!pill.visible, "idle: no pill")
            verify(!overlay.active, "idle with nothing else: nothing to draw")
        }

        function test_the_frame_by_state() {
            const frame = findChild(left, "frame")
            const expected = {
                "idle": [false],
                "listening": [false],
                "working": [false],
                "observing": [true, 2, false],
                "acting": [true, 3, false],
                "waiting": [true, 2, true],
                "paused": [true, 2, true]
            }
            for (const state in expected) {
                overlay.setActivity(state, "screen", Qt.rect(0, 0, 0, 0))
                wait(0)
                compare(frame.visible, expected[state][0], state + ": frame drawn")
                if (expected[state][0]) {
                    compare(frame.line, expected[state][1], state + ": line width")
                    compare(frame.dashed, expected[state][2], state + ": dashed")
                }
            }
            // Round a window, in this screen's coordinates.
            overlay.setActivity("observing", "window", Qt.rect(700, 50, 300, 200))
            wait(0)
            verify(!findChild(right, "frame").atEdge)
            compare(rectText(findChild(right, "frame").target), "60,50,300,200")
        }

        function test_a_highlight_has_the_designed_measures() {
            const id = overlay.addHighlight(Qt.rect(100, 120, 300, 40), {})
            wait(0)
            const mark = findChild(left, "highlight-" + id)
            verify(mark !== null)
            compare(mark.clearance, 4)
            compare(mark.line, 3)
            // 4 px clear, 3 px gold, 1 px dark: 8 px all round.
            compare(rectText(mark), "92,112,316,56")
            compare(mark.opacity, 1)
        }

        function test_the_caption_is_beside_the_highlight_never_over_it_data() {
            return [
                { tag: "room on the right", rect: Qt.rect(40, 150, 200, 40), side: "right" },
                { tag: "only room on the left", rect: Qt.rect(400, 150, 200, 40), side: "left" },
                { tag: "a row as wide as the screen", rect: Qt.rect(20, 100, 600, 40), side: "below" },
                { tag: "a wide row at the bottom", rect: Qt.rect(20, 330, 600, 40), side: "above" }
            ]
        }

        function test_the_caption_is_beside_the_highlight_never_over_it(data) {
            const id = overlay.addHighlight(data.rect, { caption: "Open this list and choose “Sunset and sunrise”.",
                                                         step: "Step 2 of 3" })
            wait(0)
            const mark = findChild(left, "highlight-" + id)
            const bubble = findChild(left, "caption-" + id)
            verify(bubble.visible && bubble.opacity === 1)
            compare(findChild(bubble, "captionStep").text, "Step 2 of 3")
            const overlaps = bubble.x < mark.x + mark.width && mark.x < bubble.x + bubble.width
                             && bubble.y < mark.y + mark.height && mark.y < bubble.y + bubble.height
            verify(!overlaps, "the caption does not cover the highlight")
            verify(bubble.x >= 0 && bubble.x + bubble.width <= 640, "the caption is on the screen")
            verify(bubble.y >= 0 && bubble.y + bubble.height <= 400, "the caption is on the screen")
            const side = bubble.x >= mark.x + mark.width ? "right"
                       : bubble.x + bubble.width <= mark.x ? "left"
                       : bubble.y >= mark.y + mark.height ? "below" : "above"
            compare(side, data.side)
            compare(findChild(right, "caption-" + id).opacity, 0, "only the screen with the highlight shows its caption")
        }

        function test_dimming_leaves_the_highlight_clear() {
            overlay.addHighlight(Qt.rect(100, 120, 300, 40), { dim: true, dimRect: Qt.rect(50, 50, 500, 300) })
            wait(0)
            const grabbed = grabImage(left)
            // Outside the window nothing is drawn: that is the plain colour.
            const plain = grabbed.pixel(600, 380)
            verify(!Qt.colorEqual(grabbed.pixel(60, 60), plain), "the window round the row is dimmed")
            verify(Qt.colorEqual(grabbed.pixel(250, 140), plain), "the row itself is not dimmed")
        }

        function test_one_pointer_across_two_screens() {
            overlay.movePointer(Qt.point(300, 200), false)
            wait(0)
            const onLeft = findChild(left, "pointer")
            const onRight = findChild(right, "pointer")
            verify(onLeft.visible)
            compare(pointText(onLeft), "300,200", "the tip is the point")
            compare(onLeft.size, 1.5 * overlay.cursorSize, "one and a half times the cursor")
            verify(findChild(onLeft, "pointerTag").visible, "at rest, the tag shows")
            // The other screen draws it where it is in its own coordinates:
            // off its left edge.
            compare(pointText(onRight), "-340,200")

            overlay.movePointer(Qt.point(900, 100), false)
            wait(0)
            compare(pointText(onRight), "260,100")
            compare(pointText(onLeft), "900,100")

            overlay.hidePointer()
            wait(0)
            verify(!onLeft.visible && !onRight.visible)
        }

        function test_nothing_takes_input() {
            overlay.setActivity("acting", "screen", Qt.rect(0, 0, 0, 0))
            overlay.addHighlight(Qt.rect(100, 120, 300, 40), { dim: true, caption: "Here", step: "Step 1 of 1" })
            overlay.movePointer(Qt.point(250, 140), false)
            wait(0)
            compare(Testing.Harness.tabStops(left).length, 0, "nothing on the surface is a tab stop")
            for (const item of items(left)) {
                const type = String(item)
                for (const handler of ["MouseArea", "TapHandler", "DragHandler", "HoverHandler", "PointHandler",
                                       "WheelHandler", "Flickable", "TextInput", "TextEdit", "Button"]) {
                    verify(type.indexOf(handler) < 0, "no " + handler + " on the overlay: " + type)
                }
                verify(!item.activeFocusOnTab, "nothing asks for the keyboard: " + type)
            }
        }
    }
}

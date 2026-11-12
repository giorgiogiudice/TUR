// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit

// Motion (desktop-experience.md, 5.5 and 13.1, X9): three durations; with
// the desktop's animation setting at zero nothing moves, and a cross-fade is
// at most 100 ms.
TestCase {
    name: "Motion"

    function cleanup() {
        Kit.Motion.factor = 1
        Kit.Motion.frozen = false
    }

    function test_three_durations() {
        Kit.Motion.factor = 1
        compare(Kit.Motion.control, 120)
        compare(Kit.Motion.appear, 200)
        compare(Kit.Motion.panel, 320)
        compare(Kit.Motion.fadePanel, 320)
        verify(Kit.Motion.loops)
        verify(!Kit.Motion.reduced)
    }

    function test_follows_the_factor() {
        Kit.Motion.factor = 0.5
        compare(Kit.Motion.control, 60)
        compare(Kit.Motion.panel, 160)
        compare(Kit.Motion.fadeAppear, 100)
    }

    function test_reduced_motion() {
        Kit.Motion.factor = 0
        verify(Kit.Motion.reduced)
        compare(Kit.Motion.control, 0, "nothing moves")
        compare(Kit.Motion.appear, 0, "nothing moves")
        compare(Kit.Motion.panel, 0, "nothing moves")
        verify(Kit.Motion.fadeControl <= 100)
        verify(Kit.Motion.fadeAppear <= 100)
        verify(Kit.Motion.fadePanel <= 100)
        compare(Kit.Motion.fadePanel, 100, "a fade is still allowed, up to 100 ms")
        verify(!Kit.Motion.loops, "nothing loops")
    }

    function test_frozen_for_pictures() {
        Kit.Motion.frozen = true
        compare(Kit.Motion.control, 0)
        compare(Kit.Motion.fadePanel, 0)
        verify(!Kit.Motion.loops)
    }

    function test_default_follows_the_desktop() {
        // TUR_ANIMATION_FACTOR is not set in the tests and there is no
        // kdeglobals: normal speed.
        compare(Kit.SystemAppearance.animationFactor, 1)
    }
}

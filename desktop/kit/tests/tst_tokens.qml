// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// The tokens as a running program sees them, against tokens.json: the same
// values, and every declared colour pair at or above its WCAG threshold, in
// both variants (desktop-experience.md, 5.2 and 13.1, X5 and X10).
TestCase {
    id: testCase

    name: "Tokens"

    property var tokens: null

    function camel(name) {
        return name.replace(/\.(\w)/g, (match, letter) => letter.toUpperCase())
    }

    // The live colour of a token, system or P@x's.
    function live(name) {
        if (name === "pax")
            return Pax.PaxTheme.pax
        if (name === "pax.fill")
            return Pax.PaxTheme.fill
        if (name === "pax.on")
            return Pax.PaxTheme.fillText
        return Kit.Theme[camel(name)]
    }

    function initTestCase() {
        tokens = Testing.Harness.readJson(Testing.Harness.tokensFile)
        verify(tokens !== undefined && tokens !== null, "tokens.json was read from " + Testing.Harness.tokensFile)
    }

    function cleanup() {
        Kit.Theme.light = false
        Kit.Theme.textScale = 1
    }

    function test_colours_match_the_source_data() {
        return [
            { tag: "dark", variant: "dark", light: false },
            { tag: "light", variant: "light", light: true }
        ]
    }

    function test_colours_match_the_source(data) {
        Kit.Theme.light = data.light
        let count = 0
        for (const name in tokens.colours) {
            const want = tokens.colours[name][data.variant]
            verify(Qt.colorEqual(live(name), want), name + " is " + live(name) + ", tokens.json says " + want)
            count++
        }
        for (const name in tokens.pax.colours) {
            const want = tokens.pax.colours[name][data.variant]
            verify(Qt.colorEqual(live(name), want), name + " is " + live(name) + ", tokens.json says " + want)
            count++
        }
        compare(count, 20, "the design's table has twenty colour tokens")
        verify(Qt.colorEqual(Pax.PaxTheme.overlayFill, tokens.pax.overlay.fill))
        verify(Qt.colorEqual(Pax.PaxTheme.overlayOutline, tokens.pax.overlay.outline))
        verify(Qt.colorEqual(Pax.PaxTheme.overlayHairline, tokens.pax.overlay.hairline))
        verify(Qt.colorEqual(Pax.PaxTheme.overlayText, tokens.pax.overlay.text))
    }

    function test_contrast_pairs_data() {
        return test_colours_match_the_source_data()
    }

    function test_contrast_pairs(data) {
        Kit.Theme.light = data.light
        const thresholds = tokens.contrast.thresholds
        const failures = []
        for (const pair of tokens.contrast.pairs) {
            const ratio = Testing.Harness.contrast(live(pair.fg), live(pair.bg))
            const need = thresholds[pair.needs]
            if (ratio < need)
                failures.push(pair.fg + " on " + pair.bg + " is " + ratio.toFixed(2) + ":1, below " + need + ":1")
            const stated = pair[data.variant]
            if (stated !== null && Math.abs(ratio - stated) > 0.06)
                failures.push(pair.fg + " on " + pair.bg + " is " + ratio.toFixed(2) + ":1, the design states " + stated)
        }
        compare(failures.join("; "), "")
        verify(tokens.contrast.pairs.length >= 30)
    }

    function test_contrast_formula() {
        // White on black, from the high-contrast palette: the largest ratio there is.
        const white = tokens.fixed.highContrast.text
        const black = tokens.fixed.highContrast.ground
        fuzzyCompare(Testing.Harness.contrast(white, black), 21, 0.001)
        fuzzyCompare(Testing.Harness.contrast(black, white), 21, 0.001)
        fuzzyCompare(Testing.Harness.contrast(white, white), 1, 0.001)
    }

    function test_metrics_match_the_source() {
        compare(Kit.Theme.spaceGrid, tokens.space.grid)
        compare(Kit.Theme.spaceRelated, tokens.space.related)
        compare(Kit.Theme.spaceGroup, tokens.space.group)
        compare(Kit.Theme.spaceMargin, tokens.space.margin)
        compare(Kit.Theme.radiusControl, tokens.radius.control)
        compare(Kit.Theme.radiusCard, tokens.radius.card)
        compare(Kit.Theme.radiusSheet, tokens.radius.sheet)
        compare(Kit.Theme.targetPointer, tokens.target.pointer)
        compare(Kit.Theme.targetTouch, tokens.target.touch)
        compare(Kit.Theme.lineFocusWidth, tokens.line.focusWidth)
        compare(Kit.Theme.typeDisplaySize, tokens.type.styles.display.size)
        compare(Kit.Theme.typeBodySize, tokens.type.styles.body.size)
        compare(Kit.Theme.typeSmallSize, tokens.type.styles.small.size)
        compare(Kit.Theme.typeHeadingWeight, tokens.type.styles.heading.weight)
        compare(Kit.Theme.monoFamily, tokens.type.mono)
        compare(Kit.Theme.motionControl, tokens.motion.control)
        compare(Kit.Theme.motionAppear, tokens.motion.appear)
        compare(Kit.Theme.motionPanel, tokens.motion.panel)
    }

    // X10: targets are at least 24 px; TUR's are 36 and 44.
    function test_targets() {
        verify(Kit.Theme.targetPointer >= Kit.Theme.targetMinimum)
        verify(Kit.Theme.targetTouch >= Kit.Theme.targetPointer)
        compare(Kit.Theme.targetMinimum, 24)
    }

    function test_nothing_is_smaller_than_small() {
        const styles = tokens.type.styles
        for (const name in styles)
            verify(styles[name].size >= styles.small.size, name + " is smaller than the smallest style")
    }

    function test_dark_by_default() {
        Kit.Theme.light = true
        Kit.Theme.followSystem()
        compare(Kit.Theme.light, Kit.SystemAppearance.prefersLight)
        compare(Kit.Theme.light, false, "the standalone look does not ask the desktop: dark")
    }

    function test_text_scale() {
        const before = Kit.Theme.fontScale
        Kit.Theme.textScale = 2
        fuzzyCompare(Kit.Theme.fontScale, before * 2, 0.0001)
    }
}

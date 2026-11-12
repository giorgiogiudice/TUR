// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxOverlay
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// Contrast of everything the overlay draws, over what it may be drawn on
// (desktop-experience.md, 13.1, X5). The overlay has a fixed palette and is
// drawn over content TUR does not control, so each mark is checked over
// every surface colour of both of TUR's variants, and over white. Gold on a
// light background is the hard case: #E8B44C on white is 1.9:1.
//
// The rules, from WCAG 2.2 (1.4.3 and 1.4.11):
//  - Words: 4.5:1 against what is behind them.
//  - A line (highlight, frame, ring): one of its lines must stand 3:1 from
//    both of its neighbours, the lines beside it or the content on either
//    side, so that the line can be seen whatever is under it.
//  - A filled shape (pill, pointer, caption): one of its layers must stand
//    3:1 from the content round it, so that its outline can be seen.
// Each component lists its colours from the outermost inwards (`layers`);
// the test reads them from the component, not from a table.
Item {
    id: root

    width: 800
    height: 300

    ActivityPill {
        id: pill
    }

    PaxPointer {
        id: pointer
    }

    HighlightMark {
        id: mark
    }

    ScreenFrame {
        id: frame
    }

    PointerRing {
        id: ring
    }

    CaptionBubble {
        id: bubble
    }

    CommandsCard {
        id: card
    }

    TestCase {
        name: "OverlayContrast"
        when: windowShown

        readonly property real line: 3.0
        readonly property real words: 4.5

        function backdrops() {
            const out = []
            for (const light of [false, true]) {
                Kit.Theme.light = light
                const variant = light ? "light" : "dark"
                out.push({ name: variant + " surface.base", colour: Kit.Theme.surfaceBase })
                out.push({ name: variant + " surface.raised", colour: Kit.Theme.surfaceRaised })
                out.push({ name: variant + " surface.overlay", colour: Kit.Theme.surfaceOverlay })
                out.push({ name: variant + " surface.sunken", colour: Kit.Theme.surfaceSunken })
            }
            Kit.Theme.light = false
            out.push({ name: "the overlay's hairline (pure white)", colour: Pax.PaxTheme.overlayHairline })
            return out
        }

        function ratio(a, b) {
            return Testing.Harness.contrast(a, b)
        }

        // The best a line can do: the highest, over its layers, of the lower
        // of the two contrasts with its neighbours.
        function lineScore(layers, behind) {
            let best = 0
            for (let i = 0; i < layers.length; ++i) {
                const outside = i === 0 ? behind : layers[i - 1]
                const inside = i === layers.length - 1 ? behind : layers[i + 1]
                best = Math.max(best, Math.min(ratio(layers[i], outside), ratio(layers[i], inside)))
            }
            return best
        }

        // A filled shape: the best of its layers against the content.
        function shapeScore(layers, behind) {
            let best = 0
            for (const colour of layers)
                best = Math.max(best, ratio(colour, behind))
            return best
        }

        function judge(kind, name, layers) {
            const failures = []
            let lowest = 21
            for (const backdrop of backdrops()) {
                const score = kind === "line" ? lineScore(layers, backdrop.colour) : shapeScore(layers, backdrop.colour)
                lowest = Math.min(lowest, score)
                if (score < line)
                    failures.push(name + " over " + backdrop.name + ": " + score.toFixed(2) + ":1")
            }
            console.info("TUR-CONTRAST overlay " + name + ": lowest " + lowest.toFixed(2) + ":1 over "
                         + backdrops().length + " backgrounds")
            compare(failures.join("; "), "", name + " can be seen on every background")
        }

        function test_lines() {
            judge("line", "highlight", mark.layers)
            judge("line", "click ring", ring.layers)
            for (const state of ["observing", "acting", "waiting", "paused"]) {
                frame.activity = state
                judge("line", "frame (" + state + ")", frame.layers)
            }
        }

        function test_shapes() {
            judge("shape", "pointer", pointer.layers)
            judge("shape", "caption", bubble.layers)
            judge("shape", "list of keys", card.layers)
            for (const state of ["idle", "listening", "working", "observing", "acting", "waiting", "paused"]) {
                pill.activity = state
                judge("shape", "pill (" + state + ")", pill.layers)
            }
        }

        function test_words() {
            for (const state of ["idle", "listening", "working", "observing", "acting", "waiting", "paused"]) {
                pill.activity = state
                const r = ratio(pill.textColour, pill.textBackground)
                console.info("TUR-CONTRAST overlay pill words (" + state + "): " + r.toFixed(2) + ":1")
                verify(r >= words, state + ": the pill's words are " + r.toFixed(2) + ":1")
                verify(ratio(pill.glyphColour, pill.textBackground) >= line, state + ": the icon")
            }
            verify(ratio(Pax.PaxTheme.overlayQuietText, Pax.PaxTheme.overlayOutline) >= words, "the caption's sentence")
            verify(ratio(Pax.PaxTheme.overlayPaused, Pax.PaxTheme.overlayOutline) >= words, "the caption's step")
            verify(ratio(Pax.PaxTheme.overlayText, Pax.PaxTheme.overlayFill) >= words, "the pointer's tag")
            verify(ratio(Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayOutline) >= words, "the keys in the list")
        }

        // Gold alone would not do: the dark line is what carries the
        // highlight on white. This is the reason for it, kept as a test.
        function test_gold_alone_fails_on_white() {
            verify(ratio(Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayHairline) < line)
            verify(lineScore([Pax.PaxTheme.overlayFill], Pax.PaxTheme.overlayHairline) < line)
        }
    }
}

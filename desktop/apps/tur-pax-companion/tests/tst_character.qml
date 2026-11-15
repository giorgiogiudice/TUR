// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxCompanion
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// The character itself (docs/architecture/tur-pax-companion.md, section 1):
//
//  - every state has its own words and its own look, and the looks differ
//    in shape or in their mark, never in colour alone;
//  - it can be seen on anything: its palette is fixed, and each of its
//    shapes has a layer that stands 3:1 from every surface colour of both
//    of TUR's variants and from white (WCAG 2.2, 1.4.11), dimmed on battery
//    included;
//  - nothing loops unless P@x is really working or controlling, and nothing
//    loops at all with reduced motion, with motion frozen, or on battery.
Item {
    id: root

    width: 400
    height: 200

    Character {
        id: character

        size: 40
    }

    StateMark {
        id: mark

        tint: Pax.PaxTheme.overlayFill
    }

    TestCase {
        name: "CompanionCharacter"
        when: windowShown

        readonly property var states: ["asleep", "idle", "noticing", "listening", "thinking", "observing", "acting",
                                       "waiting", "paused", "telling", "refused", "error"]
        readonly property real line: 3.0

        function cleanup() {
            character.phase = "idle"
            character.saving = false
            character.fuel = 1
            Kit.Motion.frozen = false
            Kit.Motion.factor = 1
            Kit.Theme.light = false
        }

        function test_every_state_has_its_own_words() {
            const seen = {}
            for (const state of states) {
                const words = Words.state(state)
                verify(words.length > 0, state + " has words")
                verify(seen[words] === undefined, state + " and " + seen[words] + " share the words “" + words + "”")
                seen[words] = state
            }
            compare(Words.state("no such state"), "")
        }

        // What a person who sees no colour can tell the states apart by:
        // the mark, the disc, whether there is an inner flame, how tall and
        // how tilted the flame is, how big its core is.
        function shape(state) {
            character.phase = state
            return [character.mark, character.loud, character.hasInner, character.hasCore,
                    character.stretch.toFixed(2), character.spread.toFixed(2), character.lean.toFixed(1),
                    character.coreScale.toFixed(2), character.coreRise.toFixed(1)].join("|")
        }

        function test_every_state_has_its_own_shape() {
            const seen = {}
            for (const state of states) {
                const drawn = shape(state)
                verify(seen[drawn] === undefined,
                       state + " and " + seen[drawn] + " would look the same without colour: " + drawn)
                seen[drawn] = state
            }
        }

        function test_every_mark_is_drawn() {
            for (const state of states) {
                character.phase = state
                if (character.mark.length === 0) {
                    // Asleep, idle and controlling are told by the flame itself.
                    verify(["asleep", "idle", "acting"].indexOf(state) >= 0, state + " has a mark")
                    continue
                }
                mark.kind = character.mark
                verify(mark.filled.length > 0 || mark.stroked.length > 0, state + ": its mark has a drawing")
            }
            // Loud is the indicator's loud: a gold disc, the flame dark in it.
            character.phase = "acting"
            verify(character.loud)
            verify(Qt.colorEqual(character.innerFill, Pax.PaxTheme.overlayFill))
            character.phase = "asleep"
            verify(!character.hasInner && !character.hasCore, "asleep, the flame is out")
        }

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
            out.push({ name: "pure white", colour: Pax.PaxTheme.overlayHairline })
            out.push({ name: "the overlay's dark", colour: Pax.PaxTheme.overlayOutline })
            return out
        }

        // A colour drawn at `opacity` over `behind`.
        function blended(colour, behind, opacity) {
            return Qt.rgba(colour.r * opacity + behind.r * (1 - opacity),
                           colour.g * opacity + behind.g * (1 - opacity),
                           colour.b * opacity + behind.b * (1 - opacity), 1)
        }

        // A filled shape: the best of its layers against the content.
        function shapeScore(layers, behind, opacity) {
            let best = 0
            for (const colour of layers)
                best = Math.max(best, Testing.Harness.contrast(blended(colour, behind, opacity), behind))
            return best
        }

        function test_it_can_be_seen_on_anything_data() {
            return [{ tag: "awake", opacity: 1 }, { tag: "dimmed-on-battery", opacity: 0.78 }]
        }

        function test_it_can_be_seen_on_anything(data) {
            const failures = []
            let lowest = 21
            for (const state of states) {
                character.phase = state
                for (const backdrop of backdrops()) {
                    const body = shapeScore(character.layers, backdrop.colour, data.opacity)
                    lowest = Math.min(lowest, body)
                    if (body < line)
                        failures.push(state + " over " + backdrop.name + ": " + body.toFixed(2) + ":1")
                    if (character.mark.length === 0)
                        continue
                    const ground = shapeScore(character.markLayers, backdrop.colour, data.opacity)
                    if (ground < line)
                        failures.push(state + "'s mark over " + backdrop.name + ": " + ground.toFixed(2) + ":1")
                }
            }
            console.info("TUR-CONTRAST companion character (" + data.tag + "): lowest " + lowest.toFixed(2)
                         + ":1 over " + backdrops().length + " backgrounds, " + states.length + " states")
            compare(failures.join("; "), "", "the character can be seen on every background")
            // The sign in the mark, on the mark's own ground.
            const sign = Testing.Harness.contrast(character.markGlyph, character.markGround)
            verify(sign >= line, "the sign in the mark is " + sign.toFixed(2) + ":1")
        }

        // Gold alone would not do on a light bar: the dark line is what
        // carries the flame there. The reason for it, kept as a test.
        function test_gold_alone_fails_on_a_light_bar() {
            verify(Testing.Harness.contrast(Pax.PaxTheme.overlayFill, Pax.PaxTheme.overlayHairline) < line)
        }

        function test_nothing_loops_unless_pax_is_really_busy() {
            Kit.Motion.frozen = false
            Kit.Motion.factor = 1
            for (const state of states) {
                character.phase = state
                compare(character.looping, state === "thinking" || state === "acting",
                        state + (character.looping ? " loops" : " does not loop"))
            }
        }

        function test_nothing_loops_with_reduced_motion_frozen_or_on_battery() {
            for (const state of ["thinking", "acting"]) {
                character.phase = state
                Kit.Motion.frozen = false
                Kit.Motion.factor = 1
                verify(character.looping)
                // Reduced motion: the desktop's animation speed is zero.
                Kit.Motion.factor = 0
                verify(!character.looping, state + " loops with reduced motion")
                compare(character.sway, 0)
                compare(character.beat, 0)
                Kit.Motion.factor = 1
                // For a picture.
                Kit.Motion.frozen = true
                verify(!character.looping, state + " loops while frozen")
                Kit.Motion.frozen = false
                // On battery it keeps still.
                character.saving = true
                verify(!character.looping, state + " loops on battery")
                compare(character.opacity, 0.78)
                character.saving = false
                compare(character.opacity, 1)
            }
        }

        function test_it_fits_its_box() {
            for (const size of [24, 40, 64]) {
                character.size = size
                compare(character.implicitWidth, size)
                compare(character.implicitHeight, size)
                verify(character.flameSize < size)
            }
            character.size = 40
        }
    }
}

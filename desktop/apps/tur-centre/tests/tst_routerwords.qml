// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit.Testing as Testing

// The router's sentences, written from their identifiers (issue #41;
// qml/RouterWords.qml), against the router's own English in the recorded
// answers: for every option of every recorded route, the reason written from
// `reason_parts`, every prerequisite written from its code and every caveat
// written from its `text_id` must be the router's text, word for word. So the
// table can be translated and cannot drift from the router.
Item {
    id: root

    width: 400
    height: 300

    TestCase {
        name: "RouterWords"

        readonly property var recorded: [
            "default/route-com.bandainamco.eldenring.json",
            "default/route-com.microsoft.office.json",
            "default/route-com.riotgames.valorant.json",
            "default/route-org.kde.krita.json",
            "default/route-org.libreoffice.libreoffice.json",
            "default/route-tur.class.android-banking.json",
            "default/route-net.sourceforge.chromium-bsu.json",
            "default/route-com.google.chrome.json",
            "default/route-org.notepad-plus-plus.notepad-plus-plus.json",
            "assumed-host/route-com.netflix.netflix.json"
        ]

        function options() {
            const out = []
            for (const file of recorded) {
                const fixture = Testing.Harness.readJson(centreFixtures + "/" + file)
                verify(fixture !== undefined, file + " can be read")
                for (const invocation of fixture.invocations) {
                    if (invocation.stdout && invocation.stdout.options) {
                        for (const option of invocation.stdout.options)
                            out.push({ file: file, option: option })
                    }
                }
            }
            return out
        }

        function test_every_reason_is_the_router_s_own_words() {
            let written = 0
            for (const { file, option } of options()) {
                // Every part has its sentence here: nothing falls back.
                for (const part of option.reason_parts) {
                    verify(RouterWords.reasonPart(part, option) !== undefined,
                           file + ": the reason part \"" + part.code + "\" has its sentence")
                }
                compare(RouterWords.reason(option), option.reason, file + ", " + option.route)
                written++
            }
            verify(written >= 20, written + " options")
        }

        function test_every_prerequisite_is_the_router_s_own_words() {
            let written = 0
            for (const { file, option } of options()) {
                for (const prerequisite of option.prerequisites) {
                    verify(prerequisite.code in RouterWords.prerequisites, file + ": " + prerequisite.code)
                    compare(RouterWords.prerequisite(prerequisite), prerequisite.text, file + ", " + prerequisite.code)
                    written++
                }
            }
            verify(written > 0)
        }

        function test_every_caveat_is_the_router_s_own_words() {
            let fromTable = 0
            for (const { file, option } of options()) {
                for (const caveat of option.caveats) {
                    compare(RouterWords.caveat(caveat), caveat.text, file + ", " + caveat.text_id)
                    if (caveat.text_id in RouterWords.caveats)
                        fromTable++
                }
            }
            verify(fromTable > 0, "some caveats are written from the table")
        }

        function test_every_reason_code_and_prerequisite_of_the_router_has_its_sentence() {
            const vocabulary = Testing.Harness.readJson(centreFixtures + "/vocabulary.json")
            for (const code of vocabulary.reason_codes) {
                const part = { code: code, params: { rating: "broken", route: "wine", better_rating: "good",
                                                     name: "Example", caveat: "community-package" } }
                const option = { caveats: [{ text_id: "community-package", params: {},
                                             text: "This package is maintained by its packagers, not by the application's publisher." }] }
                verify(RouterWords.reasonPart(part, option) !== undefined, "the reason code " + code)
            }
            for (const code of vocabulary.prerequisite_codes)
                verify(code in RouterWords.prerequisites, "the prerequisite " + code)
            for (const id of Object.keys(RouterWords.caveats))
                verify(vocabulary.caveat_text_ids.indexOf(id) >= 0, id + " is one of the router's identifiers")
        }

        function test_what_this_version_cannot_write_is_the_router_s_text() {
            const option = { reason: "The router's own sentence.",
                             reason_parts: [{ code: "something-new", params: {} }], caveats: [] }
            compare(RouterWords.reason(option), "The router's own sentence.")
            compare(RouterWords.prerequisite({ code: "something-new", text: "Do this first." }), "Do this first.")
            compare(RouterWords.caveat({ text_id: "community-package", params: { x: "1" }, text: "As the router says." }),
                    "As the router says.")
        }
    }
}

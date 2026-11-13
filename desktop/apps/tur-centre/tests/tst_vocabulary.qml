// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit.Testing as Testing

// The fixed vocabulary (desktop-experience.md, section 8.2) against the
// router's own values.
//
// fixtures/vocabulary.json lists every value of the router's types that
// reaches the screen. It is written from those types by
// services/tur-catalogue/tests/desktop_fixtures.rs, in the Rust workflow, so
// it cannot fall behind the router. This test fails if the router has a
// route, a rating, an outlook or a severity with no words here (the design's
// risk 17), and holds the words to the design's tables.
Item {
    id: root

    width: 400
    height: 300

    Catalogue {
        id: catalogue
    }

    TestCase {
        name: "Vocabulary"

        property var router: undefined

        function initTestCase() {
            router = Testing.Harness.readJson(centreFixtures + "/vocabulary.json")
            verify(router !== undefined, "fixtures/vocabulary.json can be read")
        }

        // A list as a sorted array, whatever kind of list it is.
        function sorted(list) {
            return Array.from(list).sort()
        }

        function test_every_route_has_its_label_and_its_explanation() {
            verify(router.routes.length >= 10)
            for (const route of router.routes) {
                verify(route in Words.routeLabels, "the route \"" + route + "\" has a label")
                verify(Words.routeLabel(route, "").length > 0, "the label of \"" + route + "\" is not empty")
                verify(route in Words.routeExplanations, "the route \"" + route + "\" has an explanation")
                // "No route" is explained by its own page.
                if (route !== "no-route")
                    verify(Words.routeExplanation(route).length > 0)
            }
            // And no label for a route the router does not have.
            compare(sorted(Object.keys(Words.routeLabels)), sorted(router.routes))
            compare(sorted(Object.keys(Words.routeExplanations)), sorted(router.routes))
        }

        function test_the_labels_are_the_design_s_data() {
            return [
                { tag: "flatpak", route: "flatpak", label: "Linux application",
                  explanation: "Made for Linux. Runs directly." },
                { tag: "native", route: "native", label: "Linux application",
                  explanation: "Made for Linux. Runs directly." },
                { tag: "proton", route: "proton", label: "Windows game, through Proton",
                  explanation: "Runs without Windows, through Proton, the compatibility layer used by the Steam Deck." },
                { tag: "wine", route: "wine", label: "Windows application, in the compatibility layer",
                  explanation: "Runs without Windows. Fastest and lightest, but not every application works." },
                { tag: "windows-guest", route: "windows-guest", label: "Windows application, in Windows",
                  explanation: "Runs inside a real Windows that TUR starts in the background (a virtual machine). "
                               + "Needs your own Windows licence and more memory." },
                { tag: "android-runtime", route: "android-runtime", label: "Android application",
                  explanation: "Runs in the Android system. Android must be switched on." },
                { tag: "web", route: "web", label: "Web application",
                  explanation: "Opens the service's website in its own window." },
                { tag: "darling", route: "darling", label: "macOS application (experimental)",
                  explanation: "An experiment. Most macOS applications do not run." },
                { tag: "no-route", route: "no-route", label: "Cannot run on TUR", explanation: "" }
            ]
        }

        function test_the_labels_are_the_design_s(data) {
            compare(Words.routeLabel(data.route, ""), data.label)
            compare(Words.routeExplanation(data.route), data.explanation)
        }

        function test_another_linux_system_is_named_when_the_catalogue_names_it() {
            compare(Words.routeLabel("distro-container", "debian"), "Linux application, from debian")
            compare(Words.routeLabel("distro-container", ""), "Linux application, from another Linux system")
            compare(Words.routeExplanation("distro-container"),
                    "Made for another Linux system. Runs in a small copy of that system.")
            // Only that route takes the name.
            compare(Words.routeLabel("flatpak", "debian"), "Linux application")
        }

        function test_every_rating_has_its_own_words() {
            compare(sorted(Object.keys(Words.ratingLabels)), sorted(router.ratings))
            compare(Words.ratingLabel("good"), "Works well")
            compare(Words.ratingLabel("usable"), "Works, with minor problems")
            compare(Words.ratingLabel("poor"), "Works badly")
            compare(Words.ratingLabel("broken"), "Does not work")
            // Unknown is unknown. It is not "works", and it is not "does
            // not work".
            compare(Words.ratingLabel("unknown"), "No reports")
            const seen = []
            for (const rating of router.ratings) {
                const label = Words.ratingLabel(rating)
                verify(label.length > 0, "the rating \"" + rating + "\" has words")
                verify(seen.indexOf(label) < 0, "no two ratings share their words")
                seen.push(label)
            }
        }

        function test_every_outlook_and_every_severity_is_known() {
            compare(sorted(Object.keys(Words.outlookLabels)), sorted(router.outlooks))
            // `clear` is nothing extra: it is not a promise.
            compare(Words.outlookLabel("clear"), "")
            compare(Words.outlookLabel("doubtful"), "May not work")
            compare(Words.outlookLabel("blocked"), "Cannot be used on this computer")

            compare(sorted(Object.keys(Words.severityLabels)), sorted(router.severities))
            compare(sorted(Words.severityOrder), sorted(router.severities))
            // Blockers first.
            compare(Words.severityOrder[0], "blocker")
            for (const severity of router.severities) {
                verify(Words.severityLabel(severity).length > 0)
                const spoken = Words.spokenCaveat(severity, "The router's sentence.")
                verify(spoken.indexOf("The router's sentence.") > 0, "a screen reader hears the router's sentence")
                verify(spoken.indexOf(Words.severityLabel(severity)) === 0, "after the word its sign stands for")
            }
            compare(Words.severityLabel("obstacle"), "Probably will not work fully")
        }

        function test_what_the_views_know_is_what_the_backend_accepts() {
            // src/answers.cpp refuses a value it does not know. What it
            // knows, the views have words for, and the other way round.
            const known = catalogue.knownVocabulary()
            compare(sorted(known.routes), sorted(Object.keys(Words.routeLabels)))
            compare(sorted(known.ratings), sorted(Object.keys(Words.ratingLabels)))
            compare(sorted(known.outlooks), sorted(Object.keys(Words.outlookLabels)))
            compare(sorted(known.severities), sorted(Object.keys(Words.severityLabels)))
            compare(sorted(known.rating_source_kinds), sorted(router.rating_source_kinds))
        }

        function test_every_category_has_a_label() {
            compare(sorted(Object.keys(Words.categoryLabels)), sorted(router.categories))
            for (const category of router.categories)
                verify(Words.categoryLabel(category).length > 0)
            compare(Words.categoryLabel("productivity"), "Productivity")
        }

        function test_the_stores_and_the_code_are_the_catalogue_s() {
            for (const store of Object.keys(Words.storeNames))
                verify(router.obtain.indexOf(store) >= 0, store + " is a place the catalogue says an edition is obtained")
            verify(router.error_codes.indexOf(Words.noRouteCode) >= 0,
                   Words.noRouteCode + " is one of the catalogue's codes")
            // No alternative and no requirement leaves the "no route" page
            // without words: what the design names no sentence for is said
            // in the router's.
            for (const kind of router.alternative_kinds) {
                const option = { route: "no-route", reason: "The router's reason.", caveats: [],
                                 alternative: { kind: kind, text: "The router's alternative." } }
                verify(Words.noRouteWhy(option, { category: "utility" }).length > 0, kind)
                verify(Words.noRouteAlternatives(option, { category: "utility" }).length > 0, kind)
            }
            for (const requirement of router.requirements) {
                const option = { route: "no-route", reason: "The router's reason.",
                                 caveats: [{ code: "requirement", severity: "blocker", requirement: requirement,
                                             text: "The blocker." }],
                                 alternative: { kind: "none-known", text: "The router's alternative." } }
                verify(Words.noRouteWhy(option, { category: "utility" }).length > 0, requirement)
                verify(Words.noRouteAlternatives(option, { category: "utility" }).length > 0, requirement)
            }
        }

        function test_according_to_whom_data() {
            return [
                { tag: "nobody", source: { kind: "unrated" },
                  sentence: "Nobody has reported on this way of running it. It may still work." },
                { tag: "no source at all", source: null,
                  sentence: "Nobody has reported on this way of running it. It may still work." },
                { tag: "external, with its word and the version",
                  source: { kind: "external", name: "WineHQ AppDB", url: "https://appdb.winehq.org/",
                            source_rating: "Platinum", version_tested: "CC 2024", date: "2026-11-12" },
                  sentence: "Reported by WineHQ AppDB as \"Platinum\" · version CC 2024 · checked 12 November 2026"
                            + " · Not tested by TUR" },
                { tag: "external, with its word",
                  source: { kind: "external", name: "ProtonDB", url: "https://www.protondb.com/",
                            source_rating: "Native", version_tested: null, date: "2026-11-12" },
                  sentence: "Reported by ProtonDB as \"Native\" · checked 12 November 2026 · Not tested by TUR" },
                { tag: "external, with the version only",
                  source: { kind: "external", name: "Example DB", url: "https://db.example.org/",
                            source_rating: null, version_tested: "2.6", date: "2026-01-05" },
                  sentence: "Reported by Example DB · version 2.6 · checked 5 January 2026 · Not tested by TUR" },
                { tag: "external, with neither",
                  source: { kind: "external", name: "Example DB", url: "https://db.example.org/",
                            source_rating: null, version_tested: null, date: "2026-01-05" },
                  sentence: "Reported by Example DB · checked 5 January 2026 · Not tested by TUR" },
                { tag: "TUR's own test",
                  source: { kind: "tur-test", report: "docs/reports/compat/x.md", version_tested: "1.2",
                            date: "2027-03-01" },
                  sentence: "Tested by TUR on 1 March 2027 with version 1.2" },
                { tag: "TUR's own test, no version",
                  source: { kind: "tur-test", report: "docs/reports/compat/x.md", version_tested: null,
                            date: "2027-03-01" },
                  sentence: "Tested by TUR on 1 March 2027" }
            ]
        }

        function test_according_to_whom(data) {
            const sentence = Words.sourceSentence(data.source)
            compare(sentence, data.sentence)
            // An external rating never reads as TUR's own.
            if (data.source !== null && data.source.kind === "external")
                verify(sentence.indexOf("Not tested by TUR") > 0)
        }

        function test_every_kind_of_source_has_its_sentence() {
            for (const kind of router.rating_source_kinds) {
                const sentence = Words.sourceSentence({ kind: kind, name: "A source", date: "2026-11-12",
                                                        source_rating: null, version_tested: null,
                                                        report: "docs/reports/x.md" })
                verify(sentence.length > 0, kind)
            }
        }

        function test_a_record_of_the_catalogue_reads_like_the_router_s_source() {
            const external = Words.recordSource({ route: "proton", rating: "good", reported_by: "external",
                                                  source_name: "Valve Steam Deck compatibility",
                                                  source_url: "https://store.steampowered.com/app/1245620/",
                                                  source_rating: "Verified", version_tested: null,
                                                  date: "2026-11-12" })
            compare(Words.sourceSentence(external),
                    "Reported by Valve Steam Deck compatibility as \"Verified\" · checked 12 November 2026"
                    + " · Not tested by TUR")
            compare(Words.recordSource({ reported_by: "nobody", rating: "unknown", date: "2026-11-12" }).kind, "unrated")
            compare(Words.recordSource({ reported_by: "tur-test", test_report: "docs/reports/x.md",
                                         version_tested: "3", date: "2026-11-12" }).report, "docs/reports/x.md")
            // Every reporter has its words: those that can rate a route read
            // like the router's sources; TUR's compatibility autopilot never
            // rates, and says "Works with these settings" (issue #98).
            verify(router.reporters.indexOf("tur-autopilot") >= 0, "the vocabulary lists the autopilot")
            for (const reporter of router.reporters) {
                const source = Words.recordSource({ reported_by: reporter, date: "2026-11-12", remedies: [] })
                if (reporter === "tur-autopilot") {
                    compare(source.kind, "tur-autopilot")
                    continue
                }
                verify(router.rating_source_kinds.indexOf(source.kind) >= 0, reporter)
            }
            const autopilot = Words.recordSource({ reported_by: "tur-autopilot", rating: "unknown", date: "2026-11-13",
                                                   remedies: ["dll-builtin:d3dx9_43", "windows-version-win11"],
                                                   runtime_version: "wine-9.0" })
            compare(Words.sourceSentence(autopilot, "unknown"),
                    "Works with these settings · found by TUR's compatibility autopilot on 13 November 2026 · not a rating")
            compare(Words.settingsSentence({ remedies: ["windows-version-win11"], runtime_version: "wine-9.0",
                                             date: "2026-11-13", evidence_sha256: "0".repeat(64) }),
                    "Found by TUR's compatibility autopilot on 13 November 2026 with wine-9.0. Measured once, on one "
                    + "computer; not a rating.")
        }

        // TUR's own test with no rating does not read as a rating (#67).
        function test_tur_s_own_test_without_a_rating() {
            const source = { kind: "tur-test", report: "docs/reports/windows-runtime-2.md", version_tested: "8.9.9.1",
                             date: "2026-11-12" }
            compare(Words.sourceSentence(source, "unknown"),
                    "Tested by TUR on 12 November 2026 with version 8.9.9.1 · TUR has not rated how well it works")
            compare(Words.sourceSentence(source, "good"), "Tested by TUR on 12 November 2026 with version 8.9.9.1")
        }

        function test_a_pinned_free_installer_is_installed_not_fetched() {
            const entry = { editions: [{ kind: "windows", obtain: "publisher-page", installer: { url: "https://x/y.exe" } }] }
            compare(Words.primaryAction({ route: "wine", edition_index: 0, outlook: "clear", prerequisites: [] }, entry),
                    "install")
            compare(Words.installable("wine"), true)
            compare(Words.installable("flatpak"), true)
            compare(Words.installable("windows-guest"), false)
            compare(Words.installable("web"), false)
        }

        function test_a_day_is_written_in_full_without_its_weekday() {
            compare(Words.longDate("2026-11-12"), "12 November 2026")
            compare(Words.longDate("2026-01-05"), "5 January 2026")
            // What is not a day is shown as it is, not guessed at.
            compare(Words.longDate("yesterday"), "yesterday")
            compare(Words.longDate(""), "")
        }

        function test_the_primary_button_says_what_happens_next_data() {
            const entry = {
                editions: [
                    { kind: "flatpak", obtain: null },
                    { kind: "windows", obtain: "publisher-page" },
                    { kind: "windows", obtain: "steam" },
                    { kind: "windows", obtain: "gog" },
                    { kind: "windows", obtain: "epic" },
                    { kind: "linux-native", obtain: "fedora-package" }
                ]
            }
            const option = (route, index, outlook, prerequisites) => ({
                route: route, edition_index: index, outlook: outlook, prerequisites: prerequisites
            })
            const needed = [{ code: "set-up-windows-guest", text: "The Windows guest must be set up." }]
            return [
                { tag: "nothing is needed first", option: option("flatpak", 0, "clear", []), entry: entry,
                  action: "install", label: "Install" },
                { tag: "a package of the system", option: option("native", 5, "clear", []), entry: entry,
                  action: "install", label: "Install" },
                { tag: "something is needed first", option: option("windows-guest", 1, "clear", needed), entry: entry,
                  action: "set-up", label: "Set up…" },
                { tag: "the person fetches the installer", option: option("wine", 1, "clear", []), entry: entry,
                  action: "publisher-page", label: "Open the publisher's page" },
                { tag: "a store sells it", option: option("proton", 2, "clear", []), entry: entry,
                  action: "open-store", label: "Open in Steam" },
                { tag: "another store", option: option("proton", 3, "clear", []), entry: entry,
                  action: "open-store", label: "Open in GOG" },
                { tag: "a third store", option: option("proton", 4, "doubtful", []), entry: entry,
                  action: "open-store", label: "Open in Epic Games Store" },
                { tag: "it may not work", option: option("flatpak", 0, "doubtful", []), entry: entry,
                  action: "install-anyway", label: "Install anyway" },
                // Nothing at all when there is no route.
                { tag: "no route", option: { route: "no-route", edition_index: null, outlook: "blocked",
                                             prerequisites: [] }, entry: entry, action: "", label: "" },
                { tag: "nothing loaded", option: {}, entry: {}, action: "", label: "" }
            ]
        }

        function test_the_primary_button_says_what_happens_next(data) {
            compare(Words.primaryAction(data.option, data.entry), data.action)
            compare(Words.primaryActionLabel(data.option, data.entry), data.label)
        }

        function test_caveats_are_listed_blockers_first() {
            const option = { caveats: [
                { severity: "note", text: "a" }, { severity: "warning", text: "b" },
                { severity: "blocker", text: "c" }, { severity: "note", text: "d" },
                { severity: "obstacle", text: "e" }, { severity: "blocker", text: "f" }
            ] }
            // By severity, and within one severity in the router's order.
            compare(Words.orderedCaveats(option).map(caveat => caveat.text), ["c", "f", "e", "b", "a", "d"])
            compare(Words.orderedCaveats({ caveats: [] }), [])
            compare(Words.orderedCaveats({}), [])
        }
    }
}

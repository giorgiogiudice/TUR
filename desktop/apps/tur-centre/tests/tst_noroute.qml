// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// When there is no good route (desktop-experience.md, section 8.8): the plain
// statement, the reason in a sentence a non-specialist can repeat, what the
// person can do instead, the evidence, and the code. On the router's
// recorded answers for Valorant and for the class "Banking and payment
// applications for Android".
Item {
    id: root

    width: 1080
    height: 2400

    CentreHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "NoRoute"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        function settle() {
            tryVerify(() => host.settled, 15000, "every answer that was asked for has arrived")
            wait(0)
        }

        function open(id) {
            const shell = host.start("default", "tur-centre:app/" + id)
            settle()
            compare(host.catalogue.application.status, ApplicationSession.Ready)
            verify(host.catalogue.application.noRoute)
            return findChild(shell, "shellView").item
        }

        function everything(item) {
            const out = [item]
            for (let i = 0; i < item.children.length; i++)
                out.push(...everything(item.children[i]))
            return out
        }

        function textsOf(item, name) {
            return everything(item).filter(part => part.objectName === name && part.visible).map(part => part.text)
        }

        function textOf(item, name) {
            const found = textsOf(item, name)
            compare(found.length, 1, "one visible \"" + name + "\"")
            return found[0]
        }

        function shown(item, name) {
            const found = findChild(item, name)
            return found !== null && found.visible
        }

        function test_valorant_cannot_run_and_the_page_says_so_plainly() {
            const view = open("com.riotgames.valorant")
            compare(findChild(view, "applicationName").text, "Valorant")
            compare(findChild(view, "applicationPublisher").text, "Riot Games · Game")

            // What TUR cannot do.
            compare(textOf(view, "noRouteTitle"), "Cannot run on TUR")
            // Why, in the form the design fixes for kernel anti-cheat.
            compare(textOf(view, "noRouteWhy"),
                    "This game needs anti-cheat software that works only inside Windows itself. "
                    + "It will not start through Proton, and not in the Windows that TUR runs in the background.")
            // What the person can do instead.
            compare(textsOf(view, "noRouteAlternative"),
                    ["If Windows is installed on this computer beside TUR, start Windows to play it.",
                     "Look for games like it that run on TUR."])
            verify(!shown(view, "noRouteWebsite"))

            // Nothing suggests it might run after all: no button that says
            // what happens next, no "How it will run", no "Recommended".
            verify(!shown(view, "applicationPrimary"))
            verify(!shown(view, "applicationNotYet"))
            compare(findChild(view, "recommendedWay"), null)
            compare(findChild(view, "otherWays"), null)
            compare(textsOf(view, "wayRecommended"), [])
            compare(textsOf(view, "wayNothingKnown"), [])

            // The evidence: the route that was considered, blocked, with the
            // rating, who reported it, and the caveat that says why first.
            verify(shown(view, "noRouteConsidered"))
            const proton = findChild(view, "consideredWay0")
            compare(textOf(proton, "wayLabel"), "Windows game, through Proton")
            compare(textOf(proton, "wayOutlook"), "Cannot be used on this computer")
            compare(textOf(proton, "ratingWords"), "Does not work")
            compare(textOf(proton, "waySource"),
                    "Reported by AreWeAntiCheatYet as \"Denied\" · checked 12 November 2026 · Not tested by TUR")
            compare(textsOf(proton, "caveatText"),
                    ["This game's anti-cheat will not work: it runs only on real Windows, with a kernel driver "
                     + "that a compatibility layer cannot provide. Dual boot is the only way.",
                     "Reported broken by AreWeAntiCheatYet (\"Denied\"), checked 2026-11-12. Not tested by TUR."])
            const blocker = everything(proton).filter(part => part.objectName === "caveatText")[0]
            verify(blocker.Accessible.name.indexOf("Cannot be used on this computer: This game's anti-cheat") === 0)
            compare(findChild(view, "consideredWay1"), null)

            // The code, which can be copied.
            compare(findChild(view, "noRouteCode").text, "TUR-DESKTOP-CATALOGUE-011")
        }

        function test_the_router_s_own_words_and_the_references_open_in_place() {
            const view = open("com.riotgames.valorant")
            const detail = findChild(view, "noRouteDetail")
            compare(detail.text, "Detail")
            verify(!detail.expanded)
            verify(!shown(view, "noRouteRouterWords"))
            detail.expanded = true
            wait(0)
            // The router's reason and its alternative, as they are.
            const words = everything(findChild(view, "noRouteRouterWords")).filter(part => part.text !== undefined)[0]
            compare(words.text,
                    "Why: No route on TUR runs Valorant on this machine. Proton (through umu-launcher): This game's "
                    + "anti-cheat will not work: it runs only on real Windows, with a kernel driver that a "
                    + "compatibility layer cannot provide. Dual boot is the only way.\n"
                    + "Least-bad alternative: Dual boot: install Windows beside TUR and run it there. "
                    + "That is the only way.")

            const checked = findChild(view, "noRouteChecked")
            compare(checked.text, "Where each fact was checked")
            verify(!checked.expanded)
            compare(textsOf(view, "referenceTitle"), [])
            checked.expanded = true
            wait(0)
            compare(textsOf(view, "referenceTitle").length, 3)
            compare(textsOf(view, "referenceTitle")[2], "AreWeAntiCheatYet: Valorant, status Denied")
            compare(textsOf(view, "referenceChecked")[0], "Checked 12 November 2026")
        }

        function test_banking_applications_for_android_cannot_run() {
            const view = open("tur.class.android-banking")
            compare(findChild(view, "applicationName").text, "Banking and payment applications for Android")
            compare(findChild(view, "applicationPublisher").text, "Banks and payment providers · Finance")

            compare(textOf(view, "noRouteTitle"), "Cannot run on TUR")
            compare(textOf(view, "noRouteWhy"),
                    "This application checks that it is running on a phone certified by Google. "
                    + "No computer can pass that check.")
            // The class has no website of its own, so none is offered.
            compare(textsOf(view, "noRouteAlternative"), ["Use it on a certified phone or tablet."])
            verify(!shown(view, "noRouteWebsite"))
            verify(!shown(view, "applicationPrimary"))

            const android = findChild(view, "consideredWay0")
            compare(textOf(android, "wayLabel"), "Android application")
            compare(textOf(android, "wayOutlook"), "Cannot be used on this computer")
            // Nobody has reported on it, and that is what is said: a route
            // that is blocked is not thereby rated.
            compare(textOf(android, "ratingWords"), "No reports")
            // And not "It may still work": on this way it will not.
            compare(textsOf(android, "waySource"), [])
            compare(textsOf(android, "caveatText"),
                    ["Needs a phone certified by Google (Play Integrity): it will refuse to run here. "
                     + "No container or virtual machine passes that check."])
            compare(textsOf(android, "wayPrerequisite"), ["The Android runtime must be enabled: it is off by default."])
            compare(findChild(view, "noRouteCode").text, "TUR-DESKTOP-CATALOGUE-011")
        }

        // The cases the seed fixtures do not hold, on statements shaped as
        // the router shapes them.
        function statement(requirement, kind, text) {
            return {
                route: "no-route",
                reason: "No route on TUR runs Example on this machine. The router's reason.",
                caveats: requirement === "" ? [] : [
                    { code: "requirement", severity: "blocker", requirement: requirement, text: "The blocker." }
                ],
                alternative: { kind: kind, text: text }
            }
        }

        function test_each_case_has_its_sentences_data() {
            const game = { category: "game" }
            const application = { category: "productivity" }
            const withWebsite = { category: "finance", homepage: "https://bank.example.org/" }
            return [
                { tag: "kernel anti-cheat in a game",
                  option: statement("kernel-anti-cheat", "dual-boot", "Dual boot."), entry: game, kind: "kernel-anti-cheat",
                  why: ["This game needs anti-cheat software that works only inside Windows itself.",
                        "It will not start through Proton, and not in the Windows that TUR runs in the background."],
                  alternatives: ["If Windows is installed on this computer beside TUR, start Windows to play it.",
                                 "Look for games like it that run on TUR."] },
                // Not a game: "this game" would be wrong, so the router speaks.
                { tag: "kernel anti-cheat in something that is not a game",
                  option: statement("kernel-anti-cheat", "dual-boot", "Dual boot: the router's alternative."),
                  entry: application, kind: "kernel-anti-cheat",
                  why: ["No route on TUR runs Example on this machine. The router's reason."],
                  alternatives: ["Dual boot: the router's alternative."] },
                { tag: "Play Integrity, with a website",
                  option: statement("play-integrity", "certified-android-device", "A certified phone."),
                  entry: withWebsite, kind: "play-integrity",
                  why: ["This application checks that it is running on a phone certified by Google.",
                        "No computer can pass that check."],
                  alternatives: ["Use it on a certified phone or tablet.", "Use the service's website."] },
                { tag: "built only for phone processors",
                  option: statement("arm-only-native-code", "certified-android-device", "A certified phone."),
                  entry: application, kind: "arm-only-native-code",
                  why: ["This application was built only for phone processors.",
                        "TUR does not have, and may not ship, the software that would translate it."],
                  alternatives: ["Look for another edition."] },
                { tag: "a macOS edition only",
                  option: statement("", "mac", "Use a Mac."), entry: application, kind: "macos-only",
                  why: ["macOS applications run on Apple's computers.",
                        "An experimental macOS layer exists in TUR and is switched off."],
                  alternatives: ["Look for its Windows or Linux edition.", "Use a Mac."] },
                // Anything else: the reason and the alternative in the
                // router's words.
                { tag: "this machine cannot run it",
                  option: statement("", "another-computer", "Use a computer with an x86-64 processor."),
                  entry: application, kind: "other",
                  why: ["No route on TUR runs Example on this machine. The router's reason."],
                  alternatives: ["Use a computer with an x86-64 processor."] },
                { tag: "a requirement the design names no sentence for",
                  option: statement("kernel-driver", "none-known", "No alternative is known."),
                  entry: application, kind: "other",
                  why: ["No route on TUR runs Example on this machine. The router's reason."],
                  alternatives: ["No alternative is known."] }
            ]
        }

        function test_each_case_has_its_sentences(data) {
            compare(Words.noRouteCase(data.option), data.kind)
            compare(Words.noRouteWhy(data.option, data.entry), data.why)
            compare(Words.noRouteAlternatives(data.option, data.entry), data.alternatives)
        }
    }
}

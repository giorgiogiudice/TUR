// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// An application's page (desktop-experience.md, section 8.4), in each of its
// states, on the router's recorded answers for the seed catalogue. Every
// rating asserted here is the router's, and every sentence is the design's
// fixed vocabulary or the router's own text.
Item {
    id: root

    width: 1080
    height: 2400

    CentreHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "ApplicationPage"
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

        function page(shell) {
            return findChild(shell, "shellView").item
        }

        function open(set, id) {
            const shell = host.start(set, "tur-centre:app/" + id)
            settle()
            compare(host.catalogue.application.status, ApplicationSession.Ready)
            return page(shell)
        }

        // Every item under `item`, itself included, in the order of the tree.
        function everything(item) {
            const out = [item]
            for (let i = 0; i < item.children.length; i++)
                out.push(...everything(item.children[i]))
            return out
        }

        // The texts of the visible items named `name` under `item`.
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

        function test_microsoft_365_as_the_router_routes_it() {
            const view = open("default", "com.microsoft.office")

            compare(findChild(view, "applicationName").text, "Microsoft 365")
            compare(findChild(view, "applicationPublisher").text, "Microsoft · Productivity")
            verify(!shown(view, "applicationHostAssumed"), "the machine was stated, not assumed")

            // "How it will run": the option the router recommends, in the
            // fixed order. Route, explanation, rating with its source, what
            // is needed first, the caveats.
            const way = findChild(view, "recommendedWay")
            compare(textOf(way, "wayLabel"), "Windows application, in Windows")
            compare(textOf(way, "wayRecommended"), "Recommended")
            compare(textsOf(way, "wayOutlook"), [], "clear is nothing extra")
            compare(textOf(way, "wayExplanation"),
                    "Runs inside a real Windows that TUR starts in the background (a virtual machine). "
                    + "Needs your own Windows licence and more memory.")
            // The router's reason, as it is.
            compare(textOf(way, "wayReason"),
                    "Offered because Wine is rated broken for this application. It runs in real Windows, "
                    + "in a virtual machine, and its windows appear on the desktop.")
            // Unknown is shown as unknown, with who says so: nobody.
            compare(textOf(way, "ratingWords"), "No reports")
            compare(textOf(way, "waySource"), "Nobody has reported on this way of running it. It may still work.")
            compare(textsOf(way, "wayPrerequisite"),
                    ["The Windows guest must be set up.", "You must supply a Windows licence: TUR does not include one."])
            compare(textsOf(way, "caveatText"),
                    ["Needs about 16 GB of memory while the Windows guest runs; this machine has that much."])
            compare(textsOf(way, "wayNothingKnown"), [], "a caveat is known, so nothing claims otherwise")
            verify(!shown(way, "wayAllReports"), "no record, so no list of reports")

            // Something is needed first, so the button says "Set up…". Setting
            // up Windows is not built (D-25): it does nothing yet, and says so.
            const primary = findChild(view, "applicationPrimary")
            verify(primary.visible)
            compare(primary.text, "Set up…")
            verify(!primary.enabled, "setting up Windows is not built")
            compare(textOf(view, "applicationNotYet"),
                    "The application centre cannot set up what is needed first yet.")
            verify(!shown(view, "applicationInstallerFile"), "the way it runs is not the compatibility layer")

            // "Other ways to run it": every other option, in the router's
            // order. The compatibility layer comes before the web, and is
            // listed although it is rated as not working.
            const others = findChild(view, "otherWays")
            verify(others.visible)
            const layer = findChild(others, "otherWay0")
            compare(textOf(layer, "wayLabel"), "Windows application, in the compatibility layer")
            compare(textOf(layer, "wayOutlook"), "May not work")
            compare(textsOf(layer, "wayRecommended"), [])
            compare(textOf(layer, "ratingWords"), "Does not work")
            // The rating is never without its source, the source's own
            // word, the version and the date.
            compare(textOf(layer, "waySource"),
                    "Reported by WineHQ AppDB as \"Garbage\" · version 365 Business · checked 12 November 2026"
                    + " · Not tested by TUR")
            compare(textsOf(layer, "caveatText"),
                    ["Reported broken by WineHQ AppDB (\"Garbage\"), checked 2026-11-12. Not tested by TUR."])
            compare(textsOf(layer, "wayPrerequisite"), [])

            const web = findChild(others, "otherWay1")
            compare(textOf(web, "wayLabel"), "Web application")
            compare(textOf(web, "wayExplanation"), "Opens the service's website in its own window.")
            compare(textOf(web, "ratingWords"), "No reports")
            compare(textsOf(web, "wayOutlook"), [])
            compare(findChild(others, "otherWay2"), null)

            compare(findChild(view, "applicationSummary").text, "Office suite: Word, Excel, PowerPoint and others.")
        }

        function test_where_each_fact_was_checked() {
            const view = open("default", "com.microsoft.office")
            const details = findChild(view, "applicationDetails")
            compare(details.text, "Details")
            verify(!details.expanded, "closed until asked for")
            compare(textsOf(view, "referenceTitle"), [])

            details.expanded = true
            wait(0)
            // The entry's references, once each, with the date each was
            // checked.
            compare(textsOf(view, "referenceTitle"),
                    ["WineHQ AppDB: Microsoft Office (installer only)", "Microsoft 365 sign-in page"])
            compare(textsOf(view, "referenceChecked"), ["Checked 12 November 2026", "Checked 12 November 2026"])
            // This entry records no publisher's website, so none is shown.
            verify(!shown(view, "factsWebsite"))

            // Krita's entry does.
            const krita = open("default", "org.kde.krita")
            findChild(krita, "applicationDetails").expanded = true
            wait(0)
            verify(shown(krita, "factsWebsite"))
            compare(findChild(krita, "factsHomepage").url, "https://www.krita.org/")
        }

        function test_a_link_is_handed_to_the_browser() {
            const view = open("default", "org.kde.krita")
            findChild(view, "applicationDetails").expanded = true
            wait(50)
            const requested = []
            host.shell.urlRequested.connect(url => requested.push(url))
            mouseClick(findChild(view, "factsHomepage"))
            compare(requested, ["https://www.krita.org/"])
        }

        function test_krita_runs_directly_and_nothing_is_known_against_it() {
            const view = open("default", "org.kde.krita")
            compare(findChild(view, "applicationPublisher").text, "Krita Foundation · Creative")

            const way = findChild(view, "recommendedWay")
            compare(textOf(way, "wayLabel"), "Linux application")
            compare(textOf(way, "wayExplanation"), "Made for Linux. Runs directly.")
            // Nobody has reported on it. "Nothing known stands in the way"
            // is not a rating, and the rating stays "No reports".
            compare(textOf(way, "ratingWords"), "No reports")
            compare(textOf(way, "waySource"), "Nobody has reported on this way of running it. It may still work.")
            compare(textOf(way, "wayNothingKnown"), "Nothing known stands in the way.")
            compare(textsOf(way, "caveatText"), [])
            compare(textsOf(way, "wayPrerequisite"), [])

            // Nothing is needed first: "Install", and tur-apps installs a
            // Flatpak, so it can be pressed.
            compare(findChild(view, "applicationPrimary").text, "Install")
            verify(findChild(view, "applicationPrimary").enabled)
            verify(!shown(view, "applicationNotYet"))

            // The other Linux edition, with the router's note about it.
            const other = findChild(view, "otherWay0")
            compare(textOf(other, "wayLabel"), "Linux application")
            compare(textsOf(other, "caveatText"),
                    ["This package is maintained by its packagers, not by the application's publisher."])
            const note = everything(other).filter(part => part.objectName === "caveatText")[0]
            compare(note.Accessible.name,
                    "Note: This package is maintained by its packagers, not by the application's publisher.")
        }

        function test_elden_ring_shows_the_router_s_rating_and_all_reports() {
            const view = open("default", "com.bandainamco.eldenring")
            compare(findChild(view, "applicationPublisher").text, "FromSoftware, Bandai Namco Entertainment · Game")

            const way = findChild(view, "recommendedWay")
            compare(textOf(way, "wayLabel"), "Windows game, through Proton")
            compare(textOf(way, "wayExplanation"),
                    "Runs without Windows, through Proton, the compatibility layer used by the Steam Deck.")
            // The rating the router returns for the option, with its source.
            compare(textOf(way, "ratingWords"), "Works, with minor problems")
            compare(textOf(way, "waySource"),
                    "Reported by ProtonDB as \"gold\" · checked 12 November 2026 · Not tested by TUR")

            // Two records for this way: "All reports (2)" lists both, each
            // with its own rating and source, and says what a rating is.
            const all = findChild(way, "wayAllReports")
            verify(all.visible)
            compare(all.text, "All reports (2)")
            verify(!all.expanded)
            compare(textsOf(way, "reportSource"), [])
            all.expanded = true
            wait(0)
            compare(textsOf(way, "reportSource"),
                    ["Reported by ProtonDB as \"gold\" · checked 12 November 2026 · Not tested by TUR",
                     "Reported by Valve Steam Deck compatibility as \"Verified\" · checked 12 November 2026"
                     + " · Not tested by TUR"])
            compare(textsOf(way, "ratingWords"), ["Works, with minor problems", "Works, with minor problems", "Works well"])
            verify(everything(way).some(part => part.visible && part.text
                                        === "A source's rating is evidence from its contributors' machines, not a promise."))

            // It is sold by a store: the button names the store.
            compare(findChild(view, "applicationPrimary").text, "Open in Steam")
            // A game is never offered the background Windows; no other way.
            verify(!shown(view, "otherWays"))
        }

        function test_an_assumed_machine_is_said_to_be_assumed() {
            const view = open("assumed-host", "com.netflix.netflix")
            const banner = findChild(view, "applicationHostAssumed")
            verify(banner.visible, "the router was told nothing about this machine, and the page says so")
            compare(banner.text,
                    "TUR has not checked what this computer has. How this application will run here is an "
                    + "assumption, and may turn out differently.")
            compare(banner.severity, Kit.Banner.Warning)

            const way = findChild(view, "recommendedWay")
            compare(textOf(way, "wayLabel"), "Web application")
            compare(textOf(way, "wayReason"), "The web application is the publisher's primary form of this application.")
            // tur-apps does not install web applications: the button says
            // what would happen and cannot be pressed, and the line says why.
            compare(findChild(view, "applicationPrimary").text, "Install")
            verify(!findChild(view, "applicationPrimary").enabled)
            compare(textOf(view, "applicationNotYet"),
                    "The application centre cannot yet install applications that run this way.")

            // A route that may not work, for a stated reason: an obstacle,
            // not a blocker. It says "Probably will not work fully", and
            // what must be there first is the router's text.
            const android = findChild(view, "otherWay0")
            compare(textOf(android, "wayLabel"), "Android application")
            compare(textOf(android, "wayOutlook"), "May not work")
            compare(textOf(android, "caveatWord"), "Probably will not work fully")
            compare(textsOf(android, "caveatText"),
                    ["Its copy protection needs hardware attestation, which this route cannot provide: "
                     + "protected content is limited to low quality or refused."])
            const obstacle = everything(android).filter(part => part.objectName === "caveatText")[0]
            verify(obstacle.Accessible.name.indexOf("Probably will not work fully: Its copy protection") === 0)
            compare(textsOf(android, "wayPrerequisite"),
                    ["The Android runtime must be enabled: it is off by default.",
                     "The catalogue lists this package on Google Play only: Google Play must be opted into, "
                     + "under Google's terms."])
        }

        function test_an_application_the_catalogue_does_not_know() {
            const shell = host.start("default", "tur-centre:app/org.example.missing")
            settle()
            compare(host.catalogue.application.status, ApplicationSession.Failed)
            const view = page(shell)

            // The two-layer message, with the catalogue's own code and words.
            const message = findChild(view, "applicationFailure").item
            verify(message !== null)
            compare(message.code, "TUR-DESKTOP-CATALOGUE-009")
            compare(message.title, "No application has this ID")
            verify(message.reason.indexOf("org.example.missing") > 0)
            verify(message.technicalDetail.indexOf("Exit status: 6") > 0)
            // Asking again would give the same answer: no "Try again".
            compare(findChild(message, "messageAction0"), null)
            verify(!findChild(message, "messageSupportBundle").visible)

            // And nothing that belongs to a page: no name, no button, no
            // "How it will run".
            compare(findChild(view, "applicationName"), null)
            compare(findChild(view, "applicationPrimary"), null)
            compare(findChild(view, "recommendedWay"), null)
            verify(!shown(view, "applicationNotYet"))
        }

        function test_a_page_that_is_being_fetched_says_so() {
            // The stand-in program never answers about "sleep".
            const shell = host.startOnHelper("tur-centre:app/sleep", 60000)
            compare(host.catalogue.application.status, ApplicationSession.Loading)
            wait(100)
            const view = page(shell)
            const busy = findChild(view, "applicationBusy")
            verify(busy.visible)
            compare(busy.label, "Asking the catalogue about this application")
            compare(findChild(view, "applicationName"), null)
            // The window is not held up by it.
            findChild(shell, "shellBack").clicked()
            compare(shell.view, "home")
        }

        function test_a_page_follows_another_without_leftovers() {
            const shell = host.start("default", "tur-centre:app/com.microsoft.office")
            settle()
            compare(findChild(page(shell), "applicationName").text, "Microsoft 365")

            shell.showApplication("org.example.missing")
            settle()
            compare(findChild(page(shell), "applicationName"), null)
            compare(findChild(page(shell), "applicationFailure").item.code, "TUR-DESKTOP-CATALOGUE-009")

            shell.showApplication("org.kde.krita")
            settle()
            compare(findChild(page(shell), "applicationName").text, "Krita")
            compare(findChild(page(shell), "applicationFailure").item, null)
            compare(textOf(page(shell), "wayNothingKnown"), "Nothing known stands in the way.")
        }
    }
}

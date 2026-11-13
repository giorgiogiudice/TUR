// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// The words the application centre uses (desktop-experience.md, section 8.2).
// They are fixed: the same in search results, on an application's page and on
// the "no route" page, and each stands for exactly one value of the catalogue
// or the router, so that no meaning is added or lost on the way to the screen.
//
// Everything else the application centre says about an application is the
// router's own text: its reasons, caveats and prerequisites are shown as they
// are and none is written here.
//
// tests/tst_vocabulary.qml holds these tables against the router's own lists
// of values (fixtures/vocabulary.json): a value with no words here fails it.
QtObject {
    id: words

    // --- How an application runs: the router's `route` ---------------------

    readonly property var routeLabels: ({
        "flatpak": qsTr("Linux application"),
        "native": qsTr("Linux application"),
        //: An application packaged for another Linux system, when the catalogue does not name the system.
        "distro-container": qsTr("Linux application, from another Linux system"),
        "proton": qsTr("Windows game, through Proton"),
        "wine": qsTr("Windows application, in the compatibility layer"),
        "windows-guest": qsTr("Windows application, in Windows"),
        "android-runtime": qsTr("Android application"),
        "web": qsTr("Web application"),
        "darling": qsTr("macOS application (experimental)"),
        "no-route": qsTr("Cannot run on TUR")
    })

    readonly property var routeExplanations: ({
        "flatpak": qsTr("Made for Linux. Runs directly."),
        "native": qsTr("Made for Linux. Runs directly."),
        "distro-container": qsTr("Made for another Linux system. Runs in a small copy of that system."),
        "proton": qsTr("Runs without Windows, through Proton, the compatibility layer used by the Steam Deck."),
        "wine": qsTr("Runs without Windows. Fastest and lightest, but not every application works."),
        "windows-guest": qsTr("Runs inside a real Windows that TUR starts in the background (a virtual machine). Needs your own Windows licence and more memory."),
        "android-runtime": qsTr("Runs in the Android system. Android must be switched on."),
        "web": qsTr("Opens the service's website in its own window."),
        "darling": qsTr("An experiment. Most macOS applications do not run."),
        // The "no route" page says why in its own pattern (section 8.8).
        "no-route": ""
    })

    // --- How well it works: the catalogue's `rating` -----------------------

    readonly property var ratingLabels: ({
        "good": qsTr("Works well"),
        "usable": qsTr("Works, with minor problems"),
        "poor": qsTr("Works badly"),
        "broken": qsTr("Does not work"),
        "unknown": qsTr("No reports")
    })

    // --- What stands in the way: the option's `outlook` --------------------

    readonly property var outlookLabels: ({
        // Nothing extra. `clear` is not a promise; the rating says what is known.
        "clear": "",
        "doubtful": qsTr("May not work"),
        "blocked": qsTr("Cannot be used on this computer")
    })

    // --- A caveat's `severity`, as a word ----------------------------------
    //
    // On screen a caveat is a sign and the router's sentence. The word is
    // what a screen reader says in place of the sign; for an obstacle it is
    // also shown, as the design asks.
    readonly property var severityLabels: ({
        "blocker": qsTr("Cannot be used on this computer"),
        "obstacle": qsTr("Probably will not work fully"),
        "warning": qsTr("Warning"),
        "note": qsTr("Note")
    })

    // The order caveats are listed in: blockers first.
    readonly property var severityOrder: ["blocker", "obstacle", "warning", "note"]

    // --- The catalogue's ten categories ------------------------------------

    readonly property var categoryLabels: ({
        "game": qsTr("Game"),
        "productivity": qsTr("Productivity"),
        "creative": qsTr("Creative"),
        "communication": qsTr("Communication"),
        "development": qsTr("Development"),
        "utility": qsTr("Utility"),
        "media": qsTr("Media"),
        "internet": qsTr("Internet"),
        "finance": qsTr("Finance"),
        "education": qsTr("Education")
    })

    // Stores an edition is obtained from and sold by. Names, not translated.
    readonly property var storeNames: ({
        "steam": "Steam",
        "epic": "Epic Games Store",
        "gog": "GOG"
    })

    function lookUp(table: var, key: string): string {
        const value = table[key]
        return value === undefined ? "" : value
    }

    // The label of a way of running an application. `distribution` is the
    // catalogue's name for the other Linux system, when it gives one.
    function routeLabel(route: string, distribution: string): string {
        if (route === "distro-container" && distribution.length > 0)
            //: %1 is the catalogue's name for another Linux system, for example debian.
            return qsTr("Linux application, from %1").arg(distribution)
        return words.lookUp(words.routeLabels, route)
    }

    function routeExplanation(route: string): string {
        return words.lookUp(words.routeExplanations, route)
    }

    function ratingLabel(rating: string): string {
        return words.lookUp(words.ratingLabels, rating)
    }

    function outlookLabel(outlook: string): string {
        return words.lookUp(words.outlookLabels, outlook)
    }

    function severityLabel(severity: string): string {
        return words.lookUp(words.severityLabels, severity)
    }

    function categoryLabel(category: string): string {
        return words.lookUp(words.categoryLabels, category)
    }

    // --- According to whom --------------------------------------------------
    //
    // Every rating is shown with its source, the source's own word and the
    // date. `source` is the router's `rating_source`, or one record of the
    // catalogue turned into the same shape by recordSource().
    //
    // `rating` is the rating the source is given for. TUR's own test that
    // entered no rating ("unknown") is said as that, so that it does not read
    // as a rating (issue #67). A record of TUR's compatibility autopilot is
    // never a rating: it says with which settings the application started
    // (compat-autopilot.md, section 8; issue #98).
    function sourceSentence(source: var, rating: var): string {
        if (!source || source.kind === "unrated")
            return qsTr("Nobody has reported on this way of running it. It may still work.")
        const day = words.longDate(source.date)
        if (source.kind === "tur-autopilot") {
            //: %1 is a date. The autopilot is TUR's program that looks for settings with which a Windows application starts.
            return qsTr("Works with these settings · found by TUR's compatibility autopilot on %1 · not a rating").arg(day)
        }
        if (source.kind === "tur-test" && rating === "unknown") {
            if (source.version_tested)
                //: %1 is a date, %2 a version of the application.
                return qsTr("Tested by TUR on %1 with version %2 · TUR has not rated how well it works").arg(day).arg(source.version_tested)
            //: %1 is a date.
            return qsTr("Tested by TUR on %1 · TUR has not rated how well it works").arg(day)
        }
        if (source.kind === "tur-test") {
            if (source.version_tested)
                //: %1 is a date, %2 a version of the application.
                return qsTr("Tested by TUR on %1 with version %2").arg(day).arg(source.version_tested)
            //: %1 is a date.
            return qsTr("Tested by TUR on %1").arg(day)
        }
        if (source.source_rating && source.version_tested)
            //: %1 is the name of a source such as WineHQ AppDB, %2 the source's own rating in its own word, %3 a version of the application, %4 a date.
            return qsTr("Reported by %1 as \"%2\" · version %3 · checked %4 · Not tested by TUR")
                .arg(source.name).arg(source.source_rating).arg(source.version_tested).arg(day)
        if (source.source_rating)
            //: %1 is the name of a source such as ProtonDB, %2 the source's own rating in its own word, %3 a date.
            return qsTr("Reported by %1 as \"%2\" · checked %3 · Not tested by TUR")
                .arg(source.name).arg(source.source_rating).arg(day)
        if (source.version_tested)
            //: %1 is the name of a source, %2 a version of the application, %3 a date.
            return qsTr("Reported by %1 · version %2 · checked %3 · Not tested by TUR")
                .arg(source.name).arg(source.version_tested).arg(day)
        //: %1 is the name of a source, %2 a date.
        return qsTr("Reported by %1 · checked %2 · Not tested by TUR").arg(source.name).arg(day)
    }

    // One record of the catalogue (`show`), in the shape of the router's
    // `rating_source`.
    function recordSource(record: var): var {
        if (record.reported_by === "external")
            return {
                kind: "external",
                name: record.source_name,
                url: record.source_url,
                source_rating: record.source_rating,
                version_tested: record.version_tested,
                date: record.date
            }
        if (record.reported_by === "tur-test")
            return {
                kind: "tur-test",
                report: record.test_report,
                version_tested: record.version_tested,
                date: record.date
            }
        if (record.reported_by === "tur-autopilot")
            return {
                kind: "tur-autopilot",
                remedies: record.remedies ?? [],
                runtime_version: record.runtime_version,
                version_tested: record.version_tested,
                date: record.date
            }
        return { kind: "unrated" }
    }

    // --- Works with these settings (TUR's compatibility autopilot) ---------
    //
    // The router's `settings` of an option: the remedies of the most recent
    // autopilot record, which TUR found the application to start with. Not
    // a rating (compat-autopilot.md, section 8).
    function settingsSentence(settings: var): string {
        if (!settings)
            return ""
        //: %1 is a date, %2 the version of the compatibility layer it was measured with, for example wine-9.0.
        return qsTr("Found by TUR's compatibility autopilot on %1 with %2. Measured once, on one computer; not a rating.")
            .arg(words.longDate(settings.date)).arg(settings.runtime_version)
    }

    // --- Installing (section 8.5) ------------------------------------------

    // The routes tur-apps installs by. Every other is refused by it before
    // anything is done (app-install-backend.md), so the button does not
    // offer to.
    readonly property var installableRoutes: ["flatpak", "wine"]

    function installable(route: string): bool {
        return words.installableRoutes.indexOf(route) >= 0
    }

    // A number of bytes, as the reader's locale writes a size: 5.6 MB.
    function size(bytes: var): string {
        if (bytes === null || bytes === undefined || bytes < 0)
            return ""
        return Qt.locale().formattedDataSize(bytes, 1, Locale.DataSizeSIFormat)
    }

    // What is happening, by tur-apps' step: the label of the progress.
    function stepLabel(kind: int, step: string, route: string): string {
        switch (step) {
        case "download":
            return qsTr("Downloading")
        case "verify":
            return qsTr("Checking the download")
        case "install":
            return route === "wine" ? qsTr("Setting it up in the Windows compatibility layer")
                                    : qsTr("Downloading and installing")
        case "record":
            return qsTr("Finishing")
        case "update":
            return qsTr("Updating")
        case "keep":
            return qsTr("Keeping its settings and documents")
        case "uninstall":
            return qsTr("Removing")
        case "unused":
            return qsTr("Removing what nothing else uses")
        }
        if (kind === AppsOperation.Update)
            return qsTr("Updating")
        if (kind === AppsOperation.Remove)
            return qsTr("Removing")
        return qsTr("Starting")
    }

    // What an application may reach, by the kind tur-apps gives each
    // permission (app-install-backend.md, "The plan"): the same words for
    // every route (section 8.4).
    readonly property var accessKinds: ({
        "files": qsTr("Files"),
        "devices": qsTr("Devices"),
        "network": qsTr("Network"),
        "display": qsTr("Display"),
        "sound": qsTr("Sound"),
        "services": qsTr("Services of the session"),
        "other": qsTr("Other")
    })

    function accessKind(kind: string): string {
        return words.lookUp(words.accessKinds, kind) || kind
    }

    // An edition's licence and who packages it, from the catalogue.
    function licenceSentence(edition: var): string {
        if (!edition)
            return ""
        if (edition.licence === "open-source")
            return qsTr("Open source.")
        if (edition.licence === "proprietary")
            return qsTr("Proprietary: the publisher's terms apply.")
        return qsTr("The catalogue does not say under which licence it is published.")
    }

    function packagingSentence(edition: var): string {
        if (!edition)
            return ""
        if (edition.packaging === "publisher")
            return qsTr("Packaged by its publisher.")
        if (edition.packaging === "community")
            return qsTr("Packaged by volunteers, not by its publisher.")
        return ""
    }

    // Whether an update is known: tur-apps' `update` of a listed
    // application.
    function updateState(update: var): string {
        if (!update || update.checked !== true)
            return qsTr("Not checked")
        if (update.available === true && update.version)
            //: %1 is the version an update brings.
            return qsTr("An update is available: version %1").arg(update.version)
        if (update.available === true)
            return qsTr("An update is available")
        return qsTr("Up to date")
    }

    // The state of a source, for this person (`tur-apps sources`).
    readonly property var sourceStates: ({
        "on": qsTr("On"),
        "off": qsTr("Off"),
        "not-added": qsTr("Not added")
    })

    function sourceState(state: string): string {
        return words.lookUp(words.sourceStates, state) || state
    }

    // --- Chrome, for people coming from Windows (section 17.1, answer 4) ---

    readonly property string chromeId: "com.google.chrome"

    readonly property string chromeOffer: qsTr("Coming from Windows? Google Chrome can be installed with one action. Firefox is TUR's browser; Chrome is offered, not included.")

    // Said wherever Chrome is offered or planned: where it comes from.
    readonly property string chromeOrigin: qsTr("Chrome is made by Google, and Google's terms apply to it. TUR does not include it. Flatpak downloads it from Google's own server while it installs; the package around it is kept by Flathub's volunteers, not by Google. It is updated the same way.")

    // A day the catalogue writes as 2026-11-12, as the reader's locale writes
    // a date in full, without the day of the week: 12 November 2026.
    // Anything else is returned as it is.
    function longDate(iso: string): string {
        const parts = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso)
        if (parts === null)
            return iso
        const day = new Date(Number(parts[1]), Number(parts[2]) - 1, Number(parts[3]))
        const locale = Qt.locale()
        const format = locale.dateFormat(Locale.LongFormat)
            .replace(/dddd[,.]?\s*/, "").replace(/[,.]?\s*dddd/, "").trim()
        return day.toLocaleDateString(locale, format.length > 0 ? format : Locale.LongFormat)
    }

    // --- The primary button of an application's page (section 8.4) ---------
    //
    // It says what will happen next. Which it is follows from the option the
    // router recommends and from where the catalogue says its edition is
    // obtained; nothing is decided here about how the application runs.
    function editionOf(option: var, entry: var): var {
        if (!option || !entry || !entry.editions)
            return null
        const index = option.edition_index
        if (index === null || index === undefined || index < 0 || index >= entry.editions.length)
            return null
        return entry.editions[index]
    }

    function primaryAction(option: var, entry: var): string {
        if (!option || !option.route || option.route === "no-route")
            return ""
        if (option.prerequisites && option.prerequisites.length > 0)
            return "set-up"
        const edition = words.editionOf(option, entry)
        const obtain = edition && edition.obtain ? edition.obtain : ""
        if (words.storeNames[obtain] !== undefined)
            return "open-store"
        // A free installer the catalogue pins (its address, size and
        // SHA-256) is downloaded by TUR from the publisher; otherwise the
        // person fetches it.
        if (obtain === "publisher-page" && !(option.route === "wine" && edition.installer))
            return "publisher-page"
        if (option.outlook === "doubtful")
            return "install-anyway"
        return "install"
    }

    function primaryActionLabel(option: var, entry: var): string {
        switch (words.primaryAction(option, entry)) {
        case "set-up":
            return qsTr("Set up…")
        case "open-store": {
            const edition = words.editionOf(option, entry)
            //: %1 is the name of a store, for example Steam.
            return qsTr("Open in %1").arg(words.storeNames[edition.obtain])
        }
        case "publisher-page":
            return qsTr("Open the publisher's page")
        case "install-anyway":
            return qsTr("Install anyway")
        case "install":
            return qsTr("Install")
        default:
            return ""
        }
    }

    // --- When there is no route (section 8.8) ------------------------------

    // The catalogue's code for "no route runs the application on this
    // machine". The router's JSON states the fact (its first option is
    // `no-route`, and the program ends with status 3) without printing the
    // code, so it is written here; tst_vocabulary holds it against the
    // catalogue's list of codes.
    readonly property string noRouteCode: "TUR-DESKTOP-CATALOGUE-011"

    //
    // The cases the router distinguishes, read from the blocker it repeats in
    // its "no route" statement and from the alternative it names.
    function noRouteCase(option: var): string {
        if (!option || !option.caveats)
            return "other"
        const blocking = []
        for (const caveat of option.caveats) {
            if (caveat.severity === "blocker" && caveat.requirement)
                blocking.push(caveat.requirement)
        }
        if (blocking.indexOf("kernel-anti-cheat") >= 0)
            return "kernel-anti-cheat"
        if (blocking.indexOf("play-integrity") >= 0)
            return "play-integrity"
        if (blocking.indexOf("arm-only-native-code") >= 0)
            return "arm-only-native-code"
        if (blocking.length === 0 && option.alternative && option.alternative.kind === "mac")
            return "macos-only"
        return "other"
    }

    // Why, in sentences a non-specialist can repeat. The design fixes the
    // form for the cases it names; for any other the router's own reason is
    // shown.
    function noRouteWhy(option: var, entry: var): var {
        const game = entry && entry.category === "game"
        switch (words.noRouteCase(option)) {
        case "kernel-anti-cheat":
            if (game)
                return [
                    qsTr("This game needs anti-cheat software that works only inside Windows itself."),
                    qsTr("It will not start through Proton, and not in the Windows that TUR runs in the background.")
                ]
            break
        case "play-integrity":
            return [
                qsTr("This application checks that it is running on a phone certified by Google."),
                qsTr("No computer can pass that check.")
            ]
        case "arm-only-native-code":
            return [
                qsTr("This application was built only for phone processors."),
                qsTr("TUR does not have, and may not ship, the software that would translate it.")
            ]
        case "macos-only":
            return [
                qsTr("macOS applications run on Apple's computers."),
                qsTr("An experimental macOS layer exists in TUR and is switched off.")
            ]
        }
        const reason = RouterWords.reason(option)
        return reason.length > 0 ? [reason] : []
    }

    // What the person can do instead: the router's alternative, in the form
    // the design fixes for the cases it names, and otherwise in the router's
    // own words.
    function noRouteAlternatives(option: var, entry: var): var {
        const game = entry && entry.category === "game"
        switch (words.noRouteCase(option)) {
        case "kernel-anti-cheat":
            if (game)
                return [
                    qsTr("If Windows is installed on this computer beside TUR, start Windows to play it."),
                    qsTr("Look for games like it that run on TUR.")
                ]
            break
        case "play-integrity":
            return entry && entry.homepage
                ? [qsTr("Use it on a certified phone or tablet."), qsTr("Use the service's website.")]
                : [qsTr("Use it on a certified phone or tablet.")]
        case "arm-only-native-code":
            return [qsTr("Look for another edition.")]
        case "macos-only":
            return [qsTr("Look for its Windows or Linux edition."), qsTr("Use a Mac.")]
        }
        return option && option.alternative && option.alternative.text ? [option.alternative.text] : []
    }

    // --- Caveats -----------------------------------------------------------

    // The option's caveats, blockers first, otherwise in the router's order.
    function orderedCaveats(option: var): var {
        const out = []
        if (!option || !option.caveats)
            return out
        for (const severity of words.severityOrder) {
            for (const caveat of option.caveats) {
                if (caveat.severity === severity)
                    out.push(caveat)
            }
        }
        return out
    }

    // What a screen reader says for a caveat: the word its sign stands for,
    // then the router's sentence.
    function spokenCaveat(severity: string, text: string): string {
        switch (severity) {
        case "blocker":
            //: Read by a screen reader. %1 is the router's sentence about what stands in the way.
            return qsTr("Cannot be used on this computer: %1").arg(text)
        case "obstacle":
            //: Read by a screen reader. %1 is the router's sentence about what stands in the way.
            return qsTr("Probably will not work fully: %1").arg(text)
        case "warning":
            //: Read by a screen reader. %1 is the router's sentence about what to expect.
            return qsTr("Warning: %1").arg(text)
        default:
            //: Read by a screen reader. %1 is the router's sentence about something worth knowing.
            return qsTr("Note: %1").arg(text)
        }
    }
}

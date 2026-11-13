// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// The router's own sentences, written from their stable identifiers and
// parameters so that they can be translated (issue #41;
// desktop-experience.md, 8.10, requirement 1, and 13.2).
//
// The router gives every reason as `reason_parts` (a code and its
// parameters for each sentence), every prerequisite with a `code` and every
// caveat with a `text_id` and `params`, beside its English text. Here each
// identifier has its sentence, through qsTr. In English each sentence is the
// router's own, word for word: tests/tst_routerwords.qml holds every option
// of the recorded fixtures to that, so this table cannot drift from the
// router without a test failing.
//
// The rule of 8.2 still holds: the application centre writes no reason,
// caveat or prerequisite of its own. A sentence whose identifier, or one of
// whose parameters, this version does not know is shown in the router's own
// words, as before.
QtObject {
    id: router

    // The router's names of its routes, inside its sentences ("Listed after
    // Wine because ..."). Not the application centre's labels (Words): the
    // router names the engine.
    readonly property var routeNames: ({
        "flatpak": qsTr("Flatpak"),
        "native": qsTr("Native Linux"),
        "distro-container": qsTr("Container of another distribution"),
        "proton": qsTr("Proton (through umu-launcher)"),
        "wine": qsTr("Wine"),
        "windows-guest": qsTr("Windows guest (virtual machine)"),
        "android-runtime": qsTr("Android runtime"),
        "darling": qsTr("Darling (experimental)"),
        "web": qsTr("Web application"),
        "no-route": qsTr("No route")
    })

    // A rating inside a router's sentence, as the router writes it.
    readonly property var ratingNames: ({
        "good": qsTr("good"),
        "usable": qsTr("usable"),
        "poor": qsTr("poor"),
        "broken": qsTr("broken"),
        "unknown": qsTr("unknown")
    })

    readonly property string guestTail: qsTr("It runs in real Windows, in a virtual machine, and its windows appear on the desktop.")

    // One sentence of a reason, or undefined when this version cannot write
    // it. `option` is the router's option, for the caveat a part names.
    function reasonPart(part: var, option: var): var {
        const p = part.params ?? {}
        switch (part.code) {
        case "linux-flatpak":
            return qsTr("A Linux edition exists as a Flatpak: a native Linux edition comes first.")
        case "linux-native":
            return qsTr("A native Linux edition exists: a native Linux edition comes first.")
        case "linux-other-distribution":
            return qsTr("A Linux edition exists as a package of another distribution: it runs in a container of that distribution.")
        case "windows-game-proton":
            return qsTr("A Windows edition exists and this is a game: Proton, through umu-launcher, is the first route for games.")
        case "windows-wine":
            return qsTr("A Windows edition exists: Wine, a compatibility layer, comes before a virtual machine.")
        case "guest-layer-rated":
            if (!(p.rating in router.ratingNames))
                return undefined
            //: %1 is a rating as the router writes it: good, usable, poor, broken or unknown.
            return qsTr("Offered because Wine is rated %1 for this application.").arg(router.ratingNames[p.rating])
                + " " + router.guestTail
        case "guest-known-obstacle":
            return qsTr("Offered because a known requirement stands in Wine's way and no rating says that Wine runs it all the same.")
                + " " + router.guestTail
        case "guest-asked":
            return qsTr("Offered because you asked for it.") + " " + router.guestTail
        case "android":
            return qsTr("An Android edition exists: it runs in the Android runtime.")
        case "web-primary":
            return qsTr("The web application is the publisher's primary form of this application.")
        case "web-pinned":
            return qsTr("A web edition exists, and you pinned it.")
        case "web-nothing-better":
            return qsTr("A web edition exists, and no better route is free of known obstacles.")
        case "macos-darling":
            return qsTr("A macOS edition exists and you have enabled Darling. It is listed last: an edition for Windows, Linux or the web is preferred.")
        case "listed-after-better-route":
            if (!(p.route in router.routeNames) || !(p.rating in router.ratingNames)
                    || !(p.better_rating in router.ratingNames))
                return undefined
            //: %1 is the name of a route, for example Wine; %2 and %3 are ratings as the router writes them.
            return qsTr("Listed after %1 because this edition is rated %2 and that route is rated %3.")
                .arg(router.routeNames[p.route]).arg(router.ratingNames[p.rating]).arg(router.ratingNames[p.better_rating])
        case "no-route":
            if (typeof p.name !== "string")
                return undefined
            //: %1 is the name of an application.
            return qsTr("No route on TUR runs %1 on this machine.").arg(p.name)
        case "route-blocked": {
            if (!(p.route in router.routeNames))
                return undefined
            const caveats = option && option.caveats ? option.caveats : []
            const blocker = caveats.find(caveat => caveat.text_id === p.caveat)
            if (blocker === undefined)
                return undefined
            //: %1 is the name of a route, %2 the router's sentence about what blocks it.
            return qsTr("%1: %2").arg(router.routeNames[p.route]).arg(router.caveat(blocker))
        }
        case "macos-no-darling":
            return qsTr("The macOS edition has no route: Darling, TUR's experimental macOS layer, is not enabled, and a macOS virtual machine is not an option, because Apple's licence allows macOS guests only on Apple hardware.")
        case "no-edition":
            return qsTr("The catalogue lists no edition of it.")
        }
        return undefined
    }

    // The option's reason, from its parts; the router's own text when a part
    // cannot be written here.
    function reason(option: var): string {
        if (!option)
            return ""
        const parts = option.reason_parts
        if (!parts || parts.length === 0)
            return option.reason ?? ""
        const sentences = []
        for (const part of parts) {
            const sentence = router.reasonPart(part, option)
            if (sentence === undefined)
                return option.reason ?? ""
            sentences.push(sentence)
        }
        return sentences.join(" ")
    }

    // What must be done first, by its code.
    readonly property var prerequisites: ({
        "install-flatpak": qsTr("Flatpak must be installed."),
        "install-container-tool": qsTr("A tool that runs containers of other distributions must be installed."),
        "install-wine": qsTr("Wine must be installed."),
        "install-proton": qsTr("umu-launcher must be installed, and it must fetch a Proton build: TUR does not ship Proton."),
        "enable-android-runtime": qsTr("The Android runtime must be enabled: it is off by default."),
        "opt-into-google-play": qsTr("The catalogue lists this package on Google Play only: Google Play must be opted into, under Google's terms."),
        "set-up-windows-guest": qsTr("The Windows guest must be set up."),
        "provide-windows-licence": qsTr("You must supply a Windows licence: TUR does not include one.")
    })

    function prerequisite(item: var): string {
        if (!item)
            return ""
        const sentence = router.prerequisites[item.code]
        return sentence === undefined ? (item.text ?? "") : sentence
    }

    // The caveats whose sentence has no parameter, by identifier. The rest
    // are shown in the router's words until they are added here.
    readonly property var caveats: ({
        "community-package": qsTr("This package is maintained by its packagers, not by the application's publisher."),
        "experimental.darling": qsTr("Experimental: Darling comes with no promise that this application works, and it does not run mainstream macOS graphical applications."),
        "guest-unsuitable-for-games": qsTr("A Windows guest is not offered for games: it has no usable GPU acceleration."),
        "guest-not-offered.no-virtual-machines": qsTr("A Windows guest would be offered for this application as well, and is not, because you have chosen never to use a virtual machine. What is lost: the route that runs it on real Windows, which is the one expected to work where Wine does not.")
        // Not the requirement caveats: one identifier is written two ways
        // ("Its anti-cheat ..." and, for a game, "This game's anti-cheat
        // ..."), with nothing in `params` to say which, so they are shown in
        // the router's words until the router gives the difference.
    })

    function caveat(item: var): string {
        if (!item)
            return ""
        const empty = !item.params || Object.keys(item.params).length === 0
        const sentence = router.caveats[item.text_id]
        return empty && sentence !== undefined ? sentence : (item.text ?? "")
    }
}

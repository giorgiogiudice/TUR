// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// What P@x's character says of itself, in one place (docs/architecture/
// tur-pax-companion.md; desktop-experience.md, 5.6). P@x's name and TUR's are
// names and are not translated. The sentence of a notice is never here: it
// is the words of the service the notice came from.
QtObject {
    id: words

    readonly property string pax: "P@x"
    readonly property string tur: "TUR"

    // The state in words: what a screen reader reads for the character, and
    // what "What is P@x watching?" begins with. Every state has its own
    // words, so that the picture is never the only difference.
    function state(name: string): string {
        switch (name) {
        case "asleep":
            return qsTr("P@x is off")
        case "idle":
            return qsTr("P@x is on")
        case "noticing":
            return qsTr("P@x has something for you")
        case "listening":
            return qsTr("P@x is listening")
        case "thinking":
            return qsTr("P@x is working")
        case "observing":
            return qsTr("P@x can see the screen")
        case "acting":
            return qsTr("P@x is controlling")
        case "waiting":
            return qsTr("P@x is waiting for you")
        case "paused":
            return qsTr("P@x is paused")
        case "telling":
            return qsTr("P@x is telling you something")
        case "refused":
            return qsTr("P@x was refused")
        case "error":
            return qsTr("P@x cannot work")
        }
        return ""
    }

    // What pressing the character does, for a screen reader.
    function press(state: string, waitingNotices: int): string {
        if (state === "asleep")
            return qsTr("Press to set up P@x")
        if (waitingNotices > 0)
            return qsTr("Press to hear it")
        return qsTr("Press to ask P@x")
    }

    readonly property string characterMenu: qsTr("Press the menu key for quiet, hide and settings")

    // The bubble.
    readonly property string showMe: qsTr("Show me")
    readonly property string notNow: qsTr("Not now")
    readonly property string askPax: qsTr("Ask P@x")
    function action(id: string): string {
        switch (id) {
        case "show":
            return words.showMe
        case "ask":
            return words.askPax
        }
        return words.notNow
    }
    //: Read by a screen reader before a notice: %1 is who says it, TUR or P@x.
    readonly property string says: qsTr("%1 says")
    readonly property string suggestionTag: qsTr("A suggestion")

    // The whole notice.
    readonly property string close: qsTr("Close")
    readonly property string back: qsTr("Back")
    readonly property string commandsTitle: qsTr("What you can do about it, in a terminal")
    readonly property string commandsNote: qsTr("P@x's character runs none of these. They are here for you to copy.")

    // The menu.
    readonly property string menuTitle: qsTr("P@x on the bar")
    readonly property string quietTitle: qsTr("Quiet")
    readonly property string quietHour: qsTr("For an hour")
    readonly property string quietTomorrow: qsTr("Until tomorrow")
    readonly property string quietAlways: qsTr("Until I say")
    readonly property string quietOff: qsTr("Speak again")
    readonly property string quietNow: qsTr("P@x's character is quiet. What it would have said waits as a mark; press the character to hear it. TUR's own notifications still come.")
    readonly property string quietNote: qsTr("Quiet stops the character from speaking first. P@x's own indicator and TUR's notifications are not changed.")
    readonly property string watching: qsTr("What is P@x watching?")
    readonly property string settings: qsTr("What it may tell me")
    readonly property string hide: qsTr("Hide P@x from the bar")
    readonly property string hideNote: qsTr("To bring it back, open “P@x on the bar” from the launcher.")
    readonly property string stop: qsTr("Stop P@x")
    readonly property string setUp: qsTr("Set up P@x")

    // Settings.
    readonly property string settingsTitle: qsTr("What P@x's character may tell you")
    readonly property string settingsNote: qsTr("It speaks first only about these, once each, and never more than a few times an hour. Everything else you ask for.")
    function kind(name: string): string {
        switch (name) {
        case "repair":
            return qsTr("A repair TUR made")
        case "fault":
            return qsTr("A problem waiting for you")
        case "approval":
            return qsTr("P@x is waiting for your answer")
        case "task":
            return qsTr("A task ended, or was refused")
        case "update":
            return qsTr("An update undid itself")
        }
        return name
    }
    readonly property string suggestionsSwitch: qsTr("Suggestions")
    //: %1 is a number.
    readonly property string suggestionsNote: qsTr("Off unless you switch it on. At most %1 an hour, made on this computer by plain rules from what is listed under “What is P@x watching?”. Nothing on your screen is read, and nothing is sent anywhere.")
    readonly property string stayOver: qsTr("Stay over full-screen windows")
    readonly property string stayOverNote: qsTr("Off: the character leaves with the bar, and comes back over a full-screen window only while P@x is controlling, can see the screen, or waits for you.")
    readonly property string stayAsleep: qsTr("Stay on the bar while P@x is off")

    // What is P@x watching?
    readonly property string watchingTitle: qsTr("What P@x is watching")
    readonly property string readsTitle: qsTr("The character reads")
    readonly property string readsPax: qsTr("P@x's own state: on or off, what it is doing, today's spending against your limit.")
    readonly property string readsHeal: qsTr("TUR's record of repairs and problems on this computer.")
    readonly property string readsIndicator: qsTr("What P@x's indicator at the top of the screen shows.")
    readonly property string readsBattery: qsTr("Whether the computer runs on its battery.")
    readonly property string neverTitle: qsTr("The character never reads")
    readonly property string neverList: qsTr("Your screen, your windows or their titles, what you type, the clipboard, your files, the microphone or the camera.")
    readonly property string paxNowTitle: qsTr("P@x, right now")
    readonly property string paxOff: qsTr("P@x is off. It reads nothing and does nothing.")
    readonly property string paxIdle: qsTr("P@x is on and doing nothing. It reads nothing until you ask it something.")
    readonly property string paxWorking: qsTr("P@x is working on what you asked. Each step is in its window.")
    //: %1 is what P@x is doing, in the service's words, such as "Read the test value".
    readonly property string paxStep: qsTr("Now: %1.")
    readonly property string paxObserving: qsTr("P@x can see the screen or a window. Its indicator at the top of the screen says which.")
    readonly property string paxActing: qsTr("P@x is controlling an application. Its indicator at the top of the screen shows where.")
    readonly property string paxPaused: qsTr("P@x is paused.")
    readonly property string paxProblem: qsTr("P@x is on but cannot take a task.")
    function allowed(reason: string): string {
        switch (reason) {
        case "observe_permitted":
            return qsTr("Allowed: it only looks.")
        case "reversible_permitted":
            return qsTr("Allowed: it can be undone.")
        }
        return qsTr("Allowed.")
    }
    readonly property string openActivity: qsTr("See everything P@x did")
    //: %1 is a whole number of percent.
    readonly property string fuel: qsTr("%1 % of today's spending limit is left.")
    readonly property string saving: qsTr("On battery: the character keeps still to save energy.")

    // In the whole notice, while P@x is on: the person asks P@x about it.
    readonly property string explain: qsTr("Ask P@x to explain")
    //: What is sent to P@x when the person presses "Ask P@x to explain": %1 is a stable code, %2 the problem's title.
    readonly property string explainRequest: qsTr("Explain the problem %1 (%2) on this computer and what I can do about it.")
}

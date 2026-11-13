// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// Every sentence tur-pax-ui says that is not the service's own, in one place
// (desktop-experience.md, 5.6 and 9). P@x speaks in the first person and only
// through the service; everything here is the system speaking about P@x, in
// the third person. Codes, identifiers and commands are not translated.
QtObject {
    id: words

    // --- the surface ---------------------------------------------------------
    readonly property string askField: qsTr("Ask or tell P@x")
    readonly property string askPlaceholder: qsTr("Ask or tell P@x…")
    readonly property string send: qsTr("Send")
    readonly property string speak: qsTr("Speak")
    readonly property string voiceLater: qsTr("Speaking to P@x is coming later. For now, type.")
    readonly property string keysTitle: qsTr("P@x's keys")
    readonly property string stop: qsTr("Stop")
    readonly property string pause: qsTr("Pause")
    readonly property string resume: qsTr("Resume")
    readonly property string close: qsTr("Close")
    readonly property string activity: qsTr("Activity")
    readonly property string conversation: qsTr("Conversation")
    readonly property string setup: qsTr("Set up P@x")
    readonly property string switchOff: qsTr("Switch P@x off")
    readonly property string notNow: qsTr("Not now")
    readonly property string back: qsTr("Back")
    readonly property string continueLabel: qsTr("Continue")
    readonly property string details: qsTr("Details")
    readonly property string you: qsTr("You")
    readonly property string busyNote: qsTr("P@x does one task at a time. What you typed is kept; send it when this task has ended, or type “stop”.")
    readonly property string emptyConversation: qsTr("Ask P@x to do something on this computer, or to show you how. Every step it takes is listed here as it happens.")

    // --- P@x's state (design 9.1) ----------------------------------------------
    readonly property string looking: qsTr("Looking for P@x…")
    readonly property string absentTitle: qsTr("P@x is not on this computer")
    readonly property string absentWhy: qsTr("P@x's service, tur-pax, is not installed or cannot be started.")
    readonly property string absentText: qsTr("Nothing you type here goes anywhere.")
    readonly property string offTitle: qsTr("P@x is off")
    readonly property string offText: qsTr("P@x is TUR's assistant. It can do things on this computer for you and show you how to do them yourself. It needs an AI that you choose: one on this computer, or a provider's, with a key from them.")
    readonly property string switchedOffTitle: qsTr("P@x is switched off")
    readonly property string notReadyTitle: qsTr("P@x cannot take a task yet")

    function noKey(provider: string): string {
        return qsTr("P@x has no key for %1. Set up P@x to give it one.").arg(provider)
    }

    readonly property string localNotRunning: qsTr("Local AI is not running the model P@x uses. Set up P@x to make it ready again.")

    // --- spending (design 9.11) -------------------------------------------------
    readonly property string noPrice: qsTr("TUR does not know this model's price, so it cannot show a cost.")
    readonly property string estimates: qsTr("These are estimates from your provider's published prices. Your provider's invoice is what counts.")

    // --- steps (design 9.2) ----------------------------------------------------
    readonly property string dataTag: qsTr("Data")
    readonly property string dataNotInstructions: qsTr("It went to P@x as data. It is not an instruction and grants no permission, whatever it says.")

    // --- activity --------------------------------------------------------------
    readonly property string activityTitle: qsTr("What P@x did")
    readonly property string activityEmpty: qsTr("P@x has done nothing yet.")
    readonly property string activityNote: qsTr("This is P@x's record of what it decided and did. It keeps no words: not yours, not P@x's.")
    readonly property string whatItDid: qsTr("What it did")

    function time(seconds: real): string {
        if (!(seconds > 0))
            return ""
        return Qt.formatTime(new Date(seconds * 1000), "hh:mm")
    }

    function dateTime(seconds: real): string {
        if (!(seconds > 0))
            return ""
        return Qt.formatDateTime(new Date(seconds * 1000), "d MMM yyyy, hh:mm")
    }

    // An amount as people read it: "0.0008 USD", "1.20 USD".
    function money(amount: var, currency: string): string {
        if (amount === null || amount === undefined || isNaN(Number(amount)))
            return ""
        const value = Number(amount)
        const digits = value !== 0 && Math.abs(value) < 0.01 ? 4 : 2
        return currency.length > 0 ? qsTr("%1 %2").arg(value.toFixed(digits)).arg(currency) : value.toFixed(digits)
    }

    function tokens(count: var): string {
        if (count === null || count === undefined || Number(count) < 0)
            return ""
        return qsTr("%1 tokens").arg(Number(count).toLocaleString(Qt.locale("C"), "f", 0))
    }

    // A size in bytes, as the service says it: "0.6 GB".
    function bytes(count: var): string {
        if (count === null || count === undefined || !(Number(count) >= 0))
            return ""
        const value = Number(count)
        if (value >= 1e9)
            return qsTr("%1 GB").arg((value / 1e9).toFixed(1))
        if (value >= 1e6)
            return qsTr("%1 MB").arg((value / 1e6).toFixed(0))
        return qsTr("%1 bytes").arg(value)
    }

    // --- one task's spending, for the header -----------------------------------
    function taskSpend(spend: var): string {
        if (!spend || !spend.known)
            return ""
        if (spend.taskCost !== null && spend.taskCost !== undefined)
            return qsTr("This task: %1").arg(words.money(spend.taskCost, spend.currency))
        if (spend.taskTokens >= 0)
            return qsTr("This task: %1").arg(words.tokens(spend.taskTokens))
        return ""
    }

    function daySpend(spend: var): string {
        if (!spend || !spend.known)
            return ""
        if (spend.priceKnown && spend.todayCost !== null && spend.dayLimitCost !== null)
            return qsTr("Today: %1 of %2").arg(words.money(spend.todayCost, spend.currency)).arg(words.money(spend.dayLimitCost, spend.currency))
        if (spend.todayTokens >= 0 && spend.dayLimitTokens !== null)
            return qsTr("Today: %1 of %2").arg(words.tokens(spend.todayTokens)).arg(words.tokens(spend.dayLimitTokens))
        return ""
    }

    // --- steps -----------------------------------------------------------------
    function stepState(state: string): string {
        switch (state) {
        case "running":
            return qsTr("Running")
        case "done":
            return qsTr("Done")
        case "failed":
            return qsTr("Failed")
        case "refused":
            return qsTr("Refused")
        case "rejected":
            return qsTr("Not done")
        case "needs_you":
            return qsTr("Waiting for you")
        case "stopped":
            return qsTr("Stopped before it ended")
        default:
            return state
        }
    }

    // Why the policy allowed or refused a step, in plain words. A refusal's
    // words are the service's own sentence.
    function decision(step: var): string {
        if (!step)
            return ""
        const said = String(step.message || "")
        switch (step.decision) {
        case "allowed":
            switch (step.reason) {
            case "observe_permitted":
                return qsTr("Allowed: it only looks.")
            case "reversible_permitted":
                return qsTr("Allowed: it can be undone.")
            default:
                return qsTr("Allowed.")
            }
        case "refused":
            // The step's state already says "Refused"; the reason follows it.
            return said.length > 0 ? said : qsTr("P@x's rules do not allow it.")
        case "rejected":
            return said.length > 0 ? said : qsTr("P@x asked for something it does not have.")
        case "confirmation_required":
            return qsTr("Needs your approval, which this version cannot ask for. Not done.")
        case "authentication_required":
            return qsTr("Needs the trusted screen, which this version does not have. Not done.")
        case "deferred":
            return said.length > 0 ? qsTr("Kept for you: %1").arg(said) : qsTr("Kept for you to decide.")
        default:
            return said
        }
    }

    function actionClass(value: var): string {
        switch (Number(value)) {
        case 0:
            return qsTr("Class 0: it only looks")
        case 1:
            return qsTr("Class 1: a change that can be undone")
        case 2:
            return qsTr("Class 2: sends, buys, deletes or installs; needs your approval")
        case 3:
            return qsTr("Class 3: disks, start-up or security; needs the trusted screen")
        default:
            return ""
        }
    }

    function tier(value: var): string {
        switch (Number(value)) {
        case 0:
            return qsTr("P@x's own operation")
        case 1:
            return qsTr("A tool of a TUR program")
        case 4:
            return qsTr("A command, in the read-only sandbox")
        default:
            return ""
        }
    }

    function returned(step: var): string {
        const count = Number(step.output_bytes || 0)
        if (count <= 0)
            return ""
        return qsTr("It returned %1.").arg(words.bytes(count))
    }

    // --- what the system says when a task ends, or did not start -------------
    function cardTitle(state: string): string {
        switch (state) {
        case "refused":
            return qsTr("P@x did not start")
        case "setting":
            return qsTr("P@x's service did not accept this")
        case "notice":
            return qsTr("TUR says")
        case "cancelled":
            return qsTr("P@x stopped, as you asked")
        case "spend_limit":
            return qsTr("P@x stopped: a spending limit was reached")
        case "step_limit":
            return qsTr("P@x stopped: the task took too many steps")
        case "time_limit":
            return qsTr("P@x stopped: the task took too long")
        case "scheduled":
            return qsTr("P@x started a scheduled task")
        case "provider_refused":
            return qsTr("The AI declined")
        default:
            return qsTr("P@x could not finish")
        }
    }

    function cardNote(state: string): string {
        switch (state) {
        case "cancelled":
            return qsTr("What P@x did before it stopped is listed above. Nothing else was started.")
        case "spend_limit":
            return qsTr("What was done so far is listed above. A limit can be raised only on the trusted screen, which this version does not have; the day's limit starts again tomorrow.")
        default:
            return ""
        }
    }

    function summary(detail: var): string {
        if (!detail)
            return ""
        const done = Number(detail.done || 0)
        const refused = Number(detail.refused || 0)
        let out = done === 1 ? qsTr("Finished. One step done.") : qsTr("Finished. %1 steps done.").arg(done)
        if (refused > 0)
            out += " " + (refused === 1 ? qsTr("One step was refused.") : qsTr("%1 steps were refused.").arg(refused))
        return out
    }

    // --- activity --------------------------------------------------------------
    function outcome(value: string): string {
        switch (value) {
        case "completed":
            return qsTr("Finished")
        case "cancelled":
            return qsTr("Stopped")
        case "spend_limit":
            return qsTr("Stopped at a spending limit")
        case "":
            return qsTr("Not finished")
        default:
            return qsTr("Did not finish")
        }
    }

    function auditEvent(entry: var): string {
        switch (entry.event) {
        case "task_started":
            return qsTr("Started")
        case "task_finished":
            return qsTr("Ended: %1").arg(words.outcome(String(entry.outcome || "")))
        case "decision":
            return entry.decision === "allowed" ? qsTr("Allowed: %1").arg(String(entry.action || ""))
                                                : qsTr("Refused: %1").arg(String(entry.action || ""))
        case "executed":
            return qsTr("Ran: %1").arg(String(entry.action || ""))
        case "setting_changed":
            return qsTr("Setting changed: %1").arg(String(entry.setting || ""))
        case "output_withheld":
            return qsTr("Output withheld before it left this computer")
        default:
            return String(entry.event || "")
        }
    }

    // --- setup (design 9.12) ---------------------------------------------------
    readonly property string meetTitle: qsTr("Meet P@x")
    readonly property string meetText: qsTr("P@x is the assistant built into TUR. You can ask it, by typing, to do things on this computer, to fix problems, or to show you how to do something yourself.")
    readonly property string meetAi: qsTr("P@x works through an AI that you choose: one that runs on this computer, or a provider's service that you pay for directly. TUR does not run one and does not see what you ask.")
    readonly property string meetCharge: qsTr("You stay in charge: things that can be undone, P@x does. Before sending, buying, deleting or installing, it asks. Disks, start-up and security it can never change without you approving on a screen it cannot reach. This version does only the first: it looks, and makes changes that can be undone.")
    readonly property string whereTitle: qsTr("Where should P@x's AI run?")
    readonly property string whereLocal: qsTr("On this computer (local AI)")
    readonly property string whereLocalText: qsTr("Nothing you ask leaves this computer. A model is downloaded once; how well it runs depends on this computer.")
    readonly property string whereCloud: qsTr("With an AI provider")
    readonly property string whereCloudText: qsTr("A company runs the model. You need a key from them, and they bill you for what P@x uses.")
    readonly property string localTitle: qsTr("Choose a model for this computer")
    readonly property string localNote: qsTr("Local AI looked at this computer's memory and processor. A model that does not fit cannot be chosen.")
    readonly property string recommended: qsTr("Best fit for this computer")
    readonly property string downloaded: qsTr("Already on this computer")
    readonly property string acceptLicence: qsTr("I accept this licence's restriction")
    readonly property string useModel: qsTr("Use this model")
    readonly property string downloadTitle: qsTr("Making the model ready")
    readonly property string cancelDownload: qsTr("Cancel")
    readonly property string downloadKept: qsTr("What was downloaded is kept on this computer.")
    readonly property string providerTitle: qsTr("Choose a provider")
    readonly property string providerNote: qsTr("In alphabetical order. TUR recommends none. What each one keeps of what P@x sends is governed by its own terms.")
    readonly property string onThisComputer: qsTr("Programs on this computer")
    readonly property string companies: qsTr("Companies")
    readonly property string getKey: qsTr("Get a key")
    readonly property string dataTerms: qsTr("Read its data terms")
    readonly property string keyTitle: qsTr("Your key and model")
    readonly property string keyField: qsTr("Key")
    readonly property string keyHelp: qsTr("TUR keeps the key in this computer's password store. It is given only to P@x's service, and never written to settings files, logs or support bundles. This window does not keep it.")
    readonly property string keyStored: qsTr("A key is already stored. Leave the field empty to keep it.")
    readonly property string keyNotNeeded: qsTr("This service takes no key.")
    readonly property string showKey: qsTr("Show")
    readonly property string hideKey: qsTr("Hide")
    readonly property string checkKeyLater: qsTr("Checking a key sends a small request that the provider bills; this version does not do it. A wrong key shows when P@x is first asked something.")
    readonly property string modelLabel: qsTr("Model")
    readonly property string modelTyped: qsTr("Or type a model's name")
    readonly property string notTested: qsTr("Not tested with TUR")
    readonly property string priceUnknown: qsTr("Price not known to TUR")
    readonly property string settingsTitle: qsTr("How the model answers")
    readonly property string settingsNote: qsTr("Only what this model accepts is shown. Leave a field empty for P@x's default.")
    readonly property string settingsNone: qsTr("This model accepts no settings beyond an output limit.")
    readonly property string limitsTitle: qsTr("Spending limits")
    readonly property string limitsBetween: qsTr("A task can pass its limit by the cost of one answer: providers report what was used at the end of each answer.")
    readonly property string limitTask: qsTr("Stop a task when it has cost")
    readonly property string limitDay: qsTr("Stop for the day at")
    readonly property string limitTaskTokens: qsTr("Stop a task after this many tokens")
    readonly property string limitDayTokens: qsTr("Stop for the day after this many tokens")
    readonly property string limitsTokensWhy: qsTr("TUR does not know this model's price, so the limits are in tokens, the unit providers count in.")
    readonly property string consentTitle: qsTr("What will be sent")
    readonly property string switchOn: qsTr("Switch P@x on")
    readonly property string readyTitle: qsTr("P@x is ready")
    readonly property string done: qsTr("Done")
    readonly property string notBuiltYet: qsTr("Not in this version: P@x cannot see or use the screen, and there is no trusted screen yet, so P@x looks and makes changes that can be undone, and nothing more.")

    function consentBox(provider: string): string {
        return qsTr("I understand what will be sent to %1.").arg(provider)
    }

    function stepOf(number: int, total: int): string {
        return qsTr("Step %1 of %2").arg(number).arg(total)
    }

    function readyText(askKeys: string, stopKeys: string): string {
        return qsTr("Press %1 and type. Press %2 to stop P@x at any time.").arg(askKeys).arg(stopKeys)
    }

    function tierLabel(tier: string): string {
        switch (tier) {
        case "runs-well":
            return qsTr("Runs well")
        case "slow":
            return qsTr("Slow")
        case "does-not-fit":
            return qsTr("Does not fit")
        default:
            return tier
        }
    }

    function downloadSize(model: var): string {
        if (model.installed === "installed")
            return words.downloaded
        return qsTr("Downloads %1").arg(words.bytes(model.download_bytes))
    }

    function licence(model: var): string {
        const given = model.licence || {}
        if (given.restricts_use)
            return qsTr("Licence: %1 %2").arg(String(given.summary || given.id || "")).arg(String(given.restrictions || ""))
        return qsTr("Licence: %1").arg(String(given.summary || given.id || ""))
    }

    function progress(event: var): string {
        switch (event.event) {
        case "plan":
            return Number(event.bytes_to_download || 0) > 0 ? qsTr("Downloading %1").arg(words.bytes(event.bytes_to_download))
                                                            : qsTr("Already downloaded; checking it")
        case "progress":
            return qsTr("Downloaded %1 of %2").arg(words.bytes(event.bytes_done)).arg(words.bytes(event.bytes_total))
        case "verified":
            return qsTr("Checked what was downloaded")
        case "installed":
        case "already-present":
            return qsTr("On this computer")
        case "stopping":
            return qsTr("Stopping the model that was running")
        case "starting":
            return qsTr("Starting the model")
        case "ready":
            return qsTr("The model is running")
        case "in-use":
            return String(event.message || qsTr("Chosen for P@x"))
        case "cancelled":
            return qsTr("Stopped, as you asked.")
        case "failed":
            return String(event.message || qsTr("Local AI could not make the model ready."))
        default:
            return String(event.message || event.event || "")
        }
    }

    // The data terms of a provider as TUR read them (pax-service.md,
    // section 9; ADR-0004): a list of { label, text }.
    function terms(entry: var): var {
        const out = []
        const read = entry.data_terms
        if (entry.runs_locally || (read && read.runs_locally)) {
            out.push({ label: qsTr("Where it runs"), text: qsTr("On this computer. Nothing goes to a company.") })
            return out
        }
        if (!entry.data_terms_reviewed || !read) {
            out.push({ label: qsTr("Its terms"), text: qsTr("%1's terms are not reviewed by TUR. Read them before you choose it.").arg(entry.name) })
            return out
        }
        out.push({ label: qsTr("Keeps what it is sent"), text: String(read.stores_requests || qsTr("Not stated in what TUR read.")) })
        out.push({ label: qsTr("For how long"), text: String(read.retention || qsTr("Not stated in what TUR read.")) })
        out.push({ label: qsTr("Trains on it"), text: String(read.trains_on_data || qsTr("Not stated in what TUR read.")) })
        out.push({ label: qsTr("Where it is processed"), text: String(read.processing_location || qsTr("Not stated in what TUR read.")) })
        out.push({ label: qsTr("As TUR read them"), text: qsTr("On %1. Its own terms govern, not TUR's reading of them.").arg(String(read.reviewed_on || "")) })
        return out
    }

    // The name of a generation setting.
    function settingName(name: string): string {
        switch (name) {
        case "temperature":
            return qsTr("Temperature")
        case "top_p":
            return qsTr("Top-p")
        case "top_k":
            return qsTr("Top-k")
        case "effort":
            return qsTr("Reasoning effort")
        case "thinking_budget":
            return qsTr("Thinking budget, in tokens")
        case "max_output_tokens":
            return qsTr("Longest answer, in tokens")
        case "context_tokens":
            return qsTr("Context, in tokens")
        default:
            return name
        }
    }

    function settingRange(low: var, high: var): string {
        return qsTr("From %1 to %2.").arg(low).arg(high)
    }

    function settingOutOfRange(name: string, low: var, high: var): string {
        return qsTr("%1 can be from %2 to %3.").arg(words.settingName(name)).arg(low).arg(high)
    }

    function notANumber(name: string): string {
        return qsTr("%1 must be a number.").arg(words.settingName(name))
    }

    function wholeNumber(name: string): string {
        return qsTr("%1 must be a whole number.").arg(words.settingName(name))
    }

    readonly property string effortDefault: qsTr("P@x's default")
    readonly property string limitPositive: qsTr("A limit is a number above zero.")
}

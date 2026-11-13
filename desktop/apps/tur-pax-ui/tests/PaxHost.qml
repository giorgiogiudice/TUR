// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.PaxUi

// Puts one of P@x's views on a stage for the tests and the pictures: a
// client of the stand-in for org.tur.Agent1 (`fixture`, set by tst_main.cpp),
// a conversation, and the view, the ask bar, the conversation panel or the
// setup flow. `states` is every state the walks go through and the pictures
// are taken of; reach() gets there as a person would, by asking, stopping,
// opening and choosing, with the stand-in answering as the real service did.
Item {
    id: host

    // Each state:
    //   name      its name, and its pictures'
    //   view      bar, panel or setup
    //   status    the recorded status the service answers with
    //   task      the recorded task StartTask plays; ask: what is asked
    //   hold      [member, count]: the task holds before that signal
    //   until     running or finished
    //   stop      stopped by the Stop button while it runs
    //   absent    the service is not on the bus
    //   page      the panel's page
    //   expand    the first "Details" or "What it did" opened
    //   step      the setup screen; where, provider, model, local: what was
    //             chosen before it
    readonly property var states: [
        { name: "absent", view: "panel", absent: true },
        { name: "off", view: "bar", status: "off" },
        { name: "not-ready", view: "panel", status: "no-key" },
        { name: "ask", view: "bar", status: "ready" },
        { name: "panel-empty", view: "panel", status: "ready" },
        { name: "running", view: "panel", status: "ready", task: "two-tools", hold: ["ActionCompleted", 2], until: "running" },
        { name: "done", view: "panel", status: "ready", task: "two-tools", until: "finished" },
        { name: "done-details", view: "panel", status: "ready", task: "two-tools", until: "finished", expand: true },
        { name: "refused", view: "panel", status: "ready", task: "refused", until: "finished" },
        { name: "stopped", view: "panel", status: "ready", task: "stop", stop: true, until: "finished" },
        { name: "limit", view: "panel", status: "limit-reached", task: "spend-limit", until: "finished" },
        { name: "priced", view: "panel", status: "priced", task: "priced", until: "finished" },
        { name: "activity", view: "panel", status: "ready", page: "activity" },
        { name: "activity-open", view: "panel", status: "ready", page: "activity", expand: true },
        { name: "setup-welcome", view: "setup", status: "off", step: "welcome" },
        { name: "setup-where", view: "setup", status: "off", step: "where" },
        { name: "setup-local", view: "setup", status: "off", step: "local", where: "local" },
        { name: "setup-licence", view: "setup", status: "off", step: "local", where: "local", choose: "ministral-3-3b" },
        { name: "setup-download", view: "setup", status: "off", step: "download", where: "local", local: "qwen3-0.6b", localHold: 1 },
        { name: "setup-download-cancelled", view: "setup", status: "off", step: "download", where: "local", local: "gemma-4-e2b", cancel: true },
        { name: "setup-provider", view: "setup", status: "off", step: "provider", where: "cloud", choose: "deepseek" },
        { name: "setup-key", view: "setup", status: "off", step: "key", where: "cloud", provider: "anthropic" },
        { name: "setup-settings", view: "setup", status: "off", step: "settings", where: "cloud", provider: "openai", model: "gpt-6-luna" },
        { name: "setup-settings-refused", view: "setup", status: "off", step: "settings", where: "cloud", provider: "openai", model: "gpt-6-luna", type: { temperature: "0.3" } },
        { name: "setup-settings-invalid", view: "setup", status: "off", step: "settings", where: "cloud", provider: "openai", model: "gpt-6-luna", type: { temperature: "5" } },
        { name: "setup-limits", view: "setup", status: "ready", step: "limits", where: "cloud", provider: "anthropic", model: "claude-opus-5-5" },
        { name: "setup-consent", view: "setup", status: "off", step: "consent", where: "cloud", provider: "anthropic", model: "claude-opus-5-5" },
        { name: "setup-consent-deepseek", view: "setup", status: "off", step: "consent", where: "cloud", provider: "deepseek", model: "deepseek-flash" },
        { name: "setup-ready", view: "setup", status: "ready", step: "ready", where: "cloud", provider: "anthropic", model: "claude-opus-5-5" }
    ]

    // What was asked in each recorded task (fixtures/agent1/tasks), so that
    // the request typed here is the one the service started.
    readonly property var requests: ({
        "two-tools": "Set the value for the night.",
        "refused": "Read the value and send the report to Marta.",
        "stop": "Wait for a while.",
        "spend-limit": "Read the value again and again.",
        "priced": "Think about it."
    })

    property Item stage: null
    readonly property Item view: host.stage !== null ? host.stage.view : null
    readonly property AgentClient client: host.stage !== null ? host.stage.client : null
    readonly property Conversation conversation: host.stage !== null ? host.stage.conversation : null
    // The height the view needs for nothing to scroll.
    readonly property real naturalHeight: host.view !== null && host.view.naturalHeight !== undefined ? host.view.naturalHeight : 0

    // The state of that name.
    function state(name: string): var {
        for (const entry of host.states) {
            if (entry.name === name)
                return entry
        }
        throw new Error("no state " + name)
    }

    Component {
        id: stageComponent

        Item {
            id: stageRoot

            property string kind
            property alias client: stageClient
            property alias conversation: stageConversation
            property Item view: null

            anchors.fill: parent

            AgentClient {
                id: stageClient

                busAddress: fixture.address
            }

            Conversation {
                id: stageConversation

                client: stageClient
            }

            Component {
                id: barView

                AskBar {
                    width: 640
                    client: stageClient
                    conversation: stageConversation
                }
            }

            Component {
                id: panelView

                ConversationPanel {
                    width: 420
                    height: Math.max(760, stageRoot.height)
                    client: stageClient
                    conversation: stageConversation
                }
            }

            Component {
                id: setupView

                SetupFlow {
                    width: 600
                    height: Math.max(780, stageRoot.height)
                    client: stageClient
                }
            }

            Component.onCompleted: {
                const which = stageRoot.kind === "bar" ? barView : stageRoot.kind === "setup" ? setupView : panelView
                stageRoot.view = which.createObject(stageRoot)
            }
        }
    }

    // Starts the stage of `state`. The caller then waits as reach() does.
    function enter(state: var): void {
        host.stop()
        if (state.absent)
            fixture.stopAgent()
        else
            fixture.startAgent()
        const scenario = { status: state.status || "ready", pace_ms: 20 }
        if (state.task)
            scenario.task = state.task
        if (state.hold) {
            scenario.hold_before = state.hold[0]
            scenario.hold_count = state.hold[1]
        }
        if (state.localHold !== undefined)
            scenario.local_hold_after = state.localHold
        if (state.after_enable)
            scenario.after_enable = state.after_enable
        fixture.use(scenario)
        host.stage = stageComponent.createObject(host, { kind: state.view })
        host.client.start()
    }

    function stop(): void {
        if (host.stage === null)
            return
        host.stage.visible = false
        host.stage.destroy()
        host.stage = null
    }

    // The provider entry of the model list with that identifier.
    function provider(catalogue: var, id: string): var {
        for (const entry of (catalogue.providers || [])) {
            if (entry.id === id)
                return entry
        }
        return null
    }

    // Gets to `state` as a person would. `test` is the TestCase, whose
    // tryVerify, wait and mouseClick it uses.
    function reach(state: var, test: var): void {
        host.enter(state)
        test.tryVerify(() => host.client.presence !== AgentClient.Unknown, 5000, "the service was looked for")
        if (!state.absent)
            test.tryVerify(() => host.client.statusKnown, 10000, "the status was read")
        test.wait(30)

        if (state.view === "setup") {
            host.reachSetup(state, test)
            return
        }
        if (state.task) {
            const request = state.ask || host.requests[state.task]
            host.conversation.ask(request)
            if (state.stop) {
                test.tryVerify(() => host.conversation.running && host.conversation.entriesOf("step").length > 0, 10000,
                               "the task runs a step")
                const stopButton = test.findChild(host.view, "stopButton")
                test.verify(stopButton !== null && stopButton.visible, "Stop is shown while the task runs")
                test.mouseClick(stopButton)
            }
            if (state.until === "running")
                test.tryVerify(() => host.conversation.running && host.conversation.entriesOf("step").length >= 2, 10000,
                               "the task runs its second step")
            else
                test.tryVerify(() => !host.conversation.running && host.conversation.lastOutcome.length > 0, 15000,
                               "the task ended")
        }
        if (state.page) {
            host.view.page = state.page
            const activity = test.findChild(host.view, "activityView")
            test.tryVerify(() => activity.loaded, 10000, "the activity was read")
        }
        if (state.expand) {
            test.wait(30)
            let within = host.view
            if (state.page === "activity") {
                // The newest task that decided something.
                const activity = test.findChild(host.view, "activityView")
                for (let i = 0; i < activity.tasks.length; ++i) {
                    const entry = activity.tasks[i]
                    if (entry.kind === "task" && (entry.entries || []).some(line => line.event === "decision")) {
                        within = test.findChild(activity, "activityTask-" + i)
                        break
                    }
                }
            }
            const name = state.page === "activity" ? "activityMore" : "stepDetails"
            const disclosure = test.findChild(within, name)
            test.verify(disclosure !== null, "there is a " + name)
            test.mouseClick(disclosure)
        }
        test.wait(50)
    }

    function reachSetup(state: var, test: var): void {
        const flow = host.view
        test.tryVerify(() => (flow.catalogue.providers || []).length > 0, 10000, "the model list was read")
        flow.where = state.where || "local"
        if (state.provider) {
            flow.entry = host.provider(flow.catalogue, state.provider)
            test.verify(flow.entry !== null, "the model list has " + state.provider)
        }
        if (state.model)
            flow.model = state.model
        if (state.local) {
            // As a person would: the model chosen on the list, then "Use this
            // model"; the download screen follows.
            flow.goTo("local")
            test.tryVerify(() => test.findChild(flow, "localModel-" + state.local) !== null, 10000, "the local models were listed")
            // Chosen as a click would choose it; a click itself could land on
            // another card while the list is still being laid out.
            const list = test.findChild(flow, "setupScreen").item
            list.chosenId = state.local
            test.wait(50)
            test.mouseClick(test.findChild(flow, "continueButton"))
            test.tryVerify(() => flow.step === "download", 10000, "the model is being made ready")
            if (state.localHold !== undefined)
                test.tryVerify(() => flow.localEvents.length >= state.localHold + 1, 10000, "local AI reported progress")
            if (state.cancel) {
                test.tryVerify(() => flow.localEvents.length >= 1, 10000, "local AI made its plan")
                test.mouseClick(test.findChild(flow, "cancelDownloadButton"))
                test.tryVerify(() => flow.localEvents.length > 0 && flow.localEvents[flow.localEvents.length - 1].event === "cancelled",
                               10000, "local AI stopped")
            }
            test.wait(50)
            return
        }
        flow.goTo(state.step)
        const screen = test.findChild(flow, "setupScreen")
        test.tryVerify(() => screen.item !== null, 5000, "the screen was loaded")
        switch (state.step) {
        case "local":
            test.tryVerify(() => screen.item.models.length > 0, 10000, "the local models were listed")
            break
        case "key":
            test.tryVerify(() => screen.item.listed.length > 0, 10000, "the provider's models were listed")
            break
        case "settings":
            test.tryVerify(() => !screen.item.loading, 10000, "the model's settings were read")
            break
        case "consent":
            test.tryVerify(() => screen.item.version.length > 0, 10000, "the consent text was read")
            break
        default:
            break
        }
        if (state.choose) {
            test.wait(30)
            const found = test.findChild(flow, (state.step === "local" ? "localModel-" : "provider-") + state.choose)
            test.verify(found !== null, "there is a choice " + state.choose)
            screen.item.chosenId = state.choose
        }
        if (state.type) {
            for (const name in state.type) {
                const field = test.findChild(flow, "setting-" + name)
                test.verify(field !== null, "the model takes " + name)
                field.text = state.type[name]
            }
            test.mouseClick(test.findChild(flow, "continueButton"))
            test.tryVerify(() => screen.item.problem.length > 0 || Object.keys(screen.item.invalid).length > 0, 10000,
                           "what was typed was answered")
        }
        test.wait(50)
    }
}

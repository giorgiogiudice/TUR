// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax
import Tur.Kit.Testing as Testing

// The ask bar and the conversation panel against the stand-in for
// org.tur.Agent1, which plays what the real service signalled in CI
// (desktop-experience.md, 9.1, 9.2 and 9.11):
//
//  - a request is sent; P@x's words and each step appear as the service
//    signals them, one at a time; the answer and the end follow;
//  - a step the policy refuses says so, and why, in the service's words;
//  - what a step returned is marked as data, not instructions;
//  - Stop is shown while a task runs, with the stop key on it, and stops it:
//    the button, the key, and typing "stop";
//  - a spending limit is said by the system, once, with its code; the cost
//    of the task and of the day are shown, in money when TUR knows the price
//    and in tokens when it does not;
//  - P@x off, not ready, or not there at all: each is said, and setup is
//    offered where setup cures it;
//  - P@x's keys come from desktop/kit/pax/commands.json.
Item {
    id: root

    width: 1000
    height: 2400

    PaxHost {
        id: host

        anchors.fill: parent
    }

    SignalSpy {
        id: setupSpy

        signalName: "setupRequested"
    }

    SignalSpy {
        id: closeSpy

        signalName: "closeRequested"
    }

    TestCase {
        name: "Conversation"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            setupSpy.clear()
            closeSpy.clear()
            fixture.startAgent()
            wait(0)
        }

        function kinds() {
            const out = []
            for (let row = 0; row < host.conversation.count; ++row)
                out.push(host.conversation.entry(row).kind)
            return out
        }

        function called(method) {
            return fixture.calls().filter(call => call.method === method)
        }

        function entryItems(kind) {
            const out = []
            const visit = item => {
                if (item.objectName === "entry-" + kind && item.visible)
                    out.push(item)
                for (const child of item.children)
                    visit(child)
            }
            visit(host.view)
            return out
        }

        function test_a_request_streams_steps_and_the_answer() {
            host.reach(host.state("panel-empty"), this)
            fixture.use({ status: "ready", task: "two-tools", pace_ms: 30 })
            const field = findChild(host.view, "panelField")
            verify(field.visible, "the field is there while P@x is ready")
            field.forceActiveFocus()
            field.text = "Set the value for the night."
            keyClick(Qt.Key_Return)
            tryVerify(() => host.conversation.running, 5000, "the task started")
            const stop = findChild(host.view, "stopButton")
            verify(stop.visible, "Stop is shown while the task runs")
            verify(stop.text.indexOf(Pax.PaxCommands.stopKeys) >= 0, "the stop key is written on it")
            tryVerify(() => !host.conversation.running && host.conversation.lastOutcome === "completed", 10000, "the task ended")
            verify(!stop.visible, "Stop goes when the task has ended")
            compare(kinds(), ["you", "pax", "step", "step", "pax", "summary"])
            const asked = called("StartTask")
            compare(asked.length, 1)
            compare(asked[0].args[0], "Set the value for the night.")
            compare(field.text, "", "the field is emptied once the request is sent")

            // In the view: the person's words, P@x's words in P@x's name, the
            // steps in plain words with the policy's decision.
            compare(entryItems("you").length, 1)
            compare(entryItems("pax").length, 2)
            const steps = entryItems("step")
            compare(steps.length, 2)
            const decision = findChild(steps[0], "stepDecision")
            verify(decision.text.indexOf(Words.decision({ decision: "allowed", reason: "observe_permitted" })) >= 0, decision.text)
            const data = findChild(steps[0], "stepData")
            verify(data.visible, "what the step returned is marked")
            verify(Testing.Harness.accessible(data).name.indexOf(Words.dataNotInstructions) >= 0, "as data, not instructions, for a screen reader too")
        }

        function test_steps_arrive_one_signal_at_a_time() {
            host.reach(host.state("running"), this)
            const steps = host.conversation.entriesOf("step")
            compare(steps.length, 2)
            compare(steps[0].phase, "done")
            compare(steps[1].phase, "running", "the second step is shown running while it runs")
            compare(host.conversation.entriesOf("summary").length, 0)
            verify(host.conversation.running)
            const decision = findChild(entryItems("step")[1], "stepDecision")
            verify(decision.text.indexOf(Words.stepState("running")) === 0, decision.text)
            fixture.release()
            tryVerify(() => host.conversation.lastOutcome === "completed", 10000)
            compare(host.conversation.entriesOf("step")[1].phase, "done")
        }

        function test_a_refused_step_says_why() {
            host.reach(host.state("refused"), this)
            const refused = host.conversation.entriesOf("step").filter(step => step.phase === "refused")
            compare(refused.length, 1)
            const said = String(refused[0].step.message)
            verify(said.length > 0, "the service gave its reason")
            const line = entryItems("step").filter(item => findChild(item, "stepDecision").text.indexOf(said) >= 0)
            compare(line.length, 1, "the reason is on the step's line, in the service's words")
            verify(findChild(line[0], "stepDecision").text.indexOf(Words.stepState("refused")) === 0)
            verify(String(refused[0].code).indexOf("TUR-AI-POLICY-") === 0)
            mouseClick(findChild(line[0], "stepDetails"))
            verify(findChild(line[0], "stepTechnical").visible, "Details shows the technical layer")
            verify(findChild(line[0], "stepExact").text.length > 0, "with the exact action")
        }

        function test_stop_while_a_step_runs() {
            host.reach(host.state("stopped"), this)
            compare(host.conversation.lastOutcome, "cancelled")
            const cancels = called("CancelTask")
            compare(cancels.length, 1)
            const cards = entryItems("system")
            compare(cards.length, 1)
            compare(findChild(cards[0], "cardTitle").text, Words.cardTitle("cancelled"))
            compare(findChild(cards[0], "cardCode").code, "TUR-AI-TASK-003")
            compare(host.conversation.entriesOf("step")[0].phase, "stopped")
            verify(!findChild(host.view, "stopButton").visible)
        }

        function test_typing_stop_stops() {
            const state = Object.assign({}, host.state("stopped"))
            state.stop = false
            state.until = "running"
            state.hold = ["TaskFinished", 1]
            host.enter(state)
            tryVerify(() => host.client.statusKnown, 10000)
            host.conversation.ask("Wait for a while.")
            tryVerify(() => host.conversation.running && host.conversation.entriesOf("step").length > 0, 10000)
            const field = findChild(host.view, "panelField")
            field.forceActiveFocus()
            field.text = "Open the scanner"
            keyClick(Qt.Key_Return)
            verify(findChild(host.view, "busyNote").visible, "other words wait, and the panel says so")
            compare(field.text, "Open the scanner", "and they are kept")
            compare(called("CancelTask").length, 0)
            field.text = "stop"
            keyClick(Qt.Key_Return)
            tryVerify(() => called("CancelTask").length === 1, 5000, "typing stop stops the task")
        }

        function test_the_stop_key_stops() {
            const state = Object.assign({}, host.state("stopped"))
            state.stop = false
            host.enter(state)
            tryVerify(() => host.client.statusKnown, 10000)
            host.conversation.ask("Wait for a while.")
            tryVerify(() => host.conversation.running && host.conversation.entriesOf("step").length > 0, 10000)
            findChild(host.view, "panelField").forceActiveFocus()
            keyClick(Qt.Key_Escape, Qt.MetaModifier)
            tryVerify(() => called("CancelTask").length === 1, 5000, "Meta+Esc stops the task")
            tryVerify(() => host.conversation.lastOutcome === "cancelled", 5000)
        }

        function test_a_spending_limit_is_said_once_by_the_system() {
            host.reach(host.state("limit"), this)
            const cards = entryItems("system")
            compare(cards.length, 1)
            compare(findChild(cards[0], "cardTitle").text, Words.cardTitle("spend_limit"))
            compare(findChild(cards[0], "cardCode").code, "TUR-AI-SPEND-001")
            verify(findChild(cards[0], "cardNote").text.length > 0, "the card says what can and cannot be done")
            const spend = findChild(host.view, "spendLine")
            verify(spend.visible && spend.text.length > 0, spend.text)
            compare(spend.tone, Kit.Label.Warning, "the day's line is a warning once its limit is reached")
            verify(findChild(host.view, "noPriceLine").visible, "TUR does not know the scripted model's price, and says so")
        }

        function test_the_cost_in_money_when_the_price_is_known() {
            host.reach(host.state("priced"), this)
            const spend = findChild(host.view, "spendLine")
            verify(spend.text.indexOf("USD") >= 0, spend.text)
            verify(!findChild(host.view, "noPriceLine").visible)
            const summary = host.conversation.entriesOf("summary")[0]
            verify(summary.detail.cost > 0, "the task's cost came with its end")
        }

        function test_the_service_not_there_is_said_plainly() {
            host.reach(host.state("absent"), this)
            const notice = findChild(host.view, "paxState-absent")
            verify(notice !== null && notice.visible)
            compare(findChild(notice, "stateTitle").text, Words.absentTitle)
            verify(!findChild(notice, "setupButton").visible, "nothing to set up when there is no service")
            verify(!findChild(host.view, "panelField").visible, "nothing to type into")
        }

        function test_off_offers_setup() {
            host.reach(host.state("off"), this)
            const notice = findChild(host.view, "paxState-off")
            verify(notice !== null && notice.visible)
            compare(findChild(notice, "stateText").text, Words.offText)
            verify(!findChild(host.view, "askField").visible)
            setupSpy.target = host.view
            mouseClick(findChild(notice, "setupButton"))
            compare(setupSpy.count, 1)
            compare(called("StartTask").length, 0, "nothing was asked of P@x")
        }

        function test_not_ready_says_why() {
            host.reach(host.state("not-ready"), this)
            const notice = findChild(host.view, "paxState-not-ready")
            verify(notice !== null && notice.visible)
            verify(findChild(notice, "stateText").text.length > 0)
            verify(findChild(notice, "setupButton").visible)
        }

        function test_the_keys_come_from_commands_json() {
            host.reach(host.state("ask"), this)
            const source = Testing.Harness.readJson(paxCommandsFile)
            verify(source !== undefined, "commands.json can be read: " + paxCommandsFile)
            for (const command of source.commands) {
                const shown = findChild(host.view, "key-" + command.id)
                if (command.bound) {
                    verify(shown !== null && shown.visible, command.id + " is shown")
                    verify(shown.text.indexOf(command.keys) === 0, shown.text)
                } else {
                    verify(shown === null, command.id + " is bound by nothing and not shown")
                }
            }
        }

        function test_the_bar_closes_on_escape_and_voice_is_later() {
            host.reach(host.state("ask"), this)
            closeSpy.target = host.view
            const field = findChild(host.view, "askField")
            verify(field.activeFocus || field.focus, "the field has the keyboard when the bar opens")
            field.forceActiveFocus()
            keyClick(Qt.Key_Escape)
            compare(closeSpy.count, 1)
            verify(!findChild(host.view, "speakButton").enabled, "speaking is not in this version")
            compare(findChild(host.view, "voiceLater").text, Words.voiceLater)
        }

        function test_the_bar_sends_and_the_panel_takes_over() {
            host.reach(host.state("ask"), this)
            fixture.use({ status: "ready", task: "two-tools", pace_ms: 20 })
            const field = findChild(host.view, "askField")
            field.forceActiveFocus()
            field.text = "Set the value for the night."
            keyClick(Qt.Key_Enter)
            tryVerify(() => host.conversation.lastOutcome === "completed", 10000)
            compare(called("StartTask").length, 1)
        }
    }
}

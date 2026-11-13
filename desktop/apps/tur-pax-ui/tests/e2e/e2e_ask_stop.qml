// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The end-to-end test of tur-pax-ui (desktop/tests/pax-ui/e2e.py, `ui`):
// the conversation panel against the real `tur-pax serve` on the bus that
// `e2eBus` names, with the scripted provider behind it. A person asks; P@x's
// words and its two steps appear as they happen and end done; a second
// request runs a command in the sandbox, and the Stop button stops it within
// two seconds. The service's own signals, the provider's requests and the
// killed command are checked by e2e.py on the runner.
//
// Its name does not begin with tst_: it runs only with -input, in that job.
Item {
    id: root

    width: 900
    height: 2200

    // The number of rows each time a row came.
    property var growth: []

    Connections {
        target: conversation

        function onCountChanged() {
            root.growth = root.growth.concat([conversation.count])
        }
    }

    AgentClient {
        id: client

        busAddress: e2eBus
    }

    Conversation {
        id: conversation

        client: client
    }

    ConversationPanel {
        id: panel

        width: 420
        height: 1800
        client: client
        conversation: conversation
    }

    TestCase {
        name: "EndToEnd"
        when: windowShown

        function initTestCase() {
            verify(e2eBus.length > 0, "TUR_PAX_UI_E2E_BUS names the service's bus")
            Kit.Motion.frozen = true
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function picture(name) {
            if (Testing.Harness.outputDir.length > 0)
                Testing.Harness.grab(panel, Testing.Harness.outputDir + "/e2e-" + name + ".png")
        }

        function kinds() {
            const out = []
            for (let row = 0; row < conversation.count; ++row)
                out.push(conversation.entry(row).kind)
            return out
        }

        function test_ask_see_the_steps_then_stop() {
            client.start()
            tryVerify(() => client.presence !== AgentClient.Unknown, 10000, "the service was looked for")
            console.info("TUR-E2E presence " + client.presence + " " + client.presenceDetail)
            let answer = null
            client.call("GetStatus", [], function (reply) {
                answer = reply
            })
            tryVerify(() => answer !== null, 30000, "GetStatus was answered")
            console.info("TUR-E2E GetStatus ok=" + answer.ok + " code=" + answer.code + " message=" + answer.message)
            tryVerify(() => client.statusKnown, 30000, "the service answered")
            compare(client.status.ready, true, "P@x is on, with a key: " + JSON.stringify(client.status.problem))

            // Ask.
            const field = findChild(panel, "panelField")
            verify(field.visible)
            field.forceActiveFocus()
            field.text = "Set the value for the night."
            keyClick(Qt.Key_Return)
            // The task may be over between two looks: what came, and in which
            // order, is in `growth`, which follows the conversation's rows.
            tryVerify(() => conversation.lastOutcome.length > 0, 60000, "the task ran and ended")
            compare(conversation.lastOutcome, "completed")
            compare(kinds(), ["you", "pax", "step", "step", "pax", "summary"])
            const steps = conversation.entriesOf("step")
            compare(steps.map(step => step.step.tool), ["test_observe", "test_reversible"])
            compare(steps.map(step => step.phase), ["done", "done"])
            // One row at a time, as the service signalled: the person's words,
            // P@x's words, each step, the answer, the end.
            console.info("TUR-E2E rows as they came: " + root.growth.join(", "))
            compare(root.growth.slice(0, 6), [1, 2, 3, 4, 5, 6], "the rows came one by one")
            wait(100)
            picture("done")

            // Stop.
            field.forceActiveFocus()
            field.text = "Wait for a while."
            keyClick(Qt.Key_Return)
            tryVerify(() => conversation.running && conversation.entriesOf("step").length === 3, 30000,
                      "the command was proposed")
            const command = conversation.entriesOf("step")[2]
            compare(command.step.tool, "run_readonly_command")
            compare(command.phase, "running")
            // The command runs in the sandbox for a moment.
            wait(1500)
            picture("running")
            const stop = findChild(panel, "stopButton")
            verify(stop.visible, "Stop is shown while the task runs")
            const asked = Date.now()
            mouseClick(stop)
            tryVerify(() => conversation.lastOutcome === "cancelled", 10000, "the task was stopped")
            const took = Date.now() - asked
            console.info("TUR-E2E stopped in " + took + " ms")
            verify(took < 2000, "stopped within two seconds: " + took + " ms")
            compare(conversation.entriesOf("step")[2].phase, "stopped")
            const cards = conversation.entriesOf("system")
            compare(cards.length, 1)
            compare(cards[0].code, "TUR-AI-TASK-003")
            verify(!stop.visible, "Stop goes when nothing runs")
            wait(100)
            picture("stopped")
        }
    }
}

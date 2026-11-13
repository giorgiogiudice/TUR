// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit

// P@x's record of what it did, from the service's activity list
// (ListActivity, as the real service answered it in CI): the recent tasks,
// newest first, each with its outcome and, opened, each decision and action.
Item {
    id: root

    width: 1000
    height: 2400

    PaxHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "Activity"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        function test_recent_tasks_with_their_steps() {
            host.reach(host.state("activity"), this)
            const view = findChild(host.view, "activityView")
            verify(view.visible)
            verify(view.tasks.length > 0, "the recording has tasks")
            const asked = fixture.calls().filter(call => call.method === "ListActivity")
            compare(asked.length, 1)
            compare(asked[0].args[0], 20, "the last twenty")
            const first = findChild(view, "activityTask-0")
            verify(first !== null)
            const record = view.tasks[0]
            verify(record.task !== undefined || record.kind !== undefined)
        }

        function test_a_task_opens_to_its_decisions() {
            host.reach(host.state("activity"), this)
            const view = findChild(host.view, "activityView")
            let index = -1
            for (let i = 0; i < view.tasks.length && index < 0; ++i) {
                const entry = view.tasks[i]
                if (entry.kind === "task" && (entry.entries || []).some(line => line.event === "decision"))
                    index = i
            }
            verify(index >= 0, "the recording has a task with decisions")
            const card = findChild(view, "activityTask-" + index)
            mouseClick(findChild(card, "activityMore"))
            wait(50)
            // Its decisions are shown in words, each with the service's own
            // record line.
            let shown = []
            const collect = item => {
                if (item.text !== undefined && typeof item.text === "string" && item.visible)
                    shown.push(item.text)
                for (const child of item.children)
                    collect(child)
            }
            collect(card)
            const decisions = shown.filter(text => text.indexOf(Words.auditEvent({ event: "decision", decision: "allowed", action: "" })) === 0)
            verify(decisions.length > 0, "a decision is shown in words: " + shown.slice(0, 20).join(" | "))
        }

        function test_back_to_the_conversation() {
            host.reach(host.state("activity"), this)
            const button = findChild(host.view, "activityButton")
            compare(button.text, Words.conversation)
            mouseClick(button)
            compare(host.view.page, "conversation")
            verify(findChild(host.view, "conversationView").visible)
        }
    }
}

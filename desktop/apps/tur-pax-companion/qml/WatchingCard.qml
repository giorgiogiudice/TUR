// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// "What is P@x watching?" (tur-pax-companion.md, sections 5 and 6): what
// P@x is doing at this moment in the service's own words, exactly what the
// character itself reads, and what it never reads. It is a statement of
// fact about this program, kept true by the tests: the character has no
// code that reads a screen, a window list, a key press, the clipboard or a
// file of the person's.
Leaf {
    id: card

    // The character's state.
    property string phase: "idle"
    // The step P@x is on: title, reason. Empty between steps.
    property var step: ({})
    // Why P@x cannot take a task: code, message. Empty when it can.
    property var problem: ({})
    // How much of today's limit is left, 0 to 1, and whether that is known.
    property real fuel: 1
    property bool fuelKnown: false
    property bool saving: false

    signal activityRequested()
    signal backRequested()

    readonly property string now: {
        switch (card.phase) {
        case "asleep":
            return Words.paxOff
        case "thinking":
            return Words.paxWorking
        case "observing":
            return Words.paxObserving
        case "acting":
            return Words.paxActing
        case "paused":
            return Words.paxPaused
        case "error":
            return card.problem.message !== undefined && String(card.problem.message).length > 0
                   ? String(card.problem.message) : Words.paxProblem
        }
        return Words.paxIdle
    }
    readonly property string stepTitle: card.step.title !== undefined ? String(card.step.title) : ""

    Accessible.role: Accessible.Grouping
    Accessible.name: Words.watchingTitle

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            objectName: "watchingTitle"
            Layout.fillWidth: true
            text: Words.watchingTitle
            kind: Kit.Label.Heading
        }

        Kit.Label {
            Layout.fillWidth: true
            text: Words.paxNowTitle
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Kit.Label {
            objectName: "paxNow"
            Layout.fillWidth: true
            text: card.now
            paragraph: true
        }

        Kit.Label {
            objectName: "paxStep"
            Layout.fillWidth: true
            visible: card.stepTitle.length > 0
            text: Words.paxStep.arg(card.stepTitle) + " " + Words.allowed(String(card.step.reason || ""))
            paragraph: true
        }

        Kit.CodeLabel {
            objectName: "code"
            visible: card.phase === "error" && card.problem.code !== undefined && String(card.problem.code).length > 0
            code: card.problem.code !== undefined ? String(card.problem.code) : ""
        }

        Kit.Label {
            objectName: "fuelLine"
            Layout.fillWidth: true
            visible: card.fuelKnown && card.phase !== "asleep"
            text: Words.fuel.arg(Math.round(card.fuel * 100))
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            tabular: true
        }

        Kit.Label {
            objectName: "savingLine"
            Layout.fillWidth: true
            visible: card.saving
            text: Words.saving
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        Kit.Label {
            Layout.fillWidth: true
            text: Words.readsTitle
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Repeater {
            model: [Words.readsPax, Words.readsHeal, Words.readsIndicator, Words.readsBattery]

            delegate: Kit.Label {
                id: reads

                required property string modelData

                objectName: "reads"
                Layout.fillWidth: true
                text: reads.modelData
                paragraph: true
            }
        }

        Kit.Label {
            Layout.fillWidth: true
            Layout.topMargin: Kit.Theme.spaceRelated
            text: Words.neverTitle
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
        }

        Kit.Label {
            objectName: "neverReads"
            Layout.fillWidth: true
            text: Words.neverList
            paragraph: true
        }

        Flow {
            Layout.fillWidth: true
            Layout.topMargin: Kit.Theme.spaceRelated
            spacing: Kit.Theme.spaceRelated

            Kit.Button {
                objectName: "activityButton"
                visible: card.phase !== "asleep"
                text: Words.openActivity
                onClicked: card.activityRequested()
            }

            Kit.Button {
                objectName: "backButton"
                text: Words.back
                kind: Kit.Button.Quiet
                onClicked: card.backRequested()
            }
        }
    }
}

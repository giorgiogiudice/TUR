// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// What P@x did: its audit trail as recent tasks, newest first, each with
// every decision and action in it (ListActivity; pax-service.md, 2.1). The
// trail keeps no words, neither the person's nor P@x's, so a task is shown by
// its time, its outcome and what it did. Settings ▸ P@x ▸ Activity (task
// D-19) is the full page; this is the panel's view of the same record.
Item {
    id: view

    required property AgentClient client
    // Asked for when it becomes true.
    property bool active: false
    // How many tasks to ask for.
    property int limit: 20

    property var tasks: []
    property string problem
    property bool loaded: false

    readonly property real naturalHeight: column.implicitHeight + 2 * Kit.Theme.spaceGroup

    function load(): void {
        view.client.call("ListActivity", [view.limit], function (reply) {
            view.loaded = true
            if (reply.ok) {
                view.problem = ""
                // A list from C++ is a sequence, not always a JavaScript array.
                const list = reply.value
                view.tasks = list && list.length !== undefined ? Array.prototype.slice.call(list) : []
            } else {
                view.problem = reply.message
                view.tasks = []
            }
        })
    }

    objectName: "activityView"
    implicitHeight: view.naturalHeight

    onActiveChanged: {
        if (view.active)
            view.load()
    }

    Connections {
        target: view.client

        function onTaskFinished(): void {
            if (view.active)
                view.load()
        }
    }

    Flickable {
        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: view.naturalHeight
        boundsBehavior: Flickable.StopAtBounds

        Controls.ScrollBar.vertical: Controls.ScrollBar {
        }

        ColumnLayout {
            id: column

            x: Kit.Theme.spaceGroup
            y: Kit.Theme.spaceGroup
            width: view.width - 2 * Kit.Theme.spaceGroup
            spacing: Kit.Theme.spaceGroup

            Kit.Label {
                Layout.fillWidth: true
                text: Words.activityTitle
                kind: Kit.Label.Title
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.activityNote
                tone: Kit.Label.Secondary
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.Banner {
                objectName: "activityProblem"
                Layout.fillWidth: true
                visible: view.problem.length > 0
                severity: Kit.Banner.Negative
                text: view.problem
            }

            Kit.Label {
                objectName: "activityEmpty"
                Layout.fillWidth: true
                visible: view.loaded && view.problem.length === 0 && view.tasks.length === 0
                text: Words.activityEmpty
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Repeater {
                model: view.tasks

                delegate: Kit.Card {
                    id: card

                    required property var modelData
                    required property int index

                    readonly property var record: card.modelData
                    readonly property var entries: (card.record.entries || []).filter(
                        entry => entry.event !== "task_started" && entry.event !== "task_finished")

                    objectName: "activityTask-" + card.index
                    Layout.fillWidth: true

                    ColumnLayout {
                        anchors.fill: parent
                        spacing: Kit.Theme.spaceGrid

                        Kit.Label {
                            Layout.fillWidth: true
                            text: {
                                const record = card.record
                                if (record.kind === "task") {
                                    const who = record.attended === false ? qsTr("Scheduled task") : qsTr("Task")
                                    return qsTr("%1 · %2").arg(who).arg(Words.outcome(String(record.outcome || "")))
                                }
                                return qsTr("Settings")
                            }
                            kind: Kit.Label.Strong
                            wrapMode: Text.Wrap
                        }

                        Kit.Label {
                            Layout.fillWidth: true
                            text: {
                                const record = card.record
                                const parts = [Words.dateTime(Number(record.started_at || 0))]
                                if (record.provider_id)
                                    parts.push(String(record.provider_id))
                                if (record.kind === "task")
                                    parts.push(card.entries.length === 1 ? qsTr("one entry") : qsTr("%1 entries").arg(card.entries.length))
                                return parts.filter(part => part.length > 0).join(" · ")
                            }
                            kind: Kit.Label.Small
                            tone: Kit.Label.Secondary
                            wrapMode: Text.Wrap
                        }

                        Kit.CodeLabel {
                            visible: String(card.record.error_code || "").length > 0
                            code: String(card.record.error_code || "")
                        }

                        Kit.Disclosure {
                            id: more

                            objectName: "activityMore"
                            visible: card.entries.length > 0
                            text: Words.whatItDid
                        }

                        Repeater {
                            model: more.expanded ? card.entries : []

                            delegate: ColumnLayout {
                                id: line

                                required property var modelData

                                Layout.fillWidth: true
                                spacing: 2

                                Kit.Label {
                                    Layout.fillWidth: true
                                    text: Words.auditEvent(line.modelData)
                                          + (line.modelData["class"] !== null && line.modelData["class"] !== undefined
                                             ? " · " + Words.actionClass(line.modelData["class"]) : "")
                                    tone: line.modelData.decision === "refused" ? Kit.Label.Negative : Kit.Label.Primary
                                    wrapMode: Text.Wrap
                                }

                                Kit.Label {
                                    Layout.fillWidth: true
                                    text: String(line.modelData.message || "")
                                    kind: Kit.Label.Mono
                                    tone: Kit.Label.Secondary
                                    wrapMode: Text.WrapAnywhere
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

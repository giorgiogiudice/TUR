// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// Setting up P@x (desktop-experience.md, 9.12), in its own window. The person
// can leave at any point, and P@x is switched on only on the last screen but
// one, where the consent text is accepted.
//
// Two ways through, after "Meet P@x" and "Where should P@x's AI run?":
//
//   on this computer:  model for this computer → making it ready (download,
//                      with progress and Cancel) → how it answers → limits
//                      → what will be sent → ready
//   with a provider:   provider (with its data terms) → key and model → how
//                      it answers → limits → what will be sent → ready
//
// Screens 6 and 7 of the design (seeing the screen; the trusted screen) are
// not here: the service cannot see the screen in this version, and the
// trusted screen does not exist (task D-16). P@x is then limited to classes
// 0 and 1, which the ready screen says.
Kit.Page {
    id: flow

    required property AgentClient client

    // welcome, where, local, download, provider, key, settings, limits,
    // consent, ready
    property string step: "welcome"
    property string where: "local"
    // ListModels' answer.
    property var catalogue: ({})
    // The provider chosen, as its entry of ListModels, and the model.
    property var entry: ({})
    property string model
    // The local model being made ready, and what local AI said so far.
    property var localModel: ({})
    property string job
    property var localEvents: []

    readonly property var path: flow.where === "local"
                                ? ["welcome", "where", "local", "download", "settings", "limits", "consent", "ready"]
                                : ["welcome", "where", "provider", "key", "settings", "limits", "consent", "ready"]
    readonly property int stepIndex: flow.path.indexOf(flow.step)
    readonly property string provider: flow.where === "local" ? "local" : String(flow.entry.id || "")
    readonly property string providerName: flow.where === "local" ? String(flow.localModel.name || qsTr("local AI"))
                                                                   : String(flow.entry.name || flow.provider)
    readonly property real naturalHeight: column.implicitHeight + 2 * Kit.Theme.spaceMargin

    // The person finished, or left.
    signal finished()

    function goTo(name: string): void {
        flow.step = name
        flick.contentY = 0
    }

    // Starts again from the first screen, or from `at`.
    function restart(at: string): void {
        flow.entry = ({})
        flow.model = ""
        flow.localModel = ({})
        flow.job = ""
        flow.localEvents = []
        flow.goTo(at.length > 0 ? at : "welcome")
        flow.client.call("ListModels", [], function (reply) {
            if (reply.ok)
                flow.catalogue = reply.value
        })
    }

    Component.onCompleted: flow.restart("")

    Connections {
        target: flow.client

        function onLocalModelProgress(model: string, event: var): void {
            if (model !== String(flow.localModel.id || ""))
                return
            flow.localEvents = flow.localEvents.concat([event])
        }
    }

    // A screen is laid out in the frames after it is loaded.
    Timer {
        id: focusLater

        interval: 50
        onTriggered: {
            const loaded = screen.item as SetupScreen
            if (loaded !== null && loaded.initialFocus !== null)
                loaded.initialFocus.forceActiveFocus(Qt.TabFocusReason)
        }
    }

    Component {
        id: welcomeScreen

        SetupScreen {
            title: Words.meetTitle
            initialFocus: meetContinue

            Pax.PaxName {
                objectName: "paxName"
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.meetText
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.meetAi
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.meetCharge
                paragraph: true
                wrapMode: Text.Wrap
            }

            Flow {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    objectName: "notNowButton"
                    text: Words.notNow
                    onClicked: flow.finished()
                }

                Kit.Button {
                    id: meetContinue

                    objectName: "continueButton"
                    text: Words.continueLabel
                    kind: Kit.Button.Primary
                    onClicked: flow.goTo("where")
                }
            }
        }
    }

    Component {
        id: whereScreen

        SetupScreen {
            title: Words.whereTitle
            initialFocus: localChoice

            ChoiceCard {
                id: localChoice

                objectName: "whereLocal"
                Layout.fillWidth: true
                title: Words.whereLocal
                subtitle: Words.whereLocalText
                selected: flow.where === "local"
                onClicked: flow.where = "local"
            }

            ChoiceCard {
                objectName: "whereCloud"
                Layout.fillWidth: true
                title: Words.whereCloud
                subtitle: Words.whereCloudText
                selected: flow.where === "cloud"
                onClicked: flow.where = "cloud"
            }

            Flow {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    objectName: "backButton"
                    text: Words.back
                    onClicked: flow.goTo("welcome")
                }

                Kit.Button {
                    objectName: "continueButton"
                    text: Words.continueLabel
                    kind: Kit.Button.Primary
                    onClicked: flow.goTo(flow.where === "local" ? "local" : "provider")
                }
            }
        }
    }

    Component {
        id: localScreen

        SetupLocal {
            client: flow.client
            chosenId: String(flow.localModel.id || "")
            onBackRequested: flow.goTo("where")
            onStarted: function (model, job) {
                flow.localEvents = []
                flow.localModel = model
                flow.job = job
                flow.goTo("download")
            }
        }
    }

    Component {
        id: downloadScreen

        SetupScreen {
            id: download

            readonly property var last: flow.localEvents.length > 0 ? flow.localEvents[flow.localEvents.length - 1] : ({})
            readonly property string phase: {
                const name = String(download.last.event || "")
                if (name === "cancelled" || name === "failed" || name === "in-use")
                    return name
                return "working"
            }
            // The latest figures local AI gave, from a plan or a progress line.
            readonly property var figures: {
                for (let i = flow.localEvents.length - 1; i >= 0; --i) {
                    const event = flow.localEvents[i]
                    if (event.bytes_total !== null && event.bytes_total !== undefined && event.bytes_done !== null && event.bytes_done !== undefined)
                        return event
                }
                return null
            }

            title: Words.downloadTitle
            initialFocus: download.phase === "working" ? cancelButton : downloadNext

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("%1 · %2").arg(String(flow.localModel.name || "")).arg(Words.downloadSize(flow.localModel))
                kind: Kit.Label.Strong
                wrapMode: Text.Wrap
            }

            Kit.ProgressBar {
                objectName: "downloadProgress"
                Layout.fillWidth: true
                visible: download.phase === "working"
                label: download.last.event ? Words.progress(download.last) : Words.downloadTitle
                indeterminate: download.figures === null || Number(download.figures.bytes_total) <= 0
                from: 0
                to: 1
                value: download.figures !== null && Number(download.figures.bytes_total) > 0
                       ? Number(download.figures.bytes_done) / Number(download.figures.bytes_total) : 0
                detail: download.figures !== null && Number(download.figures.bytes_total) > 0
                        ? qsTr("%1 of %2").arg(Words.bytes(download.figures.bytes_done)).arg(Words.bytes(download.figures.bytes_total))
                        : ""
            }

            Repeater {
                model: flow.localEvents

                delegate: RowLayout {
                    id: eventLine

                    required property var modelData

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceRelated

                    Kit.Glyph {
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 1
                        size: 14
                        kind: eventLine.modelData.event === "failed" ? Kit.Glyph.Negative
                              : eventLine.modelData.event === "cancelled" ? Kit.Glyph.Warning : Kit.Glyph.Check
                        tint: eventLine.modelData.event === "failed" ? Kit.Theme.negative
                              : eventLine.modelData.event === "cancelled" ? Kit.Theme.warning : Kit.Theme.positive
                    }

                    Kit.Label {
                        Layout.fillWidth: true
                        text: Words.progress(eventLine.modelData)
                        kind: Kit.Label.Small
                        wrapMode: Text.Wrap
                    }
                }
            }

            Kit.Label {
                objectName: "downloadKept"
                Layout.fillWidth: true
                visible: download.phase === "cancelled" || download.phase === "failed"
                text: Words.downloadKept
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Kit.CodeLabel {
                visible: String(download.last.code || "").length > 0 && download.phase !== "working"
                code: String(download.last.code || "")
            }

            Flow {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    id: cancelButton

                    objectName: "cancelDownloadButton"
                    visible: download.phase === "working"
                    text: Words.cancelDownload
                    onClicked: {
                        download.clearProblem()
                        flow.client.call("CancelLocalModel", [], function (reply) {
                            if (!reply.ok)
                                download.refused(reply)
                        })
                    }
                }

                Kit.Button {
                    objectName: "backButton"
                    visible: download.phase === "cancelled" || download.phase === "failed"
                    text: Words.back
                    onClicked: flow.goTo("local")
                }

                Kit.Button {
                    id: downloadNext

                    objectName: "continueButton"
                    visible: download.phase === "in-use"
                    text: Words.continueLabel
                    kind: Kit.Button.Primary
                    onClicked: {
                        flow.model = String(flow.localModel.id || "")
                        flow.goTo("settings")
                    }
                }
            }
        }
    }

    Component {
        id: providerScreen

        SetupProvider {
            catalogue: flow.catalogue
            chosenId: String(flow.entry.id || "")
            onBackRequested: flow.goTo("where")
            onChosenProvider: function (entry) {
                flow.entry = entry
                flow.goTo("key")
            }
        }
    }

    Component {
        id: keyScreen

        SetupKey {
            client: flow.client
            entry: flow.entry
            chosenModel: flow.model
            onBackRequested: flow.goTo("provider")
            onDone: function (model) {
                flow.model = model
                flow.goTo("settings")
            }
        }
    }

    Component {
        id: settingsScreen

        SetupSettings {
            client: flow.client
            provider: flow.provider
            model: flow.model
            onBackRequested: flow.goTo(flow.where === "local" ? "local" : "key")
            onDone: flow.goTo("limits")
        }
    }

    Component {
        id: limitsScreen

        SetupScreen {
            id: limits

            readonly property var status: flow.client.status
            readonly property var spending: limits.status.spending || {}
            readonly property bool inMoney: limits.status.price_known === true
            readonly property string currency: String(limits.spending.currency || "")

            function value(field: Kit.TextField): var {
                const typed = field.text.trim()
                if (typed.length === 0)
                    return null
                const number = Number(typed)
                return isNaN(number) || number <= 0 ? undefined : number
            }

            // The limits in effect, once the status says them; what the
            // person has typed is never overwritten.
            function fill(): void {
                const shown = function (limit) {
                    const value = limits.inMoney ? limit.cost : limit.tokens
                    return value === null || value === undefined ? "" : String(value)
                }
                if (taskField.text.length === 0)
                    taskField.text = shown(limits.spending.task_limit || {})
                if (dayField.text.length === 0)
                    dayField.text = shown(limits.spending.day_limit || {})
            }

            function apply(): void {
                limits.clearProblem()
                const task = limits.value(taskField)
                const day = limits.value(dayField)
                taskField.errorText = task === undefined ? Words.limitPositive : ""
                dayField.errorText = day === undefined ? Words.limitPositive : ""
                if (task === undefined || day === undefined)
                    return
                const chosen = limits.inMoney ? { task_cost: task, day_cost: day } : { task_tokens: task, day_tokens: day }
                flow.client.call("SetLimits", [JSON.stringify(chosen)], function (reply) {
                    if (reply.ok)
                        flow.goTo("consent")
                    else
                        limits.refused(reply)
                })
            }

            title: Words.limitsTitle
            initialFocus: taskField

            Component.onCompleted: {
                limits.fill()
                flow.client.refresh()
            }

            Connections {
                target: flow.client

                function onStatusChanged(): void {
                    limits.fill()
                }
            }

            Kit.Label {
                Layout.fillWidth: true
                visible: !limits.inMoney
                text: Words.limitsTokensWhy
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.TextField {
                id: taskField

                objectName: "taskLimit"
                Layout.fillWidth: true
                label: limits.inMoney ? Words.limitTask : Words.limitTaskTokens
                helperText: limits.currency.length > 0 ? limits.currency : ""
                inputMethodHints: Qt.ImhFormattedNumbersOnly
            }

            Kit.TextField {
                id: dayField

                objectName: "dayLimit"
                Layout.fillWidth: true
                label: limits.inMoney ? Words.limitDay : Words.limitDayTokens
                helperText: limits.currency.length > 0 ? limits.currency : ""
                inputMethodHints: Qt.ImhFormattedNumbersOnly
            }

            Kit.Label {
                Layout.fillWidth: true
                text: (limits.inMoney ? Words.estimates + " " : "") + Words.limitsBetween
                tone: Kit.Label.Secondary
                paragraph: true
                wrapMode: Text.Wrap
            }

            Flow {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    objectName: "backButton"
                    text: Words.back
                    onClicked: flow.goTo("settings")
                }

                Kit.Button {
                    objectName: "continueButton"
                    text: Words.continueLabel
                    kind: Kit.Button.Primary
                    onClicked: limits.apply()
                }
            }
        }
    }

    Component {
        id: consentScreen

        SetupConsent {
            client: flow.client
            provider: flow.provider
            providerName: flow.where === "local" ? "" : flow.providerName
            onBackRequested: flow.goTo("limits")
            onDone: flow.goTo("ready")
        }
    }

    Component {
        id: readyScreen

        SetupScreen {
            title: Words.readyTitle
            initialFocus: doneButton

            Component.onCompleted: flow.client.refresh()

            Pax.PaxName {
                objectName: "paxName"
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.readyText(Pax.PaxCommands.commands.find(command => command.id === "ask").keys, Pax.PaxCommands.stopKeys)
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Try: “Which programs start when I sign in?”")
                paragraph: true
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                text: Words.notBuiltYet
                tone: Kit.Label.Secondary
                paragraph: true
                wrapMode: Text.Wrap
            }

            KeysLine {
                Layout.fillWidth: true
            }

            Kit.Button {
                id: doneButton

                objectName: "doneButton"
                text: Words.done
                kind: Kit.Button.Primary
                onClicked: flow.finished()
            }
        }
    }

    Flickable {
        id: flick

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: column.implicitHeight
        boundsBehavior: Flickable.StopAtBounds

        Controls.ScrollBar.vertical: Controls.ScrollBar {
        }

        ColumnLayout {
            id: column

            width: flick.width
            spacing: Kit.Theme.spaceRelated

            Kit.Label {
                objectName: "stepOf"
                Layout.fillWidth: true
                visible: flow.stepIndex >= 0
                text: Words.stepOf(flow.stepIndex + 1, flow.path.length)
                kind: Kit.Label.Caps
                tone: Kit.Label.Secondary
            }

            Loader {
                id: screen

                objectName: "setupScreen"
                Layout.fillWidth: true
                sourceComponent: {
                    switch (flow.step) {
                    case "where":
                        return whereScreen
                    case "local":
                        return localScreen
                    case "download":
                        return downloadScreen
                    case "provider":
                        return providerScreen
                    case "key":
                        return keyScreen
                    case "settings":
                        return settingsScreen
                    case "limits":
                        return limitsScreen
                    case "consent":
                        return consentScreen
                    case "ready":
                        return readyScreen
                    default:
                        return welcomeScreen
                    }
                }
                onLoaded: focusLater.restart()
            }
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, how the model answers: the generation settings the chosen model
// accepts, and only those (GetGenerationSettings; pax-service.md, 2.5). Each
// shows its range and P@x's default; a field left empty keeps the default.
// What is typed is checked here against the range first, then by the
// service, whose refusal is shown in its own words ("This model takes
// temperature, top-p and top-k only when its reasoning effort is none …").
SetupScreen {
    id: screen

    required property AgentClient client
    required property string provider
    required property string model

    property var described: ({})
    property bool loading: true
    property string effort
    // Field name -> the reason what is typed there cannot be sent.
    property var invalid: ({})

    readonly property var accepts: screen.described.accepts || {}
    // The settings shown as fields, in the order a person thinks of them.
    readonly property var numeric: ["thinking_budget", "temperature", "top_p", "top_k", "max_output_tokens", "context_tokens"]
                                   .filter(name => screen.accepts[name] !== undefined)
    readonly property var whole: ["thinking_budget", "top_k", "max_output_tokens", "context_tokens"]
    readonly property bool takesEffort: screen.accepts.effort !== undefined
    readonly property bool takesNothing: !screen.loading && screen.numeric.length === 0 && !screen.takesEffort

    signal done()
    signal backRequested()

    // What the person typed, checked; null if something cannot be sent.
    function collect(): var {
        const out = {}
        const problems = {}
        for (let i = 0; i < fields.count; ++i) {
            const field = fields.itemAt(i) as Kit.TextField
            if (field === null)
                continue
            const name = field.objectName.replace("setting-", "")
            const typed = field.text.trim()
            if (typed.length === 0)
                continue
            const value = Number(typed)
            const range = screen.accepts[name] || {}
            if (isNaN(value))
                problems[name] = Words.notANumber(name)
            else if (screen.whole.indexOf(name) >= 0 && !Number.isInteger(value))
                problems[name] = Words.wholeNumber(name)
            else if ((range.min !== undefined && value < range.min) || (range.max !== undefined && value > range.max))
                problems[name] = Words.settingOutOfRange(name, range.min, range.max)
            else
                out[name] = value
        }
        if (screen.effort.length > 0)
            out.effort = screen.effort
        screen.invalid = problems
        return Object.keys(problems).length > 0 ? null : out
    }

    function apply(): void {
        screen.clearProblem()
        const chosen = screen.collect()
        if (chosen === null)
            return
        if (Object.keys(chosen).length === 0) {
            screen.done()
            return
        }
        screen.client.call("SetGenerationSettings", [screen.provider, screen.model, JSON.stringify(chosen)], function (reply) {
            if (reply.ok)
                screen.done()
            else
                screen.refused(reply)
        })
    }

    title: Words.settingsTitle
    initialFocus: continueButton

    Component.onCompleted: {
        screen.client.call("GetGenerationSettings", [screen.provider, screen.model], function (reply) {
            screen.loading = false
            if (!reply.ok) {
                screen.refused(reply)
                return
            }
            screen.described = reply.value
            const chosen = reply.value.chosen || {}
            if (chosen.effort)
                screen.effort = String(chosen.effort)
        })
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Model: %1").arg(screen.model)
        kind: Kit.Label.Strong
        wrapMode: Text.WrapAnywhere
    }

    Kit.Label {
        Layout.fillWidth: true
        text: screen.takesNothing ? Words.settingsNone : Words.settingsNote
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    Repeater {
        model: screen.described.notices || []

        delegate: Kit.Label {
            id: notice

            required property string modelData

            Layout.fillWidth: true
            text: notice.modelData
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }
    }

    // The reasoning effort: one of the values the model takes, or P@x's
    // default.
    Kit.Label {
        Layout.fillWidth: true
        visible: screen.takesEffort
        text: Words.settingName("effort")
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Flow {
        objectName: "effortChoices"
        Layout.fillWidth: true
        visible: screen.takesEffort
        spacing: Kit.Theme.spaceRelated

        Repeater {
            model: screen.takesEffort ? [""].concat(screen.accepts.effort.values || []) : []

            delegate: ChoiceCard {
                id: choice

                required property string modelData

                objectName: "effort-" + (choice.modelData.length > 0 ? choice.modelData : "default")
                padding: Kit.Theme.spaceRelated
                title: choice.modelData.length > 0 ? choice.modelData : Words.effortDefault
                selected: screen.effort === choice.modelData
                onClicked: screen.effort = choice.modelData
            }
        }
    }

    Repeater {
        id: fields

        model: screen.numeric

        delegate: Kit.TextField {
            id: field

            required property string modelData

            readonly property var range: screen.accepts[field.modelData] || {}
            readonly property var chosenValue: (screen.described.chosen || {})[field.modelData]

            objectName: "setting-" + field.modelData
            Layout.fillWidth: true
            label: Words.settingName(field.modelData)
            text: field.chosenValue !== undefined && field.chosenValue !== null ? String(field.chosenValue) : ""
            helperText: {
                let out = Words.settingRange(field.range.min, field.range.max)
                if (field.range["default"] !== undefined)
                    out += " " + qsTr("P@x's default: %1.").arg(field.range["default"])
                else if (field.range.provider_default !== undefined)
                    out += " " + qsTr("The provider's default: %1.").arg(field.range.provider_default)
                return out
            }
            errorText: screen.invalid[field.modelData] || ""
            inputMethodHints: Qt.ImhFormattedNumbersOnly
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.accepts.stop !== undefined
        text: qsTr("Stop sequences are not set here: tur-pax models settings sets them.")
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Flow {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "backButton"
            text: Words.back
            onClicked: screen.backRequested()
        }

        Kit.Button {
            id: continueButton

            objectName: "continueButton"
            text: Words.continueLabel
            kind: Kit.Button.Primary
            enabled: !screen.loading
            onClicked: screen.apply()
        }
    }
}

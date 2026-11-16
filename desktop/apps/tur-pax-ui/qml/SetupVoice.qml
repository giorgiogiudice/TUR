// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, voice (docs/architecture/pax-voice.md, sections 1, 6 and 7): after
// P@x is switched on, speaking to it is offered, and nothing more is
// switched on unless the person asks here.
//
// The free voice comes first: recognition and P@x's voice on this computer,
// with what is downloaded once (the sizes the service gives, GetVoiceStatus),
// each model's licence in plain words and the credit a voice asks for. Then
// the language, and "P@x answers out loud" (off). Then live conversation:
// on this computer first (free); a provider's live engine is marked
// optional and paid with the person's own key, says that the voice is sent
// to that company, and needs the box under its text ticked.
//
// "Download and switch on" calls PrepareVoice with the size that was shown,
// follows its VoiceProgress, then SetVoiceSettings. "Not now" leaves voice
// off.
//
// The owner's decisions of 16 November 2026 (pax-voice.md, report
// pax-voice-1 section 9): the language follows the system's when P@x speaks
// it, otherwise it is detected; where the model this computer runs well is
// weak in a language (Whisper tiny in Italian), the screen says so plainly
// and offers the better models as a choice, with their size, how they suit
// this computer and any credit their licence asks for (Parakeet's CC BY);
// "Always wait for me to press Send" is a setting, off; spoken answers are
// in a voice on this computer; a provider's live engine is suggested, under
// the local one, when the person has that provider's key, at its cheapest
// model's price a minute.
SetupScreen {
    id: screen

    required property AgentClient client

    property var status: ({})
    property string language: "auto"
    property bool answersAloud: false
    property string liveEngine: "local"
    property bool alwaysWait: false
    // The recognition model chosen here; empty: the one recommended.
    property string recognitionChoice: ""
    property bool working: false
    property var events: []

    readonly property var voices: {
        const given = (screen.status.synthesis || {}).voices
        if (Array.isArray(given))
            return given
        // An object of language → voice is read the same way.
        const out = []
        for (const language in (given || {}))
            out.push(Object.assign({ language: language }, given[language]))
        return out
    }
    readonly property var recognition: screen.status.recognition || {}
    readonly property string recommendedId: String(screen.recognition.id || screen.recognition.model || "")
    readonly property var alternatives: screen.status.alternatives || []
    readonly property var programs: screen.status.engines || []
    readonly property var suggestion: (screen.status.live || {}).suggestion || null
    readonly property var chosenRecognition: {
        for (const model of screen.alternatives) {
            if (String(model.id || model.model || "") === screen.recognitionChoice)
                return model
        }
        return screen.recognition
    }
    readonly property bool choseAlternative: screen.recognitionChoice.length > 0
                                             && screen.recognitionChoice !== screen.recommendedId
    readonly property var engines: (screen.status.live || {}).engines || []
    readonly property var chosenEngine: {
        for (const engine of screen.engines) {
            if (engine.id === screen.liveEngine)
                return engine
        }
        return null
    }
    readonly property bool engineNeedsConsent: screen.chosenEngine !== null && screen.chosenEngine.kind === "provider"
                                               && screen.chosenEngine.consent_current !== true
    // What will be downloaded: the service's total, with the recognition
    // model chosen here in place of the recommended one.
    readonly property real needed: {
        const total = Number(screen.status.download_bytes_needed || 0)
        if (!screen.choseAlternative)
            return total
        return total - screen.missingBytes(screen.recognition) + screen.missingBytes(screen.chosenRecognition)
    }
    // Every speech model, detector, program and voice that is to be ready,
    // by id, for PrepareVoice.
    readonly property var models: {
        const ids = []
        const add = function (model, key) {
            const id = String((model || {}).id || (model || {})[key] || "")
            if (id.length > 0 && ids.indexOf(id) < 0)
                ids.push(id)
        }
        add(screen.chosenRecognition, "model")
        add(screen.status.voice_activity, "model")
        for (const program of screen.programs)
            add(program, "id")
        for (const voice of screen.voices)
            add(voice, "voice")
        return ids
    }
    readonly property bool loaded: Object.keys(screen.status).length > 0

    signal done()
    signal skipped()

    function missingBytes(model: var): real {
        const given = model || {}
        if (given.installed === true || given.installed === "installed")
            return 0
        return Number(given.download_bytes || 0)
    }

    function read(): void {
        screen.client.call("GetVoiceStatus", [], function (reply) {
            if (!reply.ok) {
                screen.refused(reply)
                return
            }
            screen.status = reply.value
            const settings = reply.value.settings || {}
            screen.language = String(settings.language || "auto")
            screen.answersAloud = settings.spoken_replies === true
            screen.liveEngine = String((settings.live || {}).engine || "local")
            screen.alwaysWait = settings.transcript_check === "wait"
            screen.recognitionChoice = String((settings.recognition || {}).model || "")
        })
    }

    function switchOn(): void {
        screen.clearProblem()
        if (screen.engineNeedsConsent && !liveConsent.checked)
            return
        screen.working = true
        if (screen.needed > 0) {
            screen.events = []
            const options = JSON.stringify({ download_bytes: screen.needed, models: screen.models })
            screen.client.call("PrepareVoice", [options], function (reply) {
                if (!reply.ok) {
                    screen.working = false
                    screen.refused(reply)
                }
            })
            return
        }
        screen.apply()
    }

    function apply(): void {
        const chosen = {
            enabled: true,
            language: screen.language,
            spoken_replies: screen.answersAloud,
            transcript_check: screen.alwaysWait ? "wait" : "show",
            live: { engine: screen.liveEngine }
        }
        if (screen.choseAlternative)
            chosen.recognition = { model: screen.recognitionChoice }
        if (screen.suggestion !== null && screen.liveEngine === String(screen.suggestion.engine || ""))
            chosen.live.model = String(screen.suggestion.model || "")
        if (screen.chosenEngine !== null && screen.chosenEngine.kind === "provider")
            chosen.live_consent_version = String(screen.chosenEngine.consent_version || "")
        screen.client.call("SetVoiceSettings", [JSON.stringify(chosen)], function (reply) {
            screen.working = false
            if (reply.ok)
                screen.done()
            else
                screen.refused(reply)
        })
    }

    function languageName(code: string): string {
        return Words.languageName(code)
    }

    title: Words.voiceTitle
    initialFocus: switchOnButton

    Component.onCompleted: screen.read()

    Connections {
        target: screen.client

        function onVoiceProgress(model: string, event: var): void {
            if (!screen.working)
                return
            screen.events = screen.events.concat([event])
            if (event.event === "ready") {
                screen.apply()
            } else if (event.event === "failed" || event.event === "cancelled") {
                screen.working = false
                screen.problem = Words.voiceProgress(event)
                screen.problemCode = String(event.code || "")
                screen.read()
            }
        }
    }

    // --- the free voice ------------------------------------------------------------
    Kit.Label {
        objectName: "voiceFree"
        Layout.fillWidth: true
        text: Words.voiceFree
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: Words.voiceFreeText
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: Words.voiceHow
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    ColumnLayout {
        objectName: "voiceRecognition"
        Layout.fillWidth: true
        visible: screen.loaded
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            Layout.fillWidth: true
            text: Words.voiceRecognition
            kind: Kit.Label.Strong
            wrapMode: Text.Wrap
        }

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("%1 · %2").arg(String(screen.recognition.name || screen.recognition.model || ""))
                                 .arg(Words.voiceDownload(screen.recognition.installed === true ? 0 : screen.recognition.download_bytes))
            wrapMode: Text.Wrap
        }

        Kit.Label {
            Layout.fillWidth: true
            visible: text.length > 0
            text: (screen.recognition.reasons || []).join(" ")
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }

        Kit.Label {
            Layout.fillWidth: true
            visible: screen.recognition.licence !== undefined
            text: Words.licence(screen.recognition)
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }

        // Said plainly where the model this computer runs well is weak.
        Kit.Label {
            objectName: "recognitionWarning"
            Layout.fillWidth: true
            visible: text.length > 0
            text: String(screen.recognition.warning || "")
            wrapMode: Text.Wrap
        }

        Kit.Label {
            Layout.fillWidth: true
            visible: screen.alternatives.length > 0
            text: Words.voiceRecognitionChoice
            kind: Kit.Label.Strong
            wrapMode: Text.Wrap
        }

        Repeater {
            // The recommended model first, then the others the service
            // offers, better Italian first.
            model: screen.alternatives.length > 0 ? [screen.recognition].concat(screen.alternatives) : []

            delegate: ChoiceCard {
                id: modelCard

                required property var modelData
                required property int index

                readonly property string modelId: String(modelCard.modelData.id || modelCard.modelData.model || "")
                readonly property string credit: String(modelCard.modelData.credit || "none")

                objectName: "recognition-" + modelCard.modelId
                Layout.fillWidth: true
                title: String(modelCard.modelData.name || modelCard.modelId)
                subtitle: [modelCard.index === 0 ? Words.voiceRecommended : "",
                           Words.tierLabel(String(modelCard.modelData.tier || "")),
                           Words.voiceDownload(screen.missingBytes(modelCard.modelData)),
                           modelCard.credit !== "none" ? qsTr("Credit: %1").arg(modelCard.credit) : ""]
                          .filter(part => part.length > 0).join(" · ")
                unavailable: modelCard.modelData.tier === "does-not-fit"
                selected: modelCard.index === 0 ? !screen.choseAlternative
                                                : screen.recognitionChoice === modelCard.modelId
                onClicked: {
                    if (!modelCard.unavailable)
                        screen.recognitionChoice = modelCard.index === 0 ? "" : modelCard.modelId
                }
            }
        }
    }

    // The programs the voice runs, downloaded with the voices: Piper is
    // GPL-3.0-or-later and runs as a separate program.
    Repeater {
        model: screen.programs

        delegate: Kit.Label {
            id: programLine

            required property var modelData

            objectName: "voiceEngine-" + String(programLine.modelData.id || "")
            Layout.fillWidth: true
            text: qsTr("%1 · %2").arg(Words.voiceEngine(programLine.modelData))
                                 .arg(Words.voiceDownload(screen.missingBytes(programLine.modelData)))
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }
    }

    ColumnLayout {
        objectName: "voiceVoices"
        Layout.fillWidth: true
        visible: screen.voices.length > 0
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            Layout.fillWidth: true
            text: Words.voiceVoices
            kind: Kit.Label.Strong
            wrapMode: Text.Wrap
        }

        Repeater {
            model: screen.voices

            delegate: ColumnLayout {
                id: voiceLine

                required property var modelData

                objectName: "voice-" + String(voiceLine.modelData.voice || voiceLine.modelData.id || "")
                Layout.fillWidth: true
                spacing: 0

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("%1: %2 · %3").arg(screen.languageName(String(voiceLine.modelData.language || "")))
                                            .arg(String(voiceLine.modelData.name || voiceLine.modelData.voice || ""))
                                            .arg(Words.voiceDownload(voiceLine.modelData.installed === true ? 0 : voiceLine.modelData.download_bytes))
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.licence(voiceLine.modelData)
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }

                Kit.Label {
                    Layout.fillWidth: true
                    visible: String(voiceLine.modelData.credit || "none") !== "none"
                    text: qsTr("Credit: %1").arg(String(voiceLine.modelData.credit || ""))
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    // --- the language ----------------------------------------------------------------
    Kit.Label {
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceLanguage
        kind: Kit.Label.Strong
        wrapMode: Text.Wrap
    }

    Kit.Label {
        objectName: "voiceLanguageLine"
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceLanguageLine(Object.assign({}, screen.status.language || {}, { setting: screen.language }))
        wrapMode: Text.Wrap
    }

    ChoiceCard {
        objectName: "language-auto"
        Layout.fillWidth: true
        visible: screen.loaded
        title: Words.voiceAutomatic
        subtitle: Words.voiceLanguageLine(Object.assign({}, screen.status.language || {}, { setting: "auto" }))
        selected: screen.language === "auto"
        onClicked: screen.language = "auto"
    }

    Repeater {
        model: screen.voices

        delegate: ChoiceCard {
            id: languageCard

            required property var modelData

            objectName: "language-" + String(languageCard.modelData.language || "")
            Layout.fillWidth: true
            title: screen.languageName(String(languageCard.modelData.language || ""))
            selected: screen.language === String(languageCard.modelData.language || "")
            onClicked: screen.language = String(languageCard.modelData.language || "")
        }
    }

    Kit.CheckBox {
        id: aloud

        objectName: "answersAloud"
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceAnswersAloud
        checked: screen.answersAloud
        onToggled: screen.answersAloud = aloud.checked
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceAnswersAloudText
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Kit.CheckBox {
        id: alwaysWaitBox

        objectName: "alwaysWait"
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceAlwaysWait
        checked: screen.alwaysWait
        onToggled: screen.alwaysWait = alwaysWaitBox.checked
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.voiceAlwaysWaitText
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Kit.Label {
        objectName: "wakeWordLine"
        Layout.fillWidth: true
        visible: screen.loaded
        text: Words.wakeWordOff
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    // --- live conversation --------------------------------------------------------------
    Kit.Label {
        objectName: "liveTitle"
        Layout.fillWidth: true
        visible: screen.engines.length > 0
        text: Words.liveTitle
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.engines.length > 0
        text: Words.liveText
        paragraph: true
        wrapMode: Text.Wrap
    }

    Repeater {
        // On this computer first; then a provider suggested because the
        // person has its key; then the other providers, in their order.
        model: {
            const suggested = screen.suggestion !== null ? String(screen.suggestion.engine || "") : ""
            return screen.engines.filter(engine => engine.kind === "local")
                       .concat(screen.engines.filter(engine => engine.kind !== "local" && engine.id === suggested))
                       .concat(screen.engines.filter(engine => engine.kind !== "local" && engine.id !== suggested))
        }

        delegate: ChoiceCard {
            id: engineCard

            required property var modelData

            readonly property bool provider: engineCard.modelData.kind === "provider"
            readonly property bool suggested: screen.suggestion !== null
                                              && String(screen.suggestion.engine || "") === String(engineCard.modelData.id || "")

            objectName: "liveEngine-" + String(engineCard.modelData.id || "")
            Layout.fillWidth: true
            title: String(engineCard.modelData.name || engineCard.modelData.id || "")
            subtitle: engineCard.suggested
                      ? Words.liveSuggestion(screen.suggestion)
                      : engineCard.provider
                        ? [Words.liveOptional, Words.voiceSentTo(String(engineCard.modelData.company || engineCard.modelData.name || "")),
                           Words.perMinute(engineCard.modelData.price_per_minute, String(engineCard.modelData.currency || ""))]
                          .filter(part => part.length > 0).join(" · ")
                        : Words.liveLocal
            unavailable: engineCard.modelData.built === false
            selected: screen.liveEngine === String(engineCard.modelData.id || "")
            onClicked: {
                if (!engineCard.unavailable)
                    screen.liveEngine = String(engineCard.modelData.id || "")
            }

            Kit.Label {
                objectName: "liveSuggestion"
                Layout.fillWidth: true
                visible: engineCard.suggested
                text: Words.liveSuggested(String((screen.suggestion || {}).company || ""))
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }
        }
    }

    // What a provider's live engine means, in its own words, and the box.
    Rectangle {
        objectName: "liveConsentBox"
        Layout.fillWidth: true
        visible: screen.engineNeedsConsent
        implicitHeight: liveConsentText.implicitHeight + 2 * Kit.Theme.spaceGroup
        radius: Kit.Theme.radiusCard
        color: Kit.Theme.surfaceRaised
        border.width: Kit.Theme.lineWidth
        border.color: Kit.Theme.lineSubtle

        Kit.Label {
            id: liveConsentText

            objectName: "liveConsentText"
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: Kit.Theme.spaceGroup
            text: screen.chosenEngine !== null ? String(screen.chosenEngine.consent_text || "") : ""
            paragraph: true
            wrapMode: Text.Wrap
        }
    }

    Kit.CheckBox {
        id: liveConsent

        objectName: "liveConsentCheck"
        Layout.fillWidth: true
        visible: screen.engineNeedsConsent
        text: screen.chosenEngine !== null
              ? Words.liveConsentBox(String(screen.chosenEngine.company || screen.chosenEngine.name || ""))
              : ""
    }

    // --- making it ready ----------------------------------------------------------------
    Kit.ProgressBar {
        id: voiceBar

        readonly property var last: screen.events.length > 0 ? screen.events[screen.events.length - 1] : ({})

        objectName: "voiceProgress"
        Layout.fillWidth: true
        visible: screen.working && screen.needed > 0
        label: voiceBar.last.event ? Words.voiceProgress(voiceBar.last) : Words.downloadTitle
        indeterminate: !(Number(voiceBar.last.bytes_total || 0) > 0)
        from: 0
        to: 1
        value: Number(voiceBar.last.bytes_total || 0) > 0
               ? Number(voiceBar.last.bytes_done || 0) / Number(voiceBar.last.bytes_total) : 0
        detail: Number(voiceBar.last.bytes_total || 0) > 0
                ? qsTr("%1 of %2").arg(Words.bytes(voiceBar.last.bytes_done || 0)).arg(Words.bytes(voiceBar.last.bytes_total))
                : ""
    }

    Flow {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "voiceNotNowButton"
            text: Words.voiceNotNow
            enabled: !screen.working
            onClicked: screen.skipped()
        }

        Kit.Button {
            id: switchOnButton

            objectName: "voiceSwitchOnButton"
            text: screen.needed > 0 ? qsTr("%1 (%2)").arg(Words.downloadAndSwitchOn).arg(Words.bytes(screen.needed))
                                    : Words.switchVoiceOn
            kind: Kit.Button.Primary
            enabled: screen.loaded && !screen.working && (!screen.engineNeedsConsent || liveConsent.checked)
            onClicked: screen.switchOn()
        }
    }
}

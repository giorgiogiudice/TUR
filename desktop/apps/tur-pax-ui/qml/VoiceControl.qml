// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// The microphone of P@x's bar and panel (docs/architecture/pax-voice.md,
// sections 4.1 and 7). The window shows; P@x's service listens: everything
// here is a call on org.tur.Agent1, and the microphone is open only between
// StartListening and the end of the turn, which the service says
// (VoiceStateChanged, MicrophoneOpen).
//
//   ""            nothing under way: "Speak to P@x"
//   listening     the microphone is open: lit, a level meter, "Listening…";
//                 the button again, or Enter, sends; Esc drops
//   recognising   the person stopped; "Recognising…"
//   show          what was heard is in the field and a task follows: P@x
//                 acts on nothing above looking for 2 s; Esc stops the task
//   wait          what was heard is in the field and waits for Enter
//                 (SendTranscript): the settings say so, or another program
//                 was playing sound
//
// Push-to-talk by the key: the bar opened by P@x's key (Main's "ask-held")
// calls keyOpened(). If no release of Space reaches the bar within 350 ms,
// the key is held: the bar listens until Space is let go. Whether the
// release reaches a layer-shell surface that took the keyboard while the
// key was held is NOT verified on KWin; if it never comes, the service's own
// limit (30 s, or silence) ends the turn, and the microphone stays visibly
// on until then.
ColumnLayout {
    id: voice

    required property AgentClient client
    required property Conversation conversation
    // Where the words heard go, to be corrected or sent.
    required property Kit.TextField field
    // P@x can take a request now (the bar's or the panel's `ready`).
    property bool allowed: true
    // Whether "Talk with P@x" is offered here.
    property bool offersLive: true

    property string phase: ""
    property string turn
    property real level: 0
    property string waitReason
    property string shownTask
    // A sentence of the system's about the last turn, and its code.
    property string note
    property string noteCode
    property bool pushToTalk: false

    readonly property var status: voice.client.voiceStatus
    readonly property bool known: Object.keys(voice.status).length > 0
    readonly property bool available: voice.client.voice === "ready" || voice.client.voice === "speaking"
    readonly property bool active: voice.phase === "listening" || voice.phase === "recognising"
    readonly property bool liveAvailable: voice.available && ((voice.status.live || {}).available === true)
    readonly property bool holding: holdTimer.running

    signal setupRequested()

    // Opens the microphone for a turn.
    function start(trigger: string): void {
        if (!voice.available || !voice.allowed || voice.active)
            return
        voice.note = ""
        voice.noteCode = ""
        voice.phase = "listening"
        voice.level = 0
        const options = JSON.stringify({ trigger: trigger, shown_by_caller: true })
        voice.client.call("StartListening", [options], function (reply) {
            if (!reply.ok) {
                voice.reset()
                voice.note = String(reply.message || "")
                voice.noteCode = String(reply.code || "")
                return
            }
            voice.turn = String(reply.value || "")
        })
    }

    // Closes the microphone: `send` recognises what was heard, otherwise it
    // is dropped.
    function stop(send: bool): void {
        if (voice.phase !== "listening")
            return
        voice.pushToTalk = false
        if (voice.turn.length === 0) {
            voice.reset()
            return
        }
        voice.phase = send ? "recognising" : ""
        if (!send)
            voice.note = Words.voiceDropped
        voice.client.call("StopListening", [voice.turn, send], function (reply) {
            if (!reply.ok && voice.phase === "recognising") {
                voice.reset()
                voice.note = String(reply.message || "")
                voice.noteCode = String(reply.code || "")
            }
        })
    }

    function reset(): void {
        holdTimer.stop()
        showTimer.stop()
        voice.phase = ""
        voice.turn = ""
        voice.level = 0
        voice.waitReason = ""
        voice.shownTask = ""
        voice.pushToTalk = false
    }

    // Esc. True if the voice took it.
    function cancel(): bool {
        if (voice.phase === "listening") {
            voice.stop(false)
            return true
        }
        if (voice.phase === "show") {
            // The task started from what was heard: stopped, as Stop would.
            voice.conversation.stop()
            voice.reset()
            voice.field.text = ""
            return true
        }
        if (voice.phase === "wait") {
            voice.reset()
            voice.field.text = ""
            voice.note = Words.voiceDropped
            return true
        }
        return false
    }

    // Enter. True if the voice took it.
    function send(): bool {
        if (voice.phase === "listening") {
            voice.stop(true)
            return true
        }
        if (voice.phase === "wait") {
            const words = voice.field.text.trim()
            if (voice.conversation.running) {
                // One task at a time: the words are typed words now, which
                // the panel keeps, or which stop the task if they say so.
                voice.reset()
                return false
            }
            if (words.length === 0 || voice.conversation.starting)
                return true
            voice.conversation.sendTranscript(voice.turn, words)
            voice.field.text = ""
            voice.reset()
            return true
        }
        return voice.phase === "recognising"
    }

    // The bar was opened by P@x's key, which may still be held.
    // Whether voice is on is looked at when the key has been held long
    // enough: a window the key just started may not know it yet.
    function keyOpened(): void {
        if (!voice.active)
            holdTimer.restart()
    }

    // The keys of the field the bar forwards: Space held is push-to-talk.
    function keyPressed(event: var): void {
        if (event.key === Qt.Key_Space && (holdTimer.running || voice.pushToTalk))
            event.accepted = true
    }

    function keyReleased(event: var): void {
        if (event.key !== Qt.Key_Space || event.isAutoRepeat)
            return
        if (holdTimer.running) {
            // A tap: the bar is for typing.
            holdTimer.stop()
            event.accepted = true
        } else if (voice.pushToTalk) {
            event.accepted = true
            voice.stop(true)
        }
    }

    spacing: Kit.Theme.spaceGrid

    Timer {
        id: holdTimer

        interval: 350
        onTriggered: {
            if (!voice.available || !voice.allowed)
                return
            voice.pushToTalk = true
            voice.start("push-to-talk")
        }
    }

    Timer {
        id: showTimer

        interval: 2000
        onTriggered: {
            if (voice.phase !== "show")
                return
            // The words are the task's request now, in the conversation.
            voice.field.text = ""
            voice.reset()
        }
    }

    Connections {
        target: voice.client

        function onVoiceLevel(turn: string, level: real): void {
            if (voice.phase === "listening" && (voice.turn.length === 0 || turn === voice.turn))
                voice.level = level
        }

        function onVoiceStateChanged(state: string, details: var): void {
            const about = String((details || {}).turn || "")
            if (voice.turn.length === 0 || about !== voice.turn)
                return
            if (state === "recognising" && voice.phase === "listening") {
                // The service ended the turn itself (silence, its limit).
                voice.phase = "recognising"
                voice.pushToTalk = false
            } else if ((state === "ready" || state === "off") && voice.active) {
                const reason = String((details || {}).reason || "")
                if (reason === "nothing_heard" || reason === "discarded" || reason === "stop" || reason.length === 0
                        || voice.phase === "listening") {
                    voice.reset()
                    if (reason === "discarded" || reason === "stop")
                        voice.note = Words.voiceDropped
                    if (String((details || {}).message || "").length > 0) {
                        voice.note = String(details.message)
                        voice.noteCode = String(details.error_code || "")
                    }
                }
            }
        }

        function onVoiceTranscript(turn: string, details: var): void {
            if (turn !== voice.turn && !(voice.turn.length === 0 && voice.active))
                return
            const heard = details || {}
            if (heard.final === false)
                return
            voice.turn = turn
            if (heard.empty === true || String(heard.text || "").trim().length === 0) {
                voice.reset()
                voice.note = Words.voiceNothingHeard
                return
            }
            voice.field.text = String(heard.text)
            if (heard.check === "wait") {
                voice.phase = "wait"
                voice.waitReason = String(heard.reason || "")
                voice.field.forceActiveFocus()
            } else {
                voice.phase = "show"
                voice.shownTask = String(heard.task || "")
                showTimer.restart()
            }
        }
    }

    // --- the microphone, and what it is doing ------------------------------------
    // A Flow, so that at a large text size the controls go onto a second
    // line instead of being cut off.
    Flow {
        Layout.fillWidth: true
        visible: voice.available || voice.active || voice.phase !== ""
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            id: micButton

            objectName: "micButton"
            text: voice.phase === "listening" ? Words.stopListening : Words.speakToPax
            kind: voice.phase === "listening" ? Kit.Button.Primary : Kit.Button.Secondary
            enabled: voice.phase === "listening"
                     || (voice.available && voice.allowed && voice.phase !== "recognising"
                         && !voice.conversation.live && !voice.conversation.liveStarting)
            Accessible.name: text
            Accessible.description: Words.voiceHoldHint
            onClicked: {
                if (voice.phase === "listening")
                    voice.stop(true)
                else
                    voice.start("button")
            }
        }

        // The microphone is open: lit, and how loud it is, so that the person
        // sees it hears.
        Item {
            objectName: "micLit"
            visible: voice.active || voice.client.microphoneOpen
            implicitWidth: micMark.width + Kit.Theme.spaceRelated + levelMeter.width
            implicitHeight: micButton.height
            width: implicitWidth
            height: implicitHeight

            MicGlyph {
                id: micMark

                objectName: "micMark"
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                lit: voice.client.microphoneOpen || voice.phase === "listening"
                size: 18
            }

            Rectangle {
                id: levelMeter

                objectName: "levelMeter"
                anchors.left: micMark.right
                anchors.leftMargin: Kit.Theme.spaceRelated
                anchors.verticalCenter: parent.verticalCenter
                width: 64
                height: 6
                radius: 3
                visible: voice.phase === "listening"
                color: Kit.Theme.surfaceSunken
                Accessible.ignored: true

                Rectangle {
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: parent.width * Math.max(0, Math.min(1, voice.level))
                    radius: 3
                    color: Kit.Theme.accent
                }
            }
        }

        Kit.Button {
            objectName: "talkButton"
            visible: voice.offersLive && voice.liveAvailable && voice.phase === ""
                     && !voice.conversation.live && !voice.conversation.liveStarting
            text: Words.talkWithPax
            kind: Kit.Button.Quiet
            enabled: voice.allowed && !voice.conversation.running
            onClicked: voice.conversation.startLive()
        }
    }

    Kit.Label {
        objectName: "voicePhase"
        Layout.fillWidth: true
        visible: text.length > 0
        text: {
            switch (voice.phase) {
            case "listening":
                return Words.listening
            case "recognising":
                return Words.recognising
            case "show":
                return Words.voiceShow
            case "wait":
                return voice.waitReason === "other_sound" ? Words.voiceWaitOtherSound : Words.voiceWait
            default:
                return ""
            }
        }
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    // What the system says about the last turn: nothing heard, dropped,
    // or the service's refusal in its own words.
    Kit.Label {
        objectName: "voiceNote"
        Layout.fillWidth: true
        visible: voice.note.length > 0 && voice.phase === ""
        text: voice.noteCode.length > 0 ? qsTr("%1 (%2)").arg(voice.note).arg(voice.noteCode) : voice.note
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    // Voice is off, or not ready: the service's own reason, and setup.
    RowLayout {
        objectName: "voiceOff"
        Layout.fillWidth: true
        visible: voice.known && !voice.available && !voice.active && voice.phase === ""
        spacing: Kit.Theme.spaceRelated

        MicGlyph {
            Layout.alignment: Qt.AlignVCenter
            lit: false
        }

        Kit.Label {
            objectName: "voiceOffText"
            Layout.fillWidth: true
            text: {
                const problem = voice.status.problem
                if (problem && String(problem.message || "").length > 0)
                    return String(problem.message)
                return Words.voiceOffHint
            }
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            wrapMode: Text.Wrap
        }

        Kit.Button {
            objectName: "voiceSetupButton"
            text: Words.voiceSetUp
            kind: Kit.Button.Quiet
            onClicked: voice.setupRequested()
        }
    }
}

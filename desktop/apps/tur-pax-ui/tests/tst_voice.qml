// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// Speaking to P@x from its window (docs/architecture/pax-voice.md, sections
// 4.1, 5 and 7), against the stand-in for org.tur.Agent1. The stand-in's
// voice is written by hand from the design (fixtures/voice; the real
// service's voice needs a microphone and is not recorded):
//
//  - the microphone is offered when voice is ready, and says why not, with
//    setup, when it is off;
//  - pressing it opens the microphone through the service (StartListening,
//    shown by the window), lit with a level meter while it listens; pressed
//    again it sends, Esc drops what was heard;
//  - what was heard comes into the field: with "show" a task follows and
//    the person's words are marked spoken; with "wait" Enter sends it
//    (SendTranscript), as corrected;
//  - held, P@x's key listens until Space is let go; a tap does not listen;
//  - a live conversation: the microphone lit the whole time, the person's
//    words and P@x's, an interruption, End and the stop key end it;
//  - setup offers the free local voice first, with what it downloads, and a
//    provider's live engine only with its consent ticked.
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

    TestCase {
        name: "Voice"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            setupSpy.clear()
            fixture.startAgent()
            wait(0)
        }

        function called(method) {
            return fixture.calls().filter(call => call.method === method)
        }

        function bar(voiceTurn) {
            host.reach({ name: "voice-test", view: "bar", status: "ready", voice: "ready", voiceTurn: voiceTurn || "show" }, this)
            return host.view
        }

        function test_the_microphone_is_offered_when_voice_is_ready() {
            const view = bar()
            const mic = findChild(view, "micButton")
            verify(mic.visible && mic.enabled, "the microphone can be pressed")
            compare(mic.text, Words.speakToPax)
            verify(!findChild(view, "voiceOff").visible)
            verify(!findChild(view, "micMark").visible, "not lit while nothing listens")
            verify(findChild(view, "talkButton").visible, "a live conversation is offered")
        }

        function test_voice_off_says_why_and_offers_setup() {
            host.reach(host.state("ask"), this)
            tryVerify(() => host.client.voice === "off", 5000)
            verify(!findChild(host.view, "micButton").visible || !findChild(host.view, "micButton").enabled)
            const off = findChild(host.view, "voiceOff")
            verify(off.visible)
            compare(findChild(host.view, "voiceOffText").text, host.client.voiceStatus.problem.message)
            setupSpy.target = host.view
            mouseClick(findChild(host.view, "voiceSetupButton"))
            compare(setupSpy.count, 1)
            compare(called("StartListening").length, 0, "the microphone was not opened")
        }

        function test_a_turn_listens_shows_what_was_heard_and_a_task_follows() {
            const view = bar("show")
            mouseClick(findChild(view, "micButton"))
            const started = called("StartListening")
            compare(started.length, 1)
            const options = JSON.parse(started[0].args[0])
            compare(options.trigger, "button")
            compare(options.shown_by_caller, true, "the window says it shows the microphone")
            tryVerify(() => host.client.microphoneOpen, 5000, "the service says the microphone is open")
            tryVerify(() => view.voice.level > 0, 5000, "the level moves")
            verify(findChild(view, "micMark").visible && findChild(view, "micMark").lit, "the microphone is lit")
            verify(findChild(view, "levelMeter").visible)
            compare(findChild(view, "voicePhase").text, Words.listening)
            compare(findChild(view, "micButton").text, Words.stopListening)
            verify(findChild(view, "askField").readOnly, "nothing is typed while it listens")

            mouseClick(findChild(view, "micButton"))
            const stopped = called("StopListening")
            compare(stopped.length, 1)
            compare(stopped[0].args[1], true, "sent, not dropped")
            tryVerify(() => findChild(view, "askField").text === "Set the value for the night.", 5000, "what was heard is in the field")
            tryVerify(() => !host.client.microphoneOpen, 5000, "the microphone is closed")
            tryVerify(() => host.conversation.lastOutcome === "completed", 15000, "the task that followed ended")
            const you = host.conversation.entriesOf("you")
            compare(you.length, 1, "the spoken words are one row, found by the task")
            verify(you[0].spoken, "marked spoken")
            compare(you[0].phase, "started")
            compare(called("StartTask").length, 0, "the window started no task itself: the service did")
        }

        function test_escape_drops_what_was_heard() {
            const view = bar("show")
            mouseClick(findChild(view, "micButton"))
            tryVerify(() => host.client.microphoneOpen, 5000)
            findChild(view, "askField").forceActiveFocus()
            keyClick(Qt.Key_Escape)
            const stopped = called("StopListening")
            compare(stopped.length, 1)
            compare(stopped[0].args[1], false, "dropped")
            tryVerify(() => !host.client.microphoneOpen, 5000)
            compare(view.voice.phase, "")
            compare(findChild(view, "voiceNote").text, Words.voiceDropped)
            wait(200)
            compare(host.conversation.count, 0, "nothing was asked")
        }

        function test_enter_sends_what_waits_as_corrected() {
            host.reach(host.state("ask-transcript-wait"), this)
            const view = host.view
            const field = findChild(view, "askField")
            compare(field.text, "Set the value for the night.")
            compare(findChild(view, "voicePhase").text, Words.voiceWaitOtherSound)
            verify(!field.readOnly, "it can be corrected")
            compare(host.conversation.count, 0, "it is not a request until it is sent")
            field.forceActiveFocus()
            keyClick(Qt.Key_Enter)
            const sent = called("SendTranscript")
            compare(sent.length, 1)
            compare(sent[0].args[1], "Set the value for the night.")
            tryVerify(() => host.conversation.lastOutcome === "completed", 15000)
            verify(host.conversation.entriesOf("you")[0].spoken)
        }

        function test_nothing_heard_is_said_and_asks_nothing() {
            const view = bar("empty")
            mouseClick(findChild(view, "micButton"))
            tryVerify(() => host.client.microphoneOpen, 5000)
            mouseClick(findChild(view, "micButton"))
            tryVerify(() => findChild(view, "voiceNote").visible, 5000)
            compare(findChild(view, "voiceNote").text, Words.voiceNothingHeard)
            compare(host.conversation.count, 0)
        }

        function test_holding_the_key_listens_until_space_is_let_go() {
            const view = bar("show")
            const field = findChild(view, "askField")
            field.forceActiveFocus()
            view.keyOpened()
            tryVerify(() => called("StartListening").length === 1, 5000, "held past 350 ms: it listens")
            compare(JSON.parse(called("StartListening")[0].args[0]).trigger, "push-to-talk")
            tryVerify(() => host.client.microphoneOpen, 5000)
            keyRelease(Qt.Key_Space)
            tryVerify(() => called("StopListening").length === 1, 5000, "let go: it sends")
            compare(called("StopListening")[0].args[1], true)
            verify(field.text.indexOf(" ") !== 0, "the held Space typed nothing")
        }

        function test_a_tap_of_the_key_does_not_listen() {
            const view = bar("show")
            findChild(view, "askField").forceActiveFocus()
            view.keyOpened()
            wait(100)
            keyRelease(Qt.Key_Space)
            wait(500)
            compare(called("StartListening").length, 0, "a tap opens the bar for typing")
            compare(findChild(view, "askField").text, "", "and types no space")
        }

        function test_a_live_conversation_keeps_the_microphone_lit_until_it_ends() {
            host.reach(host.state("live"), this)
            const view = host.view
            compare(JSON.parse(called("StartLiveSession")[0].args[0]).shown_by_caller, true)
            const liveBar = findChild(view, "liveBar")
            verify(liveBar.visible, "the panel says a conversation runs")
            verify(findChild(view, "liveMicMark").lit)
            verify(host.client.microphoneOpen, "the service says the microphone is open")
            verify(!findChild(view, "panelVoice").visible, "no second microphone while it runs")
            const heard = host.conversation.entriesOf("you")
            verify(heard.length >= 1 && heard[0].spoken && heard[0].phase === "heard")
            const said = host.conversation.entriesOf("pax")
            verify(said[0].detail.interrupted === true, "P@x's words cut off by the person are marked")
            compare(said[said.length - 1].text, "I will leave it as it is.")

            mouseClick(findChild(view, "endLiveButton"))
            compare(called("EndLiveSession").length, 1)
            tryVerify(() => !host.conversation.live, 5000, "it ended")
            tryVerify(() => !host.client.microphoneOpen, 5000, "the microphone is closed")
            verify(!liveBar.visible)
            const cards = host.conversation.entriesOf("system")
            compare(cards[cards.length - 1].phase, "live-ended")
        }

        function test_the_stop_key_ends_a_live_conversation() {
            host.reach(host.state("live"), this)
            findChild(host.view, "panelField").forceActiveFocus()
            keyClick(Qt.Key_Escape, Qt.MetaModifier)
            tryVerify(() => called("EndLiveSession").length + called("CancelTask").length >= 1, 5000)
            tryVerify(() => !host.conversation.live, 5000)
        }

        function test_setup_offers_the_free_voice_first_and_downloads_what_it_showed() {
            host.reach(host.state("setup-voice"), this)
            const flow = host.view
            const screen = findChild(flow, "setupScreen").item
            verify(findChild(flow, "voiceFree").visible)
            verify(findChild(flow, "voice-en_US-ljspeech-medium") !== null, "an English voice")
            verify(findChild(flow, "voice-it_IT-paola-medium") !== null, "an Italian voice")
            const local = findChild(flow, "liveEngine-local")
            const provider = findChild(flow, "liveEngine-openai-realtime")
            verify(local.selected, "live conversation on this computer is chosen")
            verify(local.mapToItem(flow, 0, 0).y < provider.mapToItem(flow, 0, 0).y, "on this computer comes first")
            verify(provider.subtitle.indexOf(Words.liveOptional) === 0, "a provider is optional and paid")
            verify(!findChild(flow, "answersAloud").checked, "P@x answers out loud is off")
            const needed = host.client.voiceStatus.download_bytes_needed
            verify(needed > 0)
            const button = findChild(flow, "voiceSwitchOnButton")
            verify(button.text.indexOf(Words.bytes(needed)) >= 0, "the size is shown before anything is downloaded")
            mouseClick(button)
            const prepared = called("PrepareVoice")
            compare(prepared.length, 1)
            compare(JSON.parse(prepared[0].args[0]).download_bytes, needed, "the size that was shown")
            tryVerify(() => called("SetVoiceSettings").length === 1, 10000, "switched on once it is ready")
            const chosen = JSON.parse(called("SetVoiceSettings")[0].args[0])
            compare(chosen.enabled, true)
            compare(chosen.spoken_replies, false)
            compare(chosen.live.engine, "local")
            verify(chosen.live_consent_version === undefined)
            tryVerify(() => flow.step === "ready", 5000)
        }

        function test_a_providers_live_engine_needs_its_consent() {
            host.reach(host.state("setup-voice-live"), this)
            const flow = host.view
            const text = findChild(flow, "liveConsentText")
            verify(text.visible && text.text.indexOf("streamed to OpenAI") >= 0, "the service's words: the voice goes to OpenAI")
            const button = findChild(flow, "voiceSwitchOnButton")
            verify(!button.enabled, "not before the box is ticked")
            mouseClick(findChild(flow, "liveConsentCheck"))
            verify(button.enabled)
            mouseClick(button)
            tryVerify(() => called("SetVoiceSettings").length === 1, 10000)
            const chosen = JSON.parse(called("SetVoiceSettings")[0].args[0])
            compare(chosen.live.engine, "openai-realtime")
            compare(chosen.live_consent_version, "1")
        }

        function test_not_now_leaves_voice_off() {
            host.reach(host.state("setup-voice"), this)
            mouseClick(findChild(host.view, "voiceNotNowButton"))
            tryVerify(() => host.view.step === "ready", 5000)
            compare(called("PrepareVoice").length, 0)
            compare(called("SetVoiceSettings").length, 0)
        }
    }
}

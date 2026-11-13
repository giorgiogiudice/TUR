// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.PaxUi
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// P@x's setup (desktop-experience.md, 9.12) against the stand-in for
// org.tur.Agent1, answering as the real service answered in CI:
//
//  - consent: the service's text shown as it is; "Switch P@x on" only once
//    the box is ticked, and then Enable with the version that was shown;
//  - local AI: the models by how they run here, their download sizes and
//    licences; a licence that restricts use must be accepted; making a model
//    ready shows local AI's real progress, and Cancel stops it;
//  - providers: each one's data terms as TUR read them, or that TUR has not
//    reviewed them; the key goes to the service and stays nowhere here;
//  - generation settings: only what the model accepts, checked against its
//    ranges here and by the service, whose refusal is shown in its words;
//  - limits in the unit the price allows.
Item {
    id: root

    width: 1000
    height: 3200

    PaxHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "Setup"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        function called(method) {
            return fixture.calls().filter(call => call.method === method)
        }

        function screen() {
            return findChild(host.view, "setupScreen").item
        }

        function test_consent_gates_switching_on() {
            host.reach(host.state("setup-consent"), this)
            const text = findChild(host.view, "consentText")
            verify(text.text.indexOf("What P@x sends to Anthropic") === 0, "the service's text, as it is")
            verify(text.text.indexOf("Anthropic's terms are not reviewed by TUR") >= 0, "with what TUR knows of the provider's terms")
            const shown = screen().version
            verify(shown.length > 0)
            const switchOn = findChild(host.view, "switchOnButton")
            verify(!switchOn.enabled, "not before the box is ticked")
            compare(called("Enable").length, 0)
            mouseClick(findChild(host.view, "consentCheck"))
            verify(switchOn.enabled)
            mouseClick(switchOn)
            tryVerify(() => host.view.step === "ready", 5000, "P@x is ready")
            const enabled = called("Enable")
            compare(enabled.length, 1)
            compare(enabled[0].args[0], shown, "the version of the text that was shown")
            tryVerify(() => host.client.status.enabled === true, 5000, "and the service says P@x is on")
        }

        function test_reviewed_terms_are_in_the_consent() {
            host.reach(host.state("setup-consent-deepseek"), this)
            const text = findChild(host.view, "consentText").text
            verify(text.indexOf("What P@x sends to DeepSeek") === 0, text.slice(0, 80))
            verify(text.indexOf("People's Republic of China") >= 0, "where DeepSeek processes it, as TUR read its terms")
        }

        function test_local_models_by_how_they_run_here() {
            host.reach(host.state("setup-local"), this)
            const models = screen().models
            verify(models.length >= 3)
            for (const model of models) {
                const card = findChild(host.view, "localModel-" + model.id)
                verify(card !== null, model.id)
                compare(findChild(card, "tier").visible, true)
                const size = findChild(card, "size").text
                if (model.installed === "installed")
                    verify(size.indexOf(Words.downloaded) === 0, size)
                else
                    verify(size.indexOf(Words.bytes(model.download_bytes)) >= 0, size)
                verify(findChild(card, "licence").text.indexOf(String(model.licence.summary)) >= 0)
            }
            const best = findChild(findChild(host.view, "localModel-" + screen().offer.recommended), "recommended")
            verify(best.visible, "the best fit is marked")
            const slow = models.filter(model => model.tier.tier === "slow")
            verify(slow.length > 0, "the recording has a model that is slow here")
            compare(findChild(findChild(host.view, "localModel-" + slow[0].id), "tier").children[0].children[1].text,
                    Words.tierLabel("slow"))
        }

        function test_a_restricting_licence_must_be_accepted() {
            host.reach(host.state("setup-licence"), this)
            const box = findChild(host.view, "acceptLicence")
            verify(box.visible)
            const go = findChild(host.view, "continueButton")
            verify(!go.enabled, "not before the restriction is accepted")
            mouseClick(box)
            verify(go.enabled)
            mouseClick(go)
            tryVerify(() => called("UseLocalModel").length === 1, 5000)
            const options = JSON.parse(called("UseLocalModel")[0].args[1])
            compare(options.accept_licence, true)
            verify(options.download_bytes > 0, "with the size the person was shown")
        }

        function test_download_progress_then_ready() {
            host.reach(host.state("setup-download"), this)
            const bar = findChild(host.view, "downloadProgress")
            verify(bar.visible)
            verify(!bar.indeterminate, "local AI gave real figures")
            fuzzyCompare(bar.value, 0.5, 0.01)
            verify(bar.detail.indexOf(Words.bytes(639446688)) >= 0, bar.detail)
            const options = JSON.parse(called("UseLocalModel")[0].args[1])
            compare(options.download_bytes, 639446688, "the size the person was shown went with the request")
            fixture.release()
            tryVerify(() => findChild(host.view, "continueButton") !== null && findChild(host.view, "continueButton").visible, 10000,
                      "the model is ready and chosen")
            tryVerify(() => host.client.status.provider === "local", 5000, "the provider is local AI")
            mouseClick(findChild(host.view, "continueButton"))
            tryVerify(() => host.view.step === "settings" && screen() !== null && !screen().loading, 10000)
            verify(findChild(host.view, "setting-temperature") !== null, "the local model takes a temperature")
            verify(findChild(host.view, "effort-none") !== null, "and an effort")
        }

        function test_download_cancel() {
            host.reach(host.state("setup-download-cancelled"), this)
            compare(called("CancelLocalModel").length, 1)
            verify(findChild(host.view, "downloadKept").visible, "what was downloaded is kept, and the screen says so")
            verify(!findChild(host.view, "downloadProgress").visible)
            verify(findChild(host.view, "backButton").visible)
        }

        function test_provider_terms_are_shown() {
            host.reach(host.state("setup-provider"), this)
            const deepseek = findChild(host.view, "provider-deepseek")
            verify(deepseek !== null)
            const reviewed = host.provider(host.view.catalogue, "deepseek")
            let texts = []
            const collect = item => {
                if (item.objectName === "termText")
                    texts.push(item.text)
                for (const child of item.children)
                    collect(child)
            }
            collect(deepseek)
            verify(texts.indexOf(reviewed.data_terms.stores_requests) >= 0, "what DeepSeek keeps, as TUR read it")
            verify(texts.indexOf(reviewed.data_terms.processing_location) >= 0)
            texts = []
            collect(findChild(host.view, "provider-anthropic"))
            compare(texts.length, 1)
            verify(texts[0].indexOf("Anthropic's terms are not reviewed by TUR") === 0, texts[0])
            texts = []
            collect(findChild(host.view, "provider-ollama"))
            verify(texts[0].indexOf("On this computer") === 0, "a service on this computer says so")
            verify(findChild(host.view, "provider-local") === null, "local AI has its own way in")
            verify(findChild(host.view, "continueButton").enabled, "a provider is chosen")
        }

        function test_the_key_goes_to_the_service_and_stays_nowhere_here() {
            host.reach(host.state("setup-key"), this)
            const field = findChild(host.view, "keyField")
            compare(field.echoMode, TextInput.Password, "the key is not shown while it is typed")
            const key = "sk-test-pax-ui-setup-0123456789"
            field.text = key
            mouseClick(findChild(host.view, "continueButton"))
            tryVerify(() => host.view.step === "settings", 10000, "on to the model's settings")
            verify(fixture.keyMatches(key), "the service has the key")
            compare(called("SetProvider")[0].args[0], "anthropic")
            const stored = called("SetKey")
            compare(stored.length, 1)
            compare(stored[0].args[1].key_length, key.length)
        }

        function test_a_key_the_service_refuses() {
            host.reach(host.state("setup-key"), this)
            findChild(host.view, "keyField").text = "two words"
            mouseClick(findChild(host.view, "continueButton"))
            tryVerify(() => screen().problem.length > 0, 5000, "the refusal is shown")
            compare(screen().problemCode, "TUR-AI-KEY-004")
            compare(findChild(host.view, "keyField").text, "", "the field does not keep what was typed")
            compare(host.view.step, "key")
        }

        function test_only_the_settings_the_model_accepts() {
            host.reach(host.state("setup-settings"), this)
            const accepts = screen().accepts
            for (const name of ["thinking_budget", "temperature", "top_p", "top_k", "max_output_tokens", "context_tokens"]) {
                const field = findChild(host.view, "setting-" + name)
                compare(field !== null, accepts[name] !== undefined, name)
            }
            compare(findChild(host.view, "effortChoices").visible, accepts.effort !== undefined)
        }

        function test_a_setting_out_of_range_is_caught_here() {
            host.reach(host.state("setup-settings-invalid"), this)
            const range = screen().accepts.temperature
            compare(findChild(host.view, "setting-temperature").errorText, Words.settingOutOfRange("temperature", range.min, range.max))
            compare(called("SetGenerationSettings").length, 0, "nothing was sent")
        }

        function test_the_service_refusal_of_a_setting_is_shown_in_its_words() {
            host.reach(host.state("setup-settings-refused"), this)
            compare(called("SetGenerationSettings").length, 1)
            compare(screen().problemCode, "TUR-AI-SERVICE-006")
            verify(screen().problem.indexOf("effort") >= 0, screen().problem)
            const shown = findChild(host.view, "screenProblem")
            verify(shown.visible)
            compare(host.view.step, "settings")
        }

        function test_limits_in_tokens_when_the_price_is_unknown() {
            host.reach(host.state("setup-limits"), this)
            const task = findChild(host.view, "taskLimit")
            const day = findChild(host.view, "dayLimit")
            tryVerify(() => task.text.length > 0 && day.text.length > 0, 5000, "the limits in effect are shown")
            compare(task.label, Words.limitTaskTokens)
            task.text = "0"
            mouseClick(findChild(host.view, "continueButton"))
            compare(task.errorText, Words.limitPositive)
            compare(called("SetLimits").length, 0)
            task.text = "300000"
            mouseClick(findChild(host.view, "continueButton"))
            tryVerify(() => host.view.step === "consent", 5000)
            const limits = JSON.parse(called("SetLimits")[0].args[0])
            compare(limits.task_tokens, 300000)
            verify(limits.day_tokens > 0)
        }

        function test_not_now_leaves_with_nothing_switched_on() {
            host.reach(host.state("setup-welcome"), this)
            const spy = createTemporaryObject(spyComponent, root, { target: host.view })
            mouseClick(findChild(host.view, "notNowButton"))
            compare(spy.count, 1)
            compare(called("Enable").length, 0)
        }

        Component {
            id: spyComponent

            SignalSpy {
                signalName: "finished"
            }
        }
    }
}

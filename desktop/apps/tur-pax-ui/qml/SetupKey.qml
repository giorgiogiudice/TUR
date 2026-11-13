// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, the key and the model (desktop-experience.md, 9.12, screen 4).
//
// The key goes to the service in one call (SetKey), which keeps it in the
// password store; the field is emptied as soon as the call is made, and this
// program writes it nowhere. A key already stored is said to be there and is
// never shown. The model is one of the provider's (ListProviderModels: TUR's
// list, marked "not tested with TUR" where it has not been), or a name typed
// by hand. "Check the key" is not built: it would send a request the
// provider bills (pax-service.md, section 13).
SetupScreen {
    id: screen

    required property AgentClient client
    // The provider's entry of ListModels.
    required property var entry
    property string chosenModel

    property bool keyStored: false
    property var listed: []
    property bool working: false

    readonly property string provider: String(screen.entry.id || "")
    readonly property bool keyNeeded: screen.entry.key_required !== false && screen.entry.key_source !== "runtime_file"
    readonly property string model: typedModel.text.trim().length > 0 ? typedModel.text.trim() : screen.chosenModel

    // The provider and model were chosen, and the key stored if one was typed.
    signal done(string model)
    signal backRequested()

    function apply(): void {
        if (screen.model.length === 0)
            return
        screen.clearProblem()
        screen.working = true
        const key = keyField.text
        // The field forgets the key at once; only the call below carries it.
        keyField.text = ""
        screen.client.call("SetProvider", [screen.provider, screen.model], function (reply) {
            if (!reply.ok) {
                screen.working = false
                screen.refused(reply)
                return
            }
            if (key.length === 0) {
                screen.working = false
                screen.done(screen.model)
                return
            }
            screen.client.call("SetKey", [screen.provider, key], function (stored) {
                screen.working = false
                if (!stored.ok) {
                    screen.refused(stored)
                    return
                }
                screen.keyStored = true
                screen.done(screen.model)
            })
        })
    }

    title: Words.keyTitle
    initialFocus: screen.keyNeeded ? keyField : continueButton

    Component.onCompleted: {
        if (screen.keyNeeded) {
            screen.client.call("HasKey", [screen.provider], function (reply) {
                screen.keyStored = reply.ok && reply.value === true
            })
        }
        screen.client.call("ListProviderModels", [screen.provider, false], function (reply) {
            if (!reply.ok) {
                screen.refused(reply)
                return
            }
            screen.listed = reply.value.models || []
            if (screen.chosenModel.length === 0 && screen.listed.length > 0)
                screen.chosenModel = String(screen.listed[0].model)
        })
    }

    Kit.Label {
        Layout.fillWidth: true
        text: screen.entry.name ? qsTr("Provider: %1").arg(screen.entry.name) : ""
        kind: Kit.Label.Strong
        wrapMode: Text.Wrap
    }

    Kit.TextField {
        id: keyField

        objectName: "keyField"
        Layout.fillWidth: true
        visible: screen.keyNeeded
        label: Words.keyField
        echoMode: showKey.checked ? TextInput.Normal : TextInput.Password
        helperText: screen.keyStored ? Words.keyStored + " " + Words.keyHelp : Words.keyHelp
        inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
    }

    Kit.CheckBox {
        id: showKey

        objectName: "showKey"
        visible: screen.keyNeeded
        text: Words.showKey
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: !screen.keyNeeded
        text: Words.keyNotNeeded
        tone: Kit.Label.Secondary
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.keyNeeded
        text: Words.checkKeyLater
        kind: Kit.Label.Small
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        text: Words.modelLabel
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Repeater {
        model: screen.listed

        delegate: ChoiceCard {
            id: card

            required property var modelData

            objectName: "model-" + card.modelData.model
            Layout.fillWidth: true
            title: String(card.modelData.label || card.modelData.model)
            subtitle: {
                const parts = [String(card.modelData.model)]
                if (card.modelData.tested_with_tur !== true)
                    parts.push(Words.notTested)
                if (card.modelData.price_known !== true)
                    parts.push(Words.priceUnknown)
                return parts.join(" · ")
            }
            selected: typedModel.text.trim().length === 0 && screen.chosenModel === card.modelData.model
            onClicked: {
                typedModel.text = ""
                screen.chosenModel = String(card.modelData.model)
            }

            Kit.Label {
                Layout.fillWidth: true
                visible: card.modelData.can_see === false
                text: qsTr("It takes no pictures, so P@x will not offer it the screen.")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }
        }
    }

    Kit.TextField {
        id: typedModel

        objectName: "typedModel"
        Layout.fillWidth: true
        label: Words.modelTyped
        helperText: Words.notTested
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
            enabled: !screen.working && screen.model.length > 0
                     && (!screen.keyNeeded || screen.keyStored || keyField.text.length > 0)
            onClicked: screen.apply()
        }
    }
}

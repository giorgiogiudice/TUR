// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, local AI: the models local AI offers for this computer
// (ListLocalModels; pax-service.md, 2.5): for each, how it will run here
// (runs well, slow, does not fit) with local AI's reasons, the download
// size or that it is already here, and its licence in plain words, with
// the restriction to accept where the licence restricts use. "Use this
// model" asks the service to make it ready (UseLocalModel) with the size the
// person was shown and, where needed, the acceptance of the licence.
SetupScreen {
    id: screen

    required property AgentClient client

    property var offer: ({})
    property string chosenId
    property bool loading: true

    readonly property var models: screen.offer.models || []
    readonly property var chosen: {
        for (const model of screen.models) {
            if (model.id === screen.chosenId)
                return model
        }
        return null
    }
    readonly property bool needsLicence: screen.chosen !== null && screen.chosen.licence !== undefined
                                         && screen.chosen.licence.restricts_use === true

    // The service is making `model` ready as `job`.
    signal started(var model, string job)
    signal backRequested()

    function choosable(model: var): bool {
        return model.usable_by_pax === true && (model.tier || {}).tier !== "does-not-fit"
    }

    function use(): void {
        const model = screen.chosen
        if (model === null)
            return
        const options = {}
        if (model.installed !== "installed")
            options.download_bytes = model.download_bytes
        if (screen.needsLicence)
            options.accept_licence = true
        screen.clearProblem()
        screen.client.call("UseLocalModel", [model.id, JSON.stringify(options)], function (reply) {
            if (reply.ok)
                screen.started(model, String(reply.value))
            else
                screen.refused(reply)
        })
    }

    title: Words.localTitle
    initialFocus: continueButton

    Component.onCompleted: {
        screen.client.call("ListLocalModels", [], function (reply) {
            screen.loading = false
            if (!reply.ok) {
                screen.refused(reply)
                return
            }
            screen.offer = reply.value
            if (screen.chosenId.length === 0 && reply.value.recommended)
                screen.chosenId = String(reply.value.recommended)
        })
    }

    Kit.Label {
        Layout.fillWidth: true
        text: String(screen.offer.message || "")
        visible: text.length > 0
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: Words.localNote
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.BusyIndicator {
        visible: screen.loading
        running: screen.loading
    }

    Repeater {
        model: screen.models

        delegate: ChoiceCard {
            id: card

            required property var modelData

            readonly property var tier: card.modelData.tier || {}

            objectName: "localModel-" + card.modelData.id
            Layout.fillWidth: true
            title: String(card.modelData.name || card.modelData.id)
            subtitle: String(card.tier.message || "")
            selected: screen.chosenId === card.modelData.id
            unavailable: !screen.choosable(card.modelData)
            onClicked: {
                if (screen.choosable(card.modelData))
                    screen.chosenId = card.modelData.id
            }

            Flow {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceRelated

                // The tier as a word and a sign, never colour alone.
                Rectangle {
                    objectName: "tier"
                    implicitWidth: tierRow.implicitWidth + 2 * Kit.Theme.spaceRelated
                    implicitHeight: tierRow.implicitHeight + Kit.Theme.spaceGrid
                    radius: Kit.Theme.radiusControl
                    color: Kit.Theme.surfaceRaised
                    border.width: Kit.Theme.lineWidth
                    border.color: card.tier.tier === "runs-well" ? Kit.Theme.positive
                                  : card.tier.tier === "slow" ? Kit.Theme.warning : Kit.Theme.negative

                    Row {
                        id: tierRow

                        anchors.centerIn: parent
                        spacing: Kit.Theme.spaceGrid

                        Kit.Glyph {
                            anchors.verticalCenter: parent.verticalCenter
                            size: 14
                            kind: card.tier.tier === "runs-well" ? Kit.Glyph.Positive
                                  : card.tier.tier === "slow" ? Kit.Glyph.Warning : Kit.Glyph.Negative
                            tint: card.tier.tier === "runs-well" ? Kit.Theme.positive
                                  : card.tier.tier === "slow" ? Kit.Theme.warning : Kit.Theme.negative
                        }

                        Kit.Label {
                            text: Words.tierLabel(String(card.tier.tier || ""))
                            kind: Kit.Label.Small
                            wrapMode: Text.NoWrap
                        }
                    }
                }

                Kit.Label {
                    id: best

                    objectName: "recommended"
                    width: Math.min(best.implicitWidth, card.availableWidth)
                    visible: screen.offer.recommended === card.modelData.id
                    text: Words.recommended
                    kind: Kit.Label.Small
                    tone: Kit.Label.Accent
                    wrapMode: Text.Wrap
                }
            }

            Kit.Label {
                objectName: "size"
                Layout.fillWidth: true
                text: Words.downloadSize(card.modelData) + " · " + String(card.modelData.publisher || "")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Kit.Label {
                objectName: "licence"
                Layout.fillWidth: true
                text: Words.licence(card.modelData)
                kind: Kit.Label.Small
                tone: (card.modelData.licence || {}).restricts_use ? Kit.Label.Warning : Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Repeater {
                model: card.tier.reasons || []

                delegate: Kit.Label {
                    id: reason

                    required property string modelData

                    Layout.fillWidth: true
                    text: reason.modelData
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    Kit.CheckBox {
        id: licenceBox

        objectName: "acceptLicence"
        Layout.fillWidth: true
        visible: screen.needsLicence
        text: Words.acceptLicence
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
            text: Words.useModel
            kind: Kit.Button.Primary
            enabled: screen.chosen !== null && screen.choosable(screen.chosen) && (!screen.needsLicence || licenceBox.checked)
            onClicked: screen.use()
        }
    }
}

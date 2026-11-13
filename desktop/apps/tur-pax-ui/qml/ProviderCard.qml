// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One provider of the model list, as a choice: its name, what it keeps of
// what P@x sends as TUR read its terms on the date it says, or that TUR has
// not reviewed its terms (pax-service.md, section 9; ADR-0004); and, under
// the choice, the links to its key page and its terms. Opening a link starts
// the browser; nothing is sent from here.
ColumnLayout {
    id: provider

    // The provider's entry of ListModels.
    required property var modelData

    readonly property var entry: provider.modelData
    readonly property var terms: Words.terms(provider.entry)
    property alias selected: card.selected

    signal clicked()

    objectName: "provider-" + provider.entry.id
    spacing: Kit.Theme.spaceGrid

    ChoiceCard {
        id: card

        objectName: "providerChoice"
        Layout.fillWidth: true
        title: String(provider.entry.name || provider.entry.id)
        subtitle: provider.entry.runs_locally ? qsTr("On this computer.")
                  : provider.entry.data_terms_reviewed ? qsTr("TUR read its terms.") : qsTr("TUR has not reviewed its terms.")
        onClicked: provider.clicked()

        GridLayout {
            objectName: "terms"
            Layout.fillWidth: true
            columns: 1
            rowSpacing: Kit.Theme.spaceGrid

            Repeater {
                model: provider.terms

                delegate: ColumnLayout {
                    id: term

                    required property var modelData

                    Layout.fillWidth: true
                    spacing: 0

                    Kit.Label {
                        Layout.fillWidth: true
                        text: term.modelData.label
                        kind: Kit.Label.Small
                        tone: Kit.Label.Secondary
                        wrapMode: Text.Wrap
                    }

                    Kit.Label {
                        objectName: "termText"
                        Layout.fillWidth: true
                        text: term.modelData.text
                        kind: Kit.Label.Small
                        wrapMode: Text.Wrap
                    }
                }
            }
        }
    }

    Flow {
        Layout.fillWidth: true
        Layout.leftMargin: Kit.Theme.spaceRelated
        visible: keyPage.visible || termsPage.visible
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            id: keyPage

            objectName: "keyPage"
            visible: String(provider.entry.key_url || "").length > 0
            text: Words.getKey
            kind: Kit.Button.Quiet
            Accessible.description: String(provider.entry.key_url || "")
            onClicked: Qt.openUrlExternally(provider.entry.key_url)
        }

        Kit.Button {
            id: termsPage

            objectName: "termsPage"
            visible: String(provider.entry.data_terms_url || "").length > 0
            text: Words.dataTerms
            kind: Kit.Button.Quiet
            Accessible.description: String(provider.entry.data_terms_url || "")
            onClicked: Qt.openUrlExternally(provider.entry.data_terms_url)
        }
    }
}

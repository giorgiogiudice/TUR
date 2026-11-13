// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Setup, a provider (desktop-experience.md, 9.12, screen 2): every provider
// of the model list (ListModels; pax-service.md, section 9), in alphabetical
// order with no recommendation, each with what it keeps of what P@x sends as
// TUR read its terms, or that TUR has not reviewed them. Services that run
// on this computer are listed apart. Local AI has its own way in, on the
// screen before.
SetupScreen {
    id: screen

    // ListModels' answer.
    required property var catalogue
    property string chosenId

    readonly property var providers: (screen.catalogue.providers || []).filter(entry => entry.id !== "local")
    readonly property var companies: screen.providers.filter(entry => !entry.runs_locally)
                                         .sort((a, b) => String(a.name).localeCompare(String(b.name)))
    readonly property var programs: screen.providers.filter(entry => entry.runs_locally === true)
                                        .sort((a, b) => String(a.name).localeCompare(String(b.name)))
    readonly property var chosen: {
        for (const entry of screen.providers) {
            if (entry.id === screen.chosenId)
                return entry
        }
        return null
    }

    signal chosenProvider(var entry)
    signal backRequested()

    title: Words.providerTitle
    initialFocus: continueButton

    Kit.Label {
        Layout.fillWidth: true
        text: Words.providerNote
        tone: Kit.Label.Secondary
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.companies.length > 0
        text: Words.companies
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Repeater {
        model: screen.companies

        delegate: ProviderCard {
            id: card

            Layout.fillWidth: true
            selected: screen.chosenId === card.entry.id
            onClicked: screen.chosenId = card.entry.id
        }
    }

    Kit.Label {
        Layout.fillWidth: true
        visible: screen.programs.length > 0
        text: Words.onThisComputer
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Repeater {
        model: screen.programs

        delegate: ProviderCard {
            id: program

            Layout.fillWidth: true
            selected: screen.chosenId === program.entry.id
            onClicked: screen.chosenId = program.entry.id
        }
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
            enabled: screen.chosen !== null
            onClicked: screen.chosenProvider(screen.chosen)
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What an application's "Details" hold in part 1 (desktop-experience.md,
// section 8.4): the publisher's website, and "Where each fact was checked",
// the entry's references with their dates, so that anyone can see what a
// claim rests on.
//
// Editions, the licence, what the application can reach and where its data
// is are not here: the first two have no words in the design's vocabulary
// yet, and the last two need the application to be installed (D-11).
ColumnLayout {
    id: facts

    // The catalogue's entry.
    required property var entry

    // Leave out the publisher's website: for the "no route" page, whose
    // last row is the evidence alone.
    property bool referencesOnly: false

    // Opens a page in the browser.
    signal urlRequested(string url)

    // Every reference of every edition, once each, in the catalogue's order.
    readonly property var references: {
        const out = []
        const seen = {}
        const editions = facts.entry && facts.entry.editions ? facts.entry.editions : []
        for (const edition of editions) {
            for (const reference of edition.references) {
                const key = reference.url + "\n" + reference.title
                if (seen[key])
                    continue
                seen[key] = true
                out.push(reference)
            }
        }
        return out
    }

    readonly property string homepage: facts.entry && facts.entry.homepage ? facts.entry.homepage : ""

    spacing: Kit.Theme.spaceGroup

    ColumnLayout {
        objectName: "factsWebsite"
        Layout.fillWidth: true
        visible: !facts.referencesOnly && facts.homepage.length > 0
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("Publisher's website")
            kind: Kit.Label.Strong
        }

        LinkButton {
            objectName: "factsHomepage"
            Layout.fillWidth: true
            url: facts.homepage
            onClicked: facts.urlRequested(facts.homepage)
        }
    }

    ColumnLayout {
        objectName: "factsReferences"
        Layout.fillWidth: true
        visible: facts.references.length > 0
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            visible: !facts.referencesOnly
            text: qsTr("Where each fact was checked")
            kind: Kit.Label.Strong
        }

        Repeater {
            model: facts.references

            ColumnLayout {
                id: reference

                required property var modelData

                Layout.fillWidth: true
                spacing: 0

                Kit.Label {
                    objectName: "referenceTitle"
                    Layout.fillWidth: true
                    text: reference.modelData.title
                    paragraph: true
                }

                Kit.Label {
                    objectName: "referenceChecked"
                    Layout.fillWidth: true
                    //: Under a reference. %1 is the date the page was last checked.
                    text: qsTr("Checked %1").arg(Words.longDate(reference.modelData.checked))
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                }

                LinkButton {
                    Layout.fillWidth: true
                    url: reference.modelData.url
                    onClicked: facts.urlRequested(reference.modelData.url)
                }
            }
        }
    }
}

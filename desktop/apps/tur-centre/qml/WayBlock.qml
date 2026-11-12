// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One way of running an application: one option of the router, in the fixed
// order of desktop-experience.md, section 8.4: the route, its one-line
// explanation, the rating with its source, what is needed first, then the
// caveats. The same block serves the "How it will run" box, the list of other
// ways, and the ways that were considered when there is no route.
//
// The label, the explanation, the rating's words and "according to whom" are
// the fixed vocabulary (Words). The reason, every caveat and every
// prerequisite are the router's text, shown as it is.
ColumnLayout {
    id: block

    // The router's option, as it is in `tur-catalogue route --json`.
    required property var option

    // The catalogue's entry, for the edition the option runs.
    property var entry: null

    // Every record the catalogue holds for this way of running it.
    property var reports: []

    // The block of the "How it will run" box: it carries the headings
    // "Needed first" and "Before you install", and says so when nothing is
    // known to stand in the way.
    property bool leading: false

    readonly property var edition: Words.editionOf(block.option, block.entry)
    readonly property string label: Words.routeLabel(block.option.route,
                                                     block.edition && block.edition.distribution
                                                     ? block.edition.distribution : "")
    readonly property string outlookWords: Words.outlookLabel(block.option.outlook)
    readonly property bool blocked: block.option.outlook === "blocked"
    readonly property var caveats: Words.orderedCaveats(block.option)

    // Notes go under "More" when there are more than four caveats.
    readonly property bool foldsNotes: block.caveats.length > 4
    readonly property var shownCaveats: block.foldsNotes
                                        ? block.caveats.filter(caveat => caveat.severity !== "note") : block.caveats
    readonly property var foldedCaveats: block.foldsNotes
                                         ? block.caveats.filter(caveat => caveat.severity === "note") : []

    spacing: Kit.Theme.spaceRelated

    // The route, and whether it is the one TUR would use.
    RowLayout {
        Layout.fillWidth: true
        spacing: Kit.Theme.spaceGroup

        Kit.Label {
            objectName: "wayLabel"
            Layout.fillWidth: true
            text: block.label
            kind: block.leading ? Kit.Label.Heading : Kit.Label.Strong
            // A way that cannot be used is set back, and stays readable: the
            // reason it cannot be used is what the person came to read.
            tone: block.blocked ? Kit.Label.Secondary : Kit.Label.Primary
        }

        Kit.Label {
            objectName: "wayRecommended"
            Layout.alignment: Qt.AlignTop
            visible: block.option.recommended === true && block.option.route !== "no-route"
            text: qsTr("Recommended")
            tone: Kit.Label.Secondary
            wrapMode: Text.NoWrap
        }
    }

    // "May not work", or "Cannot be used on this computer".
    Kit.Label {
        objectName: "wayOutlook"
        Layout.fillWidth: true
        visible: block.outlookWords.length > 0
        text: block.outlookWords
        kind: Kit.Label.Strong
        tone: block.blocked ? Kit.Label.Negative : Kit.Label.Warning
    }

    Kit.Label {
        objectName: "wayExplanation"
        Layout.fillWidth: true
        visible: text.length > 0
        text: Words.routeExplanation(block.option.route)
        paragraph: true
    }

    // Why the router lists this way: its own sentence.
    Kit.Label {
        objectName: "wayReason"
        Layout.fillWidth: true
        visible: text.length > 0
        text: block.option.reason ?? ""
        tone: Kit.Label.Secondary
        paragraph: true
    }

    // How well it works, and according to whom. Never one without the other.
    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceGrid
        spacing: Kit.Theme.spaceGrid / 2

        RatingLine {
            objectName: "wayRating"
            Layout.fillWidth: true
            rating: block.option.rating
        }

        Kit.Label {
            objectName: "waySource"
            Layout.fillWidth: true
            text: Words.sourceSentence(block.option.rating_source)
            tone: Kit.Label.Secondary
            paragraph: true
        }

        // TUR's own test names its report. A path in the project's
        // repository: shown as it is, since nothing here can open it.
        Kit.Label {
            objectName: "wayTestReport"
            Layout.fillWidth: true
            visible: text.length > 0
            text: block.option.rating_source.kind === "tur-test" ? block.option.rating_source.report : ""
            kind: Kit.Label.Mono
            tone: Kit.Label.Secondary
            wrapMode: Text.WrapAnywhere
        }
    }

    // Every record, when the catalogue holds more than one for this way.
    ColumnLayout {
        Layout.fillWidth: true
        visible: block.reports.length > 1
        spacing: Kit.Theme.spaceRelated

        Kit.Disclosure {
            id: allReports

            objectName: "wayAllReports"
            //: %1 is how many reports there are; always more than one.
            text: qsTr("All reports (%1)").arg(block.reports.length)
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kit.Theme.spaceGroup
            visible: allReports.expanded
            spacing: Kit.Theme.spaceRelated

            Repeater {
                model: allReports.expanded ? block.reports : []

                ColumnLayout {
                    id: report

                    required property var modelData

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceGrid / 2

                    RatingLine {
                        Layout.fillWidth: true
                        rating: report.modelData.rating
                    }

                    Kit.Label {
                        objectName: "reportSource"
                        Layout.fillWidth: true
                        text: Words.sourceSentence(Words.recordSource(report.modelData))
                        tone: Kit.Label.Secondary
                        paragraph: true
                    }
                }
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("A source's rating is evidence from its contributors' machines, not a promise.")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
                paragraph: true
            }
        }
    }

    // What must be installed, switched on or supplied first.
    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceGrid
        visible: block.option.prerequisites.length > 0
        spacing: Kit.Theme.spaceGrid

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("Needed first")
            kind: Kit.Label.Strong
        }

        Repeater {
            model: block.option.prerequisites

            RowLayout {
                id: prerequisite

                required property var modelData

                Layout.fillWidth: true
                spacing: Kit.Theme.spaceRelated

                Kit.Label {
                    Layout.alignment: Qt.AlignTop
                    Layout.topMargin: needed.halfLeading
                    text: "•"
                    tone: Kit.Label.Secondary
                    Accessible.ignored: true
                }

                Kit.Label {
                    id: needed

                    objectName: "wayPrerequisite"
                    Layout.fillWidth: true
                    text: prerequisite.modelData.text
                    paragraph: true
                }
            }
        }
    }

    // What the person must see before choosing: blockers first.
    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceGrid
        visible: block.leading || block.caveats.length > 0
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            visible: block.leading
            text: qsTr("Before you install")
            kind: Kit.Label.Strong
        }

        Kit.Label {
            objectName: "wayNothingKnown"
            Layout.fillWidth: true
            visible: block.leading && block.caveats.length === 0
            text: qsTr("Nothing known stands in the way.")
            paragraph: true
        }

        Repeater {
            model: block.shownCaveats

            CaveatRow {
                required property var modelData

                Layout.fillWidth: true
                caveat: modelData
            }
        }

        Kit.Disclosure {
            id: more

            objectName: "wayMore"
            visible: block.foldedCaveats.length > 0
            text: qsTr("More")
        }

        Repeater {
            model: more.expanded ? block.foldedCaveats : []

            CaveatRow {
                required property var modelData

                Layout.fillWidth: true
                caveat: modelData
            }
        }
    }
}

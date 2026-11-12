// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One application found by a search (desktop-experience.md, section 8.3).
// One row for each application, whatever editions it has. The row shows the
// route the router recommends for this machine and that route's rating; the
// other ways are on the application's page. An application that cannot run
// is listed like any other, and says so.
//
// The name and the publisher are there as soon as the search answers. The
// summary, the route, the rating and what is needed first arrive with the
// catalogue's entry and the router's answer, and fill the row in.
Item {
    id: row

    required property string applicationId
    required property string name
    required property string publisher
    // False until the entry and the router's answer have arrived.
    required property bool detailed
    // The catalogue's entry, and the option the router recommends.
    required property var entry
    required property var recommended

    readonly property string summary: row.detailed && row.entry.summary ? row.entry.summary : ""
    readonly property bool noRoute: row.detailed && row.recommended.route === "no-route"
    readonly property var edition: row.detailed ? Words.editionOf(row.recommended, row.entry) : null
    readonly property var prerequisites: row.detailed && row.recommended.prerequisites
                                         ? row.recommended.prerequisites : []

    signal opened(string applicationId)

    implicitWidth: line.implicitWidth
    implicitHeight: line.implicitHeight + 2 * Kit.Theme.spaceRelated

    Accessible.role: Accessible.ListItem
    Accessible.name: row.name

    // The pointer may take the whole row; the keyboard has the button.
    TapHandler {
        onTapped: row.opened(row.applicationId)
    }

    RowLayout {
        id: line

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Kit.Theme.spaceGroup

        AppTile {
            Layout.alignment: Qt.AlignTop
            name: row.name
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            spacing: Kit.Theme.spaceGrid

            Kit.Label {
                objectName: "rowName"
                Layout.fillWidth: true
                text: row.name
                kind: Kit.Label.Strong
            }

            Kit.Label {
                objectName: "rowPublisher"
                Layout.fillWidth: true
                //: A search result's second line. %1 is the publisher, %2 one line about the application.
                text: row.summary.length > 0 ? qsTr("%1 · %2").arg(row.publisher).arg(row.summary) : row.publisher
                tone: Kit.Label.Secondary
                paragraph: true
            }

            // How it will run, and how well that is known to work.
            Kit.Label {
                objectName: "rowRoute"
                Layout.fillWidth: true
                visible: row.detailed && !row.noRoute
                text: row.detailed ? Words.routeLabel(row.recommended.route,
                                                      row.edition && row.edition.distribution
                                                      ? row.edition.distribution : "") : ""
            }

            RatingLine {
                objectName: "rowRating"
                Layout.fillWidth: true
                visible: row.detailed && !row.noRoute
                rating: row.detailed ? row.recommended.rating : "unknown"
                strong: false
            }

            // Cannot run on TUR: the page says why and what to do instead.
            RowLayout {
                objectName: "rowNoRoute"
                Layout.fillWidth: true
                visible: row.noRoute
                spacing: Kit.Theme.spaceRelated

                Kit.Glyph {
                    Layout.alignment: Qt.AlignTop
                    Layout.topMargin: 1
                    kind: Kit.Glyph.Negative
                    tint: Kit.Theme.negative
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: Words.routeLabel("no-route", "")
                    kind: Kit.Label.Strong
                }
            }

            // The router's own sentences for what must be there first.
            ColumnLayout {
                objectName: "rowNeeded"
                Layout.fillWidth: true
                visible: row.prerequisites.length > 0 && !row.noRoute
                spacing: 0

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("Needed first")
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                }

                Repeater {
                    model: row.prerequisites

                    Kit.Label {
                        required property var modelData

                        Layout.fillWidth: true
                        text: modelData.text
                        kind: Kit.Label.Small
                        paragraph: true
                    }
                }
            }
        }

        Kit.Button {
            objectName: "rowDetails"
            Layout.alignment: Qt.AlignTop
            text: qsTr("Details")
            //: Read by a screen reader for the Details button of a search result. %1 is the application's name.
            Accessible.name: qsTr("Details of %1").arg(row.name)
            onClicked: row.opened(row.applicationId)
        }
    }
}

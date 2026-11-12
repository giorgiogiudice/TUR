// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Search (desktop-experience.md, section 8.3). The field is in the window's
// header; this is what it found. The catalogue is on the machine, so the
// search is the catalogue's own: it forgives typing mistakes and knows other
// names, and the order of the rows is the catalogue's.
//
// States: nothing asked; asked and not answered yet; results; nothing found;
// the catalogue could not be asked. Stores are not searched yet (D-11), so
// nothing here speaks of them.
ColumnLayout {
    id: view

    required property SearchSession search

    // Opens an application's page.
    signal applicationRequested(string applicationId)
    // Opens a page in the browser. Nothing is sent by the application
    // centre itself.
    signal urlRequested(string url)

    // Where the project takes requests for applications it does not know.
    readonly property string issuePage: "https://github.com/giorgiogiudice/TUR/issues"

    readonly property int count: view.search.hits.count
    readonly property bool waitingForFirstAnswer: view.search.status === SearchSession.Searching && view.count === 0

    spacing: Kit.Theme.spaceGroup

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Search")
        kind: Kit.Label.Title
    }

    Kit.BusyIndicator {
        objectName: "searchBusy"
        visible: view.waitingForFirstAnswer
        running: view.waitingForFirstAnswer
        label: qsTr("Searching the catalogue")
    }

    // The catalogue found something. While a newer search is under way the
    // rows of the last answer stay, under the words they answer.
    ColumnLayout {
        objectName: "searchResults"
        Layout.fillWidth: true
        visible: view.count > 0
                 && (view.search.status === SearchSession.Results || view.search.status === SearchSession.Searching)
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            objectName: "searchCount"
            Layout.fillWidth: true
            text: view.count === 1
                  //: %1 is what the person searched for.
                  ? qsTr("1 application in TUR's catalogue for \"%1\"").arg(view.search.answeredQuery)
                  //: %1 is a number greater than one, %2 what the person searched for.
                  : qsTr("%1 applications in TUR's catalogue for \"%2\"").arg(view.count).arg(view.search.answeredQuery)
            tone: Kit.Label.Secondary
        }

        Kit.Card {
            Layout.fillWidth: true
            padding: Kit.Theme.spaceGroup

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Repeater {
                    model: view.search.hits

                    ColumnLayout {
                        id: slot

                        required property int index
                        required property string applicationId
                        required property string name
                        required property string publisher
                        required property bool detailed
                        required property var entry
                        required property var recommended

                        Layout.fillWidth: true
                        spacing: 0

                        Kit.Separator {
                            Layout.fillWidth: true
                            visible: slot.index > 0
                        }

                        ResultRow {
                            Layout.fillWidth: true
                            applicationId: slot.applicationId
                            name: slot.name
                            publisher: slot.publisher
                            detailed: slot.detailed
                            entry: slot.entry
                            recommended: slot.recommended
                            onOpened: applicationId => view.applicationRequested(applicationId)
                        }
                    }
                }
            }
        }
    }

    // Nothing found. "Open a file" is not offered until files are built
    // (D-12): TUR cannot look at a file yet, and does not say it can.
    ColumnLayout {
        objectName: "searchNothing"
        Layout.fillWidth: true
        visible: view.search.status === SearchSession.NothingFound
        spacing: Kit.Theme.spaceGroup

        Kit.Label {
            objectName: "searchNothingTitle"
            Layout.fillWidth: true
            //: %1 is what the person searched for.
            text: qsTr("Nothing found for \"%1\".").arg(view.search.answeredQuery)
            kind: Kit.Label.Heading
        }

        Kit.Button {
            objectName: "searchTellProject"
            text: qsTr("Tell the project you need it")
            onClicked: view.urlRequested(view.issuePage)
        }
    }

    Loader {
        objectName: "searchFailure"
        Layout.fillWidth: true
        active: view.search.status === SearchSession.Failed
        visible: active

        sourceComponent: FailureMessage {
            failure: view.search.failure
            onRetryRequested: view.search.retry()
        }
    }
}

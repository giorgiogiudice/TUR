// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Home: a short, honest front page (desktop-experience.md, section 8.1).
//
// Its standing sentence stands "for as long as it is true": while the
// catalogue holds no rating from TUR's own tests (`tur-catalogue validate
// --json`, rated_by_tur_tests = 0). While that is not known, nothing is
// said about it.
//
// For people coming from Windows, Google Chrome is offered with one action
// and with where it comes from said plainly (17.1, answer 4). It is offered,
// not included: installing it is the person's act, from Flathub, which
// downloads Google's own package.
//
// "What needs attention" and "Bring an application you already have" wait
// for the update service and for files (D-12).
ColumnLayout {
    id: view

    property Catalogue catalogue: null
    property Apps apps: null

    // Install Chrome: its page, with the plan.
    signal installRequested(string applicationId)
    signal applicationRequested(string applicationId)

    readonly property bool turHasRatedNothing: view.catalogue !== null
                                               && view.catalogue.statistics.rated_by_tur_tests === 0
    readonly property var chromeInstalled: view.apps !== null && view.apps.installed.length >= 0
                                           ? view.apps.installedEntry(Words.chromeId) : ({})

    spacing: Kit.Theme.spaceGroup

    Component.onCompleted: {
        if (view.catalogue !== null)
            view.catalogue.checkStatistics()
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Home")
        kind: Kit.Label.Title
    }

    Kit.Banner {
        objectName: "homeStandingSentence"
        Layout.fillWidth: true
        visible: view.turHasRatedNothing
        severity: Kit.Banner.Info
        text: qsTr("TUR has not tested applications itself yet. Every rating here is what the named source reports.")
    }

    Kit.Card {
        objectName: "homeChrome"
        Layout.fillWidth: true
        visible: view.apps !== null

        RowLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceGroup

            AppTile {
                Layout.alignment: Qt.AlignTop
                name: "Google Chrome"
                size: 48
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceGrid

                Kit.Label {
                    Layout.fillWidth: true
                    text: "Google Chrome"
                    kind: Kit.Label.Heading
                }

                Kit.Label {
                    objectName: "homeChromeOffer"
                    Layout.fillWidth: true
                    text: Words.chromeOffer
                    paragraph: true
                }

                Kit.Label {
                    objectName: "homeChromeOrigin"
                    Layout.fillWidth: true
                    text: Words.chromeOrigin
                    kind: Kit.Label.Small
                    tone: Kit.Label.Secondary
                    paragraph: true
                }

                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: Kit.Theme.spaceGrid
                    spacing: Kit.Theme.spaceRelated

                    Kit.Button {
                        objectName: "homeChromeInstall"
                        text: view.chromeInstalled.state === "installed" ? qsTr("Open Google Chrome")
                                                                          : qsTr("Install Google Chrome…")
                        kind: Kit.Button.Primary
                        onClicked: {
                            if (view.chromeInstalled.state === "installed")
                                view.apps.launch(Words.chromeId)
                            else
                                view.installRequested(Words.chromeId)
                        }
                    }

                    Kit.Button {
                        objectName: "homeChromeDetails"
                        text: qsTr("How it will run")
                        kind: Kit.Button.Quiet
                        onClicked: view.applicationRequested(Words.chromeId)
                    }
                }
            }
        }
    }
}

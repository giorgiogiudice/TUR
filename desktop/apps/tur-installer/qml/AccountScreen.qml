// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 4: your account. The design creates the account at the first start
// of the installed system, in first-run setup (desktop-experience.md, 6.2):
// that is where the language, keyboard, time zone, network and account are
// asked, by a program that runs with the rights to create a user. The
// installer asks for nothing here and says what will be asked.
WizardScreen {
    id: screen

    signal backRequested()
    signal continueRequested()

    initialFocus: continueButton

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Your account")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Nothing about you is needed now. Your account is created when TUR starts for the first time from the disk you chose.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Card {
        Layout.fillWidth: true

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceRelated

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("At the first start, TUR asks for")
                kind: Kit.Label.Heading
            }

            Repeater {
                model: [
                    qsTr("your language and keyboard"),
                    qsTr("a network, if you want one; TUR works without"),
                    qsTr("where you are, for the clock"),
                    qsTr("your name, a user name and a password"),
                ]

                RowLayout {
                    id: entry

                    required property string modelData

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceRelated

                    Kit.Glyph {
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 2
                        kind: Kit.Glyph.ChevronForward
                        tint: Kit.Theme.textSecondary
                        size: 14
                    }

                    Kit.Label {
                        Layout.fillWidth: true
                        text: entry.modelData
                        wrapMode: Text.Wrap
                    }
                }
            }

            Kit.Label {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                text: qsTr("The first account can change the system. TUR asks for its password before anything that affects disks, start-up or security.")
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }
        }
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "backButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Back")
            onClicked: screen.backRequested()
        }

        Item {
            Layout.fillWidth: true
        }

        Kit.Button {
            id: continueButton

            objectName: "continueButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Continue")
            kind: Kit.Button.Primary
            Keys.onReturnPressed: continueButton.clicked()
            Keys.onEnterPressed: continueButton.clicked()
            onClicked: screen.continueRequested()
        }
    }
}

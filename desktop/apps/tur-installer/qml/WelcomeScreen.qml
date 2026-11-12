// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// Screen 1: welcome. Language, keyboard, and what will happen
// (desktop-experience.md, 11.5).
WizardScreen {
    id: screen

    required property KeyboardLayouts keyboard

    signal continueRequested()
    signal tryRequested()

    initialFocus: continueButton

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Install TUR on this computer")
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("This installer can erase a disk. It will show you exactly what it is going to do, and it will not change anything until you confirm on the last screen before installing.")
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("You are using TUR from the installation medium. Nothing you do here is kept when the computer is switched off. To look around first, close this window: the icon on the desktop opens it again.")
        tone: Kit.Label.Secondary
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
                text: qsTr("Language")
                kind: Kit.Label.Heading
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("English. TUR's own programs are in English for now. The installed system asks for its language when it starts for the first time.")
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Kit.Label {
                Layout.fillWidth: true
                Layout.topMargin: Kit.Theme.spaceRelated
                text: qsTr("Keyboard layout")
                kind: Kit.Label.Heading
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Choose the layout that matches the keys in front of you. It is used here, for the confirmation you will type.")
                tone: Kit.Label.Secondary
                wrapMode: Text.Wrap
            }

            Controls.ComboBox {
                id: layoutBox

                objectName: "layoutCombo"
                Layout.fillWidth: true
                Layout.maximumWidth: 440
                model: screen.keyboard.layouts
                textRole: "name"
                valueRole: "code"
                currentIndex: screen.keyboard.indexOf(screen.keyboard.current)
                Accessible.name: qsTr("Keyboard layout")
                onActivated: index => {
                    const code = screen.keyboard.layouts[index].code
                    if (!screen.keyboard.apply(code))
                        Transcript.write("keyboard layout=" + code + " applied=no")
                }
            }

            Kit.TextField {
                objectName: "keyboardTry"
                Layout.fillWidth: true
                Layout.maximumWidth: 440
                label: qsTr("Try your keyboard here")
            }
        }
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceRelated
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "tryButton"
            Layout.minimumWidth: implicitWidth
            text: qsTr("Try TUR first")
            kind: Kit.Button.Quiet
            onClicked: screen.tryRequested()
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

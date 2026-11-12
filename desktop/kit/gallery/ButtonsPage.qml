// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// Every kind of button in every state.
GalleryPage {
    title: qsTr("Buttons")
    focusTarget: focused

    Specimen {
        caption: qsTr("Kinds: primary, secondary, quiet, destructive")

        Kit.Button {
            text: qsTr("Install")
            kind: Kit.Button.Primary
        }

        Kit.Button {
            text: qsTr("Back")
        }

        Kit.Button {
            text: qsTr("Learn more")
            kind: Kit.Button.Quiet
        }

        Kit.Button {
            text: qsTr("Erase disk and install")
            kind: Kit.Button.Destructive
        }
    }

    Specimen {
        caption: qsTr("Pressed")

        Kit.Button {
            text: qsTr("Install")
            kind: Kit.Button.Primary
            down: true
        }

        Kit.Button {
            text: qsTr("Back")
            down: true
        }

        Kit.Button {
            text: qsTr("Learn more")
            kind: Kit.Button.Quiet
            down: true
        }

        Kit.Button {
            text: qsTr("Erase disk and install")
            kind: Kit.Button.Destructive
            down: true
        }
    }

    Specimen {
        caption: qsTr("Keyboard focus")

        Kit.Button {
            id: focused

            text: qsTr("Create a support bundle")
        }

        Kit.Button {
            text: qsTr("Try again")
            kind: Kit.Button.Primary
        }
    }

    Specimen {
        caption: qsTr("Unavailable")

        Kit.Button {
            text: qsTr("Install")
            kind: Kit.Button.Primary
            enabled: false
        }

        Kit.Button {
            text: qsTr("Back")
            enabled: false
        }

        Kit.Button {
            text: qsTr("Learn more")
            kind: Kit.Button.Quiet
            enabled: false
        }

        Kit.Button {
            text: qsTr("Erase disk and install")
            kind: Kit.Button.Destructive
            enabled: false
        }
    }
}

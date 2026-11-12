// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// Text fields, check boxes and switches in every state.
GalleryPage {
    title: qsTr("Fields and choices")
    focusTarget: focused

    Specimen {
        caption: qsTr("Text field: empty, filled, with keyboard focus")

        Kit.TextField {
            label: qsTr("Computer name")
            placeholderText: qsTr("For example, study")
            helperText: qsTr("Other devices on your network see this name.")
        }

        Kit.TextField {
            label: qsTr("Your name")
            text: "Ada Lovelace"
        }

        Kit.TextField {
            id: focused

            label: qsTr("User name")
            text: "ada"
        }
    }

    Specimen {
        caption: qsTr("Text field: wrong, and unavailable")

        Kit.TextField {
            label: qsTr("Security phrase")
            text: "amber lantern"
            errorText: qsTr("That is not the phrase shown above.")
        }

        Kit.TextField {
            label: qsTr("Computer name")
            text: "study"
            enabled: false
        }

        Kit.TextField {
            label: qsTr("Search")
            placeholderText: qsTr("Applications, settings, files")
            enabled: false
        }
    }

    Specimen {
        caption: qsTr("Check box")

        Kit.CheckBox {
            text: qsTr("Send a report to the developers")
        }

        Kit.CheckBox {
            text: qsTr("Encrypt this disk")
            checked: true
        }

        Kit.CheckBox {
            text: qsTr("Start when I sign in")
            enabled: false
        }

        Kit.CheckBox {
            text: qsTr("Keep my files")
            checked: true
            enabled: false
        }
    }

    Specimen {
        caption: qsTr("Switch")

        Kit.Switch {
            text: qsTr("Night light")
        }

        Kit.Switch {
            text: qsTr("Automatic updates")
            checked: true
        }

        Kit.Switch {
            text: qsTr("Private mode")
            enabled: false
        }

        Kit.Switch {
            text: qsTr("Bluetooth")
            checked: true
            enabled: false
        }
    }
}

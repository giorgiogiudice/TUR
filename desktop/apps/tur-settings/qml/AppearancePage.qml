// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// Appearance (desktop-experience.md, 10.2): dark, light, or light by day and
// dark at night; everything else links to KDE's appearance pages.
//
// What the desktop is set to can be read, and is shown. Changing it from
// here means switching between TUR's two global themes, which are not part
// of the system yet (task D-04). So the choice is drawn, cannot be used, and
// says why; no option is marked, because which of the three applies cannot
// be read either. Nothing on this page changes the system: the buttons open
// KDE System Settings at the page that does.
SettingsPage {
    id: view

    address: "tur.appearance"

    SettingsGroup {
        page: view.address
        section: "variant"

        SettingRow {
            address: "tur.appearance#current"
            value: Kit.SystemAppearance.prefersLight ? qsTr("Light") : qsTr("Dark")
            // The window can be started in a variant of its own choosing
            // (--variant). Then it does not show what the desktop is set to,
            // and says so.
            detail: Kit.Theme.light === Kit.SystemAppearance.prefersLight ? ""
                    : Kit.Theme.light ? qsTr("This window was started in the light variant and does not follow the desktop.")
                                      : qsTr("This window was started in the dark variant and does not follow the desktop.")
        }

        SettingRow {
            address: "tur.appearance#variant"
            available: false
            unavailableReason: qsTr("TUR cannot change this yet: its dark and light themes are not part of the system. Until they are, choose a global theme or colours in System Settings, below.")

            ChoiceOption {
                objectName: "variantDark"
                text: qsTr("Dark")
                enabled: false
            }

            ChoiceOption {
                objectName: "variantLight"
                text: qsTr("Light")
                enabled: false
            }

            ChoiceOption {
                objectName: "variantByDay"
                text: qsTr("Light by day, dark at night")
                enabled: false
            }
        }
    }

    SettingsGroup {
        page: view.address
        section: "kde"

        LinkRow {
            address: "tur.appearance#global-theme"
            detail: qsTr("Colours, window borders, icons and the rest, changed together.")
            onFollowed: target => view.addressRequested(target)
        }

        LinkRow {
            address: "tur.appearance#colours"
            detail: qsTr("The colour scheme and the accent colour.")
            onFollowed: target => view.addressRequested(target)
        }

        LinkRow {
            address: "tur.appearance#fonts"
            detail: qsTr("The typefaces and the size of text.")
            onFollowed: target => view.addressRequested(target)
        }

        LinkRow {
            address: "tur.appearance#night-light"
            detail: qsTr("Warmer colours in the evening, and when they begin.")
            onFollowed: target => view.addressRequested(target)
        }
    }
}

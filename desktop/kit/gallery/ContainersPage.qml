// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Cards, list rows, banners and the two indicators of activity.
GalleryPage {
    title: qsTr("Cards, lists, banners and progress")
    focusTarget: focused

    Specimen {
        caption: qsTr("Card")

        Kit.Card {
            width: parent.width

            ColumnLayout {
                anchors.fill: parent
                spacing: Kit.Theme.spaceRelated

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("This computer")
                    kind: Kit.Label.Heading
                }

                Kit.Label {
                    Layout.fillWidth: true
                    text: qsTr("TUR 0.3.2, installed on 4 November. The system is up to date and the last start was normal.")
                    tone: Kit.Label.Secondary
                    paragraph: true
                }
            }
        }
    }

    Specimen {
        caption: qsTr("List rows: plain, with a second line, with keyboard focus, selected, unavailable")

        Kit.Card {
            width: parent.width
            padding: Kit.Theme.spaceGrid

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Kit.ListRow {
                    Layout.fillWidth: true
                    text: qsTr("Appearance")
                }

                Kit.ListRow {
                    Layout.fillWidth: true
                    text: qsTr("Updates and recovery")
                    subtitle: qsTr("Up to date. Last checked today at 09:12.")
                }

                Kit.ListRow {
                    id: focused

                    Layout.fillWidth: true
                    text: qsTr("Storage")
                    subtitle: qsTr("Two disks. One needs attention.")
                }

                Kit.ListRow {
                    Layout.fillWidth: true
                    text: qsTr("Devices")
                    subtitle: qsTr("Everything works.")
                    selected: true
                }

                Kit.Separator {
                    Layout.fillWidth: true
                    Layout.margins: Kit.Theme.spaceGrid
                }

                Kit.ListRow {
                    Layout.fillWidth: true
                    text: qsTr("Windows applications")
                    subtitle: qsTr("Not set up.")
                    enabled: false
                }
            }
        }
    }

    Specimen {
        caption: qsTr("Banners: note, done, warning, error")

        Kit.Banner {
            width: parent.width
            text: qsTr("A newer version of TUR is ready. It is installed the next time you restart.")
            actionText: qsTr("Restart now")
        }

        Kit.Banner {
            width: parent.width
            severity: Kit.Banner.Positive
            text: qsTr("The support bundle was created.")
        }

        Kit.Banner {
            width: parent.width
            severity: Kit.Banner.Warning
            text: qsTr("Windows did not shut this disk down properly. TUR opened it read-only to protect your data.")
        }

        Kit.Banner {
            width: parent.width
            severity: Kit.Banner.Negative
            text: qsTr("The update was not applied. Your computer is unchanged.")
            actionText: qsTr("Try again")
        }
    }

    Specimen {
        caption: qsTr("Progress: a known amount, an unknown amount, and a spinner")

        Kit.ProgressBar {
            width: parent.width
            label: qsTr("Copying the system image")
            detail: qsTr("2.1 GiB of 5.4 GiB")
            value: 0.39
        }

        Kit.ProgressBar {
            width: parent.width
            label: qsTr("Checking the disk")
            indeterminate: true
        }

        Kit.BusyIndicator {
            label: qsTr("Looking for updates")
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit
import Tur.Kit.Pax as Pax

// The tokens: every colour, the type scale, and P@x's gold beside the
// system's blue.
GalleryPage {
    title: qsTr("Foundations")

    Specimen {
        caption: qsTr("Surfaces and lines")

        Swatch { token: "surface.base"; tint: Kit.Theme.surfaceBase }
        Swatch { token: "surface.raised"; tint: Kit.Theme.surfaceRaised }
        Swatch { token: "surface.overlay"; tint: Kit.Theme.surfaceOverlay }
        Swatch { token: "surface.sunken"; tint: Kit.Theme.surfaceSunken }
        Swatch { token: "line.subtle"; tint: Kit.Theme.lineSubtle }
        Swatch { token: "line.control"; tint: Kit.Theme.lineControl }
    }

    Specimen {
        caption: qsTr("Text")

        Swatch { token: "text.primary"; tint: Kit.Theme.textPrimary }
        Swatch { token: "text.secondary"; tint: Kit.Theme.textSecondary }
        Swatch { token: "text.disabled"; tint: Kit.Theme.textDisabled }
    }

    Specimen {
        caption: qsTr("Blue is the system")

        Swatch { token: "accent"; tint: Kit.Theme.accent }
        Swatch { token: "accent.on"; tint: Kit.Theme.accentOn }
        Swatch { token: "accent.text"; tint: Kit.Theme.accentText }
        Swatch { token: "selection"; tint: Kit.Theme.selection }
        Swatch { token: "selection.on"; tint: Kit.Theme.selectionOn }
    }

    Specimen {
        caption: qsTr("Status, always with a sign and a word")

        Swatch { token: "positive"; tint: Kit.Theme.positive }
        Swatch { token: "warning"; tint: Kit.Theme.warning }
        Swatch { token: "negative"; tint: Kit.Theme.negative }
    }

    Specimen {
        caption: qsTr("Gold is P@x, and nothing else")

        Swatch { token: "pax"; tint: Pax.PaxTheme.pax }
        Swatch { token: "pax.fill"; tint: Pax.PaxTheme.fill }
        Swatch { token: "pax.on"; tint: Pax.PaxTheme.fillText }

        ColumnLayout {
            spacing: Kit.Theme.spaceRelated

            Pax.PaxName {
            }

            Pax.PaxPill {
                text: qsTr("P@x is working")
            }
        }
    }

    Specimen {
        caption: qsTr("Type")

        ColumnLayout {
            width: parent.width
            spacing: Kit.Theme.spaceRelated

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Display: set up your computer")
                kind: Kit.Label.Display
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Title: updates and recovery")
                kind: Kit.Label.Title
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Heading: the update was not applied")
                kind: Kit.Label.Heading
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Body: TUR went back to the version that worked. Your files are unchanged, and nothing else needs to be done now.")
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Strong: the one thing in a paragraph that must be seen")
                kind: Kit.Label.Strong
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Small: supporting lines and captions; nothing is smaller")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
            }

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Caps: erase this disk")
                kind: Kit.Label.Caps
                tone: Kit.Label.Secondary
            }

            Kit.Label {
                Layout.fillWidth: true
                text: "Mono: /dev/disk/by-uuid/0f3a-91c2  exit status 14  0123456789"
                kind: Kit.Label.Mono
            }

            Kit.Label {
                Layout.fillWidth: true
                //: A sample of figures that keep their width. Keep the numbers.
                text: qsTr("Tabular figures: 2.1 GiB of 5.4 GiB · 11:41 left")
                tabular: true
            }
        }
    }
}

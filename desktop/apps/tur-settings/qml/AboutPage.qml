// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// About (desktop-experience.md, 10.2): version, build and base; licences and
// third-party notices; the statement about Fedora (12.2).
//
// Everything on the page is read: from the file the image build writes, from
// os-release, and from the texts the program carries. A value the files do
// not hold is "Not recorded". The plain facts come first; the identifiers a
// person quotes when asking for help are under "Technical detail".
SettingsPage {
    id: view

    required property SystemFacts facts

    address: "tur.about"

    FailureMessage {
        objectName: "factsFailure"
        Layout.fillWidth: true
        visible: view.facts.failure.code !== undefined
        severity: Kit.Message.Warning
        failure: view.facts.failure
    }

    SettingsGroup {
        page: view.address
        section: "system"

        SettingRow {
            address: "tur.about#version"
            value: view.facts.version.length > 0 ? view.facts.version : Words.notRecorded
            technical: view.facts.version.length > 0
        }

        SettingRow {
            address: "tur.about#build"
            value: view.facts.build.length > 0 ? view.facts.build : Words.notRecorded
            technical: view.facts.build.length > 0
        }

        SettingRow {
            address: "tur.about#base"
            value: view.facts.baseVersion.length > 0 ? view.facts.baseVersion : Words.notRecorded
            technical: view.facts.baseVersion.length > 0
            detail: qsTr("The version of the system image TUR was built on.")
        }
    }

    SettingsGroup {
        page: view.address
        section: "origin"

        SettingRow {
            address: "tur.about#origin"
            detail: Words.origin
        }
    }

    SettingsGroup {
        page: view.address
        section: "licences"

        SettingRow {
            address: "tur.about#licence"
            // The licence's own name, as its text has it: not translated.
            value: "TUR Noncommercial License 1.0.0"
            // The notice the licence requires to be passed on, as the
            // project wrote it.
            detail: Documents.text("notice")
            focusTarget: licenceOpener

            Kit.Disclosure {
                id: licenceOpener

                objectName: "licenceOpener"
                Layout.fillWidth: true
                text: qsTr("Read the licence")
            }

            DocumentText {
                objectName: "licenceText"
                visible: licenceOpener.expanded
                text: Documents.text("licence")
            }
        }

        SettingRow {
            address: "tur.about#third-party"
            detail: qsTr("TUR contains software from many other projects. Each is under its own licence, which TUR does not change. The licence of each package is under /usr/share/licenses/ on this computer.")
            focusTarget: noticesOpener

            Kit.Disclosure {
                id: noticesOpener

                objectName: "noticesOpener"
                Layout.fillWidth: true
                text: qsTr("Read the third-party notices")
            }

            DocumentText {
                objectName: "noticesText"
                visible: noticesOpener.expanded
                text: Documents.text("third-party")
            }
        }
    }

    SettingsGroup {
        objectName: "technicalGroup"
        page: view.address
        section: "technical"
        collapsible: true

        SettingRow {
            address: "tur.about#source-revision"
            value: view.facts.sourceRevision.length > 0 ? view.facts.sourceRevision : Words.notRecorded
            technical: view.facts.sourceRevision.length > 0
        }

        SettingRow {
            address: "tur.about#base-image"
            value: view.facts.baseImage.length > 0 ? view.facts.baseImage : Words.notRecorded
            technical: view.facts.baseImage.length > 0
            detail: view.facts.baseDigest
        }

        SettingRow {
            address: "tur.about#system-name"
            value: view.facts.systemName.length > 0 ? view.facts.systemName : Words.notRecorded
            detail: view.facts.systemId.length > 0 ? "ID=" + view.facts.systemId : ""
        }

        SettingRow {
            address: "tur.about#settings-version"
            value: view.facts.programVersion.length > 0 ? "tur-settings " + view.facts.programVersion : "tur-settings"
            technical: true
            detail: "Qt " + view.facts.qtVersion
        }
    }
}

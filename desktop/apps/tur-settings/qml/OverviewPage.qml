// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Overview (desktop-experience.md, 10.2): the version and build, the state
// of updates, the health of the last start, and anything that needs
// attention.
//
// Of these, the version and the build can be read today, from the file the
// image build writes. The other three are reported by services that are not
// part of the system yet. Each of those rows is there, says that it cannot
// tell and why, and shows no value: "up to date" or "nothing needs
// attention" would be a reassurance nothing backs (design, principle 1).
SettingsPage {
    id: view

    required property SystemFacts facts

    address: "tur.overview"

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
            address: "tur.overview#version"
            value: view.facts.version.length > 0 ? view.facts.version : Words.notRecorded
            technical: view.facts.version.length > 0
        }

        SettingRow {
            address: "tur.overview#build"
            value: view.facts.build.length > 0 ? view.facts.build : Words.notRecorded
            technical: view.facts.build.length > 0
        }
    }

    SettingsGroup {
        page: view.address
        section: "state"

        SettingRow {
            address: "tur.overview#update-state"
            value: Words.notAvailable
            available: false
            unavailableReason: qsTr("TUR cannot say here whether an update is waiting: its update service is not part of the system yet.")
        }

        SettingRow {
            address: "tur.overview#last-start"
            value: Words.notAvailable
            available: false
            unavailableReason: qsTr("TUR cannot say here how the last start went: the record of each start is not part of the system yet.")
        }

        SettingRow {
            address: "tur.overview#attention"
            value: Words.notAvailable
            available: false
            unavailableReason: qsTr("TUR cannot say here whether anything needs attention: the services that would report it are not part of the system yet. That nothing is listed does not mean that nothing is wrong.")
        }
    }
}

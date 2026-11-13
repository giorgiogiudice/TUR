// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The application centre's window. Everything in it is Shell; the tests and
// the screenshot job load Shell without the window.
Window {
    id: window

    // "light" or "dark" to start in that variant; empty to follow the desktop.
    property string variant

    // Where the answers come from: see src/main.cpp and Catalogue.
    property string fixture
    property string program: "tur-catalogue"
    property string catalogueDirectory
    property string hostFacts
    property string preferences

    // Where installing is done: tur-apps, or recorded runs of it.
    property string appsFixture
    property string appsProgram: "tur-apps"

    // What to open: tur-centre:home, tur-centre:search?q=..., tur-centre:app/<id>.
    property string address

    width: 1080
    height: 760
    minimumWidth: 640
    minimumHeight: 480
    visible: true
    title: qsTr("Application centre")
    color: Kit.Theme.surfaceBase

    Component.onCompleted: {
        if (window.variant === "light")
            Kit.Theme.light = true
        else if (window.variant === "dark")
            Kit.Theme.light = false
        if (window.address.length > 0 && !shell.open(window.address))
            console.warn("tur-centre: not an address of the application centre: " + window.address)
        // The search field has the keyboard when the window opens.
        shell.focusSearch()
    }

    Catalogue {
        id: answers

        fixture: window.fixture
        program: window.program
        catalogueDirectory: window.catalogueDirectory
        hostFacts: window.hostFacts
        preferences: window.preferences
    }

    // tur-apps, run as a program of this session: it installs per user.
    // The same catalogue, host facts and preferences as the router's.
    Apps {
        id: installer

        fixture: window.appsFixture
        program: window.appsProgram
        catalogueDirectory: window.catalogueDirectory
        hostFacts: window.hostFacts
        preferences: window.preferences
    }

    Shell {
        id: shell

        anchors.fill: parent
        catalogue: answers
        apps: installer
        onUrlRequested: url => Qt.openUrlExternally(url)
    }
}

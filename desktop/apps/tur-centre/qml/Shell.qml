// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// The application centre's window, without the window: a header with the
// search field, a side list of the seven views, and one content area
// (desktop-experience.md, section 8.1). There are no tabs inside tabs.
//
// An application's page takes the whole width, with the way back in the
// header where the title was (section 8.4).
//
// Every view has an address: tur-centre:home, tur-centre:search?q=..., and
// tur-centre:app/<id>, where <id> is the catalogue's identifier. `address`
// is where the shell is; open() goes to one.
//
// Part 1 builds Home, Search and an application's page. The other five views
// are listed, because the design's structure has seven, and are unavailable
// until the task that builds each.
Kit.Page {
    id: shell

    required property Catalogue catalogue

    // How long after the last key a search is sent, in milliseconds.
    property int debounce: 250

    // "home", "search" or "application".
    property string view: "home"

    readonly property bool onApplication: shell.view === "application"
    readonly property bool hasQuery: shell.catalogue.search.query.length > 0

    readonly property string address: {
        if (shell.view === "application")
            return "tur-centre:app/" + encodeURIComponent(shell.catalogue.application.applicationId)
        if (shell.view === "search") {
            const query = shell.catalogue.search.query
            return query.length > 0 ? "tur-centre:search?q=" + encodeURIComponent(query) : "tur-centre:search"
        }
        return "tur-centre:home"
    }

    readonly property real sideWidth: 204

    // The height at which nothing has to scroll. The pictures are taken at
    // it, so that each shows the whole of its view.
    readonly property real naturalHeight: header.implicitHeight + 2 * Kit.Theme.spaceGroup + Kit.Theme.lineWidth
                                          + Math.max(side.visible ? side.implicitHeight + 2 * Kit.Theme.spaceRelated : 0,
                                                     pane.implicitHeight)

    readonly property var sections: [
        { view: "home", title: qsTr("Home"), built: true },
        { view: "search", title: qsTr("Search"), built: true },
        { view: "categories", title: qsTr("Categories"), built: false },
        { view: "installed", title: qsTr("Installed"), built: false },
        { view: "updates", title: qsTr("Updates"), built: false },
        { view: "sources", title: qsTr("Sources"), built: false },
        { view: "open-a-file", title: qsTr("Open a file"), built: false }
    ]

    // What has the keyboard, so that it can be kept in sight.
    readonly property Item focusItem: shell.Window.activeFocusItem

    // Opens a page in the browser. Nothing is fetched by the application
    // centre itself.
    signal urlRequested(string url)

    function showHome(): void {
        shell.view = "home"
    }

    function showSearch(query: string): void {
        pending.stop()
        field.text = query
        shell.catalogue.search.query = query
        shell.view = "search"
    }

    function showApplication(applicationId: string): void {
        shell.catalogue.application.applicationId = applicationId
        shell.view = "application"
    }

    // From an application's page: to the results, if there are any to go
    // back to, and otherwise to Home.
    function goBack(): void {
        shell.view = shell.hasQuery ? "search" : "home"
    }

    function focusSearch(): void {
        field.forceActiveFocus()
    }

    // Goes to an address. False if it is not one of the application
    // centre's; the shell then stays where it is.
    function open(target: string): bool {
        const text = target.trim()
        try {
            if (text === "tur-centre:home") {
                shell.showHome()
                return true
            }
            const page = /^tur-centre:app\/([^/?#]+)$/.exec(text)
            if (page !== null) {
                shell.showApplication(decodeURIComponent(page[1]))
                return true
            }
            const words = /^tur-centre:search(?:\?q=([^#]*))?$/.exec(text)
            if (words !== null) {
                shell.showSearch(words[1] === undefined ? "" : decodeURIComponent(words[1].replace(/\+/g, " ")))
                return true
            }
        } catch (error) {
            // An address that is not properly encoded is not an address.
        }
        return false
    }

    // Keeps whatever has the keyboard in sight (13.1, X2): a control that
    // Tab reaches below or above the visible part is scrolled to.
    function reveal(item: Item): void {
        if (item === null)
            return
        let inside = false
        for (let at = item; at !== null; at = at.parent) {
            if (at === flick.contentItem) {
                inside = true
                break
            }
        }
        if (!inside)
            return
        const top = item.mapToItem(flick.contentItem, 0, 0).y
        const bottom = top + item.height
        const room = Kit.Theme.spaceGroup
        const furthest = Math.max(0, flick.contentHeight - flick.height)
        if (top - room < flick.contentY)
            flick.contentY = Math.max(0, top - room)
        else if (bottom + room > flick.contentY + flick.height)
            flick.contentY = Math.min(furthest, bottom + room - flick.height)
    }

    padding: 0

    onFocusItemChanged: shell.reveal(shell.focusItem)
    onViewChanged: flick.contentY = 0

    // Typing is searching: the catalogue is asked once the keys pause.
    Timer {
        id: pending

        interval: shell.debounce
        onTriggered: shell.catalogue.search.query = field.text
    }

    Shortcut {
        sequences: [StandardKey.Back]
        enabled: shell.onApplication
        onActivated: shell.goBack()
    }

    Shortcut {
        sequences: [StandardKey.Find]
        onActivated: shell.focusSearch()
    }

    Component {
        id: homeView

        HomeView {
        }
    }

    Component {
        id: searchView

        SearchView {
            search: shell.catalogue.search
            onApplicationRequested: applicationId => shell.showApplication(applicationId)
            onUrlRequested: url => shell.urlRequested(url)
        }
    }

    Component {
        id: applicationView

        ApplicationView {
            application: shell.catalogue.application
            onUrlRequested: url => shell.urlRequested(url)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            id: header

            objectName: "shellHeader"
            Layout.fillWidth: true
            Layout.margins: Kit.Theme.spaceGroup
            spacing: Kit.Theme.spaceGroup

            BackButton {
                objectName: "shellBack"
                visible: shell.onApplication
                text: shell.hasQuery ? qsTr("Results") : qsTr("Home")
                spokenName: shell.hasQuery ? qsTr("Back to the results") : qsTr("Back to Home")
                onClicked: shell.goBack()
            }

            Kit.Label {
                objectName: "shellTitle"
                Layout.preferredWidth: shell.sideWidth - Kit.Theme.spaceRelated
                visible: !shell.onApplication
                text: qsTr("Application centre")
                kind: Kit.Label.Title
            }

            Kit.TextField {
                id: field

                objectName: "shellSearch"
                Layout.fillWidth: true
                placeholderText: qsTr("Search")
                onTextEdited: {
                    if (field.text.trim().length > 0 || shell.view === "search")
                        shell.view = "search"
                    pending.restart()
                }
                onAccepted: {
                    pending.stop()
                    shell.catalogue.search.query = field.text
                    shell.view = "search"
                }
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            ColumnLayout {
                id: side

                objectName: "shellSide"
                Layout.preferredWidth: shell.sideWidth
                Layout.maximumWidth: shell.sideWidth
                Layout.fillHeight: true
                Layout.margins: Kit.Theme.spaceRelated
                visible: !shell.onApplication
                spacing: Kit.Theme.spaceGrid

                Repeater {
                    model: shell.sections

                    Kit.ListRow {
                        id: section

                        required property var modelData

                        // To the next or the previous view of the list that
                        // is available; not out of the list.
                        function step(forward: bool): void {
                            const next = section.nextItemInFocusChain(forward)
                            if (next !== null && next.parent === section.parent)
                                next.forceActiveFocus(Qt.TabFocusReason)
                        }

                        objectName: "shellSection-" + modelData.view
                        Layout.fillWidth: true
                        text: modelData.title
                        enabled: modelData.built
                        selected: shell.view === modelData.view
                        // A row of a list does not take the keyboard by
                        // itself. These are the window's navigation: Tab
                        // reaches each, and the arrows move between them.
                        focusPolicy: Qt.StrongFocus
                        Keys.onDownPressed: section.step(true)
                        Keys.onUpPressed: section.step(false)
                        onClicked: shell.view = modelData.view
                    }
                }

                Item {
                    Layout.fillHeight: true
                }
            }

            Kit.Separator {
                Layout.preferredWidth: Kit.Theme.lineWidth
                Layout.fillHeight: true
                visible: side.visible
            }

            Flickable {
                id: flick

                objectName: "shellContent"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: width
                contentHeight: pane.implicitHeight
                boundsBehavior: Flickable.StopAtBounds

                Controls.ScrollBar.vertical: Controls.ScrollBar {
                }

                Item {
                    id: pane

                    width: flick.width
                    implicitHeight: content.implicitHeight + 2 * Kit.Theme.spaceMargin
                    height: implicitHeight

                    Loader {
                        id: content

                        objectName: "shellView"
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.margins: Kit.Theme.spaceMargin
                        sourceComponent: shell.view === "application" ? applicationView
                                         : shell.view === "search" ? searchView : homeView
                    }
                }
            }
        }
    }
}

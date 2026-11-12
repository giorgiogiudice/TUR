// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import Tur.Kit as Kit

// The settings window, without the window (desktop-experience.md, 10.2): the
// search field and the list of pages on one side, one page on the other, and
// at the end of the list the way into KDE System Settings.
//
// The list is the registry's: every page of this program that is listed,
// under its registered title. A page that is reserved and not built is in
// the list and cannot be opened.
//
// Addressing (10.3). The shell is where an address becomes something on
// screen: show() opens a page, brings a row into view, gives it the keyboard,
// highlights it, and says where in the window the row is. It is asked by
// SettingsHost, which has already found out from the registries that the
// address is one of this program's pages.
Kit.Page {
    id: shell

    required property SettingsHost host
    required property SystemFacts facts

    // Developer mode: the address of what is on screen is shown at the foot
    // of the window. Addresses are never shown otherwise (10.3, rule 2).
    property bool developer: false

    // How long after the last key the list of matches follows the field, in
    // milliseconds.
    property int debounce: 150

    // The page that is shown: an address of this program's registry.
    property string page: "tur.overview"

    // What is being searched for. While it is not blank, the matches are
    // shown in place of the page.
    property string query
    readonly property bool searching: shell.query.trim().length > 0
    readonly property var hits: shell.searching ? Registry.search(shell.query, 30, Registry.revision) : []

    // The row that is highlighted, and whether the rest of the window is
    // dimmed for it.
    property SettingRow marked: null
    property bool dimmed: false

    // Where the highlighted row is in the shell, with the line around it:
    // what the dimming leaves clear.
    property rect hole: Qt.rect(0, 0, 0, 0)

    // What could not be opened: code, title, reason, detail. Empty for
    // nothing.
    property var failure: ({})

    // The groups of pages in the list that are open, by their titles in the
    // registry. A group is closed until asked for: "P@x", whose pages come
    // with the assistant, is not put forward (design, principle 3).
    property var openGroups: []

    // Where the shell is: the row that is highlighted, else the page; empty
    // while the matches of a search are shown.
    readonly property string address: shell.searching ? "" : shell.marked !== null ? shell.marked.address : shell.page

    // The side grows with the size of text, so that the search field and
    // the titles of the pages keep their room at 200 % (13.1, X8).
    readonly property real sideWidth: Math.round(264 * Math.max(1, Kit.Theme.fontScale))

    // The height at which nothing has to scroll. The pictures are taken at
    // it, so that each shows the whole of its page.
    readonly property real naturalHeight: Math.max(side.implicitHeight + 2 * Kit.Theme.spaceRelated,
                                                   pane.implicitHeight)
                                          + (foot.visible ? foot.implicitHeight + Kit.Theme.lineWidth : 0)

    // What has the keyboard, so that it can be kept in sight.
    readonly property Item focusItem: shell.Window.activeFocusItem

    // Goes to a page of this program, as choosing it in the list does. What
    // was highlighted, searched for or refused is put away.
    function go(page: string): void {
        const elsewhere = shell.searching || shell.page !== page
        shell.clearHighlight()
        shell.failure = ({})
        pending.stop()
        field.text = ""
        shell.query = ""
        shell.page = page
        // Another page begins at its top.
        if (elsewhere)
            flick.contentY = 0
        // A page inside a group: the group opens, so that the list shows
        // where the window is.
        const path = Registry.entry(page, Registry.revision).path
        if (path !== undefined && path.length > 1)
            shell.setGroupOpen(path[0], true)
    }

    function focusSearch(): void {
        field.forceActiveFocus()
    }

    // Opens or closes a group of pages in the list.
    function setGroupOpen(group: string, open: bool): void {
        const others = shell.openGroups.filter(name => name !== group)
        shell.openGroups = open ? others.concat([group]) : others
    }

    // From one item of the list of pages to the next or the previous one
    // that can take the keyboard; not out of the list.
    function stepInList(from: Item, forward: bool): void {
        const next = from.nextItemInFocusChain(forward)
        if (next !== null && (next.objectName.startsWith("shellPage-") || next.objectName.startsWith("shellGroup-")))
            next.forceActiveFocus(Qt.TabFocusReason)
    }

    // Searches for some words at once, as typing them and waiting does.
    function search(words: string): void {
        shell.clearHighlight()
        pending.stop()
        field.text = words
        shell.query = words
    }

    // Opens what an address names, wherever it is: a page here, a page of
    // KDE System Settings, a page of another TUR program. What cannot be
    // opened is said on the page.
    function follow(address: string, highlight: bool): void {
        const outcome = highlight ? shell.host.reveal(address, { highlight: true }) : shell.host.open(address)
        if (outcome.ok !== true)
            shell.failure = outcome
    }

    function openSystemSettings(): void {
        const outcome = shell.host.openSystemSettings()
        shell.failure = outcome.ok === true ? ({}) : outcome
    }

    function clearHighlight(): void {
        expiry.stop()
        if (shell.marked !== null) {
            shell.marked.marked = false
            shell.marked.markedByPax = false
        }
        shell.marked = null
        shell.dimmed = false
    }

    // Shows a page of this program and, if `row` is not empty, that row of
    // it: in view, with the keyboard, highlighted if asked. Returns `found`,
    // and where the row is in the window (the page's own area, for a page).
    function show(page: string, row: string, options: var): var {
        shell.go(page)
        const view = content.item as SettingsPage
        if (view === null || view.address !== page)
            return { found: false }

        const window = shell.Window.window
        const result = {
            found: true,
            highlighted: false,
            focused: false,
            windowTitle: window !== null ? window.title : "",
            scale: shell.Screen.devicePixelRatio
        }
        if (row.length === 0) {
            shell.host.settle(shell)
            flick.contentY = 0
            const corner = flick.mapToItem(null, 0, 0)
            result.x = corner.x
            result.y = corner.y
            result.width = flick.width
            result.height = flick.height
            return result
        }

        const item = shell.host.findRow(view, page + "#" + row) as SettingRow
        if (item === null)
            return { found: false }
        // A row in a group that is closed: the group opens.
        for (let at = item.parent; at !== null; at = at.parent) {
            const group = at as SettingsGroup
            if (group !== null) {
                group.expose()
                break
            }
        }
        shell.host.settle(shell)
        shell.keepInSight(item)
        if (options.highlight === true) {
            item.markedByPax = options.pax === true
            item.marked = true
            shell.marked = item
            shell.dimmed = options.dim === true
            shell.track()
            if (options.duration > 0) {
                expiry.interval = Math.round(options.duration * 1000)
                expiry.restart()
            }
        }
        item.takeFocus()

        const origin = item.mapToItem(null, 0, 0)
        result.x = origin.x
        result.y = origin.y
        result.width = item.width
        result.height = item.height
        // The row's control, where it has one: what a person presses, and
        // so what P@x's pointer points at (tur-pax-overlay.md, section 4).
        const control = item.focusTarget
        if (control !== null && control.visible) {
            const at = control.mapToItem(null, 0, 0)
            result.controlX = at.x
            result.controlY = at.y
            result.controlWidth = control.width
            result.controlHeight = control.height
        }
        result.highlighted = item.marked
        result.focused = item.activeFocus || (item.focusTarget !== null && item.focusTarget.activeFocus)
        return result
    }

    // Keeps an item in sight (13.1, X2): one that is below or above the
    // visible part of the page, or of the list of pages, is scrolled to.
    function keepInSight(item: Item): void {
        if (item === null)
            return
        for (const area of [flick, list]) {
            let inside = false
            for (let at = item; at !== null; at = at.parent) {
                if (at === area.contentItem) {
                    inside = true
                    break
                }
            }
            if (!inside)
                continue
            const top = item.mapToItem(area.contentItem, 0, 0).y
            const bottom = top + item.height
            const room = Kit.Theme.spaceGroup
            const furthest = Math.max(0, area.contentHeight - area.height)
            if (top - room < area.contentY)
                area.contentY = Math.max(0, top - room)
            else if (bottom + room > area.contentY + area.height)
                area.contentY = Math.min(furthest, bottom + room - area.height)
        }
    }

    // Works out where the highlighted row is now. The row moves when the
    // page scrolls, the window changes size or a group above it opens; this
    // is asked before every frame while the window is dimmed.
    function track(): void {
        if (shell.marked === null)
            return
        const reach = 7
        const corner = shell.marked.mapToItem(shell, 0, 0)
        shell.hole = Qt.rect(corner.x - reach, corner.y - reach, shell.marked.width + 2 * reach,
                             shell.marked.height + 2 * reach)
    }

    // Tells the host when a row of the page that was just loaded changes, so
    // that it can say so on the bus (RowChanged).
    function watch(view: Item): void {
        const rows = shell.host.rows(view)
        for (const candidate of rows) {
            const row = candidate as SettingRow
            if (row === null)
                continue
            const address = row.address
            row.changed.connect(() => shell.host.rowChanged(address))
        }
    }

    padding: 0

    onFocusItemChanged: shell.keepInSight(shell.focusItem)
    onSearchingChanged: {
        // The page, and any row highlighted on it, give way to the matches.
        if (shell.searching)
            shell.clearHighlight()
    }

    Connections {
        target: shell.host

        function onShowRequested(page: string, row: string, options: var): void {
            shell.host.answer(shell.show(page, row, options))
        }

        function onClearRequested(): void {
            shell.clearHighlight()
        }
    }

    Connections {
        target: shell.Window.window
        enabled: shell.dimmed

        function onAfterAnimating(): void {
            shell.track()
        }
    }

    // The field is followed once the keys pause.
    Timer {
        id: pending

        interval: shell.debounce
        onTriggered: shell.query = field.text
    }

    // A highlight that was asked for with a duration.
    Timer {
        id: expiry

        onTriggered: shell.clearHighlight()
    }

    Shortcut {
        sequences: [StandardKey.Find]
        onActivated: shell.focusSearch()
    }

    Shortcut {
        sequences: [StandardKey.Cancel]
        enabled: shell.marked !== null
        onActivated: shell.clearHighlight()
    }

    Component {
        id: overviewPage

        OverviewPage {
            facts: shell.facts
        }
    }

    Component {
        id: appearancePage

        AppearancePage {
            onAddressRequested: target => shell.follow(target, false)
        }
    }

    Component {
        id: aboutPage

        AboutPage {
            facts: shell.facts
        }
    }

    Component {
        id: searchResults

        SearchResults {
            query: shell.query
            hits: shell.hits
            onChosen: address => shell.follow(address, true)
            onSystemSettingsRequested: shell.openSystemSettings()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

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
                spacing: Kit.Theme.spaceGrid

                Kit.TextField {
                    id: field

                    objectName: "shellSearch"
                    Layout.fillWidth: true
                    Layout.margins: Kit.Theme.spaceRelated
                    placeholderText: qsTr("Search settings")
                    onTextEdited: pending.restart()
                    onAccepted: {
                        pending.stop()
                        shell.query = field.text
                    }
                }

                // The pages. The list scrolls by itself in a window too low
                // for it; the field above it and the way into System
                // Settings below it stay where they are.
                Flickable {
                    id: list

                    objectName: "shellList"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    implicitHeight: pages.implicitHeight
                    clip: true
                    contentWidth: width
                    contentHeight: pages.implicitHeight
                    boundsBehavior: Flickable.StopAtBounds

                    Controls.ScrollBar.vertical: Controls.ScrollBar {
                    }

                    ColumnLayout {
                        id: pages

                        width: list.width
                        spacing: Kit.Theme.spaceGrid

                        Repeater {
                            model: Registry.navigation

                            Item {
                                id: entry

                                required property var modelData

                                readonly property bool shown: entry.modelData.header || entry.modelData.group.length === 0
                                                              || shell.openGroups.indexOf(entry.modelData.group) >= 0

                                Layout.fillWidth: true
                                implicitHeight: entry.modelData.header ? heading.implicitHeight : link.implicitHeight
                                visible: entry.shown

                                // A group of pages ("P@x"): closed until
                                // asked for, or until one of its pages is
                                // gone to.
                                Kit.Disclosure {
                                    id: heading

                                    objectName: "shellGroup-" + entry.modelData.title
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    visible: entry.modelData.header
                                    text: qsTranslate("SettingsRegistry", entry.modelData.title)
                                    checked: shell.openGroups.indexOf(entry.modelData.title) >= 0
                                    Keys.onDownPressed: shell.stepInList(heading, true)
                                    Keys.onUpPressed: shell.stepInList(heading, false)
                                    onToggled: {
                                        shell.setGroupOpen(entry.modelData.title, heading.checked)
                                        // Pressing it set `checked` by
                                        // hand; it follows the shell again.
                                        heading.checked = Qt.binding(
                                            () => shell.openGroups.indexOf(entry.modelData.title) >= 0)
                                    }
                                }

                                Kit.ListRow {
                                    id: link

                                    readonly property real indent: entry.modelData.depth * Kit.Theme.spaceGroup

                                    objectName: "shellPage-" + entry.modelData.address
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    leftPadding: Kit.Theme.spaceGroup + (link.mirrored ? 0 : link.indent)
                                    rightPadding: Kit.Theme.spaceGroup + (link.mirrored ? link.indent : 0)
                                    visible: !entry.modelData.header
                                    text: qsTranslate("SettingsRegistry", entry.modelData.title)
                                    enabled: entry.modelData.available
                                    selected: !shell.searching && shell.page === entry.modelData.address
                                    // A row of a list does not take the
                                    // keyboard by itself. These are the
                                    // window's navigation: Tab reaches each,
                                    // and the arrows move between them.
                                    focusPolicy: Qt.StrongFocus
                                    Keys.onDownPressed: shell.stepInList(link, true)
                                    Keys.onUpPressed: shell.stepInList(link, false)
                                    onClicked: shell.go(entry.modelData.address)
                                }
                            }
                        }
                    }
                }

                Kit.Separator {
                    Layout.fillWidth: true
                    Layout.margins: Kit.Theme.spaceGrid
                }

                // KDE System Settings appears inside TUR's settings (10.1).
                Kit.ListRow {
                    objectName: "shellSystemSettings"
                    Layout.fillWidth: true
                    text: qsTr("Displays, sound, network and more")
                    subtitle: qsTr("Opens System Settings")
                    focusPolicy: Qt.StrongFocus
                    onClicked: shell.openSystemSettings()
                }
            }

            Kit.Separator {
                Layout.preferredWidth: Kit.Theme.lineWidth
                Layout.fillHeight: true
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
                    implicitHeight: column.implicitHeight + 2 * Kit.Theme.spaceMargin
                    height: implicitHeight

                    ColumnLayout {
                        id: column

                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.margins: Kit.Theme.spaceMargin
                        spacing: Kit.Theme.spaceGroup

                        FailureMessage {
                            objectName: "shellFailure"
                            Layout.fillWidth: true
                            visible: shell.failure.code !== undefined
                            failure: shell.failure
                            dismissible: true
                            onDismissed: shell.failure = ({})
                        }

                        Loader {
                            id: content

                            objectName: "shellView"
                            Layout.fillWidth: true
                            sourceComponent: shell.searching ? searchResults
                                             : shell.page === "tur.appearance" ? appearancePage
                                             : shell.page === "tur.about" ? aboutPage : overviewPage
                            onLoaded: shell.watch(content.item)
                        }
                    }
                }
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
            visible: foot.visible
        }

        // Developer mode: the address of what is on screen.
        Kit.Label {
            id: foot

            objectName: "shellAddress"
            Layout.fillWidth: true
            Layout.margins: Kit.Theme.spaceRelated
            visible: shell.developer
            text: shell.address
            kind: Kit.Label.Mono
            tone: Kit.Label.Secondary
            horizontalAlignment: Text.AlignRight
        }
    }

    // The rest of the window, dimmed by a fifth, around a highlighted row
    // (9.5). Four pieces that leave the row and its line clear. There is no
    // token for this: it is the darkest tone of the variant, mostly
    // transparent, as behind the kit's sheet. Nothing here takes the pointer.
    Item {
        id: shade

        readonly property color tone: Kit.Theme.light ? Kit.Theme.textPrimary : Kit.Theme.surfaceSunken
        readonly property color wash: Qt.rgba(shade.tone.r, shade.tone.g, shade.tone.b, 0.2)
        readonly property real holeTop: Math.max(0, Math.min(shell.height, shell.hole.y))
        readonly property real holeBottom: Math.max(shade.holeTop,
                                                     Math.min(shell.height, shell.hole.y + shell.hole.height))
        readonly property real holeLeft: Math.max(0, Math.min(shell.width, shell.hole.x))
        readonly property real holeRight: Math.max(shade.holeLeft,
                                                    Math.min(shell.width, shell.hole.x + shell.hole.width))

        objectName: "shellShade"
        anchors.fill: parent
        visible: shell.dimmed && shell.marked !== null
        z: 10

        Rectangle {
            objectName: "shadeAbove"
            width: shade.width
            height: shade.holeTop
            color: shade.wash
        }

        Rectangle {
            objectName: "shadeBelow"
            y: shade.holeBottom
            width: shade.width
            height: shade.height - shade.holeBottom
            color: shade.wash
        }

        Rectangle {
            objectName: "shadeBefore"
            y: shade.holeTop
            width: shade.holeLeft
            height: shade.holeBottom - shade.holeTop
            color: shade.wash
        }

        Rectangle {
            objectName: "shadeAfter"
            x: shade.holeRight
            y: shade.holeTop
            width: shade.width - shade.holeRight
            height: shade.holeBottom - shade.holeTop
            color: shade.wash
        }
    }
}

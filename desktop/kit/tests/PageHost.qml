// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick

// Loads the gallery's pages one at a time, for the tests that walk or
// photograph every component: the screenshots, the contrast walk, the
// accessibility walk and the text-size test.
Item {
    id: host

    // The gallery's pages, in the order of its list.
    readonly property var pageNames: [
        "FoundationsPage", "ButtonsPage", "InputsPage", "ContainersPage", "MessagePage", "SheetPage"
    ]

    property real pageWidth: 800
    property Item page: null

    // "ButtonsPage" -> "buttons", for file names.
    function slug(name) {
        return name.replace(/Page$/, "").toLowerCase()
    }

    function load(name) {
        host.unload()
        const component = Qt.createComponent("Tur.Kit.Gallery", name)
        if (component.status !== Component.Ready)
            throw new Error(name + " could not be loaded: " + component.errorString())
        host.page = component.createObject(host, { width: host.pageWidth })
        if (host.page === null)
            throw new Error(name + " could not be created")
        return host.page
    }

    // Puts keyboard focus on a control so that its focus ring is drawn. Qt
    // draws the ring only when focus arrives from the keyboard, so focus that
    // is already there is taken away first.
    function showKeyboardFocus(target) {
        if (target === null)
            return
        if (target.activeFocus)
            target.focus = false
        target.forceActiveFocus(Qt.TabFocusReason)
    }

    // The caller lets the event loop run afterwards (wait(0)), so that the
    // old page is really gone before the next is looked at.
    function unload() {
        if (host.page === null)
            return
        host.page.retire()
        host.page.visible = false
        host.page.destroy()
        host.page = null
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What every screen of the installer is: a column with its title first and
// its buttons last, and the control that has the keyboard when it opens.
ColumnLayout {
    id: base

    // The control given the keyboard when the screen opens.
    property Item initialFocus: null

    // Gives the keyboard to `item`, as if Tab had reached it, so that the
    // focus ring shows where Space will act.
    function focusOn(item: Item): void {
        if (item !== null && item.enabled && item.visible)
            item.forceActiveFocus(Qt.TabFocusReason)
    }

    spacing: Kit.Theme.spaceGroup
}

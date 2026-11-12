// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick

// A stable code, such as TUR-STORAGE-NTFS-004: small, in the fixed-width
// face, always visible, and able to be selected and copied, because it is
// what a person quotes when asking for help (desktop-experience.md, 5.6).
// A code is never translated and never mirrored.
TextInput {
    id: field

    // The code itself.
    property string code

    text: field.code
    readOnly: true
    selectByMouse: true
    activeFocusOnTab: true
    font.family: Theme.monoFamily
    font.pointSize: Theme.typeSmallSize * Theme.fontScale
    color: Theme.textSecondary
    selectionColor: Theme.selection
    selectedTextColor: Theme.selectionOn

    // With the keyboard: Tab to the code, then Ctrl+C.
    onActiveFocusChanged: {
        if (field.activeFocus)
            field.selectAll()
        else
            field.deselect()
    }

    Accessible.role: Accessible.StaticText
    //: Read by a screen reader. %1 is a stable code such as TUR-STORAGE-NTFS-004.
    Accessible.name: qsTr("Code %1").arg(field.code)
    Accessible.readOnly: true

    FocusRing {
        shown: field.activeFocus
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Dialogs

// "I have the installer file": the person chooses the installer of a Windows
// application they already have (desktop-experience.md, 8.5 and 8.7). The
// system's file chooser where there is one. Nothing in the file is run to
// choose it; tur-apps hashes it in the plan, and the flow goes on from the
// hash check (app-install-backend.md, "Where the installer comes from").
//
// Loaded only when asked for, so that nothing else depends on Qt Quick
// Dialogs. An Item, so that a Loader can hold it.
Item {
    id: root

    signal chosen(string path)

    function open(): void {
        dialog.open()
    }

    FileDialog {
        id: dialog

        title: qsTr("Choose the installer")
        fileMode: FileDialog.OpenFile
        nameFilters: [qsTr("Windows installers (*.exe *.msi)"), qsTr("All files (*)")]
        onAccepted: {
            const url = dialog.selectedFile.toString()
            if (url.startsWith("file://"))
                root.chosen(decodeURIComponent(url.substring("file://".length)))
        }
    }
}

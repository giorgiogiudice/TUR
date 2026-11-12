// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// The two-layer message: closed, open, and as a plain notice. In a program
// the words come from the code's registry entry; here they are samples.
GalleryPage {
    id: page

    title: qsTr("The two-layer message")
    focusName: "messageDisclosure"

    readonly property string sampleDetail: "Device: /dev/disk/by-uuid/0f3a-91c2-77de\n"
                                           + "Driver: ntfs3\n"
                                           + "Mount policy: read-only\n"
                                           + "Exit status: 14\n"
                                           + "Evidence: journal 5d1c0c1e/s=8f2a;i=4c1"

    Specimen {
        caption: qsTr("An error, as it first appears: the technical detail is closed")

        Kit.Message {
            width: parent.width
            code: "TUR-STORAGE-NTFS-004"
            title: qsTr("The Windows volume was not mounted for writing.")
            reason: qsTr("The volume appears to be hibernated or in an unsafe state.")
            whatToDo: qsTr("Start Windows, turn off Fast Startup and hibernation, shut down fully, then try again.")
            technicalDetail: page.sampleDetail
            actions: [qsTr("Try again")]
            paxAvailable: true
        }
    }

    Specimen {
        caption: qsTr("The same error with the technical detail open, and P@x off")

        Kit.Message {
            width: parent.width
            code: "TUR-STORAGE-NTFS-004"
            title: qsTr("The Windows volume was not mounted for writing.")
            reason: qsTr("The volume appears to be hibernated or in an unsafe state.")
            whatToDo: qsTr("Start Windows, turn off Fast Startup and hibernation, shut down fully, then try again.")
            technicalDetail: page.sampleDetail
            actions: [qsTr("Try again")]
            expanded: true
        }
    }

    Specimen {
        caption: qsTr("A warning and a notable event")

        Kit.Message {
            width: parent.width
            severity: Kit.Message.Warning
            code: "TUR-UPDATE-ROLLBACK-002"
            title: qsTr("TUR went back to version 0.3.2.")
            reason: qsTr("Version 0.3.3 did not start properly three times in a row.")
            whatToDo: qsTr("You can keep working. TUR will not try version 0.3.3 again.")
        }

        Kit.Message {
            width: parent.width
            severity: Kit.Message.Done
            code: "TUR-DIAG-BUNDLE-001"
            title: qsTr("The support bundle was created.")
        }
    }
}

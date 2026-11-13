// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// End to end, part 2: the Updates view asks Flathub, lists Chromium B.S.U.
// (session.sh put an earlier commit of it in place with flatpak), and
// "Update" updates it for real. When Flathub has no earlier commit, the view
// says everything is up to date, and that is what is checked.
Item {
    id: root

    width: 1080
    height: 900

    RealCentre {
        id: centre

        anchors.fill: parent
        test: testCase
    }

    TestCase {
        id: testCase

        name: "EndToEndUpdate"
        when: windowShown

        function initTestCase() {
            verify(centreEndToEnd.apps.length > 0, "the job gives the real tur-apps")
            Kit.Motion.frozen = true
        }

        function test_update_from_the_updates_view() {
            verify(centre.shell.open("tur-centre:updates"))
            tryVerify(() => centre.apps.installedStatus === Apps.Ready && centre.apps.updatesChecked, 300000,
                      "the sources were asked for updates")
            centre.shot("8-updates")
            if (!centreEndToEnd.updateExpected) {
                verify(centre.control(centre.view(), "updatesNone").visible)
                return
            }
            const row = centre.control(centre.view(), "updatesRow-" + centre.app)
            centre.press(centre.control(row, "installedRowUpdateButton"), "Update")
            const operation = centre.apps.operation(centre.app)
            verify(operation !== null)
            compare(operation.kind, AppsOperation.Update)
            tryVerify(() => operation.phase !== AppsOperation.Running, 1500000, "the update ends")
            compare(operation.phase, AppsOperation.Succeeded, JSON.stringify(operation.failure))
            compare(operation.result.updated, true, JSON.stringify(operation.result))
            tryVerify(() => centre.apps.installedStatus === Apps.Ready, 300000)
            compare(centre.apps.installedEntry(centre.app).update.available, false)
            centre.shot("9-updated")
        }
    }
}

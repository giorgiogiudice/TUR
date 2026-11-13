// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// End to end, part 3: Remove asks its one question with "Keep its settings
// and documents" chosen, and removes the game keeping its data; then Install
// again and Cancel while flatpak works: "Cancelled. Nothing was installed."
// session.sh then checks from outside that the data is still there and that
// nothing is installed.
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

        name: "EndToEndRemove"
        when: windowShown

        function initTestCase() {
            verify(centreEndToEnd.apps.length > 0, "the job gives the real tur-apps")
            Kit.Motion.frozen = true
        }

        function test_remove_keeping_data_then_cancel_an_installation() {
            verify(centre.shell.open("tur-centre:app/" + centre.app))
            tryVerify(() => centre.catalogue.application.status === ApplicationSession.Ready, 120000)
            tryVerify(() => centre.apps.installedEntry(centre.app).state === "installed", 120000)
            const page = centre.view()
            centre.shot("10-on-this-computer")

            centre.step("Remove")
            centre.press(centre.control(page, "installedRemoveButton"), "Remove…")
            const sheet = centre.control(page, "removeSheet")
            tryVerify(() => sheet.opened, 10000)
            const keep = centre.control(sheet.contentItem, "removeKeep")
            verify(keep.checked, "keeping the data is chosen when the question opens")
            verify(!centre.control(sheet.contentItem, "removeDelete").checked)
            centre.shot("11-remove-question")
            centre.press(centre.control(sheet.contentItem, "removeConfirm"), "the sheet's Remove")
            let operation = centre.apps.operation(centre.app)
            verify(operation !== null)
            compare(operation.kind, AppsOperation.Remove)
            verify(!operation.deleteData)
            tryVerify(() => operation.phase !== AppsOperation.Running, 600000, "the removal ends")
            compare(operation.phase, AppsOperation.Succeeded, JSON.stringify(operation.failure))
            tryVerify(() => centre.apps.installedStatus === Apps.Ready
                      && centre.apps.installedEntry(centre.app).id === undefined, 120000, "it is not listed")
            centre.shot("12-removed")

            centre.step("Install again, and cancel")
            const primary = centre.control(page, "applicationPrimary")
            tryVerify(() => primary.text === "Install" && primary.visible, 10000)
            centre.press(primary, "Install")
            operation = centre.apps.operation(centre.app)
            tryVerify(() => operation.phase !== AppsOperation.Planning, 300000)
            compare(operation.phase, AppsOperation.Planned, JSON.stringify(operation.failure))
            const plan = centre.control(page, "installSheet")
            tryVerify(() => plan.opened, 10000)
            centre.press(centre.control(plan.contentItem, "planInstall"), "the sheet's Install")
            // Cancel once flatpak reports its first operation, as the
            // recorder does.
            tryVerify(() => operation.phase !== AppsOperation.Running
                      || (operation.step === "install" && operation.activity.length > 0), 600000)
            compare(operation.phase, AppsOperation.Running, "the installation was still under way")
            verify(operation.cancellable)
            centre.press(centre.control(page, "operationCancel"), "Cancel")
            verify(operation.cancelRequested)
            centre.shot("13-cancelling")
            tryVerify(() => operation.phase !== AppsOperation.Running, 300000, "the cancel is answered")
            compare(operation.phase, AppsOperation.Cancelled, JSON.stringify(operation.failure))
            compare(centre.control(page, "operationDone").text, "Cancelled. Nothing was installed.")
            centre.shot("14-cancelled")
            centre.apps.refresh(false)
            tryVerify(() => centre.apps.installedStatus === Apps.Ready, 120000)
            compare(centre.apps.installedEntry(centre.app).id, undefined, "nothing is listed after the cancel")
        }
    }
}

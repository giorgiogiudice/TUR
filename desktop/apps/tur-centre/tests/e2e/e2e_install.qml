// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit

// End to end, part 1: a fresh user switches Flathub on in Sources, opens
// Chromium B.S.U.'s page, presses Install, reads the plan, confirms, watches
// the progress, and opens the game. Real tur-apps, real flatpak, Flathub.
// Run by desktop/ci/centre-e2e/session.sh; its name does not begin with
// tst_, so the ordinary test run does not pick it up.
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

        name: "EndToEndInstall"
        when: windowShown

        function initTestCase() {
            verify(centreEndToEnd.apps.length > 0, "the job gives the real tur-apps")
            Kit.Motion.frozen = true
        }

        function sourceState(name) {
            const found = centre.apps.sources.find(source => source.name === name)
            return found ? found.state : ""
        }

        function test_install_from_flathub_and_open() {
            centre.step("Sources")
            verify(centre.shell.open("tur-centre:sources"))
            tryVerify(() => centre.apps.sourcesStatus === Apps.Ready, 120000, "the sources are listed")
            centre.shot("1-sources")
            const row = centre.control(centre.view(), "source-flathub")
            const flathubSwitch = centre.control(row, "sourceSwitch")
            if (!flathubSwitch.checked)
                centre.press(flathubSwitch, "Flathub's switch")
            tryVerify(() => centre.apps.sourceChanging === "" && sourceState("flathub") === "on"
                      && centre.apps.sourcesStatus === Apps.Ready, 180000, "Flathub is on for this user")
            centre.shot("2-sources-flathub-on")

            centre.step("The application's page")
            verify(centre.shell.open("tur-centre:app/" + centre.app))
            tryVerify(() => centre.catalogue.application.status === ApplicationSession.Ready, 120000)
            tryVerify(() => centre.apps.installedStatus === Apps.Ready, 120000)
            const page = centre.view()
            const primary = centre.control(page, "applicationPrimary")
            compare(primary.text, "Install")
            centre.shot("3-application")

            centre.step("Install: the plan")
            centre.press(primary, "Install")
            const operation = centre.apps.operation(centre.app)
            verify(operation !== null)
            tryVerify(() => operation.phase !== AppsOperation.Planning, 300000, "the plan is made")
            compare(operation.phase, AppsOperation.Planned, JSON.stringify(operation.failure))
            const sheet = centre.control(page, "installSheet")
            tryVerify(() => sheet.opened, 10000, "the plan's sheet is open")
            compare(operation.plan.route, "flatpak")
            verify(operation.plan.downloads.length > 0)
            centre.shot("4-plan")

            centre.step("Install: confirmed")
            centre.press(centre.control(sheet.contentItem, "planInstall"), "the sheet's Install")
            tryVerify(() => operation.phase === AppsOperation.Running, 20000)
            tryVerify(() => operation.activity.length > 0 || operation.phase !== AppsOperation.Running, 600000,
                      "flatpak reports what it does")
            if (operation.phase === AppsOperation.Running)
                centre.shot("5-installing")
            tryVerify(() => operation.phase !== AppsOperation.Running, 1500000, "the installation ends")
            compare(operation.phase, AppsOperation.Succeeded, JSON.stringify(operation.failure))
            tryVerify(() => centre.apps.installedEntry(centre.app).state === "installed", 120000,
                      "the list shows it installed")
            tryVerify(() => primary.text === "Open", 10000)
            centre.shot("6-installed")

            centre.step("Open")
            centre.press(primary, "Open")
            compare(page.launchFailure.code, undefined, JSON.stringify(page.launchFailure))
            // watch.sh finds the game's window, takes pictures of the screen
            // and closes it.
            wait(30000)

            centre.step("Installed")
            verify(centre.shell.open("tur-centre:installed"))
            tryVerify(() => centre.apps.installedStatus === Apps.Ready, 120000)
            verify(centre.control(centre.view(), "installedRow-" + centre.app).visible)
            centre.shot("7-installed-view")
        }
    }
}

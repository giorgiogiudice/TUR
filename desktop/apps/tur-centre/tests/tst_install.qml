// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// Installing, updating and removing (desktop-experience.md, 8.5; task D-11),
// on recorded runs of the real tur-apps (fixtures/apps, written by
// desktop/ci/centre-e2e/record.py): the plan's sheet, progress, Cancel,
// success and Open, a failure with its code, Remove keeping the data,
// Update, Sources, and "I have the installer file". Every figure and every
// sentence of tur-apps asserted here is what tur-apps printed on the runner.
Item {
    id: root

    width: 1080
    height: 2400

    CentreHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "Install"
        when: windowShown

        readonly property string app: "net.sourceforge.chromium-bsu"

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        function settle() {
            tryVerify(() => host.settled, 15000, "every answer that was asked for has arrived")
            wait(0)
        }

        function page(shell) {
            return findChild(shell, "shellView").item
        }

        // The recorded runs of a scenario.
        function recorded(scenario) {
            const runs = Testing.Harness.readJson(centreFixtures + "/apps/" + scenario + "/runs.json")
            verify(runs !== undefined, scenario + " was recorded")
            return runs.invocations
        }

        function recordedRun(scenario, command) {
            return recorded(scenario).find(run => run.arguments[0] === command)
        }

        function lastEvent(run) {
            return JSON.parse(run.stdout[run.stdout.length - 1])
        }

        function openPage(scenario, id) {
            const shell = host.start("default", "tur-centre:app/" + id, scenario)
            settle()
            compare(host.catalogue.application.status, ApplicationSession.Ready)
            return page(shell)
        }

        // Presses Install and waits for the plan: its sheet opens.
        function plan(view) {
            const primary = findChild(view, "applicationPrimary")
            verify(primary.visible && primary.enabled, "Install can be pressed")
            mouseClick(primary)
            const operation = host.apps.operation(host.catalogue.application.applicationId)
            verify(operation !== null)
            tryVerify(() => operation.phase !== AppsOperation.Planning, 5000)
            return operation
        }

        function test_install_shows_the_plan_then_installs_with_progress_and_offers_open() {
            const view = openPage("install", app)
            const primary = findChild(view, "applicationPrimary")
            compare(primary.text, "Install")
            verify(!findChild(view, "applicationOnThisComputer").visible, "not installed yet")

            const operation = plan(view)
            compare(operation.phase, AppsOperation.Planned)
            const sheet = view.planSheet
            tryVerify(() => sheet.opened, 2000, "the plan is shown before anything is done")
            const recordedPlan = JSON.parse(recordedRun("install", "plan").stdout.join("\n"))
            compare(sheet.title, "Install Chromium B.S.U.?")
            compare(findChild(sheet.contentItem, "planSourceName").text, recordedPlan.source.name)
            compare(findChild(sheet.contentItem, "planVerification").text, recordedPlan.source.verification)
            // Every download, with its size as tur-apps measured it.
            let shown = 0
            for (const item of everything(sheet.contentItem)) {
                if (item.objectName === "planDownload" && item.visible)
                    shown++
            }
            compare(shown, recordedPlan.downloads.length)
            verify(everything(sheet.contentItem).some(item => item.objectName === "planDownload" && item.visible
                                                      && item.text.indexOf(Words.size(recordedPlan.downloads[0].size)) > 0),
                   "the download's size is shown")
            // What it may reach, in the centre's words for each kind.
            let reach = 0
            for (const item of everything(sheet.contentItem)) {
                if (item.objectName === "planAccessItem" && item.visible)
                    reach++
            }
            compare(reach, recordedPlan.access.length)
            compare(findChild(sheet.contentItem, "planRemovalText").text, recordedPlan.removal.text)
            verify(everything(sheet.contentItem).some(item => item.visible && item.text === "Open source."))
            // Nothing has run yet.
            compare(operation.step, "")

            // Hold the installation at flatpak's first line, to see it.
            host.apps.holdAfter(host.linesBeforeActivity("install"))
            mouseClick(findChild(sheet.contentItem, "planInstall"))
            tryVerify(() => operation.phase === AppsOperation.Running && operation.activity.length > 0, 5000)
            verify(!primary.visible, "progress replaces the button")
            const progress = findChild(view, "operationProgress")
            verify(progress.visible)
            compare(progress.label, "Downloading and installing")
            // flatpak gives no figures: no bar with numbers is invented.
            verify(progress.indeterminate)
            compare(progress.detail, "")
            compare(findChild(view, "operationActivity").text, operation.activity)
            // flatpak is still installing: cancelling is safe, and offered.
            const cancel = findChild(view, "operationCancel")
            verify(cancel.visible && cancel.enabled)
            compare(cancel.text, "Cancel")

            host.apps.release()
            tryVerify(() => operation.phase === AppsOperation.Succeeded, 10000)
            compare(operation.result.id, app)
            settle()
            compare(findChild(view, "operationDone").text, "Chromium B.S.U. is installed.")
            tryCompare(primary, "text", "Open")
            verify(primary.visible && primary.enabled)
            // "On this computer", from the list after the installation.
            const listed = host.apps.installedEntry(app)
            compare(listed.state, "installed")
            verify(findChild(view, "applicationOnThisComputer").visible)
            compare(findChild(view, "installedFrom").text, listed.from)

            mouseClick(primary)
            compare(host.apps.launched(), [app])
            compare(view.launchFailure.code, undefined)
        }

        // Every item under `item`, itself included.
        function everything(item) {
            const out = [item]
            for (let i = 0; i < item.children.length; i++)
                out.push(...everything(item.children[i]))
            return out
        }

        function test_a_failure_is_said_with_its_code_and_the_page_is_as_before() {
            const view = openPage("failure", app)
            const operation = plan(view)
            compare(operation.phase, AppsOperation.Planned)
            tryVerify(() => view.planSheet.opened, 2000)
            mouseClick(findChild(view.planSheet.contentItem, "planInstall"))
            tryVerify(() => operation.phase === AppsOperation.Failed, 10000)
            const error = lastEvent(recordedRun("failure", "install")).error
            compare(operation.failure.code, error.code)
            compare(operation.failure.reason, error.message)
            const message = findChild(view, "operationFailure").item
            verify(message !== null)
            // The page is as it was before the attempt: Install is there.
            const primary = findChild(view, "applicationPrimary")
            verify(primary.visible)
            compare(primary.text, "Install")
            verify(!findChild(view, "applicationOnThisComputer").visible)
            // Close forgets it.
            mouseClick(findChild(view, "operationClose"))
            tryVerify(() => host.apps.operation(app) === null, 2000)
        }

        function test_cancel_takes_everything_back() {
            const view = openPage("cancel", app)
            const operation = plan(view)
            tryVerify(() => view.planSheet.opened, 2000)
            mouseClick(findChild(view.planSheet.contentItem, "planInstall"))
            // The recording stops where it was cancelled: flatpak is at work.
            tryVerify(() => operation.phase === AppsOperation.Running && operation.cancellable
                      && operation.step === "install", 5000)
            const cancel = findChild(view, "operationCancel")
            mouseClick(cancel)
            verify(operation.cancelRequested)
            compare(cancel.text, "Cancelling…")
            verify(!cancel.enabled, "asked once")
            tryVerify(() => operation.phase === AppsOperation.Cancelled, 10000)
            compare(findChild(view, "operationDone").text, "Cancelled. Nothing was installed.")
            compare(operation.failure.code, "TUR-DESKTOP-APPS-013")
            // Nothing is listed as installed.
            host.apps.refresh(false)
            settle()
            compare(host.apps.installedEntry(app).id, undefined)
            compare(findChild(view, "applicationPrimary").text, "Install")
        }

        function test_remove_asks_one_question_and_keeps_the_data_by_default() {
            const view = openPage("remove", app)
            tryVerify(() => view.installed, 5000)
            compare(findChild(view, "applicationPrimary").text, "Open")
            const card = findChild(view, "applicationOnThisComputer")
            verify(card.visible)
            mouseClick(findChild(card, "installedRemoveButton"))
            const sheet = view.removalSheet
            tryVerify(() => sheet.opened, 2000)
            compare(sheet.title, "Remove Chromium B.S.U.?")
            const keep = findChild(sheet.contentItem, "removeKeep")
            const remove = findChild(sheet.contentItem, "removeDelete")
            compare(keep.text, "Keep its settings and documents")
            compare(remove.text, "Remove its settings and data too")
            verify(keep.checked, "keeping is chosen")
            verify(!remove.checked)
            // Neither button has the keyboard when the question opens.
            verify(!findChild(sheet.contentItem, "removeConfirm").activeFocus)
            verify(!findChild(sheet.contentItem, "removeCancel").activeFocus)
            mouseClick(findChild(sheet.contentItem, "removeConfirm"))
            const operation = host.apps.operation(app)
            compare(operation.kind, AppsOperation.Remove)
            verify(!operation.deleteData)
            // The recorded run is `remove --keep-data`: a request for anything
            // else would not be answered.
            tryVerify(() => operation.phase !== AppsOperation.Running, 10000)
            compare(operation.phase, AppsOperation.Succeeded, JSON.stringify(operation.failure))
            compare(findChild(view, "operationDone").text,
                    "Chromium B.S.U. is removed. Its settings and documents were kept.")
            settle()
            compare(host.apps.installedEntry(app).id, undefined)
            compare(findChild(view, "applicationPrimary").text, "Install")
        }

        function test_updates_lists_what_has_an_update_and_updates_it() {
            const shell = host.start("default", "tur-centre:updates", "update")
            settle()
            verify(host.apps.updatesChecked)
            const view = page(shell)
            const listed = host.apps.installedEntry(app)
            verify(listed.id !== undefined, "the list holds the application")
            const result = lastEvent(recordedRun("update", "update"))
            if (listed.update.available !== true) {
                // Flathub had no earlier commit on the day of the recording.
                verify(findChild(view, "updatesNone").visible)
                compare(result.event, "result")
                return
            }
            const row = findChild(view, "updatesRow-" + app)
            verify(row !== null && row.visible)
            compare(findChild(row, "installedRowUpdate").text, Words.updateState(listed.update))
            mouseClick(findChild(row, "installedRowUpdateButton"))
            const operation = host.apps.operation(app)
            compare(operation.kind, AppsOperation.Update)
            tryVerify(() => operation.phase !== AppsOperation.Running, 10000)
            compare(operation.phase, AppsOperation.Succeeded, JSON.stringify(operation.failure))
            compare(operation.result.updated, result.result.updated)
            settle()
            compare(host.apps.installedEntry(app).update.available, false)
        }

        function test_sources_switch_flathub_on_for_this_person() {
            const shell = host.start("default", "tur-centre:sources", "sources")
            settle()
            const view = page(shell)
            verify(findChild(view, "sourcesDefault").text.indexOf("Flathub is on for everyone") > 0)
            const row = findChild(view, "source-flathub")
            verify(row !== null)
            compare(findChild(row, "sourceState").text, "Not added")
            const toggle = findChild(row, "sourceSwitch")
            verify(!toggle.checked)
            mouseClick(toggle)
            settle()
            const flathub = host.apps.sources.find(source => source.name === "flathub")
            compare(flathub.state, "on")
            compare(findChild(page(shell), "source-flathub").children.length > 0, true)
        }

        function test_a_plan_refused_because_flathub_is_off_offers_to_switch_it_on() {
            const view = openPage("sources", app)
            let operation = plan(view)
            compare(operation.phase, AppsOperation.Failed)
            compare(operation.failure.code, "TUR-DESKTOP-APPS-008")
            const switchOn = findChild(view, "operationSwitchOn")
            verify(switchOn.visible)
            compare(switchOn.text, "Switch on Flathub and try again")
            // The message has just appeared: let its row of buttons be laid
            // out before one of them is pressed.
            waitForRendering(view)
            mouseClick(switchOn)
            // Switched on, then the plan again: the recorded second plan.
            tryVerify(() => host.apps.sourceChanging === "" && host.apps.sourcesStatus === Apps.Ready, 5000,
                      "the source was switched and listed again")
            compare(JSON.stringify(host.apps.sourcesFailure), "{}")
            // (Which list of sources answers here is the recording's
            // order, not the program's: the first recorded list, made
            // before Flathub was switched on, has not been asked for yet.)
            const again = host.apps.operation(app)
            verify(again !== null)
            tryVerify(() => again.phase !== AppsOperation.Planning, 5000)
            compare(again.phase, AppsOperation.Planned, JSON.stringify(again.failure))
            tryVerify(() => view.planSheet.opened, 2000)
        }

        function test_i_have_the_installer_file() {
            const view = openPage("installer", "org.notepad-plus-plus.notepad-plus-plus")
            // A free installer TUR downloads from its publisher: "Install".
            compare(findChild(view, "applicationPrimary").text, "Install")
            const button = findChild(view, "applicationInstallerFile")
            verify(button.visible)
            compare(button.text, "I have the installer file")
            // The file chooser hands its path on; the same flow, from the
            // hash check, with --installer.
            view.chooseInstaller("/home/someone/Downloads/npp-installer.exe")
            const operation = host.apps.operation("org.notepad-plus-plus.notepad-plus-plus")
            compare(operation.installerFile, "/home/someone/Downloads/npp-installer.exe")
            tryVerify(() => operation.phase !== AppsOperation.Planning, 5000)
            const answer = JSON.parse(recorded("installer")[0].stdout.join("\n"))
            if (answer.error) {
                compare(operation.phase, AppsOperation.Failed)
                compare(operation.failure.code, answer.error.code)
                compare(operation.failure.reason, answer.error.message)
            } else {
                compare(operation.phase, AppsOperation.Planned)
            }
        }

        function test_chrome_says_where_it_comes_from_before_anything_is_done() {
            const shell = host.start("default", "tur-centre:home", "chrome")
            settle()
            mouseClick(findChild(shell, "homeChromeInstall"))
            compare(shell.view, "application")
            settle()
            const view = page(shell)
            verify(findChild(view, "applicationChromeOrigin").visible)
            const operation = host.apps.operation("com.google.chrome")
            tryVerify(() => operation.phase === AppsOperation.Planned, 5000)
            tryVerify(() => view.planSheet.opened, 2000)
            const origin = findChild(view.planSheet.contentItem, "planChromeOrigin")
            verify(origin.visible)
            verify(origin.text.indexOf("Chrome is made by Google") === 0)
            verify(everything(view.planSheet.contentItem).some(item => item.visible
                                                               && item.text === "Proprietary: the publisher's terms apply."))
            verify(everything(view.planSheet.contentItem).some(item => item.visible
                                                               && item.text === "Packaged by volunteers, not by its publisher."))
            // Cancel: nothing is installed.
            mouseClick(findChild(view.planSheet.contentItem, "planCancel"))
            tryVerify(() => host.apps.operation("com.google.chrome") === null, 2000)
        }

        function test_installed_lists_every_route_with_its_size_and_update_state() {
            const shell = host.start("default", "tur-centre:installed", "remove")
            settle()
            const view = page(shell)
            const listed = host.apps.installedEntry(app)
            const row = findChild(view, "installedRow-" + app)
            verify(row !== null && row.visible)
            compare(findChild(row, "installedRowName").text, listed.name)
            compare(findChild(row, "installedRowRoute").text, "Linux application")
            verify(findChild(row, "installedRowFacts").text.indexOf(Words.size(listed.size)) > 0)
            compare(findChild(row, "installedRowUpdate").text, Words.updateState(listed.update))
            mouseClick(findChild(row, "installedRowDetails"))
            compare(shell.view, "application")
            compare(host.catalogue.application.applicationId, app)
            // The way back leads to Installed.
            compare(findChild(shell, "shellBack").text, "Installed")
            mouseClick(findChild(shell, "shellBack"))
            compare(shell.view, "installed")
        }
    }
}

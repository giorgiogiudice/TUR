// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Installer
import Tur.Kit as Kit

// What the installer must and must not do, on the engine's recorded answers:
//
//  - the whole way through, with the keyboard, onto the blank disk;
//  - the medium the installer runs from is listed, cannot be chosen, and the
//    keyboard passes it by;
//  - the confirmation: nothing has the keyboard but the field; the button is
//    enabled only by the exact phrase; a wrong phrase, the right one in lower
//    case and a pasted one are refused; Enter does nothing;
//  - the Windows disk shows the Windows and BitLocker warning, the blank one
//    does not;
//  - "Cancel" while the engine has written nothing, and the sentence instead
//    once it writes;
//  - a failure shows its code, what state the disk is in, and the way to save
//    the log to a USB stick; a refusal before writing leads back to the disks;
//  - beside Windows (ADR-0029): offered only on a disk that holds Windows and
//    has room, with the reason when not; its own phrase, which the erase
//    phrase does not match; a failure says Windows was not written to.
Item {
    id: root

    width: 1180
    height: 900

    WizardHost {
        id: host

        anchors.fill: parent
    }

    TextEdit {
        id: clipboardSource

        visible: false
    }

    TestCase {
        name: "InstallerWizard"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            wait(0)
        }

        function state(name) {
            for (const item of host.states) {
                if (item.name === name)
                    return item
            }
            throw new Error("no state " + name)
        }

        function enter(name) {
            const wizard = host.enter(state(name))
            tryVerify(() => host.ready, 10000, "the state " + name + " was reached")
            wait(50)
            return wizard
        }

        function control(wizard, name) {
            const item = findChild(wizard, name)
            verify(item !== null, "the screen has " + name)
            return item
        }

        // The tab stop that has the keyboard, by its name.
        function focusName() {
            for (let at = root.Window.activeFocusItem; at !== null; at = at.parent) {
                if (at.objectName.length > 0)
                    return at.objectName
            }
            return ""
        }

        // Tab until `name` has the keyboard, as tests/iso does in a virtual
        // machine; then the control is pressed with Space.
        function tabTo(name) {
            for (let press = 0; press < 40; press++) {
                if (focusName() === name)
                    return
                keyClick(Qt.Key_Tab)
            }
            fail("Tab never reached " + name)
        }

        function press(name) {
            tabTo(name)
            keyClick(Qt.Key_Space)
            wait(20)
        }

        function test_the_whole_way_with_the_keyboard() {
            const wizard = enter("welcome")
            compare(focusName(), "continueButton", "Continue has the keyboard on the first screen")
            press("continueButton")
            tryCompare(wizard, "step", "computer")
            tryCompare(host.engine, "checkState", "done", 10000)
            tryVerify(() => focusName() === "continueButton", 5000, "Continue has the keyboard once the checks pass")
            press("continueButton")
            tryCompare(wizard, "step", "disk")
            tryCompare(host.engine, "disksState", "done", 10000)
            press("disk-vda")
            compare(wizard.chosenDisk.name, "vda")
            press("continueButton")
            tryCompare(wizard, "step", "account")
            press("continueButton")
            tryCompare(wizard, "step", "confirm")
            compare(focusName(), "phraseField", "the field has the keyboard: there is no default button")
            for (const key of "ERASE BOOTTEST")
                keyClick(key === " " ? Qt.Key_Space : key, key === " " ? Qt.NoModifier : Qt.ShiftModifier)
            verify(control(wizard, "eraseButton").enabled, "the exact phrase enables the button")
            press("eraseButton")
            tryCompare(wizard, "step", "done", 10000)
            compare(host.engine.installState, "success")
            verify(control(wizard, "removeMedium").visible, "the result says to remove the medium")
            compare(focusName(), "restartButton")
            press("restartButton")
            compare(host.engine.powerRequest, "reboot")
        }

        function test_the_medium_cannot_be_chosen() {
            const wizard = enter("disks")
            const medium = control(wizard, "disk-sda")
            verify(!medium.enabled, "the medium the installer runs from cannot be chosen")
            verify(!medium.activeFocusOnTab || !medium.enabled)
            mouseClick(medium)
            verify(wizard.chosenDisk.name !== "sda", "clicking it chooses nothing")
            verify(!control(wizard, "continueButton").enabled, "nothing chosen, nothing to continue with")
            // Tab goes round every control and never reaches it.
            const seen = []
            for (let press = 0; press < 30; press++) {
                keyClick(Qt.Key_Tab)
                seen.push(focusName())
            }
            verify(seen.indexOf("disk-sda") < 0, "Tab never reaches the medium: " + seen.join(", "))
            verify(seen.indexOf("disk-vda") >= 0 && seen.indexOf("disk-vdb") >= 0, "Tab reaches both disks: " + seen.join(", "))
        }

        function test_nothing_but_the_exact_phrase() {
            const wizard = enter("confirm-windows")
            const field = control(wizard, "phraseField")
            const erase = control(wizard, "eraseButton")
            compare(focusName(), "phraseField")
            verify(!erase.enabled, "nothing typed: the button is unavailable")
            verify(control(wizard, "windowsWarning").visible, "the Windows disk has the Windows warning")

            keyClick(Qt.Key_W, Qt.ShiftModifier)
            keyClick(Qt.Key_R, Qt.ShiftModifier)
            keyClick(Qt.Key_O, Qt.ShiftModifier)
            keyClick(Qt.Key_N, Qt.ShiftModifier)
            keyClick(Qt.Key_G, Qt.ShiftModifier)
            verify(!erase.enabled, "a wrong phrase does not enable it")

            field.text = "erase owsdiska"
            verify(!erase.enabled, "the right phrase in lower case does not enable it")

            field.text = "ERASE OWSDISKA "
            verify(!erase.enabled, "the phrase with a space after it does not enable it")

            // Pasted: refused.
            field.text = ""
            clipboardSource.text = "ERASE OWSDISKA"
            clipboardSource.selectAll()
            clipboardSource.copy()
            field.forceActiveFocus()
            keySequence(StandardKey.Paste)
            compare(field.text, "", "pasting is refused")
            verify(!erase.enabled)

            // Typed, and Enter does not press the button.
            field.text = "ERASE OWSDISKA"
            verify(erase.enabled, "the exact phrase enables the button")
            keyClick(Qt.Key_Return)
            keyClick(Qt.Key_Enter)
            wait(50)
            compare(wizard.step, "confirm", "Enter erases nothing")
            compare(host.engine.installState, "idle", "nothing was asked of the engine")
        }

        function test_no_windows_warning_for_the_blank_disk() {
            const wizard = enter("confirm-blank")
            verify(!control(wizard, "windowsWarning").visible)
            compare(findChild(wizard, "confirmPhrase").text, "ERASE BOOTTEST")
            verify(findChild(wizard, "confirmPath").text.indexOf("/dev/disk/by-id/virtio-tur-boot-test") >= 0,
                   "the stable path is shown")
            verify(findChild(wizard, "confirmSerial").text.indexOf("tur-boot-test") >= 0, "the serial number is shown")
        }

        function test_cancel_before_writing() {
            const wizard = enter("installing-cancellable")
            verify(host.engine.cancellable)
            const cancel = control(wizard, "cancelButton")
            verify(cancel.visible)
            compare(focusName(), "cancelButton")
            keyClick(Qt.Key_Space)
            tryCompare(wizard, "step", "done", 5000)
            compare(host.engine.installState, "cancelled")
            compare(host.engine.result.disk_state, "untouched")
            verify(control(wizard, "backToDisksButton").visible)
            press("backToDisksButton")
            tryCompare(wizard, "step", "disk")
            compare(host.engine.installState, "idle")
        }

        function test_no_cancel_once_writing() {
            const wizard = enter("installing")
            verify(!host.engine.cancellable, "the disk is being written")
            verify(!control(wizard, "cancelButton").visible, "Cancel is not offered")
            verify(control(wizard, "cancelUnavailable").visible, "the sentence says why not")
            host.engine.cancel()
            verify(host.engine.cancelRefused)
            compare(host.engine.installState, "running", "asking anyway stops nothing")
        }

        function test_a_failure_says_what_happened() {
            const wizard = enter("done-failed")
            const message = control(wizard, "failureMessage")
            verify(message.visible)
            compare(message.code, "TUR-INSTALL-MEDIUM-002")
            verify(message.whatToDo.indexOf("partly written") >= 0, message.whatToDo)
            verify(!control(wizard, "backToDisksButton").visible, "an erased disk is not a reason to go back")
            compare(focusName(), "saveLogButton", "saving the log has the keyboard")
            press("saveLogButton")
            tryCompare(host.engine, "sticksState", "done", 5000)
            compare(host.engine.sticks.length, 1)
            host.engine.saveLog(host.engine.sticks[0].device)
            tryCompare(host.engine, "saveState", "done", 5000)
            verify(String(host.engine.saveResult.folder).indexOf("tur-installer-log-") === 0)
        }

        function test_a_refusal_leads_back() {
            const wizard = enter("done-refused")
            const message = control(wizard, "failureMessage")
            compare(message.code, "TUR-INSTALL-CONFIRM-004")
            verify(message.whatToDo.indexOf("Nothing was changed") >= 0, message.whatToDo)
            verify(control(wizard, "backToDisksButton").visible)
        }

        function test_a_blocking_check_stops_here() {
            const wizard = enter("computer-blocked")
            compare(host.engine.blocking.length, 2)
            verify(!control(wizard, "continueButton").enabled, "Continue stays unavailable")
        }

        function test_no_disk_to_install_on() {
            const wizard = enter("disks-none")
            compare(host.engine.selectable, 0)
            verify(!control(wizard, "continueButton").enabled)
            verify(!control(wizard, "disk-vda").enabled, "the small disk cannot be chosen")
        }

        function test_beside_windows_only_on_a_disk_that_holds_windows() {
            const wizard = enter("disks-chosen")
            verify(control(wizard, "eraseOption").selected)
            verify(!control(wizard, "besideWindowsOption").enabled, "the blank disk holds no Windows")
            verify(control(wizard, "besideExplanation").text.indexOf("Only on a disk that holds Windows") >= 0)
        }

        function test_beside_windows_chosen_with_the_keyboard() {
            const wizard = enter("disks-windows-chosen")
            const beside = control(wizard, "besideWindowsOption")
            verify(beside.enabled, "the Windows disk with free space offers it")
            verify(control(wizard, "eraseOption").selected, "erasing stays the default")
            compare(wizard.mode, "erase")
            press("besideWindowsOption")
            compare(wizard.mode, "beside")
            verify(beside.selected)
            verify(!control(wizard, "eraseOption").selected)
            verify(beside.subtitle.indexOf("30.0 GiB") >= 0, beside.subtitle)
            // Which system starts by default: TUR unless the person chooses Windows.
            verify(control(wizard, "defaultTurOption").selected)
            compare(wizard.defaultSystem, "tur")
            press("defaultWindowsOption")
            compare(wizard.defaultSystem, "windows")
            verify(control(wizard, "defaultWindowsOption").selected)
            press("defaultTurOption")
            compare(wizard.defaultSystem, "tur")
            press("eraseOption")
            compare(wizard.mode, "erase", "and back to erasing")
        }

        function test_beside_windows_not_possible_says_why() {
            const wizard = enter("disks-beside-not-possible")
            verify(!control(wizard, "besideWindowsOption").enabled, "no free space: not possible")
            const why = control(wizard, "besideExplanation").text
            verify(why.indexOf("Shrink Volume") >= 0, "says how to make room: " + why)
            compare(wizard.mode, "erase")
        }

        function test_confirm_beside_windows() {
            const wizard = enter("confirm-beside")
            const field = control(wizard, "phraseField")
            const install = control(wizard, "eraseButton")
            compare(focusName(), "phraseField", "the field has the keyboard: there is no default button")
            compare(findChild(wizard, "confirmPhrase").text, "BESIDE BOOTTEST")
            compare(install.text, "Install beside Windows")
            verify(control(wizard, "besideNothingDestroyed").visible, "it says nothing on the disk is destroyed")
            verify(control(wizard, "besideNewPartition").text.indexOf("30.0 GiB") >= 0)
            verify(control(wizard, "besideDefaultSystem").text.indexOf("starts TUR by default") >= 0)
            verify(!control(wizard, "windowsWarning").visible, "no BitLocker on this disk")
            field.text = "ERASE BOOTTEST"
            verify(!install.enabled, "the erase phrase does not confirm installing beside Windows")
            field.text = "beside boottest"
            verify(!install.enabled, "nor the phrase in lower case")
            field.text = ""
            for (const key of "BESIDE BOOTTEST")
                keyClick(key === " " ? Qt.Key_Space : key, key === " " ? Qt.NoModifier : Qt.ShiftModifier)
            compare(field.text, "BESIDE BOOTTEST")
            verify(install.enabled, "the exact phrase enables the button")
            keyClick(Qt.Key_Return)
            wait(50)
            compare(wizard.step, "confirm", "Enter installs nothing")
            press("eraseButton")
            tryCompare(wizard, "step", "done", 10000)
            compare(host.engine.installState, "success")
            compare(host.engine.result.mode, "beside")
            verify(host.engine.stagesSeen.indexOf("boot-menu") >= 0, "the boot-menu stage was seen")
            verify(control(wizard, "windowsAfterInstall").visible, "the result says how to start Windows")
        }

        function test_shrink_windows_with_a_size_that_keeps_the_minimum() {
            const wizard = enter("disks-beside-not-possible")
            const shrink = control(wizard, "shrinkWindowsOption")
            verify(shrink.enabled, "no free space, but Windows can be shrunk")
            press("shrinkWindowsOption")
            compare(wizard.mode, "shrink")
            compare(wizard.turGiB, 25, "the smallest room TUR needs is proposed")
            verify(control(wizard, "shrinkBackupNote").visible, "the backup is recommended")
            verify(control(wizard, "continueButton").enabled)
            wizard.turGiB = 40
            verify(!control(wizard, "continueButton").enabled, "more than Windows can give: no going on")
            verify(control(wizard, "turSizeField").errorText.length > 0)
            wizard.turGiB = 24
            verify(!control(wizard, "continueButton").enabled, "less than TUR needs: no going on")
            wizard.turGiB = 30
            verify(control(wizard, "continueButton").enabled)
            verify(control(wizard, "defaultTurOption").visible, "the default system is asked here too")
            verify(!control(wizard, "shrinkExplanation").visible, "nothing to explain when it is possible")
        }

        function test_shrink_refused_says_why() {
            const wizard = enter("disks-windows-chosen")
            verify(!control(wizard, "shrinkWindowsOption").enabled, "Windows' partition is too full to give 25 GiB")
            const why = control(wizard, "shrinkExplanation")
            verify(why.visible, "the reason is on the screen")
            verify(why.text.indexOf("too full") >= 0, why.text)
        }

        function test_confirm_shrink_windows() {
            const wizard = enter("confirm-shrink")
            const field = control(wizard, "phraseField")
            const install = control(wizard, "eraseButton")
            compare(findChild(wizard, "confirmPhrase").text, "SHRINK OWSDISKA")
            verify(control(wizard, "shrinkBackupWarning").visible, "a backup is strongly recommended")
            verify(control(wizard, "shrinkWindowsLine").text.indexOf("ntfsresize") >= 0)
            compare(install.text, "Shrink Windows and install")
            field.text = "ERASE OWSDISKA"
            verify(!install.enabled, "the erase phrase does not confirm shrinking")
            field.text = "BESIDE OWSDISKA"
            verify(!install.enabled, "nor the beside phrase")
            field.text = ""
            for (const key of "SHRINK OWSDISKA")
                keyClick(key === " " ? Qt.Key_Space : key, key === " " ? Qt.NoModifier : Qt.ShiftModifier)
            verify(install.enabled)
            press("eraseButton")
            tryCompare(wizard, "step", "done", 10000)
            compare(host.engine.installState, "success")
            compare(host.engine.result.mode, "shrink")
            verify(host.engine.stagesSeen.indexOf("shrink") >= 0, "the shrink stage was seen")
        }

        function test_shrink_failure_says_let_windows_check() {
            const wizard = enter("done-shrink-failed")
            compare(host.engine.result.disk_state, "windows-check")
            verify(control(wizard, "failureMessage").whatToDo.indexOf("let it check its disk") >= 0)
        }

        function test_beside_windows_failure_keeps_windows() {
            const wizard = enter("done-beside-failed")
            compare(host.engine.result.disk_state, "partial")
            const message = control(wizard, "failureMessage")
            verify(message.whatToDo.indexOf("Windows' partitions were not written to") >= 0, message.whatToDo)
            verify(!control(wizard, "backToDisksButton").visible, "the disk was written: no going back")
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// How each control behaves: with the pointer, with the keyboard, and what it
// tells the accessibility layer.
Item {
    id: root

    width: 640
    height: 480

    Component {
        id: buttonComponent

        Kit.Button {
            text: "Install"
        }
    }

    Component {
        id: checkBoxComponent

        Kit.CheckBox {
            text: "Encrypt this disk"
        }
    }

    Component {
        id: switchComponent

        Kit.Switch {
            text: "Night light"
        }
    }

    Component {
        id: fieldComponent

        Kit.TextField {
            label: "Computer name"
        }
    }

    Component {
        id: rowComponent

        Kit.ListRow {
            width: 300
            text: "Storage"
            subtitle: "Two disks"
        }
    }

    Component {
        id: bannerComponent

        Kit.Banner {
            width: 600
            severity: Kit.Banner.Negative
            text: "The update was not applied."
            actionText: "Try again"
        }
    }

    Component {
        id: progressComponent

        Kit.ProgressBar {
            width: 300
            label: "Copying the system image"
            detail: "2.1 GiB of 5.4 GiB"
            value: 0.5
        }
    }

    Component {
        id: busyComponent

        Kit.BusyIndicator {
        }
    }

    Component {
        id: sheetComponent

        Kit.Sheet {
            title: "Erase this disk?"

            Kit.Label {
                text: "Everything on it will be removed."
            }

            buttons: [
                Kit.Button {
                    objectName: "sheetBack"
                    text: "Back"
                    focus: true
                },
                Kit.Button {
                    objectName: "sheetErase"
                    text: "Erase disk and install"
                    kind: Kit.Button.Destructive
                }
            ]
        }
    }

    SignalSpy {
        id: spy
    }

    TestCase {
        name: "Controls"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function init() {
            spy.clear()
            Kit.Theme.light = false
        }

        function watch(target, signalName) {
            spy.target = target
            spy.signalName = signalName
            verify(spy.valid, "the control has a signal named " + signalName)
        }

        function test_button_pointer_and_keyboard() {
            const button = createTemporaryObject(buttonComponent, root)
            watch(button, "clicked")
            mouseClick(button)
            compare(spy.count, 1)

            button.forceActiveFocus(Qt.TabFocusReason)
            verify(button.activeFocus)
            verify(button.visualFocus, "a button focused from the keyboard shows it")
            keyClick(Qt.Key_Space)
            compare(spy.count, 2, "Space presses the focused button")

            button.enabled = false
            mouseClick(button)
            compare(spy.count, 2, "an unavailable button does nothing")
        }

        function test_button_sizes_and_colours_data() {
            return [
                { tag: "dark", light: false },
                { tag: "light", light: true }
            ]
        }

        function test_button_sizes_and_colours(data) {
            Kit.Theme.light = data.light
            const button = createTemporaryObject(buttonComponent, root)
            verify(button.height >= Kit.Theme.targetPointer, "a button is at least 36 px high")
            verify(button.width >= Kit.Theme.targetMinimum)

            button.kind = Kit.Button.Primary
            verify(Qt.colorEqual(button.background.color, Kit.Theme.accent), "primary is the system's blue")
            verify(Qt.colorEqual(button.contentItem.color, Kit.Theme.accentOn))

            button.kind = Kit.Button.Destructive
            verify(Qt.colorEqual(button.contentItem.color, Kit.Theme.negative))

            button.enabled = false
            verify(Qt.colorEqual(button.contentItem.color, Kit.Theme.textDisabled))
        }

        function test_button_accessible() {
            const button = createTemporaryObject(buttonComponent, root)
            const seen = Testing.Harness.accessible(button)
            verify(seen.valid)
            compare(seen.name, "Install")
            compare(seen.role, Accessible.Button)
        }

        function test_checkbox() {
            const box = createTemporaryObject(checkBoxComponent, root)
            verify(box.height >= Kit.Theme.targetPointer)
            verify(!box.checked)
            mouseClick(box)
            verify(box.checked)
            box.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            verify(!box.checked, "Space toggles the focused check box")

            box.checked = true
            const seen = Testing.Harness.accessible(box)
            compare(seen.name, "Encrypt this disk")
            compare(seen.role, Accessible.CheckBox)
            verify(seen.checkable)
            verify(seen.checked)
        }

        function test_switch() {
            const item = createTemporaryObject(switchComponent, root)
            verify(item.height >= Kit.Theme.targetPointer)
            verify(!item.checked)
            item.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            verify(item.checked, "Space toggles the focused switch")
            mouseClick(item)
            verify(!item.checked)

            const seen = Testing.Harness.accessible(item)
            compare(seen.name, "Night light")
            verify(seen.checkable)
            verify(!seen.checked)
        }

        function test_text_field() {
            const field = createTemporaryObject(fieldComponent, root)
            field.forceActiveFocus(Qt.TabFocusReason)
            verify(field.activeFocus)
            keyClick(Qt.Key_S)
            keyClick(Qt.Key_T)
            compare(field.text, "st")

            let seen = Testing.Harness.accessible(field)
            compare(seen.name, "Computer name", "the label is the field's name")
            compare(seen.role, Accessible.EditableText)

            const before = field.height
            verify(!field.invalid)
            field.errorText = "That name is already in use."
            verify(field.invalid)
            verify(field.height > before, "the reason is shown under the field")
            seen = Testing.Harness.accessible(field)
            compare(seen.description, "That name is already in use.")
        }

        function test_list_row() {
            const row = createTemporaryObject(rowComponent, root)
            verify(row.height >= Kit.Theme.targetPointer)
            watch(row, "clicked")
            mouseClick(row)
            compare(spy.count, 1)
            row.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(spy.count, 2)

            verify(Qt.colorEqual(row.titleColor, Kit.Theme.textPrimary))
            row.selected = true
            verify(Qt.colorEqual(row.background.color, Kit.Theme.selection))
            verify(Qt.colorEqual(row.titleColor, Kit.Theme.selectionOn))
            row.onOverlay = true
            verify(Qt.colorEqual(row.background.color, Kit.Theme.accent),
                   "on a sheet or a menu the current row is marked with the accent")
            verify(Qt.colorEqual(row.titleColor, Kit.Theme.accentOn))

            const seen = Testing.Harness.accessible(row)
            compare(seen.name, "Storage")
            compare(seen.role, Accessible.ListItem)
        }

        function test_banner() {
            const banner = createTemporaryObject(bannerComponent, root)
            watch(banner, "triggered")
            const action = findChild(banner, "bannerAction")
            verify(action !== null)
            verify(action.visible)
            mouseClick(action)
            compare(spy.count, 1)

            const seen = Testing.Harness.accessible(banner)
            compare(seen.name, "Error: The update was not applied.", "the severity is a word, not only a colour")
            compare(seen.role, Accessible.AlertMessage)

            banner.actionText = ""
            verify(!action.visible)
        }

        function test_progress() {
            const bar = createTemporaryObject(progressComponent, root)
            const seen = Testing.Harness.accessible(bar)
            compare(seen.name, "Copying the system image")
            compare(seen.description, "2.1 GiB of 5.4 GiB")
            compare(seen.role, Accessible.ProgressBar)
            fuzzyCompare(bar.position, 0.5, 0.0001)
            bar.indeterminate = true
            verify(bar.indeterminate)
        }

        function test_busy_indicator() {
            const busy = createTemporaryObject(busyComponent, root)
            verify(busy.running)
            const seen = Testing.Harness.accessible(busy)
            verify(seen.name.length > 0, "a spinner says what is being waited for")
        }

        function test_sheet() {
            const sheet = createTemporaryObject(sheetComponent, root)
            verify(!sheet.visible)
            sheet.open()
            tryCompare(sheet, "opened", true)
            verify(sheet.modal)

            const back = findChild(sheet.sheetItem, "sheetBack")
            const erase = findChild(sheet.sheetItem, "sheetErase")
            verify(back !== null && erase !== null)
            tryCompare(back, "activeFocus", true)

            // The keyboard stays in the sheet: Tab goes round its controls.
            keyClick(Qt.Key_Tab)
            verify(erase.activeFocus)
            keyClick(Qt.Key_Tab)
            verify(back.activeFocus, "Tab does not leave an open sheet")

            keyClick(Qt.Key_Escape)
            tryCompare(sheet, "visible", false)
        }
    }
}

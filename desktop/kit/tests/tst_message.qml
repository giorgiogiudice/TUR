// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The two-layer message (specification 6.3; desktop-experience.md, 5.6).
Item {
    id: root

    width: 800
    height: 700

    Component {
        id: messageComponent

        Kit.Message {
            width: 760
            code: "TUR-STORAGE-NTFS-004"
            title: "The Windows volume was not mounted for writing."
            reason: "The volume appears to be hibernated or in an unsafe state."
            whatToDo: "Start Windows, turn off Fast Startup and hibernation, shut down fully, then try again."
            technicalDetail: "Device: /dev/disk/by-uuid/0f3a-91c2\nDriver: ntfs3\nMount policy: read-only"
            actions: ["Try again", "Open it read-only"]
        }
    }

    SignalSpy {
        id: spy
    }

    TestCase {
        name: "Message"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function init() {
            spy.clear()
        }

        function cleanup() {
            Testing.Harness.pseudo = Testing.Harness.None
        }

        function watch(target, signalName) {
            spy.target = target
            spy.signalName = signalName
            verify(spy.valid, "the message has a signal named " + signalName)
        }

        function test_first_layer() {
            const message = createTemporaryObject(messageComponent, root)
            compare(findChild(message, "messageTitle").text, message.title)
            compare(findChild(message, "messageReason").text, message.reason)
            compare(findChild(message, "messageWhatToDo").text, message.whatToDo)

            const code = findChild(message, "messageCode")
            verify(code.visible, "the code is always visible")
            compare(code.text, "TUR-STORAGE-NTFS-004")
            verify(code.readOnly)
            verify(code.selectByMouse, "the code can be selected and copied")

            verify(!message.expanded, "the technical detail is closed until asked for")
            verify(!findChild(message, "messageDetail").visible)
        }

        function test_detail_opens_in_place() {
            const message = createTemporaryObject(messageComponent, root)
            const disclosure = findChild(message, "messageDisclosure")
            const detail = findChild(message, "messageDetail")
            const closedHeight = message.implicitHeight

            mouseClick(disclosure)
            verify(message.expanded)
            verify(detail.visible)
            tryVerify(() => message.implicitHeight > closedHeight, 1000, "the message grows to show the detail")
            compare(findChild(message, "messageDetailText").text, message.technicalDetail)

            disclosure.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            verify(!message.expanded, "Space closes it again")
            verify(!detail.visible)
        }

        function test_copy_leaves_no_selection() {
            const message = createTemporaryObject(messageComponent, root)
            message.expanded = true
            const copy = findChild(message, "messageCopy")
            verify(copy.visible)
            mouseClick(copy)
            compare(findChild(message, "messageDetailText").selectedText, "")
        }

        function test_actions() {
            const message = createTemporaryObject(messageComponent, root)
            const first = findChild(message, "messageAction0")
            const second = findChild(message, "messageAction1")
            compare(first.text, "Try again", "the safe action is first")
            compare(first.kind, Kit.Button.Primary)
            compare(second.text, "Open it read-only")
            compare(second.kind, Kit.Button.Secondary)
            const firstRect = Testing.Harness.sceneRect(first)
            const secondRect = Testing.Harness.sceneRect(second)
            verify(firstRect.x < secondRect.x, "and it is at the start of the row")

            watch(message, "actionTriggered")
            mouseClick(second)
            compare(spy.count, 1)
            compare(spy.signalArguments[0][0], 1)
        }

        function test_support_bundle_whenever_it_is_an_error() {
            const message = createTemporaryObject(messageComponent, root)
            const bundle = findChild(message, "messageSupportBundle")
            verify(bundle.visible)
            watch(message, "supportBundleRequested")
            mouseClick(bundle)
            compare(spy.count, 1)

            message.severity = Kit.Message.Warning
            verify(!bundle.visible)
            message.severity = Kit.Message.Done
            verify(!bundle.visible)
        }

        function test_pax_only_when_pax_is_on() {
            const message = createTemporaryObject(messageComponent, root)
            const ask = findChild(message, "messageAskPax")
            verify(!ask.visible, "P@x is not offered unless the program says P@x is on")
            message.paxAvailable = true
            verify(ask.visible)
            watch(message, "paxRequested")
            mouseClick(ask)
            compare(spy.count, 1)
        }

        function test_severity_is_a_word() {
            const message = createTemporaryObject(messageComponent, root)
            let seen = Testing.Harness.accessible(message)
            compare(seen.role, Accessible.AlertMessage)
            compare(seen.name, "Error: The Windows volume was not mounted for writing.")
            message.severity = Kit.Message.Warning
            seen = Testing.Harness.accessible(message)
            compare(seen.name, "Warning: The Windows volume was not mounted for writing.")
        }

        function test_at_most_72_characters_wide() {
            const message = createTemporaryObject(messageComponent, root)
            message.reason = "A sentence that is a good deal longer than seventy-two characters, so that it has to be broken into two lines, or more than two."
            const reason = findChild(message, "messageReason")
            verify(message.measure > 0)
            tryVerify(() => reason.width <= message.measure + 1, 1000, "a message is at most 72 characters wide")
            tryVerify(() => reason.lineCount > 1)
        }

        // L8: a code is never translated; the words around it are.
        function test_code_is_never_translated() {
            const message = createTemporaryObject(messageComponent, root)
            const disclosure = findChild(message, "messageDisclosure")
            compare(disclosure.text, "Technical detail")
            Testing.Harness.pseudo = Testing.Harness.RtlLong
            verify(disclosure.text !== "Technical detail", "the kit's own words go through the translator")
            verify(disclosure.text.length > "Technical detail".length)
            compare(findChild(message, "messageCode").text, "TUR-STORAGE-NTFS-004")
        }
    }
}

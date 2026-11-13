// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What every screen of P@x's setup is: a column with its title first and
// its buttons last, the control that has the keyboard when it opens, and a
// place for the service's refusal when it refuses something.
ColumnLayout {
    id: screen

    property string title
    // The control given the keyboard when the screen opens.
    property Item initialFocus: null
    // The service's refusal of what this screen asked it to do.
    property string problem
    property string problemCode

    function refused(reply: var): void {
        screen.problem = String(reply.message || "")
        screen.problemCode = String(reply.code || "")
    }

    function clearProblem(): void {
        screen.problem = ""
        screen.problemCode = ""
    }

    spacing: Kit.Theme.spaceGroup

    Kit.Label {
        objectName: "screenTitle"
        Layout.fillWidth: true
        text: screen.title
        kind: Kit.Label.Display
        wrapMode: Text.Wrap
    }

    SystemCard {
        objectName: "screenProblem"
        Layout.fillWidth: true
        visible: screen.problem.length > 0
        phase: "setting"
        text: screen.problem
        code: screen.problemCode
    }
}

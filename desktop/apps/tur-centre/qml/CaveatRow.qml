// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One caveat of a way of running an application: something the person must
// see before choosing it (desktop-experience.md, section 8.2, "What stands in
// the way"). The sentence is the router's and is shown as it is. The sign
// says how much it matters: a cross for a blocker, a triangle for an obstacle
// or a warning, the information sign for a note. An obstacle also says
// "Probably will not work fully". A screen reader hears the word the sign
// stands for.
RowLayout {
    id: row

    // The router's caveat: code, severity, requirement, text.
    required property var caveat

    readonly property string severity: row.caveat.severity

    spacing: Kit.Theme.spaceRelated

    Kit.Glyph {
        Layout.alignment: Qt.AlignTop
        Layout.topMargin: sentence.halfLeading
        kind: row.severity === "blocker" ? Kit.Glyph.Close
              : row.severity === "note" ? Kit.Glyph.Info : Kit.Glyph.Warning
        tint: row.severity === "blocker" ? Kit.Theme.negative
              : row.severity === "note" ? Kit.Theme.textSecondary : Kit.Theme.warning
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 0

        Kit.Label {
            objectName: "caveatWord"
            Layout.fillWidth: true
            visible: row.severity === "obstacle"
            text: Words.severityLabel("obstacle")
            kind: Kit.Label.Strong
            // Said with the sentence below.
            Accessible.ignored: true
        }

        Kit.Label {
            id: sentence

            objectName: "caveatText"
            Layout.fillWidth: true
            text: row.caveat.text
            paragraph: true
            Accessible.name: Words.spokenCaveat(row.severity, row.caveat.text)
        }
    }
}

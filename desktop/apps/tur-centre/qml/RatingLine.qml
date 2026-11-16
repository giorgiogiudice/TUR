// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// How well it works: the rating's icon and its words, always together
// (desktop-experience.md, section 8.2). "Unknown" is shown as "No reports"
// with its own sign, never as anything better; "Starts" says under it that
// nothing beyond the first window was tested.
RowLayout {
    id: line

    // The catalogue's rating: good, usable, starts, poor, broken or unknown.
    property string rating: "unknown"

    // On a search row the words are in the body style; on a page they stand
    // out.
    property bool strong: true

    spacing: Kit.Theme.spaceRelated

    RatingGlyph {
        Layout.alignment: Qt.AlignTop
        Layout.topMargin: 1
        rating: line.rating
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 0

        Kit.Label {
            objectName: "ratingWords"
            Layout.fillWidth: true
            text: Words.ratingLabel(line.rating)
            kind: line.strong ? Kit.Label.Strong : Kit.Label.Body
        }

        // What the label alone does not say: "Starts" is not "works".
        Kit.Label {
            objectName: "ratingExplanation"
            Layout.fillWidth: true
            visible: text.length > 0
            text: Words.ratingExplanation(line.rating)
            tone: Kit.Label.Secondary
            paragraph: true
        }
    }
}

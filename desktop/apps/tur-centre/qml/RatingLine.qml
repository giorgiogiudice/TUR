// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// How well it works: the rating's icon and its words, always together
// (desktop-experience.md, section 8.2). "Unknown" is shown as "No reports"
// with its own sign, never as anything better.
RowLayout {
    id: line

    // The catalogue's rating: good, usable, poor, broken or unknown.
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

    Kit.Label {
        objectName: "ratingWords"
        Layout.fillWidth: true
        text: Words.ratingLabel(line.rating)
        kind: line.strong ? Kit.Label.Strong : Kit.Label.Body
    }
}

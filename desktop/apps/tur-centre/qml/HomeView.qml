// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Home: a short, honest front page (desktop-experience.md, section 8.1).
//
// Part 1 carries only its standing sentence. "What needs attention" and
// "Bring an application you already have" arrive with installing (D-11) and
// with files (D-12).
//
// The sentence stands "for as long as it is true". The router's answers do
// not say whether the catalogue holds a test of TUR's own, so it is shown
// unconditionally; the test the_home_view_s_standing_sentence_is_still_true,
// in services/tur-catalogue/tests/desktop_fixtures.rs, fails on the day the
// catalogue gains one.
ColumnLayout {
    id: view

    spacing: Kit.Theme.spaceGroup

    Kit.Label {
        Layout.fillWidth: true
        text: qsTr("Home")
        kind: Kit.Label.Title
    }

    Kit.Banner {
        objectName: "homeStandingSentence"
        Layout.fillWidth: true
        severity: Kit.Banner.Info
        text: qsTr("TUR has not tested applications itself yet. Every rating here is what the named source reports.")
    }
}

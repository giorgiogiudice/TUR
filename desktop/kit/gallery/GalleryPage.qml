// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// One page of the gallery: a title and a column of specimens. The screenshot
// job renders each page on its own, so a page shows one family of components
// in every state.
Kit.Page {
    id: page

    property string title
    default property alias specimens: column.data

    // The control the screenshot is taken with keyboard focus on, so that
    // the focus ring is in the picture.
    property Item focusTarget: null

    // The same, for a control inside a component: the objectName of the
    // first item to focus.
    property string focusName

    // The part of the page the keyboard can reach. The page itself, unless a
    // sheet is open and holds the keyboard.
    property Item keyboardScope: page

    // Called before the page is thrown away.
    function retire(): void {
    }

    ColumnLayout {
        id: column

        anchors.fill: parent
        spacing: Kit.Theme.spaceMargin

        Kit.Label {
            Layout.fillWidth: true
            text: page.title
            kind: Kit.Label.Title
        }
    }
}

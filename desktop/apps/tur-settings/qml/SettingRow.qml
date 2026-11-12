// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import Tur.Kit as Kit

// One row of a settings page (desktop-experience.md, 10.3). A row cannot be
// written without its address: the property is required, so a page with a
// row that has none does not load. What the row is called comes from the
// registry entry of that address and from nowhere else.
//
// A row shows its title, then what the system says (`value`), then a
// supporting sentence (`detail`), then, when what backs it is not part of
// the system, why it is not available. A control, if the row has one, goes
// under them.
T.Control {
    id: row

    // "tur.appearance#variant".
    required property string address

    // How code that walks the page tells a row from anything else.
    readonly property bool isSettingRow: true

    // The registry's entry for the address; empty if the registry has none.
    readonly property var entry: Registry.entry(row.address, Registry.revision)
    readonly property string title: row.entry.title !== undefined ? qsTranslate("SettingsRegistry", row.entry.title)
                                                                  : ""
    readonly property string kind: row.entry.kind !== undefined ? row.entry.kind : ""

    // What the system says. Empty for a row that only holds a control.
    property string value

    // The value is an identifier (a version, a revision, a path): shown in
    // the fixed-width face, never translated.
    property bool technical: false

    // A supporting sentence under the title.
    property string detail

    // False when what backs the row is not part of the system: the row then
    // says so, in `unavailableReason`, and its control cannot be used.
    property bool available: true
    property string unavailableReason

    // The row's control, if it has one.
    default property alias controls: slot.data

    // What takes the keyboard when the row is asked for by its address: the
    // row's control, or the row itself when it has none that can be used.
    property Item focusTarget: null

    // Highlighted because it was asked for by its address; `markedByPax`
    // when it was the assistant that asked.
    property bool marked: false
    property bool markedByPax: false

    // What the row says or whether it can be used changed. The window passes
    // it on to whoever is following the row (org.tur.Addressable1.RowChanged).
    signal changed()

    function takeFocus(): void {
        const target = row.focusTarget !== null && row.focusTarget.enabled && row.focusTarget.visible
                       ? row.focusTarget : row
        // What already has the keyboard keeps it. Taking it away first, to
        // give it back "from the keyboard", would hand it for a moment to
        // the card the row stands on, and the page would scroll to show the
        // whole card: a row asked for twice would move.
        if (target.activeFocus)
            return
        // As from the keyboard, so that the control draws its focus ring.
        target.forceActiveFocus(Qt.TabFocusReason)
    }

    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset,
                            implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Kit.Theme.targetPointer, implicitContentHeight + topPadding + bottomPadding)
    onValueChanged: row.changed()
    onDetailChanged: row.changed()
    onAvailableChanged: row.changed()

    Layout.fillWidth: true
    horizontalPadding: 0
    verticalPadding: Kit.Theme.spaceRelated
    // Above the rows beside it while it is highlighted, so that they do not
    // draw over the line.
    z: row.marked ? 1 : 0

    Accessible.role: Accessible.ListItem
    Accessible.name: row.title
    Accessible.description: [row.value, row.detail, row.available ? "" : row.unavailableReason]
                                .filter(text => text.length > 0).join(" ")
    Accessible.focusable: true

    contentItem: ColumnLayout {
        spacing: Kit.Theme.spaceGrid

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGroup

            Kit.Label {
                objectName: "rowTitle"
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                text: row.title
                Accessible.ignored: true
            }

            Kit.Label {
                objectName: "rowValue"
                Layout.maximumWidth: Math.max(160, row.availableWidth * 0.62)
                Layout.alignment: Qt.AlignTop
                visible: row.value.length > 0
                text: row.value
                kind: row.technical ? Kit.Label.Mono : Kit.Label.Body
                tone: Kit.Label.Secondary
                Accessible.ignored: true
            }
        }

        Kit.Label {
            objectName: "rowDetail"
            Layout.fillWidth: true
            visible: row.detail.length > 0
            text: row.detail
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            paragraph: true
            Accessible.ignored: true
        }

        Kit.Label {
            objectName: "rowUnavailable"
            Layout.fillWidth: true
            visible: !row.available && row.unavailableReason.length > 0
            text: row.unavailableReason
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            paragraph: true
            Accessible.ignored: true
        }

        ColumnLayout {
            id: slot

            Layout.fillWidth: true
            Layout.topMargin: Kit.Theme.spaceGrid
            visible: slot.children.length > 0
            spacing: Kit.Theme.spaceRelated
        }
    }

    background: Item {
        Kit.FocusRing {
            // The highlight says where the keyboard is while it is there.
            shown: row.activeFocus && !row.marked
        }

        RowHighlight {
            objectName: "rowHighlight"
            visible: row.marked
            pax: row.markedByPax
        }
    }
}

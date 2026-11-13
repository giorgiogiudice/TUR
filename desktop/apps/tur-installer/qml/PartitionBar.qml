// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// A picture of a disk: one block for each partition, as wide as its share of
// the disk, and the space that no partition uses. A picture only: the words
// are in the list beside it, and a screen reader is given them there. Colour
// is never the only sign: what each block is, is written in that list.
Item {
    id: bar

    // The disk's size in bytes and its partitions, as the engine lists them.
    property real diskSize: 1
    property var partitions: []
    // Draw the layout TUR will write instead: BIOS boot, EFI system, root.
    property bool after: false

    readonly property var blocks: {
        const out = []
        const total = Math.max(1, bar.diskSize)
        if (bar.after) {
            const esp = 512 * 1048576
            const bios = 1048576
            out.push({ share: bios / total, kind: "bios-boot" })
            out.push({ share: esp / total, kind: "esp" })
            out.push({ share: Math.max(0, total - esp - bios - 2 * 1048576) / total, kind: "tur" })
            return out
        }
        // Sorted by where they start; space before, between and after them
        // that no partition uses is shown as free.
        const parts = (bar.partitions || []).slice().sort((a, b) => (a.start || 0) - (b.start || 0))
        let cursor = 0
        for (const part of parts) {
            const start = (part.start || 0) * 512
            if (start > cursor + 16 * 1048576)
                out.push({ share: (start - cursor) / total, kind: "free" })
            out.push({ share: (part.size || 0) / total, kind: part.kind })
            cursor = Math.max(cursor, start + (part.size || 0))
        }
        if (total - cursor > 16 * 1048576)
            out.push({ share: (total - cursor) / total, kind: "free" })
        return out
    }

    function fill(kind: string): color {
        switch (kind) {
        case "tur":
            return Kit.Theme.accent
        case "windows-boot":
        case "ntfs":
        case "microsoft-reserved":
        case "windows-recovery":
        case "bitlocker":
            return Kit.Theme.warning
        case "linux":
            return Kit.Theme.positive
        case "free":
            return Kit.Theme.surfaceSunken
        default:
            return Kit.Theme.lineControl
        }
    }

    implicitHeight: 14
    implicitWidth: 320
    Accessible.ignored: true

    Rectangle {
        anchors.fill: parent
        radius: 4
        color: Kit.Theme.surfaceSunken
    }

    Row {
        anchors.fill: parent
        anchors.margins: 2
        spacing: 1

        Repeater {
            model: bar.blocks

            Rectangle {
                id: block

                required property var modelData

                // A partition too small to see still gets a sliver.
                width: Math.max(2, (bar.width - 4 - (bar.blocks.length - 1)) * block.modelData.share)
                height: bar.height - 4
                radius: 2
                color: bar.fill(block.modelData.kind)
            }
        }
    }
}

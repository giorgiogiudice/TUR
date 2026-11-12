// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Gallery
import Tur.Kit.Testing as Testing

// Text can be enlarged to 200 % without loss of content
// (desktop-experience.md, 13.1, X8), and a translation 40 % longer in a
// right-to-left script still fits (13.2, L5 and L6): layouts wrap, they do
// not clip.
Item {
    id: root

    width: 800
    height: 6000

    PageHost {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "TextSize"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.unload()
            Kit.Theme.textScale = 1
            Testing.Harness.pseudo = Testing.Harness.None
            Testing.Harness.rightToLeft = false
            wait(0)
        }

        function test_no_clipped_text_data() {
            const rows = []
            for (const name of host.pageNames) {
                rows.push({ tag: host.slug(name) + "-200-percent", page: name, scale: 2, rtl: false })
                rows.push({ tag: host.slug(name) + "-rtl-long", page: name, scale: 1, rtl: true })
                rows.push({ tag: host.slug(name) + "-200-percent-rtl-long", page: name, scale: 2, rtl: true })
            }
            return rows
        }

        function test_no_clipped_text(data) {
            Kit.Theme.textScale = data.scale
            Testing.Harness.rightToLeft = data.rtl
            Testing.Harness.pseudo = data.rtl ? Testing.Harness.RtlLong : Testing.Harness.None
            const page = host.load(data.page)
            wait(50)
            grabImage(page)
            wait(0)

            let clipped = Testing.Harness.clippedText(page)
            if (page.keyboardScope !== page)
                clipped = clipped.concat(Testing.Harness.clippedText(page.keyboardScope))
            compare(clipped.join("\n"), "", "no text is elided or cut off")
        }
    }
}

// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The one QML test of the toolchain proof (task D-01): the page loads, a
// stock control takes a click, and grabImage returns the page's real pixels
// under the offscreen platform and the software renderer. With
// TUR_SCREENSHOT_DIR set it also saves the picture that the screenshot job
// compares with desktop/tests/screenshots/probe-1x.png.
Item {
    id: root

    width: 640
    height: 360

    Loader {
        id: loader

        source: Qt.resolvedUrl("Probe.qml")
    }

    TestCase {
        name: "ToolchainProof"
        when: windowShown

        function test_1_page_loads() {
            compare(loader.status, Loader.Ready)
            compare(loader.item.width, 640)
            compare(loader.item.height, 360)
        }

        function test_2_stock_control_works() {
            const button = findChild(loader.item, "stockButton")
            verify(button !== null)
            compare(loader.item.presses, 0)
            mouseClick(button)
            compare(loader.item.presses, 1)
        }

        function test_3_grab_image_sees_the_page() {
            const ratio = Testing.Harness.devicePixelRatio
            const image = grabImage(loader.item)
            compare(image.width, Math.round(640 * ratio))
            compare(image.height, Math.round(360 * ratio))

            // The middle of the accent swatch is the accent; a corner of the
            // page is the window background.
            const swatch = findChild(loader.item, "accentSwatch")
            const middle = swatch.mapToItem(loader.item, swatch.width / 2, swatch.height / 2)
            verify(Qt.colorEqual(image.pixel(Math.round(middle.x * ratio), Math.round(middle.y * ratio)),
                                 Kit.Theme.accent), "the swatch is drawn in the accent colour")
            verify(Qt.colorEqual(image.pixel(2, 2), Kit.Theme.surfaceBase), "the page is drawn on surface.base")

            if (Testing.Harness.outputDir.length > 0)
                image.save(Testing.Harness.outputDir + "/probe-" + Testing.Harness.scaleLabel + ".png")
        }
    }
}

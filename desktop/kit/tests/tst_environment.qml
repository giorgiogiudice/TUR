// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit.Testing as Testing

// What the tests run on. A picture or a measurement means nothing unless the
// platform, the renderer, the style and the fonts are the intended ones, so
// this fails loudly when they are not, and prints them either way.
Item {
    id: root

    width: 320
    height: 200

    TestCase {
        name: "Environment"
        when: windowShown

        function test_environment() {
            const env = Testing.Harness.environment()
            console.info("TUR-ENV " + JSON.stringify(env))
            compare(env.platform, "offscreen", "the tests run on the offscreen platform")
            compare(env.sceneGraphBackend, "software", "Qt Quick's software renderer draws the pictures")
            compare(env.style, "Basic", "the kit's tests run under the Basic style")
            compare(env.interfaceFont, "Inter", "the interface font is installed in the builder image")
            compare(env.monoFont, "JetBrains Mono", "the fixed-width font is installed in the builder image")
            compare(env.fontPointSize, 10)
            fuzzyCompare(env.logicalDpi, 96, 0.5, "QT_FONT_DPI=96, as on a real session")
        }
    }
}

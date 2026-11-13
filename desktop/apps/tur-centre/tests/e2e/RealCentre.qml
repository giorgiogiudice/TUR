// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Centre
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The real application centre for the end-to-end test (desktop/ci/
// centre-e2e/session.sh): its Shell, with a Catalogue that runs the real
// tur-catalogue and an Apps that runs the real tur-apps, as the program's
// window has them (qml/Main.qml). No fixture anywhere. Driven through the
// centre's own controls by the e2e_*.qml beside this file.
//
// `centreEndToEnd` is set by tst_main.cpp from the environment the job
// gives: the programs and the catalogue.
Item {
    id: centre

    readonly property string app: "net.sourceforge.chromium-bsu"

    property alias shell: realShell
    property alias catalogue: realCatalogue
    property alias apps: realApps

    Catalogue {
        id: realCatalogue

        program: centreEndToEnd.catalogueProgram
        catalogueDirectory: centreEndToEnd.catalogue
        timeout: 60000
    }

    Apps {
        id: realApps

        program: centreEndToEnd.apps
        catalogueDirectory: centreEndToEnd.catalogue
    }

    Shell {
        id: realShell

        anchors.fill: parent
        catalogue: realCatalogue
        apps: realApps
        debounce: 0
    }

    // The test's tools: pictures of the whole window (sheets included) and
    // finding the centre's own controls by their names.
    property TestCase test: null

    function view() {
        return test.findChild(realShell, "shellView").item
    }

    function control(parent, name) {
        const item = test.findChild(parent, name)
        test.verify(item !== null, "the centre has a control named " + name)
        return item
    }

    // Presses a control as a person would: it must be shown and available.
    function press(item, what) {
        test.verify(item.visible, what + " is shown")
        test.verify(item.enabled, what + " can be pressed")
        test.mouseClick(item)
    }

    function shot(name) {
        test.wait(300)
        const file = Testing.Harness.outputDir + "/e2e-" + name + ".png"
        const picture = Testing.Harness.grab(centre.Window.contentItem, file)
        test.verify(picture.saved, "the picture was saved to " + file)
        console.info("E2E shot " + name)
    }

    function step(text) {
        console.info("E2E " + text)
    }
}

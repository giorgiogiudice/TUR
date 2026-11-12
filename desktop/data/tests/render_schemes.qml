// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Kit.Testing as Testing

// A picture of each of TUR's KDE colour schemes (task D-04), for the
// screenshot job to compare with the references in desktop/tests/screenshots.
//
// The colours come from desktop/data/schemes.json, which gen_theme.py writes
// from the same tables as TurDark.colors and TurLight.colors; this file names
// no colour. One column per colour set of KDE's format: its background, every
// text role written in its own colour with its contrast ratio, the alternate
// background, and the focus and hover decorations as frames. What KDE's
// programs make of the scheme is in the boot test's desktop pictures.
//
// Its name does not begin with tst_, so the ordinary test run does not pick
// it up; desktop/ci/screenshots.sh passes it with -input.
Item {
    id: root

    width: 1280
    height: 520

    readonly property var data: Testing.Harness.readJson(
        decodeURIComponent(Qt.resolvedUrl("../schemes.json").toString().replace(/^file:\/\//, "")))
    property string variant: "dark"
    readonly property var scheme: root.data ? root.data.schemes[root.variant] : null

    function colour(group, key) {
        return root.scheme.groups[group][key].colour
    }

    function label(group) {
        return group.replace("Colors:", "").replace("][Inactive", ", inactive")
    }

    Rectangle {
        id: specimen

        width: root.width
        height: column.height + 32
        color: root.scheme ? root.colour("Colors:Window", "BackgroundNormal") : "transparent"

        Column {
            id: column

            x: 16
            y: 16
            spacing: 12

            Text {
                text: root.scheme ? root.scheme.name + " · " + root.scheme.file + " · generated from TUR's tokens" : ""
                color: root.scheme ? root.colour("Colors:Window", "ForegroundNormal") : "transparent"
                font.family: "Inter"
                font.pointSize: 12
                font.weight: Font.DemiBold
            }

            Row {
                spacing: 8

                Repeater {
                    model: root.scheme ? root.data.sets : []

                    delegate: Rectangle {
                        id: set

                        required property string modelData

                        width: 148
                        height: setColumn.height + 16
                        color: root.colour(set.modelData, "BackgroundNormal")
                        radius: 6

                        Column {
                            id: setColumn

                            x: 8
                            y: 8
                            width: parent.width - 16
                            spacing: 4

                            Text {
                                text: root.label(set.modelData)
                                color: root.colour(set.modelData, "ForegroundNormal")
                                font.family: "Inter"
                                font.pointSize: 10
                                font.weight: Font.DemiBold
                            }

                            Repeater {
                                model: root.data.foregrounds

                                delegate: Text {
                                    required property string modelData

                                    text: modelData.replace("Foreground", "") + "  "
                                          + Testing.Harness.contrast(root.colour(set.modelData, modelData),
                                                                     root.colour(set.modelData, "BackgroundNormal")).toFixed(1)
                                    color: root.colour(set.modelData, modelData)
                                    font.family: "Inter"
                                    font.pointSize: 9
                                }
                            }

                            Rectangle {
                                width: parent.width
                                height: 24
                                color: root.colour(set.modelData, "BackgroundAlternate")

                                Text {
                                    anchors.verticalCenter: parent.verticalCenter
                                    x: 4
                                    text: "Alternate  " + Testing.Harness.contrast(
                                              root.colour(set.modelData, "ForegroundNormal"),
                                              root.colour(set.modelData, "BackgroundAlternate")).toFixed(1)
                                    color: root.colour(set.modelData, "ForegroundNormal")
                                    font.family: "Inter"
                                    font.pointSize: 9
                                }
                            }

                            Row {
                                spacing: 6

                                Repeater {
                                    model: root.data.decorations

                                    delegate: Rectangle {
                                        required property string modelData

                                        width: 62
                                        height: 22
                                        radius: 4
                                        color: root.colour(set.modelData, "BackgroundNormal")
                                        border.width: 2
                                        border.color: root.colour(set.modelData, modelData)

                                        Text {
                                            anchors.centerIn: parent
                                            text: modelData.replace("Decoration", "")
                                            color: root.colour(set.modelData, "ForegroundNormal")
                                            font.family: "Inter"
                                            font.pointSize: 8
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Row {
                spacing: 8

                Rectangle {
                    width: 300
                    height: 28
                    radius: 6
                    color: root.scheme ? root.colour("WM", "activeBackground") : "transparent"

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        x: 8
                        text: "Window title, active"
                        color: root.scheme ? root.colour("WM", "activeForeground") : "transparent"
                        font.family: "Inter"
                        font.pointSize: 10
                    }
                }

                Rectangle {
                    width: 300
                    height: 28
                    radius: 6
                    color: root.scheme ? root.colour("WM", "inactiveBackground") : "transparent"
                    border.width: 1
                    border.color: root.scheme ? root.colour("Colors:Window", "BackgroundAlternate") : "transparent"

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        x: 8
                        text: "Window title, inactive"
                        color: root.scheme ? root.colour("WM", "inactiveForeground") : "transparent"
                        font.family: "Inter"
                        font.pointSize: 10
                    }
                }
            }
        }
    }

    TestCase {
        name: "ColourSchemeScreenshots"
        when: windowShown

        function initTestCase() {
            verify(Testing.Harness.outputDir.length > 0, "TUR_SCREENSHOT_DIR says where the pictures go")
            verify(root.data !== undefined && root.data !== null, "desktop/data/schemes.json was read")
            console.info("TUR-ENV " + JSON.stringify(Testing.Harness.environment()))
        }

        function test_render_data() {
            return [
                { tag: "colour-scheme-dark-ltr-1x", variant: "dark" },
                { tag: "colour-scheme-light-ltr-1x", variant: "light" }
            ]
        }

        function test_render(data) {
            root.variant = data.variant
            wait(50)
            Testing.Harness.grab(specimen, "")
            wait(50)
            verify(specimen.height <= root.height, "the specimen (" + specimen.height + " px) fits in the window")

            // What the picture shows must read: every piece of text at 4.5:1 or
            // more against what is really behind it.
            const failures = []
            for (const finding of Testing.Harness.contrastFindings(specimen, [])) {
                if (finding.kind !== "text" || finding.exempt || finding.problem.length > 0)
                    continue
                if (finding.ratio < 4.5)
                    failures.push(finding.ratio.toFixed(2) + ":1 " + finding.foreground + " on "
                                  + finding.background + " — " + finding.where)
            }
            compare(failures.join("\n"), "", "text in the " + data.variant + " scheme reads at 4.5:1")

            const file = Testing.Harness.outputDir + "/" + data.tag + ".png"
            const shot = Testing.Harness.grab(specimen, file)
            verify(shot.saved, "the picture was saved to " + file)
        }
    }
}

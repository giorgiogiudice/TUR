// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// "How it will run" and "Other ways to run it" (desktop-experience.md,
// section 8.4).
//
// The box is the centre of the page: the option the router marks
// `recommended`. Under it, every other option the router returned, in the
// router's order, each with its own rating, outlook, caveats and
// prerequisites. A compatibility layer is listed before Windows itself even
// when the layer is rated as not working and Windows is recommended: that is
// the router's order, kept.
//
// Choosing another way ("Change…", "Always run it this way") is not built:
// nothing yet owns the preferences the router reads.
ColumnLayout {
    id: routes

    required property ApplicationSession application

    spacing: Kit.Theme.spaceGroup

    Kit.Card {
        objectName: "howItWillRun"
        Layout.fillWidth: true

        ColumnLayout {
            anchors.fill: parent
            spacing: Kit.Theme.spaceRelated

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("How it will run")
                kind: Kit.Label.Small
                tone: Kit.Label.Secondary
            }

            WayBlock {
                objectName: "recommendedWay"
                Layout.fillWidth: true
                option: routes.application.recommended
                entry: routes.application.entry
                reports: routes.application.reports(routes.application.recommended)
                leading: true
            }
        }
    }

    ColumnLayout {
        objectName: "otherWays"
        Layout.fillWidth: true
        visible: routes.application.others.length > 0
        spacing: Kit.Theme.spaceRelated

        Kit.Label {
            Layout.fillWidth: true
            text: qsTr("Other ways to run it")
            kind: Kit.Label.Heading
        }

        Kit.Card {
            Layout.fillWidth: true

            ColumnLayout {
                anchors.fill: parent
                spacing: Kit.Theme.spaceGroup

                Repeater {
                    model: routes.application.others

                    ColumnLayout {
                        id: other

                        required property var modelData
                        required property int index

                        Layout.fillWidth: true
                        spacing: Kit.Theme.spaceGroup

                        Kit.Separator {
                            Layout.fillWidth: true
                            visible: other.index > 0
                        }

                        WayBlock {
                            objectName: "otherWay" + other.index
                            Layout.fillWidth: true
                            option: other.modelData
                            entry: routes.application.entry
                            reports: routes.application.reports(other.modelData)
                        }
                    }
                }
            }
        }
    }
}

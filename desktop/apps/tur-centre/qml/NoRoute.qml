// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// When there is no good route (desktop-experience.md, section 8.8).
//
// The router never returns an empty answer. When nothing can work it returns
// "no route" first, with a reason that repeats each blocker and the
// least-bad alternative, followed by the blocked routes. This shows exactly
// that, in a fixed pattern: what TUR cannot do; why, in a sentence a
// non-specialist can repeat; what the person can do instead; the evidence;
// and the code.
//
// There is no primary button, and nothing here suggests the application
// might run after all.
Kit.Card {
    id: page

    required property ApplicationSession application

    // Opens a page in the browser.
    signal urlRequested(string url)

    // The router's "no route" statement, and the catalogue's entry.
    readonly property var statement: page.application.recommended
    readonly property var entry: page.application.entry

    readonly property var why: Words.noRouteWhy(page.statement, page.entry)
    readonly property var alternatives: Words.noRouteAlternatives(page.statement, page.entry)
    readonly property bool offersWebsite: Words.noRouteCase(page.statement) === "play-integrity"
                                          && !!page.entry.homepage

    // The router's own words, for the technical layer. The labels are the
    // ones its text report uses.
    readonly property string routerWords: {
        const lines = ["Why: " + (page.statement.reason ?? "")]
        if (page.statement.alternative && page.statement.alternative.text)
            lines.push("Least-bad alternative: " + page.statement.alternative.text)
        return lines.join("\n")
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Kit.Theme.spaceGroup

        // What TUR cannot do.
        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceRelated + Kit.Theme.spaceGrid

            Kit.Glyph {
                Layout.alignment: Qt.AlignTop
                kind: Kit.Glyph.Negative
                tint: Kit.Theme.negative
                size: 20
            }

            Kit.Label {
                objectName: "noRouteTitle"
                Layout.fillWidth: true
                text: Words.routeLabel("no-route", "")
                kind: Kit.Label.Heading
            }
        }

        // Why.
        Kit.Label {
            objectName: "noRouteWhy"
            Layout.fillWidth: true
            text: page.why.join(" ")
            paragraph: true
        }

        // What the person can do instead.
        ColumnLayout {
            Layout.fillWidth: true
            visible: page.alternatives.length > 0
            spacing: Kit.Theme.spaceGrid

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("What you can do")
                kind: Kit.Label.Strong
            }

            Repeater {
                model: page.alternatives

                RowLayout {
                    id: alternative

                    required property string modelData

                    Layout.fillWidth: true
                    spacing: Kit.Theme.spaceRelated

                    Kit.Label {
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: sentence.halfLeading
                        text: "•"
                        tone: Kit.Label.Secondary
                        Accessible.ignored: true
                    }

                    Kit.Label {
                        id: sentence

                        objectName: "noRouteAlternative"
                        Layout.fillWidth: true
                        text: alternative.modelData
                        paragraph: true
                    }
                }
            }

            Kit.Button {
                objectName: "noRouteWebsite"
                Layout.topMargin: Kit.Theme.spaceGrid
                visible: page.offersWebsite
                text: qsTr("Open the website")
                onClicked: page.urlRequested(page.entry.homepage)
            }
        }

        // The evidence: the routes the router listed, each blocked, each
        // with its rating, its source and the caveat that says why.
        ColumnLayout {
            objectName: "noRouteConsidered"
            Layout.fillWidth: true
            visible: page.application.others.length > 0
            spacing: Kit.Theme.spaceGroup

            Kit.Label {
                Layout.fillWidth: true
                text: qsTr("Ways that were considered")
                kind: Kit.Label.Strong
            }

            Repeater {
                model: page.application.others

                WayBlock {
                    required property var modelData
                    required property int index

                    objectName: "consideredWay" + index
                    Layout.fillWidth: true
                    option: modelData
                    entry: page.entry
                    reports: page.application.reports(modelData)
                }
            }
        }

        Kit.Separator {
            Layout.fillWidth: true
        }

        // Where the facts were checked, the code, and the router's own
        // words. A flow, so that the row wraps when the text is large.
        Flow {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGroup

            Kit.Disclosure {
                id: checked

                objectName: "noRouteChecked"
                text: qsTr("Where each fact was checked")
            }

            Kit.CodeLabel {
                objectName: "noRouteCode"
                height: checked.height
                verticalAlignment: TextInput.AlignVCenter
                code: Words.noRouteCode
            }

            Kit.Disclosure {
                id: detail

                objectName: "noRouteDetail"
                text: qsTr("Detail")
            }
        }

        Facts {
            Layout.fillWidth: true
            visible: checked.expanded
            entry: page.entry
            referencesOnly: true
            onUrlRequested: url => page.urlRequested(url)
        }

        Rectangle {
            objectName: "noRouteRouterWords"
            Layout.fillWidth: true
            visible: detail.expanded
            implicitHeight: routerText.implicitHeight + 2 * Kit.Theme.spaceRelated
            radius: Kit.Theme.radiusControl
            color: Kit.Theme.surfaceSunken

            Kit.Label {
                id: routerText

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Kit.Theme.spaceRelated
                text: page.routerWords
                kind: Kit.Label.Mono
            }
        }
    }
}
